/// Text chunking strategies for RAG document processing

pub struct ChunkConfig {
    /// Target chunk size in characters
    pub chunk_size: usize,
    /// Overlap between consecutive chunks in characters
    pub overlap: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            chunk_size: 512,
            overlap: 64,
        }
    }
}

/// Split text into fixed-size chunks with overlap.
pub fn chunk_fixed_size(text: &str, config: &ChunkConfig) -> Vec<String> {
    if text.is_empty() {
        return vec![];
    }

    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    let mut chunks = Vec::new();
    let mut start = 0;

    while start < total {
        let end = (start + config.chunk_size).min(total);

        // Try to end at a word boundary
        let actual_end = if end < total {
            find_word_boundary(&chars, end)
        } else {
            end
        };

        let chunk: String = chars[start..actual_end].iter().collect();
        let chunk = chunk.trim().to_string();
        if !chunk.is_empty() {
            chunks.push(chunk);
        }

        if actual_end >= total {
            break;
        }

        // Move start forward with overlap
        start = actual_end.saturating_sub(config.overlap);
        if start == 0 {
            start = actual_end;
        }
    }

    chunks
}

/// Split text at paragraph boundaries (double newlines), then merge into
/// chunks respecting the size limit.
pub fn chunk_by_paragraphs(text: &str, config: &ChunkConfig) -> Vec<String> {
    let paragraphs: Vec<&str> = text
        .split("\n\n")
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();

    let mut chunks = Vec::new();
    let mut current = String::new();

    for para in paragraphs {
        if current.is_empty() {
            current.push_str(para);
        } else if current.len() + para.len() + 2 <= config.chunk_size {
            current.push_str("\n\n");
            current.push_str(para);
        } else {
            if !current.is_empty() {
                chunks.push(current.trim().to_string());
            }
            current = para.to_string();
        }
    }

    if !current.is_empty() {
        chunks.push(current.trim().to_string());
    }

    chunks
}

fn find_word_boundary(chars: &[char], pos: usize) -> usize {
    let mut p = pos;
    while p < chars.len() && !chars[p].is_whitespace() {
        p += 1;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_fixed_size_basic() {
        let text = "Hello world. This is a test. ".repeat(20);
        let config = ChunkConfig { chunk_size: 100, overlap: 20 };
        let chunks = chunk_fixed_size(&text, &config);
        assert!(!chunks.is_empty());
        for chunk in &chunks {
            assert!(chunk.len() <= 110); // allow slight overflow for word boundary
        }
    }

    #[test]
    fn test_chunk_paragraphs() {
        let text = "Paragraph one.\n\nParagraph two.\n\nParagraph three.";
        let config = ChunkConfig::default();
        let chunks = chunk_by_paragraphs(text, &config);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_empty_text() {
        let config = ChunkConfig::default();
        assert!(chunk_fixed_size("", &config).is_empty());
        assert!(chunk_by_paragraphs("", &config).is_empty());
    }
}
