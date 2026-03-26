pub mod web_search;
pub mod code_exec;
pub mod file_ops;
pub mod db_query;
pub mod http_api;


pub use web_search::WebSearchTool;
pub use code_exec::CodeExecTool;
pub use file_ops::FileOpsTool;
pub use db_query::DbQueryTool;
pub use http_api::HttpApiTool;

#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters_schema: &'static str,
}

pub const TOOL_DEFINITIONS: &[ToolDefinition] = &[
    ToolDefinition {
        name: "web_search",
        description: "Search the web for current information. Returns a list of results with title, URL, and snippet.",
        parameters_schema: r#"{"query": "string - the search query"}"#,
    },
    ToolDefinition {
        name: "code_exec",
        description: "Execute a shell command or script. Use for computations, file transformations, or system tasks.",
        parameters_schema: r#"{"command": "string - shell command to run", "timeout_seconds": "number (optional, default 30)"}"#,
    },
    ToolDefinition {
        name: "file_read",
        description: "Read the contents of a file from the workspace.",
        parameters_schema: r#"{"path": "string - relative path to the file"}"#,
    },
    ToolDefinition {
        name: "file_write",
        description: "Write content to a file in the workspace.",
        parameters_schema: r#"{"path": "string - relative path", "content": "string - file content"}"#,
    },
    ToolDefinition {
        name: "db_query",
        description: "Execute a query against a connected database. Specify connection_id and query.",
        parameters_schema: r#"{"connection_id": "string", "query": "string"}"#,
    },
    ToolDefinition {
        name: "http_api",
        description: "Make an HTTP request to an external API. Supports GET, POST, PUT, DELETE.",
        parameters_schema: r#"{"method": "GET|POST|PUT|DELETE", "url": "string", "headers": "object (optional)", "body": "string (optional)"}"#,
    },
];

pub fn tool_definitions_as_text() -> String {
    TOOL_DEFINITIONS
        .iter()
        .map(|t| format!("- {}: {}\n  Parameters: {}", t.name, t.description, t.parameters_schema))
        .collect::<Vec<_>>()
        .join("\n\n")
}
