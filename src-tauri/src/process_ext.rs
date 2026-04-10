/// Platform helpers for hiding console windows on Windows.
///
/// On Windows, spawning a subprocess with `Command::new(...)` opens a visible
/// console window.  The `HideWindow` trait adds `.hide_window()` that applies
/// the `CREATE_NO_WINDOW` creation flag so the subprocess runs silently.
///
/// On non-Windows platforms the method is a no-op.

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

// ── std::process::Command ────────────────────────────────────────────────────

pub trait HideWindowStd {
    fn hide_window(&mut self) -> &mut Self;
}

impl HideWindowStd for std::process::Command {
    fn hide_window(&mut self) -> &mut Self {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(CREATE_NO_WINDOW);
        }
        self
    }
}

// ── tokio::process::Command ──────────────────────────────────────────────────

pub trait HideWindowTokio {
    fn hide_window(&mut self) -> &mut Self;
}

impl HideWindowTokio for tokio::process::Command {
    fn hide_window(&mut self) -> &mut Self {
        #[cfg(target_os = "windows")]
        {
            self.creation_flags(CREATE_NO_WINDOW);
        }
        self
    }
}
