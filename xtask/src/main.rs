//! Build automation tasks for the RA project.
//!
//! Run with: `cargo xtask <command>`
//!
//! Available commands:
//! - `docs` - Build and serve documentation locally

use std::env;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn main() {
    let task = env::args().nth(1);
    match task.as_deref() {
        Some("docs") => {
            let serve = env::args().any(|arg| arg == "--serve" || arg == "-s");
            if serve {
                docs_serve();
            } else {
                docs_build();
            }
        }
        Some("help") | Some("--help") | Some("-h") | None => {
            print_help();
        }
        Some(task) => {
            eprintln!("Unknown task: {}", task);
            eprintln!();
            print_help();
            std::process::exit(1);
        }
    }
}

fn print_help() {
    println!("RA Build Tasks");
    println!();
    println!("USAGE:");
    println!("    cargo xtask <task> [options]");
    println!();
    println!("TASKS:");
    println!("    docs              Build documentation (VitePress)");
    println!("    docs --serve      Build and serve documentation locally");
    println!("    help              Show this help message");
    println!();
    println!("EXAMPLES:");
    println!("    cargo xtask docs --serve    # Serve docs at http://localhost:5173");
    println!("    cargo docs                  # Alias for 'cargo xtask docs --serve'");
}

fn docs_build() {
    println!("📚 Building documentation...");

    let docs_dir = project_root().join("docs");

    // Check if npm is installed
    if !check_command("npm") {
        eprintln!("❌ Error: npm is not installed");
        eprintln!("   Install Node.js from https://nodejs.org/");
        std::process::exit(1);
    }

    // Install dependencies if node_modules doesn't exist
    if !docs_dir.join("node_modules").exists() {
        println!("📦 Installing npm dependencies...");
        run_command("npm", &["install"], &docs_dir);
    }

    // Build docs
    println!("🔨 Building VitePress site...");
    run_command("npm", &["run", "build:docs"], &docs_dir);

    println!("✅ Documentation built successfully!");
    println!("   Output: docs/.vitepress/dist/");
    println!();
    println!("To serve locally, run:");
    println!("   cargo xtask docs --serve");
}

fn docs_serve() {
    println!("📚 Building and serving documentation...");

    let docs_dir = project_root().join("docs");

    // Check if npm is installed
    if !check_command("npm") {
        eprintln!("❌ Error: npm is not installed");
        eprintln!("   Install Node.js from https://nodejs.org/");
        std::process::exit(1);
    }

    // Install dependencies if node_modules doesn't exist
    if !docs_dir.join("node_modules").exists() {
        println!("📦 Installing npm dependencies...");
        run_command("npm", &["install"], &docs_dir);
    }

    // Run VitePress dev server
    println!("🚀 Starting VitePress dev server...");
    println!();
    println!("Documentation will be available at:");
    println!("   http://localhost:5173");
    println!();
    println!("Press Ctrl+C to stop the server");
    println!();

    let status = Command::new("npm")
        .args(&["run", "dev"])
        .current_dir(&docs_dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    match status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            eprintln!("❌ npm dev server exited with status: {}", status);
            std::process::exit(status.code().unwrap_or(1));
        }
        Err(e) => {
            eprintln!("❌ Failed to run npm: {}", e);
            std::process::exit(1);
        }
    }
}

fn check_command(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn run_command(cmd: &str, args: &[&str], cwd: &PathBuf) {
    let status = Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    match status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            eprintln!("❌ Command failed with status: {}", status);
            std::process::exit(status.code().unwrap_or(1));
        }
        Err(e) => {
            eprintln!("❌ Failed to run {}: {}", cmd, e);
            std::process::exit(1);
        }
    }
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("Failed to find project root")
        .to_path_buf()
}
