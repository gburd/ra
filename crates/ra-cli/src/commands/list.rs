//! The `list` subcommand.

use std::path::Path;

use anyhow::{bail, Result};
use colored::Colorize;

use ra_parser::parse_metadata;

use crate::helpers::collect_rra_files;
use crate::output::print_header;

pub fn cmd_list(
    dir: &str,
    category_filter: Option<&str>,
    tag_filter: Option<&str>,
    quiet: bool,
) -> Result<()> {
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

    let mut entries: Vec<(String, String, String, std::path::PathBuf)> = Vec::new();

    for file in &files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        let Ok(meta) = parse_metadata(&source) else {
            continue;
        };

        if let Some(cat) = category_filter {
            if !meta.category.starts_with(cat) {
                continue;
            }
        }

        if let Some(tag) = tag_filter {
            if !meta.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
                continue;
            }
        }

        entries.push((meta.id, meta.name, meta.category, file.clone()));
    }

    entries.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)));

    if !quiet {
        let mut header = format!("{} rule(s) found", entries.len());
        if let Some(cat) = category_filter {
            header.push_str(&format!(" in category '{cat}'"));
        }
        if let Some(tag) = tag_filter {
            header.push_str(&format!(" with tag '{tag}'"));
        }
        print_header(&header);
    }

    let id_w = entries.iter().map(|e| e.0.len()).max().unwrap_or(2).max(2);
    let name_w = entries.iter().map(|e| e.1.len()).max().unwrap_or(4).max(4);
    let cat_w = entries.iter().map(|e| e.2.len()).max().unwrap_or(8).max(8);

    eprintln!(
        "  {:<id_w$}  {:<name_w$}  {:<cat_w$}",
        "ID".bold(),
        "NAME".bold(),
        "CATEGORY".bold(),
    );
    eprintln!(
        "  {:<id_w$}  {:<name_w$}  {:<cat_w$}",
        "-".repeat(id_w),
        "-".repeat(name_w),
        "-".repeat(cat_w),
    );

    for (id, name, category, _path) in &entries {
        eprintln!(
            "  {:<id_w$}  {:<name_w$}  {:<cat_w$}",
            id.cyan(),
            name,
            category.dimmed(),
        );
    }

    Ok(())
}
