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
use rusqlite::params as sql_params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
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
    /// When set, the comfyui__generate_image tool is injected and dispatched natively.
    comfyui_url: Option<String>,
    comfyui_model: Option<String>,
    /// `"checkpoint"` | `"unet"` | None (auto-detect)
    comfyui_model_type: Option<String>,
    /// CLIP model override for UNETLoader workflow
    comfyui_clip_name: Option<String>,
    /// VAE model override for UNETLoader workflow
    comfyui_vae_name: Option<String>,
    /// Named custom workflows: (id, name, workflow_json)
    comfyui_workflows: Vec<(String, String, String)>,
}

impl SkillsExecutor {
    pub fn new(skills: Arc<SkillsManager>) -> Self {
        Self {
            skills,
            code_runner_db: None,
            code_runner_conv_id: None,
            comfyui_url: None,
            comfyui_model: None,
            comfyui_model_type: None,
            comfyui_clip_name: None,
            comfyui_vae_name: None,
            comfyui_workflows: vec![],
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

    /// Enable the ComfyUI image generation tool for this invocation.
    pub fn with_comfyui(mut self, url: String, model: Option<String>) -> Self {
        self.comfyui_url = Some(url);
        self.comfyui_model = model;
        self
    }

    /// Set the ComfyUI model type and optional CLIP/VAE overrides.
    pub fn with_comfyui_model_type(
        mut self,
        model_type: Option<String>,
        clip_name: Option<String>,
        vae_name: Option<String>,
    ) -> Self {
        self.comfyui_model_type = model_type;
        self.comfyui_clip_name = clip_name;
        self.comfyui_vae_name = vae_name;
        self
    }

    /// Provide named custom workflows for the ComfyUI tool.
    pub fn with_comfyui_workflows(mut self, workflows: Vec<(String, String, String)>) -> Self {
        self.comfyui_workflows = workflows;
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

        if self.comfyui_url.is_some() {
            let workflow_names: Vec<String> = std::iter::once("Default".to_string())
                .chain(self.comfyui_workflows.iter().map(|(_, name, _)| name.clone()))
                .collect();
            tools.push(comfyui_generate_image_tool(workflow_names));
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
            let result = match engine
                .chat_stream_with_tools_detection(&messages, config, &tools, &token_tx)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let err_str = e.to_string();
                    // The LLM server returned a 500 because it tried to embed huge/complex
                    // content (e.g. an entire HTML page) inside a tool-call JSON argument,
                    // producing malformed JSON. Recover: inject a corrective message and
                    // retry once in plain-streaming mode (no tools) so the LLM can output
                    // the content directly as an artifact tag.
                    if err_str.contains("Failed to parse tool call arguments")
                        || (err_str.contains("500") && err_str.contains("parse"))
                    {
                        log_event("warn", format!(
                            "[executor] Turn {} — LLM tool-call JSON parse error; recovering without tools. err={}",
                            turn, &err_str[..err_str.len().min(200)]
                        ));
                        messages.push((
                            "user".to_string(),
                            "Your previous response could not be sent because you tried to pass \
                             large content (such as HTML or CSS) as a tool-call argument, \
                             which breaks JSON serialisation. \
                             STOP using execute_code for HTML/CSS/web content entirely. \
                             Output the content DIRECTLY in an artifact tag right now — \
                             for example: <artifact type=\"html\" title=\"Page Title\">...HTML here...</artifact>. \
                             Do NOT call any tool. Write the artifact tag immediately."
                                .to_string(),
                        ));
                        engine.chat_stream(messages, config, token_tx.clone()).await?;
                        let _ = token_tx.send("[DONE]".to_string()).await;
                        return Ok(());
                    }
                    return Err(e);
                }
            };

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
                    .dispatch_tool_call(fn_name, args, app, conv_id)
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
    /// or to the built-in code_runner / comfyui when the prefix matches.
    async fn dispatch_tool_call(
        &self,
        qualified_name: &str,
        arguments: Value,
        app: &tauri::AppHandle,
        conv_id: &str,
    ) -> Result<String> {
        // ── Built-in code_runner dispatch ─────────────────────────────────
        if let Some(tool_name) = qualified_name.strip_prefix(&format!("{}__", CODE_RUNNER_SERVER_ID)) {
            return self.dispatch_code_runner(tool_name, arguments).await;
        }

        // ── Built-in ComfyUI dispatch ──────────────────────────────────────
        if let Some(tool_name) = qualified_name.strip_prefix("comfyui__") {
            return self.dispatch_comfyui(tool_name, arguments, app, conv_id).await;
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
                app,
                conv_id,
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

                // HTML/CSS cannot be executed — intercept and hand the content
                // back to the LLM so it can wrap it in an artifact tag.
                if matches!(language, "html" | "htm" | "css") {
                    let instruction = format!(
                        "STOP — HTML/CSS cannot be executed in a terminal and must NOT be \
                         passed to execute_code again.\n\
                         You MUST output it as an artifact in your next response:\n\
                         <artifact type=\"html\" title=\"Page Title\">\n{}\n</artifact>\n\
                         Do NOT call any tool. Just write the artifact tag with the content above.",
                        code
                    );
                    return Ok(serde_json::to_string(&json!({
                        "error": "HTML_NOT_EXECUTABLE",
                        "instruction": instruction,
                    })).unwrap_or_default());
                }

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

    /// Dispatch a built-in ComfyUI image generation tool call.
    async fn dispatch_comfyui(
        &self,
        tool_name: &str,
        arguments: Value,
        app: &tauri::AppHandle,
        conv_id: &str,
    ) -> Result<String> {
        match tool_name {
            "generate_image" => {
                let base_url = self
                    .comfyui_url
                    .as_deref()
                    .context("comfyui_url not set")?
                    .trim_end_matches('/')
                    .to_string();

                let prompt = arguments
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("a beautiful landscape");
                let negative = arguments
                    .get("negative_prompt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("blurry, low quality, watermark");
                let width = arguments
                    .get("width")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(512) as u32;
                let height = arguments
                    .get("height")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(512) as u32;
                let steps = arguments
                    .get("steps")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(20) as u32;
                let seed = arguments
                    .get("seed")
                    .and_then(|v| v.as_i64())
                    .unwrap_or_else(|| rand_seed());

                let http = reqwest::Client::new();

                // Resolve which model to use and whether it lives in checkpoints/ or
                // diffusion_models/ (UNETLoader).
                let (model, is_unet) = resolve_model_and_type(
                    &http,
                    &base_url,
                    self.comfyui_model.as_deref(),
                    self.comfyui_model_type.as_deref(),
                )
                .await?;

                log::info!(
                    "[comfyui] Using {} model: {}",
                    if is_unet { "unet/diffusion" } else { "checkpoint" },
                    model
                );

                let client_id = uuid::Uuid::new_v4().to_string();

                // Check whether the LLM requested a named custom workflow.
                let workflow_name = arguments
                    .get("workflow")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Default");

                let workflow = if workflow_name != "Default" {
                    // Look up the custom workflow by name (case-insensitive).
                    let found = self.comfyui_workflows.iter().find(|(_, name, _)| {
                        name.to_lowercase() == workflow_name.to_lowercase()
                    });

                    if let Some((_, _, wf_json)) = found {
                        let mut wf: Value = serde_json::from_str(wf_json)
                            .context("Failed to parse saved workflow JSON")?;

                        substitute_placeholders(&mut wf, prompt, negative, width, height, steps, seed);

                        json!({ "client_id": client_id, "prompt": wf })
                    } else {
                        anyhow::bail!(
                            "No workflow named '{}' found. Available: {}",
                            workflow_name,
                            self.comfyui_workflows
                                .iter()
                                .map(|(_, n, _)| n.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                } else if is_unet {
                    // UNETLoader workflow: requires a separately loaded CLIP and VAE.
                    let (clip_name, vae_name) = resolve_clip_and_vae(
                        &http,
                        &base_url,
                        self.comfyui_clip_name.as_deref(),
                        self.comfyui_vae_name.as_deref(),
                    )
                    .await?;

                    log::info!(
                        "[comfyui] UNETLoader workflow — CLIP: {}, VAE: {}",
                        clip_name,
                        vae_name
                    );

                    json!({
                        "client_id": client_id,
                        "prompt": {
                            "1": {
                                "class_type": "UNETLoader",
                                "inputs": { "unet_name": model, "weight_dtype": "default" }
                            },
                            "2": {
                                "class_type": "CLIPLoader",
                                "inputs": { "clip_name": clip_name, "type": "stable_diffusion" }
                            },
                            "3": {
                                "class_type": "VAELoader",
                                "inputs": { "vae_name": vae_name }
                            },
                            "4": {
                                "class_type": "CLIPTextEncode",
                                "inputs": { "clip": ["2", 0], "text": prompt }
                            },
                            "5": {
                                "class_type": "CLIPTextEncode",
                                "inputs": { "clip": ["2", 0], "text": negative }
                            },
                            "6": {
                                "class_type": "EmptyLatentImage",
                                "inputs": { "batch_size": 1, "height": height, "width": width }
                            },
                            "7": {
                                "class_type": "KSampler",
                                "inputs": {
                                    "cfg": 7,
                                    "denoise": 1,
                                    "latent_image": ["6", 0],
                                    "model": ["1", 0],
                                    "negative": ["5", 0],
                                    "positive": ["4", 0],
                                    "sampler_name": "euler",
                                    "scheduler": "normal",
                                    "seed": seed,
                                    "steps": steps
                                }
                            },
                            "8": {
                                "class_type": "VAEDecode",
                                "inputs": { "samples": ["7", 0], "vae": ["3", 0] }
                            },
                            "9": {
                                "class_type": "SaveImage",
                                "inputs": { "filename_prefix": "XandSuite", "images": ["8", 0] }
                            }
                        }
                    })
                } else {
                    // Standard CheckpointLoaderSimple txt2img workflow.
                    json!({
                        "client_id": client_id,
                        "prompt": {
                            "1": {
                                "class_type": "CheckpointLoaderSimple",
                                "inputs": { "ckpt_name": model }
                            },
                            "2": {
                                "class_type": "CLIPTextEncode",
                                "inputs": { "clip": ["1", 1], "text": prompt }
                            },
                            "3": {
                                "class_type": "CLIPTextEncode",
                                "inputs": { "clip": ["1", 1], "text": negative }
                            },
                            "4": {
                                "class_type": "EmptyLatentImage",
                                "inputs": { "batch_size": 1, "height": height, "width": width }
                            },
                            "5": {
                                "class_type": "KSampler",
                                "inputs": {
                                    "cfg": 7,
                                    "denoise": 1,
                                    "latent_image": ["4", 0],
                                    "model": ["1", 0],
                                    "negative": ["3", 0],
                                    "positive": ["2", 0],
                                    "sampler_name": "euler",
                                    "scheduler": "normal",
                                    "seed": seed,
                                    "steps": steps
                                }
                            },
                            "6": {
                                "class_type": "VAEDecode",
                                "inputs": { "samples": ["5", 0], "vae": ["1", 2] }
                            },
                            "7": {
                                "class_type": "SaveImage",
                                "inputs": { "filename_prefix": "XandSuite", "images": ["6", 0] }
                            }
                        }
                    })
                };

                // Submit the prompt
                let queue_resp = http
                    .post(format!("{}/prompt", base_url))
                    .json(&workflow)
                    .send()
                    .await
                    .context("Failed to connect to ComfyUI — is it running?")?;

                if !queue_resp.status().is_success() {
                    let status = queue_resp.status();
                    let body = queue_resp.text().await.unwrap_or_default();
                    anyhow::bail!("ComfyUI returned {}: {}", status, body);
                }

                let queue_json: Value = queue_resp.json().await.context("Invalid JSON from /prompt")?;
                let prompt_id = queue_json["prompt_id"]
                    .as_str()
                    .context("No prompt_id in ComfyUI response")?
                    .to_string();

                log::info!("[comfyui] Queued prompt_id={}", prompt_id);

                // Poll /history/{prompt_id} until outputs are ready (timeout 120s)
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
                let image_info = loop {
                    if std::time::Instant::now() > deadline {
                        anyhow::bail!("ComfyUI timed out after 120 s — check the ComfyUI console for errors.");
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                    let hist: Value = http
                        .get(format!("{}/history/{}", base_url, prompt_id))
                        .send()
                        .await
                        .context("Failed to poll ComfyUI history")?
                        .json()
                        .await
                        .context("Invalid JSON from /history")?;

                    // The response is a map keyed by prompt_id.
                    if let Some(entry) = hist.get(&prompt_id) {
                        // Check for error status.
                        if let Some(status_str) = entry
                            .get("status")
                            .and_then(|s| s.get("status_str"))
                            .and_then(|s| s.as_str())
                        {
                            if status_str == "error" {
                                let msgs = entry["status"]["messages"]
                                    .as_array()
                                    .map(|a| {
                                        a.iter()
                                            .filter_map(|m| m.as_str())
                                            .collect::<Vec<_>>()
                                            .join("; ")
                                    })
                                    .unwrap_or_default();
                                anyhow::bail!("ComfyUI reported an error: {}", msgs);
                            }
                        }

                        // Scan ALL output nodes for any that contain an "images" array.
                        // This correctly handles both built-in workflows and arbitrary
                        // custom workflows regardless of which node ID SaveImage sits on.
                        if let Some(outputs) = entry.get("outputs").and_then(|o| o.as_object()) {
                            let mut found: Option<(String, String, String)> = None;
                            for (_node_id, node_output) in outputs {
                                if let Some(images) =
                                    node_output.get("images").and_then(|i| i.as_array())
                                {
                                    if let Some(img) = images.first() {
                                        let filename =
                                            img["filename"].as_str().unwrap_or("").to_string();
                                        let subfolder =
                                            img["subfolder"].as_str().unwrap_or("").to_string();
                                        let img_type =
                                            img["type"].as_str().unwrap_or("output").to_string();
                                        found = Some((filename, subfolder, img_type));
                                        break;
                                    }
                                }
                            }
                            if let Some(info) = found {
                                break info;
                            }
                        }
                    }
                };

                let (filename, subfolder, img_type) = image_info;
                let image_url = format!(
                    "{}/view?filename={}&subfolder={}&type={}",
                    base_url,
                    urlencoding::encode(&filename),
                    urlencoding::encode(&subfolder),
                    urlencoding::encode(&img_type),
                );

                log::info!("[comfyui] Image ready: {}", image_url);

                // Download image bytes and persist to the gallery so the image
                // survives ComfyUI restarts and is visible across sessions.
                let mut saved_gallery_id: Option<String> = None;

                if let Ok(img_resp) = http.get(&image_url).send().await {
                    if let Ok(img_bytes) = img_resp.bytes().await {
                        use base64::Engine as _;
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&img_bytes);
                        let gid = uuid::Uuid::new_v4().to_string();
                        let now = chrono::Utc::now().to_rfc3339();
                        let state = app.state::<crate::state::AppState>();
                        {
                            let db = state.db.lock().unwrap();
                            match db.conn.execute(
                                "INSERT INTO gallery_images \
                                 (id, conversation_id, source, filename, image_data, mime_type, \
                                  prompt, width, height, created_at) \
                                 VALUES (?1, ?2, 'generated', ?3, ?4, 'image/png', ?5, ?6, ?7, ?8)",
                                sql_params![
                                    gid,
                                    conv_id,
                                    filename,
                                    b64,
                                    prompt,
                                    width as i64,
                                    height as i64,
                                    now
                                ],
                            ) {
                                Ok(_) => {
                                    log::info!(
                                        "[comfyui] Gallery image saved: id={} conv={}",
                                        gid,
                                        conv_id
                                    );
                                    saved_gallery_id = Some(gid);
                                }
                                Err(e) => {
                                    log::error!(
                                        "[comfyui] Failed to save gallery image: {} (conv_id={}, filename={})",
                                        e,
                                        conv_id,
                                        filename
                                    );
                                }
                            }
                        }
                        if saved_gallery_id.is_some() {
                            let _ = app.emit(
                                "gallery_updated",
                                serde_json::json!({ "conversation_id": conv_id }),
                            );
                        }
                    }
                } else {
                    log::warn!("[comfyui] Could not download image from {} for gallery save", image_url);
                }

                Ok(serde_json::to_string(&json!({
                    "status": "generated",
                    "image_url": image_url,
                    "gallery_id": saved_gallery_id,
                    "filename": filename,
                    "width": width,
                    "height": height,
                    "prompt": prompt,
                }))
                .unwrap_or_default())
            }
            other => anyhow::bail!("Unknown comfyui tool: '{}'", other),
        }
    }
}

/// Resolve which ComfyUI model to use and whether it is a `UNETLoader` (diffusion_models/)
/// model or a `CheckpointLoaderSimple` (checkpoints/) model.
///
/// Resolution order:
/// 1. If the caller provided an explicit `model_type` ("checkpoint" or "unet"), trust it.
/// 2. If a `model_name` is given but no type, ask `/object_info` for both loaders and check
///    which list contains the name (UNETLoader checked first).
/// 3. If neither is given, auto-detect by trying CheckpointLoaderSimple first and falling
///    back to UNETLoader if the checkpoint list is empty.
async fn resolve_model_and_type(
    http: &reqwest::Client,
    base_url: &str,
    model_name: Option<&str>,
    model_type: Option<&str>,
) -> Result<(String, bool)> {
    // Helper: pull the first entry from a node's combo input via /object_info.
    let first_from_node = |node_class: &str, input_key: &str, info: &Value| -> Option<String> {
        info.get(node_class)
            .and_then(|n| n.get("input"))
            .and_then(|i| i.get("required"))
            .and_then(|r| r.get(input_key))
            .and_then(|v| v.get(0))      // first element of the [list, config] pair
            .and_then(|arr| arr.get(0))  // first model name in the list
            .and_then(|m| m.as_str())
            .map(|s| s.to_string())
    };

    // Helper: check whether a name appears in a node's combo list.
    let contains_name = |node_class: &str, input_key: &str, name: &str, info: &Value| -> bool {
        info.get(node_class)
            .and_then(|n| n.get("input"))
            .and_then(|i| i.get("required"))
            .and_then(|r| r.get(input_key))
            .and_then(|v| v.get(0))
            .and_then(|arr| arr.as_array())
            .map(|list| list.iter().any(|m| m.as_str() == Some(name)))
            .unwrap_or(false)
    };

    match (model_name.filter(|m| !m.trim().is_empty()), model_type) {
        // Explicit model + explicit type — trust the caller.
        (Some(name), Some("unet")) => Ok((name.to_string(), true)),
        (Some(name), Some("checkpoint")) => Ok((name.to_string(), false)),

        // Model name given but no type — detect by looking it up in both lists.
        (Some(name), _) => {
            let info: Value = http
                .get(format!("{}/object_info", base_url))
                .send()
                .await
                .context("Failed to query ComfyUI /object_info")?
                .json()
                .await
                .context("Invalid JSON from /object_info")?;

            if contains_name("UNETLoader", "unet_name", name, &info) {
                Ok((name.to_string(), true))
            } else if contains_name("CheckpointLoaderSimple", "ckpt_name", name, &info) {
                Ok((name.to_string(), false))
            } else {
                // Name not found in either list — assume checkpoint for backward compat.
                log::warn!(
                    "[comfyui] Model '{}' not found in CheckpointLoaderSimple or UNETLoader lists; \
                     assuming checkpoint type.",
                    name
                );
                Ok((name.to_string(), false))
            }
        }

        // No model set and explicit type "unet".
        (None, Some("unet")) => {
            let info: Value = http
                .get(format!("{}/object_info/UNETLoader", base_url))
                .send()
                .await
                .context("Failed to query ComfyUI /object_info/UNETLoader")?
                .json()
                .await
                .context("Invalid JSON from /object_info/UNETLoader")?;

            let first = first_from_node("UNETLoader", "unet_name", &info)
                .context("No diffusion models found in ComfyUI models/diffusion_models/. \
                          Please add a model and set it in Settings → Image Generation.")?;
            Ok((first, true))
        }

        // No model set and explicit type "checkpoint" (or any unrecognised type) — existing
        // behaviour: use the first checkpoint.
        (None, Some(_)) => {
            let info: Value = http
                .get(format!("{}/object_info/CheckpointLoaderSimple", base_url))
                .send()
                .await
                .context("Failed to query ComfyUI /object_info/CheckpointLoaderSimple")?
                .json()
                .await
                .context("Invalid JSON from /object_info/CheckpointLoaderSimple")?;

            let first = first_from_node("CheckpointLoaderSimple", "ckpt_name", &info)
                .context("No checkpoints found in ComfyUI models/checkpoints/. \
                          Please add a model and set it in Settings → Image Generation.")?;
            Ok((first, false))
        }

        // Full auto-detect: try checkpoints first, then diffusion_models/.
        (None, None) => {
            let info: Value = http
                .get(format!("{}/object_info", base_url))
                .send()
                .await
                .context("Failed to query ComfyUI /object_info")?
                .json()
                .await
                .context("Invalid JSON from /object_info")?;

            if let Some(name) = first_from_node("CheckpointLoaderSimple", "ckpt_name", &info) {
                return Ok((name, false));
            }
            if let Some(name) = first_from_node("UNETLoader", "unet_name", &info) {
                return Ok((name, true));
            }
            anyhow::bail!(
                "No models found in ComfyUI (checked checkpoints/ and diffusion_models/). \
                 Please download a model and configure it in Settings → Image Generation."
            )
        }
    }
}

/// Resolve CLIP and VAE model names for a UNETLoader workflow.
///
/// Uses the user-supplied overrides when set; otherwise picks the first available
/// model from each loader's `/object_info` list.
async fn resolve_clip_and_vae(
    http: &reqwest::Client,
    base_url: &str,
    clip_override: Option<&str>,
    vae_override: Option<&str>,
) -> Result<(String, String)> {
    let clip_name = match clip_override.filter(|s| !s.trim().is_empty()) {
        Some(name) => name.to_string(),
        None => {
            let info: Value = http
                .get(format!("{}/object_info/CLIPLoader", base_url))
                .send()
                .await
                .context("Failed to query ComfyUI /object_info/CLIPLoader")?
                .json()
                .await
                .context("Invalid JSON from /object_info/CLIPLoader")?;

            info.get("CLIPLoader")
                .and_then(|n| n.get("input"))
                .and_then(|i| i.get("required"))
                .and_then(|r| r.get("clip_name"))
                .and_then(|v| v.get(0))
                .and_then(|arr| arr.get(0))
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
                .context(
                    "No CLIP models found in ComfyUI models/clip/. \
                     Please add a CLIP model or set one in Settings → Image Generation.",
                )?
        }
    };

    let vae_name = match vae_override.filter(|s| !s.trim().is_empty()) {
        Some(name) => name.to_string(),
        None => {
            let info: Value = http
                .get(format!("{}/object_info/VAELoader", base_url))
                .send()
                .await
                .context("Failed to query ComfyUI /object_info/VAELoader")?
                .json()
                .await
                .context("Invalid JSON from /object_info/VAELoader")?;

            info.get("VAELoader")
                .and_then(|n| n.get("input"))
                .and_then(|i| i.get("required"))
                .and_then(|r| r.get("vae_name"))
                .and_then(|v| v.get(0))
                .and_then(|arr| arr.get(0))
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
                .context(
                    "No VAE models found in ComfyUI models/vae/. \
                     Please add a VAE model or set one in Settings → Image Generation.",
                )?
        }
    };

    Ok((clip_name, vae_name))
}

/// Recursively walk `value` and replace placeholder strings in-place.
///
/// Supported placeholders (used as string values inside the workflow JSON):
///   `__POSITIVE_PROMPT__`, `__NEGATIVE_PROMPT__`, `__WIDTH__`, `__HEIGHT__`,
///   `__STEPS__`, `__SEED__`
fn substitute_placeholders(
    value: &mut Value,
    prompt: &str,
    negative: &str,
    width: u32,
    height: u32,
    steps: u32,
    seed: i64,
) {
    match value {
        Value::String(s) => {
            if s == "__POSITIVE_PROMPT__" {
                *s = prompt.to_string();
            } else if s == "__NEGATIVE_PROMPT__" {
                *s = negative.to_string();
            } else if s == "__WIDTH__" {
                *value = json!(width);
            } else if s == "__HEIGHT__" {
                *value = json!(height);
            } else if s == "__STEPS__" {
                *value = json!(steps);
            } else if s == "__SEED__" {
                *value = json!(seed);
            }
        }
        Value::Object(map) => {
            for v in map.values_mut() {
                substitute_placeholders(v, prompt, negative, width, height, steps, seed);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                substitute_placeholders(v, prompt, negative, width, height, steps, seed);
            }
        }
        _ => {}
    }
}

fn rand_seed() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_nanos() & 0x7FFF_FFFF_FFFF_FFFF) as i64)
        .unwrap_or(42)
}

// ── Built-in tool schema definitions ─────────────────────────────────────────

fn code_runner_execute_tool() -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: FunctionDef {
            name: format!("{}__execute_code", CODE_RUNNER_SERVER_ID),
            description: "Run code in a real sandboxed subprocess and return stdout, stderr, \
                           exit code, and wall-clock time. \
                           ONLY for python, javascript, or shell. \
                           NEVER use this tool for HTML, CSS, or web pages — those must be \
                           written as <artifact type=\"html\"> tags, not executed. \
                           Call this tool when the user asks you to run, execute, test, or \
                           verify Python/JS/Shell code, or whenever you want to show real output."
                .to_string(),
            parameters: json!({
                "type": "object",
                "required": ["language", "code"],
                "properties": {
                    "language": {
                        "type": "string",
                        "enum": ["python", "javascript", "shell"],
                        "description": "The language to execute. Must be one of: python, javascript, shell. HTML and CSS are NOT supported — use an artifact tag for those."
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

fn comfyui_generate_image_tool(workflow_names: Vec<String>) -> ToolDefinition {
    let workflow_enum = serde_json::Value::Array(
        workflow_names.iter().map(|n| json!(n)).collect(),
    );
    let workflow_desc = format!(
        "Workflow to use. Available: {}. \
         Use 'Default' for the built-in SD1.5 pipeline, or pick a custom workflow by name.",
        workflow_names.join(", ")
    );

    ToolDefinition {
        kind: "function".to_string(),
        function: FunctionDef {
            name: "comfyui__generate_image".to_string(),
            description: "Generate an image using Stable Diffusion via a local ComfyUI instance. \
                           Call this whenever the user asks for an image, illustration, photo, \
                           artwork, or any visual. Write a detailed, descriptive prompt. \
                           The generated image will be displayed automatically in the chat."
                .to_string(),
            parameters: json!({
                "type": "object",
                "required": ["prompt"],
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Positive prompt describing the desired image in detail. \
                                        Include style, lighting, composition, and subject details."
                    },
                    "negative_prompt": {
                        "type": "string",
                        "description": "Negative prompt — what to avoid. Defaults to 'blurry, low quality, watermark'."
                    },
                    "workflow": {
                        "type": "string",
                        "enum": workflow_enum,
                        "description": workflow_desc
                    },
                    "width": {
                        "type": "number",
                        "description": "Image width in pixels (default 512, common values: 512, 768, 1024)."
                    },
                    "height": {
                        "type": "number",
                        "description": "Image height in pixels (default 512, common values: 512, 768, 1024)."
                    },
                    "steps": {
                        "type": "number",
                        "description": "Number of diffusion steps (default 20, range 10–50). More steps = higher quality."
                    },
                    "seed": {
                        "type": "number",
                        "description": "Random seed for reproducibility. Omit for a random image."
                    }
                }
            }),
        },
    }
}
