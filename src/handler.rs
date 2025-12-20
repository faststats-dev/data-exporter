use crate::models::{AppState, DataEntry, ExportData, ExportRequest, Project};

use axum::{
    extract::{Path, State},
    http::{
        StatusCode,
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
};

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
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Err(StatusCode::NOT_FOUND);
    }

    let project = sqlx::query_as::<_, Project>(
        "SELECT id, name, token, slug, private, template_id, created_at, owner_id FROM project WHERE id = $1"
    )
    .bind(export_request.project_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let data_entries = sqlx::query_as::<_, DataEntry>(
        "SELECT project_id, data, created_at FROM data_entries WHERE project_id = $1 ORDER BY created_at DESC"
    )
    .bind(export_request.project_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let export_data = ExportData {
        project,
        data_entries,
    };

    let json_string = serde_json::to_string_pretty(&export_data)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let filename = format!("project-{}-export.json", export_request.project_id);

    Ok((
        StatusCode::OK,
        [
            (CONTENT_TYPE, "application/json"),
            (
                CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        json_string,
    )
        .into_response())
}
