use chrono::Utc;
use rusqlite::params;
use tauri::State;
use uuid::Uuid;

use crate::models::{DbConnection, DbType, QueryResult};
use crate::state::AppState;

#[tauri::command]
pub fn list_db_connections(state: State<'_, AppState>) -> Result<Vec<DbConnection>, String> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.conn.prepare(
        "SELECT id, name, db_type, connection_string, is_active FROM db_connections ORDER BY created_at DESC"
    ).map_err(|e| e.to_string())?;

    let connections = stmt.query_map([], |row| {
        let db_type_str: String = row.get(2)?;
        let db_type = match db_type_str.as_str() {
            "mongodb" => DbType::MongoDB,
            "mysql" => DbType::MySQL,
            _ => DbType::PostgreSQL,
        };
        Ok(DbConnection {
            id: row.get(0)?,
            name: row.get(1)?,
            db_type,
            connection_string: row.get(3)?,
            is_active: row.get::<_, i32>(4)? == 1,
            created_at: Utc::now(),
        })
    })
    .map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    Ok(connections)
}

#[tauri::command]
pub async fn add_db_connection(
    name: String,
    db_type: String,
    connection_string: String,
    state: State<'_, AppState>,
) -> Result<DbConnection, String> {
    test_connection_internal(&connection_string, &db_type).await?;

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let db = state.db.lock().unwrap();
    db.conn.execute(
        "INSERT INTO db_connections (id, name, db_type, connection_string, is_active, created_at)
         VALUES (?1, ?2, ?3, ?4, 1, ?5)",
        params![id, name, db_type.to_lowercase(), connection_string, now],
    ).map_err(|e| e.to_string())?;

    let db_type_enum = match db_type.to_lowercase().as_str() {
        "mongodb" => DbType::MongoDB,
        "mysql" => DbType::MySQL,
        _ => DbType::PostgreSQL,
    };

    Ok(DbConnection { id, name, db_type: db_type_enum, connection_string, is_active: true, created_at: Utc::now() })
}

#[tauri::command]
pub fn delete_db_connection(connection_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.conn.execute("DELETE FROM db_connections WHERE id = ?1", params![connection_id]).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn execute_db_query(
    connection_id: String,
    query: String,
    state: State<'_, AppState>,
) -> Result<QueryResult, String> {
    let (db_type, conn_str) = {
        let db = state.db.lock().unwrap();
        db.conn.query_row(
            "SELECT db_type, connection_string FROM db_connections WHERE id = ?1 AND is_active = 1",
            params![connection_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).map_err(|e| format!("Connection not found: {}", e))?
    };

    match db_type.as_str() {
        "mongodb" => {
            let connector = crate::db::mongodb::MongoConnector::connect(&conn_str)
                .await.map_err(|e| e.to_string())?;
            let parts: Vec<&str> = query.splitn(2, ' ').collect();
            let coll_path: Vec<&str> = parts[0].splitn(2, '.').collect();
            if coll_path.len() != 2 {
                return Err("MongoDB format: <database>.<collection> {filter}".to_string());
            }
            let filter = if parts.len() > 1 { parts[1] } else { "{}" };
            connector.run_query(coll_path[0], coll_path[1], filter, 100).await.map_err(|e| e.to_string())
        }
        other => {
            crate::db::sql::execute_sql_query(&conn_str, &query, other).await.map_err(|e| e.to_string())
        }
    }
}

#[tauri::command]
pub async fn test_db_connection(
    connection_string: String,
    db_type: String,
) -> Result<bool, String> {
    test_connection_internal(&connection_string, &db_type).await
}

async fn test_connection_internal(connection_string: &str, db_type: &str) -> Result<bool, String> {
    match db_type.to_lowercase().as_str() {
        "mongodb" => {
            crate::db::mongodb::MongoConnector::connect(connection_string)
                .await.map(|_| true).map_err(|e| e.to_string())
        }
        _ => {
            crate::db::sql::test_connection(connection_string, db_type)
                .await.map_err(|e| e.to_string())
        }
    }
}
