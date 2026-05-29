//! The `gather-metadata` subcommand.

use anyhow::{bail, Context, Result};
use colored::Colorize;

use crate::output::print_header;

pub fn cmd_gather_metadata(
    db_url: Option<&str>,
    schema_path: Option<&str>,
    output_path: &str,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    let schema = if let Some(url) = db_url {
        if !quiet {
            let kind = ra_metadata::detect_kind(url)
                .map_or_else(|_| "unknown".to_owned(), |k| k.to_string());
            eprintln!("Connecting to {} database...", kind.cyan());
        }
        let mut connector =
            ra_metadata::connect(url).with_context(|| format!("connecting to database: {}", ra_metadata::redact_url(url)))?;
        connector
            .gather_schema()
            .with_context(|| format!("gathering schema from: {}", ra_metadata::redact_url(url)))?
    } else if let Some(path) = schema_path {
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("reading schema file: {path}"))?;
        serde_json::from_str(&source)
            .with_context(|| format!("parsing schema JSON from: {path}"))?
    } else {
        bail!("either --db <url> or --schema <path> is required");
    };

    if !quiet {
        print_header("Database Metadata");
        eprintln!("  {}: {}", "Database".bold(), schema.kind);
        eprintln!("  {}: {}", "Schema".bold(), schema.schema_name);
        eprintln!("  {}: {}", "Tables".bold(), schema.table_count());
    }

    if verbose {
        for (name, table) in &schema.tables {
            eprintln!(
                "    {}: {} columns, {} indexes",
                name.cyan(),
                table.column_count(),
                table.indexes.len(),
            );
        }
    }

    let json = serde_json::to_string_pretty(&schema).context("serializing schema to JSON")?;
    std::fs::write(output_path, json)
        .with_context(|| format!("writing output to: {output_path}"))?;

    if !quiet {
        eprintln!();
        eprintln!(
            "{}",
            format!("Wrote metadata to {output_path}").green().bold()
        );
    }

    Ok(())
}
