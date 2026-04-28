// Build script: panics are the idiomatic way to abort on fatal errors.
#![allow(clippy::panic)]

use std::env;
use std::path::PathBuf;

fn compile_c_sources(lime_src: &std::path::Path, lime_inc: &std::path::Path) {
    // --- Compile SIMD tokenizer as a separate unit ----------------------
    // The AVX2 path uses __attribute__((target("avx2"))), so no special
    // flags are needed — it compiles on any x86_64 toolchain. On ARM the
    // NEON path is selected automatically by the compiler.
    cc::Build::new()
        .file(lime_src.join("tokenize_simd.c"))
        .include(lime_inc)
        .define("_GNU_SOURCE", None)
        .std("c11")
        .warnings(false) // upstream C code, not our warnings to fix
        .compile("tokenize_simd");

    // --- Compile JIT stubs (no LLVM dependency) -------------------------
    cc::Build::new()
        .files([
            lime_src.join("jit_context.c"),
            lime_src.join("jit_codegen.c"),
            lime_src.join("jit_policy.c"),
            lime_src.join("jit_tokenizer.c"),
        ])
        .include(lime_inc)
        .define("_GNU_SOURCE", None)
        .define("LIME_NO_JIT", None)
        .std("c11")
        .warnings(false)
        .compile("lime_jit");

    // --- Compile core library -------------------------------------------
    let core_sources = [
        "version.c",
        "snapshot.c",
        "token_table.c",
        "tokenize.c",
        "parse_context.c",
        "extension.c",
        "conflict.c",
        "snapshot_modify.c",
        "dependency_resolver.c",
        "parser_manager.c",
        "parser_plugin.c",
        "parser_operations.c",
        "merkle_tree.c",
        "parser_composition.c",
        "utf8.c",
        "lime_ast.c",
        "glr.c",
    ];

    cc::Build::new()
        .files(core_sources.iter().map(|f| lime_src.join(f)))
        .include(lime_inc)
        .include(lime_src)
        .define("_GNU_SOURCE", None)
        .std("c11")
        .warnings(false)
        .compile("lime_parser");

    // Link pthreads (required by token_table.c rwlock)
    println!("cargo:rustc-link-lib=pthread");

    // Re-run if Lime sources change
    println!("cargo:rerun-if-env-changed=LIME_DIR");
    println!("cargo:rerun-if-changed={}", lime_src.display());
    println!("cargo:rerun-if-changed={}", lime_inc.display());
}

fn generate_bindings(lime_inc: &std::path::Path, lime_src: &std::path::Path) {
    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", lime_inc.display()))
        .clang_arg(format!("-I{}", lime_src.display()))
        .clang_arg("-D_GNU_SOURCE")
        // Types we want fully generated
        .allowlist_type("ParserSnapshot")
        .allowlist_type("ParseContext")
        .allowlist_type("Token")
        .allowlist_type("TokenTable")
        .allowlist_type("TokenDefinition")
        .allowlist_type("Tokenizer")
        .allowlist_type("LimeArena")
        .allowlist_type("LimeError")
        .allowlist_type("LimeLocation")
        .allowlist_type("SemVer")
        .allowlist_type("VersionOp")
        .allowlist_type("VersionConstraint")
        .allowlist_type("ParserModule")
        .allowlist_type("ParserDependency")
        // Functions we want bound
        .allowlist_function("lemon_snapshot_create")
        .allowlist_function("lemon_snapshot_acquire")
        .allowlist_function("lemon_snapshot_release")
        .allowlist_function("lemon_parser_version")
        .allowlist_function("snapshot_acquire")
        .allowlist_function("snapshot_release")
        .allowlist_function("create_base_snapshot")
        .allowlist_function("parse_begin")
        .allowlist_function("parse_end")
        .allowlist_function("parse_token")
        .allowlist_function("parse_get_snapshot")
        .allowlist_function("parse_context_create")
        .allowlist_function("parse_context_destroy")
        .allowlist_function("snap_find_shift_action")
        .allowlist_function("snap_find_reduce_action")
        .allowlist_function("tokenizer_create")
        .allowlist_function("tokenizer_destroy")
        .allowlist_function("tokenizer_next")
        .allowlist_function("tokenizer_peek")
        .allowlist_function("tokenizer_position")
        .allowlist_function("tokenizer_line")
        .allowlist_function("tokenizer_column")
        .allowlist_function("create_token_table")
        .allowlist_function("destroy_token_table")
        .allowlist_function("lookup_token")
        .allowlist_function("add_token")
        .allowlist_function("remove_tokens_by_extension")
        .allowlist_function("lime_arena_create")
        .allowlist_function("lime_arena_alloc")
        .allowlist_function("lime_arena_calloc")
        .allowlist_function("lime_arena_strdup")
        .allowlist_function("lime_arena_destroy")
        .allowlist_function("lime_arena_total_allocated")
        .allowlist_function("lime_arena_total_used")
        .allowlist_function("lime_error_free")
        .allowlist_function("lime_jit_available")
        .allowlist_function("lime_jit_compile")
        .allowlist_function("lemon_extension_registry_init")
        .allowlist_function("lemon_extension_registry_destroy")
        .allowlist_function("semver_parse")
        .allowlist_function("semver_compare")
        .allowlist_function("semver_satisfies")
        .allowlist_function("semver_destroy")
        .allowlist_function("parser_module_destroy_contents")
        .allowlist_function("parser_dependency_destroy_contents")
        // Token type constants
        .allowlist_var("TK_.*")
        .allowlist_var("INVALID_INDEX")
        // Use core types for no-std compat
        .use_core()
        // Disable layout tests: they generate #[allow] attributes that
        // conflict with the workspace deny(allow_attributes) lint.
        .layout_tests(false)
        .generate()
        .unwrap_or_else(|e| panic!("bindgen failed: {e}"));

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap_or_default());
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .unwrap_or_else(|e| panic!("failed to write bindings: {e}"));
}

fn main() {
    let lime_root =
        PathBuf::from(env::var("LIME_DIR").unwrap_or_else(|_| "../../_/lime".to_string()));
    let lime_src = lime_root.join("src");
    let lime_inc = lime_root.join("include");

    compile_c_sources(&lime_src, &lime_inc);
    generate_bindings(&lime_inc, &lime_src);
}
