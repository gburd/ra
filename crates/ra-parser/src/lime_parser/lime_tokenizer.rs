//! SIMD-accelerated SQL tokenizer calling lime-sys FFI directly.
//!
//! Bypasses the lime-rs safe wrapper to eliminate per-invocation
//! overhead (buffer copy, `TokenTable` rwlock, enum conversion).
//! The C tokenizer runs in identifier-only mode (no `TokenTable`)
//! and keyword classification is done in Rust via a match.

use std::ffi::CString;
use std::mem::MaybeUninit;

use super::lexer::{token, LexToken, RaToken};

/// SIMD padding bytes required past the end of the input buffer.
const SIMD_PAD: usize = 32;

/// C token type codes from `tokenize.h`.
mod tk {
    pub const IDENTIFIER: i32 = -1;
    pub const INTEGER: i32 = -2;
    pub const FLOAT: i32 = -3;
    pub const STRING: i32 = -4;
    pub const LPAREN: i32 = -6;
    pub const RPAREN: i32 = -7;
    pub const SEMICOLON: i32 = -8;
    pub const COMMA: i32 = -9;
    pub const DOT: i32 = -10;
    pub const STAR: i32 = -11;
    pub const PLUS: i32 = -12;
    pub const MINUS: i32 = -13;
    pub const SLASH: i32 = -14;
    pub const PERCENT: i32 = -15;
    pub const EQ: i32 = -16;
    pub const NE: i32 = -17;
    pub const LT: i32 = -18;
    pub const GT: i32 = -19;
    pub const LE: i32 = -20;
    pub const GE: i32 = -21;
    pub const CONCAT: i32 = -27;
    pub const DQUOTE_ID: i32 = -28;
}

/// Map a raw C token code to the ra grammar token code.
fn map_c_code(code: i32) -> Option<i32> {
    match code {
        tk::IDENTIFIER | tk::DQUOTE_ID => Some(token::IDENT),
        tk::INTEGER => Some(token::ICONST),
        tk::FLOAT => Some(token::FCONST),
        tk::STRING => Some(token::SCONST),
        tk::LPAREN => Some(token::LPAREN),
        tk::RPAREN => Some(token::RPAREN),
        tk::SEMICOLON => Some(token::SEMICOLON),
        tk::COMMA => Some(token::COMMA),
        tk::DOT => Some(token::DOT),
        tk::STAR => Some(token::STAR),
        tk::EQ => Some(token::EQ),
        tk::NE => Some(token::NE),
        tk::LT => Some(token::LT),
        tk::GT => Some(token::GT),
        tk::LE => Some(token::LE),
        tk::GE => Some(token::GE),
        tk::PLUS => Some(token::PLUS),
        tk::MINUS => Some(token::MINUS),
        tk::SLASH => Some(token::SLASH),
        tk::PERCENT => Some(token::PERCENT),
        tk::CONCAT => Some(token::CONCAT),
        _ => None,
    }
}

/// Fast keyword lookup on raw bytes. Max keyword length is 10.
fn keyword_lookup(bytes: &[u8]) -> Option<i32> {
    if bytes.len() > 10 {
        return None;
    }
    let mut buf = [0u8; 10];
    for (i, &b) in bytes.iter().enumerate() {
        buf[i] = b.to_ascii_uppercase();
    }
    let upper = &buf[..bytes.len()];

    match upper {
        b"AS" => Some(token::AS),
        b"BY" => Some(token::BY),
        b"IN" => Some(token::IN),
        b"IS" => Some(token::IS),
        b"ON" => Some(token::ON),
        b"OR" => Some(token::OR),
        b"ALL" => Some(token::ALL),
        b"AND" => Some(token::AND),
        b"ASC" => Some(token::ASC),
        b"END" => Some(token::END),
        b"NOT" => Some(token::NOT),
        b"CASE" => Some(token::CASE),
        b"CAST" => Some(token::CAST),
        b"DESC" => Some(token::DESC),
        b"ELSE" => Some(token::ELSE),
        b"FROM" => Some(token::FROM),
        b"FULL" => Some(token::FULL),
        b"JOIN" => Some(token::JOIN),
        b"LAST" => Some(token::LAST),
        b"LEFT" => Some(token::LEFT),
        b"LIKE" => Some(token::LIKE),
        b"NULL" => Some(token::NULL_KW),
        b"OVER" => Some(token::OVER),
        b"THEN" => Some(token::THEN),
        b"TRUE" => Some(token::TRUE_KW),
        b"WHEN" => Some(token::WHEN),
        b"WITH" => Some(token::WITH),
        b"CROSS" => Some(token::CROSS),
        b"FALSE" => Some(token::FALSE_KW),
        b"FIRST" => Some(token::FIRST),
        b"GROUP" => Some(token::GROUP),
        b"ILIKE" => Some(token::ILIKE),
        b"INNER" => Some(token::INNER),
        b"LIMIT" => Some(token::LIMIT),
        b"NULLS" => Some(token::NULLS),
        b"ORDER" => Some(token::ORDER),
        b"OUTER" => Some(token::OUTER),
        b"RIGHT" => Some(token::RIGHT),
        b"UNION" => Some(token::UNION),
        b"WHERE" => Some(token::WHERE),
        b"ARRAY" => Some(token::ARRAY),
        b"EXCEPT" => Some(token::EXCEPT),
        b"EXISTS" => Some(token::EXISTS),
        b"EXTRACT" => Some(token::EXTRACT),
        b"UNNEST" => Some(token::UNNEST),
        b"USING" => Some(token::USING),
        b"ORDINALITY" => Some(token::ORDINALITY),
        // DATE and INTERVAL are NOT keywords — used as column names commonly.
        b"HAVING" => Some(token::HAVING),
        b"OFFSET" => Some(token::OFFSET),
        b"SELECT" => Some(token::SELECT),
        b"VALUES" => Some(token::VALUES),
        b"BETWEEN" => Some(token::BETWEEN),
        b"DISTINCT" => Some(token::DISTINCT),
        b"INTERSECT" => Some(token::INTERSECT),
        b"PARTITION" => Some(token::PARTITION),
        b"RECURSIVE" => Some(token::RECURSIVE),
        b"FILTER" => Some(token::FILTER),
        b"LATERAL" => Some(token::LATERAL),
        b"ANY" => Some(token::ANY),
        b"SOME" => Some(token::SOME),
        _ => None,
    }
}

/// Strip surrounding quotes from a byte slice.
fn strip_quotes(text: &[u8]) -> String {
    if text.len() < 2 {
        return String::from_utf8_lossy(text).into_owned();
    }
    let quote = text[0];
    if (quote == b'\'' || quote == b'"' || quote == b'`') && text[text.len() - 1] == quote {
        let inner = &text[1..text.len() - 1];
        let s = String::from_utf8_lossy(inner);
        if quote == b'\'' {
            s.replace("''", "'")
        } else if quote == b'"' {
            s.replace("\"\"", "\"")
        } else {
            s.into_owned()
        }
    } else {
        String::from_utf8_lossy(text).into_owned()
    }
}

/// Compute the byte offset of a raw C token within the buffer.
///
/// Returns 0 if the token start pointer is null.
fn token_offset(raw: &lime_sys::Token, buf_base: *const u8) -> usize {
    if raw.start.is_null() {
        return 0;
    }
    // SAFETY: raw.start and buf_base both point into the same
    // allocation (the SIMD-padded buffer created by the caller).
    let signed = unsafe { raw.start.cast::<u8>().offset_from(buf_base) };
    usize::try_from(signed).unwrap_or(0)
}

/// Build a `LexToken` for a text-bearing token (ident, string).
fn build_text_lex(code: i32, offset: i32, length: i32, text: &[u8]) -> Result<LexToken, String> {
    let stripped = strip_quotes(text);
    let cstr = CString::new(stripped).map_err(|e| format!("text token at offset {offset}: {e}"))?;
    let ptr = cstr.as_ptr();
    Ok(LexToken {
        code,
        value: RaToken {
            text: ptr,
            location: offset,
            length,
            int_val: 0,
            float_val: 0.0,
        },
        text_backing: Some(cstr),
    })
}

/// Build a simple (non-text) `LexToken`.
fn simple_lex(code: i32, offset: i32, length: i32) -> LexToken {
    LexToken {
        code,
        value: RaToken {
            text: std::ptr::null(),
            location: offset,
            length,
            int_val: 0,
            float_val: 0.0,
        },
        text_backing: None,
    }
}

/// Process one raw C token into a `LexToken`.
fn process_token(
    sql_bytes: &[u8],
    raw: &lime_sys::Token,
    buf_base: *const u8,
) -> Result<Option<LexToken>, String> {
    let c_code = raw.type_;
    let offset = token_offset(raw, buf_base);
    let len = raw.length;
    let offset_i32 = i32::try_from(offset).unwrap_or(0);
    let length_i32 = i32::try_from(len).unwrap_or(0);
    let text_bytes = &sql_bytes[offset..offset + len];

    // Identifiers: check keyword table in Rust.
    if c_code == tk::IDENTIFIER {
        if let Some(kw) = keyword_lookup(text_bytes) {
            return Ok(Some(simple_lex(kw, offset_i32, length_i32)));
        }
        let cstr = CString::new(String::from_utf8_lossy(text_bytes).as_ref())
            .map_err(|e| format!("ident at {offset}: {e}"))?;
        let ptr = cstr.as_ptr();
        return Ok(Some(LexToken {
            code: token::IDENT,
            value: RaToken {
                text: ptr,
                location: offset_i32,
                length: length_i32,
                int_val: 0,
                float_val: 0.0,
            },
            text_backing: Some(cstr),
        }));
    }

    let Some(code) = map_c_code(c_code) else {
        // For characters the C tokenizer doesn't know about, or ILLEGAL:
        // return an error to trigger fallback to the Rust lexer.
        // This handles `[`, `]`, `@`, `?` and other extended-SQL characters.
        let ch = String::from_utf8_lossy(text_bytes);
        return Err(format!(
            "unrecognized token '{ch}' at line {}:{}",
            raw.line, raw.column,
        ));
    };

    if code == token::IDENT || code == token::SCONST {
        Ok(Some(build_text_lex(code, offset_i32, length_i32, text_bytes)?))
    } else if code == token::ICONST {
        let text = std::str::from_utf8(text_bytes).map_err(|e| format!("int at {offset}: {e}"))?;
        let ival: i64 = text
            .parse()
            .map_err(|e| format!("int '{text}' at {offset}: {e}"))?;
        Ok(Some(LexToken {
            code: token::ICONST,
            value: RaToken {
                text: std::ptr::null(),
                location: offset_i32,
                length: length_i32,
                int_val: ival,
                float_val: 0.0,
            },
            text_backing: None,
        }))
    } else if code == token::FCONST {
        let text =
            std::str::from_utf8(text_bytes).map_err(|e| format!("float at {offset}: {e}"))?;
        let fval: f64 = text
            .parse()
            .map_err(|e| format!("float '{text}' at {offset}: {e}"))?;
        Ok(Some(LexToken {
            code: token::FCONST,
            value: RaToken {
                text: std::ptr::null(),
                location: offset_i32,
                length: length_i32,
                int_val: 0,
                float_val: fval,
            },
            text_backing: None,
        }))
    } else {
        Ok(Some(simple_lex(code, offset_i32, length_i32)))
    }
}

/// Tokenize SQL using the SIMD-accelerated lime C tokenizer.
///
/// Calls `lime_sys` FFI directly with no `TokenTable` (NULL),
/// so the C tokenizer classifies everything as identifiers or
/// punctuation. Keyword lookup is done in Rust.
///
/// # Errors
///
/// Returns an error string on illegal characters or allocation
/// failure.
pub fn tokenize_simd(sql: &str) -> Result<Vec<LexToken>, String> {
    if sql.is_empty() {
        return Ok(Vec::new());
    }

    let input_len = sql.len();
    let mut buf = Vec::with_capacity(input_len + SIMD_PAD);
    buf.extend_from_slice(sql.as_bytes());
    buf.resize(input_len + SIMD_PAD, 0);

    // SAFETY: buf has >= 32 bytes of zero-padding. NULL table is
    // safe (identifier-only mode).
    let tok =
        unsafe { lime_sys::tokenizer_create(std::ptr::null_mut(), buf.as_ptr().cast(), input_len) };
    if tok.is_null() {
        return Err("tokenizer allocation failed".to_owned());
    }

    let mut tokens = Vec::new();
    let buf_base = buf.as_ptr();
    let sql_bytes = sql.as_bytes();

    loop {
        let mut raw = MaybeUninit::<lime_sys::Token>::uninit();
        // SAFETY: tok is valid, raw is a valid out-param.
        let has_token = unsafe { lime_sys::tokenizer_next(tok, raw.as_mut_ptr()) };
        if !has_token {
            break;
        }
        // SAFETY: tokenizer_next returned true.
        let raw = unsafe { raw.assume_init() };

        match process_token(sql_bytes, &raw, buf_base) {
            Ok(Some(lex)) => tokens.push(lex),
            Ok(None) => {}
            Err(e) => {
                // SAFETY: tok is valid.
                unsafe { lime_sys::tokenizer_destroy(tok) };
                return Err(e);
            }
        }
    }

    // SAFETY: tok is a valid tokenizer.
    unsafe { lime_sys::tokenizer_destroy(tok) };

    Ok(tokens)
}

#[cfg(test)]
#[expect(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn simd_tokenize_simple_select() {
        let tokens = tokenize_simd("SELECT * FROM users").expect("should tokenize");
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].code, token::SELECT);
        assert_eq!(tokens[1].code, token::STAR);
        assert_eq!(tokens[2].code, token::FROM);
        assert_eq!(tokens[3].code, token::IDENT);
    }

    #[test]
    fn simd_tokenize_integer() {
        let tokens = tokenize_simd("42").expect("should tokenize");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].code, token::ICONST);
        assert_eq!(tokens[0].value.int_val, 42);
    }

    #[test]
    fn simd_tokenize_float() {
        let tokens = tokenize_simd("1.25").expect("should tokenize");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].code, token::FCONST);
        assert!((tokens[0].value.float_val - 1.25).abs() < f64::EPSILON);
    }

    #[test]
    fn simd_tokenize_string() {
        let tokens = tokenize_simd("'hello'").expect("should tokenize");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].code, token::SCONST);
        assert!(tokens[0].text_backing.is_some());
        let backing = tokens[0].text_backing.as_ref().expect("backing");
        assert_eq!(backing.to_str().expect("valid utf8"), "hello");
    }

    #[test]
    fn simd_tokenize_escaped_string() {
        let tokens = tokenize_simd("'it''s'").expect("should tokenize");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].code, token::SCONST);
        let backing = tokens[0].text_backing.as_ref().expect("backing");
        assert_eq!(backing.to_str().expect("valid utf8"), "it's");
    }

    #[test]
    fn simd_tokenize_operators() {
        let tokens = tokenize_simd("<> <= >= != ||").expect("should tokenize");
        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens[0].code, token::NE);
        assert_eq!(tokens[1].code, token::LE);
        assert_eq!(tokens[2].code, token::GE);
        assert_eq!(tokens[3].code, token::NE);
        assert_eq!(tokens[4].code, token::CONCAT);
    }

    #[test]
    fn simd_tokenize_case_insensitive() {
        let tokens = tokenize_simd("select FROM Where").expect("should tokenize");
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].code, token::SELECT);
        assert_eq!(tokens[1].code, token::FROM);
        assert_eq!(tokens[2].code, token::WHERE);
    }

    #[test]
    fn simd_tokenize_empty() {
        let tokens = tokenize_simd("").expect("should tokenize");
        assert!(tokens.is_empty());
    }

    #[test]
    fn simd_tokenize_comments() {
        let tokens = tokenize_simd("SELECT -- comment\n* FROM t").expect("should tokenize");
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].code, token::SELECT);
        assert_eq!(tokens[1].code, token::STAR);
    }

    #[test]
    fn simd_tokenize_complex() {
        let tokens = tokenize_simd(
            "SELECT id, name FROM users WHERE age > 21 \
             ORDER BY name ASC",
        )
        .expect("should tokenize");
        let codes: Vec<i32> = tokens.iter().map(|t| t.code).collect();
        assert_eq!(
            codes,
            vec![
                token::SELECT,
                token::IDENT, // id
                token::COMMA,
                token::IDENT, // name
                token::FROM,
                token::IDENT, // users
                token::WHERE,
                token::IDENT, // age
                token::GT,
                token::ICONST, // 21
                token::ORDER,
                token::BY,
                token::IDENT, // name
                token::ASC,
            ]
        );
    }

    #[test]
    fn simd_tokenize_quoted_identifier() {
        let tokens = tokenize_simd("\"my table\"").expect("should tokenize");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].code, token::IDENT);
        let backing = tokens[0].text_backing.as_ref().expect("backing");
        assert_eq!(backing.to_str().expect("valid utf8"), "my table");
    }

    #[test]
    fn simd_tokenize_all_keywords() {
        let tokens = tokenize_simd(
            "CASE WHEN THEN ELSE END CAST BETWEEN \
             LIKE ILIKE IN EXISTS WITH RECURSIVE \
             PARTITION OVER VALUES INTERSECT EXCEPT",
        )
        .expect("new keywords should tokenize");
        assert_eq!(tokens.len(), 18);
        assert_eq!(tokens[0].code, token::CASE);
        assert_eq!(tokens[1].code, token::WHEN);
        assert_eq!(tokens[2].code, token::THEN);
        assert_eq!(tokens[3].code, token::ELSE);
        assert_eq!(tokens[4].code, token::END);
        assert_eq!(tokens[5].code, token::CAST);
        assert_eq!(tokens[6].code, token::BETWEEN);
        assert_eq!(tokens[7].code, token::LIKE);
        assert_eq!(tokens[8].code, token::ILIKE);
        assert_eq!(tokens[9].code, token::IN);
        assert_eq!(tokens[10].code, token::EXISTS);
        assert_eq!(tokens[11].code, token::WITH);
        assert_eq!(tokens[12].code, token::RECURSIVE);
        assert_eq!(tokens[13].code, token::PARTITION);
        assert_eq!(tokens[14].code, token::OVER);
        assert_eq!(tokens[15].code, token::VALUES);
        assert_eq!(tokens[16].code, token::INTERSECT);
        assert_eq!(tokens[17].code, token::EXCEPT);
    }

    #[test]
    fn simd_tokenize_locations() {
        let tokens = tokenize_simd("SELECT id").expect("should tokenize");
        assert_eq!(tokens[0].value.location, 0);
        assert_eq!(tokens[1].value.location, 7);
    }

    #[test]
    fn simd_tokenize_whitespace_only() {
        let tokens = tokenize_simd("   \t\n  ").expect("should tokenize");
        assert!(tokens.is_empty());
    }

    #[test]
    fn simd_tokenize_block_comment() {
        let tokens = tokenize_simd("SELECT /* block */ * FROM t").expect("should tokenize");
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].code, token::SELECT);
        assert_eq!(tokens[1].code, token::STAR);
    }
}
