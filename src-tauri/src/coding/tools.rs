use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

// ── Tool definitions ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: &'static str,
}

pub const AGENT_TOOLS: &[ToolDef] = &[
    ToolDef {
        name: "directory_tree",
        description: "List files and folders in the project recursively. Use this first to understand the project structure.",
        parameters: r#"{"path": "string (optional) - relative subpath, defaults to project root", "depth": "number (optional, default 3) - max depth"}"#,
    },
    ToolDef {
        name: "file_read",
        description: "Read the contents of a file in the project.",
        parameters: r#"{"path": "string - relative path to the file"}"#,
    },
    ToolDef {
        name: "file_write",
        description: "Write or overwrite content to a file. Creates parent directories if needed.",
        parameters: r#"{"path": "string - relative path", "content": "string - file content"}"#,
    },
    ToolDef {
        name: "file_patch",
        description: "Apply a targeted find-and-replace patch to a file. Use for editing specific parts of a file without rewriting it entirely.",
        parameters: r#"{"path": "string - relative path", "old_str": "string - exact text to find", "new_str": "string - replacement text"}"#,
    },
    ToolDef {
        name: "grep",
        description: "Search for a pattern in files. Returns matching file lines with context.",
        parameters: r#"{"pattern": "string - search pattern (regex supported)", "path": "string (optional) - relative dir/file to search in", "case_insensitive": "boolean (optional)"}"#,
    },
    ToolDef {
        name: "shell_exec",
        description: "Execute a shell command in the project directory. Use for building, testing, installing dependencies, or running scripts.",
        parameters: r#"{"command": "string - shell command to run", "timeout_seconds": "number (optional, default 30)"}"#,
    },
    ToolDef {
        name: "web_search",
        description: "Search the web for current documentation, error solutions, or best practices.",
        parameters: r#"{"query": "string - the search query"}"#,
    },
    ToolDef {
        name: "create_plan",
        description: "Create a structured plan with tasks. Call this when you have analyzed the codebase and know the steps needed.",
        parameters: r#"{"title": "string - plan title", "tasks": "array of {title, description} objects"}"#,
    },
    ToolDef {
        name: "update_task",
        description: "Update the status of a task in the current plan.",
        parameters: r#"{"task_index": "number - 0-based index", "status": "in_progress|completed|failed", "note": "string (optional)"}"#,
    },
];

pub const ASK_TOOLS: &[ToolDef] = &[
    ToolDef {
        name: "directory_tree",
        description: "List files and folders in the project recursively.",
        parameters: r#"{"path": "string (optional)", "depth": "number (optional, default 3)"}"#,
    },
    ToolDef {
        name: "file_read",
        description: "Read the contents of a file in the project.",
        parameters: r#"{"path": "string - relative path to the file"}"#,
    },
    ToolDef {
        name: "grep",
        description: "Search for a pattern in files.",
        parameters: r#"{"pattern": "string", "path": "string (optional)", "case_insensitive": "boolean (optional)"}"#,
    },
];

pub const PLAN_TOOLS: &[ToolDef] = &[
    ToolDef {
        name: "directory_tree",
        description: "List files and folders in the project recursively.",
        parameters: r#"{"path": "string (optional)", "depth": "number (optional, default 3)"}"#,
    },
    ToolDef {
        name: "file_read",
        description: "Read the contents of a file in the project.",
        parameters: r#"{"path": "string - relative path to the file"}"#,
    },
    ToolDef {
        name: "grep",
        description: "Search for a pattern in files.",
        parameters: r#"{"pattern": "string", "path": "string (optional)", "case_insensitive": "boolean (optional)"}"#,
    },
    ToolDef {
        name: "create_plan",
        description: "Create a structured plan with actionable tasks based on your analysis.",
        parameters: r#"{"title": "string - plan title", "tasks": "array of {title, description} objects"}"#,
    },
];

pub const DEBUG_TOOLS: &[ToolDef] = &[
    ToolDef {
        name: "directory_tree",
        description: "List files and folders.",
        parameters: r#"{"path": "string (optional)", "depth": "number (optional, default 3)"}"#,
    },
    ToolDef {
        name: "file_read",
        description: "Read a file to find the source of an error.",
        parameters: r#"{"path": "string - relative path to the file"}"#,
    },
    ToolDef {
        name: "grep",
        description: "Search for error messages, function names, or patterns.",
        parameters: r#"{"pattern": "string", "path": "string (optional)", "case_insensitive": "boolean (optional)"}"#,
    },
    ToolDef {
        name: "shell_exec",
        description: "Run tests, linters, or type-checkers to reproduce errors.",
        parameters: r#"{"command": "string - command to run", "timeout_seconds": "number (optional, default 30)"}"#,
    },
    ToolDef {
        name: "file_patch",
        description: "Apply a targeted fix to a file.",
        parameters: r#"{"path": "string", "old_str": "string", "new_str": "string"}"#,
    },
    ToolDef {
        name: "web_search",
        description: "Search for solutions to error messages or bugs.",
        parameters: r#"{"query": "string"}"#,
    },
];

pub fn tools_as_text(tools: &[ToolDef]) -> String {
    tools
        .iter()
        .map(|t| format!("- {}: {}\n  Parameters: {}", t.name, t.description, t.parameters))
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ── Tool executor ─────────────────────────────────────────────────────────────

pub struct CodingToolExecutor {
    pub project_root: PathBuf,
    /// Canonicalized root used for traversal checks (resolves UNC prefix on Windows).
    canonical_root: PathBuf,
}

impl CodingToolExecutor {
    pub fn new(project_root: PathBuf) -> Self {
        // Canonicalize once at construction time so all comparisons are consistent.
        let canonical_root = project_root.canonicalize().unwrap_or_else(|_| project_root.clone());
        Self { project_root, canonical_root }
    }

    fn safe_path(&self, relative: &str) -> Result<PathBuf> {
        if relative.is_empty() || relative == "." {
            return Ok(self.project_root.clone());
        }
        // Strip leading slashes / backslashes
        let stripped = relative.trim_start_matches('/').trim_start_matches('\\');
        let joined = self.project_root.join(stripped);

        // Walk up the ancestor chain until we find a path component that
        // actually exists on disk, canonicalize that, then re-attach the
        // remaining suffix.  This handles deeply-nested new paths (e.g.
        // new_dir/subdir/file.py) where intermediate directories don't exist
        // yet — the old single-level parent fallback produced a non-UNC path
        // that failed the starts_with check against the canonical root on Windows.
        let canonical_target = {
            let mut check: &Path = joined.as_path();
            loop {
                if check.exists() {
                    let canon = check.canonicalize().unwrap_or_else(|_| check.to_path_buf());
                    // Reconstruct by appending the portion that doesn't exist yet
                    let suffix = joined.strip_prefix(check).unwrap_or(Path::new(""));
                    break canon.join(suffix);
                }
                match check.parent() {
                    Some(p) if p != check => check = p,
                    // Reached filesystem root without finding an existing ancestor
                    _ => break joined.clone(),
                }
            }
        };

        if !canonical_target.starts_with(&self.canonical_root) {
            anyhow::bail!("Path traversal not allowed: {}", relative);
        }
        Ok(joined)
    }

    pub async fn execute(&self, tool: &str, input: &Value) -> Result<Value> {
        match tool {
            "directory_tree" => {
                let path = input["path"].as_str().unwrap_or(".");
                let depth = input["depth"].as_u64().unwrap_or(3) as usize;
                let root = self.safe_path(path)?;
                let tree = build_tree(&root, &self.project_root, depth, 0)?;
                Ok(serde_json::json!({ "tree": tree, "root": path }))
            }
            "file_read" => {
                let path = input["path"].as_str().context("path required")?;
                let full = self.safe_path(path)?;
                let content = tokio::fs::read_to_string(&full)
                    .await
                    .with_context(|| format!("Cannot read: {}", path))?;
                Ok(serde_json::json!({ "path": path, "content": content, "lines": content.lines().count() }))
            }
            "file_write" => {
                // Primary extraction from parsed JSON object.
                // Fallback: when JSON parsing upstream failed (input is a raw string),
                // use targeted field extraction so a single broken encoding doesn't
                // prevent the file from being written.
                let (path_str, content_str) = match (input["path"].as_str(), input["content"].as_str()) {
                    (Some(p), Some(c)) => (p.to_string(), c.to_string()),
                    _ => {
                        if let Value::String(raw) = input {
                            extract_file_write_fields(raw)?
                        } else {
                            anyhow::bail!(
                                "JSON parse failed for file_write. \
                                 Ensure Action Input is a single-line JSON object: \
                                 {{\"path\": \"relative/path.py\", \"content\": \"line1\\nline2\"}}. \
                                 Escape ALL double-quotes inside content as \\\", \
                                 and use \\n for newlines — do NOT use real newline characters."
                            );
                        }
                    }
                };
                let full = self.safe_path(&path_str)?;
                if let Some(parent) = full.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::write(&full, &content_str).await
                    .with_context(|| format!("Cannot write: {}", path_str))?;
                Ok(serde_json::json!({ "path": path_str, "bytes_written": content_str.len(), "success": true }))
            }
            "file_patch" => {
                let path = input["path"].as_str().context("path required")?;
                let old_str = input["old_str"].as_str().context("old_str required")?;
                let new_str = input["new_str"].as_str().context("new_str required")?;
                let full = self.safe_path(path)?;
                let original = tokio::fs::read_to_string(&full)
                    .await
                    .with_context(|| format!("Cannot read: {}", path))?;
                if !original.contains(old_str) {
                    anyhow::bail!("old_str not found in file: {}", path);
                }
                let patched = original.replacen(old_str, new_str, 1);
                tokio::fs::write(&full, &patched).await
                    .with_context(|| format!("Cannot write: {}", path))?;
                Ok(serde_json::json!({ "path": path, "success": true, "message": "Patch applied" }))
            }
            "grep" => {
                let pattern = input["pattern"].as_str().context("pattern required")?;
                let search_path = input["path"].as_str().unwrap_or(".");
                let case_insensitive = input["case_insensitive"].as_bool().unwrap_or(false);
                let target = self.safe_path(search_path)?;
                let results = grep_files(&target, pattern, case_insensitive, 50).await?;
                Ok(serde_json::json!({ "pattern": pattern, "matches": results }))
            }
            "shell_exec" => {
                let command = input["command"].as_str().context("command required")?;
                let timeout_secs = input["timeout_seconds"].as_u64().unwrap_or(30);
                shell_exec(command, &self.project_root, timeout_secs).await
            }
            "web_search" => {
                let query = input["query"].as_str().unwrap_or("");
                crate::agent::tools::WebSearchTool::new().search(query).await
            }
            "create_plan" | "update_task" => {
                // These are handled specially in the runtime to update plan state.
                // Return success so the runtime can parse the plan from the input.
                Ok(serde_json::json!({ "success": true, "tool": tool, "input": input }))
            }
            other => anyhow::bail!("Unknown tool: {}", other),
        }
    }
}

// ── Directory tree builder ─────────────────────────────────────────────────────

fn should_skip(name: &str) -> bool {
    matches!(
        name,
        "node_modules" | ".git" | "target" | "dist" | "build" | ".next"
            | "__pycache__" | ".pytest_cache" | "venv" | ".venv" | ".cache"
            | "coverage" | ".nyc_output"
    )
}

fn build_tree(dir: &Path, root: &Path, max_depth: usize, current_depth: usize) -> Result<Vec<Value>> {
    if current_depth >= max_depth {
        return Ok(vec![]);
    }

    let mut entries = vec![];
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return Ok(vec![]),
    };

    let mut items: Vec<_> = read.filter_map(|e| e.ok()).collect();
    items.sort_by_key(|e| {
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        (!is_dir, e.file_name())
    });

    for entry in items {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && current_depth > 0 {
            continue; // skip hidden files after root
        }
        if should_skip(&name) {
            continue;
        }

        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let rel_path = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(&entry.path())
            .to_string_lossy()
            .replace('\\', "/");

        if is_dir {
            let children = build_tree(&entry.path(), root, max_depth, current_depth + 1)?;
            entries.push(serde_json::json!({
                "name": name,
                "path": rel_path,
                "type": "directory",
                "children": children
            }));
        } else {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            entries.push(serde_json::json!({
                "name": name,
                "path": rel_path,
                "type": "file",
                "size": size
            }));
        }
    }

    Ok(entries)
}

// ── Grep ──────────────────────────────────────────────────────────────────────

async fn grep_files(
    root: &Path,
    pattern: &str,
    case_insensitive: bool,
    max_results: usize,
) -> Result<Vec<Value>> {
    let regex = if case_insensitive {
        regex::RegexBuilder::new(pattern)
            .case_insensitive(true)
            .build()
            .context("Invalid regex pattern")?
    } else {
        regex::Regex::new(pattern).context("Invalid regex pattern")?
    };

    let mut results = vec![];
    grep_walk(root, root, &regex, &mut results, max_results)?;
    Ok(results)
}

fn grep_walk(
    dir: &Path,
    root: &Path,
    regex: &regex::Regex,
    results: &mut Vec<Value>,
    max_results: usize,
) -> Result<()> {
    if results.len() >= max_results {
        return Ok(());
    }

    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    for entry in read.filter_map(|e| e.ok()) {
        if results.len() >= max_results {
            break;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if should_skip(&name) || name.starts_with('.') {
            continue;
        }
        let ftype = entry.file_type().unwrap_or_else(|_| {
            entry.metadata().map(|m| m.file_type()).unwrap()
        });
        if ftype.is_dir() {
            grep_walk(&entry.path(), root, regex, results, max_results)?;
        } else if ftype.is_file() {
            if let Ok(content) = std::fs::read_to_string(&entry.path()) {
                let rel_path = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap_or(&entry.path())
                    .to_string_lossy()
                    .replace('\\', "/");
                for (i, line) in content.lines().enumerate() {
                    if regex.is_match(line) {
                        results.push(serde_json::json!({
                            "file": rel_path,
                            "line": i + 1,
                            "content": line.trim()
                        }));
                        if results.len() >= max_results {
                            break;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

// ── Shell exec ─────────────────────────────────────────────────────────────────

async fn shell_exec(command: &str, cwd: &Path, timeout_secs: u64) -> Result<Value> {
    std::fs::create_dir_all(cwd).ok();

    let child = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", command])
            .current_dir(cwd)
            .output()
    } else {
        Command::new("sh")
            .args(["-c", command])
            .current_dir(cwd)
            .output()
    };

    let result = timeout(Duration::from_secs(timeout_secs), child)
        .await
        .context("Command timed out")?
        .context("Failed to execute command")?;

    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    let exit_code = result.status.code().unwrap_or(-1);

    Ok(serde_json::json!({
        "command": command,
        "stdout": stdout,
        "stderr": stderr,
        "exit_code": exit_code,
        "success": result.status.success()
    }))
}

// ── JSON field extraction fallback ───────────────────────────────────────────
//
// When all upstream JSON parsing has failed and `file_write` receives a
// `Value::String(raw)`, we attempt a targeted extraction of the `path` and
// `content` fields without relying on a valid JSON parse.
//
// Strategy:
//   • `path`    — simple file path, no `"` in it; find `"path"` key, read until
//                 the next unescaped `"` (reliable and unambiguous).
//   • `content` — everything between the opening `"` after `"content":` and the
//                 very last `"` in the string (which is the closing quote of the
//                 content value in the JSON object).  Then JSON-unescape it.

fn extract_file_write_fields(raw: &str) -> Result<(String, String)> {
    let path = extract_simple_string_field(raw, "path")
        .ok_or_else(|| anyhow::anyhow!(
            "Could not extract 'path' from Action Input. \
             Use this exact format on ONE line: \
             {{\"path\": \"relative/path.py\", \"content\": \"content with \\\\n for newlines\"}}"
        ))?;

    let content = extract_content_field(raw).unwrap_or_default();
    Ok((path, content))
}

/// Extract a simple string field (no embedded `"`) from a possibly-malformed JSON string.
fn extract_simple_string_field(s: &str, field: &str) -> Option<String> {
    // Find `"field"` then `:` then `"value"`
    let key = format!("\"{}\"", field);
    let key_pos = s.find(&key)?;
    let after_key = s[key_pos + key.len()..].trim_start();
    let after_colon = after_key.strip_prefix(':')?.trim_start();
    let value_start = after_colon.strip_prefix('"')?;
    // Value ends at the next unescaped `"` — file paths never contain `"`
    let end = value_start.find('"')?;
    Some(value_start[..end].to_string())
}

/// Extract the `content` value from a possibly-malformed JSON string.
/// Takes everything from the first `"` after `"content":` to the last `"` in `s`,
/// then JSON-unescapes the result.
fn extract_content_field(s: &str) -> Option<String> {
    let key_pos = s.find("\"content\"")?;
    let after_key = s[key_pos + "\"content\"".len()..].trim_start();
    let after_colon = after_key.strip_prefix(':')?.trim_start();
    let value_start = after_colon.strip_prefix('"')?;

    // The last `"` in the entire remaining string is the closing quote of the content value.
    let end = value_start.rfind('"').unwrap_or(value_start.len());
    let raw_content = &value_start[..end];
    Some(json_unescape(raw_content))
}

/// Unescape JSON string escape sequences: `\n`, `\r`, `\t`, `\"`, `\\`.
fn json_unescape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n')  => result.push('\n'),
                Some('r')  => result.push('\r'),
                Some('t')  => result.push('\t'),
                Some('"')  => result.push('"'),
                Some('\\') => result.push('\\'),
                Some('/')  => result.push('/'),
                Some(c)    => { result.push('\\'); result.push(c); }
                None       => break,
            }
        } else {
            result.push(ch);
        }
    }
    result
}
