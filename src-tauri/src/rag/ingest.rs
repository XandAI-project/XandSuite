use anyhow::{Context, Result};
use regex::Regex;
use serde_json::Value;
use std::path::Path;

/// Ingest a file and return (content: String, metadata: Value)
pub async fn ingest_file(path: &Path) -> Result<Vec<(String, Value)>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "pdf" => ingest_pdf(path).await,
        "csv" => ingest_csv(path).await,
        "json" | "jsonl" => ingest_json(path).await,
        "txt" | "md" => ingest_text(path).await,
        _ => anyhow::bail!(
            "Unsupported file type: .{}. Supported: pdf, csv, json, jsonl, txt, md",
            ext
        ),
    }
}

async fn ingest_pdf(path: &Path) -> Result<Vec<(String, Value)>> {
    let bytes = tokio::fs::read(path)
        .await
        .context("Failed to read PDF file")?;

    let raw = pdf_extract::extract_text_from_mem(&bytes)
        .context("Failed to extract text from PDF")?;

    let text = fix_pdf_char_spacing(&raw);

    let metadata = serde_json::json!({
        "source": path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
        "type": "pdf",
        "pages": "unknown"
    });

    Ok(vec![(text, metadata)])
}

/// Many PDFs produced by certain tools (e.g. scanned + OCR, or old PostScript exporters)
/// store each glyph with an explicit advance, causing `pdf_extract` to insert a space
/// after every single character: "G l i n d a" instead of "Glinda".
///
/// This function detects the pattern (>50 % of whitespace-delimited tokens are a single
/// character) and reconstructs normal word-spaced text by treating two-or-more consecutive
/// spaces as a word boundary and collapsing single spaces between individual characters.
fn fix_pdf_char_spacing(text: &str) -> String {
    // Quick bail-out: not enough content to bother or already looks normal.
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.len() < 10 {
        return text.to_string();
    }
    let single_count = tokens.iter().filter(|t| t.chars().count() == 1).count();
    if single_count * 2 < tokens.len() {
        return text.to_string();
    }

    // Two-or-more spaces (or a tab) signal a word boundary in char-spaced PDFs.
    let re_word_sep = Regex::new(r"[ \t]{2,}").unwrap();

    text.lines()
        .map(|line| {
            if line.trim().is_empty() {
                return String::new();
            }
            if re_word_sep.is_match(line) {
                // Reconstruct: split on multi-space (word gap), then within each
                // segment remove the single spaces between individual glyphs.
                re_word_sep
                    .split(line)
                    .map(|chunk| {
                        chunk
                            .split(' ')
                            .filter(|s| !s.is_empty())
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                // No double-space markers: collapse every space on this line.
                // Word boundaries are lost but at least the characters are readable.
                line.split(' ')
                    .filter(|s| !s.is_empty())
                    .collect::<String>()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn ingest_csv(path: &Path) -> Result<Vec<(String, Value)>> {
    let content = tokio::fs::read_to_string(path)
        .await
        .context("Failed to read CSV file")?;

    let mut reader = csv::Reader::from_reader(content.as_bytes());
    let headers: Vec<String> = reader
        .headers()
        .context("Failed to read CSV headers")?
        .iter()
        .map(String::from)
        .collect();

    let mut records = Vec::new();

    for (i, result) in reader.records().enumerate() {
        let record = result.context("Failed to parse CSV record")?;
        let row_map: serde_json::Map<String, Value> = headers
            .iter()
            .zip(record.iter())
            .map(|(h, v)| (h.clone(), Value::String(v.to_string())))
            .collect();

        let content_parts: Vec<String> = headers
            .iter()
            .zip(record.iter())
            .map(|(h, v)| format!("{}: {}", h, v))
            .collect();
        let content = content_parts.join(", ");

        let metadata = serde_json::json!({
            "source": path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            "type": "csv",
            "row_index": i,
            "row_data": row_map
        });

        records.push((content, metadata));
    }

    Ok(records)
}

async fn ingest_json(path: &Path) -> Result<Vec<(String, Value)>> {
    let content = tokio::fs::read_to_string(path)
        .await
        .context("Failed to read JSON file")?;

    let source_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    let is_jsonl = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e == "jsonl")
        .unwrap_or(false);

    if is_jsonl {
        // JSONL: one JSON object per line
        let mut records = Vec::new();
        for (i, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<Value>(line) {
                let text = flatten_json_to_text(&val);
                let metadata = serde_json::json!({
                    "source": source_name,
                    "type": "jsonl",
                    "line_index": i
                });
                records.push((text, metadata));
            }
        }
        Ok(records)
    } else {
        // Regular JSON
        let val: Value = serde_json::from_str(&content)
            .context("Failed to parse JSON file")?;

        match val {
            Value::Array(items) => {
                let records = items
                    .into_iter()
                    .enumerate()
                    .map(|(i, item)| {
                        let text = flatten_json_to_text(&item);
                        let meta = serde_json::json!({
                            "source": source_name,
                            "type": "json",
                            "index": i
                        });
                        (text, meta)
                    })
                    .collect();
                Ok(records)
            }
            other => {
                let text = flatten_json_to_text(&other);
                let metadata = serde_json::json!({
                    "source": source_name,
                    "type": "json"
                });
                Ok(vec![(text, metadata)])
            }
        }
    }
}

async fn ingest_text(path: &Path) -> Result<Vec<(String, Value)>> {
    let content = tokio::fs::read_to_string(path)
        .await
        .context("Failed to read text file")?;

    let metadata = serde_json::json!({
        "source": path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
        "type": "text"
    });

    Ok(vec![(content, metadata)])
}

fn flatten_json_to_text(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(arr) => arr
            .iter()
            .map(flatten_json_to_text)
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(obj) => obj
            .iter()
            .map(|(k, v)| format!("{}: {}", k, flatten_json_to_text(v)))
            .collect::<Vec<_>>()
            .join("; "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn test_ingest_csv() {
        let mut f = tempfile::NamedTempFile::with_suffix(".csv").unwrap();
        writeln!(f, "name,age\nAlice,30\nBob,25").unwrap();
        let path = f.path().to_path_buf();
        let records = ingest_file(&path).await.unwrap();
        assert_eq!(records.len(), 2);
        assert!(records[0].0.contains("Alice"));
    }

    #[tokio::test]
    async fn test_ingest_text() {
        let mut f = tempfile::NamedTempFile::with_suffix(".txt").unwrap();
        writeln!(f, "Hello world").unwrap();
        let records = ingest_file(f.path()).await.unwrap();
        assert_eq!(records.len(), 1);
        assert!(records[0].0.contains("Hello world"));
    }
}
