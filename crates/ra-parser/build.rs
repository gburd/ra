// Build script for ra-parser: compiles the Lime grammar into a C parser,
// then compiles and links the generated C code.
//
// Pipeline:
//   1. Build the `lime` tool from _/lime/lime.c (host tool)
//   2. Run `lime -T limpar.c -d OUT_DIR grammar/ra_sql.lime`
//   3. Compile the generated ra_sql.c with cc
//
// Panics are idiomatic in build scripts for fatal errors.
#![allow(clippy::panic, clippy::expect_used)]

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect(
            "CARGO_MANIFEST_DIR must be set",
        ));
    let out_dir =
        PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must be set"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("workspace root not found");

    let lime_root = workspace_root.join("_/lime");
    let grammar_dir = manifest_dir.join("grammar");
    let grammar_file = grammar_dir.join("ra_sql.lime");

    // Skip lime compilation if the grammar file doesn't exist (allows
    // building without the grammar for crates that only use the Rust API).
    if !grammar_file.exists() {
        return;
    }

    // Step 1: Build the lime tool from source.
    let lime_tool = build_lime_tool(&lime_root, &out_dir);

    // Step 2: Run lime on the grammar to produce C source.
    let generated_c = run_lime(
        &lime_tool,
        &lime_root,
        &grammar_file,
        &out_dir,
    );

    // Step 3: Compile the generated C parser.
    compile_generated_parser(
        &generated_c,
        &grammar_dir,
        &lime_root,
        &out_dir,
    );

    // Re-run triggers.
    println!("cargo:rerun-if-changed=grammar/ra_sql.lime");
    println!("cargo:rerun-if-changed=grammar/ra_ffi.h");
    println!(
        "cargo:rerun-if-changed={}",
        lime_root.join("lime.c").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        lime_root.join("limpar.c").display()
    );
}

/// Compile lime.c into the `lime` host tool.
fn build_lime_tool(lime_root: &Path, out_dir: &Path) -> PathBuf {
    let lime_src = lime_root.join("lime.c");
    assert!(
        lime_src.exists(),
        "lime.c not found at {}",
        lime_src.display()
    );

    let lime_bin = out_dir.join("lime");

    let status = Command::new("cc")
        .arg("-O2")
        .arg("-o")
        .arg(&lime_bin)
        .arg(&lime_src)
        .status()
        .expect("failed to invoke C compiler for lime tool");

    assert!(
        status.success(),
        "failed to compile lime tool (exit code: {status})"
    );

    lime_bin
}

/// Run the lime tool on the grammar file to produce `ra_sql.c` and `ra_sql.h`.
fn run_lime(
    lime_tool: &Path,
    lime_root: &Path,
    grammar_file: &Path,
    out_dir: &Path,
) -> PathBuf {
    let template = lime_root.join("limpar.c");
    assert!(
        template.exists(),
        "limpar.c template not found at {}",
        template.display()
    );

    // Copy grammar to OUT_DIR so lime generates output there.
    let work_grammar = out_dir.join("ra_sql.lime");
    std::fs::copy(grammar_file, &work_grammar).unwrap_or_else(|e| {
        panic!("failed to copy grammar to OUT_DIR: {e}")
    });

    let output = Command::new(lime_tool)
        .arg(format!("-T{}", template.display()))
        .arg("-q")
        .arg(&work_grammar)
        .output()
        .unwrap_or_else(|e| {
            panic!("failed to run lime tool: {e}")
        });

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        panic!(
            "lime tool failed (exit {}):\nstdout: {stdout}\nstderr: {stderr}",
            output.status
        );
    }

    let generated = out_dir.join("ra_sql.c");
    assert!(
        generated.exists(),
        "lime did not produce ra_sql.c in {}",
        out_dir.display()
    );

    generated
}

/// Compile the generated C parser and link it into the crate.
fn compile_generated_parser(
    generated_c: &Path,
    grammar_dir: &Path,
    lime_root: &Path,
    out_dir: &Path,
) {
    cc::Build::new()
        .file(generated_c)
        .include(grammar_dir)
        .include(lime_root.join("include"))
        .include(out_dir)
        .std("c11")
        .warnings(true)
        .extra_warnings(true)
        .compile("ra_sql_parser");
}
