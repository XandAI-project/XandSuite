/// Startup helper: reads `tools/registry.json` and connects all enabled
/// built-in MCP servers, then loads any user-added servers from the DB.

use anyhow::Result;
use std::path::PathBuf;

use crate::paths::resolve_tools_dir;
use crate::skills::{McpServerConfig, McpTransport, SkillsManager};

pub async fn connect_builtin_servers(
    skills: &SkillsManager,
    _data_dir: &PathBuf,
) -> Result<()> {
    // tools/ directory lives next to the app binary (bundled) or at
    // <project_root>/tools during development.
    let tools_dir = resolve_tools_dir();

    let registry_path = tools_dir.join("registry.json");
    if !registry_path.exists() {
        log::warn!(
            "MCP registry not found at {:?}. Skipping builtin tool servers.",
            registry_path
        );
        return Ok(());
    }

    let registry_text = std::fs::read_to_string(&registry_path)?;
    let registry: serde_json::Value = serde_json::from_str(&registry_text)?;

    let tools_dir_str = tools_dir.to_string_lossy().to_string();

    if let Some(servers) = registry.get("servers").and_then(|v| v.as_array()) {
        for entry in servers {
            let enabled = entry.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
            if !enabled {
                continue;
            }

            let id = entry.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or(&id).to_string();
            let description = entry.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let icon = entry.get("icon").and_then(|v| v.as_str()).unwrap_or("Wrench").to_string();
            let transport_type = entry.get("transport").and_then(|v| v.as_str()).unwrap_or("stdio");

            let transport = match transport_type {
                "http" => {
                    let url = entry.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    McpTransport::Http { url, auth: None }
                }
                _ => {
                    // registry.json hardcodes "python" for every builtin server.
                    // On Linux/macOS that binary frequently doesn't exist (only
                    // `python3` is installed), which silently broke every
                    // built-in tool (web search, calculator, file ops, code
                    // runner). Substitute the probed interpreter whenever the
                    // registry says the plain "python" default; an explicit,
                    // non-default command (e.g. a future non-Python server)
                    // is left untouched.
                    let raw_command = entry.get("command").and_then(|v| v.as_str()).unwrap_or("python");
                    let command = if raw_command == "python" {
                        crate::commands::packages::resolve_python().to_string()
                    } else {
                        raw_command.to_string()
                    };
                    let args: Vec<String> = entry
                        .get("args")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|a| a.as_str())
                                .map(|a| a.replace("${TOOLS_DIR}", &tools_dir_str))
                                .collect()
                        })
                        .unwrap_or_default();
                    McpTransport::Stdio { command, args }
                }
            };

            let cfg = McpServerConfig {
                id: id.clone(),
                name,
                description,
                transport,
                builtin: true,
                enabled: true,
                icon,
            };

            if let Err(e) = skills.connect_server(cfg).await {
                log::warn!("Failed to connect builtin MCP server '{}': {}", id, e);
            }
        }
    }

    Ok(())
}
