> **RESOLVED in Lime v1.1.0 (2026-06-06).** Lime shipped `YY_TOKEN_NAMES`,
> `token_name(code)`, `expected_tokens_in_state(state)`, and
> `yy_find_shift_action` on the Rust target (default-on for `--target=rust`).
> Ra now builds the structured "expected one of …" diagnostics from these in
> `rust_parser::driver`, and the legacy C parser has been fully retired. The
> original request follows for the record.

Re: Lime v1.0.0 — Rust target shipped in production; one diagnostics gap left
=============================================================================

To:       Greg Burd, Lime maintainer
From:     Ra (PostgreSQL planner replacement) — crates/ra-parser
Subject:  C→Rust parser migration complete on v1.0.0; request: syntax-error
          introspection (token names + expected-token set) on the Rust target
Date:     Sat Jun 6 2026


Part 1 — we shipped the Rust target
-----------------------------------

The full flag-day migration is done. As of v1.0.0, `ra-parser` parses SQL
through Lime's `--target=rust` output by default; the C parser path
(`ra_sql.c` + the `ra()`/`raAlloc` FFI) is retired to `--no-default-features`
and no longer generated or compiled on the default build.

What it took on our side, for the record:
  - 288 productions in `ra_sql.lime` now carry `%action_rust` twins alongside
    their C bodies (33 pure `A = B` passthroughs compile verbatim on both
    targets). The `%action_c`/`%action_rust` pair you added in v0.12.0 is
    exactly what made a production-by-production migration possible — both
    targets stayed green at every commit. Thank you.
  - A native Rust builder layer (63 fns) backs the reduction actions.
  - A Rust driver feeds our existing tokenizer's tokens to the generated
    `raParser` via `new()` / `push(code, value)` / `finalize()`.

Result: our full parser suite (583 tests) passes on the Rust target with
RelExpr trees identical to the C target, and the wider engine (2013 tests)
is green with the Rust parser as the default. The codegen contract behaved
exactly as documented in `docs/RUST_OUTPUT.md` — single `Value` type,
`ReduceCtx { lhs, rhs, user }`, alias locals bound in `%action_rust` bodies,
`FIRST_TOKEN = 0` so our hand-maintained token codes matched 1:1.

A couple of small notes that may help the next user (not requests):
  - The generated `ra_sql.rs` carries its own crate-level `#![allow(...)]`
    inner attributes. Because we `include!` it into a module (rather than use
    it as a crate root), we strip those `#![...]` lines in our build script —
    a one-liner, but worth a sentence in RUST_OUTPUT.md for `include!` users.
  - Nested reduction calls that both take the user arg (e.g.
    `binop(user, op, const_int(user, 0), b)`) double-borrow `&mut UserArg`
    in safe Rust. We sidestep it by having our builders take `*mut State`
    (Copy) rather than `&mut State`. Not a Lime issue, but a foot-gun worth a
    note for grammars whose actions thread the user arg through nested calls.


Part 2 — the one gap: structured syntax-error hints on the Rust target
----------------------------------------------------------------------

This is the only feature the Rust target cannot yet match on the C target,
and it's the thing we need from a future Lime release.

On the C target we produce rich syntax errors like:

    syntax error: unexpected 'FORM' — expected one of: FROM, ',', '.', JOIN, WHERE

We build that in `crates/ra-parser/src/lime_parser/diagnostics.rs` from three
helpers we hang off the C parser's generated tables/state:

    raTokenName(code)            -> the %token name for a terminal code
                                    (reads the generated yyTokenName table)
    raState(parser)              -> the parser's current LALR state number
    raExpectedTokens(stateno, …) -> walks the action table for `stateno` and
                                    returns every terminal with a non-error
                                    (shift/reduce) action — i.e. the valid
                                    lookahead set at that state

The Rust target gives us the offending token and state via the
`%rust_syntax_error { … }` hook (`_token: u16`, `_state: u16`, with `self`
available), and it exposes the `YY_ACTION` / `YY_LOOKAHEAD` / `YY_SHIFT_OFST`
/ `YY_REDUCE_OFST` / `YY_DEFAULT` tables as `pub static`s. But it does **not**
expose:

  1. A **token-name table** — there is no Rust equivalent of `yyTokenName`.
     We can name `SELECT` only because we keep our own hand-maintained code↔
     name list; a grammar without one cannot name the offending token at all.

  2. A way to **enumerate the expected/valid terminals at a state**. The
     shift-action lookup (the Rust equivalent of `yy_find_shift_action`) is a
     private fn, so we cannot replicate `raExpectedTokens`' table walk without
     copying internal dispatch logic that is not part of the stable surface.

So on the Rust path we currently degrade to:

    syntax error at token 23 (parser state 47)

— correct and non-fatal, but it drops both the token name and the
"expected one of …" set. (This is purely an error-message regression; parsing
and all RelExpr output are identical.)


What we need from a future Lime release (concrete)
--------------------------------------------------

Any one of these closes the gap; (a)+(b) together is the ideal:

  (a) Emit a token-name table in the Rust output, mirroring `yyTokenName`:

          pub static YY_TOKEN_NAMES: &[&str] = &[ /* indexed by token code */ ];

      Gate it behind a flag if size is a concern (e.g.
      `--enable=token-names`), exactly as Lemon gates `yyTokenName` behind
      `-d`/NDEBUG on the C side.

  (b) A public, stable way to compute the expected-token set. Preferred form
      is a method on the generated parser:

          impl <Name>Parser {
              /// Terminal codes with a non-error action in the current state.
              pub fn expected_tokens(&self) -> Vec<u16>;
              // or, state-indexed:
              pub fn expected_tokens_in_state(state: u16) -> Vec<u16>;
          }

      Equivalently, exposing the shift-action lookup as
      `pub fn yy_find_shift_action(state: u16, token: u16) -> u16` plus the
      `YY_*` dispatch constants would let us compute it ourselves from the
      already-public tables.

  (c) (Nice-to-have) Let `%rust_syntax_error` receive the expected set
      directly, e.g. the hook body sees an `expected: &[u16]` in scope, so the
      message can be built inline without a second call. With (a)+(b) this is
      unnecessary.

The C side already proves the table walk is cheap and self-contained
(`raExpectedTokens` is ~30 lines over the action table). We're asking for the
same capability to be surfaced on the Rust target.


Status / tracking
-----------------

  - We are on Lime v1.0.0, Rust target, in production. No action needed from us
    to keep current builds working.
  - This request is the **only** remaining reason we'd ever reach for the C
    parser. Once Lime ships (a)+(b) on the Rust target, we will reimplement
    `diagnostics.rs` against the Rust API and can delete the legacy C path
    (and the `--no-default-features` fallback) outright.
  - TODO for Ra, on the next Lime release: re-read the changelog for Rust-side
    token-name / expected-token support; if present, port `diagnostics.rs` to
    it, restore "expected one of …" hints on the Rust path, and retire the C
    parser entirely.

Thanks again — the v0.12.0 per-target action blocks and the v1.0.0 stability
commitment made this migration genuinely smooth.

— Ra / ra-parser
