use anyhow::{Context, Result};
use serde_json::Value;
use std::sync::{Arc, Mutex};

use crate::db::AppDb;

pub struct DbQueryTool {
    db: Arc<Mutex<AppDb>>,
}

impl DbQueryTool {
    pub fn new(db: Arc<Mutex<AppDb>>) -> Self {
        Self { db }
    }

    pub async fn execute(
        &self,
        connection_id: &str,
        query: &str,
    ) -> Result<Value> {
        // Look up connection (sync, drop guard before await)
        let (db_type, conn_str) = {
            let db = self.db.lock().unwrap();
            let mut stmt = db.conn.prepare(
                "SELECT db_type, connection_string FROM db_connections WHERE id = ?1 AND is_active = 1"
            )?;
            stmt.query_row(rusqlite::params![connection_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .context("Connection not found or inactive")?
        }; // guard dropped here

        let result = crate::db::sql::execute_sql_query(&conn_str, query, &db_type).await?;

        Ok(serde_json::json!({
            "connection_id": connection_id,
            "query": query,
            "row_count": result.row_count,
            "columns": result.columns,
            "rows": result.rows,
            "duration_ms": result.duration_ms
        }))
    }
}
