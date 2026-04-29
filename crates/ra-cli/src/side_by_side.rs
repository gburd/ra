//! Side-by-side plan comparison rendering.
//!
//! Renders original and optimized plans in a two-column layout
//! for wide terminals, with a synchronized view of both plans.

#![allow(clippy::cast_possible_truncation)]

use std::fmt::Write;

use colored::Colorize;

// ── Public API ──────────────────────────────────────────────

/// Render two plan texts side by side.
///
/// Uses a two-column layout when the content fits, with column
/// headers "Original" and "Optimized".
#[must_use]
pub fn render_side_by_side(original: &str, optimized: &str) -> String {
    let term_width = terminal_width();
    render_side_by_side_width(original, optimized, term_width)
}

/// Render side by side with an explicit terminal width.
///
/// If the terminal is too narrow (< 80 columns), falls back to
/// a stacked vertical layout.
#[must_use]
pub fn render_side_by_side_width(original: &str, optimized: &str, width: usize) -> String {
    if width < 80 {
        return render_stacked(original, optimized);
    }

    let sep = " | ";
    let cw = (width - sep.len()) / 2;

    let orig_lines: Vec<&str> = original.lines().collect();
    let opt_lines: Vec<&str> = optimized.lines().collect();
    let max_lines = orig_lines.len().max(opt_lines.len());

    let mut out = String::new();

    // Headers
    let _ = writeln!(
        out,
        "{:<cw$}{sep}{:<cw$}",
        format!("{}", "Original".bold()),
        format!("{}", "Optimized".bold()),
    );
    let _ = writeln!(out, "{:<cw$}{sep}{:<cw$}", "-".repeat(cw), "-".repeat(cw),);

    for idx in 0..max_lines {
        let left = orig_lines.get(idx).copied().unwrap_or("");
        let right = opt_lines.get(idx).copied().unwrap_or("");

        let lt = truncate_str(left, cw);
        let rt = truncate_str(right, cw);

        let _ = writeln!(out, "{lt:<cw$}{sep}{rt:<cw$}");
    }

    out
}

// ── Stacked fallback ────────────────────────────────────────

/// Render plans stacked vertically for narrow terminals.
fn render_stacked(original: &str, optimized: &str) -> String {
    let mut out = String::new();

    let _ = writeln!(out, "{}", "Original:".bold());
    let _ = writeln!(out, "{original}");
    let _ = writeln!(out);
    let _ = writeln!(out, "{}", "Optimized:".bold());
    let _ = writeln!(out, "{optimized}");

    out
}

// ── Helpers ─────────────────────────────────────────────────

/// Get the terminal width, defaulting to 120 columns.
fn terminal_width() -> usize {
    // Try COLUMNS env var first
    if let Ok(cols) = std::env::var("COLUMNS") {
        if let Ok(w) = cols.parse::<usize>() {
            if w > 0 {
                return w;
            }
        }
    }

    // Try terminal_size via ioctl (crossterm does this)
    if let Some((w, _)) = term_size() {
        return w;
    }

    120
}

/// Query the terminal size using TIOCGWINSZ ioctl.
fn term_size() -> Option<(usize, usize)> {
    // Use a simple approach: try to get from crossterm if available,
    // otherwise fall back to environment.
    #[cfg(unix)]
    {
        // Use libc::winsize directly (C struct from <sys/ioctl.h>).
        let mut size = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: TIOCGWINSZ is a well-known ioctl for getting terminal
        // size. The winsize struct layout matches the kernel's expectation.
        let ret = unsafe { libc::ioctl(libc::STDERR_FILENO, libc::TIOCGWINSZ, &mut size) };
        if ret == 0 && size.ws_col > 0 && size.ws_row > 0 {
            return Some((size.ws_col as usize, size.ws_row as usize));
        }
    }

    None
}

/// Truncate a string to fit within `max_width` characters.
fn truncate_str(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        return s.to_owned();
    }
    if max_width < 4 {
        return s.chars().take(max_width).collect();
    }
    let mut result: String = s.chars().take(max_width - 3).collect();
    result.push_str("...");
    result
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_by_side_basic() {
        let left = "Line 1\nLine 2\nLine 3";
        let right = "Line A\nLine B";
        let output = render_side_by_side_width(left, right, 120);
        assert!(output.contains("Original"));
        assert!(output.contains("Optimized"));
        assert!(output.contains("Line 1"));
        assert!(output.contains("Line A"));
    }

    #[test]
    fn side_by_side_equal_lines() {
        let text = "Scan(users)\nFilter\nProject";
        let output = render_side_by_side_width(text, text, 120);
        assert!(output.contains("Scan(users)"));
        assert!(output.contains("Filter"));
        assert!(output.contains("Project"));
    }

    #[test]
    fn side_by_side_narrow_falls_back() {
        let left = "Left plan";
        let right = "Right plan";
        let output = render_side_by_side_width(left, right, 60);
        // Narrow terminal falls back to stacked
        assert!(output.contains("Original:"));
        assert!(output.contains("Optimized:"));
    }

    #[test]
    fn side_by_side_very_narrow() {
        let left = "A";
        let right = "B";
        let output = render_side_by_side_width(left, right, 40);
        assert!(output.contains("Original:"));
        assert!(output.contains("Optimized:"));
    }

    #[test]
    fn side_by_side_empty_left() {
        let output = render_side_by_side_width("", "Right", 120);
        assert!(output.contains("Original"));
        assert!(output.contains("Right"));
    }

    #[test]
    fn side_by_side_empty_right() {
        let output = render_side_by_side_width("Left", "", 120);
        assert!(output.contains("Left"));
        assert!(output.contains("Optimized"));
    }

    #[test]
    fn side_by_side_both_empty() {
        let output = render_side_by_side_width("", "", 120);
        assert!(output.contains("Original"));
        assert!(output.contains("Optimized"));
    }

    #[test]
    fn side_by_side_long_lines_truncated() {
        let long_line = "x".repeat(200);
        let output = render_side_by_side_width(&long_line, "short", 120);
        // Should contain truncation indicator
        assert!(output.contains("..."));
    }

    #[test]
    fn stacked_rendering() {
        let output = render_stacked("Plan A", "Plan B");
        assert!(output.contains("Original:"));
        assert!(output.contains("Plan A"));
        assert!(output.contains("Optimized:"));
        assert!(output.contains("Plan B"));
    }

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn truncate_exact_length() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn truncate_long_string() {
        let result = truncate_str("hello world", 8);
        assert_eq!(result, "hello...");
    }

    #[test]
    fn truncate_very_short_max() {
        let result = truncate_str("hello", 2);
        assert_eq!(result, "he");
    }

    #[test]
    fn side_by_side_width_120() {
        let left = "Filter\n  Scan(users)";
        let right = "Project\n  Filter\n    Scan(users)";
        let output = render_side_by_side_width(left, right, 120);
        // Check that the separator is present
        assert!(output.contains(" | "));
    }

    #[test]
    fn side_by_side_width_80() {
        let left = "Scan(a)";
        let right = "Scan(b)";
        let output = render_side_by_side_width(left, right, 80);
        assert!(output.contains(" | "));
        assert!(output.contains("Original"));
    }

    #[test]
    fn side_by_side_unequal_line_counts() {
        let left = "A\nB\nC\nD\nE";
        let right = "X";
        let output = render_side_by_side_width(left, right, 120);
        // All left lines should be present
        assert!(output.contains('A'));
        assert!(output.contains('E'));
        assert!(output.contains('X'));
    }

    #[test]
    fn side_by_side_preserves_tree_chars() {
        let left = "└─ Scan(users)";
        let right = "└─ Scan(orders)";
        let output = render_side_by_side_width(left, right, 120);
        assert!(output.contains("Scan(users)"));
        assert!(output.contains("Scan(orders)"));
    }
}
