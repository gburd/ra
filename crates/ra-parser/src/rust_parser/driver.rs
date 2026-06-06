//! Rust parse driver: tokenize with the existing Rust/SIMD tokenizer, feed the
//! tokens to the Lime-generated `raParser`, and extract the root `RelExpr`.
//!
//! This is the native-Rust counterpart to [`crate::lime_parser::parse_sql`].
//! It reuses the same tokenizers and the same `RaParseState` arenas + builder
//! logic, so it produces identical `RelExpr` trees; only the parser core (the
//! LALR state machine + reduction dispatch) changes from C to Rust.
//!
//! On a syntax error, the driver builds a [`StructuredParseError`] using Lime
//! v1.1.0's Rust-target introspection (`token_name` + `expected_tokens_in_state`
//! from the generated module) together with the offending token's source
//! position/text — matching the rich "expected one of …" diagnostics the C
//! path produced via `raExpectedTokens`.

use ra_core::algebra::RelExpr;

use crate::ffi::node::{ParseErrors, RaParseState, StructuredParseError};
use crate::lime_parser::lexer::{self, LexToken};
use crate::lime_parser::lime_tokenizer;
use crate::rust_parser::generated::{self, raParser, ParseError};
use crate::rust_parser::Value;

/// Parse a SQL string into a `RelExpr` using the Lime-generated Rust parser.
///
/// # Errors
/// Returns accumulated parse/builder errors if the SQL cannot be parsed.
pub fn parse_sql(sql: &str) -> Result<RelExpr, ParseErrors> {
    // Same tokenization strategy as the C path: SIMD tokenizer first, falling
    // back to the pure-Rust lexer.
    let tokens = lime_tokenizer::tokenize_simd(sql)
        .or_else(|_| lexer::tokenize(sql))
        .map_err(|e| ParseErrors::Strings(vec![e]))?;

    let mut parser = raParser::new();
    let mut errored = false;

    for tok in &tokens {
        let text = tok
            .text_backing
            .as_ref()
            .and_then(|c| c.to_str().ok())
            .unwrap_or("");
        let value =
            Value::from_token(text, tok.value.int_val, tok.value.float_val, tok.value.location);
        let code = u16::try_from(tok.code).unwrap_or(0);
        if let Err(e) = parser.push(code, value) {
            record_parse_error(&mut parser.user, &e, Some(tok), sql);
            errored = true;
            break;
        }
    }
    // Feed EOF (token code 0) to finalize. A premature-EOF syntax error
    // surfaces here (no offending token — point at end of input).
    if !errored {
        if let Err(e) = parser.finalize() {
            record_parse_error(&mut parser.user, &e, None, sql);
        }
    }

    // The arenas (and any recorded errors) live in the parser's user arg.
    let state: RaParseState = parser.user;
    state.take_result()
}

/// Record a parser error into the parse state. Syntax errors become rich
/// [`StructuredParseError`]s (token name + expected-token set + position);
/// other parser errors become a plain message.
fn record_parse_error(
    state: &mut RaParseState,
    err: &ParseError,
    tok: Option<&LexToken>,
    sql: &str,
) {
    match err {
        ParseError::SyntaxError { token, state: pstate } => {
            state.push_structured_error(build_syntax_error(*token, *pstate, tok, sql));
        }
        other => {
            state.push_error(format!("parser error: {other:?}"));
        }
    }
}

/// Build a `StructuredParseError` from the offending token code, the parser
/// state (for the expected-token set), and the source token (for position and
/// text). Uses the generated v1.1.0 introspection helpers.
fn build_syntax_error(
    token: u16,
    pstate: u16,
    tok: Option<&LexToken>,
    sql: &str,
) -> StructuredParseError {
    let token_name = generated::token_name(token)
        .unwrap_or("end of input")
        .to_owned();
    let expected_tokens: Vec<String> = generated::expected_tokens_in_state(pstate)
        .into_iter()
        .filter_map(generated::token_name)
        .map(str::to_owned)
        .collect();

    let (position, token_length, token_text) = match tok {
        Some(t) => {
            let txt = t
                .text_backing
                .as_ref()
                .and_then(|c| c.to_str().ok())
                .map(str::to_owned);
            (
                usize::try_from(t.value.location).unwrap_or(0),
                usize::try_from(t.value.length).unwrap_or(1),
                txt,
            )
        }
        None => (sql.len(), 0, None),
    };

    let message = match &token_text {
        Some(t) => format!("syntax error: unexpected {token_name} '{t}'"),
        None => format!("syntax error: unexpected {token_name}"),
    };

    StructuredParseError {
        position,
        token_length,
        token_text,
        token_name,
        message,
        expected_tokens,
    }
}
