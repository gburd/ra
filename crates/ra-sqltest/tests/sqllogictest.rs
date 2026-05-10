//! SQL logic test runner for Ra.
//!
//! Discovers and runs all `.slt` files in the `tests/slt/` directory.

use ra_sqltest::RaDb;
use sqllogictest::Runner;
use std::path::Path;

fn main() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        run_all_tests().await;
    });
}

async fn run_all_tests() {
    let slt_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/slt");

    if !slt_dir.exists() {
        eprintln!("No slt directory found at {}", slt_dir.display());
        return;
    }

    let pattern = format!("{}/**/*.slt", slt_dir.display());
    let files: Vec<_> = glob::glob(&pattern)
        .expect("valid glob pattern")
        .filter_map(Result::ok)
        .collect();

    if files.is_empty() {
        eprintln!("No .slt files found in {}", slt_dir.display());
        return;
    }

    let mut passed = 0;
    let mut failed = 0;
    let mut errors: Vec<String> = Vec::new();

    for file in &files {
        let mut runner = Runner::new(|| async { Ok(RaDb::new()) });

        let relative = file
            .strip_prefix(&slt_dir)
            .unwrap_or(file.as_path());

        match runner.run_file_async(file).await {
            Ok(()) => {
                passed += 1;
                eprintln!("  PASS: {}", relative.display());
            }
            Err(e) => {
                failed += 1;
                let msg = format!("  FAIL: {} — {}", relative.display(), e);
                eprintln!("{msg}");
                errors.push(msg);
            }
        }
    }

    eprintln!("\n--- Results: {} passed, {} failed ---", passed, failed);

    if failed > 0 {
        eprintln!("\nFailures:");
        for err in &errors {
            eprintln!("{err}");
        }
        std::process::exit(1);
    }
}
