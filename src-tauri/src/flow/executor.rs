use anyhow::Result;
use chrono::Utc;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::nodes::{NodeContext, evaluate_condition, render_template};
use crate::models::{
    Flow, FlowEdge, FlowExecution, FlowExecutionStatus, FlowNode, FlowNodeType, NodeResult,
};

pub struct FlowExecutor {
    engine: Arc<crate::engine::EngineManager>,
    app_handle: Option<tauri::AppHandle>,
}

impl FlowExecutor {
    pub fn new(engine: Arc<crate::engine::EngineManager>) -> Self {
        Self { engine, app_handle: None }
    }

    pub fn with_app_handle(
        engine: Arc<crate::engine::EngineManager>,
        app_handle: tauri::AppHandle,
    ) -> Self {
        Self { engine, app_handle: Some(app_handle) }
    }

    fn emit_progress(&self, node_id: &str, node_label: &str, step: usize, total: usize, status: &str) {
        if let Some(app) = &self.app_handle {
            let _ = tauri::Emitter::emit(app, "flow_progress", serde_json::json!({
                "node_id": node_id,
                "node_label": node_label,
                "step": step,
                "total": total,
                "status": status,
            }));
        }
    }

    pub async fn execute(
        &self,
        flow: &Flow,
        initial_input: Option<Value>,
    ) -> Result<FlowExecution> {
        let execution_id = Uuid::new_v4().to_string();
        let mut node_results: Vec<NodeResult> = Vec::new();
        let mut ctx = NodeContext::new();

        if let Some(input) = initial_input {
            ctx.set("input", input);
        }

        // Build adjacency maps
        let adj = build_adjacency(&flow.edges);
        let in_degree = build_in_degree(&flow.nodes, &flow.edges);

        // Topological sort (Kahn's algorithm)
        let ordered = topological_sort(&flow.nodes, &adj, &in_degree)?;

        let total = ordered.len();
        let mut execution_status = FlowExecutionStatus::Completed;

        for (step_idx, node_id) in ordered.iter().enumerate() {
            let node = flow
                .nodes
                .iter()
                .find(|n| &n.id == node_id)
                .ok_or_else(|| anyhow::anyhow!("Node not found: {}", node_id))?;

            let node_label = node.data.get("label")
                .and_then(|v| v.as_str())
                .unwrap_or(node_id.as_str())
                .to_string();

            // Notify UI that this node is now running
            self.emit_progress(node_id, &node_label, step_idx + 1, total, "running");

            let start = Instant::now();
            let result = self.execute_node(node, &mut ctx).await;
            let duration_ms = start.elapsed().as_millis() as u64;

            match result {
                Ok(output) => {
                    ctx.set(&format!("node_{}", node_id), output.clone());
                    ctx.last_output = Some(output.clone());
                    self.emit_progress(node_id, &node_label, step_idx + 1, total, "done");
                    node_results.push(NodeResult {
                        node_id: node_id.clone(),
                        output,
                        error: None,
                        duration_ms,
                    });
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    self.emit_progress(node_id, &node_label, step_idx + 1, total, "error");
                    node_results.push(NodeResult {
                        node_id: node_id.clone(),
                        output: Value::Null,
                        error: Some(err_msg),
                        duration_ms,
                    });
                    execution_status = FlowExecutionStatus::Failed;
                    // Continue executing remaining nodes even after failure
                }
            }
        }

        // Signal execution finished so the UI clears the active node
        self.emit_progress("", "", 0, total, "completed");

        Ok(FlowExecution {
            id: execution_id,
            flow_id: flow.id.clone(),
            status: execution_status,
            node_results,
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
        })
    }

    async fn execute_node(&self, node: &FlowNode, ctx: &mut NodeContext) -> Result<Value> {
        let data = &node.data;

        match node.node_type {
            FlowNodeType::Trigger => {
                let trigger_type = data["trigger_type"].as_str().unwrap_or("manual");
                let label = data["label"].as_str().unwrap_or("Trigger");
                ctx.set("trigger_type", Value::String(trigger_type.to_string()));
                Ok(serde_json::json!({
                    "trigger_type": trigger_type,
                    "label": label,
                    "triggered_at": chrono::Utc::now().to_rfc3339(),
                }))
            }

            FlowNodeType::SystemPrompt => {
                let prompt = data["prompt"].as_str().unwrap_or("");
                let rendered = render_template(prompt, ctx);
                ctx.set("system_prompt", Value::String(rendered.clone()));
                Ok(Value::String(rendered))
            }

            FlowNodeType::UserPrompt | FlowNodeType::TemplatePrompt => {
                let prompt = data["prompt"].as_str().unwrap_or("");
                let rendered = render_template(prompt, ctx);

                let system = ctx.get("system_prompt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let mut config = crate::models::InferenceConfig::default();
                if let Some(temp) = data["temperature"].as_f64() {
                    config.temperature = temp as f32;
                }
                if let Some(max_tok) = data["max_tokens"].as_u64() {
                    config.max_tokens = max_tok as u32;
                }
                if let Some(top_p) = data["top_p"].as_f64() {
                    config.top_p = top_p as f32;
                }

                let mut messages: Vec<(String, String)> = Vec::new();

                if !system.is_empty() {
                    messages.push(("system".to_string(), system));
                }
                messages.push(("user".to_string(), rendered));

                let (token_tx, mut token_rx) = mpsc::channel::<String>(256);
                let engine = self.engine.clone();
                let msgs = messages.clone();
                let cfg = config.clone();

                let gen = tokio::spawn(async move {
                    engine.chat_stream(msgs, cfg, token_tx).await
                });

                let mut response = String::new();
                while let Some(token) = token_rx.recv().await {
                    if token == "[DONE]" {
                        break;
                    }
                    response.push_str(&token);
                }
                let _ = gen.await;

                ctx.set("last_response", Value::String(response.clone()));
                Ok(Value::String(response))
            }

            FlowNodeType::Conditional => {
                let condition = data["condition"].as_str().unwrap_or("true");
                let result = evaluate_condition(condition, ctx);
                Ok(Value::Bool(result))
            }

            FlowNodeType::Input => {
                let var_name = data["variable"].as_str().unwrap_or("input");
                let value = ctx.get(var_name).cloned().unwrap_or(Value::Null);
                Ok(value)
            }

            FlowNodeType::Output => {
                let var_name = data["variable"].as_str().unwrap_or("last_response");
                let value = ctx.get(var_name).cloned()
                    .or_else(|| ctx.last_output.clone())
                    .unwrap_or(Value::Null);
                Ok(value)
            }

            FlowNodeType::Merge => {
                let output = ctx.last_output.clone().unwrap_or(Value::Null);
                Ok(output)
            }

            FlowNodeType::Loop => {
                let iterations = data["iterations"].as_u64().unwrap_or(1);
                let loop_var = data["loop_variable"].as_str().unwrap_or("i");
                ctx.set(loop_var, Value::Number(iterations.into()));
                Ok(serde_json::json!({
                    "iterations": iterations,
                    "loop_variable": loop_var,
                }))
            }

            FlowNodeType::WebSearch => {
                let query_template = data["query"].as_str().unwrap_or("");
                let query = render_template(query_template, ctx);
                let tool = crate::agent::tools::WebSearchTool::new();
                tool.search(&query).await
            }

            FlowNodeType::CodeExec => {
                let cmd_template = data["command"].as_str().unwrap_or("");
                let command = render_template(cmd_template, ctx);
                let timeout = data["timeout_seconds"].as_u64().unwrap_or(30);
                let workspace = dirs::data_dir()
                    .unwrap_or_default()
                    .join("XandSuite")
                    .join("flow_workspace");
                let tool = crate::agent::tools::CodeExecTool::new(workspace);
                tool.execute(&command, timeout).await
            }

            FlowNodeType::HttpApi => {
                let method = data["method"].as_str().unwrap_or("GET");
                let url_template = data["url"].as_str().unwrap_or("");
                let url = render_template(url_template, ctx);

                let headers: Option<std::collections::HashMap<String, String>> = data
                    .get("headers")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .and_then(|s| serde_json::from_str(s).ok());

                let body_rendered: Option<String> = data
                    .get("body")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| render_template(s, ctx));

                let tool = crate::agent::tools::HttpApiTool::new();
                tool.request(method, &url, headers, body_rendered.as_deref()).await
            }

            FlowNodeType::DbQuery => {
                let query_template = data["query"].as_str().unwrap_or("");
                let query = render_template(query_template, ctx);
                let connection_id = data["connection_id"].as_str().unwrap_or("");
                if connection_id.is_empty() || query.is_empty() {
                    Ok(serde_json::json!({
                        "note": "DbQuery node requires a connection_id and query. Configure in node settings."
                    }))
                } else {
                    Ok(serde_json::json!({
                        "connection_id": connection_id,
                        "query": query,
                        "note": "Query prepared. Execution requires active DB connection."
                    }))
                }
            }
        }
    }
}

fn build_adjacency(edges: &[FlowEdge]) -> HashMap<String, Vec<String>> {
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for edge in edges {
        adj.entry(edge.source.clone())
            .or_default()
            .push(edge.target.clone());
    }
    adj
}

fn build_in_degree(nodes: &[FlowNode], edges: &[FlowEdge]) -> HashMap<String, usize> {
    let mut in_degree: HashMap<String, usize> = nodes
        .iter()
        .map(|n| (n.id.clone(), 0))
        .collect();
    for edge in edges {
        *in_degree.entry(edge.target.clone()).or_insert(0) += 1;
    }
    in_degree
}

fn topological_sort(
    nodes: &[FlowNode],
    adj: &HashMap<String, Vec<String>>,
    in_degree: &HashMap<String, usize>,
) -> Result<Vec<String>> {
    let mut in_deg = in_degree.clone();
    let mut queue: VecDeque<String> = in_deg
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(id, _)| id.clone())
        .collect();

    let mut order = Vec::new();

    while let Some(node_id) = queue.pop_front() {
        order.push(node_id.clone());
        if let Some(neighbors) = adj.get(&node_id) {
            for neighbor in neighbors {
                let deg = in_deg.entry(neighbor.clone()).or_insert(0);
                *deg = deg.saturating_sub(1);
                if *deg == 0 {
                    queue.push_back(neighbor.clone());
                }
            }
        }
    }

    if order.len() != nodes.len() {
        anyhow::bail!("Flow graph contains a cycle");
    }

    Ok(order)
}
