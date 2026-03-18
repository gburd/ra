//! CLI support for differential plan validation.
//!
//! Provides formatting and output for comparing RA optimizer
//! plans with database EXPLAIN plans.

use colored::Colorize;

use ra_metadata::diff::{DiffAspect, DiffReport};

/// Format a [`DiffReport`] for terminal output.
pub fn format_diff_report(report: &DiffReport) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "  {}: {}\n",
        "Engine".bold(),
        report.engine
    ));
    out.push_str(&format!(
        "  {}: {}\n",
        "Query".bold(),
        report.query
    ));
    out.push_str(&format!(
        "  {}: {:.0}%\n\n",
        "Confidence".bold(),
        report.confidence * 100.0
    ));

    if !report.agreements.is_empty() {
        out.push_str(&format!(
            "{}\n",
            "Agreements:".green().bold()
        ));
        for point in &report.agreements {
            out.push_str(&format!(
                "  {} {}: {} vs {}\n",
                "[AGREE]".green(),
                aspect_label(point.aspect),
                point.ra_value,
                point.db_value,
            ));
            out.push_str(&format!(
                "         {}\n",
                point.explanation.dimmed()
            ));
        }
        out.push('\n');
    }

    if !report.disagreements.is_empty() {
        out.push_str(&format!(
            "{}\n",
            "Disagreements:".red().bold()
        ));
        for point in &report.disagreements {
            out.push_str(&format!(
                "  {} {}: {} vs {}\n",
                "[DIFFER]".red(),
                aspect_label(point.aspect),
                point.ra_value,
                point.db_value,
            ));
            out.push_str(&format!(
                "          {}\n",
                point.explanation.yellow()
            ));
        }
        out.push('\n');
    }

    out.push_str(&format!(
        "{}: {}\n",
        "Summary".bold(),
        report.summary
    ));

    out
}

fn aspect_label(aspect: DiffAspect) -> String {
    format!("[{}]", aspect)
}

/// Format a schema info output for the gather-metadata command.
pub fn format_schema_summary(
    schema: &ra_metadata::SchemaInfo,
) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "  {}: {}\n",
        "Database".bold(),
        schema.database
    ));
    out.push_str(&format!(
        "  {}: {}\n",
        "Tables".bold(),
        schema.tables.len()
    ));
    out.push_str(&format!(
        "  {}: {}\n\n",
        "Views".bold(),
        schema.views.len()
    ));

    for table in &schema.tables {
        out.push_str(&format!(
            "  {}.{} ({} columns, {} indexes",
            table.schema.dimmed(),
            table.name.cyan(),
            table.columns.len(),
            table.indexes.len(),
        ));
        if let Some(rows) = table.estimated_rows {
            out.push_str(&format!(", ~{rows} rows"));
        }
        out.push_str(")\n");
    }

    if !schema.views.is_empty() {
        out.push_str(&format!(
            "\n{}:\n",
            "Views".bold()
        ));
        for view in &schema.views {
            out.push_str(&format!(
                "  {}.{}\n",
                view.schema.dimmed(),
                view.name.cyan(),
            ));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_metadata::diff::{DiffPoint, DiffReport};

    #[test]
    fn format_empty_report() {
        let report = DiffReport {
            query: "SELECT 1".to_string(),
            engine: "SQLite".to_string(),
            agreements: vec![],
            disagreements: vec![],
            confidence: 0.5,
            summary: "No comparison points".to_string(),
        };

        let output = format_diff_report(&report);
        assert!(output.contains("SQLite"));
        assert!(output.contains("SELECT 1"));
        assert!(output.contains("50%"));
    }

    #[test]
    fn format_report_with_agreements() {
        let report = DiffReport {
            query: "SELECT * FROM users".to_string(),
            engine: "PostgreSQL".to_string(),
            agreements: vec![DiffPoint {
                aspect: DiffAspect::TableAccess,
                ra_value: "users".to_string(),
                db_value: "users".to_string(),
                agrees: true,
                confidence: 0.9,
                explanation: "match".to_string(),
            }],
            disagreements: vec![],
            confidence: 0.9,
            summary: "1 agreement".to_string(),
        };

        let output = format_diff_report(&report);
        assert!(output.contains("AGREE"));
        assert!(output.contains("Table Access"));
    }

    #[test]
    fn format_schema_summary_basic() {
        let schema = ra_metadata::SchemaInfo {
            database: "testdb".to_string(),
            tables: vec![ra_metadata::connector::TableInfo {
                schema: "public".to_string(),
                name: "users".to_string(),
                columns: vec![],
                constraints: vec![],
                indexes: vec![],
                estimated_rows: Some(1000),
            }],
            views: vec![],
        };

        let output = format_schema_summary(&schema);
        assert!(output.contains("testdb"));
        assert!(output.contains("users"));
        assert!(output.contains("~1000 rows"));
    }
}
