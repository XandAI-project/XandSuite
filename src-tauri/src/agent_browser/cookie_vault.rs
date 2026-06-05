//! Persistent storage for user-pasted browser cookies.
//!
//! Most pages worth automating (Gmail, LinkedIn, internal dashboards) require
//! the user to be logged in. Re-doing the login flow inside the embedded
//! Chromium every time is hostile UX, and persisting the *whole* user-data-dir
//! is overkill (and would also drag along extensions, history, etc.).
//!
//! Instead we let the user paste cookies once — from a browser extension like
//! Cookie-Editor / EditThisCookie or from a `document.cookie` header dump —
//! save them to a small JSON vault on disk, and re-apply them via
//! `Network.setCookies` (CDP) when a `BrowserController` is launched with the
//! corresponding session id.
//!
//! ## File layout
//! `<app_data>/browser-agent/cookie-sessions.json` — a single JSON file
//! containing `Vec<BrowserCookieSession>`. Cheap to read/write at every
//! mutation; the dataset is tiny (a few KB per session, typically <100 KB
//! total) and we want the changes to be durable immediately.
//!
//! ## Supported paste formats
//! - **JSON array** (Cookie-Editor / EditThisCookie export): the canonical
//!   case. Each entry has `name`, `value`, `domain`, `path`, plus optional
//!   `secure`, `httpOnly`, `sameSite`, `expirationDate`/`expires`.
//! - **JSON object with `cookies` field**: same content nested under
//!   `{"cookies": [...]}`.
//! - **Header / `document.cookie` line**: `name1=val1; name2=val2`. Requires
//!   a `default_domain` to be useful — without it the cookies are written
//!   without a domain and silently scoped to the current page.
//! - **Netscape `cookies.txt` format**: one cookie per line, tab-separated:
//!   `domain  flag  path  secure  expiration  name  value`. Lines that begin
//!   with `#` are treated as comments. This is what `curl --cookie-jar` and
//!   most CLI tools emit.
//!
//! ## Security
//! - The file is written with default OS permissions, same as any other
//!   appdata file. Cookies grant authenticated access to the originating
//!   site, so this is treated as **user-sensitive data**: it must never be
//!   surfaced over the public HTTP API and must never be returned in tool
//!   results to the LLM.
//! - The frontend never sees raw cookie values when it lists sessions — the
//!   `CookieDigest` returned by `list()` only carries metadata.

use anyhow::{anyhow, Context, Result};
use chromiumoxide::cdp::browser_protocol::network::{
    CookieParam, CookieSameSite, TimeSinceEpoch,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// One cookie in our internal canonical form. We deliberately store everything
/// the LLM-driven controller might need to recreate the cookie precisely; the
/// CDP `CookieParam` is built from this on demand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieEntry {
    pub name: String,
    pub value: String,
    /// Bare domain (e.g. `linkedin.com`) or dot-prefixed (`.linkedin.com`).
    /// `None` means "scope to the page currently open" — discouraged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default = "default_path", skip_serializing_if = "is_default_path")]
    pub path: String,
    #[serde(default)]
    pub secure: bool,
    #[serde(default, rename = "httpOnly")]
    pub http_only: bool,
    /// Lowercase: `"strict" | "lax" | "none"`. Anything else is treated as
    /// missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub same_site: Option<String>,
    /// Unix epoch seconds (matches Cookie-Editor's `expirationDate`).
    /// `None` → session cookie.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<f64>,
}

fn default_path() -> String { "/".to_string() }
fn is_default_path(p: &String) -> bool { p == "/" }

/// One named "saved login" / cookie bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserCookieSession {
    pub id: String,
    pub name: String,
    /// Free-form notes from the user (e.g. "personal LinkedIn, expires Dec").
    #[serde(default)]
    pub notes: String,
    /// Default domain used when pasting `header` / `name=value;…` style input
    /// that doesn't carry a domain. Stored so editing the session preserves
    /// the original intent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_domain: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub cookies: Vec<CookieEntry>,
}

/// Public, *redacted* view used by the frontend list screen.
#[derive(Debug, Clone, Serialize)]
pub struct CookieSessionDigest {
    pub id: String,
    pub name: String,
    pub notes: String,
    pub default_domain: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub cookie_count: usize,
    /// Distinct domains, sorted, capped at 20 — useful for a UI subtitle.
    pub domains: Vec<String>,
}

impl From<&BrowserCookieSession> for CookieSessionDigest {
    fn from(s: &BrowserCookieSession) -> Self {
        let mut domains: Vec<String> = s
            .cookies
            .iter()
            .filter_map(|c| c.domain.clone())
            .map(|d| d.trim_start_matches('.').to_string())
            .collect();
        domains.sort();
        domains.dedup();
        domains.truncate(20);
        CookieSessionDigest {
            id: s.id.clone(),
            name: s.name.clone(),
            notes: s.notes.clone(),
            default_domain: s.default_domain.clone(),
            created_at: s.created_at,
            updated_at: s.updated_at,
            cookie_count: s.cookies.len(),
            domains,
        }
    }
}

/// On-disk store of cookie sessions.
pub struct CookieVault {
    path: PathBuf,
    sessions: RwLock<Vec<BrowserCookieSession>>,
}

impl CookieVault {
    /// Open or create the vault file under `<app_data>/browser-agent/`.
    pub fn open(app_data_dir: &Path) -> Result<Self> {
        let dir = app_data_dir.join("browser-agent");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("cookie-sessions.json");

        let sessions = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("read {}", path.display()))?;
            if raw.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str::<Vec<BrowserCookieSession>>(&raw)
                    .with_context(|| format!("parse {}", path.display()))?
            }
        } else {
            Vec::new()
        };

        Ok(Self {
            path,
            sessions: RwLock::new(sessions),
        })
    }

    fn flush(&self) -> Result<()> {
        let guard = self.sessions.read().expect("cookie vault poisoned");
        let json = serde_json::to_string_pretty(&*guard)
            .context("serialise cookie vault")?;
        std::fs::write(&self.path, json)
            .with_context(|| format!("write {}", self.path.display()))?;
        Ok(())
    }

    /// Lightweight list — never exposes raw cookie values.
    pub fn list_digests(&self) -> Vec<CookieSessionDigest> {
        self.sessions
            .read()
            .expect("cookie vault poisoned")
            .iter()
            .map(CookieSessionDigest::from)
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<BrowserCookieSession> {
        self.sessions
            .read()
            .expect("cookie vault poisoned")
            .iter()
            .find(|s| s.id == id)
            .cloned()
    }

    /// Insert a new session; returns the persisted record.
    pub fn create(
        &self,
        name: String,
        cookies: Vec<CookieEntry>,
        notes: String,
        default_domain: Option<String>,
    ) -> Result<BrowserCookieSession> {
        let now = Utc::now();
        let session = BrowserCookieSession {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.trim().to_string(),
            notes,
            default_domain,
            created_at: now,
            updated_at: now,
            cookies,
        };
        if session.name.is_empty() {
            return Err(anyhow!("session name cannot be empty"));
        }
        if session.cookies.is_empty() {
            return Err(anyhow!("at least one cookie is required"));
        }
        self.sessions
            .write()
            .expect("cookie vault poisoned")
            .push(session.clone());
        self.flush()?;
        Ok(session)
    }

    pub fn update(
        &self,
        id: &str,
        name: Option<String>,
        cookies: Option<Vec<CookieEntry>>,
        notes: Option<String>,
        default_domain: Option<Option<String>>,
    ) -> Result<BrowserCookieSession> {
        let mut guard = self.sessions.write().expect("cookie vault poisoned");
        let s = guard
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| anyhow!("session {} not found", id))?;
        if let Some(name) = name {
            let trimmed = name.trim().to_string();
            if trimmed.is_empty() {
                return Err(anyhow!("session name cannot be empty"));
            }
            s.name = trimmed;
        }
        if let Some(cookies) = cookies {
            if cookies.is_empty() {
                return Err(anyhow!("at least one cookie is required"));
            }
            s.cookies = cookies;
        }
        if let Some(notes) = notes {
            s.notes = notes;
        }
        if let Some(default_domain) = default_domain {
            s.default_domain = default_domain;
        }
        s.updated_at = Utc::now();
        let updated = s.clone();
        drop(guard);
        self.flush()?;
        Ok(updated)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let mut guard = self.sessions.write().expect("cookie vault poisoned");
        let before = guard.len();
        guard.retain(|s| s.id != id);
        if guard.len() == before {
            return Err(anyhow!("session {} not found", id));
        }
        drop(guard);
        self.flush()?;
        Ok(())
    }

    /// Return every saved session that contains at least one cookie whose
    /// domain matches the host of `url`.
    ///
    /// This is the core of the "auto-inject" flow: whenever the agent or
    /// toolbar navigates somewhere, we scan the vault and apply every
    /// matching session so the user never has to pick one by hand. The
    /// returned sessions are deep-cloned so the caller can use them
    /// without holding the vault lock.
    pub fn sessions_for_domain(&self, url: &str) -> Vec<BrowserCookieSession> {
        let host = extract_host(url);
        if host.is_empty() {
            return Vec::new();
        }
        self.sessions
            .read()
            .expect("cookie vault poisoned")
            .iter()
            .filter(|s| {
                s.cookies
                    .iter()
                    .any(|c| cookie_domain_matches(c.domain.as_deref(), &host))
            })
            .cloned()
            .collect()
    }
}

// ───────────────────────── Domain matching ───────────────────────────

/// Extract the lowercase host portion of `url`.
///
/// Returns an empty string for schemes that have no meaningful host
/// (`about:blank`, `data:…`, `chrome://new-tab`), which the caller
/// treats as "nothing to inject".
fn extract_host(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // Skip pseudo-schemes that don't carry a host.
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("about:")
        || lower.starts_with("data:")
        || lower.starts_with("javascript:")
        || lower.starts_with("chrome:")
        || lower.starts_with("chrome-error:")
        || lower.starts_with("view-source:")
        || lower.starts_with("file:")
    {
        return String::new();
    }
    // Strip scheme.
    let after_scheme = match trimmed.find("://") {
        Some(i) => &trimmed[i + 3..],
        None => trimmed,
    };
    // Strip userinfo, path, query, fragment. Host ends at the first `/`,
    // `?`, `#`, or `:` (port). `@` separates userinfo from host.
    let host_with_port = after_scheme
        .split(|c: char| matches!(c, '/' | '?' | '#'))
        .next()
        .unwrap_or("");
    let host_with_port = match host_with_port.rfind('@') {
        Some(i) => &host_with_port[i + 1..],
        None => host_with_port,
    };
    let host = host_with_port
        .split(':')
        .next()
        .unwrap_or("")
        .trim_end_matches('.');
    host.to_ascii_lowercase()
}

/// Returns `true` if `cookie_domain` should apply to a page whose host is
/// `host`. Follows the standard cookie scoping rule:
/// a dot-prefixed or bare domain `D` matches `host` iff
/// `host == D` or `host.ends_with("." + D)`.
fn cookie_domain_matches(cookie_domain: Option<&str>, host: &str) -> bool {
    let Some(raw) = cookie_domain else { return false };
    let d = raw.trim().trim_start_matches('.').to_ascii_lowercase();
    if d.is_empty() || host.is_empty() {
        return false;
    }
    if host == d {
        return true;
    }
    host.len() > d.len() + 1 && host.ends_with(&d) && {
        // ensure the boundary character is a dot, not part of a larger
        // label (e.g. `evil-google.com` must NOT match `google.com`).
        let boundary_idx = host.len() - d.len() - 1;
        host.as_bytes().get(boundary_idx).copied() == Some(b'.')
    }
}

// ───────────────────────────── Parsing ──────────────────────────────

/// Auto-detect the format of `raw` and return canonical `CookieEntry`s.
pub fn parse_cookies(
    raw: &str,
    default_domain: Option<&str>,
) -> Result<Vec<CookieEntry>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("input is empty"));
    }

    // Format 1: JSON array (most extensions output this).
    // Format 2: JSON object with `cookies` field.
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        return parse_json(trimmed, default_domain);
    }

    // Format 3: Netscape cookies.txt — tab-separated.
    let looks_like_netscape = trimmed.lines().any(|l| {
        let l = l.trim_start_matches("#HttpOnly_");
        !l.starts_with('#') && l.matches('\t').count() >= 6
    });
    if looks_like_netscape {
        return parse_netscape(trimmed, default_domain);
    }

    // Format 4: header / document.cookie style.
    parse_header(trimmed, default_domain)
}

fn parse_json(raw: &str, default_domain: Option<&str>) -> Result<Vec<CookieEntry>> {
    // Accept either a top-level array or an object with `cookies`.
    let value: serde_json::Value =
        serde_json::from_str(raw).context("not valid JSON")?;
    let arr = match value {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(mut obj) => obj
            .remove("cookies")
            .and_then(|v| if let serde_json::Value::Array(a) = v { Some(a) } else { None })
            .ok_or_else(|| anyhow!("JSON object missing `cookies` array"))?,
        _ => return Err(anyhow!("JSON must be an array or {{cookies:[...]}}")),
    };

    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        let obj = v
            .as_object()
            .ok_or_else(|| anyhow!("cookie entries must be JSON objects"))?;

        let name = obj
            .get("name")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow!("cookie missing `name`"))?
            .to_string();
        let value = obj
            .get("value")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow!("cookie missing `value`"))?
            .to_string();
        let domain = obj
            .get("domain")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .or_else(|| default_domain.map(|s| s.to_string()));
        let path = obj
            .get("path")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(default_path);
        let secure = obj.get("secure").and_then(|x| x.as_bool()).unwrap_or(false);
        let http_only = obj
            .get("httpOnly")
            .or_else(|| obj.get("http_only"))
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        let same_site = obj
            .get("sameSite")
            .or_else(|| obj.get("same_site"))
            .and_then(|x| x.as_str())
            .map(normalise_same_site);
        // Cookie-Editor uses `expirationDate` (float seconds); some tools use
        // `expires` as an integer; allow either.
        let expires = obj
            .get("expirationDate")
            .or_else(|| obj.get("expires"))
            .and_then(|x| x.as_f64());

        out.push(CookieEntry {
            name,
            value,
            domain,
            path,
            secure,
            http_only,
            same_site,
            expires,
        });
    }
    if out.is_empty() {
        return Err(anyhow!("JSON array contained no cookies"));
    }
    Ok(out)
}

fn parse_netscape(raw: &str, default_domain: Option<&str>) -> Result<Vec<CookieEntry>> {
    let mut out = Vec::new();
    for line in raw.lines() {
        // Comment lines start with `#`, except `#HttpOnly_` which prefixes
        // a real cookie line whose domain is HTTP-only.
        let (line, http_only) = if let Some(rest) = line.strip_prefix("#HttpOnly_") {
            (rest, true)
        } else if line.starts_with('#') || line.trim().is_empty() {
            continue;
        } else {
            (line, false)
        };

        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 7 {
            continue;
        }
        let domain = if parts[0].is_empty() {
            default_domain.map(|s| s.to_string())
        } else {
            Some(parts[0].to_string())
        };
        let path = parts[2].to_string();
        let secure = parts[3].eq_ignore_ascii_case("TRUE");
        let expires = parts[4].parse::<f64>().ok().filter(|&v| v > 0.0);
        let name = parts[5].to_string();
        let value = parts[6].to_string();
        if name.is_empty() {
            continue;
        }
        out.push(CookieEntry {
            name,
            value,
            domain,
            path: if path.is_empty() { default_path() } else { path },
            secure,
            http_only,
            same_site: None,
            expires,
        });
    }
    if out.is_empty() {
        return Err(anyhow!("no cookie lines found in Netscape input"));
    }
    Ok(out)
}

fn parse_header(raw: &str, default_domain: Option<&str>) -> Result<Vec<CookieEntry>> {
    let mut out = Vec::new();
    // The header may include a leading `Cookie:` label — strip it.
    let raw = raw.trim_start_matches("Cookie:").trim_start_matches("cookie:");
    for pair in raw.split(';') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let name = name.trim().to_string();
        let value = value.trim().to_string();
        if name.is_empty() {
            continue;
        }
        out.push(CookieEntry {
            name,
            value,
            domain: default_domain.map(|s| s.to_string()),
            path: default_path(),
            secure: false,
            http_only: false,
            same_site: None,
            expires: None,
        });
    }
    if out.is_empty() {
        return Err(anyhow!("no `name=value` pairs found"));
    }
    Ok(out)
}

fn normalise_same_site(s: &str) -> String {
    let s = s.trim().to_lowercase();
    match s.as_str() {
        "strict" | "lax" | "none" => s,
        // `no_restriction` (Cookie-Editor) maps to None.
        "no_restriction" => "none".to_string(),
        // `unspecified` / anything else — drop.
        _ => "lax".to_string(),
    }
}

// ───────────────────────────── To CDP ────────────────────────────────

/// Convert canonical entries to the CDP type. Cookies that have neither
/// `domain` nor `url` are silently dropped — Chromium would reject them
/// anyway.
pub fn to_cookie_params(entries: &[CookieEntry]) -> Vec<CookieParam> {
    entries
        .iter()
        .filter_map(|c| {
            let mut b = CookieParam::builder().name(&c.name).value(&c.value);
            // Need either domain or url; we always pass domain when present.
            if let Some(domain) = c.domain.as_deref() {
                b = b.domain(domain);
            } else {
                // Without a domain Chromium needs a `url`. Fabricate a https
                // URL from the path so the cookie still loads against pages
                // the user later navigates to manually.
                return None;
            }
            b = b.path(&c.path);
            if c.secure {
                b = b.secure(true);
            }
            if c.http_only {
                b = b.http_only(true);
            }
            if let Some(ss) = c.same_site.as_deref() {
                let cdp = match ss {
                    "strict" => CookieSameSite::Strict,
                    "none" => CookieSameSite::None,
                    _ => CookieSameSite::Lax,
                };
                b = b.same_site(cdp);
            }
            if let Some(exp) = c.expires {
                b = b.expires(TimeSinceEpoch::new(exp));
            }
            b.build().ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cookie_editor_json_array() {
        let raw = r#"[
            {
                "name": "li_at",
                "value": "AQEDAR…",
                "domain": ".linkedin.com",
                "path": "/",
                "secure": true,
                "httpOnly": true,
                "sameSite": "no_restriction",
                "expirationDate": 1799999999.5
            },
            {
                "name": "JSESSIONID",
                "value": "ajax:1234",
                "domain": ".linkedin.com",
                "path": "/"
            }
        ]"#;
        let cookies = parse_cookies(raw, None).unwrap();
        assert_eq!(cookies.len(), 2);
        assert_eq!(cookies[0].name, "li_at");
        assert_eq!(cookies[0].domain.as_deref(), Some(".linkedin.com"));
        assert!(cookies[0].secure);
        assert!(cookies[0].http_only);
        assert_eq!(cookies[0].same_site.as_deref(), Some("none"));
        assert_eq!(cookies[0].expires, Some(1799999999.5));
        assert!(!cookies[1].secure);
    }

    #[test]
    fn parses_object_with_cookies_field() {
        let raw = r#"{"cookies": [{"name": "k", "value": "v", "domain": "a.com"}]}"#;
        let cookies = parse_cookies(raw, None).unwrap();
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].name, "k");
    }

    #[test]
    fn parses_header_format() {
        let raw = "session=abc; theme=dark; lang=en-US";
        let cookies = parse_cookies(raw, Some("example.com")).unwrap();
        assert_eq!(cookies.len(), 3);
        assert_eq!(cookies[0].name, "session");
        assert_eq!(cookies[0].value, "abc");
        assert_eq!(cookies[2].domain.as_deref(), Some("example.com"));
    }

    #[test]
    fn parses_header_with_label() {
        let raw = "Cookie: a=1; b=2";
        let cookies = parse_cookies(raw, Some("x.com")).unwrap();
        assert_eq!(cookies.len(), 2);
    }

    #[test]
    fn parses_netscape_format() {
        let raw = "# Netscape HTTP Cookie File\n\
                   # this is a comment\n\
                   .example.com\tTRUE\t/\tFALSE\t1799999999\tfoo\tbar\n\
                   #HttpOnly_.example.com\tTRUE\t/secret\tTRUE\t0\tsecret\tdeadbeef\n";
        let cookies = parse_cookies(raw, None).unwrap();
        assert_eq!(cookies.len(), 2);
        assert_eq!(cookies[0].name, "foo");
        assert_eq!(cookies[0].domain.as_deref(), Some(".example.com"));
        assert!(!cookies[0].secure);
        assert_eq!(cookies[0].expires, Some(1799999999.0));
        assert_eq!(cookies[1].name, "secret");
        assert!(cookies[1].http_only);
        assert!(cookies[1].secure);
        // expires=0 means session cookie → dropped.
        assert!(cookies[1].expires.is_none());
    }

    #[test]
    fn rejects_empty_input() {
        assert!(parse_cookies("   \n\t  ", None).is_err());
    }

    #[test]
    fn vault_create_get_update_delete_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = CookieVault::open(tmp.path()).unwrap();
        assert!(vault.list_digests().is_empty());

        let cookies = vec![CookieEntry {
            name: "k".into(),
            value: "v".into(),
            domain: Some(".x.com".into()),
            path: "/".into(),
            secure: true,
            http_only: false,
            same_site: Some("lax".into()),
            expires: None,
        }];

        let s = vault
            .create(
                "x.com login".into(),
                cookies,
                "test note".into(),
                Some("x.com".into()),
            )
            .unwrap();
        assert_eq!(s.cookies.len(), 1);

        let digests = vault.list_digests();
        assert_eq!(digests.len(), 1);
        assert_eq!(digests[0].cookie_count, 1);
        assert_eq!(digests[0].domains, vec!["x.com".to_string()]);

        let updated = vault
            .update(
                &s.id,
                Some("renamed".into()),
                None,
                Some("new note".into()),
                None,
            )
            .unwrap();
        assert_eq!(updated.name, "renamed");
        assert_eq!(updated.notes, "new note");
        assert!(updated.updated_at >= s.created_at);

        // Reopen — must persist.
        drop(vault);
        let vault2 = CookieVault::open(tmp.path()).unwrap();
        assert_eq!(vault2.list_digests().len(), 1);
        assert_eq!(vault2.get(&s.id).unwrap().name, "renamed");

        vault2.delete(&s.id).unwrap();
        assert!(vault2.list_digests().is_empty());
    }

    #[test]
    fn create_rejects_empty_name_or_no_cookies() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = CookieVault::open(tmp.path()).unwrap();
        let cookies = vec![CookieEntry {
            name: "k".into(),
            value: "v".into(),
            domain: Some("a.com".into()),
            path: "/".into(),
            secure: false,
            http_only: false,
            same_site: None,
            expires: None,
        }];
        assert!(vault.create("   ".into(), cookies.clone(), "".into(), None).is_err());
        assert!(vault.create("ok".into(), vec![], "".into(), None).is_err());
    }

    #[test]
    fn extract_host_handles_common_urls() {
        assert_eq!(extract_host("https://www.google.com/search?q=rust"), "www.google.com");
        assert_eq!(extract_host("http://example.com"), "example.com");
        assert_eq!(extract_host("https://user:pass@Accounts.Google.COM/path"), "accounts.google.com");
        assert_eq!(extract_host("https://host.example.com:8443/x"), "host.example.com");
        assert_eq!(extract_host("example.com/path"), "example.com");
        // Pseudo-schemes return empty.
        assert_eq!(extract_host("about:blank"), "");
        assert_eq!(extract_host("data:text/html,hi"), "");
        assert_eq!(extract_host("chrome://settings"), "");
        assert_eq!(extract_host("file:///C:/tmp.html"), "");
        assert_eq!(extract_host(""), "");
    }

    #[test]
    fn cookie_domain_matches_rules() {
        // Exact matches.
        assert!(cookie_domain_matches(Some("google.com"), "google.com"));
        assert!(cookie_domain_matches(Some(".google.com"), "google.com"));
        // Subdomain matches.
        assert!(cookie_domain_matches(Some(".google.com"), "www.google.com"));
        assert!(cookie_domain_matches(Some("google.com"), "accounts.google.com"));
        // Must respect label boundary — not a suffix-trick match.
        assert!(!cookie_domain_matches(Some("google.com"), "evilgoogle.com"));
        assert!(!cookie_domain_matches(Some(".google.com"), "notgoogle.com"));
        // Non-matches.
        assert!(!cookie_domain_matches(Some("linkedin.com"), "google.com"));
        // None / empty.
        assert!(!cookie_domain_matches(None, "google.com"));
        assert!(!cookie_domain_matches(Some(""), "google.com"));
        assert!(!cookie_domain_matches(Some("google.com"), ""));
        // Case-insensitive.
        assert!(cookie_domain_matches(Some("Google.COM"), "google.com"));
    }

    #[test]
    fn sessions_for_domain_finds_matching_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = CookieVault::open(tmp.path()).unwrap();
        let google_cookies = vec![CookieEntry {
            name: "SID".into(), value: "x".into(),
            domain: Some(".google.com".into()), path: "/".into(),
            secure: true, http_only: true, same_site: None, expires: None,
        }];
        let linkedin_cookies = vec![CookieEntry {
            name: "li_at".into(), value: "x".into(),
            domain: Some(".linkedin.com".into()), path: "/".into(),
            secure: true, http_only: true, same_site: None, expires: None,
        }];
        vault.create("Google".into(), google_cookies, "".into(), None).unwrap();
        vault.create("LinkedIn".into(), linkedin_cookies, "".into(), None).unwrap();

        // Exact domain hit.
        let hits = vault.sessions_for_domain("https://google.com/");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "Google");
        // Subdomain hit.
        let hits = vault.sessions_for_domain("https://accounts.google.com/signin");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "Google");
        // Different site → no match.
        let hits = vault.sessions_for_domain("https://bing.com/");
        assert!(hits.is_empty());
        // about:blank → empty host → no matches.
        let hits = vault.sessions_for_domain("about:blank");
        assert!(hits.is_empty());
    }

    #[test]
    fn to_cookie_params_drops_entries_without_domain() {
        let entries = vec![
            CookieEntry {
                name: "ok".into(),
                value: "v".into(),
                domain: Some("a.com".into()),
                path: "/".into(),
                secure: false,
                http_only: false,
                same_site: Some("strict".into()),
                expires: None,
            },
            CookieEntry {
                name: "no_domain".into(),
                value: "v".into(),
                domain: None,
                path: "/".into(),
                secure: false,
                http_only: false,
                same_site: None,
                expires: None,
            },
        ];
        let params = to_cookie_params(&entries);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "ok");
    }
}
