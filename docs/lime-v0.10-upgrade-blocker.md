# Lime v0.10.0 upgrade blocker — spurious `--enable=safe` warning breaks C-target builds

**To:** Lime maintainers (codeberg.org/gregburd/lime)
**From:** Ra (PostgreSQL planner replacement) — `crates/ra-parser`
**Severity:** Blocks upgrade from v0.8.7 → v0.10.0
**Affected versions:** v0.9.3 through v0.10.0 (the `--enable`/`--target` feature-flag scheme)
**Target in use:** C skin (via `-T limpar.c`), i.e. *not* `--target=rust`

## Summary

When generating a **C** parser (the default/`-T <template>` path, no `--target=rust`),
the v0.10.0 `lime` tool prints an unconditional, spurious line to stderr:

```
warning: --enable=safe has no effect without --target=rust
```

We never pass `--enable=safe`. The warning originates from a tool **default**
(`safe` defaults ON) that is simultaneously marked **rust-only**, so the
"has no effect" warning fires on *every* C-target invocation. v0.8.7 did not
emit this line.

The parser is still generated correctly, but the extra stderr line breaks
downstream build systems that scan `lime`'s stderr to distinguish *resolved
shift/reduce conflicts* (tolerated; `lime` exits non-zero for these) from
*real errors* (fatal). Our `build.rs` tolerates an exit code of 1 only when
every stderr line is a conflict line; the new warning line makes that check
fail and aborts the build.

## Environment

- macOS (arm64, Apple clang) and Linux (gcc) — reproduces on both
- Lime built from tag `v0.10.0` (commit `85bbef6`)
- Grammar: a ~1000-rule LALR(1) SQL grammar (PostgreSQL dialect) that resolves
  to 59 shift/reduce conflicts (all resolved by Lime's default SHIFT
  preference — expected and correct)

## Reproduction

```sh
git clone https://codeberg.org/gregburd/lime && cd lime && git checkout v0.10.0

# Build the host tool (the source set grew in v0.10; mirrors meson.build's
# `lime` executable + the lex-compiler static library):
cc -O2 -DLIME_HAS_LEX_COMPILER -DLIME_HAS_RUST_OUTPUT \
   -Isrc -Isrc/lex -Iinclude -o /tmp/lime \
   lime.c src/emit_rust.c src/emit_c_skin_bison.c src/jit_inline.c \
   src/lex/lex_tokenize.c src/lex/lex_ast.c src/lex/lex_parse.c \
   src/lex/lex_resolve.c src/lex/lex_pretty.c src/lex/lex_main.c \
   src/lex/lex_regex.c src/lex/lex_nfa.c src/lex/lex_dfa.c \
   src/lex/lex_dfa_min.c src/lex/lex_compile.c src/lex/lex_emit.c \
   src/lex/emit_rust_lex.c src/lex/emit_rust_skin_logos.c \
   src/lex/emit_c_skin_flex.c src/lex/lex_introspect.c

# Run on ANY grammar that has resolved conflicts, C target, no --enable flags:
/tmp/lime -T limpar.c -q your_grammar.lime
```

### Actual (v0.10.0)

```
warning: --enable=safe has no effect without --target=rust
59 parsing conflicts.
```
(exit code 1; `your_grammar.c` / `.h` ARE generated)

### Expected (matches v0.8.7)

```
59 parsing conflicts.
```
(exit code 1; output generated) — i.e. **no `--enable=safe` warning**, because
the user never enabled `safe`.

## Root cause (in `lime.c`, tag v0.10.0 / commit `85bbef6`)

1. **`lime.c:4822`** — the global default initializer sets `safe` ON:
   ```c
   static feature_flag_state g_features = {
       ...
       .safe = 1,
   ```
2. **`lime.c:4869`** — `safe` is marked **rust-only** (third column `1`,
   matching `crate`/`nostd`):
   ```c
   } g_feature_table[] = {
       { "simd",          ..., 0 },
       { "memchr",        ..., 1 },
       { "crate",         ..., 1 },
       { "nostd",         ..., 1 },
       { "safe",          ..., 1 },   // rust_only = 1
   ```
3. **`lime.c:~5446`** — the "no effect" warning loop fires for any rust-only
   feature whose flag is set when the target is not Rust:
   ```c
   if (!rust_target) {
       for (...) {
           if (!g_feature_table[fi].rust_only) continue;
           if (*fp) {   // <-- *fp == 1 for `safe` BY DEFAULT
               fprintf(stderr,
                 "warning: --enable=%s has no effect without --target=rust\n",
                 g_feature_table[fi].name);
           }
       }
   }
   ```
4. **`lime.c:~5450`** — the code's own comment documents the invariant that
   this **violates**:
   > "Only warn when the user OPT'd in (value 1). Defaults are off for all
   > rust_only features so this never fires absent an explicit `--enable`."

   `safe` is the one rust-only feature whose default is **on**, so the warning
   fires unconditionally for every C-target build, with no explicit `--enable`.

## Suggested fixes (any one unblocks us)

- **Preferred:** track whether each feature was *explicitly* set on the command
  line vs left at its default, and only emit the "no effect" warning for
  explicitly-enabled features (this is the stated intent of the comment at
  `lime.c:~5450`).
- Or: only default `safe = 1` once `--target=rust` is selected (make the
  default target-conditional), leaving it off for the C target.
- Or: don't classify `safe` as `rust_only`, or suppress the warning for `safe`
  specifically when it is at its default value.

## Why we are holding at v0.8.7

The generated parser is correct, but a clean stderr is part of Lime's contract
with build systems that classify exit-1 (resolved conflicts vs hard error).
Rather than special-case the warning string in our `build.rs` (brittle — it
would mask future legitimate warnings), we are pinning `crates/lime-sys/lime`
to v0.8.7 until the spurious warning is resolved upstream.

Secondary, non-blocking note for the upgrade: v0.10.0 split the generator into
~40 sources plus a `lime_lex_compiler` static library, so the host-tool build
recipe changed (see the `cc` line above, derived from `meson.build`). A
documented "canonical host-tool source set" (or a stable build target /
amalgamation) would make consumer build scripts robust across releases.
