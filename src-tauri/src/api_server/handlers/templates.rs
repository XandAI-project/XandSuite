use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use rusqlite::params;
use std::sync::Arc;
use uuid::Uuid;

use crate::commands::templates::{CreateTemplateInput, PromptTemplate, UpdateTemplateInput};
use crate::state::AppState;

fn row_to_template(row: &rusqlite::Row<'_>) -> rusqlite::Result<PromptTemplate> {
    Ok(PromptTemplate {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        description: row.get(3)?,
        category: row.get(4)?,
        shortcut: row.get(5)?,
        requires: row.get(6)?,
        use_count: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

pub async fn list_templates(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<PromptTemplate>>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.conn
        .prepare(
            "SELECT id, title, content, description, category, shortcut, requires,
                    use_count, created_at, updated_at
             FROM prompt_templates ORDER BY use_count DESC, title ASC",
        )
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let templates: Vec<PromptTemplate> = stmt
        .query_map([], row_to_template)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(templates))
}

pub async fn create_template(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateTemplateInput>,
) -> Result<Json<PromptTemplate>, (StatusCode, String)> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let db = state.db.lock().unwrap();
    db.conn.execute(
        "INSERT INTO prompt_templates
         (id, title, content, description, category, shortcut, requires, use_count, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?8)",
        params![id, input.title, input.content, input.description, input.category, input.shortcut, input.requires, now],
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(PromptTemplate {
        id,
        title: input.title,
        content: input.content,
        description: input.description,
        category: input.category,
        shortcut: input.shortcut,
        requires: input.requires,
        use_count: 0,
        created_at: now.clone(),
        updated_at: now,
    }))
}

pub async fn update_template(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(mut input): Json<UpdateTemplateInput>,
) -> Result<Json<PromptTemplate>, (StatusCode, String)> {
    input.id = id;
    let now = Utc::now().to_rfc3339();
    let db = state.db.lock().unwrap();

    let mut set_clauses: Vec<String> = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(ref v) = input.title {
        set_clauses.push(format!("title = ?{}", set_clauses.len() + 1));
        values.push(Box::new(v.clone()));
    }
    if let Some(ref v) = input.content {
        set_clauses.push(format!("content = ?{}", set_clauses.len() + 1));
        values.push(Box::new(v.clone()));
    }
    set_clauses.push(format!("description = ?{}", set_clauses.len() + 1));
    values.push(Box::new(input.description.clone()));
    set_clauses.push(format!("category = ?{}", set_clauses.len() + 1));
    values.push(Box::new(input.category.clone()));
    set_clauses.push(format!("shortcut = ?{}", set_clauses.len() + 1));
    values.push(Box::new(input.shortcut.clone()));
    set_clauses.push(format!("requires = ?{}", set_clauses.len() + 1));
    values.push(Box::new(input.requires.clone()));
    set_clauses.push(format!("updated_at = ?{}", set_clauses.len() + 1));
    values.push(Box::new(now.clone()));

    let id_idx = values.len() + 1;
    values.push(Box::new(input.id.clone()));

    let sql = format!("UPDATE prompt_templates SET {} WHERE id = ?{}", set_clauses.join(", "), id_idx);
    let refs: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();
    db.conn.execute(&sql, refs.as_slice())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let template = db.conn.query_row(
        "SELECT id, title, content, description, category, shortcut, requires,
                use_count, created_at, updated_at
         FROM prompt_templates WHERE id = ?1",
        params![input.id],
        row_to_template,
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(template))
}

pub async fn delete_template(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    db.conn.execute("DELETE FROM prompt_templates WHERE id = ?1", params![id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn increment_template_use(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    db.conn.execute("UPDATE prompt_templates SET use_count = use_count + 1 WHERE id = ?1", params![id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}
