//! Shared helpers for ra-parser integration tests.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// Absolute path to the workspace root.
pub fn workspace_root() -> PathBuf {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Navigate from crates/ra-parser/ up to workspace root.
    crate_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root should exist")
        .to_path_buf()
}

/// Absolute path to the `tests/fixtures/` directory.
pub fn fixtures_dir() -> PathBuf {
    workspace_root().join("tests").join("fixtures")
}

/// Absolute path to the `rules/` directory.
pub fn rules_dir() -> PathBuf {
    workspace_root().join("rules")
}

/// Load the raw text of a test fixture by file name.
///
/// # Panics
///
/// Panics if the fixture file cannot be read.
pub fn load_fixture(name: &str) -> String {
    let path = fixtures_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read fixture {}: {e}",
            path.display()
        )
    })
}

/// Recursively collect all `.rra` files under `dir`.
pub fn collect_rra_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_rra(dir, &mut files);
    files.sort();
    files
}

fn walk_rra(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.ends_with("templates") {
                continue;
            }
            walk_rra(&path, out);
        } else if path
            .extension()
            .is_some_and(|ext| ext == "rra")
        {
            out.push(path);
        }
    }
}
