pub mod downloader;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use crate::process_ext::HideWindowStd;

/// Must match _STAMP_VERSION in kokoro_server.py
const STAMP_VERSION: &str = "2";

pub struct KokoroManager {
    process: Option<Child>,
    port: u16,
    last_start: Option<Instant>,
    log_path: Option<PathBuf>,
}

impl KokoroManager {
    pub fn new() -> Self {
        Self {
            process: None,
            port: 8766,
            last_start: None,
            log_path: None,
        }
    }

    /// Path to the bundled kokoro_server.py script.
    pub fn script_path() -> PathBuf {
        if let Ok(exe) = std::env::current_exe() {
            let candidate = exe
                .parent()
                .unwrap_or(&exe)
                .join("tools")
                .join("tts")
                .join("kokoro_server.py");
            if candidate.exists() {
                return candidate;
            }
        }
        // Dev mode: CARGO_MANIFEST_DIR is src-tauri/, tools/ is one level up
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(manifest)
            .parent()
            .unwrap_or(&PathBuf::from("."))
            .join("tools")
            .join("tts")
            .join("kokoro_server.py")
    }

    /// App-local HuggingFace cache directory (HF_HOME for the kokoro process).
    /// All model weights are downloaded here and loaded from here — fully offline
    /// after the first `--download` run.
    pub fn hf_home(data_dir: &Path) -> PathBuf {
        data_dir.join("kokoro-models")
    }

    /// Returns true when the model snapshot has been downloaded to the local
    /// HF cache.  Checks for the expected HuggingFace hub directory structure.
    pub fn models_exist(data_dir: &Path) -> bool {
        let hub_dir = Self::hf_home(data_dir)
            .join("hub")
            .join("models--hexgrad--Kokoro-82M");
        hub_dir.join("snapshots").exists()
    }

    /// Returns true when Python deps have been fully installed for the given
    /// device variant.  Reads the stamp file written by kokoro_server.py after
    /// a successful `_ensure_deps` run.
    pub fn deps_ready(data_dir: &Path, device: &str) -> bool {
        let stamp = Self::hf_home(data_dir)
            .join("tts-venv")
            .join(format!(".deps-ok-{}", device));
        match std::fs::read_to_string(&stamp) {
            Ok(content) => content.trim() == STAMP_VERSION,
            Err(_) => false,
        }
    }

    /// Spawn kokoro_server.py in the background (non-blocking).
    /// When deps are already installed (stamp file valid), passes --skip-deps
    /// for near-instant startup.
    /// Poll [`is_healthy`] to know when it is ready.
    pub fn spawn(&mut self, port: u16, data_dir: &Path, device: &str) -> Result<()> {
        self.stop();

        let script = Self::script_path();
        if !script.exists() {
            anyhow::bail!(
                "kokoro_server.py not found at {:?}. \
                 This file should have been bundled with XandSuite.",
                script
            );
        }

        let hf_home = Self::hf_home(data_dir);
        let skip_deps = Self::deps_ready(data_dir, device);

        // Log file
        let log_path = data_dir.join("kokoro-server.log");
        let fallback_log = std::env::temp_dir().join("kokoro-server.log");
        let actual_log_path = if std::fs::File::create(&log_path).is_ok() {
            log_path
        } else {
            fallback_log
        };
        let _ = std::fs::write(&actual_log_path, "");

        let mut child = None;
        for interp in &["python", "python3"] {
            let stderr_stdio: Stdio = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&actual_log_path)
                .map(Stdio::from)
                .unwrap_or_else(|_| Stdio::null());

            let mut cmd = Command::new(interp);
            cmd.hide_window();
            cmd.env("PYTHONUNBUFFERED", "1");
            cmd.env("PYTHONUTF8", "1");
            cmd.env("HF_HUB_DISABLE_PROGRESS_BARS", "1");

            let mut args = vec![
                script.to_string_lossy().to_string(),
                "--port".to_string(),
                port.to_string(),
                "--hf-home".to_string(),
                hf_home.to_string_lossy().to_string(),
                "--device".to_string(),
                device.to_string(),
                "--log-file".to_string(),
                actual_log_path.to_string_lossy().to_string(),
            ];
            if skip_deps {
                args.push("--skip-deps".to_string());
            }

            cmd.args(&args);
            cmd.stdout(Stdio::null()).stderr(stderr_stdio);

            if let Ok(c) = cmd.spawn() {
                child = Some(c);
                break;
            }
        }

        let child = child.context(
            "Failed to spawn kokoro_server.py — is Python 3 on your PATH?",
        )?;

        self.process = Some(child);
        self.port = port;
        self.last_start = Some(Instant::now());
        self.log_path = Some(actual_log_path);

        log::info!(
            "kokoro-server spawned on port {} (skip_deps={}, waiting for /health)",
            port,
            skip_deps
        );
        Ok(())
    }

    /// Spawn in setup mode: installs deps for the given device, writes stamp,
    /// and exits.  Returns the child process — caller should wait on it.
    pub fn spawn_setup(data_dir: &Path, device: &str, log_path: &Path) -> Result<Child> {
        let script = Self::script_path();
        if !script.exists() {
            anyhow::bail!("kokoro_server.py not found at {:?}", script);
        }

        let hf_home = Self::hf_home(data_dir);
        let _ = std::fs::write(log_path, "");

        for interp in &["python", "python3"] {
            let stderr_stdio: Stdio = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_path)
                .map(Stdio::from)
                .unwrap_or_else(|_| Stdio::null());

            let mut cmd = Command::new(interp);
            cmd.hide_window();
            cmd.env("PYTHONUNBUFFERED", "1");
            cmd.env("PYTHONUTF8", "1");
            cmd.env("HF_HUB_DISABLE_PROGRESS_BARS", "1");
            cmd.args([
                script.to_string_lossy().as_ref(),
                "--setup",
                "--hf-home",
                &hf_home.to_string_lossy(),
                "--device",
                device,
                "--log-file",
                &log_path.to_string_lossy(),
            ]);
            cmd.stdout(Stdio::null()).stderr(stderr_stdio);

            if let Ok(child) = cmd.spawn() {
                return Ok(child);
            }
        }
        anyhow::bail!("Failed to spawn Python — is Python 3 on your PATH?")
    }

    /// Spawn in download mode: runs `kokoro_server.py --download --hf-home <path>`.
    /// Returns the child process — caller should wait on it.
    pub fn spawn_download(data_dir: &Path, log_path: &Path, device: &str) -> Result<Child> {
        let script = Self::script_path();
        if !script.exists() {
            anyhow::bail!("kokoro_server.py not found at {:?}", script);
        }

        let hf_home = Self::hf_home(data_dir);
        let _ = std::fs::write(log_path, "");

        for interp in &["python", "python3"] {
            let stderr_stdio: Stdio = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_path)
                .map(Stdio::from)
                .unwrap_or_else(|_| Stdio::null());

            let mut cmd = Command::new(interp);
            cmd.hide_window();
            cmd.env("PYTHONUNBUFFERED", "1");
            cmd.env("PYTHONUTF8", "1");
            cmd.env("HF_HUB_DISABLE_PROGRESS_BARS", "1");
            cmd.args([
                script.to_string_lossy().as_ref(),
                "--download",
                "--hf-home",
                &hf_home.to_string_lossy(),
                "--device",
                device,
                "--log-file",
                &log_path.to_string_lossy(),
            ]);
            cmd.stdout(Stdio::null()).stderr(stderr_stdio);

            if let Ok(child) = cmd.spawn() {
                return Ok(child);
            }
        }
        anyhow::bail!("Failed to spawn Python — is Python 3 on your PATH?")
    }

    /// Block-wait for the server to become healthy (used in auto-start flows).
    pub async fn start(&mut self, port: u16, data_dir: &Path, device: &str) -> Result<()> {
        self.spawn(port, data_dir, device)?;
        self.wait_ready(port, 600).await.map_err(|e| {
            let log = self.read_log().unwrap_or_default();
            self.stop();
            if log.is_empty() {
                anyhow::anyhow!("{}", e)
            } else {
                anyhow::anyhow!("kokoro-server failed to start: {}\n\nLog:\n{}", e, log.trim())
            }
        })
    }

    /// Ping the /health endpoint.
    pub async fn is_healthy(&mut self) -> bool {
        if !self.is_running() {
            return false;
        }
        let url = format!("http://127.0.0.1:{}/health", self.port);
        let Ok(client) = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
        else {
            return false;
        };
        client
            .get(&url)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    pub fn read_log(&self) -> Option<String> {
        std::fs::read_to_string(self.log_path.as_ref()?).ok()
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn is_running(&mut self) -> bool {
        if let Some(child) = &mut self.process {
            matches!(child.try_wait(), Ok(None))
        } else {
            false
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Stream TTS audio as PCM chunks via Tauri events.
    ///
    /// POSTs to `/tts/stream` and reads the chunked response body
    /// progressively.  Each chunk is emitted as a `tts_audio_chunk` event
    /// with `{ request_id, data: Vec<u8>, done: false }`.  A final event
    /// with `done: true` signals completion.
    pub async fn synthesize_stream(
        &self,
        text: &str,
        voice: &str,
        speed: f32,
        app: &tauri::AppHandle,
        request_id: &str,
    ) -> Result<()> {
        use futures_util::StreamExt;
        use tauri::Emitter;

        let url = format!("http://127.0.0.1:{}/tts/stream", self.port);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;

        let body = serde_json::json!({
            "text": text,
            "voice": voice,
            "speed": speed,
        });

        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to reach kokoro-server /tts/stream")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let msg = resp.text().await.unwrap_or_default();
            anyhow::bail!("kokoro-server /tts/stream returned {}: {}", status, msg);
        }

        let mut stream = resp.bytes_stream();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.context("Error reading TTS stream chunk")?;
            let _ = app.emit("tts_audio_chunk", serde_json::json!({
                "request_id": request_id,
                "data": chunk.to_vec(),
                "done": false,
            }));
        }

        let _ = app.emit("tts_audio_chunk", serde_json::json!({
            "request_id": request_id,
            "data": [],
            "done": true,
        }));

        Ok(())
    }

    /// Send text to the running server and return WAV bytes.
    pub async fn synthesize(&self, text: &str, voice: &str, speed: f32) -> Result<Vec<u8>> {
        let url = format!("http://127.0.0.1:{}/tts", self.port);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;

        let body = serde_json::json!({
            "text": text,
            "voice": voice,
            "speed": speed,
        });

        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to reach kokoro-server")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let msg = resp.text().await.unwrap_or_default();
            anyhow::bail!("kokoro-server returned {}: {}", status, msg);
        }

        Ok(resp.bytes().await.context("Failed to read TTS audio")?.to_vec())
    }

    async fn wait_ready(&mut self, port: u16, timeout_secs: u64) -> Result<()> {
        let url = format!("http://127.0.0.1:{}/health", port);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()?;
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

        loop {
            if let Some(child) = &mut self.process {
                if let Ok(Some(status)) = child.try_wait() {
                    anyhow::bail!("kokoro-server exited with status {}.", status);
                }
            }
            if std::time::Instant::now() >= deadline {
                anyhow::bail!(
                    "kokoro-server did not become ready within {} seconds.",
                    timeout_secs
                );
            }
            if client
                .get(&url)
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
            {
                return Ok(());
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(600)).await;
        }
    }
}

impl Default for KokoroManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for KokoroManager {
    fn drop(&mut self) {
        self.stop();
    }
}
