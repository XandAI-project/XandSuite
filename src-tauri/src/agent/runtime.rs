/// ReAct (Reason + Act) agentic runtime.
/// Implements the Thought -> Action -> Observation loop with safety guardrails:
///   - thinking tokens stripped before parsing
///   - actual wall-clock timeout enforced
///   - repetition guard to break stuck loops
///   - cancellation support

use anyhow::Result;
use chrono::Utc;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::tools::{
    CodeExecTool, DbQueryTool, FileOpsTool, HttpApiTool, WebSearchTool,
    tool_definitions_as_text,
};
use crate::db::AppDb;
use crate::models::{AgentAction, AgentStep, AgentTask, AgentTaskStatus, InferenceConfig};

pub struct AgentRuntime {
    db: Arc<Mutex<AppDb>>,
    workspace_dir: PathBuf,
    max_iterations: u32,
    timeout_seconds: u64,
    /// Per-task cancellation flags — set to `true` to request cancellation.
    cancel_tokens: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

#[derive(Debug, Clone)]
pub struct AgentEvent {
    pub task_id: String,
    pub event_type: AgentEventType,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub enum AgentEventType {
    Started,
    /// LLM is generating the next step (spinner cue for the UI).
    LlmGenerating,
    Thought,
    Action,
    Observation,
    Completed,
    Failed,
    Cancelled,
}

impl AgentRuntime {
    pub fn new(
        db: Arc<Mutex<AppDb>>,
        workspace_dir: PathBuf,
        max_iterations: u32,
        timeout_seconds: u64,
    ) -> Self {
        Self {
            db,
            workspace_dir,
            max_iterations,
            timeout_seconds,
            cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a cancellation flag for a newly created task.
    pub fn register_cancel(&self, task_id: &str, flag: Arc<AtomicBool>) {
        self.cancel_tokens
            .lock()
            .unwrap()
            .insert(task_id.to_string(), flag);
    }

    /// Signal cancellation for a running task.  Returns `true` if the task was found.
    pub fn cancel_task(&self, task_id: &str) -> bool {
        let tokens = self.cancel_tokens.lock().unwrap();
        if let Some(flag) = tokens.get(task_id) {
            flag.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    fn is_cancelled(&self, task_id: &str) -> bool {
        self.cancel_tokens
            .lock()
            .unwrap()
            .get(task_id)
            .map_or(false, |f| f.load(Ordering::SeqCst))
    }

    /// Main entry point called by the command handler.
    /// The `task` is already persisted in the DB with `Running` status.
    /// This method drives the ReAct loop and updates the task on every step.
    pub async fn run_loop(
        &self,
        mut task: AgentTask,
        engine: Arc<crate::engine::EngineManager>,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<()> {
        let timeout_dur = std::time::Duration::from_secs(self.timeout_seconds);

        let result = tokio::time::timeout(
            timeout_dur,
            self.execute_loop(&mut task, engine, &event_tx),
        )
        .await;

        match result {
            Ok(Ok(())) => {} // loop exited cleanly (completed/failed/cancelled)
            Ok(Err(e)) => {
                task.status = AgentTaskStatus::Failed;
                task.result = Some(format!("Runtime error: {}", e));
                task.completed_at = Some(Utc::now());
                let _ = self.save_task(&task);
                let _ = event_tx
                    .send(AgentEvent {
                        task_id: task.id.clone(),
                        event_type: AgentEventType::Failed,
                        payload: serde_json::json!({ "reason": e.to_string() }),
                    })
                    .await;
            }
            Err(_elapsed) => {
                task.status = AgentTaskStatus::Failed;
                task.result = Some(format!(
                    "Task timed out after {} seconds.",
                    self.timeout_seconds
                ));
                task.completed_at = Some(Utc::now());
                let _ = self.save_task(&task);
                let _ = event_tx
                    .send(AgentEvent {
                        task_id: task.id.clone(),
                        event_type: AgentEventType::Failed,
                        payload: serde_json::json!({ "reason": "timeout" }),
                    })
                    .await;
            }
        }

        // Clean up the cancel token slot
        self.cancel_tokens.lock().unwrap().remove(&task.id);
        Ok(())
    }

    async fn execute_loop(
        &self,
        task: &mut AgentTask,
        engine: Arc<crate::engine::EngineManager>,
        event_tx: &mpsc::Sender<AgentEvent>,
    ) -> Result<()> {
        let tools_description = tool_definitions_as_text();
        let system_prompt = build_react_system_prompt(&tools_description);

        let mut conversation: Vec<(String, String)> = vec![
            ("system".to_string(), system_prompt),
            (
                "user".to_string(),
                format!("Task: {}", task.description),
            ),
        ];

        let web_search = WebSearchTool::new();
        let code_exec = CodeExecTool::new(self.workspace_dir.join(&task.id));
        let file_ops = FileOpsTool::new(self.workspace_dir.join(&task.id));
        let http_api = HttpApiTool::new();
        let db_query = DbQueryTool::new(self.db.clone());

        // Disable thinking so the model outputs plain ReAct-formatted text,
        // not thinking tokens that confuse the parser.
        let mut inference_config = InferenceConfig::default();
        inference_config.enable_thinking = false;
        inference_config.thinking_budget_tokens = 0;

        let mut last_action: Option<(String, String)> = None;

        for iteration in 0..self.max_iterations {
            // ── Cancellation check ───────────────────────────────────────────
            if self.is_cancelled(&task.id) {
                task.status = AgentTaskStatus::Cancelled;
                task.result = Some("Task was cancelled.".to_string());
                task.completed_at = Some(Utc::now());
                self.save_task(task)?;
                let _ = event_tx
                    .send(AgentEvent {
                        task_id: task.id.clone(),
                        event_type: AgentEventType::Cancelled,
                        payload: serde_json::json!({}),
                    })
                    .await;
                return Ok(());
            }

            let step_num = iteration + 1;

            // ── Notify UI that the LLM is now generating ─────────────────────
            let _ = event_tx
                .send(AgentEvent {
                    task_id: task.id.clone(),
                    event_type: AgentEventType::LlmGenerating,
                    payload: serde_json::json!({ "step": step_num }),
                })
                .await;

            // ── Stream LLM response ──────────────────────────────────────────
            let (token_tx, mut token_rx) = mpsc::channel::<String>(256);
            let engine_clone = engine.clone();
            let conv_clone = conversation.clone();
            let config_clone = inference_config.clone();

            let gen_handle = tokio::spawn(async move {
                engine_clone
                    .chat_stream(conv_clone, config_clone, token_tx)
                    .await
            });

            let mut raw_output = String::new();
            while let Some(token) = token_rx.recv().await {
                if token == "[DONE]" {
                    break;
                }
                raw_output.push_str(&token);
            }
            let _ = gen_handle.await;

            // Strip thinking tokens/tags before ReAct parsing
            let llm_output = strip_thinking(&raw_output);

            // If the LLM produced absolutely nothing, inference is broken
            if llm_output.trim().is_empty() {
                task.status = AgentTaskStatus::Failed;
                task.result =
                    Some("No response from LLM — is a model loaded?".to_string());
                task.completed_at = Some(Utc::now());
                self.save_task(task)?;
                let _ = event_tx
                    .send(AgentEvent {
                        task_id: task.id.clone(),
                        event_type: AgentEventType::Failed,
                        payload: serde_json::json!({ "reason": "no_llm_response" }),
                    })
                    .await;
                return Ok(());
            }

            conversation.push(("assistant".to_string(), llm_output.clone()));

            // ── Parse ReAct format ───────────────────────────────────────────
            let parsed = parse_react_output(&llm_output);
            let thought = parsed.thought.clone().unwrap_or_default();
            let action_name = parsed.action_name.clone();
            let action_input = parsed.action_input.clone();

            // Emit Thought even when it is empty so the UI can show raw output
            let _ = event_tx
                .send(AgentEvent {
                    task_id: task.id.clone(),
                    event_type: AgentEventType::Thought,
                    payload: serde_json::json!({
                        "thought": thought,
                        "raw": if thought.is_empty() { &llm_output } else { &thought },
                        "step": step_num,
                    }),
                })
                .await;

            // ── Final answer or no action ────────────────────────────────────
            if parsed.final_answer.is_some() || action_name.is_none() {
                let result =
                    parsed.final_answer.unwrap_or_else(|| llm_output.clone());
                task.status = AgentTaskStatus::Completed;
                task.result = Some(result.clone());
                task.completed_at = Some(Utc::now());
                self.save_task(task)?;
                let _ = event_tx
                    .send(AgentEvent {
                        task_id: task.id.clone(),
                        event_type: AgentEventType::Completed,
                        payload: serde_json::json!({ "result": result }),
                    })
                    .await;
                return Ok(());
            }

            let tool_name = action_name.unwrap();
            let tool_input =
                action_input.unwrap_or(Value::Object(serde_json::Map::new()));
            let input_str =
                serde_json::to_string(&tool_input).unwrap_or_default();

            // ── Repetition guard ─────────────────────────────────────────────
            let current_action = (tool_name.clone(), input_str.clone());
            if Some(&current_action) == last_action.as_ref() {
                task.status = AgentTaskStatus::Failed;
                task.result = Some(format!(
                    "Agent stuck: '{}' called with identical input twice in a row.",
                    tool_name
                ));
                task.completed_at = Some(Utc::now());
                self.save_task(task)?;
                let _ = event_tx
                    .send(AgentEvent {
                        task_id: task.id.clone(),
                        event_type: AgentEventType::Failed,
                        payload: serde_json::json!({
                            "reason": "repetition_loop",
                            "tool": tool_name,
                        }),
                    })
                    .await;
                return Ok(());
            }
            last_action = Some(current_action);

            // ── Emit Action event ────────────────────────────────────────────
            let _ = event_tx
                .send(AgentEvent {
                    task_id: task.id.clone(),
                    event_type: AgentEventType::Action,
                    payload: serde_json::json!({
                        "thought": thought,
                        "tool": tool_name,
                        "input": tool_input,
                        "step": step_num,
                    }),
                })
                .await;

            // ── Execute tool ─────────────────────────────────────────────────
            let observation = self
                .execute_tool(
                    &tool_name,
                    &tool_input,
                    &web_search,
                    &code_exec,
                    &file_ops,
                    &http_api,
                    &db_query,
                )
                .await;

            let (obs_value, obs_error) = match &observation {
                Ok(v) => (v.clone(), None::<String>),
                Err(e) => (Value::Null, Some(e.to_string())),
            };
            let is_error = obs_error.is_some();
            let obs_text = obs_error.clone().unwrap_or_else(|| {
                serde_json::to_string_pretty(&obs_value).unwrap_or_default()
            });

            // ── Emit Observation event ───────────────────────────────────────
            let _ = event_tx
                .send(AgentEvent {
                    task_id: task.id.clone(),
                    event_type: AgentEventType::Observation,
                    payload: serde_json::json!({
                        "observation": obs_text,
                        "tool": tool_name,
                        "step": step_num,
                        "error": is_error,
                    }),
                })
                .await;

            conversation.push((
                "user".to_string(),
                format!("Observation: {}", obs_text),
            ));

            let step = AgentStep {
                step_number: step_num,
                thought: thought.clone(),
                action: Some(AgentAction {
                    tool_name: tool_name.clone(),
                    input: tool_input,
                    output: observation.ok(),
                    error: obs_error,
                }),
                observation: Some(obs_text),
                created_at: Utc::now(),
            };
            task.steps.push(step);
            self.save_task(task)?;
        }

        // ── Max iterations reached ───────────────────────────────────────────
        task.status = AgentTaskStatus::Failed;
        task.result = Some(format!(
            "Task exceeded maximum iterations ({}). Last observation: {}",
            self.max_iterations,
            task.steps
                .last()
                .and_then(|s| s.observation.as_deref())
                .unwrap_or("none")
        ));
        task.completed_at = Some(Utc::now());
        self.save_task(task)?;
        let _ = event_tx
            .send(AgentEvent {
                task_id: task.id.clone(),
                event_type: AgentEventType::Failed,
                payload: serde_json::json!({ "reason": "max_iterations_exceeded" }),
            })
            .await;
        Ok(())
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        input: &Value,
        web_search: &WebSearchTool,
        code_exec: &CodeExecTool,
        file_ops: &FileOpsTool,
        http_api: &HttpApiTool,
        db_query: &DbQueryTool,
    ) -> Result<Value> {
        match tool_name {
            "web_search" => {
                let query = input["query"].as_str().unwrap_or("");
                web_search.search(query).await
            }
            "code_exec" => {
                let command = input["command"].as_str().unwrap_or("");
                let timeout = input["timeout_seconds"].as_u64().unwrap_or(30);
                code_exec.execute(command, timeout).await
            }
            "file_read" => {
                let path = input["path"].as_str().unwrap_or("");
                file_ops.read_file(path).await
            }
            "file_write" => {
                let path = input["path"].as_str().unwrap_or("");
                let content = input["content"].as_str().unwrap_or("");
                file_ops.write_file(path, content).await
            }
            "db_query" => {
                let conn_id = input["connection_id"].as_str().unwrap_or("");
                let query = input["query"].as_str().unwrap_or("");
                db_query.execute(conn_id, query).await
            }
            "http_api" => {
                let method = input["method"].as_str().unwrap_or("GET");
                let url = input["url"].as_str().unwrap_or("");
                let headers = input["headers"].as_object().map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                });
                let body = input["body"].as_str();
                http_api.request(method, url, headers, body).await
            }
            other => anyhow::bail!("Unknown tool: {}", other),
        }
    }

    fn save_task(&self, task: &AgentTask) -> Result<()> {
        let steps_json = serde_json::to_string(&task.steps)?;
        let status = format!("{:?}", task.status).to_lowercase();
        let db = self.db.lock().unwrap();
        db.conn.execute(
            r#"INSERT OR REPLACE INTO agent_tasks
               (id, title, description, status, steps_json, created_at, completed_at, result)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
            rusqlite::params![
                task.id,
                task.title,
                task.description,
                status,
                steps_json,
                task.created_at.to_rfc3339(),
                task.completed_at.map(|t| t.to_rfc3339()),
                task.result
            ],
        )?;
        Ok(())
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Remove thinking content that must not be parsed as ReAct output:
///  1. Lines prefixed with the internal THINKING_PREFIX sentinel (`\x01think:`)
///  2. `<think>…</think>` blocks (possibly spanning many lines)
fn strip_thinking(text: &str) -> String {
    let thinking_prefix = crate::engine::remote::THINKING_PREFIX;

    // Pass 1: drop sentinel-prefixed lines
    let step1: String = text
        .lines()
        .filter(|line| !line.starts_with(thinking_prefix))
        .collect::<Vec<_>>()
        .join("\n");

    // Pass 2: remove <think>…</think> spans
    let mut result = String::with_capacity(step1.len());
    let mut remaining = step1.as_str();
    loop {
        if let Some(start) = remaining.find("<think>") {
            result.push_str(&remaining[..start]);
            let after_open = &remaining[start + "<think>".len()..];
            if let Some(end) = after_open.find("</think>") {
                remaining = &after_open[end + "</think>".len()..];
            } else {
                break; // unclosed tag — discard rest
            }
        } else {
            result.push_str(remaining);
            break;
        }
    }

    result.trim().to_string()
}

struct ReactOutput {
    thought: Option<String>,
    action_name: Option<String>,
    action_input: Option<Value>,
    final_answer: Option<String>,
}

/// Parse multi-line ReAct output.  Each section accumulates until the next
/// recognised prefix so that multi-line thoughts / final answers are captured.
fn parse_react_output(text: &str) -> ReactOutput {
    #[derive(PartialEq)]
    enum Section { None, Thought, FinalAnswer }

    let mut thought_lines: Vec<String> = vec![];
    let mut action_name: Option<String> = None;
    let mut action_input: Option<Value> = None;
    let mut final_lines: Vec<String> = vec![];
    let mut section = Section::None;

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Thought:") {
            section = Section::Thought;
            thought_lines.clear();
            let v = rest.trim().to_string();
            if !v.is_empty() { thought_lines.push(v); }
        } else if let Some(rest) = trimmed.strip_prefix("Action:") {
            section = Section::None;
            action_name = Some(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("Action Input:") {
            section = Section::None;
            let raw = rest.trim();
            action_input = serde_json::from_str(raw)
                .ok()
                .or_else(|| Some(Value::String(raw.to_string())));
        } else if let Some(rest) = trimmed.strip_prefix("Final Answer:") {
            section = Section::FinalAnswer;
            final_lines.clear();
            let v = rest.trim().to_string();
            if !v.is_empty() { final_lines.push(v); }
        } else if !trimmed.is_empty() {
            match section {
                Section::Thought => thought_lines.push(trimmed.to_string()),
                Section::FinalAnswer => final_lines.push(trimmed.to_string()),
                Section::None => {}
            }
        }
    }

    ReactOutput {
        thought: if thought_lines.is_empty() {
            None
        } else {
            Some(thought_lines.join("\n"))
        },
        action_name,
        action_input,
        final_answer: if final_lines.is_empty() {
            None
        } else {
            Some(final_lines.join("\n"))
        },
    }
}

fn build_react_system_prompt(tools: &str) -> String {
    format!(
        r#"You are XandSuite Agent, an autonomous AI assistant that solves tasks by reasoning and using tools.

Use the following format strictly:

Thought: [your reasoning about what to do next]
Action: [tool name from the list below]
Action Input: [JSON object with tool parameters]

After receiving an observation, continue with another Thought/Action cycle or provide the final answer:

Thought: [final reasoning]
Final Answer: [your complete answer to the task]

Available tools:
{}

Guidelines:
- Break complex tasks into small steps
- Verify assumptions before acting
- Handle errors gracefully and try alternatives
- Be concise in your thoughts
- Always provide a Final Answer when done
- Do NOT wrap your response in <think> tags — output Thought/Action/Final Answer directly

Begin!"#,
        tools
    )
}

// Keep the old `run_task` for backward-compat with any internal callers
impl AgentRuntime {
    #[allow(dead_code)]
    pub async fn run_task(
        &self,
        task_description: &str,
        engine: Arc<crate::engine::EngineManager>,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<AgentTask> {
        let task_id = Uuid::new_v4().to_string();
        let task = AgentTask {
            id: task_id.clone(),
            title: task_description.chars().take(80).collect(),
            description: task_description.to_string(),
            status: AgentTaskStatus::Running,
            steps: vec![],
            created_at: Utc::now(),
            completed_at: None,
            result: None,
        };
        self.save_task(&task)?;
        let _ = event_tx.send(AgentEvent {
            task_id: task_id.clone(),
            event_type: AgentEventType::Started,
            payload: serde_json::json!({ "task": task_description }),
        }).await;

        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.register_cancel(&task_id, cancel_flag);
        self.run_loop(task.clone(), engine, event_tx).await?;

        // Reload from DB to get the final state
        let db = self.db.lock().unwrap();
        let updated: Option<AgentTask> = db.conn.query_row(
            "SELECT id, title, description, status, steps_json, result
             FROM agent_tasks WHERE id = ?1",
            rusqlite::params![task_id],
            |row| {
                let steps_json: String = row.get(4)?;
                let steps = serde_json::from_str(&steps_json).unwrap_or_default();
                let status_str: String = row.get(3)?;
                let status = match status_str.as_str() {
                    "running" => AgentTaskStatus::Running,
                    "completed" => AgentTaskStatus::Completed,
                    "failed" => AgentTaskStatus::Failed,
                    "cancelled" => AgentTaskStatus::Cancelled,
                    _ => AgentTaskStatus::Pending,
                };
                Ok(AgentTask {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    description: row.get(2)?,
                    status,
                    steps,
                    created_at: Utc::now(),
                    completed_at: None,
                    result: row.get(5)?,
                })
            },
        ).ok();
        Ok(updated.unwrap_or(task))
    }
}
