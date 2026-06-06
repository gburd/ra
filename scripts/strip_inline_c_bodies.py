#!/usr/bin/env python3
"""Strip inline C action bodies from a Lime grammar (.lime).

Ra's Lime grammar carries, on most productions, BOTH an inline C action body
(`rule. { ...C... }`) and a Rust twin (`%action_rust { ...Rust... }`). The Rust
target uses only the `%action_rust` body; the inline C body is inert for codegen
but was historically required for Lime's alias-usage analysis. As of Lime
c0d68d0 that analysis scans the `%action_rust` body too, so the inline C bodies
can be removed.

This stripper is deliberately MINIMAL: it removes only the inline C action body
that immediately follows a rule's `.` terminator (preserving any per-rule
precedence marker, `rule. [UMINUS] { ... }`). It never touches directives
(`%include`, `%token_type`, ...), `%action_rust` blocks, token declarations,
precedence, or the productions themselves.

  ⚠️  BLOCKED ON A LIME FIX — DO NOT APPLY TO ra_sql.lime YET.

  The inline C bodies are LOAD-BEARING for Lime's Rust target. Lime derives
  `rule->noCode` from the *C* body only (`rp->code`), ignoring `rust_code`.
  Two consequences break the generated parser when inline C bodies are removed:
    1. Rule numbering: `rp->iRule = rp->code ? i++ : -1;` (lime.c:4416/5850/
       13580) numbers C-coded rules first and no-code rules last, so removing
       bodies reshuffles rule numbers.
    2. SHIFTREDUCE optimization (lime.c:13183): `if (rp->noCode==0) continue;`
       — Lime collapses every `nrhs==1` noCode nonterminal rule into a
       shift-reduce, DISCARDING its reduce action. A rule like
       `target_list ::= target_item` builds a list in its %action_rust; once
       its C body is gone it is treated as noCode, the reduce is discarded, the
       list-building never runs, and parsing breaks globally.
  The fix is in Lime: make `noCode` (and the iRule/SHIFTREDUCE logic) consider
  `rp->rust_code` for the Rust target. Once Lime treats a %rust_action body as
  "has code", this stripper produces a correct parser (verified: a single body
  removal already round-trips correctly; the failures are entirely the
  noCode/SHIFTREDUCE collapse above).

Two cases per inline C body:
  * twinned    -> a `%action_rust` block follows: delete the C body.
  * passthrough -> no `%action_rust` follows: promote the C body to a
    `%action_rust` block, translating the handful of C-isms to their Rust
    equivalents (the only forms that occur are `A = B;` passthroughs plus a few
    builder calls; see TRANSLATIONS).

Usage:  strip_inline_c_bodies.py GRAMMAR.lime [--report]
  --report  : classify bodies and print passthrough-only bodies, write nothing.
"""
from __future__ import annotations

import re
import sys

# C-ism -> Rust translations applied when promoting a passthrough-only body.
TRANSLATIONS = [
    (re.compile(r"\bpstate\b"), "ctx.user"),
    # `(void)X;` (C "use and discard") has no Rust form; bind-and-ignore.
    (re.compile(r"\(void\)\s*(\w+)\s*;"), r"let _ = \1;"),
]


def find_bodies(src: str):
    """Yield (open_idx, close_idx, kind) for every brace body in `src`.

    kind is 'rule' when the `{` follows a rule terminator `.`, 'rust' when it
    follows `%action_rust`, or 'directive' for anything else (%include etc.).
    Brace matching is string/char/comment aware.
    """
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == "{":
            # classify by the last non-space, non-comment token before `{`
            kind = classify_open(src, i)
            j = match_brace(src, i)
            yield (i, j, kind)
            i = j + 1
            continue
        i += 1


def classify_open(src: str, open_idx: int) -> str:
    """Classify a `{` by what precedes it (skipping whitespace/comments).

    A rule body's `{` is preceded by the rule terminator `.`, optionally with a
    per-rule precedence marker in between (`rule. [UMINUS] { ... }`). We skip a
    trailing `[...]` marker so precedence-tagged rules are still recognised as
    rule bodies (and stripped uniformly — Lime numbers rules with a C body
    before those without, so a partial strip splits the rule order and corrupts
    the tables; every rule must end up with no inline body).
    """
    k = open_idx - 1
    while k >= 0 and src[k] in " \t\r\n":
        k -= 1
    # skip a precedence marker `[TOKEN]` if present
    if k >= 0 and src[k] == "]":
        lb = src.rfind("[", 0, k)
        if lb != -1:
            k = lb - 1
            while k >= 0 and src[k] in " \t\r\n":
                k -= 1
    if k >= 0 and src[k] == ".":
        return "rule"
    # look back for a %action_rust / %include-style directive keyword
    start = max(0, k - 40)
    prev = src[start : k + 1]
    if re.search(r"%action_rust\s*$", prev):
        return "rust"
    return "directive"


def match_brace(src: str, open_idx: int) -> int:
    """Return index of the `}` matching the `{` at open_idx (string/comment aware)."""
    depth = 0
    i, n = open_idx, len(src)
    while i < n:
        c = src[i]
        two = src[i : i + 2]
        if two == "/*":
            end = src.find("*/", i + 2)
            i = (end + 2) if end != -1 else n
            continue
        if two == "//":
            end = src.find("\n", i + 2)
            i = (end + 1) if end != -1 else n
            continue
        if c == '"' or c == "'":
            i = skip_string(src, i)
            continue
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    raise ValueError(f"unbalanced brace opened at {open_idx}")


def skip_string(src: str, i: int) -> int:
    """Return index just past the string/char literal starting at i."""
    quote = src[i]
    i += 1
    n = len(src)
    while i < n:
        if src[i] == "\\":
            i += 2
            continue
        if src[i] == quote:
            return i + 1
        i += 1
    return n


def next_nonspace_is_action_rust(src: str, after_idx: int) -> bool:
    """True if the next non-whitespace/comment content after after_idx is %action_rust."""
    i, n = after_idx, len(src)
    while i < n:
        c = src[i]
        if c in " \t\r\n":
            i += 1
            continue
        if src[i : i + 2] == "/*":
            end = src.find("*/", i + 2)
            i = (end + 2) if end != -1 else n
            continue
        if src[i : i + 2] == "//":
            end = src.find("\n", i + 2)
            i = (end + 1) if end != -1 else n
            continue
        return src.startswith("%action_rust", i)
    return False


def promote_body(c_body: str) -> str:
    """Translate a passthrough C body to its Rust equivalent."""
    rust = c_body
    for pat, repl in TRANSLATIONS:
        rust = pat.sub(repl, rust)
    return rust


def transform(src: str, report: bool) -> str:
    bodies = list(find_bodies(src))
    edits = []  # (start, end, replacement) over the ORIGINAL src
    twinned = passthrough = 0
    for open_idx, close_idx, kind in bodies:
        if kind != "rule":
            continue
        # the body text including braces is src[open_idx:close_idx+1].
        # We remove ONLY the `{...}` braces, preserving everything between the
        # rule terminator `.` and the body — in particular a per-rule precedence
        # marker like `. [UMINUS] { ... }`, which is part of the grammar and
        # whose removal would change conflict resolution and the LALR tables.
        body_text = src[open_idx : close_idx + 1]
        if next_nonspace_is_action_rust(src, close_idx + 1):
            twinned += 1
            # delete just the inline C body braces; the %action_rust twin stays.
            edits.append((open_idx, close_idx + 1, ""))
        else:
            passthrough += 1
            inner = src[open_idx + 1 : close_idx]
            rust = promote_body(inner)
            if report:
                print(f"--- passthrough @char {open_idx}: {body_text.strip()}")
            # replace the C body in place with a %action_rust block.
            edits.append((open_idx, close_idx + 1, "%action_rust {" + rust + "}"))
    if report:
        print(f"\ntwinned (delete C body): {twinned}")
        print(f"passthrough (promote to %action_rust): {passthrough}")
        return src
    # apply edits back-to-front so indices stay valid
    out = src
    for start, end, repl in sorted(edits, key=lambda e: e[0], reverse=True):
        out = out[:start] + repl + out[end:]
    return out


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    report = "--report" in sys.argv
    if len(args) != 1:
        print(__doc__)
        return 2
    path = args[0]
    src = open(path, encoding="utf-8").read()
    out = transform(src, report)
    if report:
        return 0
    open(path, "w", encoding="utf-8").write(out)
    print(f"stripped inline C bodies from {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
