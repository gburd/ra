//! Plan diff computation and colorized rendering.
//!
//! Computes structural differences between original and optimized
//! relational algebra plans, then renders them with ANSI colors.

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]

use std::fmt::Write;
use std::io::IsTerminal;

use colored::{ColoredString, Colorize};
use ra_core::algebra::RelExpr;

use crate::display::format_plan_tree;

// ── Types ───────────────────────────────────────────────────

/// A single change between two plan nodes.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// The operator type changed (e.g., Join -> Semi Join).
    OperatorType { from: String, to: String },
    /// The join algorithm or strategy changed.
    Algorithm { from: String, to: String },
    /// A node was removed from the plan.
    Removed { description: String },
    /// A node was added to the plan.
    Added { description: String },
    /// A structural modification occurred.
    Structure { description: String },
}

/// Classification of a diff node.
#[derive(Debug, Clone, PartialEq)]
pub enum DiffNode {
    /// Node is unchanged between plans.
    Unchanged { label: String },
    /// Node was removed from the plan.
    Removed { label: String },
    /// Node was added to the plan.
    Added { label: String },
    /// Node was modified between plans.
    Modified { label: String, changes: Vec<Change> },
}

/// Result of diffing two query plans.
#[derive(Debug, Clone)]
pub struct PlanDiff {
    /// Individual diff nodes describing each change.
    pub nodes: Vec<DiffNode>,
    /// Summary of the diff.
    pub summary: DiffSummary,
}

/// Aggregate statistics about a plan diff.
#[derive(Debug, Clone, Default)]
pub struct DiffSummary {
    /// Number of unchanged nodes.
    pub unchanged: usize,
    /// Number of removed nodes.
    pub removed: usize,
    /// Number of added nodes.
    pub added: usize,
    /// Number of modified nodes.
    pub modified: usize,
}

// ── Color configuration ─────────────────────────────────────

/// Controls whether color output is enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// Automatically detect terminal capability.
    Auto,
    /// Force color output.
    Always,
    /// Disable color output.
    Never,
}

/// Output format for diffs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffFormat {
    /// Colored inline diff.
    Colored,
    /// Plain text diff (no ANSI codes).
    Plain,
    /// Side-by-side comparison.
    SideBySide,
    /// Compact one-line summary.
    Compact,
}

// ── Color detection ─────────────────────────────────────────

/// Detect whether the current environment supports colors.
#[must_use]
pub fn detect_color_support() -> bool {
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }
    if std::env::var("FORCE_COLOR").is_ok() {
        return true;
    }
    if let Ok(term) = std::env::var("TERM") {
        if term == "dumb" {
            return false;
        }
    }
    // Check if stderr is a TTY (we write to stderr)
    std::io::stderr().is_terminal()
}

/// Apply the color mode, configuring the `colored` crate.
pub fn apply_color_mode(mode: ColorMode) {
    match mode {
        ColorMode::Auto => {
            if !detect_color_support() {
                colored::control::set_override(false);
            }
        }
        ColorMode::Always => {
            colored::control::set_override(true);
        }
        ColorMode::Never => {
            colored::control::set_override(false);
        }
    }
}

// ── Diff computation ────────────────────────────────────────

/// Extract the operator label for a `RelExpr` node.
fn operator_label(expr: &RelExpr) -> String {
    match expr {
        RelExpr::Scan { table, alias } => {
            let mut s = format!("Scan({table})");
            if let Some(a) = alias {
                let _ = write!(s, " AS {a}");
            }
            s
        }
        RelExpr::Filter { .. } => "Filter".to_owned(),
        RelExpr::Project { .. } => "Project".to_owned(),
        RelExpr::Join { join_type, .. } => {
            format!("{join_type} Join")
        }
        RelExpr::Aggregate { .. } => "Aggregate".to_owned(),
        RelExpr::Sort { .. } => "Sort".to_owned(),
        RelExpr::IncrementalSort {
            prefix_keys,
            suffix_keys,
            ..
        } => {
            format!(
                "IncrementalSort(prefix={}, suffix={})",
                prefix_keys.len(),
                suffix_keys.len()
            )
        }
        RelExpr::Limit { count, offset, .. } => {
            format!("Limit(count={count}, offset={offset})")
        }
        RelExpr::Union { all, .. } => if *all { "Union ALL" } else { "Union" }.to_owned(),
        RelExpr::Intersect { all, .. } => {
            if *all { "Intersect ALL" } else { "Intersect" }.to_owned()
        }
        RelExpr::Except { all, .. } => if *all { "Except ALL" } else { "Except" }.to_owned(),
        RelExpr::CTE { name, .. } => format!("CTE({name})"),
        RelExpr::Window { functions, .. } => {
            format!("Window({} fn)", functions.len())
        }
        RelExpr::Distinct { .. } => "Distinct".to_owned(),
        RelExpr::Values { rows } => {
            format!("Values({} rows)", rows.len())
        }
        RelExpr::RecursiveCTE { name, .. } => {
            format!("RecursiveCTE({name})")
        }
        RelExpr::Unnest { alias, .. } => {
            format!("Unnest({})", alias.as_deref().unwrap_or("?"))
        }
        RelExpr::MultiUnnest { .. } => "MultiUnnest".to_owned(),
        RelExpr::TableFunction { name, .. } => {
            format!("TableFunction({name})")
        }
        RelExpr::RowPattern { .. } => "MatchRecognize".to_owned(),
        RelExpr::IndexScan { table, column, .. } => {
            format!("IndexScan({table}.{column})")
        }
        RelExpr::BitmapIndexScan { table, index, .. } => {
            format!("BitmapIndexScan({table}.{index})")
        }
        RelExpr::BitmapAnd { inputs } => {
            format!("BitmapAnd({} inputs)", inputs.len())
        }
        RelExpr::BitmapOr { inputs } => {
            format!("BitmapOr({} inputs)", inputs.len())
        }
        RelExpr::BitmapHeapScan { table, .. } => {
            format!("BitmapHeapScan({table})")
        }
        RelExpr::IndexOnlyScan { table, index, .. } => {
            format!("IndexOnlyScan({table}.{index})")
        }
        RelExpr::ParallelScan { table, .. } => {
            format!("ParallelScan({table})")
        }
        RelExpr::ParallelHashJoin { join_type, .. } => {
            format!("Parallel{join_type}Join")
        }
        RelExpr::ParallelAggregate { .. } => "ParallelAggregate".to_owned(),
        RelExpr::Gather { .. } => "Gather".to_owned(),
        RelExpr::MvScan { view_name, .. } => {
            format!("MvScan({view_name})")
        }
        RelExpr::TopK { k, .. } => {
            format!("TopK(k={k})")
        }
        RelExpr::VectorFilter { threshold, .. } => {
            format!("VectorFilter(threshold={threshold})")
        }
    }
}

/// Extract an ordered list of operator labels from a plan by
/// pre-order traversal.
fn flatten_plan(expr: &RelExpr) -> Vec<String> {
    let mut labels = Vec::new();
    flatten_plan_impl(expr, &mut labels);
    labels
}

fn flatten_plan_impl(expr: &RelExpr, out: &mut Vec<String>) {
    out.push(operator_label(expr));
    for child in expr.children() {
        flatten_plan_impl(child, out);
    }
}

/// Compute the diff between two plans.
#[must_use]
pub fn compute_diff(original: &RelExpr, optimized: &RelExpr) -> PlanDiff {
    let orig_labels = flatten_plan(original);
    let opt_labels = flatten_plan(optimized);

    let mut nodes = Vec::new();

    // Use a simple LCS-based diff on the flattened label sequences.
    let lcs = longest_common_subsequence(&orig_labels, &opt_labels);

    let mut oi = 0;
    let mut ni = 0;
    let mut li = 0;

    while oi < orig_labels.len() || ni < opt_labels.len() {
        if li < lcs.len()
            && oi < orig_labels.len()
            && ni < opt_labels.len()
            && orig_labels[oi] == lcs[li]
            && opt_labels[ni] == lcs[li]
        {
            // Matched in both
            nodes.push(DiffNode::Unchanged {
                label: orig_labels[oi].clone(),
            });
            oi += 1;
            ni += 1;
            li += 1;
        } else if li < lcs.len()
            && oi < orig_labels.len()
            && orig_labels[oi] != lcs[li]
            && ni < opt_labels.len()
            && opt_labels[ni] != lcs[li]
        {
            // Both differ from LCS -- classify the change
            let changes = classify_changes(&orig_labels[oi], &opt_labels[ni]);
            nodes.push(DiffNode::Modified {
                label: opt_labels[ni].clone(),
                changes,
            });
            oi += 1;
            ni += 1;
        } else if oi < orig_labels.len() && (li >= lcs.len() || orig_labels[oi] != lcs[li]) {
            nodes.push(DiffNode::Removed {
                label: orig_labels[oi].clone(),
            });
            oi += 1;
        } else if ni < opt_labels.len() && (li >= lcs.len() || opt_labels[ni] != lcs[li]) {
            nodes.push(DiffNode::Added {
                label: opt_labels[ni].clone(),
            });
            ni += 1;
        } else {
            // Safety: at least one index should advance
            break;
        }
    }

    let summary = compute_summary(&nodes);
    PlanDiff { nodes, summary }
}

/// Classify the type of change between two operator labels.
fn classify_changes(from: &str, to: &str) -> Vec<Change> {
    let from_base = extract_operator_base(from);
    let to_base = extract_operator_base(to);

    // Same base operator (e.g., both are Joins) but different specifics
    if from_base == to_base && from_base == "Join" {
        return vec![Change::Algorithm {
            from: from.to_owned(),
            to: to.to_owned(),
        }];
    }

    // One was removed and another added at the same position
    if from_base != to_base {
        return vec![
            Change::Removed {
                description: from.to_owned(),
            },
            Change::Added {
                description: to.to_owned(),
            },
            Change::Structure {
                description: format!("replaced {from_base} with {to_base}"),
            },
        ];
    }

    // Default: operator type change
    vec![Change::OperatorType {
        from: from.to_owned(),
        to: to.to_owned(),
    }]
}

/// Extract the base operator name from a label (e.g., "INNER Join" -> "Join").
fn extract_operator_base(label: &str) -> &str {
    if label.contains("Join") {
        return "Join";
    }
    if label.starts_with("Scan(") {
        return "Scan";
    }
    if label.starts_with("Limit(") {
        return "Limit";
    }
    if label.starts_with("CTE(") {
        return "CTE";
    }
    if label.starts_with("RecursiveCTE(") {
        return "RecursiveCTE";
    }
    if label.starts_with("Values(") {
        return "Values";
    }
    if label.starts_with("Window(") {
        return "Window";
    }
    // For simple labels like "Filter", "Project", etc.
    label.split('(').next().unwrap_or(label)
}

fn compute_summary(nodes: &[DiffNode]) -> DiffSummary {
    let mut s = DiffSummary::default();
    for node in nodes {
        match node {
            DiffNode::Unchanged { .. } => s.unchanged += 1,
            DiffNode::Removed { .. } => s.removed += 1,
            DiffNode::Added { .. } => s.added += 1,
            DiffNode::Modified { .. } => s.modified += 1,
        }
    }
    s
}

/// Compute the longest common subsequence of two string slices.
fn longest_common_subsequence(left: &[String], right: &[String]) -> Vec<String> {
    let rows = left.len();
    let cols = right.len();
    let mut dp = vec![vec![0u32; cols + 1]; rows + 1];

    for row in 1..=rows {
        for col in 1..=cols {
            if left[row - 1] == right[col - 1] {
                dp[row][col] = dp[row - 1][col - 1] + 1;
            } else {
                dp[row][col] = dp[row - 1][col].max(dp[row][col - 1]);
            }
        }
    }

    // Backtrack to find the subsequence
    let mut result = Vec::new();
    let mut row = rows;
    let mut col = cols;
    while row > 0 && col > 0 {
        if left[row - 1] == right[col - 1] {
            result.push(left[row - 1].clone());
            row -= 1;
            col -= 1;
        } else if dp[row - 1][col] > dp[row][col - 1] {
            row -= 1;
        } else {
            col -= 1;
        }
    }
    result.reverse();
    result
}

// ── Rendering ───────────────────────────────────────────────

/// Render a plan diff with full ANSI color support.
#[must_use]
pub fn render_colored(diff: &PlanDiff) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{}", "Plan Diff:".bold());
    let _ = writeln!(out);

    for node in &diff.nodes {
        match node {
            DiffNode::Unchanged { label } => {
                let _ = writeln!(out, "  {label}");
            }
            DiffNode::Removed { label } => {
                let _ = writeln!(out, "  {} {}", "-".red().bold(), label.red());
            }
            DiffNode::Added { label } => {
                let _ = writeln!(out, "  {} {}", "+".green().bold(), label.green());
            }
            DiffNode::Modified { label, changes } => {
                let _ = writeln!(out, "  {} {}", "~".yellow().bold(), label.yellow());
                for change in changes {
                    let _ = writeln!(out, "    {}", format_change_colored(change));
                }
            }
        }
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "{}", format_summary_colored(&diff.summary));

    out
}

/// Render a plan diff as plain text (no ANSI codes).
#[must_use]
pub fn render_plain(diff: &PlanDiff) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Plan Diff:");
    let _ = writeln!(out);

    for node in &diff.nodes {
        match node {
            DiffNode::Unchanged { label } => {
                let _ = writeln!(out, "  {label}");
            }
            DiffNode::Removed { label } => {
                let _ = writeln!(out, "  - {label}");
            }
            DiffNode::Added { label } => {
                let _ = writeln!(out, "  + {label}");
            }
            DiffNode::Modified { label, changes } => {
                let _ = writeln!(out, "  ~ {label}");
                for change in changes {
                    let _ = writeln!(out, "    {}", format_change_plain(change));
                }
            }
        }
    }

    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Summary: {} unchanged, {} removed, {} added, {} modified",
        diff.summary.unchanged, diff.summary.removed, diff.summary.added, diff.summary.modified,
    );

    out
}

/// Render a compact one-line summary.
#[must_use]
pub fn render_compact(diff: &PlanDiff) -> String {
    let s = &diff.summary;
    let total_changes = s.removed + s.added + s.modified;

    if total_changes == 0 {
        return format!(
            "{}",
            "No changes between original and optimized plan.".dimmed()
        );
    }

    let mut parts = Vec::new();
    if s.removed > 0 {
        parts.push(format!("{}", format!("-{}", s.removed).red()));
    }
    if s.added > 0 {
        parts.push(format!("{}", format!("+{}", s.added).green()));
    }
    if s.modified > 0 {
        parts.push(format!("{}", format!("~{}", s.modified).yellow()));
    }

    format!(
        "{}: {} ({})",
        "Diff".bold(),
        parts.join(", "),
        format!("{total_changes} change(s)").cyan(),
    )
}

/// Render the full diff output according to the specified format.
#[must_use]
pub fn render_diff(original: &RelExpr, optimized: &RelExpr, format: DiffFormat) -> String {
    let diff = compute_diff(original, optimized);

    match format {
        DiffFormat::Colored => render_colored(&diff),
        DiffFormat::Plain => render_plain(&diff),
        DiffFormat::Compact => render_compact(&diff),
        DiffFormat::SideBySide => {
            let orig_text = format_plan_tree(original);
            let opt_text = format_plan_tree(optimized);
            crate::side_by_side::render_side_by_side(&orig_text, &opt_text)
        }
    }
}

// ── Internal formatting helpers ─────────────────────────────

fn format_change_colored(change: &Change) -> ColoredString {
    match change {
        Change::OperatorType { from, to } => format!("{from} -> {to}").yellow(),
        Change::Algorithm { from, to } => format!("algorithm: {from} -> {to}").cyan(),
        Change::Removed { description } => format!("removed: {description}").red(),
        Change::Added { description } => format!("added: {description}").green(),
        Change::Structure { description } => description.clone().yellow(),
    }
}

fn format_change_plain(change: &Change) -> String {
    match change {
        Change::OperatorType { from, to } => {
            format!("{from} -> {to}")
        }
        Change::Algorithm { from, to } => {
            format!("algorithm: {from} -> {to}")
        }
        Change::Removed { description } => {
            format!("removed: {description}")
        }
        Change::Added { description } => {
            format!("added: {description}")
        }
        Change::Structure { description } => description.clone(),
    }
}

fn format_summary_colored(summary: &DiffSummary) -> String {
    let total = summary.unchanged + summary.removed + summary.added + summary.modified;
    let mut parts = Vec::new();

    if summary.unchanged > 0 {
        parts.push(format!("{} unchanged", summary.unchanged));
    }
    if summary.removed > 0 {
        parts.push(format!("{}", format!("{} removed", summary.removed).red()));
    }
    if summary.added > 0 {
        parts.push(format!("{}", format!("{} added", summary.added).green()));
    }
    if summary.modified > 0 {
        parts.push(format!(
            "{}",
            format!("{} modified", summary.modified).yellow()
        ));
    }

    format!(
        "{}: {} ({total} total node(s))",
        "Summary".bold(),
        parts.join(", "),
    )
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::{JoinType, ProjectionColumn};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn simple_scan() -> RelExpr {
        RelExpr::scan("users")
    }

    fn scan_with_filter() -> RelExpr {
        RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        })
    }

    fn join_plan() -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("u", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("o", "user_id"))),
            },
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        }
    }

    fn semi_join_plan() -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Semi,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("u", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("o", "user_id"))),
            },
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        }
    }

    // ── operator_label tests ────────────────────────────────

    #[test]
    fn label_scan() {
        assert_eq!(operator_label(&simple_scan()), "Scan(users)");
    }

    #[test]
    fn label_scan_with_alias() {
        let plan = RelExpr::Scan {
            table: "users".to_owned(),
            alias: Some("u".to_owned()),
        };
        assert_eq!(operator_label(&plan), "Scan(users) AS u");
    }

    #[test]
    fn label_filter() {
        assert_eq!(operator_label(&scan_with_filter()), "Filter");
    }

    #[test]
    fn label_project() {
        let plan = simple_scan().project(vec![ProjectionColumn {
            expr: Expr::Column(ColumnRef::new("name")),
            alias: None,
        }]);
        assert_eq!(operator_label(&plan), "Project");
    }

    #[test]
    fn label_inner_join() {
        assert_eq!(operator_label(&join_plan()), "INNER Join");
    }

    #[test]
    fn label_semi_join() {
        assert_eq!(operator_label(&semi_join_plan()), "SEMI Join");
    }

    #[test]
    fn label_aggregate() {
        let plan = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![],
            input: Box::new(simple_scan()),
        };
        assert_eq!(operator_label(&plan), "Aggregate");
    }

    #[test]
    fn label_sort() {
        let plan = RelExpr::Sort {
            keys: vec![],
            input: Box::new(simple_scan()),
        };
        assert_eq!(operator_label(&plan), "Sort");
    }

    #[test]
    fn label_limit() {
        let plan = simple_scan().limit(10, 5);
        assert_eq!(operator_label(&plan), "Limit(count=10, offset=5)");
    }

    #[test]
    fn label_distinct() {
        let plan = simple_scan().distinct();
        assert_eq!(operator_label(&plan), "Distinct");
    }

    #[test]
    fn label_union_all() {
        let plan = RelExpr::Union {
            all: true,
            left: Box::new(simple_scan()),
            right: Box::new(simple_scan()),
        };
        assert_eq!(operator_label(&plan), "Union ALL");
    }

    #[test]
    fn label_union_distinct() {
        let plan = RelExpr::Union {
            all: false,
            left: Box::new(simple_scan()),
            right: Box::new(simple_scan()),
        };
        assert_eq!(operator_label(&plan), "Union");
    }

    #[test]
    fn label_cte() {
        let plan = RelExpr::CTE {
            name: "tmp".to_owned(),
            definition: Box::new(simple_scan()),
            body: Box::new(simple_scan()),
        };
        assert_eq!(operator_label(&plan), "CTE(tmp)");
    }

    #[test]
    fn label_recursive_cte() {
        let plan = RelExpr::RecursiveCTE {
            name: "r".to_owned(),
            base_case: Box::new(simple_scan()),
            recursive_case: Box::new(simple_scan()),
            body: Box::new(simple_scan()),
            cycle_detection: None,
        };
        assert_eq!(operator_label(&plan), "RecursiveCTE(r)");
    }

    #[test]
    fn label_values() {
        let plan = RelExpr::Values {
            rows: vec![
                vec![Expr::Const(Const::Int(1))],
                vec![Expr::Const(Const::Int(2))],
            ],
        };
        assert_eq!(operator_label(&plan), "Values(2 rows)");
    }

    // ── flatten_plan tests ──────────────────────────────────

    #[test]
    fn flatten_single_scan() {
        let labels = flatten_plan(&simple_scan());
        assert_eq!(labels, vec!["Scan(users)"]);
    }

    #[test]
    fn flatten_filter_scan() {
        let labels = flatten_plan(&scan_with_filter());
        assert_eq!(labels, vec!["Filter", "Scan(users)"]);
    }

    #[test]
    fn flatten_join() {
        let labels = flatten_plan(&join_plan());
        assert_eq!(labels, vec!["INNER Join", "Scan(users)", "Scan(orders)"]);
    }

    // ── LCS tests ───────────────────────────────────────────

    #[test]
    fn lcs_identical() {
        let a = vec!["A".to_owned(), "B".to_owned()];
        let b = vec!["A".to_owned(), "B".to_owned()];
        let lcs = longest_common_subsequence(&a, &b);
        assert_eq!(lcs, vec!["A", "B"]);
    }

    #[test]
    fn lcs_empty() {
        let a: Vec<String> = vec![];
        let b: Vec<String> = vec![];
        let lcs = longest_common_subsequence(&a, &b);
        assert!(lcs.is_empty());
    }

    #[test]
    fn lcs_one_empty() {
        let a = vec!["A".to_owned()];
        let b: Vec<String> = vec![];
        let lcs = longest_common_subsequence(&a, &b);
        assert!(lcs.is_empty());
    }

    #[test]
    fn lcs_no_common() {
        let a = vec!["A".to_owned(), "B".to_owned()];
        let b = vec!["C".to_owned(), "D".to_owned()];
        let lcs = longest_common_subsequence(&a, &b);
        assert!(lcs.is_empty());
    }

    #[test]
    fn lcs_partial_match() {
        let a = vec!["Filter".to_owned(), "Scan(users)".to_owned()];
        let b = vec![
            "Project".to_owned(),
            "Filter".to_owned(),
            "Scan(users)".to_owned(),
        ];
        let lcs = longest_common_subsequence(&a, &b);
        assert_eq!(lcs, vec!["Filter", "Scan(users)"]);
    }

    // ── compute_diff tests ──────────────────────────────────

    #[test]
    fn diff_identical_plans() {
        let plan = scan_with_filter();
        let diff = compute_diff(&plan, &plan);
        assert_eq!(diff.summary.unchanged, 2);
        assert_eq!(diff.summary.removed, 0);
        assert_eq!(diff.summary.added, 0);
        assert_eq!(diff.summary.modified, 0);
    }

    #[test]
    fn diff_added_node() {
        let original = simple_scan();
        let optimized = scan_with_filter();
        let diff = compute_diff(&original, &optimized);
        assert_eq!(diff.summary.added, 1);
        assert_eq!(diff.summary.unchanged, 1);
    }

    #[test]
    fn diff_removed_node() {
        let original = scan_with_filter();
        let optimized = simple_scan();
        let diff = compute_diff(&original, &optimized);
        assert_eq!(diff.summary.removed, 1);
        assert_eq!(diff.summary.unchanged, 1);
    }

    #[test]
    fn diff_modified_join_type() {
        let original = join_plan();
        let optimized = semi_join_plan();
        let diff = compute_diff(&original, &optimized);
        assert!(diff.summary.modified > 0);
    }

    #[test]
    fn diff_completely_different() {
        let original = simple_scan();
        let optimized = RelExpr::scan("orders");
        let diff = compute_diff(&original, &optimized);
        assert!(diff.summary.removed > 0 || diff.summary.modified > 0);
    }

    #[test]
    fn diff_empty_values_to_scan() {
        let original = RelExpr::Values { rows: vec![] };
        let optimized = simple_scan();
        let diff = compute_diff(&original, &optimized);
        let total = diff.summary.removed + diff.summary.added + diff.summary.modified;
        assert!(total > 0);
    }

    // ── render tests ────────────────────────────────────────

    #[test]
    fn render_colored_identical() {
        let plan = simple_scan();
        let diff = compute_diff(&plan, &plan);
        let output = render_colored(&diff);
        assert!(output.contains("Scan(users)"));
        assert!(output.contains("Summary"));
    }

    #[test]
    fn render_colored_contains_ansi_on_changes() {
        // Serialize tests that modify colored's global override to prevent
        // parallel test interference.
        use std::sync::Mutex;
        static COLORED_LOCK: Mutex<()> = Mutex::new(());
        let _guard = COLORED_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        colored::control::set_override(true);
        let diff = compute_diff(&simple_scan(), &scan_with_filter());
        let output = render_colored(&diff);
        colored::control::unset_override();

        // ANSI escape codes start with \x1b[
        assert!(
            output.contains("\x1b["),
            "expected ANSI codes in colored output"
        );
    }

    #[test]
    fn render_plain_no_ansi() {
        let diff = compute_diff(&simple_scan(), &scan_with_filter());
        let output = render_plain(&diff);
        assert!(
            !output.contains("\x1b["),
            "plain output should not contain ANSI codes"
        );
        assert!(output.contains("+ Filter"));
    }

    #[test]
    fn render_plain_identical() {
        let plan = simple_scan();
        let diff = compute_diff(&plan, &plan);
        let output = render_plain(&diff);
        assert!(output.contains("Scan(users)"));
        assert!(output.contains("0 removed"));
        assert!(output.contains("0 added"));
    }

    #[test]
    fn render_compact_no_changes() {
        let plan = simple_scan();
        let diff = compute_diff(&plan, &plan);
        let output = render_compact(&diff);
        assert!(output.contains("No changes"));
    }

    #[test]
    fn render_compact_with_changes() {
        colored::control::set_override(true);
        let diff = compute_diff(&simple_scan(), &scan_with_filter());
        let output = render_compact(&diff);
        assert!(output.contains("Diff"));
        assert!(output.contains("change"));
        colored::control::unset_override();
    }

    #[test]
    fn render_diff_colored_format() {
        let output = render_diff(&simple_scan(), &scan_with_filter(), DiffFormat::Colored);
        assert!(output.contains("Plan Diff"));
    }

    #[test]
    fn render_diff_plain_format() {
        let output = render_diff(&simple_scan(), &scan_with_filter(), DiffFormat::Plain);
        assert!(output.contains("Plan Diff"));
        assert!(!output.contains("\x1b["));
    }

    #[test]
    fn render_diff_compact_format() {
        let output = render_diff(&simple_scan(), &scan_with_filter(), DiffFormat::Compact);
        assert!(output.contains("Diff") || output.contains("No changes"));
    }

    #[test]
    fn render_diff_side_by_side_format() {
        let output = render_diff(&simple_scan(), &scan_with_filter(), DiffFormat::SideBySide);
        assert!(output.contains("Original"));
        assert!(output.contains("Optimized"));
    }

    // ── Change formatting tests ─────────────────────────────

    #[test]
    fn format_operator_type_change_plain() {
        let change = Change::OperatorType {
            from: "INNER Join".to_owned(),
            to: "SEMI Join".to_owned(),
        };
        let output = format_change_plain(&change);
        assert_eq!(output, "INNER Join -> SEMI Join");
    }

    #[test]
    fn format_algorithm_change_plain() {
        let change = Change::Algorithm {
            from: "NestedLoop".to_owned(),
            to: "HashJoin".to_owned(),
        };
        let output = format_change_plain(&change);
        assert_eq!(output, "algorithm: NestedLoop -> HashJoin");
    }

    #[test]
    fn format_removed_change_plain() {
        let change = Change::Removed {
            description: "Sort node".to_owned(),
        };
        let output = format_change_plain(&change);
        assert_eq!(output, "removed: Sort node");
    }

    #[test]
    fn format_added_change_plain() {
        let change = Change::Added {
            description: "Index scan".to_owned(),
        };
        let output = format_change_plain(&change);
        assert_eq!(output, "added: Index scan");
    }

    #[test]
    fn format_structure_change_plain() {
        let change = Change::Structure {
            description: "reordered joins".to_owned(),
        };
        let output = format_change_plain(&change);
        assert_eq!(output, "reordered joins");
    }

    // ── DiffSummary tests ───────────────────────────────────

    #[test]
    fn summary_default() {
        let s = DiffSummary::default();
        assert_eq!(s.unchanged, 0);
        assert_eq!(s.removed, 0);
        assert_eq!(s.added, 0);
        assert_eq!(s.modified, 0);
    }

    #[test]
    fn compute_summary_counts() {
        let nodes = vec![
            DiffNode::Unchanged {
                label: "a".to_owned(),
            },
            DiffNode::Unchanged {
                label: "b".to_owned(),
            },
            DiffNode::Removed {
                label: "c".to_owned(),
            },
            DiffNode::Added {
                label: "d".to_owned(),
            },
            DiffNode::Modified {
                label: "e".to_owned(),
                changes: vec![],
            },
        ];
        let s = compute_summary(&nodes);
        assert_eq!(s.unchanged, 2);
        assert_eq!(s.removed, 1);
        assert_eq!(s.added, 1);
        assert_eq!(s.modified, 1);
    }

    // ── Color detection tests ───────────────────────────────

    #[test]
    fn color_mode_enum_values() {
        assert_ne!(ColorMode::Auto, ColorMode::Always);
        assert_ne!(ColorMode::Always, ColorMode::Never);
        assert_ne!(ColorMode::Auto, ColorMode::Never);
    }

    #[test]
    fn diff_format_enum_values() {
        assert_ne!(DiffFormat::Colored, DiffFormat::Plain);
        assert_ne!(DiffFormat::SideBySide, DiffFormat::Compact);
    }

    // ── Complex plan diff tests ─────────────────────────────

    #[test]
    fn diff_nested_plan_with_project() {
        let original = RelExpr::scan("users")
            .filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(18))),
            })
            .project(vec![ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("name")),
                alias: None,
            }]);

        let optimized = RelExpr::scan("users")
            .project(vec![ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("name")),
                alias: None,
            }])
            .filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(18))),
            });

        let diff = compute_diff(&original, &optimized);
        // Project and Filter are swapped; LCS matches 2 of 3 labels
        assert_eq!(diff.summary.unchanged, 2);
    }

    #[test]
    fn diff_join_plan_to_filter_plan() {
        let original = join_plan();
        let optimized = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Const(Const::Int(1))),
        });
        let diff = compute_diff(&original, &optimized);
        let total = diff.summary.removed
            + diff.summary.added
            + diff.summary.modified
            + diff.summary.unchanged;
        assert!(total > 0);
    }
}
