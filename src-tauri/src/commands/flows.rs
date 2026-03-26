use chrono::Utc;
use rusqlite::params;
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::models::{Flow, FlowEdge, FlowExecution, FlowNode};
use crate::state::AppState;

#[tauri::command]
pub fn list_flows(state: State<'_, AppState>) -> Result<Vec<Flow>, String> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.conn.prepare(
        "SELECT id, name, description, nodes_json, edges_json FROM flows ORDER BY updated_at DESC"
    ).map_err(|e| e.to_string())?;

    let flows = stmt.query_map([], |row| {
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
    .map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    Ok(flows)
}

#[tauri::command]
pub fn save_flow(flow: Flow, state: State<'_, AppState>) -> Result<Flow, String> {
    let db = state.db.lock().unwrap();
    let nodes_json = serde_json::to_string(&flow.nodes).map_err(|e| e.to_string())?;
    let edges_json = serde_json::to_string(&flow.edges).map_err(|e| e.to_string())?;
    let now = Utc::now().to_rfc3339();
    let id = if flow.id.is_empty() { Uuid::new_v4().to_string() } else { flow.id.clone() };

    db.conn.execute(
        r#"INSERT OR REPLACE INTO flows
           (id, name, description, nodes_json, edges_json, created_at, updated_at)
           VALUES (?1, ?2, ?3, ?4, ?5, COALESCE((SELECT created_at FROM flows WHERE id = ?1), ?6), ?6)"#,
        params![id, flow.name, flow.description, nodes_json, edges_json, now],
    ).map_err(|e| e.to_string())?;

    Ok(Flow { id, ..flow })
}

#[tauri::command]
pub fn delete_flow(flow_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.conn.execute("DELETE FROM flows WHERE id = ?1", params![flow_id]).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn execute_flow(
    app: AppHandle,
    flow_id: String,
    input: Option<serde_json::Value>,
    state: State<'_, AppState>,
) -> Result<FlowExecution, String> {
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
        ).map_err(|e| format!("Flow not found: {}", e))?;

        let nodes: Vec<FlowNode> = serde_json::from_str(&nodes_json).map_err(|e| e.to_string())?;
        let edges: Vec<FlowEdge> = serde_json::from_str(&edges_json).map_err(|e| e.to_string())?;
        Flow { id, name, description, nodes, edges, created_at: Utc::now(), updated_at: Utc::now() }
    };

    let executor = crate::flow::FlowExecutor::with_app_handle(state.engine.clone(), app);
    let execution = executor.execute(&flow, input).await.map_err(|e| e.to_string())?;

    {
        let db = state.db.lock().unwrap();
        let results_json = serde_json::to_string(&execution.node_results).map_err(|e| e.to_string())?;
        let status = format!("{:?}", execution.status).to_lowercase();
        db.conn.execute(
            "INSERT INTO flow_executions (id, flow_id, status, node_results_json, started_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                execution.id, execution.flow_id, status, results_json,
                execution.started_at.to_rfc3339(),
                execution.completed_at.map(|t| t.to_rfc3339())
            ],
        ).map_err(|e| e.to_string())?;
    }

    Ok(execution)
}
