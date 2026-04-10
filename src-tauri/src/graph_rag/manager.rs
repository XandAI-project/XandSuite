use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;

use crate::process_ext::HideWindowStd;

/// Manages the graphrag-server sidecar process, mirroring `LlamaServerManager`.
pub struct GraphRagManager {
    process: Option<Child>,
    port: u16,
}

impl GraphRagManager {
    pub fn new() -> Self {
        Self {
            process: None,
            port: 3848,
        }
    }

    /// Resolve the path to the graphrag-server binary.
    /// Uses `server_path` override if provided; otherwise falls back to
    /// `<data_dir>/graphrag-server[.exe]`.
    pub fn binary_path(data_dir: &PathBuf, server_path: Option<&str>) -> PathBuf {
        if let Some(p) = server_path {
            return PathBuf::from(p);
        }
        let exe = if cfg!(windows) {
            "graphrag-server.exe"
        } else {
            "graphrag-server"
        };
        data_dir.join(exe)
    }

    /// Start the graphrag-server process.
    pub fn start(
        &mut self,
        data_dir: &PathBuf,
        port: u16,
        vector_db: &str,
        embedding_model: &str,
        server_path: Option<&str>,
    ) -> Result<()> {
        if self.is_running() {
            return Ok(());
        }

        let bin = Self::binary_path(data_dir, server_path);
        if !bin.exists() {
            anyhow::bail!(
                "graphrag-server binary not found at {:?}. \
                 Download it from https://github.com/Abraxas-365/graphrag-rs/releases \
                 and place it in your app data directory.",
                bin
            );
        }

        let mut cmd = Command::new(&bin);
        cmd.hide_window();
        let child = cmd
            .args([
                "--port", &port.to_string(),
                "--vector-db", vector_db,
                "--embedding-model", embedding_model,
            ])
            .spawn()
            .context("Failed to spawn graphrag-server")?;

        self.process = Some(child);
        self.port = port;
        log::info!("graphrag-server started on port {}", port);
        Ok(())
    }

    /// Stop the graphrag-server process.
    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
            log::info!("graphrag-server stopped.");
        }
    }

    pub fn is_running(&mut self) -> bool {
        if let Some(child) = &mut self.process {
            matches!(child.try_wait(), Ok(None))
        } else {
            false
        }
    }

    /// Poll `GET /health` until the server is ready or timeout is exceeded.
    pub async fn wait_ready(&self, timeout_secs: u64) -> Result<()> {
        let url = format!("http://127.0.0.1:{}/health", self.port);
        let client = reqwest::Client::new();
        let deadline = tokio::time::Instant::now()
            + Duration::from_secs(timeout_secs);

        loop {
            if tokio::time::Instant::now() > deadline {
                anyhow::bail!("graphrag-server did not become ready within {}s", timeout_secs);
            }
            if let Ok(resp) = client.get(&url).send().await {
                if resp.status().is_success() {
                    log::info!("graphrag-server is ready.");
                    return Ok(());
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Default for GraphRagManager {
    fn default() -> Self {
        Self::new()
    }
}
