pub mod downloader;

use anyhow::{Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use crate::process_ext::HideWindowStd;

pub struct WhisperManager {
    process: Option<Child>,
    port: u16,
    model_path: Option<String>,
    last_start: Option<Instant>,
}

impl WhisperManager {
    pub fn new() -> Self {
        Self {
            process: None,
            port: 8765,
            model_path: None,
            last_start: None,
        }
    }

    pub fn binary_path(data_dir: &Path) -> PathBuf {
        let bin_name = if cfg!(target_os = "windows") {
            "whisper-server.exe"
        } else {
            "whisper-server"
        };
        // Whisper lives in its own sub-directory so its bundled DLLs never
        // collide with the llama-server DLLs in the parent bin/ folder.
        data_dir.join("bin").join("whisper").join(bin_name)
    }

    pub fn binary_exists(data_dir: &Path) -> bool {
        Self::binary_path(data_dir).exists()
    }

    /// Start the whisper-server subprocess and wait until its HTTP port is ready.
    pub async fn start(&mut self, model_path: &str, port: u16, data_dir: &Path) -> Result<()> {
        self.stop();

        let bin = Self::binary_path(data_dir);
        if !bin.exists() {
            anyhow::bail!(
                "whisper-server binary not found at {:?}. Download it from Settings → Voice Input.",
                bin
            );
        }

        let mut cmd = Command::new(&bin);
        cmd.hide_window();
        if let Some(bin_dir) = bin.parent() {
            cmd.current_dir(bin_dir);
        }

        cmd.arg("--model").arg(model_path)
            .arg("--port").arg(port.to_string())
            .arg("--host").arg("127.0.0.1");

        cmd.stdout(Stdio::null()).stderr(Stdio::piped());

        let mut child = cmd.spawn().context("Failed to spawn whisper-server process")?;
        let mut stderr_pipe = child.stderr.take();

        self.process = Some(child);
        self.port = port;
        self.model_path = Some(model_path.to_string());
        self.last_start = Some(Instant::now());

        let result = self.wait_ready(port, 60).await;

        if let Err(ref e) = result {
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
            } else {
                return Err(anyhow::anyhow!(
                    "whisper-server failed: {}\n\nOutput:\n{}", e, stderr_text.trim()
                ));
            }
        }

        log::info!("whisper-server ready on port {}", port);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.model_path = None;
    }

    pub fn is_running(&mut self) -> bool {
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

    /// POST audio bytes to the running whisper-server and return the transcript.
    /// `ext` should be `"webm"` or `"wav"`.
    pub async fn transcribe(&self, audio_bytes: &[u8], ext: &str, language: &str) -> Result<String> {
        let url = format!("http://127.0.0.1:{}/inference", self.port);

        // Write to a named temp file so whisper-server can detect the format.
        let tmp_path = std::env::temp_dir().join(format!("xandsuite_audio.{}", ext));
        tokio::fs::write(&tmp_path, audio_bytes)
            .await
            .context("Failed to write audio temp file")?;

        let file_bytes = tokio::fs::read(&tmp_path)
            .await
            .context("Failed to read audio temp file")?;

        let mime = if ext == "wav" { "audio/wav" } else { "audio/webm" };
        let file_part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(format!("audio.{}", ext))
            .mime_str(mime)?;

        let mut form = reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("response_format", "json");

        if language != "auto" && !language.is_empty() {
            form = form.text("language", language.to_string());
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;

        let resp = client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .context("Failed to reach whisper-server")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("whisper-server returned {}: {}", status, body);
        }

        let json: serde_json::Value = resp.json().await.context("Invalid JSON from whisper-server")?;

        let text = json
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        // Clean up temp file (best-effort)
        let _ = tokio::fs::remove_file(&tmp_path).await;

        Ok(text)
    }

    async fn wait_ready(&mut self, port: u16, timeout_secs: u64) -> Result<()> {
        let url = format!("http://127.0.0.1:{}/", port);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()?;
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

        loop {
            if let Some(child) = &mut self.process {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        anyhow::bail!(
                            "whisper-server exited immediately with status {}.",
                            status
                        );
                    }
                    Ok(None) => {}
                    Err(_) => {}
                }
            }

            if std::time::Instant::now() >= deadline {
                anyhow::bail!(
                    "whisper-server did not become ready within {} seconds.",
                    timeout_secs
                );
            }

            // whisper-server v1.8+ returns 200 on GET / when ready
            if client.get(&url).send().await.map(|r| r.status().is_success()).unwrap_or(false) {
                return Ok(());
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }
}

impl Default for WhisperManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WhisperManager {
    fn drop(&mut self) {
        self.stop();
    }
}
