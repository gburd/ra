Re: Lime v0.10.0 upgrade blocker -- spurious `--enable=safe` warning
====================================================================

To:       Ra (PostgreSQL planner replacement) -- crates/ra-parser
From:     Lime maintainer
Subject:  Closed in v0.11.0 (commit a24a93f, tag v0.11.0)
Date:     Thu Jun 4 2026


Confirmed and fixed.  Pull v0.11.0 (commit a24a93f).


Diagnosis matched yours
-----------------------

Your three points landed exactly where the bug was:

  1. lime.c:4822 -- g_features.safe defaulting to 1
  2. lime.c:4869 -- safe marked rust_only = 1
  3. lime.c:~5446 -- warning loop gating on `*fp` only,
     not on whether the user explicitly opted in

The fourth point you cited -- the comment at lime.c:~5450
documenting an invariant "defaults are off for all rust_only
features so this never fires absent an explicit --enable" --
was true through v0.9.0 but became false in v0.9.3 when "safe
Rust default" flipped safe's default to 1.  The warning code
didn't get the memo.  My bug; my apology for not catching it
in the v0.9.3 ASan/UBSan run -- those tests don't scrape
stderr for spurious lines.


The fix
-------

Implemented your *preferred* option: explicit-tracking.

  static int g_feature_explicit[8] = {0};
  static void feature_mark_explicit(int idx) { ... }

Three sites mark explicit:

  1. feature_apply_list() for --enable=<list> / --disable=<list>
  2. handle_target_option() for --target=rust,unsafe
  3. The deprecated --rustlex-simd / --rustlex-memchr /
     --per-token-dfa aliases

The warning loop now gates on (`*fp` && `g_feature_explicit[fi]`)
instead of just (`*fp`).  Defaults that were never touched by
user flags no longer trigger the warning.

Two new sub-tests in tests/test_flag_scheme.c lock down the
regression:

  #19 -- C-target build with no --enable flags emits ZERO
         rust-only warnings (regression for your blocker).

  #20 -- explicit --enable=safe on a C-target DOES emit the
         "no effect" warning (verifies the warning still fires
         when it should -- I broke that once already in v0.9.0
         when I added the gating, no need to repeat).


Verification
------------

  $ build/lime g.lime
    exit 0, no warning   (was: spurious safe warning)

  $ build/lime --enable=safe g.lime
    exit 0, warning      (correct: user explicitly opted in)

  $ build/lime --enable=memchr g.lime
    exit 0, warning      (existing behaviour preserved)

Tests: 126 / 0 / 2 (stock Linux x86_64).  No regressions in
the rest of the suite.

Your build.rs's exit-1-only-when-stderr-is-conflicts logic
should now classify clean builds correctly without any
special-case string match.  If you still need to handle
shift/reduce-conflicts-as-warnings differently from real
errors, the existing exit-1-with-conflict-summary stderr
shape is unchanged.


On the secondary "canonical host-tool source set" concern
---------------------------------------------------------

You wrote:

  > v0.10.0 split the generator into ~40 sources plus a
  > lime_lex_compiler static library, so the host-tool build
  > recipe changed (see the cc line above, derived from
  > meson.build).  A documented "canonical host-tool source
  > set" (or a stable build target / amalgamation) would make
  > consumer build scripts robust across releases.

This is a legitimate ask.  Your hand-curated cc line is
fragile against future source-set changes (we added more
sources for the bison/flex/logos skin emitters, and v0.11.x
will likely add more).

Two paths I can offer:

  Option A: ship a `tools/lime-amalgamation.c` -- a single
            #include-based concatenation of the host-tool
            source set, generated from meson.build via a
            small script we run at release time.  Your
            build.rs compiles ONE file: cc -o lime
            tools/lime-amalgamation.c.  Stable across
            releases as long as the public CLI doesn't
            change.

  Option B: install the static archives + headers as part
            of `ninja install`, and document
            `pkg-config --libs lime-tool` for downstream
            consumers wanting to build their own host
            tool.  Heavier; requires consumers to handle
            link order, dependencies on libdl/libpthread/
            libLLVM (when JIT is enabled), etc.

I'm leaning Option A because it's a single file users can
drop into a build.rs without thinking.  Filed as v0.11.x or
v0.12.0 work; not blocking your unblock-at-v0.11.0 path.

Let me know if Option A would work, or if you'd prefer
something else (e.g. shipping pre-built lime binaries on
GitHub Releases for common targets).


On Part 2 (Rust-generated parser)
---------------------------------

Your letter mentions a "Part 2 ... Rust-generated parser
section near the end" but the file ends at line 146 with the
v0.8.7 hold-back rationale -- the Rust section isn't present
in the version I have.  If you intended to include it and
it got truncated, please send it as a follow-up letter and
I'll address it.  Otherwise we're at "Part 1 closed" with
no outstanding items.


Open ledger after v0.11.0
-------------------------

Closed:
  - Spurious --enable=safe warning on C-target builds
    (this letter)

Pending (none blocking your upgrade):
  - Canonical host-tool source set / amalgamation (your
    secondary ask).  Filed as v0.11.x.
  - Async LSP diagnostics + parse-only fast-lint
    (PG team's Lime-Letter-27, queued v0.10.x; carrying
     forward to v0.12).


Repo / commits
--------------

  Repo:    https://codeberg.org/gregburd/lime
  Mirror:  https://github.com/gburd/lime
  HEAD:    a24a93f
  Tag:     v0.11.0

Both pushed.  ABI unchanged from v0.10.0; no other consumer
should need to recompile.

-- Greg Burd, Lime maintainer
