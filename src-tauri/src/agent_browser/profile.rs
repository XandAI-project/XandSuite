//! Per-session Chromium user-data-dir management.
//!
//! Two profile kinds are supported:
//!
//! - **Disposable** (default): a temporary `user-data-dir` created under
//!   `$APP_DATA/browser-agent/profiles/<session_id>/` and deleted when the
//!   session closes. Starts from a fresh state every time; no access to the
//!   user's real cookies.
//! - **Named** (opt-in): a persistent profile under
//!   `$APP_DATA/browser-agent/vaults/<name>/` used for authenticated flows.
//!   Callers should constrain it with a domain allow-list higher up the
//!   stack (`SafetyGate`) so a compromised agent can't browse unrelated
//!   sites using the profile's cookies.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Which profile layout to materialise for a session.
#[derive(Debug, Clone)]
pub enum ProfileKind {
    /// A throw-away profile; cleaned up when the session ends.
    Disposable { session_id: String },
    /// A persistent named profile (e.g. "personal-email"). Never cleaned up
    /// automatically.
    Named { name: String },
}

pub struct ProfileManager {
    root: PathBuf,
}

impl ProfileManager {
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            root: app_data_dir.join("browser-agent"),
        }
    }

    fn profiles_dir(&self) -> PathBuf {
        self.root.join("profiles")
    }

    fn vaults_dir(&self) -> PathBuf {
        self.root.join("vaults")
    }

    pub fn downloads_dir(&self, session_id: &str) -> PathBuf {
        self.root.join("downloads").join(session_id)
    }

    /// Create the user-data-dir for a profile kind, returning its path.
    ///
    /// For `Disposable`, wipes any leftover directory from a prior session
    /// with the same id. For `Named`, reuses the directory as-is so the
    /// session keeps its cookies/local storage.
    pub fn ensure(&self, kind: &ProfileKind) -> Result<PathBuf> {
        std::fs::create_dir_all(self.profiles_dir()).ok();
        std::fs::create_dir_all(self.vaults_dir()).ok();

        let path = match kind {
            ProfileKind::Disposable { session_id } => {
                let path = self.profiles_dir().join(session_id);
                if path.exists() {
                    let _ = std::fs::remove_dir_all(&path);
                }
                std::fs::create_dir_all(&path).with_context(|| {
                    format!(
                        "failed to create disposable profile dir at {}",
                        path.display()
                    )
                })?;
                path
            }
            ProfileKind::Named { name } => {
                if name.is_empty()
                    || name.contains('/')
                    || name.contains('\\')
                    || name.contains("..")
                {
                    anyhow::bail!(
                        "invalid named profile '{}': must be a plain segment",
                        name
                    );
                }
                let path = self.vaults_dir().join(name);
                std::fs::create_dir_all(&path).with_context(|| {
                    format!(
                        "failed to create named profile dir at {}",
                        path.display()
                    )
                })?;
                path
            }
        };
        // Ensure downloads directory mirrors the profile lifecycle.
        if let ProfileKind::Disposable { session_id } = kind {
            let dl = self.downloads_dir(session_id);
            let _ = std::fs::create_dir_all(&dl);
        }
        Ok(path)
    }

    /// Remove the disposable profile when its session ends.
    /// No-op for named profiles.
    pub fn cleanup(&self, kind: &ProfileKind) {
        if let ProfileKind::Disposable { session_id } = kind {
            let path = self.profiles_dir().join(session_id);
            if let Err(e) = std::fs::remove_dir_all(&path) {
                log::warn!(
                    "[browser-agent] failed to remove disposable profile {}: {}",
                    path.display(),
                    e
                );
            }
            let dl = self.downloads_dir(session_id);
            let _ = std::fs::remove_dir_all(&dl);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposable_profile_is_created_and_cleaned_up() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ProfileManager::new(tmp.path());
        let kind = ProfileKind::Disposable {
            session_id: "abc123".into(),
        };
        let path = mgr.ensure(&kind).unwrap();
        assert!(path.exists());
        mgr.cleanup(&kind);
        assert!(!path.exists());
    }

    #[test]
    fn named_profile_is_persistent() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ProfileManager::new(tmp.path());
        let kind = ProfileKind::Named {
            name: "vault-a".into(),
        };
        let path = mgr.ensure(&kind).unwrap();
        assert!(path.exists());
        mgr.cleanup(&kind);
        assert!(path.exists(), "named profiles must survive cleanup()");
    }

    #[test]
    fn named_profile_rejects_traversal_segments() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = ProfileManager::new(tmp.path());
        let bad = ProfileKind::Named {
            name: "../escape".into(),
        };
        assert!(mgr.ensure(&bad).is_err());
    }
}
