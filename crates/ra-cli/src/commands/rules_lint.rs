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

use ra_core::proof_obligation::{required_obligations, ObligationKind};
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

/// The `rules lint --check-obligations` mode (RA-STEERING §5.4, Codeberg #13).
///
/// For each `.rra` rule, look up the obligation kinds its category (and, for the
/// outer-join case, its id) *requires* via [`required_obligations`], then check
/// the rule's declared `proof_obligations:` cover each required kind. A rule is
/// "missing" when a required kind has no declared obligation of that kind (an
/// explicit `none` opt-out does *not* satisfy a required kind).
///
/// This is a **ratchet**, mirroring the malformed-rule ratchet above: with
/// `--obligations-baseline N` it fails only when the missing count *exceeds* N,
/// so the current backlog stays visible without turning CI red, and new rules
/// that skip a required obligation are blocked. The eventual hard flip ("a rule
/// that does not declare its obligations does not load") is the follow-on once
/// the backlog is annotated.
pub fn cmd_rules_obligations(path: &str, baseline: usize, verbose: bool) -> Result<()> {
    let files = collect_rra_files(path)?;
    if files.is_empty() {
        bail!("no .rra files found in {path}");
    }

    print_header(&format!(
        "Checking proof obligations in {} rule file(s)",
        files.len()
    ));

    let mut pass = 0u32;
    let mut fail = 0u32;
    // (file, id, missing-kind) rows, plus a per-kind tally for the summary.
    let mut missing: Vec<(String, String, ObligationKind)> = Vec::new();
    let mut by_kind: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();

    for file in &files {
        let source =
            std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;

        // Malformed frontmatter can't be checked; skip (the malformed-rule
        // ratchet already covers structurally-broken rules).
        let Ok(rule) = parse_rule_file(&source) else {
            continue;
        };
        let meta = &rule.metadata;

        let required = required_obligations(&meta.category, &meta.id);
        if required.is_empty() {
            pass += 1;
            if verbose {
                print_status(
                    "PASS",
                    &format!("{} (no required obligations)", meta.id),
                    true,
                );
            }
            continue;
        }

        let mut file_missing: Vec<ObligationKind> = Vec::new();
        for req in &required {
            let covered = meta.proof_obligations.iter().any(|o| req.satisfied_by(o));
            if !covered {
                file_missing.push(*req);
            }
        }

        if file_missing.is_empty() {
            pass += 1;
            if verbose {
                print_status("PASS", &meta.id, true);
            }
        } else {
            fail += 1;
            print_status("FAIL", &meta.id, false);
            for kind in &file_missing {
                if verbose {
                    println!("    missing required obligation: {}", kind.as_str());
                }
                *by_kind.entry(kind.as_str()).or_insert(0) += 1;
                missing.push((file.display().to_string(), meta.id.clone(), *kind));
            }
        }
    }

    print_summary(pass, fail);

    let count = missing.len();
    if count > 0 {
        println!("\nMissing required obligations by kind:");
        for (kind, n) in &by_kind {
            println!("  {kind}: {n}");
        }
    }

    if count > baseline {
        bail!(
            "{count} rule(s) missing a required proof obligation, exceeding the \
             baseline of {baseline}. RA-STEERING §5.4 requires each rule to \
             declare (in machine-checkable `proof_obligations:`) why its rewrite \
             is sound. Add the required obligation, or (only if intentionally \
             shelving) raise --obligations-baseline."
        );
    }
    if count > 0 {
        println!(
            "\n{count} rule(s) missing a required obligation at or under the \
             baseline of {baseline} (tracked §5.4 backlog). Run with --verbose \
             to list them."
        );
    }

    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code uses unwrap for assertions")]
mod tests {
    use super::*;

    fn write_rra(dir: &std::path::Path, name: &str, frontmatter: &str) {
        let body = format!("---\n{frontmatter}---\n\n# {name}\n\nbody\n");
        std::fs::write(dir.join(name), body).unwrap();
    }

    // A join-elimination rule requires uniqueness_fd. One rule declares it, one
    // omits it: the linter must count exactly one missing, pass under baseline
    // 1, and fail at baseline 0.
    #[test]
    fn obligations_linter_counts_and_ratchets() {
        let dir = tempfile::tempdir().unwrap();
        write_rra(
            dir.path(),
            "declared.rra",
            "id: declared\nname: Declared\ncategory: logical/join-elimination\n\
             proof_obligations:\n  - type: uniqueness_fd\n    keys: pk\n",
        );
        write_rra(
            dir.path(),
            "missing.rra",
            "id: missing\nname: Missing\ncategory: logical/join-elimination\n",
        );
        let path = dir.path().to_str().unwrap();

        // One rule is missing its required obligation.
        assert!(
            cmd_rules_obligations(path, 0, false).is_err(),
            "baseline 0 must fail with 1 missing obligation"
        );
        // At/under the baseline the ratchet passes (backlog visible, CI green).
        assert!(
            cmd_rules_obligations(path, 1, false).is_ok(),
            "baseline 1 must pass with 1 missing obligation"
        );
    }

    // A rule whose category requires nothing passes even with no obligations.
    #[test]
    fn obligations_linter_passes_when_none_required() {
        let dir = tempfile::tempdir().unwrap();
        write_rra(
            dir.path(),
            "plain.rra",
            "id: plain\nname: Plain\ncategory: logical/projection-pushdown\n",
        );
        let path = dir.path().to_str().unwrap();
        assert!(cmd_rules_obligations(path, 0, false).is_ok());
    }
}
