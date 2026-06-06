// Build script for ra-parser: compiles the Lime grammar into a C parser,
// then compiles and links the generated C code.
//
// Pipeline:
//   1. Build the `lime` tool from lime/lime.c (host tool)
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
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must be set"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("workspace root not found");

    let lime_root = workspace_root.join("crates/lime-sys/lime");
    let grammar_dir = manifest_dir.join("grammar");
    let grammar_file = grammar_dir.join("ra_sql.lime");

    // Skip lime compilation if the grammar file doesn't exist (allows
    // building without the grammar for crates that only use the Rust API).
    if !grammar_file.exists() {
        return;
    }

    // Step 1: Build the lime tool from source.
    let lime_tool = build_lime_tool(&lime_root, &out_dir);

    // Step 2: Emit the native-Rust parser (ra_sql.rs). The C parser is fully
    // retired; only the Rust target is generated.
    run_lime_rust(&lime_tool, &lime_root, &grammar_file, &out_dir);

    // Re-run triggers.
    println!("cargo:rerun-if-changed=grammar/ra_sql.lime");
    println!(
        "cargo:rerun-if-changed={}",
        lime_root.join("lime.c").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        lime_root.join("limpar.c").display()
    );
    // The host lime tool is compiled from lime.c + the src/ emitters; rebuild
    // it (and regenerate the parser) when any of that source changes, so an
    // in-place Lime source edit can't leave a stale tool/parser behind.
    println!(
        "cargo:rerun-if-changed={}",
        lime_root.join("src").display()
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

    // The lime host tool links several sources (mirrors lime/meson.build's
    // `lime` executable + the lex-compiler static library). The set grew in
    // Lime v0.10/v0.11 (skin emitters + lex compiler), so compile lime.c plus
    // the Rust/bison emitters, jit_inline, and the full lex compiler library.
    let mut sources: Vec<PathBuf> = vec![
        lime_src.clone(),
        lime_root.join("src/emit_rust.c"),
        lime_root.join("src/emit_c_skin_bison.c"),
        lime_root.join("src/jit_inline.c"),
    ];
    for f in [
        "lex_tokenize.c",
        "lex_ast.c",
        "lex_parse.c",
        "lex_resolve.c",
        "lex_pretty.c",
        "lex_main.c",
        "lex_regex.c",
        "lex_nfa.c",
        "lex_dfa.c",
        "lex_dfa_min.c",
        "lex_compile.c",
        "lex_emit.c",
        "emit_rust_lex.c",
        "emit_rust_skin_logos.c",
        "emit_c_skin_flex.c",
        "lex_introspect.c",
    ] {
        let p = lime_root.join("src/lex").join(f);
        if p.exists() {
            sources.push(p);
        }
    }
    let mut cmd = Command::new("cc");
    cmd.arg("-O2")
        .arg("-w")
        .arg("-DLIME_HAS_LEX_COMPILER")
        .arg("-DLIME_HAS_RUST_OUTPUT")
        .arg("-o")
        .arg(&lime_bin)
        .arg(format!("-I{}", lime_root.join("src").display()))
        .arg(format!("-I{}", lime_root.join("src/lex").display()))
        .arg(format!("-I{}", lime_root.join("include").display()));
    for s in &sources {
        cmd.arg(s);
    }
    let status = cmd
        .status()
        .expect("failed to invoke C compiler for lime tool");

    assert!(
        status.success(),
        "failed to compile lime tool (exit code: {status})"
    );

    lime_bin
}

/// Run lime with `--target=rust` to additionally emit `ra_sql.rs` next to the
/// C output in `OUT_DIR`. The Rust source is `include!`d by the crate's
/// `rust-parser` module; it is not compiled here. Conflict tolerance mirrors
/// `run_lime` (resolved shift/reduce conflicts are expected and benign).
fn run_lime_rust(lime_tool: &Path, lime_root: &Path, grammar_file: &Path, out_dir: &Path) {
    let template = lime_root.join("limpar.c");
    let work_grammar = out_dir.join("ra_sql.lime");
    // Always copy the current grammar into OUT_DIR. A conditional copy
    // ("only if missing") reuses a stale grammar from a previous build when
    // build.rs re-runs after a grammar edit, generating ra_sql.rs from the old
    // grammar (observed: unused/`_`-prefixed aliases that then fail to compile
    // against the new `%action_rust` bodies).
    std::fs::copy(grammar_file, &work_grammar)
        .unwrap_or_else(|e| panic!("failed to copy grammar to OUT_DIR: {e}"));

    let output = Command::new(lime_tool)
        .arg("--target=rust")
        .arg(format!("-T{}", template.display()))
        .arg("-q")
        .arg(&work_grammar)
        .output()
        .unwrap_or_else(|e| panic!("failed to run lime --target=rust: {e}"));

    let generated_ok = out_dir.join("ra_sql.rs").exists();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let only_conflicts = stderr
            .lines()
            .all(|l| l.trim().is_empty() || l.contains("parsing conflict"));
        assert!(
            only_conflicts && generated_ok,
            "lime --target=rust failed (exit {}):\nstderr: {stderr}",
            output.status
        );
    }
    assert!(
        generated_ok,
        "lime --target=rust did not produce ra_sql.rs in {}",
        out_dir.display()
    );

    // Strip the generated file's crate-level inner attributes (`#![...]`).
    // The file is `include!`d into our `generated` module, which already
    // supplies the needed `#![allow(...)]`; leaving the generated `#![...]`
    // in place is a hard error ("inner attribute not permitted") because the
    // include site is not the first token of the module.
    let rs_path = out_dir.join("ra_sql.rs");
    let contents = std::fs::read_to_string(&rs_path)
        .unwrap_or_else(|e| panic!("failed to read generated ra_sql.rs: {e}"));
    let stripped: String = contents
        .lines()
        .filter(|l| !l.trim_start().starts_with("#!["))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&rs_path, stripped)
        .unwrap_or_else(|e| panic!("failed to rewrite generated ra_sql.rs: {e}"));
}
