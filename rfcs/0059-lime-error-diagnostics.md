# RFC 0059: Lime Parser Error Diagnostics Requirements

## Summary

Ra currently uses the Lime LALR(1) parser generator to parse SQL into relational algebra. Error reporting is limited to plain strings with minimal context. To match Rust compiler-style diagnostics (precise source locations, expected token hints, multiple errors per parse), Lime must be enhanced to expose LALR parser state information and the tokenizer must provide token span data.

## Current State

### Error Information Flow

1. **Tokenizer**: `lime_tokenizer.rs` (SIMD C code via FFI) and pure Rust fallback `lexer.rs`
   - Produces tokens with only **start byte offset** (`location: i32`)
   - No token length field
   - Location only: `RaToken { text, location, int_val, float_val }`

2. **Lime Parser**: Generated C from `grammar/ra_sql.lime`
   - Configured with `%syntax_error` directive (lines 35-40 of grammar)
   - Currently discards all context: `(void)pstate; (void)yymajor; (void)yyminor;`
   - `%parse_failure` exists but unused (lines 42-44)
   - No access to LALR action table or expected token set

3. **Parse State**: `RaParseState` in `ffi/node.rs`
   - `errors: Vec<String>` (lines 140)
   - `push_error(&mut self, msg: String)` method (lines 212-215)
   - All errors are unstructured strings

4. **FFI Builders**: `ffi/builders.rs`
   - 100+ reduction actions calling Rust from C
   - Error handling: decode failure -> `push_error()` -> null return
   - Example: `decode_rel()` returns `None` on mismatch, triggers error string

5. **Error Display**: `ra-cli/src/output/errors.rs`
   - Manual regex parsing to extract "at position X" from error strings
   - Regex parsing for "Line:N, Column:M" patterns
   - **Heuristic-based** token detection via `extract_token()`
   - Hardcoded help for specific operators (::, ->, ->>, INTERVAL, JSON literals)
   - Caret length guessed from token text or defaulted to 1 character

### Limitations

| Issue | Current Approach | Problem |
|-------|------------------|---------|
| Token span | Start offset only | Can't underline multi-char tokens; users see single `^` for "BETWEEN" |
| Expected tokens | None | Parser says "syntax error" without telling user valid options |
| Multiple errors | Stops at first | Users see one error, fix it, then hit next error |
| Location precision | String parsing | Regex fragile if error message format changes; false positives |
| Parser state access | None | No way to query LALR tables for expected tokens at a state |
| Error context | Manual extraction | Heuristic-based help; doesn't scale to new unsupported features |

## Required Changes: Lime Enhancements

### 1. Structured Error Data Type (C level)

**File**: `_/lime/limpar.c` (Lime parser template)

```c
typedef struct {
    int position;                   /* byte offset in source */
    int length;                     /* byte length of problematic token */
    const char *text;               /* token text (may be NULL) */
    const char *message;            /* human-readable error, no position */
    const char **expected_tokens;   /* array of token names */
    int expected_count;             /* length of expected_tokens */
} LimeParseError;
```

### 2. Token Span Information (FFI level)

**Current**: `lime_tokenizer.c` outputs tokens with only `location` (start)

**Required**: Extend `RaToken` struct to include end position or length

**File**: `grammar/ra_ffi.h`

```c
typedef struct {
    const char *text;      /* NUL-terminated text (for IDENT, SCONST) */
    int location;          /* byte offset of token start */
    int length;            /* byte length of token */
    int64_t int_val;       /* integer value for ICONST */
    double float_val;      /* float value for FCONST */
} RaToken;
```

**Impact**:
- Lexer (`lexer.rs`): Compute `length` in each `LexToken` construction
- SIMD tokenizer (`lime_tokenizer.rs`): Extract length from C tokenizer output
- Grammar rules: Pass token length via FFI when building nodes needing spans
- Backward compatibility: Old code can ignore `length` field (defaults to 0 or 1)

### 3. Expected Token Set Extraction

**Solution Path A (Lime-native, preferred)**:

Lime generates a helper function:

```c
void raGetExpectedTokens(
    int state,
    const char **token_names,     /* output: array of token name strings */
    int *token_count              /* output: count of expected tokens */
);
```

**Solution Path B (Fallback - heuristic)**:

Maintain a static mapping in Ra:

```rust
fn common_expected_after_state(last_token: TokenCode) -> Vec<&'static str> {
    match last_token {
        token::SELECT => vec!["IDENT", "STAR", "LPAREN"],
        token::FROM => vec!["IDENT"],
        token::WHERE => vec!["IDENT", "LPAREN", "NOT"],
        // ...
    }
}
```

### 4. Error Callback Signature Change

**File**: `grammar/ra_sql.lime`

**Current**:
```c
%syntax_error {
    (void)pstate;
    (void)yymajor;
    (void)yyminor;
}
```

**Required**:
```c
%syntax_error {
    ra_record_parse_error(pstate, yymajor, yyminor);
}
```

**Implementation** (`ffi/builders.rs`):

```rust
#[no_mangle]
extern "C" fn ra_record_parse_error(
    pstate: *mut RaParseState,
    token_code: c_int,
    token: RaToken,
) {
    let state = match unsafe { state_ref(pstate) } {
        Some(s) => s,
        None => return,
    };

    let error = StructuredParseError {
        position: token.location as usize,
        token_length: token.length.max(1) as usize,
        token_text: unsafe { c_str_to_string(token.text) },
        message: format!("unexpected token {}", token_code_name(token_code)),
        expected_tokens: query_expected_tokens(token_code),
    };

    state.push_structured_error(error);
}
```

### 5. Multi-Error Recovery

**Mechanism**: Use Lime's `%parse_failure` directive + error tokens

```c
%parse_failure {
    /* Called when parser cannot recover from error. */
}

%token ERROR.

stmt(A) ::= error(E). {
    A = NULL;  /* consume error token, continue parsing */
}
```

## Required Changes: Ra Integration

### 1. Replace `RaParseState.errors: Vec<String>`

**File**: `ffi/node.rs`

```rust
#[derive(Debug, Clone)]
pub struct StructuredParseError {
    pub position: usize,
    pub token_length: usize,
    pub token_text: String,
    pub message: String,
    pub expected_tokens: Vec<String>,
}
```

### 2. Source Spans in AST Nodes

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn from_token(token: &RaToken) -> Self {
        Self {
            start: token.location as usize,
            end: (token.location as usize) + (token.length as usize),
        }
    }

    pub fn merge(a: Span, b: Span) -> Self {
        Self {
            start: a.start.min(b.start),
            end: a.end.max(b.end),
        }
    }
}
```

## Priority Order

### Phase 1 (High Impact, Medium Effort)

1. **Token span information** -- enables precise caret positioning
2. **Structured error data type** -- replaces fragile string parsing
3. **Update error display** -- use structured data for clean output

### Phase 2 (High Impact, High Effort)

4. **Expected token extraction** -- users see valid alternatives
5. **Multi-error recovery** -- single pass reports all errors

### Phase 3 (Medium Impact, Low Effort)

6. **Source spans in AST** -- later pipeline phases can report errors with locations

## Summary Table

| Component | Current | Required | Effort | Priority |
|-----------|---------|----------|--------|----------|
| Token span | start only | start + length | Low | P0 |
| Error type | string | struct with position, length, expected | Low | P0 |
| Expected tokens | none | extracted from LALR state | Medium | P1 |
| Multi-error | no | optional recovery productions | Medium | P1 |
| Error callback | void callback | access token + state info | Low | P0 |
| AST spans | none | optional Span field | Low | P2 |

## Dependencies

### External: Lime Parser Generator

- Located at `_/lime/` in workspace
- Modified files: `limpar.c` (template), `lime.c` (generator), headers
- Fallback: heuristic lookup table maintains 80% of benefit with zero Lime changes

### Internal: Ra Codebase

- `push_error(String)` -> `push_error_legacy(String)` for backward compat
- `RaToken.length` defaults to 0 (old code doesn't set it)
- Grammar rules unchanged; only FFI side effects differ
