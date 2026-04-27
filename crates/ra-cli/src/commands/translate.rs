//! The `translate` subcommand.

use anyhow::{Context, Result};
use colored::Colorize;

use crate::helpers::parse_dialect;
use crate::output::print_header;

pub fn cmd_translate(query: &str, from: &str, to: &str, quiet: bool) -> Result<()> {
    let source_dialect = parse_dialect(from)?;
    let target_dialect = parse_dialect(to)?;

    let translator = ra_dialect::DialectTranslator::new(source_dialect, target_dialect);

    let result = translator
        .translate(query)
        .with_context(|| format!("translating SQL from {from} to {to}: {query}"))?;

    if !quiet {
        print_header(&format!(
            "SQL Translation: {} -> {}",
            source_dialect, target_dialect
        ));
        eprintln!("  {}: {query}", "Input".bold());
        eprintln!();
        eprintln!("{}", "Translated:".bold());
        eprintln!("  {}", result.sql);

        if !result.warnings.is_empty() {
            eprintln!();
            eprintln!("{}", "Warnings:".bold());
            for w in &result.warnings {
                eprintln!("  {} {}", format!("[{}]", w.severity).yellow(), w.message);
                if let Some(ref hint) = w.hint {
                    eprintln!("    {}: {hint}", "hint".dimmed());
                }
            }
        }
    }

    Ok(())
}
