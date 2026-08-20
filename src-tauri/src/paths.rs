/// Shared path-resolution helpers used by multiple startup/runtime paths
/// (Tauri `setup()`, builtin MCP server registration, and the package
/// manager). Centralized here so the three previously-drifted copies of
/// this logic can't disagree again.
use std::path::PathBuf;

/// Resolve the `tools/` directory shipped with XandSuite, trying — in
/// order — an explicit override, a bundled production layout, a macOS
/// `.app` resources layout, and finally the Cargo workspace root used in
/// development.
///
/// `CARGO_MANIFEST_DIR` is a *compile-time* environment variable baked into
/// the binary at build time. It is only meaningful for the machine and
/// checkout that built the binary — in a packaged release it typically
/// points at a path that doesn't exist on the end user's machine, silently
/// degrading `tools_dir` to `tools` relative to the process's current
/// working directory. The exe-relative and resources-relative candidates
/// below are checked first so packaged builds resolve correctly; the
/// manifest-dir fallback only matters for `cargo run` in this checkout.
pub fn resolve_tools_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XANDSUITE_TOOLS_DIR") {
        return PathBuf::from(dir);
    }

    if let Ok(exe) = std::env::current_exe() {
        let exe_dir = exe.parent().unwrap_or(&exe);

        // Bundled install: tools/ sits next to the binary
        let candidate = exe_dir.join("tools");
        if candidate.exists() {
            return candidate;
        }

        // Some Tauri bundles place resources one level up (e.g. macOS .app)
        if let Some(parent) = exe_dir.parent() {
            let candidate = parent.join("resources").join("tools");
            if candidate.exists() {
                return candidate;
            }
        }
    }

    // Dev mode: CARGO_MANIFEST_DIR is src-tauri/, tools/ is one level up
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(manifest)
        .parent()
        .unwrap_or(&PathBuf::from("."))
        .join("tools")
}
