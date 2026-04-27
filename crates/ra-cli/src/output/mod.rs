//! Output formatting utilities for ra-cli.

pub mod errors;
pub mod plans;

use colored::Colorize;

use ra_parser::ParseError;
use std::path::Path;

pub fn print_header(msg: &str) {
    eprintln!();
    eprintln!("{}", msg.bold());
    eprintln!();
}

pub fn print_status(label: &str, detail: &str, ok: bool) {
    if ok {
        eprintln!("  {} {detail}", format!("[{label}]").green().bold(),);
    } else {
        eprintln!("  {} {detail}", format!("[{label}]").red().bold(),);
    }
}

pub fn print_detail(msg: &str) {
    eprintln!("        {}", msg.yellow());
}

pub fn print_parse_error(err: &ParseError, path: &Path) {
    match err {
        ParseError::MissingFrontmatter => {
            print_detail(&format!(
                "{}: missing YAML frontmatter (---)",
                path.display()
            ));
        }
        ParseError::InvalidYaml { line, source } => {
            print_detail(&format!("{}:{line}: {source}", path.display()));
        }
        ParseError::Validation(v) => {
            print_detail(&format!("{}: {v}", path.display()));
        }
        ParseError::Other(msg) => {
            print_detail(&format!("{}: {msg}", path.display()));
        }
    }
}

pub fn print_summary(pass: u32, fail: u32) {
    eprintln!();
    let total = pass + fail;
    if fail == 0 {
        eprintln!(
            "{}",
            format!("All {total} file(s) passed validation.")
                .green()
                .bold()
        );
    } else {
        eprintln!(
            "{}: {pass} passed, {fail} failed out of {total}",
            "Summary".bold(),
        );
    }
}
