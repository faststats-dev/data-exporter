use crate::models::{AppState, DataEntry, ExportRequest, Project};
use axum::{
    extract::{Path, State},
    http::{header::LOCATION, StatusCode},
    response::{IntoResponse, Response},
};
use futures::stream::StreamExt;
use tokio::io::AsyncWriteExt;

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

    let file_exists = state.s3_bucket.object_exists(&s3_key).await.map_err(|e| {
        println!("Error checking S3 file existence: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if !file_exists {
        stream_export_to_s3(&state, export_request.project_id, &s3_key).await?;
    }

    let presigned_url = state
        .s3_bucket
        .presign_get(&s3_key, 300, None)
        .await
        .map_err(|e| {
            println!("Error generating presigned URL: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok((StatusCode::FOUND, [(LOCATION, presigned_url.as_str())]).into_response())
}

fn indent_json(json: &[u8], spaces: usize) -> Vec<u8> {
    let mut result = Vec::with_capacity(json.len() + (json.len() >> 3));

    for &byte in json {
        result.push(byte);
        if byte == b'\n' {
            result.extend(std::iter::repeat_n(b' ', spaces));
        }
    }

    while result.last() == Some(&b' ') {
        result.pop();
    }

    result
}

async fn stream_export_to_s3(
    state: &AppState,
    project_id: sqlx::types::Uuid,
    s3_key: &str,
) -> Result<(), StatusCode> {
    let (writer, mut reader) = tokio::io::duplex(128 * 1024);

    let pool = state.pool.clone();
    let s3_bucket = state.s3_bucket.clone();
    let s3_key_owned = s3_key.to_string();

    let writer_handle = tokio::spawn(async move {
        let project = sqlx::query_as::<_, Project>(
            "SELECT id, name, token, slug, private, template_id, created_at, owner_id FROM project WHERE id = $1"
        )
        .bind(project_id)
        .fetch_optional(&pool)
        .await
        .map_err(std::io::Error::other)?
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Project not found"))?;

        write_export_json(writer, project, &pool, project_id).await
    });

    let upload_handle = tokio::spawn(async move {
        s3_bucket
            .put_object_stream(&mut reader, &s3_key_owned)
            .await
            .map_err(std::io::Error::other)
    });

    writer_handle
        .await
        .map_err(|e| {
            println!("Writer task failed: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .map_err(|e| {
            println!("Error writing JSON: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    upload_handle
        .await
        .map_err(|e| {
            println!("Upload task failed: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .map_err(|e| {
            println!("Error uploading to S3: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(())
}

async fn write_export_json<W>(
    mut writer: W,
    project: Project,
    pool: &sqlx::PgPool,
    project_id: sqlx::types::Uuid,
) -> Result<(), std::io::Error>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buffer = Vec::with_capacity(131072);

    writer.write_all(b"{\n  \"project\": ").await?;

    serde_json::to_writer_pretty(&mut buffer, &project).map_err(std::io::Error::other)?;

    let indented = indent_json(&buffer, 2);
    writer.write_all(&indented).await?;

    writer.write_all(b",\n  \"data_entries\": [\n").await?;

    let mut entries_stream = sqlx::query_as::<_, DataEntry>(
        "SELECT data, created_at FROM data_entries WHERE project_id = $1 ORDER BY created_at DESC",
    )
    .bind(project_id)
    .fetch(pool);

    let mut first = true;
    while let Some(entry_result) = entries_stream.next().await {
        let entry = entry_result.map_err(std::io::Error::other)?;

        if !first {
            writer.write_all(b",\n").await?;
        }
        first = false;

        writer.write_all(b"    ").await?;

        buffer.clear();
        serde_json::to_writer_pretty(&mut buffer, &entry).map_err(std::io::Error::other)?;

        let indented = indent_json(&buffer, 4);
        writer.write_all(&indented).await?;
    }

    writer.write_all(b"\n  ]\n}\n").await?;
    writer.shutdown().await
}
