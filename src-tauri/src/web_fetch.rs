/// Fetch a URL and extract its readable text content.
///
/// Uses `reqwest` for HTTP and `scraper` for HTML parsing.  Both crates are
/// already present in Cargo.toml.  On success the returned string is stripped
/// of markup and capped at 15 000 characters so it does not overflow a typical
/// LLM context window.

use std::time::Duration;

const MAX_CONTENT_CHARS: usize = 15_000;

/// Fetch `url` and return the visible text extracted from the HTML body.
/// Returns `Err(reason)` on any network or parse failure — never panics.
pub async fn fetch_url_content(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        // A browser-like UA avoids 403s from servers that reject bots.
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
             AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/124.0.0.0 Safari/537.36",
        )
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("HTTP client build error: {}", e))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    // Only process text responses (HTML, plain text).
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    if content_type.contains("application/")
        && !content_type.contains("json")
        && !content_type.contains("xml")
    {
        return Err(format!(
            "Unsupported content type: {}",
            content_type
        ));
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    // Plain-text responses don't need HTML parsing.
    if content_type.contains("text/plain") {
        let trimmed: String = body.chars().take(MAX_CONTENT_CHARS).collect();
        return Ok(trimmed.trim().to_string());
    }

    Ok(extract_text_from_html(&body))
}

/// Extract human-readable text from an HTML document.
///
/// Preference order for the main content container:
///   1. `<main>`
///   2. `<article>`
///   3. `<body>` (fallback)
///
/// `<script>`, `<style>`, `<nav>`, `<header>`, `<footer>`, and `<aside>`
/// elements are excluded to avoid injecting boilerplate / menus.
fn extract_text_from_html(html: &str) -> String {
    use scraper::{Html, Selector};

    let document = Html::parse_document(html);

    // Tags whose content we always strip — they add noise, not information.
    let noise_tags = ["script", "style", "nav", "header", "footer", "aside", "noscript"];

    // Try progressively broader containers.
    let container_selectors = ["main", "article", "body"];

    let noise_sel: Vec<Selector> = noise_tags
        .iter()
        .filter_map(|t| Selector::parse(t).ok())
        .collect();

    for container_tag in &container_selectors {
        let Ok(sel) = Selector::parse(container_tag) else { continue };
        let Some(root) = document.select(&sel).next() else { continue };

        // Collect text nodes, skipping noise subtrees.
        let mut text = String::new();
        for node in root.descendants() {
            // Skip if the node is inside a noise element.
            let is_noise = noise_sel.iter().any(|ns| {
                node.ancestors().any(|anc| {
                    scraper::ElementRef::wrap(anc)
                        .map(|el| ns.matches(&el))
                        .unwrap_or(false)
                })
            });
            if is_noise {
                continue;
            }

            if let Some(t) = node.value().as_text() {
                let piece = t.trim();
                if !piece.is_empty() {
                    text.push_str(piece);
                    text.push(' ');
                }
            }
        }

        let cleaned = collapse_whitespace(&text);
        if !cleaned.is_empty() {
            let capped: String = cleaned.chars().take(MAX_CONTENT_CHARS).collect();
            return capped.trim().to_string();
        }
    }

    // Last resort: grab all text nodes from the full document.
    let mut text = String::new();
    for node in document.root_element().descendants() {
        if let Some(t) = node.value().as_text() {
            let piece = t.trim();
            if !piece.is_empty() {
                text.push_str(piece);
                text.push(' ');
            }
        }
    }

    let cleaned = collapse_whitespace(&text);
    let capped: String = cleaned.chars().take(MAX_CONTENT_CHARS).collect();
    capped.trim().to_string()
}

/// Replace runs of whitespace (including newlines) with a single space.
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}
