use crate::models::{AppState, DataEntry, ExportRequest, Project};
use crate::s3_helpers;
use bytes::Bytes;

use axum::{
    extract::{Path, State},
    http::{StatusCode, header::LOCATION},
    response::{IntoResponse, Response},
};
use futures::stream::StreamExt;
use sqlx::Row;

pub async fn export(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Response, StatusCode> {
    let export_request = sqlx::query_as::<_, ExportRequest>(
        "SELECT id, token, project_id, created_at FROM export_requests WHERE token = $1 LIMIT 1",
    )
    .bind(&token)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        println!("{:?}", e);
        StatusCode::NOT_FOUND
    })?;

    let now = chrono::Utc::now().naive_utc();
    let expiry = now - chrono::Duration::hours(24);

    if export_request.created_at < expiry {
        sqlx::query("DELETE FROM export_requests WHERE token = $1")
            .bind(&token)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                println!("Error while deleting export request: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        return Err(StatusCode::NOT_FOUND);
    }

    let s3_key = format!("exports/{}.json", token);

    let file_exists = s3_helpers::check_file_exists(&state.s3_bucket, &s3_key)
        .await
        .map_err(|e| {
            println!("Error checking S3 file existence: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if !file_exists {
        let export_data = generate_export_data(&state, export_request.project_id).await?;

        s3_helpers::upload_file(&state.s3_bucket, &s3_key, export_data)
            .await
            .map_err(|e| {
                println!("Error uploading to S3: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    }

    let presigned_url = s3_helpers::generate_presigned_url(&state.s3_bucket, &s3_key, 300)
        .await
        .map_err(|e| {
            println!("Error generating presigned URL: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok((StatusCode::FOUND, [(LOCATION, presigned_url.as_str())]).into_response())
}

async fn generate_export_data(
    state: &AppState,
    project_id: sqlx::types::Uuid,
) -> Result<Bytes, StatusCode> {
    let project = sqlx::query_as::<_, Project>(
        "SELECT id, name, token, slug, private, template_id, created_at, owner_id FROM project WHERE id = $1"
    )
    .bind(project_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        println!("Error while fetching project: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    let mut data_entries = Vec::new();
    let mut row_stream = sqlx::query(
        "SELECT data, created_at FROM data_entries WHERE project_id = $1 ORDER BY created_at DESC",
    )
    .bind(project_id)
    .fetch(&state.pool);

    while let Some(row) = row_stream.next().await {
        match row {
            Ok(row) => {
                let data_entry = DataEntry {
                    data: row.try_get("data").ok(),
                    created_at: row.get("created_at"),
                };
                data_entries.push(data_entry);
            }
            Err(e) => {
                println!("Error while streaming data entry: {:?}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    let export_data = serde_json::json!({
        "project": project,
        "data_entries": data_entries
    });

    let json_string = serde_json::to_string_pretty(&export_data).map_err(|e| {
        println!("Error while serializing export data: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Bytes::from(json_string))
}
