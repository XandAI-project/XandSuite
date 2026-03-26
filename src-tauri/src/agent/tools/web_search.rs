use anyhow::{Context, Result};
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;

pub struct WebSearchTool {
    client: Client,
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent(
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
                     AppleWebKit/537.36 (KHTML, like Gecko) \
                     Chrome/124.0.0.0 Safari/537.36",
                )
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Search using DuckDuckGo HTML endpoint — returns real web results,
    /// no API key required.
    pub async fn search(&self, query: &str) -> Result<Value> {
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        let html_text = self
            .client
            .get(&url)
            // Accept header convinces DDG to return full HTML results
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("Accept-Language", "en-US,en;q=0.9")
            .send()
            .await
            .context("Failed to connect to DuckDuckGo")?
            .text()
            .await
            .context("Failed to read DuckDuckGo response")?;

        let document = Html::parse_document(&html_text);

        // Selectors for DDG HTML layout
        let title_sel = Selector::parse("a.result__a").unwrap();
        let snippet_sel = Selector::parse(".result__snippet").unwrap();
        let url_sel = Selector::parse("a.result__url").unwrap();

        let titles: Vec<_> = document.select(&title_sel).collect();
        let snippets: Vec<_> = document.select(&snippet_sel).collect();
        let urls: Vec<_> = document.select(&url_sel).collect();

        let mut results = Vec::new();

        for i in 0..titles.len().min(8) {
            let title = titles[i]
                .text()
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");

            // DDG href is a redirect: /l/?uddg=<encoded_url>&...
            // Extract the real URL from the uddg query parameter
            let href = titles[i].value().attr("href").unwrap_or("");
            let real_url = extract_uddg_url(href).unwrap_or_else(|| href.to_string());

            let snippet = snippets
                .get(i)
                .map(|e| {
                    e.text()
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();

            let display_url = urls
                .get(i)
                .map(|e| {
                    e.text()
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_else(|| real_url.clone());

            if !title.is_empty() {
                results.push(serde_json::json!({
                    "title": title,
                    "url": real_url,
                    "display_url": display_url,
                    "snippet": snippet,
                }));
            }
        }

        Ok(serde_json::json!({
            "query": query,
            "results": results,
            "result_count": results.len(),
        }))
    }
}

/// Extract the real destination URL from a DuckDuckGo redirect href.
/// DDG redirects look like: /l/?uddg=https%3A%2F%2Fexample.com%2F&rut=...
fn extract_uddg_url(href: &str) -> Option<String> {
    // Find "uddg=" parameter
    let key = "uddg=";
    let start = href.find(key)? + key.len();
    let rest = &href[start..];
    // The value ends at '&' or end of string
    let end = rest.find('&').unwrap_or(rest.len());
    let encoded = &rest[..end];
    urlencoding::decode(encoded).ok().map(|s| s.into_owned())
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}
