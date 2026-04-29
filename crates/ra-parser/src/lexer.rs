//! Lightweight lexer for relational algebra notation.
//!
//! Tokenizes the algebra mini-language used in `.rra` code blocks
//! tagged with `algebra` or `ra`.  This is used for syntax
//! highlighting, validation, and future AST construction.

use thiserror::Error;

/// Errors produced during lexing.
#[derive(Debug, Error)]
#[error("lex error at byte {offset}: {message}")]
pub struct LexError {
    /// Byte offset in the input where the error occurred.
    pub offset: usize,
    /// Description of the problem.
    pub message: String,
}

/// Token kinds produced by the algebra lexer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// Greek letter operator: sigma, pi, rho, delta, gamma, etc.
    Operator(String),
    /// Identifier (relation name, attribute name).
    Ident(String),
    /// Arrow: `->` or `->`.
    Arrow,
    /// Left bracket `[`.
    LBracket,
    /// Right bracket `]`.
    RBracket,
    /// Left parenthesis `(`.
    LParen,
    /// Right parenthesis `)`.
    RParen,
    /// Join symbol: `join`, `JOIN`.
    Join,
    /// Cross product: `cross`, `x`.
    Cross,
    /// Union: `union`, `UNION`.
    Union,
    /// Intersection: `intersect`, `INTERSECT`.
    Intersect,
    /// Difference: `minus`, `-`, `\`.
    Difference,
    /// `where` keyword.
    Where,
    /// `subset` or `SUBSETEQ` keyword.
    Subset,
    /// Comma.
    Comma,
    /// Dot.
    Dot,
    /// Whitespace run (preserved for round-tripping).
    Whitespace(String),
    /// Anything we don't recognize.
    Unknown(char),
}

/// A single token with its position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// What kind of token this is.
    pub kind: TokenKind,
    /// Byte offset in the source.
    pub offset: usize,
    /// Length of the token in bytes.
    pub len: usize,
}

/// Known operator keywords (ASCII spellings of Greek letters).
const OPERATORS: &[&str] = &["sigma", "pi", "rho", "delta", "gamma", "tau", "attrs"];

/// Known join/set keywords.
const KEYWORDS: &[(&str, TokenKind)] = &[
    ("join", TokenKind::Join),
    ("cross", TokenKind::Cross),
    ("union", TokenKind::Union),
    ("intersect", TokenKind::Intersect),
    ("minus", TokenKind::Difference),
    ("where", TokenKind::Where),
    ("subset", TokenKind::Subset),
];

type CharIter<'a> = std::iter::Peekable<std::str::CharIndices<'a>>;

/// Lex the given algebra notation into a sequence of tokens.
///
/// # Errors
///
/// Returns [`LexError`] if the input contains an unrecoverable
/// lexing problem (currently all input is accepted with unknown
/// characters emitted as [`TokenKind::Unknown`]).
pub fn lex(source: &str) -> Result<Vec<Token>, LexError> {
    let mut tokens = Vec::new();
    let mut chars = source.char_indices().peekable();

    while let Some(&(offset, ch)) = chars.peek() {
        let tok = lex_one(source, &mut chars, offset, ch);
        tokens.push(tok);
    }

    Ok(tokens)
}

fn lex_one(source: &str, chars: &mut CharIter<'_>, offset: usize, ch: char) -> Token {
    match ch {
        c if c.is_whitespace() => lex_whitespace(source, chars),
        '[' | ']' | '(' | ')' | ',' | '.' => lex_punct(chars, ch, offset),
        '-' => lex_arrow_or_unknown(chars, offset),
        c if is_unicode_symbol(c) => lex_unicode(chars, ch, offset),
        c if c.is_alphabetic() || c == '_' => lex_word(chars),
        _ => {
            chars.next();
            Token {
                kind: TokenKind::Unknown(ch),
                offset,
                len: ch.len_utf8(),
            }
        }
    }
}

fn lex_whitespace(source: &str, chars: &mut CharIter<'_>) -> Token {
    let start = chars.peek().map_or(source.len(), |&(o, _)| o);
    let mut ws = String::new();
    while let Some(&(_, c)) = chars.peek() {
        if c.is_whitespace() {
            ws.push(c);
            chars.next();
        } else {
            break;
        }
    }
    let end = chars.peek().map_or(source.len(), |&(o, _)| o);
    Token {
        kind: TokenKind::Whitespace(ws),
        offset: start,
        len: end - start,
    }
}

fn lex_punct(chars: &mut CharIter<'_>, ch: char, offset: usize) -> Token {
    chars.next();
    let kind = match ch {
        '[' => TokenKind::LBracket,
        ']' => TokenKind::RBracket,
        '(' => TokenKind::LParen,
        ')' => TokenKind::RParen,
        ',' => TokenKind::Comma,
        '.' => TokenKind::Dot,
        // Only called with the chars above.
        _ => TokenKind::Unknown(ch),
    };
    Token {
        kind,
        offset,
        len: 1,
    }
}

fn lex_arrow_or_unknown(chars: &mut CharIter<'_>, offset: usize) -> Token {
    chars.next();
    if let Some(&(_, '>')) = chars.peek() {
        chars.next();
        Token {
            kind: TokenKind::Arrow,
            offset,
            len: 2,
        }
    } else {
        Token {
            kind: TokenKind::Unknown('-'),
            offset,
            len: 1,
        }
    }
}

fn is_unicode_symbol(ch: char) -> bool {
    matches!(
        ch,
        '\u{2192}' // ->
        | '\u{22C8}' // JOIN
        | '\u{00D7}' // x
        | '\u{222A}' // UNION
        | '\u{2229}' // INTERSECT
        | '\u{2212}' // -
        | '\u{2286}' // SUBSETEQ
        | '\u{03C3}' // sigma
        | '\u{03C0}' // pi
        | '\u{03C1}' // rho
        | '\u{03B3}' // gamma
    )
}

fn lex_unicode(chars: &mut CharIter<'_>, ch: char, offset: usize) -> Token {
    chars.next();
    let kind = match ch {
        '\u{2192}' => TokenKind::Arrow,
        '\u{22C8}' => TokenKind::Join,
        '\u{00D7}' => TokenKind::Cross,
        '\u{222A}' => TokenKind::Union,
        '\u{2229}' => TokenKind::Intersect,
        '\u{2212}' => TokenKind::Difference,
        '\u{2286}' => TokenKind::Subset,
        '\u{03C3}' => TokenKind::Operator("sigma".to_owned()),
        '\u{03C0}' => TokenKind::Operator("pi".to_owned()),
        '\u{03C1}' => TokenKind::Operator("rho".to_owned()),
        '\u{03B3}' => TokenKind::Operator("gamma".to_owned()),
        _ => TokenKind::Unknown(ch),
    };
    Token {
        kind,
        offset,
        len: ch.len_utf8(),
    }
}

fn lex_word(chars: &mut CharIter<'_>) -> Token {
    let start = chars.peek().map_or(0, |&(o, _)| o);
    let mut word = String::new();
    while let Some(&(_, c)) = chars.peek() {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
            chars.next();
        } else {
            break;
        }
    }
    let kind = classify_word(&word);
    Token {
        kind,
        offset: start,
        len: word.len(),
    }
}

fn classify_word(word: &str) -> TokenKind {
    let lower = word.to_lowercase();
    if OPERATORS.contains(&lower.as_str()) {
        return TokenKind::Operator(lower);
    }
    for &(kw, ref kind) in KEYWORDS {
        if lower == kw {
            return kind.clone();
        }
    }
    TokenKind::Ident(word.to_owned())
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "Test code appropriately uses expect for deterministic lexing"
)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<TokenKind> {
        lex(source)
            .expect("lex should succeed")
            .into_iter()
            .filter(|t| !matches!(t.kind, TokenKind::Whitespace(_)))
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn lex_simple_expression() {
        let tokens = kinds("sigma[p](R)");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Operator("sigma".to_owned()),
                TokenKind::LBracket,
                TokenKind::Ident("p".to_owned()),
                TokenKind::RBracket,
                TokenKind::LParen,
                TokenKind::Ident("R".to_owned()),
                TokenKind::RParen,
            ]
        );
    }

    #[test]
    fn lex_arrow() {
        let tokens = kinds("A -> B");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("A".to_owned()),
                TokenKind::Arrow,
                TokenKind::Ident("B".to_owned()),
            ]
        );
    }

    #[test]
    fn lex_unicode_arrow() {
        let tokens = kinds("A \u{2192} B");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("A".to_owned()),
                TokenKind::Arrow,
                TokenKind::Ident("B".to_owned()),
            ]
        );
    }

    #[test]
    fn lex_join_keyword() {
        let tokens = kinds("R join[c] S");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("R".to_owned()),
                TokenKind::Join,
                TokenKind::LBracket,
                TokenKind::Ident("c".to_owned()),
                TokenKind::RBracket,
                TokenKind::Ident("S".to_owned()),
            ]
        );
    }

    #[test]
    fn lex_unicode_join() {
        let tokens = kinds("R \u{22C8} S");
        assert!(tokens.contains(&TokenKind::Join));
    }

    #[test]
    fn lex_unicode_sigma() {
        let tokens = kinds("\u{03C3}[p](R)");
        assert_eq!(tokens[0], TokenKind::Operator("sigma".to_owned()));
    }

    #[test]
    fn lex_where_subset() {
        let tokens = kinds("where attrs(p) subset attrs(R)");
        assert!(tokens.contains(&TokenKind::Where));
        assert!(tokens.contains(&TokenKind::Subset));
    }

    #[test]
    fn lex_empty_input() {
        let tokens = lex("").expect("should lex empty");
        assert!(tokens.is_empty());
    }

    #[test]
    fn lex_unknown_chars() {
        let tokens = kinds("@#");
        assert_eq!(tokens.len(), 2);
        assert!(matches!(tokens[0], TokenKind::Unknown('@')));
        assert!(matches!(tokens[1], TokenKind::Unknown('#')));
    }

    #[test]
    fn lex_set_operations() {
        let tokens = kinds("R union S minus T intersect U");
        assert!(tokens.contains(&TokenKind::Union));
        assert!(tokens.contains(&TokenKind::Difference));
        assert!(tokens.contains(&TokenKind::Intersect));
    }
}
