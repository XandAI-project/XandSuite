use chrono::Utc;
use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::models::{Conversation, InferenceConfig, Message, MessageRole};
use crate::skills::SkillsExecutor;
use crate::state::AppState;

// ── Artifact helpers ──────────────────────────────────────────────────────────

struct RawArtifact {
    title: String,
    artifact_type: String,
    language: Option<String>,
    content: String,
}

/// Extract artifact tags from LLM output.
/// Returns one entry per complete `<artifact ...>...</artifact>` block.
fn extract_artifacts(text: &str) -> Vec<RawArtifact> {
    // (?si) = dotall (. matches \n) + case-insensitive
    let Ok(artifact_re) = regex::Regex::new(r"(?si)<artifact\s+([^>]*)>(.*?)</artifact>") else {
        return vec![];
    };
    // Also accept single-quoted attribute values
    let Ok(attr_re) = regex::Regex::new(r#"(\w[\w-]*)=["']([^"']*)["']"#) else {
        return vec![];
    };
    let Ok(fence_re) = regex::Regex::new(r"(?s)^```[\w]*\n?(.*?)```\s*$") else {
        return vec![];
    };

    artifact_re
        .captures_iter(text)
        .map(|cap| {
            let attr_str = cap[1].to_string();
            let raw = cap[2].trim().to_string();

            // Strip markdown code fences the LLM sometimes wraps inside tags
            let content = fence_re
                .captures(&raw)
                .map(|fc| fc[1].trim().to_string())
                .unwrap_or(raw);

            let mut title = "Untitled".to_string();
            let mut artifact_type = "text".to_string();
            let mut language: Option<String> = None;

            for am in attr_re.captures_iter(&attr_str) {
                match &am[1] {
                    "title"    => title = am[2].to_string(),
                    "type"     => artifact_type = am[2].to_string(),
                    "language" => language = Some(am[2].to_string()),
                    _ => {}
                }
            }

            // If the LLM used type="text" but also specified a language,
            // it almost certainly meant type="code".
            if artifact_type == "text" && language.is_some() {
                artifact_type = "code".to_string();
            }

            RawArtifact { title, artifact_type, language, content }
        })
        .collect()
}

#[derive(Serialize)]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub model_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: u64,
}

#[tauri::command]
pub fn list_conversations(state: State<'_, AppState>) -> Result<Vec<ConversationSummary>, String> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.conn.prepare(
        r#"SELECT c.id, c.title, c.model_id, c.created_at, c.updated_at,
           COUNT(m.id) as msg_count
           FROM conversations c
           LEFT JOIN messages m ON m.conversation_id = c.id
           GROUP BY c.id
           ORDER BY c.updated_at DESC"#
    ).map_err(|e| e.to_string())?;

    let summaries = stmt.query_map([], |row| {
        Ok(ConversationSummary {
            id: row.get(0)?,
            title: row.get(1)?,
            model_id: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            message_count: row.get(5)?,
        })
    })
    .map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    Ok(summaries)
}

#[tauri::command]
pub fn create_conversation(
    title: String,
    system_prompt: Option<String>,
    state: State<'_, AppState>,
) -> Result<Conversation, String> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let db = state.db.lock().unwrap();
    db.conn.execute(
        "INSERT INTO conversations (id, title, model_id, system_prompt, created_at, updated_at)
         VALUES (?1, ?2, NULL, ?3, ?4, ?4)",
        params![id, title, system_prompt, now],
    ).map_err(|e| e.to_string())?;

    let messages = match &system_prompt {
        Some(sp) if !sp.is_empty() => {
            let msg_id = Uuid::new_v4().to_string();
            db.conn.execute(
                "INSERT INTO messages (id, conversation_id, role, content, created_at)
                 VALUES (?1, ?2, 'system', ?3, ?4)",
                params![msg_id, id, sp, now],
            ).map_err(|e| e.to_string())?;
            vec![Message {
                id: msg_id,
                conversation_id: id.clone(),
                role: MessageRole::System,
                content: sp.clone(),
                created_at: Utc::now(),
                token_count: None,
                metadata: None,
            }]
        }
        _ => vec![],
    };

    Ok(Conversation {
        id,
        title,
        model_id: None,
        system_prompt,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        messages,
    })
}

#[tauri::command]
pub fn update_conversation(
    conversation_id: String,
    title: Option<String>,
    system_prompt: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    let db = state.db.lock().unwrap();
    if let Some(t) = title {
        db.conn.execute(
            "UPDATE conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![t, now, conversation_id],
        ).map_err(|e| e.to_string())?;
    }
    if let Some(sp) = &system_prompt {
        db.conn.execute(
            "UPDATE conversations SET system_prompt = ?1, updated_at = ?2 WHERE id = ?3",
            params![sp, now, conversation_id],
        ).map_err(|e| e.to_string())?;
        // Also upsert the system message row so the LLM sees the updated prompt
        let existing_sys: Option<String> = db.conn.query_row(
            "SELECT id FROM messages WHERE conversation_id = ?1 AND role = 'system' LIMIT 1",
            params![conversation_id],
            |row| row.get(0),
        ).ok();
        if let Some(msg_id) = existing_sys {
            db.conn.execute(
                "UPDATE messages SET content = ?1 WHERE id = ?2",
                params![sp, msg_id],
            ).map_err(|e| e.to_string())?;
        } else if !sp.is_empty() {
            let msg_id = uuid::Uuid::new_v4().to_string();
            db.conn.execute(
                "INSERT INTO messages (id, conversation_id, role, content, created_at)
                 VALUES (?1, ?2, 'system', ?3, ?4)",
                params![msg_id, conversation_id, sp, now],
            ).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn get_conversation(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Conversation, String> {
    let db = state.db.lock().unwrap();

    let (id, title, model_id, system_prompt) = db.conn.query_row(
        "SELECT id, title, model_id, system_prompt FROM conversations WHERE id = ?1",
        params![conversation_id],
        |row| Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
        )),
    ).map_err(|e| format!("Conversation not found: {}", e))?;

    let mut msg_stmt = db.conn.prepare(
        "SELECT id, role, content, metadata FROM messages
         WHERE conversation_id = ?1 ORDER BY rowid ASC"
    ).map_err(|e| e.to_string())?;

    let messages: Vec<Message> = msg_stmt.query_map(params![id], |row| {
        let role_str: String = row.get(1)?;
        let role = match role_str.as_str() {
            "system" => MessageRole::System,
            "assistant" => MessageRole::Assistant,
            "tool" => MessageRole::Tool,
            _ => MessageRole::User,
        };
        let metadata_raw: Option<String> = row.get(3)?;
        let metadata = metadata_raw.and_then(|s| serde_json::from_str(&s).ok());
        Ok(Message {
            id: row.get(0)?,
            conversation_id: id.clone(),
            role,
            content: row.get(2)?,
            created_at: Utc::now(),
            token_count: None,
            metadata,
        })
    })
    .map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    Ok(Conversation {
        id,
        title,
        model_id,
        system_prompt,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        messages,
    })
}

#[tauri::command]
pub fn delete_conversation(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.conn.execute(
        "DELETE FROM conversations WHERE id = ?1",
        params![conversation_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Delete `from_message_id` and every message inserted after it in `conversation_id`.
/// Used by the frontend to implement "edit & resend" and "regenerate".
#[tauri::command]
pub fn truncate_conversation(
    conversation_id: String,
    from_message_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.conn.execute(
        "DELETE FROM messages
         WHERE conversation_id = ?1
           AND rowid >= (
               SELECT rowid FROM messages
               WHERE id = ?2 AND conversation_id = ?1
           )",
        params![conversation_id, from_message_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    conversation_id: String,
    content: String,
    use_rag: bool,
    rag_collection_id: Option<String>,
    use_skills: Option<bool>,
    attachments: Option<Vec<String>>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let user_msg_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp"];

    fn ext_mime(ext: &str) -> &'static str {
        match ext {
            "jpg" | "jpeg" => "image/jpeg",
            "png"          => "image/png",
            "gif"          => "image/gif",
            "webp"         => "image/webp",
            "bmp"          => "image/bmp",
            _              => "image/jpeg",
        }
    }

    // Read attachment file contents before saving (async, no DB lock held).
    // Images are base64-encoded for VLM multimodal content; text files are
    // ingested as before.
    let mut attachment_blocks: Vec<(String, String)> = Vec::new();
    // (filename, mime_type, base64_data, full_path)
    let mut image_blocks: Vec<(String, String, String, String)> = Vec::new();

    if let Some(ref paths) = attachments {
        for path_str in paths {
            let path = std::path::Path::new(path_str);
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
                match std::fs::read(path_str) {
                    Ok(bytes) => {
                        use base64::Engine as _;
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        let mime = ext_mime(&ext);
                        image_blocks.push((filename, mime.to_string(), b64, path_str.clone()));
                    }
                    Err(e) => {
                        log::warn!("Failed to read image attachment '{}': {}", filename, e);
                    }
                }
            } else {
                match crate::rag::ingest::ingest_file(path).await {
                    Ok(chunks) => {
                        let content_text = chunks
                            .into_iter()
                            .map(|(text, _)| text)
                            .collect::<Vec<_>>()
                            .join("\n\n");
                        attachment_blocks.push((filename, content_text));
                    }
                    Err(e) => {
                        log::warn!("Failed to read attachment '{}': {}", filename, e);
                    }
                }
            }
        }
    }

    // Build metadata JSON — text attachment basenames + image full paths stored
    // so the frontend can reload thumbnails from chat history.
    let metadata_json: Option<String> = {
        let mut meta = serde_json::Map::new();
        if !attachment_blocks.is_empty() {
            let names: Vec<&str> = attachment_blocks.iter().map(|(n, _)| n.as_str()).collect();
            meta.insert("attachments".into(), serde_json::json!(names));
        }
        if !image_blocks.is_empty() {
            // Store full paths so the frontend can reload thumbnails.
            let paths: Vec<&str> = image_blocks.iter().map(|(_, _, _, p)| p.as_str()).collect();
            meta.insert("images".into(), serde_json::json!(paths));
        }
        if meta.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(meta).to_string())
        }
    };

    // Save user message (sync, short lock)
    {
        let db = state.db.lock().unwrap();
        db.conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content, created_at, metadata)
             VALUES (?1, ?2, 'user', ?3, ?4, ?5)",
            params![user_msg_id, conversation_id, content, now, metadata_json],
        ).map_err(|e| e.to_string())?;
        db.conn.execute(
            "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
            params![now, conversation_id],
        ).map_err(|e| e.to_string())?;
    }

    // Build message history (sync, short lock)
    let mut messages: Vec<(String, String)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.conn.prepare(
            "SELECT role, content FROM messages WHERE conversation_id = ?1 ORDER BY rowid ASC"
        ).map_err(|e| e.to_string())?;
        let rows: Vec<(String, String)> = stmt.query_map(params![conversation_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
        rows
    };

    // Helper: emit an app_log event AND write to the Rust log in one call.
    // Defined early so it can be used by URL-fetch, RAG, and the streaming
    // spawn block below.
    let emit_log = {
        let app_ref = app.clone();
        move |level: &str, message: String| {
            match level {
                "error" => log::error!("{}", message),
                "warn"  => log::warn!("{}", message),
                _       => log::info!("{}", message),
            }
            let _ = app_ref.emit("app_log", serde_json::json!({
                "level": level,
                "message": message,
                "ts": chrono::Utc::now().to_rfc3339(),
            }));
        }
    };

    // Inject attachment content into the last user message (before RAG context).
    // For text files we prepend extracted text; for images we build a multimodal
    // JSON marker so remote.rs can emit the OpenAI VLM content array.
    if !attachment_blocks.is_empty() {
        let attachment_text: String = attachment_blocks
            .iter()
            .map(|(name, content_text)| {
                format!(
                    "[Attached file: {}]\n{}\n[End of file: {}]",
                    name, content_text, name
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        if let Some(last_user) = messages.iter_mut().rev().find(|(role, _)| role == "user") {
            last_user.1 = format!("{}\n\n{}", attachment_text, last_user.1);
        }
    }

    if !image_blocks.is_empty() {
        // Replace the last user message content with a multimodal marker JSON
        // so build_messages in remote.rs can construct the VLM content array.
        if let Some(last_user) = messages.iter_mut().rev().find(|(role, _)| role == "user") {
            let text_part = serde_json::json!({
                "type": "text",
                "text": last_user.1
            });
            let mut parts = vec![text_part];
            for (_, mime, b64, _) in &image_blocks {
                parts.push(serde_json::json!({
                    "type": "image_url",
                    "image_url": {
                        "url": format!("data:{};base64,{}", mime, b64)
                    }
                }));
            }
            last_user.1 = serde_json::json!({
                "__mm": true,
                "parts": parts
            })
            .to_string();
        }
    }

    // ── Auto-fetch URLs found in the user message ────────────────────────────
    // Detects http(s) URLs in the original content, fetches each one, and
    // prepends the extracted text to the last user message so the LLM has
    // the page content available.  Nothing is written to the DB; this is
    // purely an in-memory augmentation (same pattern as attachments / RAG).
    {
        let url_re = regex::Regex::new(r#"https?://[^\s>"'\)\]]+"#).unwrap();
        let found_urls: Vec<String> = url_re
            .find_iter(&content)
            .map(|m| m.as_str().trim_end_matches('.').to_string())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if !found_urls.is_empty() {
            let mut url_blocks: Vec<String> = Vec::new();
            for url in &found_urls {
                emit_log("info", format!("[web_fetch] Fetching {}", url));
                match crate::web_fetch::fetch_url_content(url).await {
                    Ok(text) => {
                        emit_log("info", format!(
                            "[web_fetch] OK {} ({} chars)", url, text.len()
                        ));
                        url_blocks.push(format!(
                            "[Web page: {}]\n{}\n[End of web page: {}]",
                            url, text, url
                        ));
                    }
                    Err(e) => {
                        emit_log("warn", format!("[web_fetch] Failed {}: {}", url, e));
                        url_blocks.push(format!(
                            "[Web page: {}]\n[Could not retrieve content: {}]\n[End of web page: {}]",
                            url, e, url
                        ));
                    }
                }
            }
            if !url_blocks.is_empty() {
                let block_text = url_blocks.join("\n\n");
                if let Some(last_user) = messages.iter_mut().rev().find(|(role, _)| role == "user") {
                    last_user.1 = format!("{}\n\n{}", last_user.1, block_text);
                }
            }
        }
    }

    // Inject RAG context if enabled
    if use_rag {
        let rag = state.rag.lock().await;
        let context = rag.build_rag_context(&content, rag_collection_id.as_deref());
        if !context.is_empty() {
            // Instruct the model to ground its answer in the retrieved excerpts.
            // This is appended to an existing system message or inserted as a new
            // one so it takes effect for this single inference call without being
            // persisted to the conversation history.
            let rag_instruction = "You have access to excerpts from a knowledge base \
                that are included at the top of the user's message. \
                Answer the user's question using ONLY those excerpts. \
                If the answer is not contained in the excerpts, say so explicitly \
                instead of guessing or drawing on outside knowledge.";

            if let Some(sys_msg) = messages.iter_mut().find(|(role, _)| role == "system") {
                sys_msg.1 = format!("{}\n\n{}", sys_msg.1, rag_instruction);
            } else {
                messages.insert(0, ("system".to_string(), rag_instruction.to_string()));
            }

            if let Some(last_user) = messages.iter_mut().rev().find(|(role, _)| role == "user") {
                last_user.1 = format!("{}\n\n{}", context, last_user.1);
            }
        }
    }

    // Read feature flags before building system prompt additions.
    let code_execution_enabled = state.settings.lock().unwrap().enable_code_execution;

    // Always inject artifact instructions so the model knows to wrap standalone
    // outputs in <artifact> tags. Appended to the existing system message (or
    // inserted as a new one) so it is never persisted to the DB.
    {
        let mut artifact_instruction = String::from("When your response includes a standalone file, \
            document, code snippet longer than ~20 lines, or any output intended to \
            be saved or reused, wrap it in an artifact tag:\n\
            <artifact type=\"TYPE\" language=\"LANG\" title=\"Descriptive Name\">\n\
            ...content...\n\
            </artifact>\n\n\
            TYPE must be one of:\n\
            - \"code\"     → ANY source code in any programming language (Python, JS, Rust, etc.).\n\
            \t\t\t\t\tALWAYS use type=\"code\" for code. NEVER use type=\"text\" for code.\n\
            - \"html\"     → self-contained HTML/CSS/JS pages or charts.\n\
            - \"markdown\" → formatted documentation, reports, README files.\n\
            - \"text\"     → plain-text prose that is NOT code (config files without syntax, logs, etc.).\n\n\
            LANG is required whenever type=\"code\" — set it to the file's language (e.g. language=\"python\").\n\
            Use a clear, specific title including the file extension (e.g. \"fizzbuzz.py\"). \
            One artifact per distinct output. \
            Do NOT wrap conversational text, short answers, or explanations in artifacts.\n\n\
            For charts, graphs, and data visualizations, use type=\"html\" with Chart.js loaded from CDN. \
            The HTML must be fully self-contained. Use this pattern:\n\
            <artifact type=\"html\" title=\"Chart Title\">\n\
            <!DOCTYPE html><html><head>\n\
            <script src=\"https://cdn.jsdelivr.net/npm/chart.js@4/dist/chart.umd.min.js\"></script>\n\
            <style>body{margin:0;background:#1e1e2e;display:flex;align-items:center;justify-content:center;min-height:100vh}\
            canvas{max-width:100%;max-height:90vh}</style>\n\
            </head><body>\n\
            <canvas id=\"c\"></canvas>\n\
            <script>\n\
            new Chart(document.getElementById('c'), {\n\
              type: 'bar',\n\
              data: { labels: [...], datasets: [{ label: '...', data: [...] }] },\n\
              options: { responsive: true, plugins: { legend: { labels: { color: '#cdd6f4' } } }, \
            scales: { x: { ticks: { color: '#cdd6f4' } }, y: { ticks: { color: '#cdd6f4' } } } }\n\
            });\n\
            </script>\n\
            </body></html>\n\
            </artifact>\n\
            Supported Chart.js types: bar, line, pie, doughnut, radar, polarArea, scatter, bubble.\n\
            For documents combining text and images, use type=\"html\" with inline base64 images or public URLs.\n\
            Never put markdown code fences (```) inside artifact tags — write the raw content directly.");

        if code_execution_enabled {
            artifact_instruction.push_str("\n\n\
                ## Code Execution — CRITICAL RULES\n\
                You have a REAL, WORKING code execution tool called `execute_code`. \
                You are RUNNING INSIDE A SYSTEM THAT CAN EXECUTE CODE. \
                NEVER say 'I cannot run code', 'I cannot execute code', or \
                'I am not able to run programs' — these statements are FALSE in this environment.\n\n\
                ### WHAT execute_code IS for (actual runnable code only)\n\
                - Running Python, JavaScript, or Shell to compute results, verify logic, generate output.\n\
                - Supported languages: python, javascript, shell.\n\n\
                ### WHAT execute_code is NOT for (never do these)\n\
                - NEVER call execute_code to create a document, report, story, or markdown file.\n\
                - NEVER pass language=\"markdown\", language=\"text\", or language=\"html\" to execute_code.\n\
                - NEVER write a Python script just to print or save text to a file — that is pointless.\n\
                - To create a document or text artifact, write the content DIRECTLY inside an \
                  <artifact type=\"markdown\"> or <artifact type=\"text\"> tag. No tool call needed.\n\n\
                MANDATORY behaviour:\n\
                1. Whenever you write a Python, JavaScript, or Shell code snippet that the user \
                   wants to run, or whenever you produce computed results, ALWAYS call \
                   `execute_code` immediately after writing the code artifact.\n\
                2. If the user asks you to 'run', 'execute', 'test', 'try', 'check the output of', \
                   or 'verify' any code — call `execute_code`. Do NOT explain why you cannot; just run it.\n\
                3. Show the real stdout/stderr output to the user and explain the results.\n\
                4. If execution fails, show the error and fix the code, then run again.\n\
                5. Use `list_recent_artifacts` to find code artifacts you already created in this \
                   conversation before duplicating code.");
        }

        if let Some(sys_msg) = messages.iter_mut().find(|(role, _)| role == "system") {
            sys_msg.1 = format!("{}\n\n{}", sys_msg.1, artifact_instruction);
        } else {
            messages.insert(0, ("system".to_string(), artifact_instruction));
        }
    }

    // Auto-restart the internal server if it was idle-stopped
    {
        let settings = state.settings.lock().unwrap().clone();
        if settings.default_engine_mode == "local" {
            let is_running = state.server.lock().await.is_running();
            if !is_running {
                if let Some(ref model_path) = settings.last_server_model {
                    log::info!("Auto-restarting llama-server (was idle-stopped)");
                    let port = settings.llama_server_port;
                    let data_dir = state.data_dir.clone();
                    let mp = model_path.clone();
                    {
                        let mut srv = state.server.lock().await;
                        if let Err(e) = srv.start(&mp, &settings, &data_dir).await {
                            log::error!("Failed to auto-restart llama-server: {}", e);
                        }
                    }
                    let _ = state.engine.connect_remote(
                        format!("http://127.0.0.1:{}", port), None, None
                    );
                }
            }
        }
    }

    // Stream response (async)
    let (token_tx, mut token_rx) = mpsc::channel::<String>(512);
    let engine = state.engine.clone();
    let server_arc = state.server.clone();
    let conv_id = conversation_id.clone();
    let skills_arc = state.skills.clone();
    let db_arc = state.db.clone();

    // Build config from current settings
    let config = {
        let s = state.settings.lock().unwrap();
        let mut cfg = InferenceConfig::default();
        cfg.enable_thinking = s.enable_thinking;
        cfg.thinking_budget_tokens = s.thinking_budget_tokens;
        cfg.max_tokens = s.max_response_tokens;
        cfg
    };

    // Activate the agentic executor when:
    //  a) The frontend explicitly enables skills AND MCP tools are connected, OR
    //  b) Code execution is enabled in settings (always forces tool mode).
    let has_mcp_tools = use_skills.unwrap_or(false)
        && !skills_arc.all_tools().await.is_empty();
    let has_tools = has_mcp_tools || code_execution_enabled;

    let app_clone = app.clone();
    let conv_id_clone = conv_id.clone();
    tauri::async_runtime::spawn(async move {
        if has_tools {
            // Use the agentic executor (handles tool_calls loop internally)
            let mut executor = SkillsExecutor::new(skills_arc);
            if code_execution_enabled {
                executor = executor.with_code_runner(db_arc, conv_id_clone.clone());
            }
            if let Some(remote) = engine.get_remote() {
                if let Err(e) = executor
                    .run(messages, &config, &remote, &app_clone, &conv_id_clone, token_tx)
                    .await
                {
                    log::error!("Skills executor error: {}", e);
                    let _ = app_clone.emit("app_log", serde_json::json!({
                        "level": "error",
                        "message": format!("Skills executor error: {}", e),
                        "ts": chrono::Utc::now().to_rfc3339(),
                    }));
                }
            } else {
                // No remote engine — fall back to plain streaming
                if let Err(e) = engine.chat_stream(messages, config, token_tx).await {
                    log::error!("Inference error: {}", e);
                    let _ = app_clone.emit("app_log", serde_json::json!({
                        "level": "error",
                        "message": format!("Inference error: {}", e),
                        "ts": chrono::Utc::now().to_rfc3339(),
                    }));
                }
            }
        } else {
            // No tools connected — plain streaming chat
            if let Err(e) = engine.chat_stream(messages, config, token_tx).await {
                log::error!("Inference error: {}", e);
                let _ = app_clone.emit("app_log", serde_json::json!({
                    "level": "error",
                    "message": format!("Inference error: {}", e),
                    "ts": chrono::Utc::now().to_rfc3339(),
                }));
            }
        }
    });

    let thinking_prefix = crate::engine::remote::THINKING_PREFIX;
    let mut full_response = String::new();
    while let Some(token) = token_rx.recv().await {
        if token == "[DONE]" {
            break;
        }
        if let Some(thought) = token.strip_prefix(thinking_prefix) {
            // Thinking/reasoning token — send on a separate event channel
            let _ = app.emit("chat_thinking", serde_json::json!({
                "conversation_id": conv_id,
                "token": thought,
            }));
        } else {
            full_response.push_str(&token);
            let _ = app.emit("chat_token", serde_json::json!({
                "conversation_id": conv_id,
                "token": token,
                "done": false
            }));
        }
    }

    // Reset idle timer — model is warm, keep it loaded
    server_arc.lock().await.touch();

    // ── Persist assistant message ─────────────────────────────────────────────
    let assistant_msg_id = Uuid::new_v4().to_string();
    let now2 = Utc::now().to_rfc3339();
    {
        let db = state.db.lock().unwrap();
        match db.conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content, created_at)
             VALUES (?1, ?2, 'assistant', ?3, ?4)",
            params![assistant_msg_id, conversation_id, full_response, now2],
        ) {
            Ok(_) => emit_log("info", format!(
                "[chat] Assistant message saved (id={}, {} chars)", assistant_msg_id, full_response.len()
            )),
            Err(e) => {
                emit_log("error", format!("[chat] Failed to save assistant message: {}", e));
                return Err(e.to_string());
            }
        }
    }

    // ── Parse and persist artifacts ───────────────────────────────────────────
    // Done *before* emitting done:true so the frontend re-fetch always sees them.
    {
        emit_log("info", format!(
            "[artifacts] Response complete ({} chars). Scanning for artifact tags…",
            full_response.len()
        ));

        let artifacts = extract_artifacts(&full_response);
        emit_log("info", format!("[artifacts] Found {} artifact(s)", artifacts.len()));

        if !artifacts.is_empty() {
            let db = state.db.lock().unwrap();
            for art in &artifacts {
                let preview = art.content.chars().take(60).collect::<String>();
                emit_log("info", format!(
                    "[artifacts] Processing '{}' type={} lang={:?} content_len={} preview={:?}",
                    art.title, art.artifact_type, art.language, art.content.len(), preview
                ));

                // Skip duplicates: same conversation + title + type
                let exists: bool = db.conn.query_row(
                    "SELECT COUNT(*) > 0 FROM artifacts
                     WHERE conversation_id = ?1 AND title = ?2 AND artifact_type = ?3",
                    params![conversation_id, art.title, art.artifact_type],
                    |row| row.get(0),
                ).unwrap_or(false);

                if exists {
                    emit_log("warn", format!(
                        "[artifacts] Skipping duplicate: '{}' ({})", art.title, art.artifact_type
                    ));
                    continue;
                }

                let art_id = Uuid::new_v4().to_string();
                match db.conn.execute(
                    "INSERT INTO artifacts
                     (id, conversation_id, message_id, title, artifact_type, language, content, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                    params![
                        art_id,
                        conversation_id,
                        assistant_msg_id,
                        art.title,
                        art.artifact_type,
                        art.language,
                        art.content,
                        now2
                    ],
                ) {
                    Ok(_) => emit_log("info", format!(
                        "[artifacts] Saved '{}' ({}) id={}", art.title, art.artifact_type, art_id
                    )),
                    Err(e) => emit_log("error", format!(
                        "[artifacts] INSERT failed for '{}': {}", art.title, e
                    )),
                }
            }
        } else if !full_response.is_empty() {
            // Log a snippet to help diagnose missing tags
            let snippet: String = full_response.chars().take(300).collect();
            emit_log("warn", format!(
                "[artifacts] No artifact tags found. Response starts with: {:?}", snippet
            ));
        }
    }

    // ── Notify frontend streaming is complete ─────────────────────────────────
    // Emitted AFTER DB writes so fetchArtifacts on the frontend always sees
    // the persisted data.
    let _ = app.emit("chat_token", serde_json::json!({
        "conversation_id": conv_id,
        "token": "",
        "done": true
    }));

    Ok(assistant_msg_id)
}
