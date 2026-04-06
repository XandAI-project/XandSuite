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
