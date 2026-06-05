use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::path::PathBuf;

pub struct AppDb {
    pub conn: Connection,
}

impl AppDb {
    pub fn open(data_dir: &PathBuf) -> Result<Self> {
        std::fs::create_dir_all(data_dir)
            .context("Failed to create data directory")?;
        let db_path = data_dir.join("xandsuite.db");
        let conn = Connection::open(&db_path)
            .context("Failed to open SQLite database")?;

        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .context("Failed to set WAL mode")?;

        let db = AppDb { conn };
        db.run_migrations()?;
        db.seed_examples()?;
        Ok(db)
    }

    fn seed_examples(&self) -> Result<()> {
        // Skip if any example flows already exist (idempotent)
        let already: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM flows WHERE id LIKE 'example-%'",
            [],
            |row| row.get(0),
        )?;
        if already > 0 {
            return Ok(());
        }

        let ts = "2026-01-01T00:00:00Z";

        // ── Flow 1: Content Summarizer ────────────────────────────────────────
        self.conn.execute(
            "INSERT OR IGNORE INTO flows
             (id, name, description, nodes_json, edges_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                "example-content-summarizer",
                "Content Summarizer",
                "Summarize any text into a clear, concise summary using AI.",
                r#"[
  {"id":"n1","node_type":"trigger","position_x":80,"position_y":200,
   "data":{"label":"Start","nodeType":"trigger","color":"bg-rose-500/20 border-rose-500/30 text-rose-300","trigger_type":"manual","description":"Run the flow manually"}},
  {"id":"n2","node_type":"system_prompt","position_x":320,"position_y":200,
   "data":{"label":"Summarizer Role","nodeType":"system_prompt","color":"bg-purple-500/20 border-purple-500/30 text-purple-300","prompt":"You are an expert summarizer. Create clear, concise summaries that capture the key points and main arguments. Keep summaries under 200 words unless instructed otherwise.","description":"Define the AI role"}},
  {"id":"n3","node_type":"user_prompt","position_x":560,"position_y":200,
   "data":{"label":"Summarize","nodeType":"user_prompt","color":"bg-blue-500/20 border-blue-500/30 text-blue-300","prompt":"Please summarize the following text concisely, highlighting the most important points:\n\n{{input}}","temperature":0.4,"max_tokens":512,"top_p":0.9,"description":"Generate the summary"}},
  {"id":"n4","node_type":"output","position_x":800,"position_y":200,
   "data":{"label":"Summary","nodeType":"output","color":"bg-red-500/20 border-red-500/30 text-red-300","variable":"last_response","format":"text","description":"Output the result"}}
]"#,
                r#"[
  {"id":"e1","source":"n1","target":"n2","source_handle":null,"target_handle":null},
  {"id":"e2","source":"n2","target":"n3","source_handle":null,"target_handle":null},
  {"id":"e3","source":"n3","target":"n4","source_handle":null,"target_handle":null}
]"#,
                ts, ts,
            ],
        )?;

        // ── Flow 2: Web Research Report ───────────────────────────────────────
        self.conn.execute(
            "INSERT OR IGNORE INTO flows
             (id, name, description, nodes_json, edges_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                "example-web-research",
                "Web Research Report",
                "Search the web for a topic and generate a structured research report.",
                r#"[
  {"id":"n1","node_type":"trigger","position_x":80,"position_y":200,
   "data":{"label":"Start","nodeType":"trigger","color":"bg-rose-500/20 border-rose-500/30 text-rose-300","trigger_type":"manual","description":"Run manually"}},
  {"id":"n2","node_type":"web_search","position_x":300,"position_y":200,
   "data":{"label":"Search Web","nodeType":"web_search","color":"bg-emerald-500/20 border-emerald-500/30 text-emerald-300","query":"{{input}}","max_results":5,"description":"Search for the topic"}},
  {"id":"n3","node_type":"system_prompt","position_x":520,"position_y":200,
   "data":{"label":"Analyst Role","nodeType":"system_prompt","color":"bg-purple-500/20 border-purple-500/30 text-purple-300","prompt":"You are a professional research analyst. Synthesize information from multiple sources into clear, well-structured reports with sections for Overview, Key Findings, and Conclusions.","description":"Set analyst role"}},
  {"id":"n4","node_type":"user_prompt","position_x":740,"position_y":200,
   "data":{"label":"Write Report","nodeType":"user_prompt","color":"bg-blue-500/20 border-blue-500/30 text-blue-300","prompt":"Using the following web search results, write a comprehensive research report on the topic '{{input}}':\n\n{{node_n2}}","temperature":0.6,"max_tokens":1024,"top_p":0.9,"description":"Generate research report"}},
  {"id":"n5","node_type":"output","position_x":960,"position_y":200,
   "data":{"label":"Report","nodeType":"output","color":"bg-red-500/20 border-red-500/30 text-red-300","variable":"last_response","format":"markdown","description":"Output the report"}}
]"#,
                r#"[
  {"id":"e1","source":"n1","target":"n2","source_handle":null,"target_handle":null},
  {"id":"e2","source":"n2","target":"n3","source_handle":null,"target_handle":null},
  {"id":"e3","source":"n3","target":"n4","source_handle":null,"target_handle":null},
  {"id":"e4","source":"n4","target":"n5","source_handle":null,"target_handle":null}
]"#,
                ts, ts,
            ],
        )?;

        // ── Flow 3: Code Reviewer ─────────────────────────────────────────────
        self.conn.execute(
            "INSERT OR IGNORE INTO flows
             (id, name, description, nodes_json, edges_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                "example-code-reviewer",
                "Code Reviewer",
                "Paste code as input and get a thorough AI-powered code review.",
                r#"[
  {"id":"n1","node_type":"trigger","position_x":80,"position_y":200,
   "data":{"label":"Start","nodeType":"trigger","color":"bg-rose-500/20 border-rose-500/30 text-rose-300","trigger_type":"manual","description":"Trigger the review"}},
  {"id":"n2","node_type":"system_prompt","position_x":320,"position_y":200,
   "data":{"label":"Reviewer Role","nodeType":"system_prompt","color":"bg-purple-500/20 border-purple-500/30 text-purple-300","prompt":"You are a senior software engineer with 15 years of experience. Conduct thorough code reviews identifying: bugs and logic errors, security vulnerabilities, performance bottlenecks, code style and readability issues, missing error handling, and opportunities to apply design patterns. Be specific and constructive.","description":"Define the reviewer persona"}},
  {"id":"n3","node_type":"user_prompt","position_x":560,"position_y":200,
   "data":{"label":"Review Code","nodeType":"user_prompt","color":"bg-blue-500/20 border-blue-500/30 text-blue-300","prompt":"Please review the following code. Provide a structured review with sections: Summary, Issues Found, Suggestions, and Verdict.\n\n```\n{{input}}\n```","temperature":0.3,"max_tokens":1024,"top_p":0.9,"description":"Generate code review"}},
  {"id":"n4","node_type":"output","position_x":800,"position_y":200,
   "data":{"label":"Review","nodeType":"output","color":"bg-red-500/20 border-red-500/30 text-red-300","variable":"last_response","format":"markdown","description":"Output the review"}}
]"#,
                r#"[
  {"id":"e1","source":"n1","target":"n2","source_handle":null,"target_handle":null},
  {"id":"e2","source":"n2","target":"n3","source_handle":null,"target_handle":null},
  {"id":"e3","source":"n3","target":"n4","source_handle":null,"target_handle":null}
]"#,
                ts, ts,
            ],
        )?;

        // ── Flow 4: REST API Data Fetcher ─────────────────────────────────────
        self.conn.execute(
            "INSERT OR IGNORE INTO flows
             (id, name, description, nodes_json, edges_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                "example-api-fetcher",
                "REST API Data Fetcher",
                "Fetch data from any REST API URL and get an AI-powered analysis of the response.",
                r#"[
  {"id":"n1","node_type":"trigger","position_x":80,"position_y":200,
   "data":{"label":"Start","nodeType":"trigger","color":"bg-rose-500/20 border-rose-500/30 text-rose-300","trigger_type":"manual","description":"Run the flow"}},
  {"id":"n2","node_type":"http_api","position_x":300,"position_y":200,
   "data":{"label":"Fetch API","nodeType":"http_api","color":"bg-cyan-500/20 border-cyan-500/30 text-cyan-300","method":"GET","url":"{{input}}","headers":"{}","body":"","content_type":"application/json","description":"Fetch data from the URL in {{input}}"}},
  {"id":"n3","node_type":"system_prompt","position_x":520,"position_y":200,
   "data":{"label":"Analyst Role","nodeType":"system_prompt","color":"bg-purple-500/20 border-purple-500/30 text-purple-300","prompt":"You are a data analyst expert in APIs and JSON. Extract and present key information from API responses in a clear, structured format with tables where appropriate.","description":"Set analyst role"}},
  {"id":"n4","node_type":"user_prompt","position_x":740,"position_y":200,
   "data":{"label":"Analyze Response","nodeType":"user_prompt","color":"bg-blue-500/20 border-blue-500/30 text-blue-300","prompt":"Analyze and summarize the following API response. Highlight the most important data, identify patterns, and present it in a readable format:\n\n{{node_n2}}","temperature":0.5,"max_tokens":768,"top_p":0.9,"description":"Analyze API data"}},
  {"id":"n5","node_type":"output","position_x":960,"position_y":200,
   "data":{"label":"Analysis","nodeType":"output","color":"bg-red-500/20 border-red-500/30 text-red-300","variable":"last_response","format":"markdown","description":"Output analysis"}}
]"#,
                r#"[
  {"id":"e1","source":"n1","target":"n2","source_handle":null,"target_handle":null},
  {"id":"e2","source":"n2","target":"n3","source_handle":null,"target_handle":null},
  {"id":"e3","source":"n3","target":"n4","source_handle":null,"target_handle":null},
  {"id":"e4","source":"n4","target":"n5","source_handle":null,"target_handle":null}
]"#,
                ts, ts,
            ],
        )?;

        // ── Flow 5: Multi-step Writing Assistant ──────────────────────────────
        self.conn.execute(
            "INSERT OR IGNORE INTO flows
             (id, name, description, nodes_json, edges_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                "example-writing-assistant",
                "Writing Assistant",
                "Draft then refine: generates a first draft and then improves it in a second AI pass.",
                r#"[
  {"id":"n1","node_type":"trigger","position_x":80,"position_y":200,
   "data":{"label":"Start","nodeType":"trigger","color":"bg-rose-500/20 border-rose-500/30 text-rose-300","trigger_type":"manual","description":"Start the writing process"}},
  {"id":"n2","node_type":"system_prompt","position_x":280,"position_y":200,
   "data":{"label":"Writer Role","nodeType":"system_prompt","color":"bg-purple-500/20 border-purple-500/30 text-purple-300","prompt":"You are an expert writer and editor. You produce clear, engaging, well-structured content. Your writing is professional yet approachable.","description":"Set writer persona"}},
  {"id":"n3","node_type":"user_prompt","position_x":480,"position_y":200,
   "data":{"label":"First Draft","nodeType":"user_prompt","color":"bg-blue-500/20 border-blue-500/30 text-blue-300","prompt":"Write a first draft for the following request:\n\n{{input}}","temperature":0.8,"max_tokens":800,"top_p":0.95,"description":"Generate first draft"}},
  {"id":"n4","node_type":"user_prompt","position_x":680,"position_y":200,
   "data":{"label":"Refine & Polish","nodeType":"user_prompt","color":"bg-indigo-500/20 border-indigo-500/30 text-indigo-300","prompt":"Improve the following draft. Fix grammar, enhance clarity, strengthen the opening and closing, and ensure a consistent tone. Return only the final polished version:\n\n{{last_response}}","temperature":0.4,"max_tokens":800,"top_p":0.9,"description":"Polish the draft"}},
  {"id":"n5","node_type":"output","position_x":880,"position_y":200,
   "data":{"label":"Final Text","nodeType":"output","color":"bg-red-500/20 border-red-500/30 text-red-300","variable":"last_response","format":"markdown","description":"Final polished output"}}
]"#,
                r#"[
  {"id":"e1","source":"n1","target":"n2","source_handle":null,"target_handle":null},
  {"id":"e2","source":"n2","target":"n3","source_handle":null,"target_handle":null},
  {"id":"e3","source":"n3","target":"n4","source_handle":null,"target_handle":null},
  {"id":"e4","source":"n4","target":"n5","source_handle":null,"target_handle":null}
]"#,
                ts, ts,
            ],
        )?;

        Ok(())
    }

    fn run_migrations(&self) -> Result<()> {
        self.conn.execute_batch(r#"
            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                model_id TEXT,
                system_prompt TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                token_count INTEGER,
                metadata TEXT,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS flows (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                nodes_json TEXT NOT NULL DEFAULT '[]',
                edges_json TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS flow_executions (
                id TEXT PRIMARY KEY,
                flow_id TEXT NOT NULL,
                status TEXT NOT NULL,
                node_results_json TEXT NOT NULL DEFAULT '[]',
                started_at TEXT NOT NULL,
                completed_at TEXT,
                FOREIGN KEY (flow_id) REFERENCES flows(id)
            );

            CREATE TABLE IF NOT EXISTS agent_tasks (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                description TEXT NOT NULL,
                status TEXT NOT NULL,
                steps_json TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL,
                completed_at TEXT,
                result TEXT
            );

            CREATE TABLE IF NOT EXISTS rag_collections (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS rag_documents (
                id TEXT PRIMARY KEY,
                collection_id TEXT NOT NULL,
                source_file TEXT NOT NULL,
                content TEXT NOT NULL,
                metadata TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                FOREIGN KEY (collection_id) REFERENCES rag_collections(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS db_connections (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                db_type TEXT NOT NULL,
                connection_string TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS artifacts (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                message_id TEXT,
                title TEXT NOT NULL,
                artifact_type TEXT NOT NULL,
                language TEXT,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_messages_conversation
                ON messages(conversation_id);
            CREATE INDEX IF NOT EXISTS idx_rag_documents_collection
                ON rag_documents(collection_id);
            CREATE INDEX IF NOT EXISTS idx_artifacts_conversation
                ON artifacts(conversation_id);

            CREATE TABLE IF NOT EXISTS artifact_edit_history (
                id TEXT PRIMARY KEY,
                artifact_id TEXT NOT NULL,
                prev_title TEXT NOT NULL,
                prev_content TEXT NOT NULL,
                edited_at TEXT NOT NULL,
                FOREIGN KEY (artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_artifact_edit_history_aid_time
                ON artifact_edit_history(artifact_id, edited_at DESC);

            CREATE TABLE IF NOT EXISTS coding_sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                mode TEXT NOT NULL DEFAULT 'agent',
                project_path TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS coding_messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                events_json TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES coding_sessions(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS coding_plans (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                message_id TEXT,
                tasks_json TEXT NOT NULL DEFAULT '[]',
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES coding_sessions(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_coding_messages_session
                ON coding_messages(session_id);
            CREATE INDEX IF NOT EXISTS idx_coding_plans_session
                ON coding_plans(session_id);

            CREATE TABLE IF NOT EXISTS comfyui_workflows (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL UNIQUE,
                description TEXT,
                workflow_json TEXT NOT NULL,
                created_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS gallery_images (
                id              TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                source          TEXT NOT NULL,
                filename        TEXT NOT NULL,
                image_data      TEXT NOT NULL,
                mime_type       TEXT NOT NULL DEFAULT 'image/png',
                prompt          TEXT,
                width           INTEGER,
                height          INTEGER,
                created_at      TEXT NOT NULL,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_gallery_conversation
                ON gallery_images(conversation_id);

            -- Add tool_steps column to existing messages rows (idempotent ALTER).
            -- SQLite ignores "duplicate column" errors via OR IGNORE is not available
            -- for ALTER TABLE, so we use a separate execute_batch line below.
        "#).context("Failed to run database migrations")?;

        // ALTER TABLE is not idempotent in SQLite — ignore "duplicate column" errors.
        let _ = self.conn.execute_batch(
            "ALTER TABLE messages ADD COLUMN tool_steps TEXT;"
        );
        let _ = self.conn.execute_batch(
            "ALTER TABLE rag_collections ADD COLUMN retrieval_mode TEXT NOT NULL DEFAULT 'hybrid';"
        );
        let _ = self.conn.execute_batch(
            "ALTER TABLE rag_collections ADD COLUMN graph_indexed INTEGER NOT NULL DEFAULT 0;"
        );
        let _ = self.conn.execute_batch(
            "ALTER TABLE conversations ADD COLUMN persona_id TEXT REFERENCES personas(id);"
        );
        let _ = self.conn.execute_batch(
            "ALTER TABLE conversations ADD COLUMN context_summary TEXT;"
        );
        let _ = self.conn.execute_batch(
            "ALTER TABLE conversations ADD COLUMN summary_up_to_rowid INTEGER NOT NULL DEFAULT 0;"
        );
        let _ = self.conn.execute_batch(
            "ALTER TABLE gallery_images ADD COLUMN file_path TEXT;"
        );

        // Personas table (idempotent)
        self.conn.execute_batch(r#"
            CREATE TABLE IF NOT EXISTS personas (
                id                   TEXT PRIMARY KEY,
                name                 TEXT NOT NULL,
                description          TEXT,
                avatar               TEXT,
                system_prompt        TEXT NOT NULL DEFAULT '',
                model_id             TEXT,
                rag_collection_ids   TEXT NOT NULL DEFAULT '[]',
                memory_enabled       INTEGER NOT NULL DEFAULT 0,
                memory_collection_id TEXT,
                created_at           TEXT NOT NULL,
                updated_at           TEXT NOT NULL
            );
        "#).context("Failed to create personas table")?;

        self.conn.execute_batch(r#"
            INSERT OR IGNORE INTO rag_collections (id, name, description, created_at)
            VALUES ('xand_internal_memory', 'Internal Memory', 'Auto-generated from conversations', datetime('now'));
        "#).context("Failed to run seed migrations")?;

        // Prompt templates table (idempotent)
        self.conn.execute_batch(r#"
            CREATE TABLE IF NOT EXISTS prompt_templates (
                id          TEXT PRIMARY KEY,
                title       TEXT NOT NULL,
                content     TEXT NOT NULL,
                description TEXT,
                category    TEXT,
                shortcut    TEXT,
                use_count   INTEGER NOT NULL DEFAULT 0,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_prompt_templates_category
                ON prompt_templates(category);

            -- Built-in starter templates (skipped if already present)
            INSERT OR IGNORE INTO prompt_templates
                (id, title, content, description, category, shortcut, use_count, created_at, updated_at)
            VALUES
                ('builtin-summarise',  'Summarise text',
                 'Please summarise the following text concisely, preserving the key points:

{{text}}',
                 'Condense long text into key points', 'Writing', '/sum', 0, datetime('now'), datetime('now')),

                ('builtin-explain',    'Explain code',
                 'Explain the following code step by step in plain English:

{{code}}',
                 'Break down code in plain English', 'Code', '/explain', 0, datetime('now'), datetime('now')),

                ('builtin-translate',  'Translate to {{language}}',
                 'Translate the following text to {{language}}. Keep the tone and formatting intact:

{{text}}',
                 'Translate text to any language', 'Writing', '/translate', 0, datetime('now'), datetime('now')),

                ('builtin-tests',      'Write unit tests for {{function}}',
                 'Write comprehensive unit tests for the {{language}} function named {{function}}. Cover edge cases, happy paths, and error conditions:

{{code}}',
                 'Generate unit tests for any function', 'Code', '/test', 0, datetime('now'), datetime('now')),

                ('builtin-report',     'Research report on {{topic}}',
                 'Write a structured research report on the topic: {{topic}}

Include:
1. Overview
2. Key findings
3. Pros and cons
4. Conclusion',
                 'Generate a structured research report', 'Research', '/report', 0, datetime('now'), datetime('now')),

                ('builtin-concise',    'Rewrite more concisely',
                 'Rewrite the following text to be more concise without losing meaning:

{{text}}',
                 'Shorten verbose text', 'Writing', '/concise', 0, datetime('now'), datetime('now'));
        "#).context("Failed to create prompt_templates table")?;

        // ── Migration: add requires column (safe no-op if already present) ──
        let _ = self.conn.execute(
            "ALTER TABLE prompt_templates ADD COLUMN requires TEXT",
            [],
        );

        // ── Built-in package-powered templates ────────────────────────────────
        // Use INSERT OR REPLACE so template content is always refreshed on
        // app launch — this ensures prompt changes in code updates actually
        // reach users who already have the old version in their DB.
        self.conn.execute_batch(r#"
            INSERT OR REPLACE INTO prompt_templates
                (id, title, content, description, category, shortcut, requires, use_count, created_at, updated_at)
            VALUES

            -- Image Generation
            ('builtin-img-portrait', 'Photorealistic portrait',
             'Generate a photorealistic portrait photo of {{subject, e.g. a young woman with red hair}}.
Style: {{style, e.g. studio lighting, golden hour, cinematic}}
Mood: {{mood, e.g. confident, serene, mysterious}}
Background: {{background, e.g. blurred bokeh studio, outdoor nature, plain white}}',
             'Generate a photorealistic person portrait', 'Creative', '/portrait',
             'ComfyUI Images', 0, datetime('now'), datetime('now')),

            ('builtin-img-cinematic', 'Cinematic scene',
             'Generate a cinematic, high-quality image of {{scene description, e.g. a futuristic city at night}}.
Lighting: {{lighting, e.g. dramatic side lighting, neon glow, golden sunset}}
Camera style: {{camera, e.g. wide angle, close-up, aerial drone shot}}
Mood: {{mood, e.g. epic, tense, peaceful, mysterious}}
Art direction: photorealistic, 4K, film grain, cinematic colour grading',
             'Generate a cinematic movie-style image', 'Creative', '/cinematic',
             'ComfyUI Images', 0, datetime('now'), datetime('now')),

            ('builtin-img-concept', 'Concept art',
             'Create detailed concept art for {{subject, e.g. a mech warrior, a fantasy castle, an alien creature}}.
Genre: {{genre, e.g. sci-fi, fantasy, cyberpunk, post-apocalyptic}}
Colour palette: {{palette, e.g. warm earth tones, cool blues and greens, vibrant neons}}
Style: digital painting, highly detailed, professional concept art, dramatic lighting',
             'Generate professional concept art', 'Creative', '/concept',
             'ComfyUI Images', 0, datetime('now'), datetime('now')),

            -- Image Editing
            ('builtin-imgedit-style', 'Artistic style transfer',
             'Edit the attached image and repaint it in the style of {{art style, e.g. Van Gogh oil painting, Japanese watercolour, anime illustration, Studio Ghibli}}.
Preserve the original subject and composition exactly. Transform only the colours, textures, and brush strokes to match the chosen style while keeping the subject fully recognisable.',
             'Repaint an image in a famous art style', 'Creative', '/style',
             'ComfyUI Image Edit', 0, datetime('now'), datetime('now')),

            ('builtin-imgedit-costume', 'Character costume redesign',
             'Edit the character in the attached image: replace their current outfit with {{new costume, e.g. cyberpunk armour with neon accents, Victorian ball gown, futuristic spacesuit}}.
Keep the character''s face, skin, pose, and body proportions exactly the same. Only change the clothing, accessories, and any related props. Maintain the same lighting and background.',
             'Swap a character outfit keeping face and pose', 'Creative', '/costume',
             'ComfyUI Image Edit', 0, datetime('now'), datetime('now')),

            ('builtin-imgedit-bg', 'Background replacement',
             'Edit the attached image and replace the background with {{new background, e.g. a sunset beach, a neon-lit cyberpunk alley, a snowy mountain range, a professional studio backdrop}}.
Keep the foreground subject perfectly intact — same lighting, edges, and details. Blend the subject naturally into the new environment.',
             'Replace image background keeping foreground intact', 'Creative', '/background',
             'ComfyUI Image Edit', 0, datetime('now'), datetime('now')),

            -- Video Generation
            ('builtin-vid-cinematic', 'Cinematic video clip',
             'Generate a short cinematic video clip of {{scene description, e.g. a spaceship flying over a ringed planet, waves crashing on a rocky coast at sunset}}.
Camera motion: {{camera motion, e.g. slow push-in, smooth pan, aerial descend}}
Style: cinematic, high quality, smooth motion, film grain
Mood: {{mood, e.g. epic and grand, calm and meditative, tense and dramatic}}',
             'Generate a high-quality cinematic video clip', 'Creative', '/video',
             'ComfyUI Video', 0, datetime('now'), datetime('now')),

            ('builtin-vid-animate', 'Animate a scene',
             'Generate a smooth looping animation of {{scene description, e.g. a bonfire in a dark forest, a waterfall with mist, leaves blowing in the wind}}.
Motion elements: {{motion, e.g. flickering fire, flowing water, swaying branches}}
Animation style: {{style, e.g. photorealistic, hand-drawn 2D, stylised 3D}}
Loop: seamless, ~3–5 seconds',
             'Generate a seamlessly looping animated scene', 'Creative', '/animate',
             'ComfyUI Video', 0, datetime('now'), datetime('now')),

            -- Jellyfin
            ('builtin-jf-search', 'Search media library',
             'Search my Jellyfin media library for {{what to find, e.g. action movies from the 90s, documentaries about space, episodes of Breaking Bad}}.
Show me the results with titles, years, and short descriptions. If there are many results, group them by type (Movies, TV Shows, etc.).',
             'Search your Jellyfin library for content', 'Media', '/media',
             'Jellyfin', 0, datetime('now'), datetime('now')),

            ('builtin-jf-recent', 'What''s recently added',
             'Show me what''s been recently added to my Jellyfin library.
Display the latest {{count, e.g. 10}} items, grouped by type (Movies, TV Shows, Music) if possible. Include titles, years, and a one-line description for each.',
             'Show recently added items in Jellyfin', 'Media', '/recent',
             'Jellyfin', 0, datetime('now'), datetime('now')),

            -- Rich Responses
            ('builtin-rich-chart', 'Visualise data as a chart',
             'Analyse the following data and visualise it as a {{chart type, e.g. line chart, bar chart, pie chart}}:

{{data — paste numbers, a table, or describe the dataset}}

Chart title: {{title}}
Label the axes clearly. Use colours that highlight trends or comparisons. Add a brief 1–2 sentence interpretation below the chart.',
             'Turn raw data into an SVG chart', 'Productivity', '/chart',
             'Rich Responses', 0, datetime('now'), datetime('now')),

            ('builtin-rich-table', 'Format data as a table',
             'Format the following data as a clean, styled table:

{{data — paste rows, CSV, or describe the dataset}}

Columns: {{columns, e.g. Name, Revenue, Growth %, Region}}
Highlight the {{highlight column, e.g. Growth % column}} to show positive values in green and negative values in red.
Add a brief summary sentence below the table.',
             'Render structured data as a styled table', 'Productivity', '/table',
             'Rich Responses', 0, datetime('now'), datetime('now')),

            ('builtin-rich-metrics', 'KPI metric cards',
             'Create a set of KPI metric cards for the following data:

{{metrics — e.g. Revenue: $1.2M (+8%), Active Users: 45,230 (+12.5%), Churn Rate: 3.1% (-0.4%)}}

For each metric show:
- A clear label
- The main value in large text
- A change indicator (green for positive, red for negative)
Display them in a responsive grid. Add a one-line summary after the cards.',
             'Display key numbers as visual metric cards', 'Productivity', '/metrics',
             'Rich Responses', 0, datetime('now'), datetime('now')),

            -- Iterative Image Refiner
            ('builtin-image-think', 'Iterative image refinement',
'You are an iterative image refinement agent. Your workflow is simple: plan → generate → inspect → edit → repeat.

## Task
{{prompt}}
Iterations: {{iterations, e.g. 3}}

## Step 1: Plan
Call `iterative_image_refiner__plan_refinement` with the prompt. Save the enhanced_prompt and negative_prompt.

## Step 2: Generate
Generate the image. Use whichever image generation tool is available:
- `iterative_image_refiner__generate_initial_image` (if configured with ComfyUI)
- `comfyui_image__generate_image` (if installed)
Pass the enhanced_prompt and negative_prompt. Save the image_url.

## Step 3: Inspect + Log
Look at the image. Rate each of the 10 checklist categories (none/minor/major/critical).
Call `iterative_image_refiner__log_iteration` with your findings.
If next_action is "edit", go to Step 4. If "accept", go to Step 5.

## Step 4: Edit
Edit the image using the corrective_prompt_hint from the log. Use whichever edit tool is available:
- `iterative_image_refiner__refine_image` (if configured)
- `comfyui_image_edit__edit_image` (if installed)
Go back to Step 3.

## Step 5: Present
Show the final image and quality score.

## Rules
- Call ONE tool per turn. Do not write code — use the tools above.
- Never skip inspection. Be honest about artifacts.
- Fix worst issues first (critical > major > minor).',
             'Generate an image and iteratively refine it by inspecting for AI artifacts (hands, text, anatomy, lighting) and editing to fix them. Uses vision-guided multi-pass refinement.', 'Creative', '/image-think',
             'Iterative Image Refiner', 0, datetime('now'), datetime('now')),

            -- Blender MCP
            ('builtin-blender', 'Blender 3D scene builder',
'You are a Blender 3D assistant driving Blender through the BlenderMCP tools. Build, modify, and inspect 3D scenes from natural-language requests.

## Your task
{{request — e.g. Create a low-poly dungeon scene with a dragon guarding a pot of gold}}

## Protocol

### Step 1: Inspect the scene first
ALWAYS call `blender_mcp__get_scene_info` before making changes, so you know what already exists. Use `blender_mcp__get_viewport_screenshot` whenever you need to see the current state visually.

### Step 2: Choose the best asset source
Check which integrations are enabled and prefer ready-made assets over scripting:
1. `blender_mcp__get_sketchfab_status` — Sketchfab has the widest variety of realistic downloadable models. Search with `search_sketchfab_models`, then `download_sketchfab_model` (always pass a real-world `target_size` in meters).
2. `blender_mcp__get_polyhaven_status` — Poly Haven is best for HDRIs (environment lighting), textures, and generic models. Use `search_polyhaven_assets` then `download_polyhaven_asset`.
3. `blender_mcp__get_hyper3d_status` / `get_hunyuan3d_status` — for custom/unique single objects not in the libraries, generate via text or images, poll the job status, then import.

### Step 3: Build — use the high-level wrapper tools (NOT raw code)
ALWAYS prefer these wrapper tools over `execute_blender_code`:
- `create_object(type, name, location, scale, ...)` — create primitives: CUBE, SPHERE, ICO_SPHERE, CYLINDER, CONE, PLANE, TORUS, CIRCLE, MONKEY
- `move_object(name, location)` — move to [x, y, z]
- `rotate_object(name, rotation, degrees=True)` — rotate to [rx, ry, rz] in degrees
- `scale_object(name, scale)` — scale to [sx, sy, sz]
- `set_material(name, color, material_name, metallic, roughness)` — apply color [R,G,B] 0-1
- `delete_object(name)` — delete by name
- `rename_object(old_name, new_name)` — rename
- `duplicate_object(name, new_name, offset)` — copy with offset
- `join_objects(names, result_name)` — merge objects
- `clear_scene(keep_camera, keep_lights)` — remove all objects
- `add_modifier(name, modifier_type, levels)` — SUBSURF, MIRROR, ARRAY, etc.
- `set_origin(name, origin_type)` — GEOMETRY, CURSOR, BOUNDS

**Complex objects** — build multi-part shapes in a SINGLE call:
- `create_compound_object(name, parts, join, color, location)` — assemble multiple primitives at once. Each part: {type, name, location, rotation, scale, size, radius, depth, color}. Optionally join + color all parts.
- `create_text_3d(text, name, size, extrude, color, ...)` — 3D text with font, alignment, color
- `create_array(source_name, pattern, count, offset, radius, merge)` — LINEAR, GRID, or CIRCULAR copies

Only use `execute_blender_code` for operations that the wrappers cannot do (custom geometry, node setups, animation, etc.).

#### Example 1: simple object (use individual tools)
Call 1: `create_object(type="CYLINDER", name="Fuselage", radius=0.5, depth=3)`
Call 2: `rotate_object(name="Fuselage", rotation=[90, 0, 0])`
Call 3: `set_material(name="Fuselage", color=[1, 0, 0])`

#### Example 2: complex object (use create_compound_object — preferred for multi-part objects)
ONE call: `create_compound_object(name="Airplane", parts=[{"type":"CYLINDER","name":"Fuselage","radius":0.3,"depth":4,"rotation":[0,90,0]}, {"type":"PLANE","name":"Wings","scale":[3,0.5,1]}, {"type":"CONE","name":"Nose","radius":0.3,"depth":0.8,"location":[2.4,0,0],"rotation":[0,90,0]}], join=True, color=[0.2,0.4,0.8])`

#### Rules for building
- For multi-part objects, ALWAYS prefer `create_compound_object` over individual `create_object` calls.
- Verify with `get_scene_info` after building.
- NEVER reference object names you did not create or confirm via `get_scene_info`.
- Downloaded/generated assets (Sketchfab, Poly Haven, Hyper3D, Hunyuan3D) are imported by their own tools.

### Step 4: Verify spatial layout
Check each object''s bounding box via `blender_mcp__get_object_info` and adjust with `move_object`/`scale_object` so nothing clips.

### Step 5: Show the result
Capture a final `blender_mcp__get_viewport_screenshot` and summarize what you built.

## Rules
- Inspect before you act; never assume the scene is empty.
- Prefer downloaded/generated assets over hand-scripted geometry unless a simple primitive is explicitly requested.
- ALWAYS prefer wrapper tools (create_object, move_object, etc.) over execute_blender_code.
- Requires Blender running with the BlenderMCP addon and "Connect to Claude" enabled.',
             'Build and edit 3D scenes in Blender: create objects, apply materials, run bpy code, and pull assets from Sketchfab, Poly Haven, Hyper3D, and Hunyuan3D.', 'Creative', '/blender',
             'Blender MCP', 0, datetime('now'), datetime('now'));
        "#).context("Failed to seed package templates")?;

        // ── Built-in Browser Agent templates ──────────────────────────────────
        // Kept in a separate batch so adding/updating them doesn't risk
        // breaking the larger package-template seed above. `INSERT OR IGNORE`
        // makes this a safe no-op on every subsequent launch.
        //
        // SQLite string-literal escaping note
        // ────────────────────────────────────
        // Inside a single-quoted SQL string, a literal apostrophe must be
        // doubled (`'` -> `''`). The `content` column for WhatsApp therefore
        // uses `''` for every English contraction ("person''s", "don''t").
        self.conn.execute_batch(r#"
            INSERT OR REPLACE INTO prompt_templates
                (id, title, content, description, category, shortcut, requires, use_count, created_at, updated_at)
            VALUES

            ('builtin-whatsapp-agent', 'WhatsApp Web Agent',
'You are operating WhatsApp Web (https://web.whatsapp.com) inside the Browser Agent tab.

## Page layout
- LEFT SIDEBAR: list of recent conversations. Each row shows a contact photo, the contact/group name, a preview of the last message, and a timestamp. CLICKING a row OPENS that chat in the right panel.
- RIGHT PANEL: the currently-open chat. Messages appear here; a text input ("Type a message") sits at the very bottom.
- SEARCH BAR: at the top of the left sidebar ("Search or start a new chat" / "Pesquisar ou comecar uma nova conversa"). Use it to find a contact by name when the contact is not visible in the sidebar.

## How to open a chat (MANDATORY)
1. Take a `browser_agent__snapshot` first so you can see the indexed interactive nodes.
2. Look in the LEFT SIDEBAR for the contact name. If you find it, CLICK ONCE on that conversation row. One click is enough to open the chat — do NOT double-click.
3. If the contact is NOT visible in the first screen of the sidebar, DO NOT scroll the sidebar more than twice. Switch to search instead:
   a. Click the SEARCH input at the top of the sidebar.
   b. Type the contact name with `browser_agent__type`.
   c. Wait ~500ms, take a new snapshot, and click the matching result row.
4. Confirm success by taking a snapshot and verifying the RIGHT PANEL header shows the contact name and the message history is visible.

## How to send a message
1. Confirm the correct chat is open (right-panel header matches the contact).
2. Click the message input field at the BOTTOM of the right panel (placeholder "Type a message" / "Digite uma mensagem"). This element is a `contenteditable` textbox — the snapshot exposes it with role `textbox`.
3. Type the message with `browser_agent__type`.
4. Press Enter (`browser_agent__press_key` with key "Enter") OR click the send button.
5. Take a snapshot to verify the sent message appears in the chat history.

## Hard rules
- NEVER scroll the left sidebar more than 2 times without finding the contact — switch to search.
- NEVER try to navigate to a per-chat URL (e.g. `/chat/<id>`). WhatsApp Web uses client-side routing; only `https://web.whatsapp.com/` is valid entry point.
- If you see a QR code, a phone-number prompt, or the "Download WhatsApp" page, the session is NOT logged in. Stop immediately and tell the user to scan the QR code in the Browser Agent viewport, then start a new task.
- After every click / type / press_key, take a snapshot to confirm the expected change actually happened before issuing the next action.
- Messages you send on behalf of the user are real and cannot be unsent after a short window — double-check the contact and the message body in the snapshot before pressing Enter.',
             'Step-by-step instructions for the browser agent to navigate and send messages on WhatsApp Web. Pair with a named browser session (e.g. "whatsapp") so the QR scan persists.',
             'Browser Agent', '/whatsapp', NULL, 0, datetime('now'), datetime('now'));
        "#).context("Failed to seed browser-agent templates")?;

        Ok(())
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn
            .prepare("SELECT value FROM settings WHERE key = ?1")?;
        let result = stmt
            .query_row(params![key], |row| row.get(0))
            .optional()?;
        Ok(result)
    }
}

trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for rusqlite::Result<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
