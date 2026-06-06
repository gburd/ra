//! Rust parse driver: tokenize with the existing Rust/SIMD tokenizer, feed the
//! tokens to the Lime-generated `raParser`, and extract the root `RelExpr`.
//!
//! This is the native-Rust counterpart to [`crate::lime_parser::parse_sql`].
//! It reuses the same tokenizers and the same `RaParseState` arenas + builder
//! logic, so it produces identical `RelExpr` trees; only the parser core (the
//! LALR state machine + reduction dispatch) changes from C to Rust.

use ra_core::algebra::RelExpr;

use crate::ffi::node::{ParseErrors, RaParseState};
use crate::lime_parser::{lexer, lime_tokenizer};
use crate::rust_parser::generated::raParser;
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

    for tok in &tokens {
        let text = tok
            .text_backing
            .as_ref()
            .and_then(|c| c.to_str().ok())
            .unwrap_or("");
        let value = Value::from_token(text, tok.value.int_val, tok.value.float_val, tok.value.location);
        // A push error means a syntax error was hit; the %rust_syntax_error
        // hook has already recorded it in the user state. Stop feeding.
        let code = u16::try_from(tok.code).unwrap_or(0);
        if parser.push(code, value).is_err() {
            break;
        }
    }
    // Feed EOF (token code 0) to finalize.
    let _ = parser.finalize();

    // The arenas (and any recorded errors) live in the parser's user arg.
    let state: RaParseState = parser.user;
    state.take_result()
}
