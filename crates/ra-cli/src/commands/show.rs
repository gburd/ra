//! The `show` subcommand.

use anyhow::{bail, Result};
use colored::Colorize;

use crate::helpers::{collect_rra_files, find_rule_by_id};

pub fn cmd_show(rule_id: &str, dir: &str) -> Result<()> {
    let files = collect_rra_files(dir)?;

    let Some((rule, path)) = find_rule_by_id(rule_id, &files) else {
        bail!(
            "rule '{rule_id}' not found in {dir}\n\
             hint: run 'ra-cli list' to see available rules"
        );
    };

    eprintln!("{}", format!("Rule: {}", rule.metadata.id).bold());
    eprintln!("  {}: {}", "Name".bold(), rule.metadata.name);
    eprintln!("  {}: {}", "Category".bold(), rule.metadata.category);
    eprintln!("  {}: {}", "Version".bold(), rule.metadata.version);
    eprintln!("  {}: {}", "File".bold(), path.display());

    if !rule.metadata.databases.is_empty() {
        eprintln!(
            "  {}: {}",
            "Databases".bold(),
            rule.metadata.databases.join(", ")
        );
    }
    if !rule.metadata.authors.is_empty() {
        eprintln!(
            "  {}: {}",
            "Authors".bold(),
            rule.metadata.authors.join(", ")
        );
    }
    if !rule.metadata.tags.is_empty() {
        eprintln!("  {}: {}", "Tags".bold(), rule.metadata.tags.join(", "));
    }
    if let Some(ref std) = rule.metadata.standard {
        eprintln!("  {}: {std}", "Standard".bold());
    }

    if !rule.description.is_empty() {
        eprintln!();
        eprintln!("{}", "Description:".bold());
        for line in rule.description.lines() {
            eprintln!("  {line}");
        }
    }

    if let Some(ref alg) = rule.algebra_notation {
        eprintln!();
        eprintln!("{}", "Relational Algebra:".bold());
        for line in alg.lines() {
            eprintln!("  {}", line.green());
        }
    }

    if let Some(ref impl_code) = rule.implementation {
        eprintln!();
        eprintln!("{}", "Implementation:".bold());
        for line in impl_code.lines() {
            eprintln!("  {line}");
        }
    }

    if !rule.test_cases.is_empty() {
        eprintln!();
        eprintln!(
            "{}",
            format!("Test Cases: {} block(s)", rule.test_cases.len()).bold()
        );
    }

    if !rule.references.is_empty() {
        eprintln!();
        eprintln!("{}", "References:".bold());
        for r in &rule.references {
            eprintln!("  - {r}");
        }
    }

    Ok(())
}
