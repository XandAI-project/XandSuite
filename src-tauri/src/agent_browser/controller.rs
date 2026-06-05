//! `BrowserController` — the thin Rust layer that owns one Chromium process
//! and one active `Page` per session, plus a screencast task.
//!
//! We intentionally keep the controller API small and *imperative* (navigate,
//! click_xy, type_text, snapshot, screenshot). The reasoning-heavy work
//! (pruning the a11y tree, translating numeric indices to pixel coordinates,
//! throttling screencast frames) lives in `snapshot.rs` / `tools.rs`.
//!
//! ## Concurrency model
//! - The `chromiumoxide::Browser` object drives its handler as a stream.
//!   We spawn a background task per browser that polls `handler.next()` so
//!   CDP events flow; if that task ends, the browser is considered dead.
//! - All public methods take `&self` and are safe to call from multiple
//!   tasks (the underlying `Page` is `Send + Sync`).
//! - `paused` and `takeover` are `AtomicBool` flags checked by the input
//!   forwarding path. When `paused`, new CDP input dispatches are dropped.
//!   When `takeover`, the agent's own tools still work but the user has
//!   priority on the canvas.

use anyhow::{anyhow, Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams,
    DispatchMouseEventType, MouseButton,
};
use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, CaptureScreenshotParams, NavigateParams, ReloadParams,
    StartScreencastFormat, StartScreencastParams, StopScreencastParams,
};
use chromiumoxide::cdp::js_protocol::runtime::EvaluateParams;
use chromiumoxide::Page;
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use tokio::task::JoinHandle;

use super::profile::{ProfileKind, ProfileManager};

/// Coarse controller state used by the UI status pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerState {
    Starting,
    Ready,
    Closed,
}

pub struct BrowserController {
    pub session_id: String,
    #[allow(dead_code)]
    profile_kind: ProfileKind,
    #[allow(dead_code)]
    profile_path: PathBuf,
    browser: TokioMutex<Option<Browser>>,
    /// The single active page for MVP. Multi-tab support lives in the
    /// post-MVP plan.
    page: TokioMutex<Option<Page>>,
    handler_task: TokioMutex<Option<JoinHandle<()>>>,
    screencast_task: TokioMutex<Option<JoinHandle<()>>>,
    paused: AtomicBool,
    takeover: AtomicBool,
    state: TokioMutex<ControllerState>,
}

impl BrowserController {
    /// Spawn a new Chromium instance bound to the given profile.
    ///
    /// Fails fast when Chromium cannot be found. The user-facing setup
    /// flow (Browser Agent tab settings) teaches the user how to set a
    /// custom executable path.
    pub async fn launch(
        session_id: String,
        profile_mgr: &ProfileManager,
        profile_kind: ProfileKind,
        initial_url: Option<String>,
        chrome_executable: Option<PathBuf>,
    ) -> Result<Arc<Self>> {
        let profile_path = profile_mgr.ensure(&profile_kind)?;

        // Headless by default — the viewport is streamed into the XandSuite
        // canvas via `Page.startScreencast`, so a visible OS window would
        // detach the browser from our UI (two places to look, confusing UX).
        // chromiumoxide's default BrowserConfig passes `--headless` for us;
        // we just don't call `.with_head()`.
        //
        // Canvas / WebGL rendering note
        // ─────────────────────────────
        // Headless Chrome (chromiumoxide default) adds `--disable-gpu` which
        // prevents the GPU compositor from running.  Canvas elements are
        // rendered via Skia but are only included in `Page.startScreencast`
        // frames when the compositor is active.  The result: any <canvas>-
        // based QR code, chart, or animation appears white/blank in the
        // screencast even though the DOM is populated.
        //
        // Fix: force SwiftShader as the software OpenGL implementation.
        // `--use-gl=swiftshader` gives Chromium a real OpenGL surface backed
        // by software so the compositor pipeline runs end-to-end and canvas
        // output is correctly included in every screencast frame.
        // `--ignore-gpu-blocklist` lets SwiftShader initialise even when the
        // GPU entry is on the driver blocklist (common in server/CI contexts).
        //
        // Background networking note
        // ──────────────────────────
        // `--disable-background-networking` was originally here to reduce
        // noise, but it also suppresses the WebSocket keep-alive path that
        // sites like WhatsApp Web use to receive live QR / auth payloads.
        // Removed so that real-time WebSocket connections work correctly.
        let mut builder = BrowserConfig::builder()
            .user_data_dir(&profile_path)
            // ── Canvas / compositor ──────────────────────────────────────────
            // SwiftShader gives headless Chrome a full software GL stack so
            // <canvas> elements are composited into screencast frames.
            .arg("--use-gl=swiftshader")
            .arg("--ignore-gpu-blocklist")
            // ── Standard headless hardening ──────────────────────────────────
            .arg("--hide-scrollbars")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-sync")
            .arg("--disable-features=Translate,OptimizationHints")
            .arg("--disable-backgrounding-occluded-windows")
            .arg("--disable-extensions")
            .arg("--disable-client-side-phishing-detection")
            .arg("--disable-component-update")
            .arg("--password-store=basic")
            .arg("--use-mock-keychain")
            .arg("--mute-audio")
            .arg("--window-size=1280,800");

        if let Some(exe) = chrome_executable {
            builder = builder.chrome_executable(exe);
        }

        let config = builder
            .build()
            .map_err(|e| anyhow!("failed to build browser config: {}", e))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .context("failed to launch Chromium for the Browser Agent")?;

        // Drive the CDP handler continuously. If this task exits the browser
        // is dead — we signal that by dropping the Page and flipping state.
        let handler_task = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(e) = event {
                    log::warn!("[browser-agent] CDP handler error: {}", e);
                    break;
                }
            }
            log::info!("[browser-agent] CDP handler loop ended");
        });

        let start_url = initial_url.unwrap_or_else(|| "about:blank".to_string());
        let page = browser
            .new_page(start_url.as_str())
            .await
            .context("failed to open initial page")?;

        let ctl = Arc::new(Self {
            session_id,
            profile_kind,
            profile_path,
            browser: TokioMutex::new(Some(browser)),
            page: TokioMutex::new(Some(page)),
            handler_task: TokioMutex::new(Some(handler_task)),
            screencast_task: TokioMutex::new(None),
            paused: AtomicBool::new(false),
            takeover: AtomicBool::new(false),
            state: TokioMutex::new(ControllerState::Ready),
        });

        Ok(ctl)
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, v: bool) {
        self.paused.store(v, Ordering::Relaxed);
    }

    pub fn is_takeover(&self) -> bool {
        self.takeover.load(Ordering::Relaxed)
    }

    pub fn set_takeover(&self, v: bool) {
        self.takeover.store(v, Ordering::Relaxed);
    }

    pub async fn current_state(&self) -> ControllerState {
        *self.state.lock().await
    }

    // ── Navigation ──────────────────────────────────────────────────────

    pub async fn navigate(&self, url: &str) -> Result<()> {
        let page = self.active_page().await?;
        let params = NavigateParams::builder()
            .url(url)
            .build()
            .map_err(|e| anyhow!("bad navigate params: {}", e))?;
        page.execute(params).await?;
        page.wait_for_navigation().await?;
        Ok(())
    }

    pub async fn reload(&self) -> Result<()> {
        let page = self.active_page().await?;
        page.execute(ReloadParams::default()).await?;
        page.wait_for_navigation().await?;
        Ok(())
    }

    pub async fn go_back(&self) -> Result<()> {
        let page = self.active_page().await?;
        // No high-level helper for history navigation — evaluate JS.
        let params = EvaluateParams::builder()
            .expression("window.history.back()")
            .build()
            .map_err(|e| anyhow!("eval params: {}", e))?;
        page.execute(params).await?;
        Ok(())
    }

    pub async fn go_forward(&self) -> Result<()> {
        let page = self.active_page().await?;
        let params = EvaluateParams::builder()
            .expression("window.history.forward()")
            .build()
            .map_err(|e| anyhow!("eval params: {}", e))?;
        page.execute(params).await?;
        Ok(())
    }

    /// Bulk-install cookies into the browser via `Network.setCookies`.
    ///
    /// Called once on launch when the user picked a saved cookie session, so
    /// the embedded Chromium starts already-authenticated against the target
    /// site instead of forcing the user to log in inside the agent viewport.
    pub async fn set_cookies(&self, cookies: Vec<CookieParam>) -> Result<usize> {
        if cookies.is_empty() {
            return Ok(0);
        }
        let count = cookies.len();
        let guard = self.browser.lock().await;
        let browser = guard
            .as_ref()
            .ok_or_else(|| anyhow!("browser is not running"))?;
        browser
            .set_cookies(cookies)
            .await
            .context("Network.setCookies failed")?;
        Ok(count)
    }

    pub async fn current_url(&self) -> Result<String> {
        let page = self.active_page().await?;
        let url = page.url().await?.unwrap_or_default();
        Ok(url)
    }

    pub async fn current_title(&self) -> Result<String> {
        let page = self.active_page().await?;
        let title = page.get_title().await?.unwrap_or_default();
        Ok(title)
    }

    // ── Input dispatch ──────────────────────────────────────────────────

    /// Dispatch a mouse event at CSS-pixel coordinates. Honoured only when
    /// not paused and (for user-driven calls) not in agent-control mode.
    pub async fn dispatch_mouse(
        &self,
        kind: DispatchMouseEventType,
        x: f64,
        y: f64,
        button: Option<MouseButton>,
        click_count: Option<i64>,
    ) -> Result<()> {
        if self.is_paused() {
            return Err(anyhow!("browser controller is paused"));
        }
        let page = self.active_page().await?;
        let mut b = DispatchMouseEventParams::builder()
            .r#type(kind)
            .x(x)
            .y(y);
        if let Some(btn) = button {
            b = b.button(btn);
        }
        if let Some(c) = click_count {
            b = b.click_count(c);
        }
        let params = b.build().map_err(|e| anyhow!("bad mouse params: {}", e))?;
        page.execute(params).await?;
        Ok(())
    }

    /// Dispatch a CDP `mouseWheel` event at `(x, y)` with the given deltas.
    ///
    /// `delta_y` positive = scroll DOWN (content moves up), matching how
    /// DOM wheel events report vertical deltas. This triggers real wheel
    /// listeners on the page, which is what infinite-scroll feeds and
    /// virtualised lists react to — a plain `window.scrollBy` does not.
    pub async fn scroll_wheel(
        &self,
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
    ) -> Result<()> {
        if self.is_paused() {
            return Err(anyhow!("browser controller is paused"));
        }
        let page = self.active_page().await?;
        let params = DispatchMouseEventParams::builder()
            .r#type(DispatchMouseEventType::MouseWheel)
            .x(x)
            .y(y)
            .delta_x(delta_x)
            .delta_y(delta_y)
            .build()
            .map_err(|e| anyhow!("bad wheel params: {}", e))?;
        page.execute(params).await?;
        Ok(())
    }

    pub async fn dispatch_key(
        &self,
        kind: DispatchKeyEventType,
        key: Option<String>,
        text: Option<String>,
        code: Option<String>,
    ) -> Result<()> {
        if self.is_paused() {
            return Err(anyhow!("browser controller is paused"));
        }
        let page = self.active_page().await?;
        let mut b = DispatchKeyEventParams::builder().r#type(kind);
        if let Some(k) = key {
            b = b.key(k);
        }
        if let Some(t) = text {
            b = b.text(t);
        }
        if let Some(c) = code {
            b = b.code(c);
        }
        let params = b.build().map_err(|e| anyhow!("bad key params: {}", e))?;
        page.execute(params).await?;
        Ok(())
    }

    /// Type a string into the currently focused element by dispatching a
    /// sequence of `char` key events. Suitable for short literal input —
    /// for IME / long paste use `Input.insertText` directly.
    pub async fn type_text(&self, text: &str) -> Result<()> {
        for ch in text.chars() {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf).to_string();
            self.dispatch_key(
                DispatchKeyEventType::Char,
                None,
                Some(s),
                None,
            )
            .await?;
        }
        Ok(())
    }

    // ── Screenshots ─────────────────────────────────────────────────────

    /// Capture a one-shot base64 JPEG of the current viewport.
    pub async fn screenshot_jpeg(&self, quality: i64) -> Result<String> {
        let page = self.active_page().await?;
        let params = CaptureScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Jpeg)
            .quality(quality)
            .build();
        let resp = page.execute(params).await?;
        Ok(resp.data.clone().into())
    }

    /// Start a live screencast loop. The caller supplies an `on_frame`
    /// callback that is invoked with `(data_base64, width, height)` per
    /// frame; the controller handles `Page.screencastFrameAck` internally.
    ///
    /// Cancelling the returned `JoinHandle` stops the screencast. The
    /// handle is also stored on the controller so `shutdown` can clean it.
    pub async fn start_screencast(
        self: &Arc<Self>,
        quality: i32,
        max_width: u32,
        max_height: u32,
        every_nth_frame: i32,
        on_frame: impl Fn(String, u32, u32) + Send + Sync + 'static,
    ) -> Result<()> {
        self.stop_screencast().await;

        let page = self.active_page().await?;
        let params = StartScreencastParams::builder()
            .format(StartScreencastFormat::Jpeg)
            .quality(quality as i64)
            .max_width(max_width as i64)
            .max_height(max_height as i64)
            .every_nth_frame(every_nth_frame as i64)
            .build();
        page.execute(params).await?;

        // Subscribe to screencast frame events; the closure ack's each one.
        use chromiumoxide::cdp::browser_protocol::page::{
            EventScreencastFrame, ScreencastFrameAckParams,
        };
        let mut events = page
            .event_listener::<EventScreencastFrame>()
            .await
            .context("failed to subscribe to screencast frames")?;

        let page_clone = page.clone();
        let on_frame = Arc::new(on_frame);
        let task = tokio::spawn(async move {
            while let Some(ev) = events.next().await {
                let data = ev.data.clone();
                let meta = ev.metadata.clone();
                let session_id = ev.session_id;

                let width = meta.device_width as u32;
                let height = meta.device_height as u32;
                on_frame(data.into(), width, height);

                // Ack so the next frame is delivered.
                let ack = ScreencastFrameAckParams::new(session_id);
                if let Err(e) = page_clone.execute(ack).await {
                    log::warn!("[browser-agent] screencast ack failed: {}", e);
                    break;
                }
            }
        });

        *self.screencast_task.lock().await = Some(task);
        Ok(())
    }

    pub async fn stop_screencast(&self) {
        if let Some(handle) = self.screencast_task.lock().await.take() {
            handle.abort();
        }
        if let Ok(page) = self.active_page().await {
            let _ = page.execute(StopScreencastParams::default()).await;
        }
    }

    /// Evaluate a JS snippet and return the JSON-serialised result.
    pub async fn eval_json(&self, expr: &str) -> Result<serde_json::Value> {
        let page = self.active_page().await?;
        let params = EvaluateParams::builder()
            .expression(expr)
            .return_by_value(true)
            .build()
            .map_err(|e| anyhow!("eval params: {}", e))?;
        let resp = page.execute(params).await?;
        Ok(resp
            .result
            .result
            .value
            .clone()
            .unwrap_or(serde_json::Value::Null))
    }

    // ── Lifecycle ───────────────────────────────────────────────────────

    async fn active_page(&self) -> Result<Page> {
        self.page
            .lock()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| anyhow!("browser controller has no active page"))
    }

    /// Gracefully shut down the browser. Safe to call multiple times.
    pub async fn shutdown(&self) {
        *self.state.lock().await = ControllerState::Closed;
        self.stop_screencast().await;

        // Drop the page first so any in-flight execute() calls return.
        *self.page.lock().await = None;

        if let Some(mut browser) = self.browser.lock().await.take() {
            if let Err(e) = browser.close().await {
                log::warn!("[browser-agent] browser.close() failed: {}", e);
            }
            let _ = browser.wait().await;
        }

        if let Some(handle) = self.handler_task.lock().await.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod smoke_tests {
    //! Integration smoke test for the Browser Agent.
    //!
    //! Gated behind `#[ignore]` so CI doesn't require Chromium to be
    //! installed. Run it locally with:
    //!
    //!   cargo test -p xandsuite_lib browser_agent_launches_and_navigates -- --ignored
    //!
    //! The test launches Chromium headed, navigates to `about:blank`, reads
    //! the current URL back, and shuts down cleanly. If Chromium isn't
    //! discoverable the launch step returns an error and the test is
    //! reported as failed — hence the `--ignored` guard.
    use super::*;
    use crate::agent_browser::profile::{ProfileKind, ProfileManager};

    #[tokio::test]
    #[ignore = "requires a local Chromium install; run with --ignored"]
    async fn browser_agent_launches_and_navigates() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let profiles = ProfileManager::new(tmp.path());
        let kind = ProfileKind::Disposable {
            session_id: "smoke-test".into(),
        };

        let controller = BrowserController::launch(
            "smoke-test".into(),
            &profiles,
            kind,
            Some("about:blank".into()),
            None,
        )
        .await
        .expect("launch Chromium (install Chrome/Chromium locally to run this)");

        let url = controller.current_url().await.expect("current_url");
        assert!(url.starts_with("about:blank"), "got url = {}", url);

        controller.shutdown().await;
    }
}
