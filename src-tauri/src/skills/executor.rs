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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
    /// Counts hard code-execution failures (exit_code != 0 or non-empty stderr) for retry capping.
    code_exec_retries: AtomicUsize,
}

impl SkillsExecutor {
    pub fn new(skills: Arc<SkillsManager>) -> Self {
        Self {
            skills,
            code_runner_db: None,
            code_runner_conv_id: None,
            code_exec_retries: AtomicUsize::new(0),
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
        cancelled: Arc<AtomicBool>,
    ) -> Result<()> {
        let tools = self.build_tool_definitions().await;

        if tools.is_empty() {
            // No tools registered — fall back to plain streaming chat.
            return engine.chat_stream(messages, config, token_tx).await;
        }

        /// A single generated media item collected across all tool-call turns.
        #[derive(Debug)]
        struct GeneratedMedia {
            /// `"image"` or `"video"`
            kind: &'static str,
            /// The live URL returned by the tool (ComfyUI /view endpoint).
            url: String,
            /// Gallery row id injected after saving, if available.
            gallery_id: Option<String>,
            /// Filename hint for the markdown alt-text.
            filename: String,
        }

        // Accumulated across turns so we can inject markdown at the very end
        // if the LLM never included the URL itself.
        let mut generated_media: Vec<GeneratedMedia> = Vec::new();

        // Accumulated HTML from rich_responses tools; injected after the LLM's
        // final response so the LLM never has to deal with large HTML strings.
        let mut generated_html: Vec<String> = Vec::new();

        /// A PDF file created by a pdf_tools tool call.
        #[derive(Debug)]
        struct GeneratedPdf {
            /// Filename used as the artifact title (e.g. "report.pdf").
            title: String,
            /// JSON metadata stored as artifact content: path, filename, pages.
            content: String,
        }

        // Accumulated PDF artifacts; injected as <artifact type="pdf"> tags at
        // the end so chat.rs saves them and MessageBubble renders a card.
        let mut generated_pdfs: Vec<GeneratedPdf> = Vec::new();

        // Helper: emit app_log event from the executor
        let log_event = |level: &str, msg: String| {
            let _ = app.emit("app_log", json!({
                "level": level,
                "message": msg,
                "ts": chrono::Utc::now().to_rfc3339(),
            }));
        };

        for turn in 0..MAX_TOOL_TURNS {
            // Check cancellation at the start of every turn (covers the gap
            // between tool dispatch and the next LLM call).
            if cancelled.load(Ordering::Relaxed) {
                log_event("info", format!("[executor] Turn {} — cancelled by user", turn));
                let _ = token_tx.send("[DONE]".to_string()).await;
                return Ok(());
            }

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

                // ── Append media markdown if the LLM didn't include it ───
                // The LLM sometimes just describes the result in text without
                // embedding the image/video. Check each generated media item;
                // if its URL does not appear anywhere in the response, inject
                // a markdown image/video block so the user can see it directly.
                if !generated_media.is_empty() {
                    // Use only the visible text that comes after the last </think> tag.
                    // Models with reasoning (Qwen3, DeepSeek-R1, etc.) include the full
                    // tool-result JSON — including the video/image URL — inside their
                    // <think>…</think> block.  A check against the raw result.content
                    // would falsely match the URL in that hidden thinking section and
                    // suppress injection, leaving the chat with no media link.
                    let visible_response: &str = result.content
                        .rfind("</think>")
                        .map(|i| &result.content[i + 8..])
                        .unwrap_or(&result.content);
                    let mut appendix = String::new();
                    for media in &generated_media {
                        // Only skip injection if the URL already appears as a markdown
                        // link "(url)" in the visible (post-thinking) part of the reply.
                        let already_embedded = visible_response
                            .contains(&format!("({})", media.url));
                        if !already_embedded {
                            let stem = media.filename
                                .rsplit('.')
                                .last()
                                .unwrap_or(&media.filename);
                            // Both images and videos use the markdown image syntax.
                            // The frontend's markdown renderer detects video URLs
                            // via filename extension and renders a <video> element
                            // instead of <img> automatically.
                            appendix.push_str(&format!(
                                "\n\n![{}]({})",
                                stem, media.url
                            ));
                        }
                    }
                    if !appendix.is_empty() {
                        log_event("info", format!(
                            "[executor] Appending {} media item(s) to response",
                            generated_media.len()
                        ));
                        let _ = token_tx.send(appendix).await;
                    }
                }

                // ── Inject buffered rich-response HTML ───────────────────
                if !generated_html.is_empty() {
                    log_event("info", format!(
                        "[executor] Injecting {} rich HTML block(s) into response",
                        generated_html.len()
                    ));
                    for html in &generated_html {
                        let _ = token_tx.send(format!("\n\n{}", html)).await;
                    }
                }

                // ── Inject PDF artifact tags ──────────────────────────────
                // Each tag is parsed by chat.rs (saved to DB) and by
                // MessageBubble (rendered as a clickable ArtifactCard).
                if !generated_pdfs.is_empty() {
                    log_event("info", format!(
                        "[executor] Injecting {} PDF artifact tag(s) into response",
                        generated_pdfs.len()
                    ));
                    for pdf in &generated_pdfs {
                        let tag = format!(
                            "\n\n<artifact type=\"pdf\" title=\"{}\">{}</artifact>",
                            pdf.title, pdf.content
                        );
                        let _ = token_tx.send(tag).await;
                    }
                }

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

                // Emit tool-call event to frontend and HTTP SSE clients
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
                {
                    use tauri::Manager;
                    if let Some(st) = app.try_state::<crate::state::AppState>() {
                        let _ = st.event_tx.send(crate::api_server::events::ApiEvent::ChatToolCall {
                            conversation_id: conv_id.to_string(),
                            tool_call_id: tc.id.clone(),
                            function_name: fn_name.to_string(),
                            arguments: args.clone(),
                            turn: turn as u32,
                        });
                    }
                }

                // Resolve any local gallery image URLs to temp files so the
                // Python subprocess never needs to make HTTP requests to localhost
                // (which fails on Windows due to IPv4/IPv6 loopback differences).
                let (resolved_args, _temp_files) = {
                    use tauri::Manager;
                    let state = app.state::<crate::state::AppState>();
                    resolve_gallery_image_urls(args, &state)
                };

                // Signal that a tool is running so the idle-watcher does not
                // kill the LLM server while we wait for the external process.
                {
                    use tauri::Manager;
                    let state = app.state::<crate::state::AppState>();
                    state.tool_active.store(true, Ordering::Relaxed);
                }

                let result_text = self
                    .dispatch_tool_call(fn_name, resolved_args, app, conv_id)
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

                // Tool finished — clear the active flag so the idle-watcher
                // resumes normal keep-alive behaviour.
                {
                    use tauri::Manager;
                    let state = app.state::<crate::state::AppState>();
                    state.tool_active.store(false, Ordering::Relaxed);
                }

                // ── Check cancellation after potentially long tool call ──
                if cancelled.load(Ordering::Relaxed) {
                    log_event("info", format!(
                        "[executor] Tool '{}' completed but generation was cancelled — stopping", fn_name
                    ));
                    let _ = token_tx.send("[DONE]".to_string()).await;
                    return Ok(());
                }

                // ── Auto-save generation results to gallery ─────────────
                let result_text = maybe_save_video_to_gallery(
                    &result_text, fn_name, app, conv_id,
                );
                let result_text = maybe_save_image_to_gallery(
                    &result_text, fn_name, app, conv_id,
                ).await;

                // ── Buffer rich HTML and strip it from the LLM context ───
                // Keeps large SVG/HTML out of the model's context window.
                let result_text = if let Some(html) = extract_inline_html(&result_text) {
                    generated_html.push(html);
                    strip_html_from_result(&result_text)
                } else {
                    result_text
                };

                // ── Track generated PDFs as artifacts ────────────────────
                // Detect create_pdf_document / create_pdf_report results and
                // queue them for <artifact type="pdf"> injection at loop end.
                {
                    let fn_lower = fn_name.to_lowercase();
                    if fn_lower.contains("create_pdf") {
                        if let Ok(rv) = serde_json::from_str::<Value>(&result_text) {
                            let status = rv.get("status").and_then(|v| v.as_str()).unwrap_or("");
                            let path = rv.get("path").and_then(|v| v.as_str()).unwrap_or("");
                            if status == "created" && path.to_lowercase().ends_with(".pdf") {
                                let filename = rv
                                    .get("filename")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("document.pdf")
                                    .to_string();
                                let pages = rv.get("pages").and_then(|v| v.as_i64()).unwrap_or(0);
                                let content = serde_json::to_string(&json!({
                                    "path": path,
                                    "filename": filename,
                                    "pages": pages,
                                }))
                                .unwrap_or_default();
                                log_event("info", format!(
                                    "[executor] PDF created: {} ({} pages) → queuing artifact",
                                    filename, pages
                                ));
                                generated_pdfs.push(GeneratedPdf {
                                    title: filename,
                                    content,
                                });
                            }
                        }
                    }
                }

                // ── Track generated media for later markdown injection ────
                if let Ok(rv) = serde_json::from_str::<Value>(&result_text) {
                    let status = rv.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    if status == "generated" {
                        let fn_lower = fn_name.to_lowercase();
                        let is_video = fn_lower.contains("generate_video") || fn_lower.contains("image_to_video");
                        let url_key = if is_video { "video_url" } else { "image_url" };
                        let kind = if is_video { "video" } else { "image" };
                        if let Some(url) = rv.get(url_key).and_then(|v| v.as_str()) {
                            let filename = rv
                                .get("filename")
                                .and_then(|v| v.as_str())
                                .unwrap_or("generated")
                                .to_string();
                            let gallery_id = rv
                                .get("gallery_id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            generated_media.push(GeneratedMedia {
                                kind,
                                url: url.to_string(),
                                gallery_id,
                                filename,
                            });
                        }
                    }
                }

                let result_preview: String = result_text.chars().take(100).collect();
                log_event("info", format!(
                    "[executor] Tool '{}' result ({} chars): {}", fn_name, result_text.len(), result_preview
                ));

                // Emit result event to frontend and HTTP SSE clients
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
                {
                    use tauri::Manager;
                    if let Some(st) = app.try_state::<crate::state::AppState>() {
                        let _ = st.event_tx.send(crate::api_server::events::ApiEvent::ChatToolResult {
                            conversation_id: conv_id.to_string(),
                            tool_call_id: tc.id.clone(),
                            result: result_text.clone(),
                        });
                    }
                }

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

        // ── MCP server dispatch ───────────────────────────────────────────
        // Use rfind so that server IDs containing "__" (e.g. "pkg__jellyfin")
        // are preserved: "pkg__jellyfin__get_recently_added" → ("pkg__jellyfin", "get_recently_added")
        let (server_id, tool_name) = if let Some(pos) = qualified_name.rfind("__") {
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

                // Only Python may be executed. Every other language is redirected
                // back to the LLM with an instruction to emit an artifact instead.
                if language != "python" {
                    let instruction = match language {
                        "html" | "htm" | "css" => format!(
                            "STOP — HTML/CSS must NOT be passed to execute_code.\n\
                             Output it as an artifact in your NEXT response (no tool call):\n\
                             <artifact type=\"html\" title=\"Page Title\">\n{}\n</artifact>\n\
                             Do NOT call any tool.",
                            code
                        ),
                        "javascript" | "typescript" | "js" | "ts" => format!(
                            "STOP — JavaScript/TypeScript cannot be executed here.\n\
                             Output it as a code artifact in your NEXT response (no tool call):\n\
                             <artifact type=\"code\" language=\"javascript\" title=\"Script\">\n{}\n</artifact>\n\
                             Do NOT call any tool.",
                            code
                        ),
                        "shell" | "bash" | "sh" | "zsh" | "powershell" | "ps1" => format!(
                            "STOP — Shell scripts cannot be executed here.\n\
                             Output it as a code artifact in your NEXT response (no tool call):\n\
                             <artifact type=\"code\" language=\"shell\" title=\"Shell Script\">\n{}\n</artifact>\n\
                             Do NOT call any tool.",
                            code
                        ),
                        other => format!(
                            "STOP — Only Python can be executed with execute_code. \
                             '{}' is not a supported execution language.\n\
                             If this is displayable code, output it as a code artifact in your \
                             NEXT response (no tool call):\n\
                             <artifact type=\"code\" language=\"{}\" title=\"Code\">\n{}\n</artifact>\n\
                             Do NOT call any tool.",
                            other, other, code
                        ),
                    };
                    log::warn!("[executor] Non-Python execute_code attempt (language={}) — redirecting to artifact", language);
                    return Ok(serde_json::to_string(&json!({
                        "error": "NOT_EXECUTABLE",
                        "instruction": instruction,
                    })).unwrap_or_default());
                }

                // ── Pre-flight guards ──────────────────────────────────────────
                // Guard 1: HTML/CSS content stored inside a Python variable.
                // Pattern: html_content = """<!DOCTYPE html>..."""
                // This never produces useful output and always either fails JSON
                // parsing (if large enough) or runs silently and returns nothing.
                let code_lower = code.to_lowercase();
                let has_html_signature = code_lower.contains("<!doctype")
                    || (code_lower.contains("<html") && code_lower.contains("</html>"));
                if has_html_signature && code.len() > 512 {
                    log::warn!(
                        "[executor] execute_code called with HTML embedded in Python ({} chars) — redirecting to artifact",
                        code.len()
                    );
                    return Ok(serde_json::to_string(&json!({
                        "error": "HTML_IN_PYTHON",
                        "instruction":
                            "STOP — You are embedding HTML inside a Python variable and passing it \
                             to execute_code. This is WRONG and will always fail or produce no output.\n\
                             Output the HTML DIRECTLY as an artifact in your NEXT response — \
                             do NOT call any tool:\n\
                             <artifact type=\"html\" title=\"Page Title\">\n\
                             <!DOCTYPE html>\n...(full HTML here)...\n\
                             </artifact>\n\
                             Write the complete artifact tag immediately. No tool call."
                    })).unwrap_or_default());
                }

                // Guard 2: oversized code (> 12 KB).
                // Large code blocks can cause llama-server JSON parse failures
                // when the argument string is truncated mid-generation.
                const MAX_CODE_BYTES: usize = 12_288;
                if code.len() > MAX_CODE_BYTES {
                    log::warn!(
                        "[executor] execute_code code too large ({} chars > {} limit) — redirecting",
                        code.len(), MAX_CODE_BYTES
                    );
                    return Ok(serde_json::to_string(&json!({
                        "error": "CODE_TOO_LARGE",
                        "instruction": format!(
                            "Your Python code is {} characters, which exceeds the safe limit of {} bytes.\n\
                             If you are generating file content (HTML, data, documents), output it DIRECTLY \
                             as an artifact instead — no tool call needed:\n\
                             <artifact type=\"html\" title=\"Title\">...content...</artifact>\n\
                             If this is genuine computational code, split it into smaller functions and \
                             call execute_code for each part separately.",
                            code.len(), MAX_CODE_BYTES
                        )
                    })).unwrap_or_default());
                }
                // ── End pre-flight guards ──────────────────────────────────────

                log::info!("[executor] Executing {} code ({} chars)", language, code.len());
                let run = crate::code_runner::execute_code(language, code).await?;
                log::info!(
                    "[executor] Code execution complete: exit_code={} stdout_len={} stderr_len={}",
                    run.exit_code, run.stdout.len(), run.stderr.len()
                );

                // Build result as a mutable JSON value so we can attach retry hints.
                let mut result_val = serde_json::to_value(&run).unwrap_or(json!({}));

                let hard_fail = run.exit_code != 0 || !run.stderr.is_empty();
                let soft_fail = !hard_fail && {
                    let out = run.stdout.to_ascii_lowercase();
                    out.contains("traceback") || out.contains("error:") || out.contains("exception")
                };

                if hard_fail {
                    // Increment the persistent retry counter for this executor session.
                    let attempt = self.code_exec_retries.fetch_add(1, Ordering::Relaxed) + 1;
                    log::warn!("[executor] Hard code failure (attempt {}/3) exit_code={}", attempt, run.exit_code);
                    if attempt <= 3 {
                        result_val["_retry_hint"] = json!(format!(
                            "FAILURE: Code exited with code {}. \
                             Analyze the error above, fix the code, and call execute_code again immediately. \
                             This is attempt {}/3. DO NOT describe the error — rewrite and retry.",
                            run.exit_code, attempt
                        ));
                    } else {
                        result_val["_retry_hint"] = json!(
                            "MAX_RETRIES_REACHED: 3 code execution attempts have failed. \
                             Do NOT call execute_code again. \
                             Summarize what went wrong and provide your best explanation."
                        );
                    }
                } else if soft_fail {
                    log::info!("[executor] Soft code failure detected in stdout (caught exception?)");
                    result_val["_retry_hint"] = json!(
                        "WARNING: The output suggests a runtime error was caught and printed. \
                         If the result is wrong or an error occurred, fix the code and call \
                         execute_code again (max 3 total attempts). \
                         Always use sys.exit(1) on failure so errors are reliably detected."
                    );
                }

                Ok(result_val.to_string())
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

/// If `result_text` looks like a successful video generation result (has `video_url`
/// and `status: "generated"`), persist it to the gallery and inject `gallery_id` into
/// the returned JSON so the frontend can reference it.
///
/// Videos are stored by URL reference (not base64) because they can be very large.
/// The `image_data` column holds the video URL; the `mime_type` distinguishes
/// video entries from image entries.
fn maybe_save_video_to_gallery(
    result_text: &str,
    fn_name: &str,
    app: &tauri::AppHandle,
    conv_id: &str,
) -> String {
    // Only process tool calls that look like video generation.
    let fn_lower = fn_name.to_lowercase();
    if !fn_lower.contains("generate_video") && !fn_lower.contains("image_to_video") {
        return result_text.to_string();
    }

    let mut result_val: Value = match serde_json::from_str(result_text) {
        Ok(v) => v,
        Err(_) => return result_text.to_string(),
    };

    let status = result_val.get("status").and_then(|v| v.as_str()).unwrap_or("");
    let video_url = result_val.get("video_url").and_then(|v| v.as_str()).unwrap_or("");
    if status != "generated" || video_url.is_empty() {
        return result_text.to_string();
    }

    let filename = result_val.get("filename").and_then(|v| v.as_str()).unwrap_or("video.mp4").to_string();
    let prompt = result_val.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let width = result_val.get("width").and_then(|v| v.as_i64());
    let height = result_val.get("height").and_then(|v| v.as_i64());

    let mime = if filename.to_lowercase().ends_with(".gif") {
        "image/gif"
    } else if filename.to_lowercase().ends_with(".webm") {
        "video/webm"
    } else {
        "video/mp4"
    };

    let gid = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    use tauri::Manager;
    let state = app.state::<crate::state::AppState>();
    {
        let db = state.db.lock().unwrap();
        match db.conn.execute(
            "INSERT INTO gallery_images \
             (id, conversation_id, source, filename, image_data, mime_type, \
              prompt, width, height, created_at) \
             VALUES (?1, ?2, 'generated', ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            sql_params![
                gid,
                conv_id,
                filename,
                video_url,
                mime,
                prompt,
                width,
                height,
                now
            ],
        ) {
            Ok(_) => {
                log::info!(
                    "[video-gallery] Saved video to gallery: id={} conv={} file={}",
                    gid, conv_id, filename
                );
                result_val["gallery_id"] = json!(gid);
            }
            Err(e) => {
                log::error!(
                    "[video-gallery] Failed to save video to gallery: {} (conv_id={}, filename={})",
                    e, conv_id, filename
                );
            }
        }
    }

    // Emit gallery_updated so the frontend refreshes
    if result_val.get("gallery_id").is_some() {
        let _ = app.emit(
            "gallery_updated",
            json!({ "conversation_id": conv_id }),
        );
        if let Some(st) = app.try_state::<crate::state::AppState>() {
            let _ = st.event_tx.send(crate::api_server::events::ApiEvent::GalleryUpdated {
                conversation_id: conv_id.to_string(),
            });
        }
    }

    serde_json::to_string(&result_val).unwrap_or_else(|_| result_text.to_string())
}

/// If `result_text` looks like a successful image generation or editing result
/// (has `image_url` and `status: "generated"`), downloads the image, writes it
/// to `{data_dir}/gallery/{id}.ext` on disk, and stores the file path in the DB.
/// This avoids base64 bloat in SQLite and makes images loadable via Tauri's
/// `asset://` protocol — no HTTP round-trip through the WebView.
async fn maybe_save_image_to_gallery(
    result_text: &str,
    fn_name: &str,
    app: &tauri::AppHandle,
    conv_id: &str,
) -> String {
    let fn_lower = fn_name.to_lowercase();
    if !fn_lower.contains("generate_image") && !fn_lower.contains("edit_image") {
        return result_text.to_string();
    }

    let mut result_val: Value = match serde_json::from_str(result_text) {
        Ok(v) => v,
        Err(_) => return result_text.to_string(),
    };

    let status = result_val.get("status").and_then(|v| v.as_str()).unwrap_or("");
    let image_url = result_val.get("image_url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if status != "generated" || image_url.is_empty() {
        return result_text.to_string();
    }

    if result_val.get("gallery_id").is_some() {
        return result_text.to_string();
    }

    let filename = result_val
        .get("filename")
        .and_then(|v| v.as_str())
        .unwrap_or("image.png")
        .to_string();
    let prompt = result_val.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let width = result_val.get("width").and_then(|v| v.as_i64());
    let height = result_val.get("height").and_then(|v| v.as_i64());

    let ext = if filename.to_lowercase().ends_with(".webp") {
        "webp"
    } else if filename.to_lowercase().ends_with(".jpg") || filename.to_lowercase().ends_with(".jpeg") {
        "jpg"
    } else {
        "png"
    };
    let mime = match ext {
        "webp" => "image/webp",
        "jpg" => "image/jpeg",
        _ => "image/png",
    };

    let gid = uuid::Uuid::new_v4().to_string();

    use tauri::Manager;
    let state = app.state::<crate::state::AppState>();
    let gallery_dir = state.data_dir.join("gallery");
    let _ = std::fs::create_dir_all(&gallery_dir);
    let dest_path = gallery_dir.join(format!("{}.{}", gid, ext));

    // Download with timeout + retry, write directly to disk (no base64).
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut file_path_str = String::new();
    let mut fallback_image_data = String::new();

    if image_url.starts_with("http://") || image_url.starts_with("https://") {
        let mut downloaded = false;
        for attempt in 1..=3u32 {
            match client.get(&image_url).send().await {
                Ok(resp) => match resp.bytes().await {
                    Ok(bytes) => {
                        match std::fs::write(&dest_path, &bytes) {
                            Ok(_) => {
                                file_path_str = dest_path.to_string_lossy().to_string();
                                downloaded = true;
                                log::info!(
                                    "[image-gallery] Saved {} bytes to {} (attempt {})",
                                    bytes.len(), file_path_str, attempt
                                );
                            }
                            Err(e) => {
                                log::warn!("[image-gallery] fs::write failed: {} (attempt {})", e, attempt);
                            }
                        }
                        break;
                    }
                    Err(e) => {
                        log::warn!("[image-gallery] read bytes failed: {} (attempt {})", e, attempt);
                    }
                },
                Err(e) => {
                    log::warn!("[image-gallery] download failed: {} (attempt {})", e, attempt);
                }
            }
            if attempt < 3 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
            }
        }
        if !downloaded {
            log::warn!("[image-gallery] All 3 download attempts failed, storing URL as fallback");
            fallback_image_data = image_url.clone();
        }
    } else {
        fallback_image_data = image_url.clone();
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mobile_port = state.settings.lock().unwrap().mobile_api_port;

    // image_data is empty when file_path is set; holds URL/base64 only as legacy fallback.
    let db_image_data = if file_path_str.is_empty() { &fallback_image_data } else { "" };
    let db_file_path: Option<&str> = if file_path_str.is_empty() { None } else { Some(&file_path_str) };

    {
        let db = state.db.lock().unwrap();
        match db.conn.execute(
            "INSERT INTO gallery_images \
             (id, conversation_id, source, filename, image_data, mime_type, \
              prompt, width, height, created_at, file_path) \
             VALUES (?1, ?2, 'generated', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            sql_params![
                gid,
                conv_id,
                filename,
                db_image_data,
                mime,
                prompt,
                width,
                height,
                now,
                db_file_path
            ],
        ) {
            Ok(_) => {
                log::info!(
                    "[image-gallery] Saved to gallery: id={} conv={} file_path={:?}",
                    gid, conv_id, db_file_path
                );
                let local_url = format!("http://localhost:{}/images/{}", mobile_port, gid);
                result_val["gallery_id"] = json!(gid);
                result_val["image_url"] = json!(local_url);
                if let Some(fp) = db_file_path {
                    result_val["file_path"] = json!(fp);
                }
            }
            Err(e) => {
                log::error!(
                    "[image-gallery] Failed to insert gallery row: {} (conv_id={}, filename={})",
                    e, conv_id, filename
                );
            }
        }
    }

    if result_val.get("gallery_id").is_some() {
        let _ = app.emit(
            "gallery_updated",
            json!({ "conversation_id": conv_id }),
        );
        if let Some(st) = app.try_state::<crate::state::AppState>() {
            let _ = st.event_tx.send(crate::api_server::events::ApiEvent::GalleryUpdated {
                conversation_id: conv_id.to_string(),
            });
        }
    }

    serde_json::to_string(&result_val).unwrap_or_else(|_| result_text.to_string())
}

// ── Gallery URL → temp file resolution ───────────────────────────────────────
//
// When the executor is about to dispatch a tool call, any JSON argument string
// that matches `http://localhost:{port}/images/{gallery_id}` is replaced with
// an absolute path to a temp file containing the raw image bytes from SQLite.
//
// This avoids the Python subprocess needing to make HTTP requests to the local
// API server, which fails on Windows because the server binds to `0.0.0.0`
// (IPv4 only) but Python may resolve `localhost` to `::1` (IPv6).
//
// The returned Vec<String> holds the temp file paths; they are deleted when the
// Vec is dropped (via a wrapper that removes the files on drop).

struct TempFile(String);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn resolve_gallery_image_urls(
    mut args: Value,
    state: &crate::state::AppState,
) -> (Value, Vec<TempFile>) {
    let mut temp_files: Vec<TempFile> = Vec::new();

    let port = state.settings.lock().unwrap().mobile_api_port;
    let prefix = format!("http://localhost:{}/images/", port);

    // Walk every string value in the JSON args.
    walk_and_replace(&mut args, &prefix, state, &mut temp_files);

    (args, temp_files)
}

fn walk_and_replace(
    val: &mut Value,
    prefix: &str,
    state: &crate::state::AppState,
    temp_files: &mut Vec<TempFile>,
) {
    match val {
        Value::String(s) => {
            if s.starts_with(prefix) {
                let gallery_id = &s[prefix.len()..];
                if let Some(path) = gallery_id_to_temp_file(gallery_id, state) {
                    *s = path.clone();
                    temp_files.push(TempFile(path));
                }
            }
        }
        Value::Object(map) => {
            for v in map.values_mut() {
                walk_and_replace(v, prefix, state, temp_files);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                walk_and_replace(v, prefix, state, temp_files);
            }
        }
        _ => {}
    }
}

fn gallery_id_to_temp_file(gallery_id: &str, state: &crate::state::AppState) -> Option<String> {
    // Load image_data and mime_type from the gallery.
    let (image_data, mime_type, filename): (String, String, String) = {
        let db = state.db.lock().unwrap();
        db.conn.query_row(
            "SELECT image_data, mime_type, filename FROM gallery_images WHERE id = ?1",
            rusqlite::params![gallery_id],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            )),
        ).ok()?
    };

    // If `image_data` is base64 — decode to bytes.
    // If it's still a URL (legacy), we can't help here; skip.
    if image_data.starts_with("http://") || image_data.starts_with("https://") {
        log::warn!(
            "[gallery-resolve] Gallery entry {} has a URL instead of base64 data; skipping temp-file resolution",
            gallery_id
        );
        return None;
    }

    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(&image_data).ok()?;

    // Determine extension from mime type.
    let ext = match mime_type.as_str() {
        "image/jpeg" | "image/jpg" => ".jpg",
        "image/png" => ".png",
        "image/webp" => ".webp",
        "image/gif" => ".gif",
        _ => {
            // Fall back to the filename's extension.
            std::path::Path::new(&filename)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| if e.starts_with('.') { e } else { "" })
                .unwrap_or(".png")
        }
    };

    let tmp_dir = std::env::temp_dir();
    let tmp_name = format!("xandsuite_img_{}{}", gallery_id, ext);
    let tmp_path = tmp_dir.join(&tmp_name);

    match std::fs::write(&tmp_path, &bytes) {
        Ok(_) => {
            log::info!(
                "[gallery-resolve] Wrote gallery image {} to {}",
                gallery_id,
                tmp_path.display()
            );
            Some(tmp_path.to_string_lossy().into_owned())
        }
        Err(e) => {
            log::error!("[gallery-resolve] Failed to write temp file: {}", e);
            None
        }
    }
}

// ── Built-in tool schema definitions ─────────────────────────────────────────

fn code_runner_execute_tool() -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: FunctionDef {
            name: format!("{}__execute_code", CODE_RUNNER_SERVER_ID),
            description: "Run Python code in a real sandboxed subprocess and return stdout, \
                           stderr, exit code, and wall-clock time. \
                           ONLY Python is supported — do NOT pass any other language to this tool. \
                           For HTML/CSS pages use: <artifact type=\"html\" title=\"...\">...</artifact>. \
                           For JavaScript/TypeScript use: <artifact type=\"code\" language=\"javascript\" title=\"...\">...</artifact>. \
                           For shell scripts use: <artifact type=\"code\" language=\"shell\" title=\"...\">...</artifact>. \
                           Call this tool only when the user asks you to run, execute, test, or \
                           verify Python code, or whenever you need to show real computed output. \
                           IMPORTANT: Always signal failures with `sys.exit(1)` — do NOT silently \
                           catch exceptions and print them; raise them so errors are reliably \
                           detected and retried. \
                           If a result has `_retry_hint` in the response, read it and act on it."
                .to_string(),
            parameters: json!({
                "type": "object",
                "required": ["language", "code"],
                "properties": {
                    "language": {
                        "type": "string",
                        "enum": ["python"],
                        "description": "Must be 'python'. This is the only supported execution language. For all other languages output an artifact tag instead of calling this tool."
                    },
                    "code": {
                        "type": "string",
                        "description": "The Python source code to run. Write it exactly as you would in a .py file."
                    }
                }
            }),
        },
    }
}

/// Parse a rich-response tool result and return the `html` field value if the
/// result has `"display": "inline_html"`. Returns `None` for all other results.
fn extract_inline_html(result: &str) -> Option<String> {
    let v: Value = serde_json::from_str(result).ok()?;
    if v.get("display").and_then(|d| d.as_str()) != Some("inline_html") {
        return None;
    }
    v.get("html").and_then(|h| h.as_str()).map(|s| s.to_string())
}

/// Return the tool result JSON with the `html` field replaced by a short
/// acknowledgement so the LLM context stays small.
fn strip_html_from_result(result: &str) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(result) else {
        return result.to_string();
    };
    if let Some(obj) = v.as_object_mut() {
        obj.insert("html".to_string(), json!("[rendered inline]"));
    }
    serde_json::to_string(&v).unwrap_or_else(|_| result.to_string())
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

