use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use rusqlite::params;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

use crate::models::{Flow, FlowEdge, FlowExecution, FlowNode};
use crate::state::AppState;

pub async fn list_flows(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Flow>>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.conn.prepare(
        "SELECT id, name, description, nodes_json, edges_json FROM flows ORDER BY updated_at DESC"
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let flows: Vec<Flow> = stmt.query_map([], |row| {
        let nodes_json: String = row.get(3)?;
        let edges_json: String = row.get(4)?;
        let nodes: Vec<FlowNode> = serde_json::from_str(&nodes_json).unwrap_or_default();
        let edges: Vec<FlowEdge> = serde_json::from_str(&edges_json).unwrap_or_default();
        Ok(Flow {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            nodes,
            edges,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .filter_map(|r| r.ok())
    .collect();

    Ok(Json(flows))
}

pub async fn save_flow(
    State(state): State<Arc<AppState>>,
    Json(flow): Json<Flow>,
) -> Result<Json<Flow>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    let nodes_json = serde_json::to_string(&flow.nodes)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let edges_json = serde_json::to_string(&flow.edges)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let now = Utc::now().to_rfc3339();
    let id = if flow.id.is_empty() { uuid::Uuid::new_v4().to_string() } else { flow.id.clone() };

    db.conn.execute(
        r#"INSERT OR REPLACE INTO flows
           (id, name, description, nodes_json, edges_json, created_at, updated_at)
           VALUES (?1, ?2, ?3, ?4, ?5, COALESCE((SELECT created_at FROM flows WHERE id = ?1), ?6), ?6)"#,
        params![id, flow.name, flow.description, nodes_json, edges_json, now],
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(Flow { id, ..flow }))
}

pub async fn delete_flow(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();
    db.conn.execute("DELETE FROM flows WHERE id = ?1", params![id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
pub struct ExecuteFlowBody {
    pub input: Option<Value>,
}

pub async fn execute_flow(
    State(state): State<Arc<AppState>>,
    Path(flow_id): Path<String>,
    Json(body): Json<ExecuteFlowBody>,
) -> Result<Json<FlowExecution>, (StatusCode, String)> {
    let flow = {
        let db = state.db.lock().unwrap();
        let (id, name, description, nodes_json, edges_json) = db.conn.query_row(
            "SELECT id, name, description, nodes_json, edges_json FROM flows WHERE id = ?1",
            params![flow_id],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            )),
        ).map_err(|e| (StatusCode::NOT_FOUND, format!("Flow not found: {}", e)))?;

        let nodes: Vec<FlowNode> = serde_json::from_str(&nodes_json)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let edges: Vec<FlowEdge> = serde_json::from_str(&edges_json)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Flow { id, name, description, nodes, edges, created_at: Utc::now(), updated_at: Utc::now() }
    };

    let app_handle = state.app_handle.clone();
    let executor = crate::flow::FlowExecutor::with_app_handle(state.engine.clone(), app_handle);
    let execution = executor.execute(&flow, body.input).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    {
        let db = state.db.lock().unwrap();
        let results_json = serde_json::to_string(&execution.node_results)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let status = format!("{:?}", execution.status).to_lowercase();
        db.conn.execute(
            "INSERT INTO flow_executions (id, flow_id, status, node_results_json, started_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                execution.id, execution.flow_id, status, results_json,
                execution.started_at.to_rfc3339(),
                execution.completed_at.map(|t| t.to_rfc3339())
            ],
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    Ok(Json(execution))
}
