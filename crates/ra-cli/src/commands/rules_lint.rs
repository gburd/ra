//! The `rules lint` subcommand.
//!
//! Walks `.rra` files and checks each rewrite pattern for the three
//! pathologies that cause a rule to be silently dropped at build time
//! (empty LHS/RHS, no-op LHS==RHS, unbound RHS metavariable). Uses the same
//! [`ra_engine::rule_lint`] logic the build script runs, so what the linter
//! flags is exactly what the build would drop.
//!
//! CI ratchet: the current corpus has a known backlog of malformed rules
//! (tracked). `--baseline N` lets CI fail only when the count *exceeds* N, so
//! new malformed rules are blocked and the existing backlog stays visible
//! without turning CI permanently red. Passing `--baseline 0` (the default)
//! makes any malformed rule an error.
#![expect(clippy::print_stdout, reason = "CLI output")]

use anyhow::{bail, Context, Result};

use ra_engine::rule_lint::{extract_rewrite_invocations, rewrite_pair_pathology};
use ra_parser::parse_rule_file;

use crate::helpers::collect_rra_files;
use crate::output::{print_header, print_status, print_summary};

pub fn cmd_rules_lint(path: &str, baseline: usize, verbose: bool) -> Result<()> {
    let files = collect_rra_files(path)?;
    if files.is_empty() {
        bail!("no .rra files found in {path}");
    }

    print_header(&format!(
        "Linting rewrite patterns in {} file(s)",
        files.len()
    ));

    let mut pass = 0u32;
    let mut fail = 0u32;
    let mut malformed: Vec<(String, String)> = Vec::new();

    for file in &files {
        let source =
            std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;

        let id = parse_rule_file(&source).map_or_else(
            |_| {
                file.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            },
            |r| r.metadata.id,
        );

        let body = source.splitn(3, "---").nth(2).unwrap_or(&source);

        let invocations = extract_rewrite_invocations(body, &id);
        let mut file_reasons: Vec<String> = Vec::new();
        for inv in &invocations {
            if let Some(reason) = rewrite_pair_pathology(inv) {
                file_reasons.push(reason);
            }
        }

        if file_reasons.is_empty() {
            pass += 1;
            if verbose {
                print_status("PASS", &file.display().to_string(), true);
            }
        } else {
            fail += 1;
            print_status("FAIL", &file.display().to_string(), false);
            for reason in &file_reasons {
                if verbose {
                    println!("    {reason}");
                }
                malformed.push((file.display().to_string(), reason.clone()));
            }
        }
    }

    print_summary(pass, fail);

    let count = malformed.len();
    if count > baseline {
        bail!(
            "{count} malformed rewrite rule(s) found, exceeding the baseline of \
             {baseline}. These rules have an empty/no-op pattern or an unbound RHS \
             metavariable and are silently dropped at build time. Fix the new \
             rule(s), or (only if intentionally shelving) raise --baseline."
        );
    }
    if count > 0 {
        println!(
            "\n{count} known malformed rule(s) at or under the baseline of \
             {baseline} (tracked backlog). Run with --verbose to list them."
        );
    }

    Ok(())
}
