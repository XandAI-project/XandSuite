pub mod downloader;

use anyhow::{Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use crate::models::AppSettings;

pub struct LlamaServerManager {
    process: Option<Child>,
    port: u16,
    model_path: Option<String>,
    bin_path: Option<PathBuf>,
    /// Timestamp of the last completed inference request.
    /// Used by the idle-watcher to decide when to stop the server.
    last_activity: Option<Instant>,
    /// Set to true when we detected a server that was started outside our
    /// lifetime (e.g. orphaned from a previous session). We track its
    /// model/port but have no Child handle for it.
    adopted: bool,
}

impl LlamaServerManager {
    pub fn new() -> Self {
        Self {
            process: None,
            port: 11434,
            model_path: None,
            bin_path: None,
            last_activity: None,
            adopted: false,
        }
    }

    /// Mark an externally-running server as ours without spawning a new process.
    /// Called when we detect an orphaned server on startup.
    pub fn adopt(&mut self, port: u16, model_path: Option<String>) {
        self.port = port;
        self.model_path = model_path;
        self.last_activity = Some(Instant::now());
        self.adopted = true;
    }

    /// Returns true when the server was adopted (no child handle) rather than
    /// started by this process.
    pub fn is_adopted(&self) -> bool {
        self.adopted
    }

    /// Clear the adopted flag without killing anything. Used before spawning a
    /// replacement process so `start()` does not treat the port as free.
    pub fn clear_adopted(&mut self) {
        self.adopted = false;
        self.model_path = None;
        self.last_activity = None;
    }

    pub fn binary_path(data_dir: &Path) -> PathBuf {
        let bin_name = if cfg!(target_os = "windows") {
            "llama-server.exe"
        } else {
            "llama-server"
        };
        data_dir.join("bin").join(bin_name)
    }

    pub fn binary_exists(data_dir: &Path) -> bool {
        Self::binary_path(data_dir).exists()
    }

    /// Start the llama-server subprocess with the given model and settings.
    /// Polls `/health` until the server is ready, the process dies, or timeout.
    pub async fn start(
        &mut self,
        model_path: &str,
        settings: &AppSettings,
        data_dir: &Path,
    ) -> Result<()> {
        self.stop();

        let bin = Self::binary_path(data_dir);
        if !bin.exists() {
            anyhow::bail!(
                "llama-server binary not found at {:?}. Download it from Settings → Local Server.",
                bin
            );
        }

        let port = settings.llama_server_port;
        let mut cmd = Command::new(&bin);

        // Set the bin directory as the working directory so any DLLs next to
        // the binary (e.g. CUDA runtime DLLs) are found automatically.
        if let Some(bin_dir) = bin.parent() {
            cmd.current_dir(bin_dir);
        }

        cmd.arg("--model").arg(model_path)
           .arg("--port").arg(port.to_string())
           .arg("--host").arg("127.0.0.1")
           .arg("--ctx-size").arg(settings.server_context_size.to_string())
           .arg("--batch-size").arg(settings.server_batch_size.to_string())
           .arg("--n-gpu-layers").arg(settings.n_gpu_layers.to_string());

        if settings.server_threads > 0 {
            cmd.arg("--threads").arg(settings.server_threads.to_string());
        }

        // VLM: pass the multimodal projection file when configured
        if let Some(ref mmproj) = settings.mmproj_path {
            if !mmproj.is_empty() {
                cmd.arg("--mmproj").arg(mmproj);
                log::info!("Starting with mmproj: {}", mmproj);
            }
        }

        // b5000+: --flash-attn expects an explicit value [on|off|auto]
        cmd.arg("--flash-attn").arg(if settings.flash_attention { "on" } else { "off" });

        // Reasoning/thinking format — exposes <think> content via API
        if settings.reasoning_format != "none" {
            cmd.arg("--reasoning-format").arg(&settings.reasoning_format);
        }
        if !settings.use_mmap {
            cmd.arg("--no-mmap");
        }

        // Pipe stderr so we can surface crash messages to the user.
        // stdout stays null (verbose model-loading logs aren't useful here).
        cmd.stdout(Stdio::null()).stderr(Stdio::piped());

        let mut child = cmd.spawn().context("Failed to spawn llama-server process")?;

        // Extract the stderr pipe before moving `child` into self
        let mut stderr_pipe = child.stderr.take();

        self.process = Some(child);
        self.port = port;
        self.model_path = Some(model_path.to_string());
        self.bin_path = Some(bin);
        self.last_activity = Some(Instant::now());

        // Poll /health, bail early if process exits, or fail after timeout.
        let result = self.wait_ready(port, 120).await;

        if let Err(ref e) = result {
            // Read any stderr the process emitted for a richer error message.
            let stderr_text = stderr_pipe
                .as_mut()
                .and_then(|p| {
                    let mut buf = String::new();
                    p.read_to_string(&mut buf).ok()?;
                    if buf.is_empty() { None } else { Some(buf) }
                })
                .unwrap_or_default();

            self.stop();

            if stderr_text.is_empty() {
                return Err(anyhow::anyhow!("{}", e));
            }

            // Detect common known-bad patterns and surface a much more helpful message.
            let trimmed = stderr_text.trim();

            if trimmed.contains("unknown model architecture") {
                // Extract the architecture name from the log line (e.g. "unknown model architecture: 'gemma4'")
                let arch = trimmed
                    .find("unknown model architecture: '")
                    .map(|pos| {
                        let rest = &trimmed[pos + "unknown model architecture: '".len()..];
                        rest.split('\'').next().unwrap_or("unknown")
                    })
                    .unwrap_or("unknown");
                return Err(anyhow::anyhow!(
                    "Model architecture '{}' is not supported by your current llama-server binary.\n\
                     Your binary is too old to run this model. Please update it:\n\
                     Settings → Local Server → Update Binary\n\n\
                     llama-server output:\n{}",
                    arch, trimmed
                ));
            }

            if trimmed.contains("CUDA error") || trimmed.contains("failed to initialize CUDA") {
                return Err(anyhow::anyhow!(
                    "CUDA initialisation failed. Make sure your GPU drivers are up to date and \
                     the correct CUDA variant of llama-server is installed.\n\n\
                     llama-server output:\n{}",
                    trimmed
                ));
            }

            if trimmed.contains("failed to load model") || trimmed.contains("error loading model") {
                return Err(anyhow::anyhow!(
                    "llama-server could not load the model file. The file may be corrupt, \
                     incomplete, or in an unsupported format.\n\n\
                     llama-server output:\n{}",
                    trimmed
                ));
            }

            // Generic fallback with full output
            return Err(anyhow::anyhow!(
                "Server error: {}\n\nllama-server output:\n{}", e, trimmed
            ));
        }

        log::info!("llama-server ready on port {}", port);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.model_path = None;
        self.last_activity = None;
        self.adopted = false;
    }

    /// Call this every time an inference request completes to reset the idle timer.
    pub fn touch(&mut self) {
        self.last_activity = Some(Instant::now());
    }

    /// Returns `true` when the server has been idle longer than `keep_alive_mins`.
    /// A `keep_alive_mins` of 0 means never auto-stop.
    pub fn is_idle(&self, keep_alive_mins: u32) -> bool {
        if keep_alive_mins == 0 {
            return false;
        }
        match self.last_activity {
            Some(t) => t.elapsed().as_secs() >= (keep_alive_mins as u64) * 60,
            None => false,
        }
    }

    pub fn is_running(&mut self) -> bool {
        if self.adopted {
            // We don't hold a Child handle — trust that it is still alive.
            // If the process has actually died, the engine's HTTP client will
            // fail and the user can restart manually.
            return true;
        }
        if let Some(child) = &mut self.process {
            matches!(child.try_wait(), Ok(None))
        } else {
            false
        }
    }

    pub fn current_model(&self) -> Option<&str> {
        self.model_path.as_deref()
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Poll GET /health until 200, process exits, or timeout.
    async fn wait_ready(&mut self, port: u16, timeout_secs: u64) -> Result<()> {
        let url = format!("http://127.0.0.1:{}/health", port);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()?;
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

        loop {
            // Bail immediately if the process already exited (crash / bad args)
            if let Some(child) = &mut self.process {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        anyhow::bail!(
                            "llama-server exited immediately with status {}. \
                            Check that the model path is valid and any required \
                            GPU drivers/DLLs are installed.",
                            status
                        );
                    }
                    Ok(None) => {} // still running
                    Err(_) => {}
                }
            }

            if std::time::Instant::now() >= deadline {
                anyhow::bail!(
                    "llama-server did not become ready within {} seconds. \
                    Try reducing context size or GPU layers in Settings.",
                    timeout_secs
                );
            }

            match client.get(&url).send().await {
                Ok(r) if r.status().is_success() || r.status().as_u16() == 503 => {
                    // 503 = "loading model" — keep waiting but process is alive
                    if r.status().is_success() {
                        return Ok(());
                    }
                }
                _ => {}
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }
}

impl Default for LlamaServerManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for LlamaServerManager {
    fn drop(&mut self) {
        self.stop();
    }
}
