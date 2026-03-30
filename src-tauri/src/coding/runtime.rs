use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::prompts::build_system_prompt;
use super::tools::CodingToolExecutor;
use crate::db::AppDb;
use crate::models::InferenceConfig;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingSession {
    pub id: String,
    pub title: String,
    pub mode: String,
    pub project_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub events: Vec<CodingEventPayload>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingPlanTask {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: String, // pending | in_progress | completed | failed
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingPlan {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub tasks: Vec<CodingPlanTask>,
    pub status: String, // pending | in_progress | completed
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingEventPayload {
    pub event_type: String,
    pub payload: Value,
}

// ── Runtime event (sent over the channel to the command handler) ──────────────

#[derive(Debug, Clone)]
pub struct CodingEvent {
    pub session_id: String,
    pub event_type: CodingEventType,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub enum CodingEventType {
    Started,
    Thinking,
    Action,
    Observation,
    PlanCreated,
    TaskUpdated,
    Completed,
    Failed,
    Cancelled,
}

impl CodingEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            CodingEventType::Started => "started",
            CodingEventType::Thinking => "thinking",
            CodingEventType::Action => "action",
            CodingEventType::Observation => "observation",
            CodingEventType::PlanCreated => "plan_created",
            CodingEventType::TaskUpdated => "task_updated",
            CodingEventType::Completed => "completed",
            CodingEventType::Failed => "failed",
            CodingEventType::Cancelled => "cancelled",
        }
    }
}

// ── Runtime ───────────────────────────────────────────────────────────────────

pub struct CodingRuntime {
    db: Arc<Mutex<AppDb>>,
    max_iterations: u32,
    timeout_seconds: u64,
    cancel_tokens: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl CodingRuntime {
    pub fn new(db: Arc<Mutex<AppDb>>, max_iterations: u32, timeout_seconds: u64) -> Self {
        Self {
            db,
            max_iterations,
            timeout_seconds,
            cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register_cancel(&self, session_id: &str, flag: Arc<AtomicBool>) {
        self.cancel_tokens
            .lock()
            .unwrap()
            .insert(session_id.to_string(), flag);
    }

    pub fn cancel_session(&self, session_id: &str) -> bool {
        let tokens = self.cancel_tokens.lock().unwrap();
        if let Some(flag) = tokens.get(session_id) {
            flag.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    fn is_cancelled(&self, session_id: &str) -> bool {
        self.cancel_tokens
            .lock()
            .unwrap()
            .get(session_id)
            .map_or(false, |f| f.load(Ordering::SeqCst))
    }

    /// Run the coding loop for a single user message, firing events as it progresses.
    pub async fn run(
        &self,
        session_id: String,
        user_message: String,
        mode: String,
        project_path: Option<String>,
        history: Vec<(String, String)>, // (role, content) prior messages
        engine: Arc<crate::engine::EngineManager>,
        event_tx: mpsc::Sender<CodingEvent>,
    ) -> Result<String> {
        let timeout_dur = std::time::Duration::from_secs(self.timeout_seconds);

        let result = tokio::time::timeout(
            timeout_dur,
            self.execute_loop(
                session_id.clone(),
                user_message,
                mode,
                project_path,
                history,
                engine,
                &event_tx,
            ),
        )
        .await;

        match result {
            Ok(Ok(answer)) => Ok(answer),
            Ok(Err(e)) => {
                let _ = event_tx
                    .send(CodingEvent {
                        session_id: session_id.clone(),
                        event_type: CodingEventType::Failed,
                        payload: serde_json::json!({ "reason": e.to_string() }),
                    })
                    .await;
                Err(e)
            }
            Err(_) => {
                let _ = event_tx
                    .send(CodingEvent {
                        session_id: session_id.clone(),
                        event_type: CodingEventType::Failed,
                        payload: serde_json::json!({ "reason": "timeout" }),
                    })
                    .await;
                anyhow::bail!("Session timed out after {} seconds", self.timeout_seconds)
            }
        }
    }

    async fn execute_loop(
        &self,
        session_id: String,
        user_message: String,
        mode: String,
        project_path: Option<String>,
        history: Vec<(String, String)>,
        engine: Arc<crate::engine::EngineManager>,
        event_tx: &mpsc::Sender<CodingEvent>,
    ) -> Result<String> {
        let project_root = project_path
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs::document_dir().unwrap_or_else(|| PathBuf::from(".")));

        let executor = CodingToolExecutor::new(project_root.clone());
        let system_prompt = build_system_prompt(&mode, project_path.as_deref());

        // Build conversation: system + history + new user message
        let mut conversation: Vec<(String, String)> = vec![
            ("system".to_string(), system_prompt),
        ];
        conversation.extend(history);
        conversation.push(("user".to_string(), user_message.clone()));

        let _ = event_tx
            .send(CodingEvent {
                session_id: session_id.clone(),
                event_type: CodingEventType::Started,
                payload: serde_json::json!({ "message": user_message }),
            })
            .await;

        let mut inference_config = InferenceConfig::default();
        inference_config.enable_thinking = false;
        inference_config.thinking_budget_tokens = 0;

        // last_success_action: the most recent tool call that SUCCEEDED.
        // Used to detect true repetition loops (successful step replayed unchanged).
        let mut last_success_action: Option<(String, String)> = None;
        // last_fail_key / fail_streak: track consecutive failures of the same call
        // so we can bail gracefully instead of running to max_iterations.
        let mut last_fail_key: Option<(String, String)> = None;
        let mut fail_streak: u32 = 0;

        // Load any plan that was created in a prior run (e.g. Plan-mode session)
        // so that update_task can actually update it in Agent mode.
        let mut current_plan: Option<CodingPlan> = self.load_plan(&session_id).ok().flatten();

        // Tracks how many real tool calls have been executed in this run.
        // Used to reject a premature Final Answer on the very first iteration.
        let mut tool_calls_made: u32 = 0;

        for iteration in 0..self.max_iterations {
            if self.is_cancelled(&session_id) {
                let _ = event_tx
                    .send(CodingEvent {
                        session_id: session_id.clone(),
                        event_type: CodingEventType::Cancelled,
                        payload: serde_json::json!({}),
                    })
                    .await;
                return Ok("Cancelled.".to_string());
            }

            let step_num = iteration + 1;

            // LLM call
            let (token_tx, mut token_rx) = mpsc::channel::<String>(256);
            let engine_clone = engine.clone();
            let conv_clone = conversation.clone();
            let cfg_clone = inference_config.clone();

            let gen_handle = tokio::spawn(async move {
                engine_clone.chat_stream(conv_clone, cfg_clone, token_tx).await
            });

            let mut raw = String::new();
            while let Some(token) = token_rx.recv().await {
                if token == "[DONE]" {
                    break;
                }
                raw.push_str(&token);
            }
            let _ = gen_handle.await;

            let llm_output = strip_thinking(&raw);

            if llm_output.trim().is_empty() {
                let _ = event_tx
                    .send(CodingEvent {
                        session_id: session_id.clone(),
                        event_type: CodingEventType::Failed,
                        payload: serde_json::json!({ "reason": "no_llm_response" }),
                    })
                    .await;
                anyhow::bail!("No response from LLM — is a model loaded?");
            }

            conversation.push(("assistant".to_string(), llm_output.clone()));

            let parsed = parse_react_output(&llm_output);
            let thought = parsed.thought.clone().unwrap_or_default();

            // Emit thinking event
            let _ = event_tx
                .send(CodingEvent {
                    session_id: session_id.clone(),
                    event_type: CodingEventType::Thinking,
                    payload: serde_json::json!({
                        "thought": thought,
                        "raw": if thought.is_empty() { &llm_output } else { &thought },
                        "step": step_num,
                    }),
                })
                .await;

            // Final answer — return, but reject it when no tools have been used yet.
            // Each mode has its mandatory minimum:
            //   ask   → may answer in prose with 0 tool calls (pure Q&A)
            //   plan  → MUST call create_plan before answering
            //   agent/debug → MUST call at least one file/shell tool before answering
            if parsed.final_answer.is_some() || parsed.action_name.is_none() {
                if tool_calls_made == 0 && mode != "ask" {
                    let correction = if mode == "plan" {
                        "ERROR — You described the plan in text but you have NOT called \
                         create_plan yet. You MUST call create_plan with a structured task list \
                         before giving your Final Answer. Use the create_plan tool now:\n\
                         Thought: [your plan reasoning]\n\
                         Action: create_plan\n\
                         Action Input: {\"title\": \"...\", \"tasks\": [{\"title\": \"...\", \"description\": \"...\"}]}"
                            .to_string()
                    } else {
                        "ERROR — You gave a Final Answer but you have NOT called any tools yet. \
                         You MUST actually use your tools (file_write, shell_exec, directory_tree, \
                         etc.) to perform the work before answering. \
                         Do NOT describe or summarise what you would do — DO IT. \
                         Continue immediately with:\nThought: [what you will do first]\n\
                         Action: [tool name]\nAction Input: [JSON]"
                            .to_string()
                    };
                    conversation.push(("user".to_string(), format!("Observation: {}", correction)));
                    continue;
                }

                let answer = parsed.final_answer.unwrap_or_else(|| llm_output.clone());
                let _ = event_tx
                    .send(CodingEvent {
                        session_id: session_id.clone(),
                        event_type: CodingEventType::Completed,
                        payload: serde_json::json!({ "answer": answer }),
                    })
                    .await;
                self.cancel_tokens.lock().unwrap().remove(&session_id);
                return Ok(answer);
            }

            let tool_name = parsed.action_name.unwrap();
            let tool_input = parsed.action_input.unwrap_or(Value::Object(serde_json::Map::new()));

            // Some models emit `Action: Final Answer` / `Action Input: {"answer": "..."}` instead
            // of the plain `Final Answer: ...` syntax.  Treat them as equivalent.
            if tool_name.to_lowercase().contains("final answer") || tool_name.to_lowercase() == "finalanswer" {
                let answer = tool_input["answer"]
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| serde_json::to_string_pretty(&tool_input).ok())
                    .unwrap_or_else(|| tool_name.clone());
                let _ = event_tx
                    .send(CodingEvent {
                        session_id: session_id.clone(),
                        event_type: CodingEventType::Completed,
                        payload: serde_json::json!({ "answer": answer }),
                    })
                    .await;
                self.cancel_tokens.lock().unwrap().remove(&session_id);
                return Ok(answer);
            }

            let input_str = serde_json::to_string(&tool_input).unwrap_or_default();

            // Repetition guard (success path only).
            // A successful call repeated with identical input = stuck loop → bail.
            // We deliberately do NOT check failed calls here: the agent must be allowed
            // to retry a tool that just errored (e.g. a transient parse failure).
            // Consecutive failure protection is handled after the tool executes.
            let current_action = (tool_name.clone(), input_str.clone());
            if Some(&current_action) == last_success_action.as_ref() {
                let _ = event_tx
                    .send(CodingEvent {
                        session_id: session_id.clone(),
                        event_type: CodingEventType::Failed,
                        payload: serde_json::json!({ "reason": "repetition_loop", "tool": tool_name }),
                    })
                    .await;
                anyhow::bail!("Agent stuck: '{}' succeeded but was called again with identical input.", tool_name);
            }

            // Emit action event
            let _ = event_tx
                .send(CodingEvent {
                    session_id: session_id.clone(),
                    event_type: CodingEventType::Action,
                    payload: serde_json::json!({
                        "tool": tool_name,
                        "input": tool_input,
                        "thought": thought,
                        "step": step_num,
                    }),
                })
                .await;

            // Special handling for plan tools before executor
            if tool_name == "create_plan" {
                if let Some(new_plan) = handle_create_plan(&session_id, &tool_input) {
                    // Save to DB
                    self.save_plan(&new_plan)?;
                    let plan_json = serde_json::to_value(&new_plan).unwrap_or_default();
                    let _ = event_tx
                        .send(CodingEvent {
                            session_id: session_id.clone(),
                            event_type: CodingEventType::PlanCreated,
                            payload: plan_json,
                        })
                        .await;
                    tool_calls_made += 1;

                    // In Plan mode the plan IS the entire deliverable — terminate immediately
                    // so the model never has a chance to invent file-writing tools.
                    if mode == "plan" {
                        let summary = format!(
                            "Plan '{}' created with {} tasks. Switch to Agent mode and click \
                             'Execute Plan' to implement it.",
                            new_plan.title,
                            new_plan.tasks.len()
                        );
                        let _ = event_tx
                            .send(CodingEvent {
                                session_id: session_id.clone(),
                                event_type: CodingEventType::Completed,
                                payload: serde_json::json!({ "answer": summary }),
                            })
                            .await;
                        self.cancel_tokens.lock().unwrap().remove(&session_id);
                        return Ok(summary);
                    }

                    // Agent mode: keep the plan in memory and continue executing tasks.
                    current_plan = Some(new_plan);
                    let obs = "Plan created successfully.";
                    conversation.push(("user".to_string(), format!("Observation: {}", obs)));
                    continue;
                }
            }

            if tool_name == "update_task" {
                if let Some(ref mut plan) = current_plan {
                    let task_index = tool_input["task_index"].as_u64().unwrap_or(0) as usize;
                    let status = tool_input["status"].as_str().unwrap_or("in_progress");
                    let note = tool_input["note"].as_str().map(str::to_string);
                    if let Some(task) = plan.tasks.get_mut(task_index) {
                        task.status = status.to_string();
                        task.note = note;
                    }
                    plan.updated_at = Utc::now().to_rfc3339();
                    self.save_plan(plan)?;
                    let plan_json = serde_json::to_value(&*plan).unwrap_or_default();
                    let _ = event_tx
                        .send(CodingEvent {
                            session_id: session_id.clone(),
                            event_type: CodingEventType::TaskUpdated,
                            payload: plan_json,
                        })
                        .await;
                    tool_calls_made += 1;
                    let obs = format!("Task {} updated to {}.", task_index, status);
                    conversation.push(("user".to_string(), format!("Observation: {}", obs)));
                    continue;
                } else {
                    // No plan in memory — inform the agent so it doesn't silently fail
                    conversation.push(("user".to_string(),
                        "Observation: update_task failed — no active plan exists for this session. \
                         If you need to track progress, use create_plan first.".to_string()
                    ));
                    continue;
                }
            }

            // Execute tool
            let obs_result = executor.execute(&tool_name, &tool_input).await;
            let (obs_text, is_error) = match obs_result {
                Ok(v) => (serde_json::to_string_pretty(&v).unwrap_or_default(), false),
                Err(e) => (e.to_string(), true),
            };

            // Update success/failure tracking.
            // On success: record the action so a direct repeat is caught next iteration.
            // On failure: count how many times the same call fails in a row; after 3
            //             consecutive identical failures the agent is genuinely stuck.
            if !is_error {
                tool_calls_made += 1;
                last_success_action = Some(current_action.clone());
                last_fail_key = None;
                fail_streak = 0;
            } else {
                if Some(&current_action) == last_fail_key.as_ref() {
                    fail_streak += 1;
                    if fail_streak >= 2 {
                        // Same call has now failed 3 times in a row — give up.
                        let _ = event_tx
                            .send(CodingEvent {
                                session_id: session_id.clone(),
                                event_type: CodingEventType::Failed,
                                payload: serde_json::json!({
                                    "reason": "repeated_failure",
                                    "tool": tool_name,
                                    "error": obs_text,
                                }),
                            })
                            .await;
                        anyhow::bail!(
                            "Agent stuck: '{}' failed {} times with the same input. Last error: {}",
                            tool_name, fail_streak + 1, obs_text
                        );
                    }
                } else {
                    last_fail_key = Some(current_action.clone());
                    fail_streak = 0;
                }
            }

            // Replace generic "Unknown tool" errors with a mode-specific correction so the
            // model understands exactly what it is allowed to do instead of inventing new tools.
            let obs_text = if is_error && obs_text.starts_with("Unknown tool:") {
                match mode.as_str() {
                    "plan" => format!(
                        "ERROR: '{}' is not a valid tool in Plan mode. \
                         Your ONLY available tools are: directory_tree, file_read, grep, create_plan. \
                         You CANNOT create, write, or execute files. \
                         Call create_plan with your tasks now, then give your Final Answer.",
                        tool_name
                    ),
                    "ask" => format!(
                        "ERROR: '{}' is not a valid tool in Ask mode. \
                         Your ONLY available tools are: directory_tree, file_read, grep. \
                         You CANNOT modify files.",
                        tool_name
                    ),
                    _ => format!(
                        "ERROR: '{}' is not a recognised tool. \
                         Check the available tools list in your instructions and use only those.",
                        tool_name
                    ),
                }
            } else {
                obs_text
            };

            let _ = event_tx
                .send(CodingEvent {
                    session_id: session_id.clone(),
                    event_type: CodingEventType::Observation,
                    payload: serde_json::json!({
                        "tool": tool_name,
                        "observation": obs_text,
                        "error": is_error,
                        "step": step_num,
                    }),
                })
                .await;

            conversation.push(("user".to_string(), format!("Observation: {}", obs_text)));
        }

        anyhow::bail!("Max iterations ({}) reached.", self.max_iterations)
    }

    fn save_plan(&self, plan: &CodingPlan) -> Result<()> {
        let tasks_json = serde_json::to_string(&plan.tasks)?;
        let db = self.db.lock().unwrap();
        db.conn.execute(
            r#"INSERT OR REPLACE INTO coding_plans
               (id, session_id, tasks_json, status, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6)"#,
            rusqlite::params![
                plan.id, plan.session_id, tasks_json, plan.status,
                plan.created_at, plan.updated_at
            ],
        )?;
        Ok(())
    }

    /// Load the most recent plan for a session from the database.
    /// Called at the start of every Agent run so that update_task works even
    /// when the plan was created in a prior Plan-mode run.
    fn load_plan(&self, session_id: &str) -> Result<Option<CodingPlan>> {
        let db = self.db.lock().unwrap();
        let result = db.conn.query_row(
            "SELECT id, session_id, tasks_json, status, created_at, updated_at
             FROM coding_plans WHERE session_id = ?1 ORDER BY created_at DESC LIMIT 1",
            rusqlite::params![session_id],
            |row| {
                let tasks_json: String = row.get(2)?;
                let tasks = serde_json::from_str(&tasks_json).unwrap_or_default();
                Ok(CodingPlan {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    title: String::new(),
                    tasks,
                    status: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        );
        match result {
            Ok(plan) => Ok(Some(plan)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

// ── Plan tool helpers ─────────────────────────────────────────────────────────

fn handle_create_plan(session_id: &str, input: &Value) -> Option<CodingPlan> {
    let title = input["title"].as_str()?.to_string();
    let tasks_raw = input["tasks"].as_array()?;
    let tasks: Vec<CodingPlanTask> = tasks_raw
        .iter()
        .enumerate()
        .map(|(i, t)| CodingPlanTask {
            id: format!("task-{}", i),
            title: t["title"].as_str().unwrap_or("Task").to_string(),
            description: t["description"].as_str().unwrap_or("").to_string(),
            status: "pending".to_string(),
            note: None,
        })
        .collect();

    let now = Utc::now().to_rfc3339();
    Some(CodingPlan {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.to_string(),
        title,
        tasks,
        status: "pending".to_string(),
        created_at: now.clone(),
        updated_at: now,
    })
}

// ── ReAct parser (same pattern as agent runtime) ──────────────────────────────

struct ReactOutput {
    thought: Option<String>,
    action_name: Option<String>,
    action_input: Option<Value>,
    final_answer: Option<String>,
}

fn parse_react_output(text: &str) -> ReactOutput {
    #[derive(PartialEq)]
    enum Section { None, Thought, ActionInput, FinalAnswer }

    let mut thought_lines: Vec<String> = vec![];
    let mut action_name: Option<String> = None;
    let mut action_input: Option<Value> = None;
    let mut action_input_lines: Vec<String> = vec![];
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
            // Start collecting — JSON may span multiple lines
            section = Section::ActionInput;
            action_input_lines.clear();
            let v = rest.trim().to_string();
            if !v.is_empty() { action_input_lines.push(v); }
        } else if let Some(rest) = trimmed.strip_prefix("Final Answer:") {
            section = Section::FinalAnswer;
            final_lines.clear();
            let v = rest.trim().to_string();
            if !v.is_empty() { final_lines.push(v); }
        } else if !trimmed.is_empty() {
            match section {
                Section::Thought => thought_lines.push(trimmed.to_string()),
                // Preserve original line (not trimmed) so JSON indentation is intact
                Section::ActionInput => action_input_lines.push(line.to_string()),
                Section::FinalAnswer => final_lines.push(trimmed.to_string()),
                Section::None => {}
            }
        }
    }

    // Parse accumulated action input (handles multi-line JSON and double-encoded strings)
    if !action_input_lines.is_empty() {
        action_input = parse_action_input(&action_input_lines.join("\n"));
    }

    ReactOutput {
        thought: if thought_lines.is_empty() { None } else { Some(thought_lines.join("\n")) },
        action_name,
        action_input,
        final_answer: if final_lines.is_empty() { None } else { Some(final_lines.join("\n")) },
    }
}

/// Parse the raw text after "Action Input:" into a JSON Value.
///
/// Handles all common failure modes from LLMs:
/// 1. Standard JSON object/array  →  direct parse
/// 2. Double-encoded JSON         →  model wrapped the object in a string with escaped quotes
/// 3. Literal newlines in values  →  model emitted bare newline bytes inside a JSON string
///    (e.g. `\n` single-escape in the outer string becomes 0x0A inside the inner value,
///    making the inner value invalid JSON).  We repair by escaping them before re-parsing.
/// 4. Combination of 2 + 3
fn parse_action_input(raw: &str) -> Option<Value> {
    let raw = raw.trim();

    // Attempt 1: direct parse.
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::String(inner)) => {
            // Double-encoded: the outer JSON is just a quoted wrapper.
            // Try the inner string as-is first.
            if let Ok(v) = serde_json::from_str::<Value>(&inner) {
                return Some(v);
            }
            // Inner string may have bare newline bytes — repair and retry.
            let fixed = escape_literal_newlines_in_json(&inner);
            if let Ok(v) = serde_json::from_str::<Value>(&fixed) {
                return Some(v);
            }
            return Some(Value::String(inner));
        }
        Ok(v) => return Some(v),
        Err(_) => {}
    }

    // Attempt 2: the raw JSON itself has bare newlines inside string values
    // (e.g. multi-line JSON with unescaped newlines in a string literal).
    let fixed = escape_literal_newlines_in_json(raw);
    match serde_json::from_str::<Value>(&fixed) {
        Ok(Value::String(inner)) => {
            // Still double-encoded after the newline fix
            if let Ok(v) = serde_json::from_str::<Value>(&inner) {
                return Some(v);
            }
            return Some(Value::String(inner));
        }
        Ok(v) => return Some(v),
        Err(_) => {}
    }

    // Last resort: return as raw string value so callers see a useful error.
    Some(Value::String(raw.to_string()))
}

/// Escape bare newline / carriage-return / tab characters that appear **inside**
/// JSON string literals.  This repairs malformed JSON produced by LLMs that emit
/// actual newline bytes inside a string instead of the `\n` escape sequence.
///
/// The scan is escape-sequence-aware: `\"` does not toggle the in-string state,
/// and `\\` is consumed as a two-character unit so the next char is not mis-read.
fn escape_literal_newlines_in_json(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 32);
    let mut in_string = false;
    let mut chars = s.chars();

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                // Escape sequence — consume the next char verbatim so `\"` is
                // never treated as a string boundary.
                result.push(ch);
                if let Some(next) = chars.next() {
                    result.push(next);
                }
            }
            '"' => {
                in_string = !in_string;
                result.push(ch);
            }
            '\n' if in_string => result.push_str("\\n"),
            '\r' if in_string => result.push_str("\\r"),
            '\t' if in_string => result.push_str("\\t"),
            _ => result.push(ch),
        }
    }
    result
}

fn strip_thinking(text: &str) -> String {
    let thinking_prefix = crate::engine::remote::THINKING_PREFIX;
    let step1: String = text
        .lines()
        .filter(|l| !l.starts_with(thinking_prefix))
        .collect::<Vec<_>>()
        .join("\n");

    let mut result = String::with_capacity(step1.len());
    let mut remaining = step1.as_str();
    loop {
        if let Some(start) = remaining.find("<think>") {
            result.push_str(&remaining[..start]);
            let after = &remaining[start + "<think>".len()..];
            if let Some(end) = after.find("</think>") {
                remaining = &after[end + "</think>".len()..];
            } else {
                break;
            }
        } else {
            result.push_str(remaining);
            break;
        }
    }
    // Remove any orphan </think> tags the model may have output without a matching opener
    result.replace("</think>", "").trim().to_string()
}
