# HOWTO: Consume the RFC 0059 Diagnostics API in Ra

This guide walks a Ra developer through adopting the parser diagnostics
API that Lime now exposes for RFC 0059. It assumes you've read the
RFC (`rfcs/0059-lime-error-diagnostics.md`) and have a working checkout
of `ra` with `_/lime` at commit `8c0a0d4` or later.

Lime's upstream documentation for the API lives at
`_/lime/docs/DIAGNOSTICS.md`. This HOWTO is Ra-specific.

## What Lime Now Provides

Three new public functions are emitted into every generated parser.
Names are renameable via `%name` or `-P`; the Ra grammar uses default
names so these are what you'll see in `ra_sql.c`:

```c
const char *ParseTokenName(int tokenCode);
int         ParseState(void *parser);
int         ParseExpectedTokens(int stateno, int *out, int max);
```

Plus an optional error-list helper in `_/lime/include/lime_error.h`:

```c
LimeError *lime_error_append(...);
size_t     lime_error_count(const LimeError *list);
void       lime_error_free(LimeError *err);
```

Ra will want to **ignore** `LimeError` and use its own
`StructuredParseError` (as the RFC describes) — the three diagnostic
functions are the real payload.

## What Lime Does NOT Provide (and Why That's Fine)

| RFC ask | Lime response | Ra action |
|---------|---------------|-----------|
| Token `length` field | Already in Lime's `Token` struct since day one | Expose it through FFI |
| `Span::merge` helper | `lime_location_merge` exists in `lime_location.h` | Use it, or roll your own in Rust |
| AST source spans | Host concern — Lime gives you the span per token | Thread it through `ra-parser` manually |

---

## Step 1: Update `lime-sys` FFI Bindings

**File:** `crates/lime-sys/src/lib.rs` (or wherever `extern "C"` blocks
live)

Add the three new function signatures:

```rust
extern "C" {
    // Existing bindings...

    /// Return the string name of a terminal token code, or NULL if
    /// out of range.  Safe to call with any integer.
    pub fn ParseTokenName(token_code: c_int) -> *const c_char;

    /// Return the current parser state number, or -1 if the handle
    /// is invalid.  A freshly-initialized parser is in state 0.
    pub fn ParseState(parser: *mut c_void) -> c_int;

    /// Fill `out` (up to `max` entries) with token codes valid at
    /// `stateno`.  Returns the total count that would be written
    /// (may exceed `max` -- caller should size the buffer and call
    /// again).  Pass `out = NULL` and `max = 0` to query the count.
    pub fn ParseExpectedTokens(
        stateno: c_int,
        out: *mut c_int,
        max: c_int,
    ) -> c_int;
}
```

## Step 2: Add `length` to `RaToken`

The RFC already specified this. **File:** `grammar/ra_ffi.h` (or
wherever the C-facing token struct lives):

```c
typedef struct {
    const char *text;      /* NUL-terminated text (for IDENT, SCONST) */
    int location;          /* byte offset of token start */
    int length;            /* byte length of token */    /* <-- NEW */
    int64_t int_val;
    double float_val;
} RaToken;
```

Rust side, mirror the change in the matching `repr(C)` struct and
update every construction site.

Sources of token length in Ra today:

- **SIMD tokenizer** (`lime_tokenizer.rs`): Lime's `Token` struct has
  `length: size_t`. Cast to `i32` and store.
- **Pure Rust lexer** (`lexer.rs`): compute `length` from each
  `LexToken`'s span — it's `span.end - span.start`.

Default `length` to 0 for old code paths. The RFC recommends treating 0
as "unknown, underline 1 char"; Ra's display layer can apply that
fallback.

## Step 3: Idiomatic Rust Wrappers

Put a thin safety/ergonomics layer in `crates/lime-rs/src/diagnostics.rs`
(or similar):

```rust
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

/// Safe wrapper around ParseTokenName.
pub fn token_name(code: i32) -> Option<&'static str> {
    let p: *const c_char = unsafe { lime_sys::ParseTokenName(code as c_int) };
    if p.is_null() {
        None
    } else {
        // yyTokenName strings are static data in the generated parser
        // so 'static is correct here.
        unsafe { Some(CStr::from_ptr(p).to_str().ok()?) }
    }
}

/// Current parser state.  Returns None if handle is invalid.
pub fn parser_state(parser: *mut c_void) -> Option<i32> {
    let s = unsafe { lime_sys::ParseState(parser) };
    if s < 0 { None } else { Some(s) }
}

/// Tokens valid at the given state, resolved to names.
pub fn expected_tokens(stateno: i32) -> Vec<&'static str> {
    if stateno < 0 { return Vec::new(); }

    let n = unsafe {
        lime_sys::ParseExpectedTokens(stateno as c_int, std::ptr::null_mut(), 0)
    };
    if n <= 0 { return Vec::new(); }

    let mut codes = vec![0 as c_int; n as usize];
    let written = unsafe {
        lime_sys::ParseExpectedTokens(stateno as c_int, codes.as_mut_ptr(), n)
    };
    assert_eq!(written, n, "ParseExpectedTokens count mismatch");

    codes.into_iter()
         .filter_map(|c| token_name(c as i32))
         .collect()
}
```

Why I put this in `lime-rs` not `lime-sys`: `-sys` crates should be
thin `extern "C"` bindings only. Ergonomics go in a sibling crate.

## Step 4: Rewrite the `%syntax_error` Hook

**File:** `grammar/ra_sql.lime`

Current state:

```c
%syntax_error {
    (void)pstate;
    (void)yymajor;
    (void)yyminor;
}
```

Replace with:

```c
%syntax_error {
    ra_record_parse_error(pstate, yymajor, yyminor, yypParser);
}
```

Pass `yypParser` in so the Rust side can call `ParseState` /
`ParseExpectedTokens`. `pstate` is your existing `RaParseState*`.

Then in `ffi/builders.rs`:

```rust
use lime_rs::diagnostics;

#[no_mangle]
pub extern "C" fn ra_record_parse_error(
    pstate:     *mut RaParseState,
    token_code: c_int,
    token:      RaToken,
    parser:     *mut c_void,
) {
    let Some(state) = (unsafe { state_ref_mut(pstate) }) else { return; };

    let expected = diagnostics::parser_state(parser)
        .map(diagnostics::expected_tokens)
        .unwrap_or_default();

    let token_name = diagnostics::token_name(token_code as i32)
        .unwrap_or("<unknown>")
        .to_string();

    state.push_structured_error(StructuredParseError {
        position:       token.location as usize,
        token_length:   std::cmp::max(token.length, 1) as usize,
        token_text:     safe_c_str(token.text),
        token_name,
        message:        "syntax error".into(),
        expected_tokens: expected.into_iter().map(String::from).collect(),
    });
}
```

Note: the `yypParser` identifier inside a `%syntax_error` block is
part of the RFC 0059 surface and is now documented in
`man/lime_grammar.5`. You don't need to redeclare it — Lime exposes
it automatically.

## Step 5: Replace `RaParseState.errors: Vec<String>`

**File:** `ffi/node.rs`

```rust
#[derive(Debug, Clone)]
pub struct StructuredParseError {
    pub position:        usize,
    pub token_length:    usize,
    pub token_text:      Option<String>,
    pub token_name:      String,        // e.g. "FROM", "SELECT"
    pub message:         String,
    pub expected_tokens: Vec<String>,
}

pub struct RaParseState {
    // ... existing fields ...
    pub structured_errors: Vec<StructuredParseError>,
    pub errors_legacy:     Vec<String>,  // keep during transition
}

impl RaParseState {
    pub fn push_error(&mut self, msg: String) {
        // Kept for backward compat; new code uses push_structured_error
        self.errors_legacy.push(msg);
    }

    pub fn push_structured_error(&mut self, err: StructuredParseError) {
        self.structured_errors.push(err);
    }
}
```

During migration, have the output layer read from *both* vectors. Once
all callsites are converted, `errors_legacy` can be deleted.

## Step 6: Rewrite the Error Display Layer

**File:** `ra-cli/src/output/errors.rs`

Rip out the regex-based position and caret extraction. The new flow:

```rust
pub fn render_error(source: &str, err: &StructuredParseError, out: &mut dyn Write) {
    let (line_no, col) = byte_offset_to_line_col(source, err.position);
    let line_text = line_at(source, line_no);

    writeln!(out, "error: {}", err.message).unwrap();
    writeln!(out, "  --> <input>:{}:{}", line_no, col).unwrap();
    writeln!(out, "   |").unwrap();
    writeln!(out, "{:>3}| {}", line_no, line_text).unwrap();
    write!(out, "   | {}", " ".repeat(col - 1)).unwrap();
    for _ in 0..err.token_length { write!(out, "^").unwrap(); }
    writeln!(out).unwrap();

    if !err.expected_tokens.is_empty() {
        writeln!(out, "   = expected one of: {}",
                 err.expected_tokens.join(", ")).unwrap();
    }
}
```

No more heuristic "extract_token" regex — the position and length come
directly from the token the parser rejected. The caret width is always
correct because `token_length` is authoritative.

Delete:
- The "at position X" regex
- The "Line:N, Column:M" regex
- The hardcoded help lookup table (`::`, `->`, etc.) — or keep it as a
  secondary "hint" layer keyed on `token_name` if you like the output

## Step 7: Multi-Error Recovery (Optional, Higher Value)

This is the change that lets users see every error in one pass instead
of running the parser seven times.

Add an error-recovery production at statement granularity in
`grammar/ra_sql.lime`:

```
stmt_list ::= stmt.
stmt_list ::= stmt_list SEMI stmt.

stmt ::= select_stmt.
stmt ::= insert_stmt.
stmt ::= update_stmt.
stmt ::= delete_stmt.
/* New: */
stmt ::= error.   /* recover at next SEMI */
```

Lime's parser will now:

1. Call `%syntax_error` on a bad token.
2. Pop states until it finds one where `error` can be shifted.
3. Skip tokens until it finds what follows `error` in a recovery rule
   (here, `SEMI` in `stmt_list`).
4. Continue parsing from there.

The built-in three-token resync rule means after shifting `error`, the
parser won't report another error until three real tokens have been
successfully shifted. Keeps error cascades sane.

Wire up `%parse_failure` for the case where recovery fails completely:

```c
%parse_failure {
    ra_record_parse_failure(pstate);
}
```

## Step 8: AST Source Spans

This is fully on the Ra side. The RFC's suggested `Span` type is fine;
the key change is at the construction sites in `ffi/builders.rs`:

```rust
impl Span {
    pub fn from_token(tok: &RaToken) -> Self {
        Self {
            start: tok.location as usize,
            end:   (tok.location as usize) + (tok.length as usize),
        }
    }
}
```

Every grammar reduction that constructs an AST node should set a `Span`
that merges the spans of its constituent tokens. `lime_location_merge`
is a ready-made helper for the C side; in Rust:

```rust
impl Span {
    pub fn merge(a: Span, b: Span) -> Self {
        Self { start: a.start.min(b.start), end: a.end.max(b.end) }
    }
}
```

## Verification Checklist

After each step:

- [ ] `cargo build -p lime-sys` — FFI bindings compile
- [ ] `cargo test -p ra-sql-parser` — parser tests still pass
- [ ] `cargo test -p ra-cli` — CLI output tests pass (you may need to
      update golden files because error formatting changed)
- [ ] Manual check: run `ra parse "SELECT * FROM"` and confirm the
      output shows:
      - Caret at position 14, length 0 (EOF) with a sensible fallback
      - "expected one of: IDENT" or similar
- [ ] Manual check: run `ra parse "SELECT * FROM t WHERE ?; SELECT 1"`
      and confirm BOTH errors appear (recovery works)

## Memory Safety Notes

- **`ParseTokenName` lifetime:** the returned `*const c_char` points
  into the generated parser's static `yyTokenName[]` array. It's safe
  to cast to `&'static CStr` in safe Rust.
- **`ParseState` / `ParseExpectedTokens`:** don't call them after
  `ParseFree`. Ra's parser lifecycle already scopes `*mut c_void`
  correctly inside parse sessions, so this should be automatic.
- **Buffer sizing:** always use the two-call pattern for
  `ParseExpectedTokens` — first call with `NULL, 0` to get the count,
  then allocate, then call again. The wrapper in Step 3 does this
  correctly.

## When Things Go Wrong

**`ParseTokenName` returns NULL for a valid-looking code.**
The `yyTokenName` table covers only terminals (codes `0..YYNTOKEN-1`).
Non-terminal codes and the internal `YYNOCODE` sentinel return NULL.
This is correct behavior — filter NULLs out at the edge, as the Step 3
wrapper does.

**`ParseState` returns -1.**
Either the handle is NULL, or the parser is somehow in an invalid
state. Check the handle; if it's non-null, you're probably calling
after `ParseFree`.

**`ParseExpectedTokens` returns 0 for a valid state.**
Some grammar states have empty expected-token sets (e.g., just before
reduction). In Ra's grammar this is rare and almost always indicates
the error happened at a state with a single reduce action. Fall back
to "unexpected token X" without the "expected one of" hint.

**Cascading errors after recovery.**
The three-token resync rule suppresses the first few secondary errors,
but not all. If the grammar's `error` rule placement is too loose,
you'll see noise. Tighten recovery points to statement boundaries —
don't add `error` productions inside expressions.

## Reference

- **RFC:** `rfcs/0059-lime-error-diagnostics.md`
- **Lime guide:** `_/lime/docs/DIAGNOSTICS.md`
- **Lime man page:** `_/lime/man/lime_grammar.5` (section "Diagnostics API")
- **Lime commit:** `fa14b77` — diagnostics API implementation
- **Example test:** `_/lime/tests/test_diagnostics.c` — working end-to-end use
