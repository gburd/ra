//! SQL lexer that produces token codes matching the Lime grammar.
//!
//! Tokenizes SQL input into `(token_code, RaToken)` pairs where the
//! token codes match the constants generated in `ra_sql.h`.

use std::ffi::CString;
use std::os::raw::c_char;

/// Token codes matching the generated `ra_sql.h` constants.
///
/// These must stay in sync with the `%token` declarations in
/// `grammar/ra_sql.lime`.
#[expect(missing_docs)]
pub mod token {
    pub const IDENT: i32 = 1;
    pub const ICONST: i32 = 2;
    pub const FCONST: i32 = 3;
    pub const SCONST: i32 = 4;
    pub const COMMA: i32 = 5;
    pub const LPAREN: i32 = 6;
    pub const RPAREN: i32 = 7;
    pub const DOT: i32 = 8;
    pub const SEMICOLON: i32 = 9;
    pub const STAR: i32 = 10;
    pub const EQ: i32 = 11;
    pub const NE: i32 = 12;
    pub const LT: i32 = 13;
    pub const LE: i32 = 14;
    pub const GT: i32 = 15;
    pub const GE: i32 = 16;
    pub const PLUS: i32 = 17;
    pub const MINUS: i32 = 18;
    pub const SLASH: i32 = 19;
    pub const PERCENT: i32 = 20;
    pub const CONCAT: i32 = 21;
    pub const SELECT: i32 = 22;
    pub const FROM: i32 = 23;
    pub const WHERE: i32 = 24;
    pub const JOIN: i32 = 25;
    pub const INNER: i32 = 26;
    pub const LEFT: i32 = 27;
    pub const RIGHT: i32 = 28;
    pub const FULL: i32 = 29;
    pub const OUTER: i32 = 30;
    pub const CROSS: i32 = 31;
    pub const ON: i32 = 32;
    pub const AND: i32 = 33;
    pub const OR: i32 = 34;
    pub const NOT: i32 = 35;
    pub const AS: i32 = 36;
    pub const GROUP: i32 = 37;
    pub const BY: i32 = 38;
    pub const HAVING: i32 = 39;
    pub const ORDER: i32 = 40;
    pub const ASC: i32 = 41;
    pub const DESC: i32 = 42;
    pub const NULLS: i32 = 43;
    pub const FIRST: i32 = 44;
    pub const LAST: i32 = 45;
    pub const LIMIT: i32 = 46;
    pub const OFFSET: i32 = 47;
    pub const UNION: i32 = 48;
    pub const ALL: i32 = 49;
    pub const ANY: i32 = 50;
    pub const SOME: i32 = 51;
    pub const DISTINCT: i32 = 52;
    pub const NULL_KW: i32 = 53;
    pub const IS: i32 = 54;
    pub const TRUE_KW: i32 = 55;
    pub const FALSE_KW: i32 = 56;

    // New keywords for expanded grammar
    pub const CASE: i32 = 57;
    pub const WHEN: i32 = 58;
    pub const THEN: i32 = 59;
    pub const ELSE: i32 = 60;
    pub const END: i32 = 61;
    pub const CAST: i32 = 62;
    pub const BETWEEN: i32 = 63;
    pub const LIKE: i32 = 64;
    pub const ILIKE: i32 = 65;
    pub const IN: i32 = 66;
    pub const EXISTS: i32 = 67;
    pub const WITH: i32 = 68;
    pub const RECURSIVE: i32 = 69;
    pub const PARTITION: i32 = 70;
    pub const OVER: i32 = 71;
    pub const VALUES: i32 = 72;
    pub const INTERSECT: i32 = 73;
    pub const EXCEPT: i32 = 74;
    pub const USING: i32 = 75;

    // JSONB operators (two-character symbol tokens)
    pub const AT_GT: i32 = 76; /* @>  JSON contains */
    pub const LT_AT: i32 = 77; /* <@  JSON contained by */
    pub const AT_QUESTION: i32 = 78; /* @?  JSON path exists */
    pub const AT_AT: i32 = 79; /* @@  JSON path match */

    // Square brackets for array subscripting
    pub const LBRACKET: i32 = 80; /* [ */
    pub const RBRACKET: i32 = 81; /* ] */

    // New keywords
    pub const ARRAY: i32 = 82;
    pub const UNNEST: i32 = 83;
    pub const ORDINALITY: i32 = 84;
    pub const DATE_KW: i32 = 85;
    pub const INTERVAL_KW: i32 = 86;
    pub const EXTRACT: i32 = 87;
    pub const PLACEHOLDER: i32 = 88; /* ? parameter */
    pub const COLONCOLON: i32 = 89; /* :: PostgreSQL type cast operator */
    pub const ARROW: i32 = 90; /* ->  JSON field access (returns JSON) */
    pub const ARROW_TEXT: i32 = 91; /* ->> JSON field text extraction */

    // JSONB operators (key-exists family + path extraction)
    pub const JSONB_EXISTS: i32 = 92; /* ?   JSONB key exists */
    pub const JSONB_TEXT_PATH: i32 = 93; /* #>> JSONB text path extraction */
    pub const JSONB_ANY_KEY: i32 = 94; /* ?|  JSONB any key exists */
    pub const JSONB_ALL_KEYS: i32 = 95; /* ?&  JSONB all keys exist */

    // Phase 4-5: FILTER and LATERAL keywords
    pub const FILTER: i32 = 96;
    pub const LATERAL: i32 = 97;

    // DML keywords
    pub const INSERT: i32 = 98;
    pub const INTO: i32 = 99;
    pub const UPDATE: i32 = 100;
    pub const SET: i32 = 101;
    pub const DELETE: i32 = 102;
    pub const RETURNING: i32 = 103;
    pub const CONFLICT: i32 = 104;
    pub const DO: i32 = 105;
    pub const NOTHING: i32 = 106;
    pub const DEFAULT: i32 = 107;

    // Parser gap fixes: NATURAL, WINDOW, WITHIN, MATERIALIZED, TILDE
    pub const NATURAL: i32 = 108;
    pub const WINDOW: i32 = 109;
    pub const WITHIN: i32 = 110;
    pub const MATERIALIZED: i32 = 111;
    pub const TILDE: i32 = 112;

    // SIMILAR TO operator
    pub const SIMILAR: i32 = 113;
}

/// C-compatible token value passed to the Lime parser.
///
/// This must match the `RaToken` typedef in `grammar/ra_sql.lime`.
#[repr(C)]
#[derive(Clone)]
pub struct RaToken {
    /// Pointer to NUL-terminated text (for identifiers/strings).
    /// Null when not applicable.
    pub text: *const c_char,
    /// Byte offset in the source string where this token starts.
    pub location: i32,
    /// Length of this token in bytes (for precise caret widths).
    pub length: i32,
    /// Integer value for `ICONST` tokens.
    pub int_val: i64,
    /// Float value for `FCONST` tokens.
    pub float_val: f64,
}

impl Default for RaToken {
    fn default() -> Self {
        Self {
            text: std::ptr::null(),
            location: 0,
            length: 0,
            int_val: 0,
            float_val: 0.0,
        }
    }
}

impl std::fmt::Debug for RaToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RaToken")
            .field("text", &self.text)
            .field("location", &self.location)
            .field("length", &self.length)
            .field("int_val", &self.int_val)
            .field("float_val", &self.float_val)
            .finish()
    }
}

/// A produced token with its code and value, plus any owned string data.
#[derive(Debug)]
pub struct LexToken {
    /// The token code (one of the `token::*` constants, or 0 for EOF).
    pub code: i32,
    /// The C-compatible token value.
    pub value: RaToken,
    /// Owned `CString` backing `value.text` (kept alive for the
    /// duration of parsing). `None` if no text is needed.
    pub text_backing: Option<CString>,
}

/// Create a simple token with no text backing.
fn simple_token(code: i32, location: usize, length: i32) -> LexToken {
    LexToken {
        code,
        value: RaToken {
            text: std::ptr::null(),
            location: i32::try_from(location).unwrap_or(0),
            length,
            int_val: 0,
            float_val: 0.0,
        },
        text_backing: None,
    }
}

/// Create a token backed by a C string.
fn text_token(code: i32, location: usize, length: i32, text: &str) -> Result<LexToken, String> {
    let cstr =
        CString::new(text).map_err(|e| format!("invalid text at position {location}: {e}"))?;
    let ptr = cstr.as_ptr();
    Ok(LexToken {
        code,
        value: RaToken {
            text: ptr,
            location: i32::try_from(location).unwrap_or(0),
            length,
            int_val: 0,
            float_val: 0.0,
        },
        text_backing: Some(cstr),
    })
}

/// Look up a keyword, returning its token code. Case-insensitive.
fn keyword_lookup(word: &str) -> Option<i32> {
    match word.to_ascii_uppercase().as_str() {
        "SELECT" => Some(token::SELECT),
        "FROM" => Some(token::FROM),
        "WHERE" => Some(token::WHERE),
        "JOIN" => Some(token::JOIN),
        "INNER" => Some(token::INNER),
        "LEFT" => Some(token::LEFT),
        "RIGHT" => Some(token::RIGHT),
        "FULL" => Some(token::FULL),
        "OUTER" => Some(token::OUTER),
        "CROSS" => Some(token::CROSS),
        "ON" => Some(token::ON),
        "AND" => Some(token::AND),
        "OR" => Some(token::OR),
        "NOT" => Some(token::NOT),
        "AS" => Some(token::AS),
        "GROUP" => Some(token::GROUP),
        "BY" => Some(token::BY),
        "HAVING" => Some(token::HAVING),
        "ORDER" => Some(token::ORDER),
        "ASC" => Some(token::ASC),
        "DESC" => Some(token::DESC),
        "NULLS" => Some(token::NULLS),
        "FIRST" => Some(token::FIRST),
        "LAST" => Some(token::LAST),
        "LIMIT" => Some(token::LIMIT),
        "OFFSET" => Some(token::OFFSET),
        "UNION" => Some(token::UNION),
        "ALL" => Some(token::ALL),
        "ANY" => Some(token::ANY),
        "SOME" => Some(token::SOME),
        "DISTINCT" => Some(token::DISTINCT),
        "NULL" => Some(token::NULL_KW),
        "IS" => Some(token::IS),
        "TRUE" => Some(token::TRUE_KW),
        "FALSE" => Some(token::FALSE_KW),
        "CASE" => Some(token::CASE),
        "WHEN" => Some(token::WHEN),
        "THEN" => Some(token::THEN),
        "ELSE" => Some(token::ELSE),
        "END" => Some(token::END),
        "CAST" => Some(token::CAST),
        "BETWEEN" => Some(token::BETWEEN),
        "LIKE" => Some(token::LIKE),
        "ILIKE" => Some(token::ILIKE),
        "IN" => Some(token::IN),
        "EXISTS" => Some(token::EXISTS),
        "WITH" => Some(token::WITH),
        "RECURSIVE" => Some(token::RECURSIVE),
        "PARTITION" => Some(token::PARTITION),
        "OVER" => Some(token::OVER),
        "VALUES" => Some(token::VALUES),
        "INTERSECT" => Some(token::INTERSECT),
        "EXCEPT" => Some(token::EXCEPT),
        "USING" => Some(token::USING),
        "ARRAY" => Some(token::ARRAY),
        "UNNEST" => Some(token::UNNEST),
        "ORDINALITY" => Some(token::ORDINALITY),
        "EXTRACT" => Some(token::EXTRACT),
        "FILTER" => Some(token::FILTER),
        "LATERAL" => Some(token::LATERAL),
        // DML keywords
        "INSERT" => Some(token::INSERT),
        "INTO" => Some(token::INTO),
        "UPDATE" => Some(token::UPDATE),
        "SET" => Some(token::SET),
        "DELETE" => Some(token::DELETE),
        "RETURNING" => Some(token::RETURNING),
        "CONFLICT" => Some(token::CONFLICT),
        "DO" => Some(token::DO),
        "NOTHING" => Some(token::NOTHING),
        "DEFAULT" => Some(token::DEFAULT),
        // SIMILAR TO operator
        "SIMILAR" => Some(token::SIMILAR),
        // Note: DATE, INTERVAL, and SUBSTRING are intentionally NOT keywords
        // — they are commonly used as column/function names. SUBSTRING is
        // handled via IDENT rules for FROM...FOR syntax.
        _ => None,
    }
}

/// Match a three-character operator starting at `pos`.
///
/// Must be checked BEFORE `match_two_char_op` to avoid ambiguity
/// (e.g. `->>` vs `->`).
fn match_three_char_op(sql: &str, pos: usize) -> Option<i32> {
    if pos + 2 >= sql.len() {
        return None;
    }
    match &sql[pos..pos + 3] {
        "->>" => Some(token::ARROW_TEXT),
        "#>>" => Some(token::JSONB_TEXT_PATH),
        _ => None,
    }
}

/// Match a two-character operator starting at `pos`.
fn match_two_char_op(sql: &str, pos: usize) -> Option<i32> {
    if pos + 1 >= sql.len() {
        return None;
    }
    match &sql[pos..pos + 2] {
        "<>" | "!=" => Some(token::NE),
        "<=" => Some(token::LE),
        ">=" => Some(token::GE),
        "||" => Some(token::CONCAT),
        "@>" => Some(token::AT_GT),
        "<@" => Some(token::LT_AT),
        "@?" => Some(token::AT_QUESTION),
        "@@" => Some(token::AT_AT),
        "::" => Some(token::COLONCOLON),
        "->" => Some(token::ARROW),
        "?|" => Some(token::JSONB_ANY_KEY),
        "?&" => Some(token::JSONB_ALL_KEYS),
        _ => None,
    }
}

/// Match a single-character operator or punctuation.
fn match_single_char_op(byte: u8) -> Option<i32> {
    match byte {
        b',' => Some(token::COMMA),
        b'(' => Some(token::LPAREN),
        b')' => Some(token::RPAREN),
        b'.' => Some(token::DOT),
        b';' => Some(token::SEMICOLON),
        b'*' => Some(token::STAR),
        b'=' => Some(token::EQ),
        b'<' => Some(token::LT),
        b'>' => Some(token::GT),
        b'+' => Some(token::PLUS),
        b'-' => Some(token::MINUS),
        b'/' => Some(token::SLASH),
        b'%' => Some(token::PERCENT),
        b'[' => Some(token::LBRACKET),
        b']' => Some(token::RBRACKET),
        b'?' => Some(token::PLACEHOLDER),
        b'~' => Some(token::TILDE),
        _ => None,
    }
}

/// Mutable cursor over SQL input bytes.
struct Cursor<'a> {
    sql: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(sql: &'a str) -> Self {
        Self {
            sql,
            bytes: sql.as_bytes(),
            pos: 0,
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn peek(&self) -> u8 {
        self.bytes[self.pos]
    }

    fn peek_next(&self) -> Option<u8> {
        self.bytes.get(self.pos + 1).copied()
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn advance_by(&mut self, n: usize) {
        self.pos += n;
    }

    fn slice(&self, start: usize, end: usize) -> &'a str {
        &self.sql[start..end]
    }
}

/// Skip whitespace and comments, returning `true` if anything was
/// skipped (the main loop should re-check the current position).
fn skip_trivia(cur: &mut Cursor<'_>) -> Result<bool, String> {
    if cur.at_end() {
        return Ok(false);
    }
    if cur.peek().is_ascii_whitespace() {
        cur.advance();
        return Ok(true);
    }
    if cur.peek() == b'-' && cur.peek_next() == Some(b'-') {
        cur.advance_by(2);
        while !cur.at_end() && cur.peek() != b'\n' {
            cur.advance();
        }
        return Ok(true);
    }
    if cur.peek() == b'/' && cur.peek_next() == Some(b'*') {
        cur.advance_by(2);
        let mut depth = 1u32;
        while cur.pos + 1 < cur.bytes.len() && depth > 0 {
            if cur.peek() == b'/' && cur.peek_next() == Some(b'*') {
                depth += 1;
                cur.advance_by(2);
            } else if cur.peek() == b'*' && cur.peek_next() == Some(b'/') {
                depth -= 1;
                cur.advance_by(2);
            } else {
                cur.advance();
            }
        }
        if depth > 0 {
            return Err(format!(
                "unterminated block comment at position {}",
                cur.pos
            ));
        }
        return Ok(true);
    }
    Ok(false)
}

/// Lex a single-quoted string literal.
fn lex_string(cur: &mut Cursor<'_>) -> Result<LexToken, String> {
    let start = cur.pos;
    cur.advance(); // skip opening quote
    let mut value = String::new();
    loop {
        if cur.at_end() {
            return Err(format!(
                "unterminated string literal starting at \
                 position {start}"
            ));
        }
        if cur.peek() == b'\'' {
            cur.advance();
            if !cur.at_end() && cur.peek() == b'\'' {
                value.push('\'');
                cur.advance();
                continue;
            }
            break;
        }
        value.push(char::from(cur.peek()));
        cur.advance();
    }
    let length = i32::try_from(cur.pos - start).unwrap_or(0);
    let cstr =
        CString::new(value).map_err(|e| format!("invalid string at position {start}: {e}"))?;
    let ptr = cstr.as_ptr();
    Ok(LexToken {
        code: token::SCONST,
        value: RaToken {
            text: ptr,
            location: i32::try_from(start).unwrap_or(0),
            length,
            int_val: 0,
            float_val: 0.0,
        },
        text_backing: Some(cstr),
    })
}

/// Lex a numeric literal (integer or float).
fn lex_number(cur: &mut Cursor<'_>) -> Result<LexToken, String> {
    let start = cur.pos;
    while !cur.at_end() && cur.peek().is_ascii_digit() {
        cur.advance();
    }
    let is_float =
        !cur.at_end() && cur.peek() == b'.' && cur.peek_next().is_some_and(|b| b.is_ascii_digit());
    if is_float {
        cur.advance(); // skip '.'
        while !cur.at_end() && cur.peek().is_ascii_digit() {
            cur.advance();
        }
        if !cur.at_end() && (cur.peek() == b'e' || cur.peek() == b'E') {
            cur.advance();
            if !cur.at_end() && (cur.peek() == b'+' || cur.peek() == b'-') {
                cur.advance();
            }
            while !cur.at_end() && cur.peek().is_ascii_digit() {
                cur.advance();
            }
        }
        let text = cur.slice(start, cur.pos);
        let length = i32::try_from(cur.pos - start).unwrap_or(0);
        let fval: f64 = text.parse().map_err(|e| {
            format!(
                "invalid float literal '{text}' at position \
                 {start}: {e}"
            )
        })?;
        Ok(LexToken {
            code: token::FCONST,
            value: RaToken {
                text: std::ptr::null(),
                location: i32::try_from(start).unwrap_or(0),
                length,
                int_val: 0,
                float_val: fval,
            },
            text_backing: None,
        })
    } else {
        let text = cur.slice(start, cur.pos);
        let length = i32::try_from(cur.pos - start).unwrap_or(0);
        let ival: i64 = text.parse().map_err(|e| {
            format!(
                "invalid integer literal '{text}' at position \
                 {start}: {e}"
            )
        })?;
        Ok(LexToken {
            code: token::ICONST,
            value: RaToken {
                text: std::ptr::null(),
                location: i32::try_from(start).unwrap_or(0),
                length,
                int_val: ival,
                float_val: 0.0,
            },
            text_backing: None,
        })
    }
}

/// Lex an identifier or keyword.
fn lex_word(cur: &mut Cursor<'_>) -> Result<LexToken, String> {
    let start = cur.pos;
    cur.advance();
    while !cur.at_end() && (cur.peek().is_ascii_alphanumeric() || cur.peek() == b'_') {
        cur.advance();
    }
    let length = i32::try_from(cur.pos - start).unwrap_or(0);
    let word = cur.slice(start, cur.pos);
    if let Some(kw_code) = keyword_lookup(word) {
        Ok(simple_token(kw_code, start, length))
    } else {
        text_token(token::IDENT, start, length, word)
    }
}

/// Lex a double-quoted identifier.
fn lex_quoted_ident(cur: &mut Cursor<'_>) -> Result<LexToken, String> {
    let start = cur.pos;
    cur.advance(); // skip opening '"'
    let content_start = cur.pos;
    while !cur.at_end() && cur.peek() != b'"' {
        cur.advance();
    }
    if cur.at_end() {
        return Err(format!(
            "unterminated quoted identifier starting at \
             position {start}"
        ));
    }
    let ident = cur.slice(content_start, cur.pos);
    cur.advance(); // skip closing '"'
    let length = i32::try_from(cur.pos - start).unwrap_or(0);
    text_token(token::IDENT, start, length, ident)
}

/// Tokenize a SQL string into a sequence of `LexToken` values.
///
/// The returned vector does NOT include an EOF token; the caller
/// should feed token code `0` after all tokens are consumed.
///
/// # Errors
///
/// Returns an error string if the input contains an unterminated
/// string literal or an unrecognized character.
pub fn tokenize(sql: &str) -> Result<Vec<LexToken>, String> {
    let mut cur = Cursor::new(sql);
    let mut tokens = Vec::new();

    while !cur.at_end() {
        if skip_trivia(&mut cur)? {
            continue;
        }

        let b = cur.peek();

        if b == b'\'' {
            tokens.push(lex_string(&mut cur)?);
        } else if b.is_ascii_digit() {
            tokens.push(lex_number(&mut cur)?);
        } else if b.is_ascii_alphabetic() || b == b'_' {
            tokens.push(lex_word(&mut cur)?);
        } else if b == b'"' {
            tokens.push(lex_quoted_ident(&mut cur)?);
        } else if let Some(code) = match_three_char_op(cur.sql, cur.pos) {
            tokens.push(simple_token(code, cur.pos, 3));
            cur.advance_by(3);
        } else if let Some(code) = match_two_char_op(cur.sql, cur.pos) {
            tokens.push(simple_token(code, cur.pos, 2));
            cur.advance_by(2);
        } else if let Some(code) = match_single_char_op(b) {
            tokens.push(simple_token(code, cur.pos, 1));
            cur.advance();
        } else {
            return Err(format!(
                "unexpected character '{}' at position {}",
                char::from(b),
                cur.pos
            ));
        }
    }

    Ok(tokens)
}

#[cfg(test)]
#[expect(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_simple_select() {
        let tokens = tokenize("SELECT * FROM users").expect("should tokenize");
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].code, token::SELECT);
        assert_eq!(tokens[1].code, token::STAR);
        assert_eq!(tokens[2].code, token::FROM);
        assert_eq!(tokens[3].code, token::IDENT);
    }

    #[test]
    fn tokenize_integer_literal() {
        let tokens = tokenize("42").expect("should tokenize");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].code, token::ICONST);
        assert_eq!(tokens[0].value.int_val, 42);
    }

    #[test]
    fn tokenize_float_literal() {
        let tokens = tokenize("1.25").expect("should tokenize");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].code, token::FCONST);
        assert!((tokens[0].value.float_val - 1.25).abs() < f64::EPSILON);
    }

    #[test]
    fn tokenize_string_literal() {
        let tokens = tokenize("'hello'").expect("should tokenize");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].code, token::SCONST);
        assert!(tokens[0].text_backing.is_some());
    }

    #[test]
    fn tokenize_escaped_string() {
        let tokens = tokenize("'it''s'").expect("should tokenize");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].code, token::SCONST);
        let backing = tokens[0]
            .text_backing
            .as_ref()
            .expect("should have backing");
        assert_eq!(backing.to_str().expect("valid utf8"), "it's");
    }

    #[test]
    fn tokenize_two_char_operators() {
        let tokens = tokenize("<> <= >= != ||").expect("should tokenize");
        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens[0].code, token::NE);
        assert_eq!(tokens[1].code, token::LE);
        assert_eq!(tokens[2].code, token::GE);
        assert_eq!(tokens[3].code, token::NE);
        assert_eq!(tokens[4].code, token::CONCAT);
    }

    #[test]
    fn tokenize_keywords_case_insensitive() {
        let tokens = tokenize("select FROM Where").expect("should tokenize");
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].code, token::SELECT);
        assert_eq!(tokens[1].code, token::FROM);
        assert_eq!(tokens[2].code, token::WHERE);
    }

    #[test]
    fn tokenize_unterminated_string() {
        let result = tokenize("'unterminated");
        assert!(result.is_err());
        let err = result.expect_err("should error");
        assert!(err.contains("unterminated string literal"));
    }

    #[test]
    fn tokenize_unexpected_character() {
        let result = tokenize("SELECT @");
        assert!(result.is_err());
        let err = result.expect_err("should error");
        assert!(err.contains("unexpected character"));
    }

    #[test]
    fn tokenize_empty_input() {
        let tokens = tokenize("").expect("should tokenize");
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_whitespace_only() {
        let tokens = tokenize("   \t\n  ").expect("should tokenize");
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_single_line_comment() {
        let tokens = tokenize("SELECT -- a comment\n* FROM t").expect("should tokenize");
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].code, token::SELECT);
        assert_eq!(tokens[1].code, token::STAR);
    }

    #[test]
    fn tokenize_block_comment() {
        let tokens = tokenize("SELECT /* block */ * FROM t").expect("should tokenize");
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].code, token::SELECT);
        assert_eq!(tokens[1].code, token::STAR);
    }

    #[test]
    fn tokenize_quoted_identifier() {
        let tokens = tokenize("\"my table\"").expect("should tokenize");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].code, token::IDENT);
        let backing = tokens[0]
            .text_backing
            .as_ref()
            .expect("should have backing");
        assert_eq!(backing.to_str().expect("valid utf8"), "my table");
    }

    #[test]
    fn tokenize_locations_tracked() {
        let tokens = tokenize("SELECT id").expect("should tokenize");
        assert_eq!(tokens[0].value.location, 0);
        assert_eq!(tokens[1].value.location, 7);
    }

    #[test]
    fn tokenize_complex_query() {
        let tokens = tokenize(
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
}
