//! The `parse` subcommand.
//!
//! `ra parse <sql>` prints Ra's RelExpr tree. With `--compare-pg` it runs the
//! PostgreSQL parse oracle (RA-STEERING §5.2): parse the SQL with PostgreSQL's
//! own parser (via libpg_query), extract comparable parse facts from both PG
//! and Ra's RelExpr, and report any divergence. Divergences exit non-zero so
//! this doubles as a corpus checker.
//!
//! `--tokens` is intentionally omitted: the Lime tokenizer only exposes an
//! FFI-oriented `LexToken` (raw `*const c_char` + `CString` backing), which is
//! not cleanly printable without decoding token codes to names. Add it when a
//! stable token-name accessor exists on the tokenizer.
#![expect(clippy::print_stdout, reason = "CLI output")]

use std::path::Path;

use anyhow::{bail, Result};

use ra_parser::sql_to_relexpr;

use crate::display::format_plan_tree;
use crate::output::errors::format_sql_error;

#[cfg(feature = "pg-oracle")]
#[path = "parse/corpus.rs"]
mod corpus;

/// Output format for `ra parse`.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum ParseFormat {
    /// Human-readable text.
    Text,
    /// JSON.
    Json,
}

pub fn cmd_parse(
    query: &str,
    compare_pg: bool,
    corpus: Option<&Path>,
    sql_file: Option<&Path>,
    format: ParseFormat,
) -> Result<()> {
    // Batch/corpus mode: `--corpus <dir>` and/or `--sql-file <f>`. Requires
    // --compare-pg (the oracle is the whole point of the corpus run).
    if corpus.is_some() || sql_file.is_some() {
        if !compare_pg {
            bail!("--corpus/--sql-file require --compare-pg");
        }
        return run_corpus(corpus, sql_file, format);
    }

    if compare_pg {
        return cmd_parse_compare(query, format);
    }

    // Default: print Ra's RelExpr tree (reuse the explain renderer).
    let plan = sql_to_relexpr(query).map_err(|e| format_sql_error(&e, query))?;
    match format {
        ParseFormat::Text => {
            print!("{}", format_plan_tree(&plan));
        }
        ParseFormat::Json => {
            let json = serde_json::to_string_pretty(&plan)?;
            println!("{json}");
        }
    }
    Ok(())
}

#[cfg(feature = "pg-oracle")]
fn cmd_parse_compare(query: &str, format: ParseFormat) -> Result<()> {
    let cmp = ra_oracle::compare(query)?;

    match format {
        ParseFormat::Json => {
            let json = serde_json::to_string_pretty(&cmp)?;
            println!("{json}");
        }
        ParseFormat::Text => {
            println!("SQL: {}", cmp.sql);
            println!();
            println!("PostgreSQL parse facts:");
            print_facts(&cmp.pg);
            println!();
            println!("Ra parse facts:");
            print_facts(&cmp.ra);
            println!();
            if cmp.is_equivalent() {
                if let Some(note) = &cmp.both_rejected {
                    println!("parse-equivalent: {note}");
                } else {
                    println!("parse-equivalent: no divergences");
                }
            } else {
                println!("DIVERGENCES ({}):", cmp.divergences.len());
                for d in &cmp.divergences {
                    println!("  - {d}");
                }
            }
        }
    }

    if cmp.is_equivalent() {
        Ok(())
    } else {
        // Non-zero exit: usable as a corpus checker. The router/main maps a
        // returned Err to exit code 1.
        bail!("{} parse divergence(s) found", cmp.divergences.len());
    }
}

#[cfg(feature = "pg-oracle")]
fn print_facts(f: &ra_oracle::ParseFacts) {
    let tables: Vec<&str> = f.tables.iter().map(String::as_str).collect();
    println!("  tables:       {{{}}}", tables.join(","));
    println!("  output_arity: {:?}", f.output_arity);
    println!("  join_count:   {}", f.join_count);
    println!("  has_where:    {}", f.has_where);
    println!("  has_group_by: {}", f.has_group_by);
    println!("  has_having:   {}", f.has_having);
    println!("  has_order_by: {}", f.has_order_by);
    println!("  has_limit:    {}", f.has_limit);
    println!("  has_distinct: {}", f.has_distinct);
}

#[cfg(not(feature = "pg-oracle"))]
fn cmd_parse_compare(_query: &str, _format: ParseFormat) -> Result<()> {
    bail!(
        "--compare-pg requires the PostgreSQL parse oracle, which is not \
         compiled in.\nrebuild with --features pg-oracle, e.g.\n  \
         cargo run -p ra-cli --features pg-oracle --bin ra -- parse <sql> --compare-pg"
    );
}

#[cfg(feature = "pg-oracle")]
fn run_corpus(corpus: Option<&Path>, sql_file: Option<&Path>, format: ParseFormat) -> Result<()> {
    corpus::run(corpus, sql_file, format)
}

#[cfg(not(feature = "pg-oracle"))]
fn run_corpus(
    _corpus: Option<&Path>,
    _sql_file: Option<&Path>,
    _format: ParseFormat,
) -> Result<()> {
    bail!(
        "--corpus/--sql-file with --compare-pg requires the PostgreSQL parse \
         oracle, which is not compiled in.\nrebuild with --features pg-oracle, e.g.\n  \
         cargo run -p ra-cli --features pg-oracle --bin ra -- parse --compare-pg --corpus <dir>"
    );
}
