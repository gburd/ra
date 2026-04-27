//! The `validate` subcommand.

use anyhow::{bail, Context, Result};

use ra_parser::{parse_rule_file, validate_metadata_all};

use crate::helpers::collect_rra_files;
use crate::output::{print_detail, print_header, print_parse_error, print_status, print_summary};

pub fn cmd_validate(path: &str, verbose: bool, quiet: bool) -> Result<()> {
    let files = collect_rra_files(path)?;

    if files.is_empty() {
        bail!("no .rra files found in {path}");
    }

    if !quiet {
        print_header(&format!("Validating {} file(s)", files.len()));
    }

    let mut pass = 0u32;
    let mut fail = 0u32;

    for file in &files {
        let source =
            std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;

        match parse_rule_file(&source) {
            Ok(rule) => {
                let extra_errors = validate_metadata_all(&rule.metadata);
                if extra_errors.is_empty() {
                    pass += 1;
                    if verbose {
                        print_status("PASS", &file.display().to_string(), true);
                    }
                } else {
                    fail += 1;
                    print_status("FAIL", &file.display().to_string(), false);
                    for err in &extra_errors {
                        print_detail(&format!("  {err}"));
                    }
                }
            }
            Err(e) => {
                fail += 1;
                print_status("FAIL", &file.display().to_string(), false);
                print_parse_error(&e, file);
            }
        }
    }

    if !quiet {
        print_summary(pass, fail);
    }

    if fail > 0 {
        bail!("{fail} file(s) failed validation");
    }

    Ok(())
}
