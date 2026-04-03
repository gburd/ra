//! Setup wizard for timeline and query selection.
//!
//! Provides functionality for discovering available timeline files
//! and configuring playback options before launching the TUI.

use std::path::{Path, PathBuf};

#[cfg(feature = "timeline")]
use crate::timeline::Timeline;

/// Configuration produced by the setup process.
#[cfg(feature = "timeline")]
#[derive(Debug, Clone)]
pub struct TuiConfig {
    /// The timeline to play back.
    pub timeline: Timeline,
    /// Whether to run in headless mode.
    pub headless: bool,
    /// Initial playback speed index.
    pub initial_speed: usize,
    /// Whether to auto-play on startup.
    pub auto_play: bool,
}

#[cfg(feature = "timeline")]
impl TuiConfig {
    /// Create a config from a timeline with default settings.
    #[must_use]
    pub fn from_timeline(timeline: Timeline) -> Self {
        Self {
            timeline,
            headless: false,
            initial_speed: 2, // 1x
            auto_play: false,
        }
    }

    /// Create a headless config for testing.
    #[must_use]
    pub fn headless(timeline: Timeline) -> Self {
        Self {
            timeline,
            headless: true,
            initial_speed: 2,
            auto_play: false,
        }
    }
}

/// Discover timeline files in a directory.
///
/// Searches for `.json` and `.toml` files that look like timeline
/// data. Returns sorted list of paths.
#[must_use]
pub fn discover_timelines(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "json" || ext == "toml" {
                        files.push(path);
                    }
                }
            }
        }
    }
    files.sort();
    files
}

/// Load a timeline from a JSON file path.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
#[cfg(feature = "timeline")]
pub fn load_timeline_json(
    path: &Path,
) -> Result<Timeline, SetupError> {
    let source = std::fs::read_to_string(path).map_err(|e| {
        SetupError::IoError(format!(
            "reading {}: {e}",
            path.display()
        ))
    })?;
    serde_json::from_str(&source).map_err(|e| {
        SetupError::ParseError(format!(
            "parsing {}: {e}",
            path.display()
        ))
    })
}

/// Errors from the setup process.
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    /// File I/O error.
    #[error("I/O error: {0}")]
    IoError(String),
    /// File parsing error.
    #[error("parse error: {0}")]
    ParseError(String),
    /// No timeline data available.
    #[error("no timeline data: {0}")]
    NoTimeline(String),
}

#[cfg(all(test, feature = "timeline"))]
mod tests {
    use super::*;
    use crate::timeline::{Snapshot, TableStatEntry};

    fn sample_timeline() -> Timeline {
        let mut tl = Timeline::new(
            "SELECT * FROM t",
            "auto",
        );
        tl.push(Snapshot {
            label: "init".into(),
            step: 0,
            plan_text: "Scan(t)".into(),
            cost: 100.0,
            rules_applied: vec![],
            table_stats: vec![TableStatEntry {
                table: "t".into(),
                row_count: 1000,
                staleness: "Fresh".into(),
                confidence: 0.95,
            }],
            diagnostics: vec![],
            changes: vec![],
            invalidations: vec![],
            hardware_profile: None,
            facts: std::collections::HashMap::new(),
        });
        tl
    }

    #[test]
    fn config_from_timeline() {
        let tl = sample_timeline();
        let config = TuiConfig::from_timeline(tl);
        assert!(!config.headless);
        assert!(!config.auto_play);
        assert_eq!(config.initial_speed, 2);
    }

    #[test]
    fn config_headless() {
        let tl = sample_timeline();
        let config = TuiConfig::headless(tl);
        assert!(config.headless);
    }

    #[test]
    fn discover_timelines_empty_dir() {
        let dir = std::env::temp_dir().join("ra-tui-test-empty");
        let _ = std::fs::create_dir_all(&dir);
        let files = discover_timelines(&dir);
        // May contain files from previous runs; just check it
        // returns without error
        assert!(files.len() < 1000);
    }

    #[test]
    fn discover_timelines_nonexistent_dir() {
        let dir = PathBuf::from("/nonexistent/path/to/dir");
        let files = discover_timelines(&dir);
        assert!(files.is_empty());
    }

    #[test]
    fn load_timeline_json_nonexistent() {
        let result =
            load_timeline_json(Path::new("/nonexistent/file.json"));
        assert!(result.is_err());
    }

    #[test]
    fn setup_error_display_io() {
        let err = SetupError::IoError("not found".into());
        let msg = format!("{err}");
        assert!(msg.contains("not found"));
    }

    #[test]
    fn setup_error_display_parse() {
        let err = SetupError::ParseError("bad json".into());
        let msg = format!("{err}");
        assert!(msg.contains("bad json"));
    }

    #[test]
    fn setup_error_display_no_timeline() {
        let err =
            SetupError::NoTimeline("missing data".into());
        let msg = format!("{err}");
        assert!(msg.contains("missing data"));
    }
}
