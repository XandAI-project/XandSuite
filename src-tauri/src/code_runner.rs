/// Sandboxed code execution for the LLM tool loop.
///
/// Supports Python, JavaScript (Node.js), and Shell (PowerShell / bash).
/// Each invocation writes code to a temp file, runs it in a child process,
/// captures stdout/stderr (capped at 8 KiB each), and cleans up on exit.
///
/// The `list_recent_artifacts` helper lets the LLM inspect previously
/// generated code artifacts in the current conversation.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::process_ext::HideWindowTokio;
use tokio::time::timeout;

use crate::db::AppDb;

const MAX_OUTPUT_BYTES: usize = 8_192;
const EXEC_TIMEOUT_SECS: u64 = 30;

// ── Public result types ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct CodeRunResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub execution_time_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ArtifactSummary {
    pub id: String,
    pub title: String,
    pub artifact_type: String,
    pub language: Option<String>,
    /// First 2 KiB of content — enough context for the LLM without flooding its window.
    pub content_preview: String,
    pub created_at: String,
}

// ── Execution ─────────────────────────────────────────────────────────────────

/// Run `code` in the given `language` inside a temporary file.
///
/// Returns stdout, stderr, exit code, and wall-clock time in milliseconds.
/// Errors only for infrastructure failures (e.g. interpreter not found);
/// non-zero exit codes are reported inside `CodeRunResult`.
pub async fn execute_code(language: &str, code: &str) -> Result<CodeRunResult> {
    let lang = language.to_lowercase();

    // Write code to a named temp file with the right extension
    let (tmp_path, child) = match lang.as_str() {
        "python" | "python3" => {
            let path = write_temp_file(code, "py").await?;
            let cmd = spawn_python(&path).await?;
            (path, cmd)
        }
        "javascript" | "js" | "node" => {
            let path = write_temp_file(code, "js").await?;
            let cmd = spawn_node(&path).await?;
            (path, cmd)
        }
        "shell" | "bash" | "sh" | "powershell" | "ps1" => {
            let path = write_temp_file(code, shell_extension()).await?;
            let cmd = spawn_shell(&path).await?;
            (path, cmd)
        }
        "html" | "css" | "htm" => bail!(
            "HTML/CSS cannot be executed in a terminal. \
             Write the HTML directly inside an artifact tag instead:\n\
             <artifact type=\"html\" title=\"Page Title\">\n\
             <!DOCTYPE html>...\n\
             </artifact>\n\
             Do NOT call execute_code for HTML."
        ),
        other => bail!("Unsupported language: '{}'. Supported: python, javascript, shell.", other),
    };

    let t0 = Instant::now();

    let result = timeout(
        Duration::from_secs(EXEC_TIMEOUT_SECS),
        collect_output(child),
    )
    .await;

    // Clean up temp file regardless of outcome
    let _ = tokio::fs::remove_file(&tmp_path).await;

    match result {
        Ok(Ok(run)) => {
            let elapsed = t0.elapsed().as_millis() as u64;
            Ok(CodeRunResult {
                stdout: truncate(run.stdout, MAX_OUTPUT_BYTES),
                stderr: truncate(run.stderr, MAX_OUTPUT_BYTES),
                exit_code: run.exit_code,
                execution_time_ms: elapsed,
            })
        }
        Ok(Err(e)) => Err(e),
        Err(_timeout) => {
            Ok(CodeRunResult {
                stdout: String::new(),
                stderr: format!(
                    "Execution timed out after {} seconds.",
                    EXEC_TIMEOUT_SECS
                ),
                exit_code: -1,
                execution_time_ms: EXEC_TIMEOUT_SECS * 1000,
            })
        }
    }
}

// ── Artifact listing ──────────────────────────────────────────────────────────

/// Return the most recent `limit` code/text artifacts for `conversation_id`.
pub fn list_recent_artifacts(
    db: &Arc<Mutex<AppDb>>,
    conversation_id: &str,
    limit: usize,
) -> Result<Vec<ArtifactSummary>> {
    let db = db.lock().map_err(|e| anyhow::anyhow!("DB lock: {}", e))?;
    let mut stmt = db.conn.prepare(
        "SELECT id, title, artifact_type, language, content, created_at
         FROM artifacts
         WHERE conversation_id = ?1
         ORDER BY created_at DESC
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(
        rusqlite::params![conversation_id, limit as i64],
        |row| {
            let content: String = row.get(4)?;
            let preview = content.chars().take(2048).collect::<String>();
            Ok(ArtifactSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                artifact_type: row.get(2)?,
                language: row.get(3)?,
                content_preview: preview,
                created_at: row.get(5)?,
            })
        },
    )?;

    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

// ── Internal helpers ──────────────────────────────────────────────────────────

struct RawOutput {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

async fn collect_output(mut child: tokio::process::Child) -> Result<RawOutput> {
    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();

    if let Some(mut so) = child.stdout.take() {
        so.read_to_end(&mut stdout_buf).await?;
    }
    if let Some(mut se) = child.stderr.take() {
        se.read_to_end(&mut stderr_buf).await?;
    }

    let status = child.wait().await?;
    let exit_code = status.code().unwrap_or(-1);

    Ok(RawOutput {
        stdout: String::from_utf8_lossy(&stdout_buf).into_owned(),
        stderr: String::from_utf8_lossy(&stderr_buf).into_owned(),
        exit_code,
    })
}

async fn write_temp_file(code: &str, ext: &str) -> Result<std::path::PathBuf> {
    let dir = std::env::temp_dir();
    let name = format!("xandsuite_run_{}.{}", uuid_short(), ext);
    let path = dir.join(name);
    tokio::fs::write(&path, code).await?;
    Ok(path)
}

fn uuid_short() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("{:x}", ns)
}

async fn spawn_python(path: &std::path::Path) -> Result<tokio::process::Child> {
    // Try `python3` first, then `python`.
    let interpreters = ["python3", "python"];
    for interp in &interpreters {
        let mut cmd = Command::new(interp);
        cmd.hide_window();
        cmd.arg("-u")
            .arg(path)
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if let Ok(child) = cmd.spawn() {
            return Ok(child);
        }
    }
    bail!("Python interpreter not found. Please install Python 3 and ensure it is on your PATH.")
}

async fn spawn_node(path: &std::path::Path) -> Result<tokio::process::Child> {
    let mut cmd = Command::new("node");
    cmd.hide_window();
    cmd.arg(path)
        .env("NODE_ICU_DATA", "")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    cmd.spawn()
        .map_err(|_| anyhow::anyhow!("Node.js interpreter not found. Please install Node.js and ensure it is on your PATH."))
}

async fn spawn_shell(path: &std::path::Path) -> Result<tokio::process::Child> {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("powershell");
        cmd.hide_window();
        cmd.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
                     [Console]::InputEncoding  = [System.Text.Encoding]::UTF8; \
                     & '{}'",
                    path.to_string_lossy().replace('\'', "''")
                ),
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        cmd.spawn()
            .map_err(|e| anyhow::anyhow!("Failed to launch PowerShell: {}", e))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("bash")
            .arg(path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to launch bash: {}", e))
    }
}

fn shell_extension() -> &'static str {
    #[cfg(target_os = "windows")]
    { "ps1" }
    #[cfg(not(target_os = "windows"))]
    { "sh" }
}

fn truncate(s: String, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s;
    }
    // Truncate at a char boundary
    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n... [output truncated at {} bytes]",
        &s[..end], max_bytes
    )
}
