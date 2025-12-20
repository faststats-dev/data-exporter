use crate::models::{AppState, DataEntry, ExportRequest, Project};
use bytes::Bytes;

use axum::{
    body::Body,
    extract::{Path, State},
    http::{
        StatusCode,
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
    },
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

    let project = sqlx::query_as::<_, Project>(
        "SELECT id, name, token, slug, private, template_id, created_at, owner_id FROM project WHERE id = $1"
    )
    .bind(export_request.project_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        println!("Error while fetching project: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    let project_json = serde_json::to_string(&project).map_err(|e| {
        println!("Error while serializing project: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let filename = format!("project-{}-export.json", export_request.project_id);
    let project_id = export_request.project_id;

    let stream = async_stream::stream! {
        yield Ok::<_, std::io::Error>(Bytes::from(format!("{{\"project\":{},\"data_entries\":[", project_json)));

        let mut row_stream = sqlx::query("SELECT data, created_at FROM data_entries WHERE project_id = $1 ORDER BY created_at DESC")
            .bind(project_id)
            .fetch(&state.pool);

        let mut first = true;
        while let Some(row) = row_stream.next().await {
            match row {
                Ok(row) => {
                    let data_entry = DataEntry {
                        data: row.try_get("data").ok(),
                        created_at: row.get("created_at"),
                    };

                    if let Ok(entry_json) = serde_json::to_string(&data_entry) {
                        if !first {
                            yield Ok(Bytes::from(","));
                        }
                        first = false;
                        yield Ok(Bytes::from(entry_json));
                    }
                }
                Err(e) => {
                    println!("Error while streaming data entry: {:?}", e);
                    break;
                }
            }
        }

        yield Ok(Bytes::from("]}"));
    };

    let body = Body::from_stream(stream);

    Ok((
        StatusCode::OK,
        [
            (CONTENT_TYPE, "application/json"),
            (
                CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        body,
    )
        .into_response())
}
