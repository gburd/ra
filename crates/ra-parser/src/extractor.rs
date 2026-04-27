//! Code block extraction from markdown text.
//!
//! Walks fenced code blocks and returns structured [`CodeBlock`]
//! values preserving language tags and content.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser as MdParser, Tag};

/// A fenced code block extracted from markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBlock {
    /// Language tag from the opening fence (e.g. `rust`, `sql`).
    /// Empty string when no language is specified.
    pub language: String,
    /// Full info string after the language (may include metadata).
    pub info_string: String,
    /// Raw content of the code block (without fences).
    pub content: String,
    /// Byte offset in the source where this block starts.
    pub offset: usize,
}

/// Known languages that carry semantic meaning for `.rra` files.
pub const KNOWN_LANGUAGES: &[&str] = &["algebra", "ra", "rust", "sql", "tla", "test"];

/// Returns `true` when `lang` is in [`KNOWN_LANGUAGES`].
#[must_use]
pub fn is_known_language(lang: &str) -> bool {
    KNOWN_LANGUAGES.contains(&lang)
}

/// Extract all fenced code blocks from `source` markdown.
///
/// Blocks are returned in document order.  Indented code blocks
/// are ignored; only fenced blocks (triple-backtick or tilde)
/// are captured.
#[must_use]
pub fn extract_code_blocks(source: &str) -> Vec<CodeBlock> {
    let opts =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_SMART_PUNCTUATION;

    let mut blocks = Vec::new();
    let mut current: Option<CodeBlockBuilder> = None;

    for (event, range) in MdParser::new_ext(source, opts).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let (language, info_string) = match &kind {
                    CodeBlockKind::Fenced(info) => parse_info_string(info),
                    CodeBlockKind::Indented => (String::new(), String::new()),
                };
                current = Some(CodeBlockBuilder {
                    language,
                    info_string,
                    content: String::new(),
                    offset: range.start,
                });
            }
            Event::Text(ref text) if current.is_some() => {
                if let Some(ref mut b) = current {
                    b.content.push_str(text);
                }
            }
            Event::End(Tag::CodeBlock(_)) => {
                if let Some(b) = current.take() {
                    blocks.push(b.build());
                }
            }
            _ => {}
        }
    }

    blocks
}

/// Extract only code blocks whose language is in
/// [`KNOWN_LANGUAGES`].
#[must_use]
pub fn extract_known_blocks(source: &str) -> Vec<CodeBlock> {
    extract_code_blocks(source)
        .into_iter()
        .filter(|b| is_known_language(&b.language))
        .collect()
}

// ── Helpers ──────────────────────────────────────────────────

/// Parse the info string into `(language, full_info)`.
///
/// The language is the first whitespace-delimited token;
/// the rest is preserved as-is.
fn parse_info_string(info: &str) -> (String, String) {
    let info = info.trim();
    let language = info.split_whitespace().next().unwrap_or("").to_lowercase();
    (language, info.to_owned())
}

struct CodeBlockBuilder {
    language: String,
    info_string: String,
    content: String,
    offset: usize,
}

impl CodeBlockBuilder {
    fn build(self) -> CodeBlock {
        CodeBlock {
            language: self.language,
            info_string: self.info_string,
            content: self.content.trim_end().to_owned(),
            offset: self.offset,
        }
    }
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_single_fenced_block() {
        let md = "# Title\n\n```rust\nfn main() {}\n```\n";
        let blocks = extract_code_blocks(md);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].language, "rust");
        assert_eq!(blocks[0].content, "fn main() {}");
    }

    #[test]
    fn extract_multiple_blocks() {
        let md = "\
```sql
SELECT 1;
```

Some text.

```rust
let x = 1;
```
";
        let blocks = extract_code_blocks(md);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].language, "sql");
        assert_eq!(blocks[1].language, "rust");
    }

    #[test]
    fn language_tag_is_lowercased() {
        let md = "```SQL\nSELECT 1;\n```\n";
        let blocks = extract_code_blocks(md);
        assert_eq!(blocks[0].language, "sql");
    }

    #[test]
    fn no_language_tag() {
        let md = "```\nplain text\n```\n";
        let blocks = extract_code_blocks(md);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].language, "");
    }

    #[test]
    fn info_string_preserved() {
        let md = "```rust ignore\ncode\n```\n";
        let blocks = extract_code_blocks(md);
        assert_eq!(blocks[0].info_string, "rust ignore");
        assert_eq!(blocks[0].language, "rust");
    }

    #[test]
    fn extract_known_blocks_filters() {
        let md = "\
```rust
let x = 1;
```

```python
x = 1
```

```sql
SELECT 1;
```
";
        let known = extract_known_blocks(md);
        assert_eq!(known.len(), 2);
        assert_eq!(known[0].language, "rust");
        assert_eq!(known[1].language, "sql");
    }

    #[test]
    fn empty_code_block() {
        let md = "```rust\n```\n";
        let blocks = extract_code_blocks(md);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].content, "");
    }

    #[test]
    fn algebra_language_recognized() {
        let md = "```algebra\nsigma[p](R)\n```\n";
        let blocks = extract_known_blocks(md);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].language, "algebra");
    }

    #[test]
    fn offset_is_set() {
        let md = "# Heading\n\n```rust\ncode\n```\n";
        let blocks = extract_code_blocks(md);
        assert!(blocks[0].offset > 0);
    }

    #[test]
    fn multiline_content_preserved() {
        let md = "```sql\nSELECT 1;\nSELECT 2;\nSELECT 3;\n```\n";
        let blocks = extract_code_blocks(md);
        assert_eq!(blocks[0].content, "SELECT 1;\nSELECT 2;\nSELECT 3;");
    }
}
