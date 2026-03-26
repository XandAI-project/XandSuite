/// Agentic tool-calling loop.
///
/// Algorithm:
///   1. Build the chat request with tool definitions attached.
///   2. Send to llama-server using streaming tool detection so the user sees
///      output immediately while tool calls are detected from the stream.
///   3. If the stream ends with tool_calls, dispatch each one via SkillsManager,
///      append the results as `tool` role messages, go to step 2.
///   4. When no tool_calls are returned the final answer was already streamed;
///      just send `[DONE]` on the channel.
///
/// All intermediate tool-call/result steps are emitted as Tauri events so the
/// frontend can show them as expandable cards in the chat.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use super::manager::SkillsManager;
use crate::db::AppDb;
use crate::engine::remote::RemoteEngine;
use crate::models::InferenceConfig;

const MAX_TOOL_TURNS: usize = 8;
const CODE_RUNNER_SERVER_ID: &str = "code_runner";

// ── OpenAI tool schema types (for request serialisation) ──────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: String, // always "function"
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: Value, // JSON Schema object
}

// ── OpenAI tool_calls response types ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String, // JSON string
}

// ── SkillsExecutor ────────────────────────────────────────────────────────

pub struct SkillsExecutor {
    pub skills: Arc<SkillsManager>,
    /// When set, the code_runner built-in tools are injected and dispatched natively.
    code_runner_db: Option<Arc<Mutex<AppDb>>>,
    code_runner_conv_id: Option<String>,
}

impl SkillsExecutor {
    pub fn new(skills: Arc<SkillsManager>) -> Self {
        Self {
            skills,
            code_runner_db: None,
            code_runner_conv_id: None,
        }
    }

    /// Enable the built-in code execution tools for this invocation.
    pub fn with_code_runner(
        mut self,
        db: Arc<Mutex<AppDb>>,
        conversation_id: String,
    ) -> Self {
        self.code_runner_db = Some(db);
        self.code_runner_conv_id = Some(conversation_id);
        self
    }

    /// Build OpenAI-compatible tool definitions from all connected MCP servers,
    /// plus the built-in code_runner tools when enabled.
    pub async fn build_tool_definitions(&self) -> Vec<ToolDefinition> {
        let mut tools: Vec<ToolDefinition> = self
            .skills
            .all_tools()
            .await
            .into_iter()
            .map(|tagged| ToolDefinition {
                kind: "function".to_string(),
                function: FunctionDef {
                    name: format!("{}__{}", tagged.server_id, tagged.tool.name),
                    description: tagged.tool.description.unwrap_or_default(),
                    parameters: tagged.tool.input_schema,
                },
            })
            .collect();

        if self.code_runner_db.is_some() {
            tools.push(code_runner_execute_tool());
            tools.push(code_runner_list_artifacts_tool());
        }

        tools
    }

    /// Run the agentic loop.
    ///
    /// Uses `chat_stream_with_tools_detection` so that every turn streams
    /// content tokens to the frontend in real-time.  Tool calls detected in
    /// the stream are dispatched, results appended, and the loop continues.
    pub async fn run(
        &self,
        mut messages: Vec<(String, String)>,
        config: &InferenceConfig,
        engine: &RemoteEngine,
        app: &AppHandle,
        conv_id: &str,
        token_tx: mpsc::Sender<String>,
    ) -> Result<()> {
        let tools = self.build_tool_definitions().await;

        if tools.is_empty() {
            // No tools registered — fall back to plain streaming chat.
            return engine.chat_stream(messages, config, token_tx).await;
        }

        // Helper: emit app_log event from the executor
        let log_event = |level: &str, msg: String| {
            let _ = app.emit("app_log", json!({
                "level": level,
                "message": msg,
                "ts": chrono::Utc::now().to_rfc3339(),
            }));
        };

        for turn in 0..MAX_TOOL_TURNS {
            log_event("info", format!("[executor] Turn {} — sending request to LLM", turn));

            // ── Streaming call: pipes content to token_tx, detects tool calls ─
            let result = engine
                .chat_stream_with_tools_detection(&messages, config, &tools, &token_tx)
                .await?;

            // Deserialise the assembled tool calls (empty Vec when none)
            let tool_calls: Vec<ToolCall> = serde_json::from_value(result.tool_calls_raw.clone())
                .unwrap_or_default();

            if tool_calls.is_empty() {
                // No tool calls — the final answer was already streamed live.
                log_event("info", format!(
                    "[executor] Turn {} — no tool calls detected, streaming complete (finish_reason={})",
                    turn, result.finish_reason
                ));
                let _ = token_tx.send("[DONE]".to_string()).await;
                return Ok(());
            }

            log_event("info", format!(
                "[executor] Turn {} — {} tool call(s) detected", turn, tool_calls.len()
            ));

            // ── Tool calls detected — reconstruct the assistant history message ─
            //
            // We store the assistant message in the same JSON shape as the old
            // non-streaming path so that `build_messages` in remote.rs can
            // correctly reconstruct it on the next turn.
            let assistant_history_msg = json!({
                "role": "assistant",
                "content": if result.content.is_empty() {
                    Value::Null
                } else {
                    Value::String(result.content.clone())
                },
                "tool_calls": result.tool_calls_raw,
            });
            messages.push((
                "assistant".to_string(),
                serde_json::to_string(&assistant_history_msg).unwrap_or_default(),
            ));

            // ── Execute each tool call ─────────────────────────────────────
            for tc in &tool_calls {
                let fn_name = &tc.function.name;
                let args: Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(json!({}));

                let args_preview = serde_json::to_string(&args)
                    .unwrap_or_default()
                    .chars()
                    .take(120)
                    .collect::<String>();
                log_event("info", format!(
                    "[executor] Dispatching tool '{}' (id={}) args={}", fn_name, tc.id, args_preview
                ));

                // Emit tool-call event to frontend
                let _ = app.emit(
                    "chat_tool_call",
                    json!({
                        "conversation_id": conv_id,
                        "tool_call_id": tc.id,
                        "function_name": fn_name,
                        "arguments": args,
                        "turn": turn,
                    }),
                );

                let result_text = self
                    .dispatch_tool_call(fn_name, args)
                    .await
                    .unwrap_or_else(|e| {
                        let err_msg = format!("Tool '{}' error: {}", fn_name, e);
                        log::error!("{}", err_msg);
                        let _ = app.emit("app_log", json!({
                            "level": "error",
                            "message": err_msg,
                            "ts": chrono::Utc::now().to_rfc3339(),
                        }));
                        json!({"error": e.to_string()}).to_string()
                    });

                let result_preview: String = result_text.chars().take(100).collect();
                log_event("info", format!(
                    "[executor] Tool '{}' result ({} chars): {}", fn_name, result_text.len(), result_preview
                ));

                // Emit result event
                let _ = app.emit(
                    "chat_tool_result",
                    json!({
                        "conversation_id": conv_id,
                        "tool_call_id": tc.id,
                        "function_name": fn_name,
                        "result": result_text,
                        "turn": turn,
                    }),
                );

                // Append tool result as a "tool" role message
                messages.push((
                    format!("tool::{}", tc.id),
                    result_text,
                ));
            }
            // Continue the loop for the next model turn
        }

        // Exceeded max turns — inform the user
        log_event("warn", format!("[executor] Exceeded maximum tool-call turns ({})", MAX_TOOL_TURNS));
        let _ = token_tx
            .send("[Max tool-call turns exceeded]".to_string())
            .await;
        let _ = token_tx.send("[DONE]".to_string()).await;
        Ok(())
    }

    /// Route a qualified tool name (`server_id__tool_name`) to the right MCP server,
    /// or to the built-in code_runner when the prefix matches.
    async fn dispatch_tool_call(&self, qualified_name: &str, arguments: Value) -> Result<String> {
        // ── Built-in code_runner dispatch ─────────────────────────────────
        if let Some(tool_name) = qualified_name.strip_prefix(&format!("{}__", CODE_RUNNER_SERVER_ID)) {
            return self.dispatch_code_runner(tool_name, arguments).await;
        }

        // ── MCP server dispatch ───────────────────────────────────────────
        let (server_id, tool_name) = if let Some(pos) = qualified_name.find("__") {
            (&qualified_name[..pos], &qualified_name[pos + 2..])
        } else {
            // Unqualified — try to find the server
            let sid = self
                .skills
                .find_server_for_tool(qualified_name)
                .await
                .with_context(|| format!("No MCP server found for tool '{}'", qualified_name))?;
            return Box::pin(self.dispatch_tool_call(
                &format!("{}__{}", sid, qualified_name),
                arguments,
            ))
            .await;
        };

        let result = self
            .skills
            .call_tool(server_id, tool_name, arguments)
            .await?;

        if result.is_error {
            anyhow::bail!("Tool error: {}", result.text());
        }
        Ok(result.text())
    }

    /// Dispatch a built-in code_runner tool call.
    async fn dispatch_code_runner(&self, tool_name: &str, arguments: Value) -> Result<String> {
        match tool_name {
            "execute_code" => {
                let language = arguments
                    .get("language")
                    .and_then(|v| v.as_str())
                    .unwrap_or("python");
                let code = arguments
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                log::info!("[executor] Executing {} code ({} chars)", language, code.len());
                let run = crate::code_runner::execute_code(language, code).await?;
                log::info!(
                    "[executor] Code execution complete: exit_code={} stdout_len={} stderr_len={}",
                    run.exit_code, run.stdout.len(), run.stderr.len()
                );
                Ok(serde_json::to_string(&run).unwrap_or_default())
            }
            "list_recent_artifacts" => {
                let limit = arguments
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5) as usize;

                let db = self
                    .code_runner_db
                    .as_ref()
                    .context("code_runner_db not initialised")?;
                let conv_id = self
                    .code_runner_conv_id
                    .as_deref()
                    .context("code_runner_conv_id not initialised")?;

                let artifacts =
                    crate::code_runner::list_recent_artifacts(db, conv_id, limit)?;
                Ok(serde_json::to_string(&artifacts).unwrap_or_default())
            }
            other => anyhow::bail!("Unknown code_runner tool: '{}'", other),
        }
    }
}

// ── Built-in tool schema definitions ─────────────────────────────────────────

fn code_runner_execute_tool() -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: FunctionDef {
            name: format!("{}__execute_code", CODE_RUNNER_SERVER_ID),
            description: "Run code in a real sandboxed subprocess and return stdout, stderr, \
                           exit code, and wall-clock time. \
                           ALWAYS call this tool when the user asks you to run, execute, test, \
                           or verify code — you are capable of running code in this environment. \
                           Call it immediately after writing a code artifact, or whenever the \
                           user wants to see actual output. \
                           Supported languages: python, javascript, shell."
                .to_string(),
            parameters: json!({
                "type": "object",
                "required": ["language", "code"],
                "properties": {
                    "language": {
                        "type": "string",
                        "enum": ["python", "javascript", "shell"],
                        "description": "The programming language to execute."
                    },
                    "code": {
                        "type": "string",
                        "description": "The source code to run. Write it exactly as you would in a file."
                    }
                }
            }),
        },
    }
}

fn code_runner_list_artifacts_tool() -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: FunctionDef {
            name: format!("{}__list_recent_artifacts", CODE_RUNNER_SERVER_ID),
            description: "List recent artifacts (code, markdown, html, text) created in this \
                           conversation. Returns title, type, language, and a content preview. \
                           Use this to find and re-run code from earlier in the conversation."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "number",
                        "description": "Maximum number of artifacts to return (default 5, max 20)."
                    }
                }
            }),
        },
    }
}
