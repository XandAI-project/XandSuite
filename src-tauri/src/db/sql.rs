use anyhow::{Context, Result};
use serde_json::Value;
use std::time::Instant;

use crate::models::QueryResult;

pub async fn execute_sql_query(
    connection_string: &str,
    query: &str,
    db_type: &str,
) -> Result<QueryResult> {
    let start = Instant::now();

    match db_type.to_lowercase().as_str() {
        "postgresql" | "postgres" => {
            use sqlx::{postgres::PgPoolOptions, Column, Row};

            let pool = PgPoolOptions::new()
                .max_connections(1)
                .connect(connection_string)
                .await
                .context("Failed to connect to PostgreSQL")?;

            let rows = sqlx::query(query)
                .fetch_all(&pool)
                .await
                .context("Failed to execute PostgreSQL query")?;

            let duration_ms = start.elapsed().as_millis() as u64;

            let columns: Vec<String> = if let Some(row) = rows.first() {
                row.columns().iter().map(|c| c.name().to_string()).collect()
            } else {
                vec![]
            };

            let json_rows: Vec<Value> = rows
                .iter()
                .map(|row| {
                    let mut map = serde_json::Map::new();
                    for col in row.columns() {
                        let val: Value = row
                            .try_get::<Option<String>, _>(col.name())
                            .ok()
                            .flatten()
                            .map(Value::String)
                            .unwrap_or(Value::Null);
                        map.insert(col.name().to_string(), val);
                    }
                    Value::Object(map)
                })
                .collect();

            Ok(QueryResult {
                columns,
                rows: json_rows,
                row_count: rows.len(),
                duration_ms,
            })
        }
        "mysql" => {
            use sqlx::{mysql::MySqlPoolOptions, Column, Row};

            let pool = MySqlPoolOptions::new()
                .max_connections(1)
                .connect(connection_string)
                .await
                .context("Failed to connect to MySQL")?;

            let rows = sqlx::query(query)
                .fetch_all(&pool)
                .await
                .context("Failed to execute MySQL query")?;

            let duration_ms = start.elapsed().as_millis() as u64;

            let columns: Vec<String> = if let Some(row) = rows.first() {
                row.columns().iter().map(|c| c.name().to_string()).collect()
            } else {
                vec![]
            };

            let json_rows: Vec<Value> = rows
                .iter()
                .map(|row| {
                    let mut map = serde_json::Map::new();
                    for col in row.columns() {
                        let val: Value = row
                            .try_get::<Option<String>, _>(col.name())
                            .ok()
                            .flatten()
                            .map(Value::String)
                            .unwrap_or(Value::Null);
                        map.insert(col.name().to_string(), val);
                    }
                    Value::Object(map)
                })
                .collect();

            Ok(QueryResult {
                columns,
                rows: json_rows,
                row_count: rows.len(),
                duration_ms,
            })
        }
        _ => anyhow::bail!("Unsupported database type: {}", db_type),
    }
}

pub async fn test_connection(connection_string: &str, db_type: &str) -> Result<bool> {
    match db_type.to_lowercase().as_str() {
        "postgresql" | "postgres" => {
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(connection_string)
                .await
                .context("Failed to connect to PostgreSQL")?;
            Ok(true)
        }
        "mysql" => {
            sqlx::mysql::MySqlPoolOptions::new()
                .max_connections(1)
                .connect(connection_string)
                .await
                .context("Failed to connect to MySQL")?;
            Ok(true)
        }
        _ => anyhow::bail!("Unsupported database type: {}", db_type),
    }
}
