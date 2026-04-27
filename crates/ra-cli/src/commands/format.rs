//! The `format` subcommand.

use std::io::Read;

use anyhow::{bail, Context, Result};

pub fn cmd_format(
    query: Option<&str>,
    stdin: bool,
    capitalize: &str,
    indent: &str,
    quiet: bool,
) -> Result<()> {
    let sql = if stdin || query.is_none() {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading SQL from stdin")?;
        buf
    } else {
        query.unwrap_or_default().to_owned()
    };

    let cap_mode = match capitalize.to_lowercase().as_str() {
        "keywords" | "kw" => ra_parser::CapitalizeMode::Keywords,
        "all" => ra_parser::CapitalizeMode::All,
        "none" => ra_parser::CapitalizeMode::None,
        other => bail!(
            "unknown capitalize mode: '{other}'. \
             Valid: keywords, all, none"
        ),
    };

    let indent_style = match indent.to_lowercase().as_str() {
        "spaces2" | "2" => ra_parser::IndentStyle::Spaces(2),
        "spaces4" | "4" => ra_parser::IndentStyle::Spaces(4),
        "tab" | "tabs" => ra_parser::IndentStyle::Tab,
        other => bail!(
            "unknown indent style: '{other}'. \
             Valid: spaces2, spaces4, tab"
        ),
    };

    let config = ra_parser::FormatConfig {
        capitalize: cap_mode,
        indent: indent_style,
        ..ra_parser::FormatConfig::default()
    };

    let formatter = ra_parser::SqlFormatter::new(config);
    let formatted = formatter
        .format(&sql)
        .with_context(|| format!("formatting SQL: {sql}"))?;

    if !quiet {
        eprintln!("{formatted}");
    }

    Ok(())
}
