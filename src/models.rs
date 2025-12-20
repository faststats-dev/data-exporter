use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::{PgPool, types::Uuid};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ExportRequest {
    pub id: Uuid,
    pub token: String,
    pub project_id: Uuid,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub token: String,
    pub slug: String,
    pub private: bool,
    pub template_id: Option<String>,
    pub created_at: NaiveDateTime,
    pub owner_id: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DataEntry {
    pub data: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Serialize)]
pub struct ExportData {
    pub project: Project,
    pub data_entries: Vec<DataEntry>,
}
