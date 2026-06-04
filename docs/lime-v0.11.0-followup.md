Re: Lime v0.11.0 reply — follow-up (Part 2: Rust-generated parser)
==================================================================

To:       Greg Burd, Lime maintainer
From:     Ra (PostgreSQL planner replacement) — crates/ra-parser
Subject:  v0.11.0 confirmed unblocked on C target; resending Part 2
Date:     Thu Jun 4 2026


Part 1 — confirmed fixed in v0.11.0
-----------------------------------

Pulled v0.11.0 (a24a93f). The spurious `--enable=safe` warning is gone on our
C-target build:

    $ lime -T limpar.c -q ra_sql.lime
      59 parsing conflicts.        (exit 1, conflicts only — no safe warning)

Our build.rs conflict-tolerance check now classifies the build as clean with no
special-case string match. ra-parser builds and its 499 tests pass; lime-sys
runtime lib builds. We've upgraded the submodule to v0.11.0. Thank you for the
quick turnaround — and for the regression sub-tests #19/#20.

On the secondary "canonical host-tool source set" ask: **Option A (a single
tools/lime-amalgamation.c) is exactly what we want.** Our build.rs currently
hand-curates the host-tool source list (lime.c + emit_rust.c +
emit_c_skin_bison.c + jit_inline.c + the 16 src/lex/*.c files, with
-DLIME_HAS_LEX_COMPILER -DLIME_HAS_RUST_OUTPUT) — fragile exactly as you note.
One `cc tools/lime-amalgamation.c` would make it robust. Option A preferred
over pre-built binaries (we build on macOS/arm64 and Linux/x86_64 from source).


Part 2 — Rust-generated parser (resent; was truncated in your copy)
-------------------------------------------------------------------

You're right that the Rust section didn't reach you. Here it is. We evaluated
migrating Ra off the C-generated parser (ra_sql.c via cc + the lime-sys FFI) to
`--target=rust`, which would delete the entire C build + FFI shim. The generated
API is clean and self-contained — `pub struct raParser` with `new()` /
`push(token_code, value)` / `finalize() -> Result<bool, ParseError>`, a
`ReduceCtx`, overridable `on_syntax_error` / `on_parse_accept` callbacks, no
runtime-crate dependency. Three issues block adoption, in priority order.

### 2a. Action blocks are emitted verbatim → a C-action grammar yields non-compiling Rust (the real blocker)

`lime --target=rust` copies each production's action body unchanged into the
generated reduce code. Our grammar (ra_sql.lime, 1626 lines, **306 action
blocks**) has **C** bodies that call **65** C builder functions, e.g.

    expr(A) ::= expr(B) EQ ANY LPAREN expr(E) RPAREN. {
        RaNode *a = ra_list_new(pstate);
        a = ra_list_push(pstate, a, B);
        a = ra_list_push(pstate, a, E);
        A = ra_func(pstate, "__saoarr_eq_any", a);
    }

`ra_sql.rs` contains that body verbatim (`RaNode *a`, `->`, C calls, `C.text`),
which cannot compile as Rust — and lime reports success writing the file, with
no diagnostic. So a C-action grammar has no migration path short of hand-porting
all 306 actions and all 65 builder functions to Rust in a single flag-day cut
(the C and Rust parsers can't coexist during the port).

**Ask (your "preferred option" framing applies here too):** support
**per-target action blocks** in one grammar so we can port incrementally while
both targets keep building, e.g.

    expr(A) ::= expr(B) EQ ANY LPAREN expr(E) RPAREN.
        %action_c    { RaNode *a = ra_list_new(pstate); ... }
        %action_rust { let a = ra_list_new(pstate); ... A = ra_func(pstate, "__saoarr_eq_any", a); }

with `--target=c` emitting only the C bodies and `--target=rust` only the Rust
bodies (and a hard error — not silent success — if the chosen target has no body
for a production). That lets a large grammar migrate production-by-production
with a green build at every step. Short of that, please at least **emit a hard
error** when an action body is not valid for the selected target, so the failure
is at generation time rather than as a wall of rustc errors.

### 2b. `--target=rust` still requires a C template, with the wrong default name

    $ lime --target=rust ra_sql.lime
      Wrote Rust parser to ra_sql.rs
      Can't open the template file "./lempar.c".
      59 parsing conflicts.
      (exit 1)

A pure Rust target shouldn't need the C parser template at all; and the default
it looks for is `lempar.c` (the historic Lemon name) while the repo ships
`limpar.c`. Still present in v0.11.0.

**Ask:** under `--target=rust`, skip C-template emission entirely (or gate it
behind an explicit `--target=rust:bison`-style skin), and align the default
template name with the shipped `limpar.c`.

### 2c. Non-zero exit when the Rust artifact was produced

Like the conflicts case, the run exits non-zero even though `ra_sql.rs` was
written correctly (here mixing resolved-conflict exit with the lempar.c error),
so a build system can't distinguish "Rust parser generated, only resolved
conflicts remain" from a genuine failure.

**Ask:** exit 0 when the requested artifact was produced and the only
diagnostics are resolved shift/reduce conflicts.


Where this leaves us
--------------------

We're upgraded and stable on v0.11.0 (C target). We'd very much like to move to
the Rust parser to drop ~2k lines of C FFI, but 2a is a hard blocker without
per-target action support (a 306-action flag-day port is too risky for our
drop-in-PostgreSQL correctness bar). If you can land per-target action blocks
(2a) plus the lempar.c/exit fixes (2b/2c), we'll commit to porting Ra's grammar
to Rust actions incrementally and report back.

Thanks again,
-- Ra / crates/ra-parser
