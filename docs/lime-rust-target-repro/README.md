# Lime `--target=rust` reproducer kit

Self-contained, minimal reproductions of the three issues blocking Ra from
adopting Lime's Rust-generated parser. Tested against **v0.11.0** (commit
`a24a93f`). Hand to the Lime maintainer alongside `docs/lime-v0.11.0-followup.md`.

Files: `multiline_action.lime` (20-line grammar, two rules differing only in
action-body layout).

## Build the host tool

```sh
# from a v0.11.0 checkout of the lime repo:
cc -O2 -DLIME_HAS_LEX_COMPILER -DLIME_HAS_RUST_OUTPUT -Isrc -Isrc/lex -Iinclude \
   -o /tmp/lime lime.c src/emit_rust.c src/emit_c_skin_bison.c src/jit_inline.c \
   src/lex/*.c
```

## Issue 1 — multi-line action bodies are SILENTLY DROPPED (most severe)

The Rust emitter keeps an action body only when text follows `{` **on the same
line**. When `{` is immediately followed by a newline — the normal multi-line
form — the body is replaced with `// empty action` and silently discarded. No
warning, no error. The generated parser compiles but the production does
nothing, so the parse produces wrong results with no diagnostic.

```sh
/tmp/lime --target=rust multiline_action.lime
```

`multiline_action.lime` has two reachable rules for `stmt`:

```
stmt(A) ::= NUM(B) PLUS NUM(C). {      <-- '{' then newline
    A = B + C;
}
stmt(A) ::= NUM(B). { A = B; }         <-- body on the '{' line
```

### Actual (v0.11.0) — generated `multiline_action.rs`

```rust
fn yy_rule_0(ctx: &mut ReduceCtx) {            // NUM PLUS NUM  (multi-line)
    let mut A: Value = Value::default();
    let B: Value = ctx.rhs[0].clone();
    let _rhs1: Value = ctx.rhs[1].clone();
    let C: Value = ctx.rhs[2].clone();
    // empty action                            <-- BODY `A = B + C;` DROPPED
    *ctx.lhs = A;
}
fn yy_rule_1(ctx: &mut ReduceCtx) {            // NUM           (single-line)
    let mut A: Value = Value::default();
    let B: Value = ctx.rhs[0].clone();
    // user action body (literal copy with $$/$N substitution)
     A = B;                                     <-- body kept
    *ctx.lhs = A;
}
```

### Expected

Both bodies preserved. `yy_rule_0` should contain `A = B + C;`. The body
extractor must capture the full `{ ... }` span regardless of where the first
newline falls (almost every real-world action opens with `{`-then-newline).

## Issue 2 — bodies are copied verbatim (no C→Rust translation)

Note the comment in `yy_rule_1`: *"user action body (literal copy with $$/$N
substitution)"*. Bodies that survive Issue 1 are copied **literally**. A grammar
whose actions are written for the C target (`Node *n = ...; n->field; (cast)x;`)
therefore emits Rust that does not compile, with no generation-time diagnostic.
Confirmed for `A = add(B,C);`, `Node *n; A = B;`, `A = x->y;`, `A = (int)B;` —
all copied verbatim.

This is the fundamental barrier for a large existing C-action grammar (Ra's is
306 action blocks / 65 builder functions): there is no incremental path.

**Ask (preferred):** per-target action blocks in one grammar, e.g.

```
stmt(A) ::= NUM(B) PLUS NUM(C).
    %action_c    { A = make_int(node_add(B, C)); }
    %action_rust { A = self.make_int(self.node_add(B, C)); }
```

so `--target=c` and `--target=rust` each emit only their own bodies and a grammar
can migrate production-by-production with a green build at every step. Failing
that, **emit a hard error at generation time** when a body is not valid for the
selected target (instead of silent drop / verbatim-copy-then-rustc-explosion).

## Issue 3 — `--target=rust` requires a C template (wrong default name) and exits non-zero

```
Wrote Rust parser to multiline_action.rs
Can't open the template file "/tmp/lime/lempar.c".   <-- ships as limpar.c, not lempar.c
EXIT=1
```

A pure Rust target should not need the C parser template at all. The default it
looks for is `lempar.c` (the historic Lemon name) resolved next to the tool
binary, while the repo ships `limpar.c`. And the run exits non-zero even though
`multiline_action.rs` was written, so a build system cannot distinguish success
from failure.

**Ask:** under `--target=rust`, skip C-template emission entirely (or gate it
behind an explicit `--target=rust:bison`-style skin); align the default template
name with the shipped `limpar.c`; and exit 0 when the requested artifact was
produced and the only remaining diagnostics are resolved shift/reduce conflicts.

## Note on the generated API (this part is good)

The emitted `raParser` API is clean and self-contained — `new()` /
`push(token_code, value)` / `finalize() -> Result<bool, ParseError>`, a
`ReduceCtx`, overridable `on_syntax_error` / `on_parse_accept`, no runtime-crate
dependency. Once Issues 1–3 are addressed, Ra is ready to port its grammar's
actions to Rust and drop ~2k lines of C FFI.
