use anyhow::{Context, Result};
use reqwest::{Client, Method};
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;

pub struct HttpApiTool {
    client: Client,
}

impl HttpApiTool {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    pub async fn request(
        &self,
        method: &str,
        url: &str,
        headers: Option<HashMap<String, String>>,
        body: Option<&str>,
    ) -> Result<Value> {
        let http_method = Method::from_str(method.to_uppercase().as_str())
            .with_context(|| format!("Invalid HTTP method: {}", method))?;

        let mut req = self.client.request(http_method, url);

        if let Some(hdrs) = headers {
            for (k, v) in hdrs {
                req = req.header(k, v);
            }
        }

        if let Some(b) = body {
            req = req.body(b.to_string());
        }

        let response = req
            .send()
            .await
            .with_context(|| format!("Failed to make HTTP request to {}", url))?;

        let status = response.status().as_u16();
        let response_headers: HashMap<String, String> = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or("").to_string(),
                )
            })
            .collect();

        let body_text = response
            .text()
            .await
            .unwrap_or_default();

        // Try to parse body as JSON, fall back to string
        let body_value: Value = serde_json::from_str(&body_text)
            .unwrap_or(Value::String(body_text.clone()));

        Ok(serde_json::json!({
            "status": status,
            "success": status >= 200 && status < 300,
            "headers": response_headers,
            "body": body_value
        }))
    }
}

impl Default for HttpApiTool {
    fn default() -> Self {
        Self::new()
    }
}
