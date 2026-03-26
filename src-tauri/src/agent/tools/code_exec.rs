use anyhow::{Context, Result};
use serde_json::Value;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

pub struct CodeExecTool {
    workspace_dir: std::path::PathBuf,
}

impl CodeExecTool {
    pub fn new(workspace_dir: std::path::PathBuf) -> Self {
        Self { workspace_dir }
    }

    /// Execute a shell command in the workspace directory with a timeout.
    /// Returns stdout, stderr, and exit code.
    pub async fn execute(&self, command: &str, timeout_secs: u64) -> Result<Value> {
        std::fs::create_dir_all(&self.workspace_dir)
            .context("Failed to create agent workspace")?;

        let child = if cfg!(target_os = "windows") {
            Command::new("cmd")
                .args(["/C", command])
                .current_dir(&self.workspace_dir)
                .output()
        } else {
            Command::new("sh")
                .args(["-c", command])
                .current_dir(&self.workspace_dir)
                .output()
        };

        let result = timeout(
            Duration::from_secs(timeout_secs),
            child,
        )
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
}
