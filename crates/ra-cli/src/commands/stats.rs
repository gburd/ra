//! The `stats` subcommand.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use colored::Colorize;

use ra_parser::{parse_rule_file, validate_metadata_all};

use crate::helpers::collect_rra_files;
use crate::output::print_header;

pub fn cmd_stats(dir: &str, verbose: bool, quiet: bool) -> Result<()> {
    let rules_dir = Path::new(dir);
    if !rules_dir.is_dir() {
        bail!(
            "rules directory not found: {dir}\n\
             hint: pass --dir <path> or run from the repo root"
        );
    }

    let files = collect_rra_files(dir)?;

    if files.is_empty() {
        if !quiet {
            eprintln!("{}", "No .rra files found.".dimmed());
        }
        return Ok(());
    }

    let mut by_category: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut by_id: std::collections::HashMap<String, Vec<PathBuf>> =
        std::collections::HashMap::new();
    let mut parse_ok = 0u32;
    let mut parse_fail = 0u32;
    let mut valid_ok = 0u32;
    let mut valid_fail = 0u32;

    for file in &files {
        let Ok(source) = std::fs::read_to_string(file) else {
            parse_fail += 1;
            continue;
        };
        match parse_rule_file(&source) {
            Ok(rule) => {
                parse_ok += 1;
                let errs = validate_metadata_all(&rule.metadata);
                if errs.is_empty() {
                    valid_ok += 1;
                } else {
                    valid_fail += 1;
                }

                let cat_prefix = rule
                    .metadata
                    .category
                    .split('/')
                    .take(2)
                    .collect::<Vec<_>>()
                    .join("/");
                by_category
                    .entry(cat_prefix)
                    .or_default()
                    .push(rule.metadata.id.clone());

                by_id
                    .entry(rule.metadata.id)
                    .or_default()
                    .push(file.clone());
            }
            Err(_) => {
                parse_fail += 1;
            }
        }
    }

    let total = files.len();
    let duplicates: Vec<_> = by_id.iter().filter(|(_, v)| v.len() > 1).collect();

    if !quiet {
        print_header(&format!("Rule Collection Statistics ({total} files)"));

        eprintln!("  {}: {}", "Total .rra files".bold(), total);
        eprintln!(
            "  {}: {} ({} failed)",
            "Parsed successfully".bold(),
            parse_ok,
            parse_fail,
        );
        eprintln!(
            "  {}: {} ({} with issues)",
            "Validated".bold(),
            valid_ok,
            valid_fail,
        );
        eprintln!("  {}: {}", "Unique rule IDs".bold(), by_id.len());
        eprintln!("  {}: {}", "Duplicate IDs".bold(), duplicates.len());
        eprintln!("  {}: {}", "Categories".bold(), by_category.len());

        eprintln!();
        eprintln!("{}", "Rules by Category:".bold());
        for (cat, rules) in &by_category {
            eprintln!("  {:>4}  {}", rules.len().to_string().cyan(), cat,);
        }

        if !duplicates.is_empty() {
            eprintln!();
            eprintln!("{}", "Duplicate Rule IDs:".bold());
            for (id, paths) in &duplicates {
                eprintln!("  {} ({}x):", id.yellow(), paths.len());
                if verbose {
                    for p in *paths {
                        eprintln!("    - {}", p.display());
                    }
                }
            }
        }
    }

    Ok(())
}
