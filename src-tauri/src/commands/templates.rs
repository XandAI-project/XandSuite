use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::state::AppState;

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub id: String,
    pub title: String,
    pub content: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub shortcut: Option<String>,
    /// Optional package name that must be installed to use this template.
    pub requires: Option<String>,
    pub use_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateTemplateInput {
    pub title: String,
    pub content: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub shortcut: Option<String>,
    pub requires: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTemplateInput {
    pub id: String,
    pub title: Option<String>,
    pub content: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub shortcut: Option<String>,
    pub requires: Option<String>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

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

// ── Commands ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_templates(state: State<'_, AppState>) -> Result<Vec<PromptTemplate>, String> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.conn
        .prepare(
            "SELECT id, title, content, description, category, shortcut, requires,
                    use_count, created_at, updated_at
             FROM prompt_templates
             ORDER BY use_count DESC, title ASC",
        )
        .map_err(|e| e.to_string())?;

    let templates = stmt
        .query_map([], row_to_template)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(templates)
}

#[tauri::command]
pub fn create_template(
    input: CreateTemplateInput,
    state: State<'_, AppState>,
) -> Result<PromptTemplate, String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let db = state.db.lock().unwrap();
    db.conn.execute(
        "INSERT INTO prompt_templates
         (id, title, content, description, category, shortcut, requires, use_count, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?8)",
        params![
            id,
            input.title,
            input.content,
            input.description,
            input.category,
            input.shortcut,
            input.requires,
            now,
        ],
    ).map_err(|e| e.to_string())?;

    Ok(PromptTemplate {
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
    })
}

#[tauri::command]
pub fn update_template(
    input: UpdateTemplateInput,
    state: State<'_, AppState>,
) -> Result<PromptTemplate, String> {
    let now = Utc::now().to_rfc3339();
    let db = state.db.lock().unwrap();

    // Build a single UPDATE statement covering all provided fields
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
    // These fields are always written (they arrive from the full-form save):
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

    // id param for WHERE
    let id_param_idx = values.len() + 1;
    values.push(Box::new(input.id.clone()));

    let sql = format!(
        "UPDATE prompt_templates SET {} WHERE id = ?{}",
        set_clauses.join(", "),
        id_param_idx
    );

    let refs: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();
    db.conn.execute(&sql, refs.as_slice()).map_err(|e| e.to_string())?;

    let template = db.conn.query_row(
        "SELECT id, title, content, description, category, shortcut, requires,
                use_count, created_at, updated_at
         FROM prompt_templates WHERE id = ?1",
        params![input.id],
        row_to_template,
    ).map_err(|e| e.to_string())?;

    Ok(template)
}

#[tauri::command]
pub fn delete_template(
    template_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.conn.execute(
        "DELETE FROM prompt_templates WHERE id = ?1",
        params![template_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn increment_template_use(
    template_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.conn.execute(
        "UPDATE prompt_templates SET use_count = use_count + 1 WHERE id = ?1",
        params![template_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}
