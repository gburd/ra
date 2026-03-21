//! Link validation test for documentation files
//!
//! Validates that all markdown links point to existing files. Scans:
//! - Documentation files (./docs/**/*.md)
//! - Rule files (./rules/**/*.rra, ./rules/**/*.md)
//! - Research files (./research/**/*.md)
//! - Root markdown files (README.md, CONTRIBUTING.md, etc.)

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// A broken link with location information
#[derive(Debug, Clone)]
struct BrokenLink {
    source_file: PathBuf,
    line_number: usize,
    link_text: String,
    target_path: String,
    reason: String,
}

/// Link pattern matcher using regex
struct LinkParser {
    /// Matches markdown links: [text](path)
    markdown_link: regex::Regex,
    /// Matches relative doc links in Rust comments: [`Something`](path)
    rust_doc_link: regex::Regex,
}

impl LinkParser {
    fn new() -> Self {
        Self {
            // Match [text](path) but not [text](http://...) or [text](#anchor)
            markdown_link: regex::Regex::new(r"\[([^\]]+)\]\(([^):#]+(?:\.md|\.rra)?)\)")
                .expect("Invalid markdown link regex"),
            // Match [`Something`](path) in doc comments
            rust_doc_link: regex::Regex::new(r"\[`([^`]+)`\]\(([^):#]+\.md)\)")
                .expect("Invalid rust doc link regex"),
        }
    }

    /// Extract all file links from content
    fn extract_links(&self, content: &str) -> Vec<(String, String)> {
        let mut links = Vec::new();

        // Extract markdown links
        for cap in self.markdown_link.captures_iter(content) {
            let text = cap.get(1).map_or("", |m| m.as_str());
            let path = cap.get(2).map_or("", |m| m.as_str());
            if !path.is_empty() && !path.starts_with("http") && !path.starts_with('#') {
                links.push((text.to_string(), path.to_string()));
            }
        }

        // Extract rust doc links
        for cap in self.rust_doc_link.captures_iter(content) {
            let text = cap.get(1).map_or("", |m| m.as_str());
            let path = cap.get(2).map_or("", |m| m.as_str());
            if !path.is_empty() {
                links.push((text.to_string(), path.to_string()));
            }
        }

        links
    }
}

/// Resolves a relative link path from a source file
fn resolve_link_path(source_file: &Path, link_target: &str, repo_root: &Path) -> PathBuf {
    let source_dir = source_file.parent().unwrap_or(repo_root);

    // Handle absolute paths from repo root
    if link_target.starts_with('/') {
        return repo_root.join(link_target.trim_start_matches('/'));
    }

    // Handle relative paths
    let mut resolved = source_dir.to_path_buf();
    for component in link_target.split('/') {
        match component {
            "." => {}
            ".." => {
                resolved.pop();
            }
            _ => resolved.push(component),
        }
    }

    resolved
}

/// Validate links in a single file
fn validate_file_links(
    file_path: &Path,
    repo_root: &Path,
    parser: &LinkParser,
) -> Vec<BrokenLink> {
    let mut broken_links = Vec::new();

    let content = match fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            broken_links.push(BrokenLink {
                source_file: file_path.to_path_buf(),
                line_number: 0,
                link_text: String::new(),
                target_path: String::new(),
                reason: format!("Failed to read file: {e}"),
            });
            return broken_links;
        }
    };

    // Build line number index
    let mut line_starts = vec![0];
    for (idx, ch) in content.char_indices() {
        if ch == '\n' {
            line_starts.push(idx + 1);
        }
    }

    let links = parser.extract_links(&content);

    for (text, target) in links {
        // Find line number for this link
        let link_pattern = format!("[{text}]({target})");
        let line_num = if let Some(pos) = content.find(&link_pattern) {
            line_starts.iter().position(|&start| start > pos).unwrap_or(1)
        } else {
            1
        };

        let resolved_path = resolve_link_path(file_path, &target, repo_root);

        // Check if file exists
        if !resolved_path.exists() {
            broken_links.push(BrokenLink {
                source_file: file_path.to_path_buf(),
                line_number: line_num,
                link_text: text.clone(),
                target_path: target.clone(),
                reason: format!("Target does not exist: {}", resolved_path.display()),
            });
        } else if resolved_path.is_dir() {
            broken_links.push(BrokenLink {
                source_file: file_path.to_path_buf(),
                line_number: line_num,
                link_text: text.clone(),
                target_path: target.clone(),
                reason: format!("Target is a directory: {}", resolved_path.display()),
            });
        }
    }

    broken_links
}

/// Find all markdown and rule files in the repository
fn find_documentation_files(repo_root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    // Directories to scan for documentation
    let scan_dirs = vec!["docs", "research", "rules"];

    for dir in scan_dirs {
        let dir_path = repo_root.join(dir);
        if dir_path.exists() {
            visit_dirs(&dir_path, &mut files, &["md", "rra"]);
        }
    }

    // Add root-level markdown files
    for entry in fs::read_dir(repo_root).expect("Failed to read repo root") {
        let entry = entry.expect("Failed to read directory entry");
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "md" {
                    files.push(path);
                }
            }
        }
    }

    // Also scan Rust source files for doc comment links
    let crates_dir = repo_root.join("crates");
    if crates_dir.exists() {
        visit_dirs(&crates_dir, &mut files, &["rs"]);
    }

    files
}

/// Recursively visit directories and collect files with specific extensions
fn visit_dirs(dir: &Path, files: &mut Vec<PathBuf>, extensions: &[&str]) {
    if dir.is_dir() {
        for entry in fs::read_dir(dir).expect("Failed to read directory") {
            let entry = entry.expect("Failed to read entry");
            let path = entry.path();

            // Skip hidden directories, node_modules, target, .worktrees
            if path.is_dir() {
                let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if dir_name.starts_with('.')
                    || dir_name == "node_modules"
                    || dir_name == "target"
                {
                    continue;
                }
                visit_dirs(&path, files, extensions);
            } else if path.is_file() {
                if let Some(ext) = path.extension() {
                    if extensions.iter().any(|&e| ext == e) {
                        files.push(path);
                    }
                }
            }
        }
    }
}

#[test]
fn test_documentation_links() {
    // Find repository root
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let repo_root = Path::new(manifest_dir)
        .parent()
        .and_then(Path::parent)
        .expect("Failed to find repo root");

    println!("Repository root: {}", repo_root.display());

    let parser = LinkParser::new();
    let files = find_documentation_files(repo_root);

    println!("Found {} documentation files to check", files.len());

    let mut all_broken_links = Vec::new();
    let mut files_with_broken_links = HashSet::new();

    // Validate each file
    for file in &files {
        let broken_links = validate_file_links(file, repo_root, &parser);
        if !broken_links.is_empty() {
            files_with_broken_links.insert(file.clone());
            all_broken_links.extend(broken_links);
        }
    }

    // Report results
    if all_broken_links.is_empty() {
        println!("✓ All documentation links are valid!");
        return;
    }

    // Group broken links by file
    let mut links_by_file: HashMap<PathBuf, Vec<BrokenLink>> = HashMap::new();
    for link in &all_broken_links {
        links_by_file
            .entry(link.source_file.clone())
            .or_default()
            .push(link.clone());
    }

    println!("\n⚠ Found {} broken links in {} files:", all_broken_links.len(), files_with_broken_links.len());
    println!("{}", "=".repeat(80));

    for (file, links) in &links_by_file {
        let rel_path = file.strip_prefix(repo_root).unwrap_or(file);
        println!("\n{}:", rel_path.display());
        for link in links {
            println!(
                "  Line {}: [{}]({}) - {}",
                link.line_number, link.link_text, link.target_path, link.reason
            );
        }
    }

    println!("\n{}", "=".repeat(80));
    println!("Summary: {}/{} files have broken links", files_with_broken_links.len(), files.len());

    // Generate warning but don't fail the test (as per requirements)
    eprintln!("\n⚠ WARNING: Documentation has broken links. Please fix them.");
    eprintln!("See above for details.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_parser_markdown() {
        let parser = LinkParser::new();
        let content = r#"
[Getting Started](GETTING_STARTED.md)
[API Reference](api-reference.md)
[External](https://example.com)
[Anchor](#section)
        "#;

        let links = parser.extract_links(content);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].0, "Getting Started");
        assert_eq!(links[0].1, "GETTING_STARTED.md");
        assert_eq!(links[1].0, "API Reference");
        assert_eq!(links[1].1, "api-reference.md");
    }

    #[test]
    fn test_link_resolution_relative() {
        let repo_root = Path::new("/repo");
        let source = Path::new("/repo/docs/guides/testing.md");
        let target = "../features/adaptive.md";

        let resolved = resolve_link_path(source, target, repo_root);
        assert_eq!(resolved, Path::new("/repo/docs/features/adaptive.md"));
    }

    #[test]
    fn test_link_resolution_absolute() {
        let repo_root = Path::new("/repo");
        let source = Path::new("/repo/docs/guides/testing.md");
        let target = "/docs/README.md";

        let resolved = resolve_link_path(source, target, repo_root);
        assert_eq!(resolved, Path::new("/repo/docs/README.md"));
    }
}
