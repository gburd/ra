use std::marker::PhantomData;
use std::mem::MaybeUninit;

use crate::token_table::TokenTable;
use crate::Error;

/// Extra zero bytes appended past the input for SIMD tokenizer safety.
const SIMD_PAD: usize = 32;

/// A SQL token produced by the tokenizer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Classified token type.
    pub kind: TokenKind,
    /// The token text, copied from the source buffer.
    pub text: String,
    /// 1-based line number in the source.
    pub line: u32,
    /// 1-based column number in the source.
    pub column: u32,
}

impl Token {
    /// Convert the raw C `Token` fields into a safe Rust `Token`.
    ///
    /// # Safety
    ///
    /// `raw.start` must be a valid pointer into a live buffer with at
    /// least `raw.length` readable bytes.
    unsafe fn from_raw(raw: &lime_sys::Token) -> Self {
        let text = if raw.start.is_null() || raw.length == 0 {
            String::new()
        } else {
            // SAFETY: caller guarantees start..start+length is readable.
            let slice = unsafe { std::slice::from_raw_parts(raw.start.cast::<u8>(), raw.length) };
            String::from_utf8_lossy(slice).into_owned()
        };

        Self {
            kind: TokenKind::from_raw(raw.type_),
            text,
            line: raw.line,
            column: raw.column,
        }
    }

    /// The raw integer token code, suitable for passing to
    /// `ParseSession::feed_token`.
    #[must_use]
    pub fn raw_code(&self) -> i32 {
        self.kind.to_raw()
    }
}

/// Classification of a token produced by the Lime tokenizer.
///
/// Negative codes are generic (punctuation, literals, identifiers).
/// Positive codes are grammar-specific keywords looked up via the
/// `TokenTable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// End of input (`TK_EOF = 0`).
    Eof,
    /// An unrecognized identifier.
    Identifier,
    /// Integer literal.
    Integer,
    /// Floating-point literal.
    Float,
    /// Single-quoted string literal.
    StringLiteral,
    /// Hex blob literal (`X'...'`).
    Blob,
    /// `(`.
    LParen,
    /// `)`.
    RParen,
    /// `;`.
    Semicolon,
    /// `,`.
    Comma,
    /// `.`.
    Dot,
    /// `*`.
    Star,
    /// `+`.
    Plus,
    /// `-`.
    Minus,
    /// `/`.
    Slash,
    /// `%`.
    Percent,
    /// `=` or `==`.
    Eq,
    /// `!=` or `<>`.
    Ne,
    /// `<`.
    Lt,
    /// `>`.
    Gt,
    /// `<=`.
    Le,
    /// `>=`.
    Ge,
    /// `&`.
    BitAnd,
    /// `|`.
    BitOr,
    /// `~`.
    BitNot,
    /// `<<`.
    LShift,
    /// `>>`.
    RShift,
    /// `||`.
    Concat,
    /// Double-quoted identifier (`"..."`).
    DoubleQuotedId,
    /// Backtick identifier (`` `...` ``).
    BacktickId,
    /// Bracket identifier (`[...]`).
    BracketId,
    /// Unicode escape string (`U&'...'`).
    UnicodeString,
    /// Unrecognized character.
    Illegal,
    /// A grammar-defined keyword token (positive code from `TokenTable`).
    Keyword(i32),
    /// An unrecognized negative token code not in the known set.
    Unknown(i32),
}

impl TokenKind {
    /// Convert a raw C token code to a `TokenKind`.
    #[must_use]
    pub fn from_raw(code: i32) -> Self {
        match code {
            0 => Self::Eof,
            -1 => Self::Identifier,
            -2 => Self::Integer,
            -3 => Self::Float,
            -4 => Self::StringLiteral,
            -5 => Self::Blob,
            -6 => Self::LParen,
            -7 => Self::RParen,
            -8 => Self::Semicolon,
            -9 => Self::Comma,
            -10 => Self::Dot,
            -11 => Self::Star,
            -12 => Self::Plus,
            -13 => Self::Minus,
            -14 => Self::Slash,
            -15 => Self::Percent,
            -16 => Self::Eq,
            -17 => Self::Ne,
            -18 => Self::Lt,
            -19 => Self::Gt,
            -20 => Self::Le,
            -21 => Self::Ge,
            -22 => Self::BitAnd,
            -23 => Self::BitOr,
            -24 => Self::BitNot,
            -25 => Self::LShift,
            -26 => Self::RShift,
            -27 => Self::Concat,
            -28 => Self::DoubleQuotedId,
            -29 => Self::BacktickId,
            -30 => Self::BracketId,
            -31 => Self::UnicodeString,
            -32 => Self::Illegal,
            c if c > 0 => Self::Keyword(c),
            c => Self::Unknown(c),
        }
    }

    /// Convert back to the raw C token code.
    #[must_use]
    pub fn to_raw(self) -> i32 {
        match self {
            Self::Eof => 0,
            Self::Identifier => -1,
            Self::Integer => -2,
            Self::Float => -3,
            Self::StringLiteral => -4,
            Self::Blob => -5,
            Self::LParen => -6,
            Self::RParen => -7,
            Self::Semicolon => -8,
            Self::Comma => -9,
            Self::Dot => -10,
            Self::Star => -11,
            Self::Plus => -12,
            Self::Minus => -13,
            Self::Slash => -14,
            Self::Percent => -15,
            Self::Eq => -16,
            Self::Ne => -17,
            Self::Lt => -18,
            Self::Gt => -19,
            Self::Le => -20,
            Self::Ge => -21,
            Self::BitAnd => -22,
            Self::BitOr => -23,
            Self::BitNot => -24,
            Self::LShift => -25,
            Self::RShift => -26,
            Self::Concat => -27,
            Self::DoubleQuotedId => -28,
            Self::BacktickId => -29,
            Self::BracketId => -30,
            Self::UnicodeString => -31,
            Self::Illegal => -32,
            Self::Keyword(c) | Self::Unknown(c) => c,
        }
    }
}

/// A zero-copy token that references the original input buffer.
///
/// Unlike `Token`, this type does not allocate a `String` for the
/// token text. Instead it stores the byte offset and length into
/// the input buffer passed to the `Tokenizer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawToken {
    /// Classified token type.
    pub kind: TokenKind,
    /// Byte offset of the token start in the input buffer.
    pub offset: usize,
    /// Length of the token in bytes.
    pub length: usize,
    /// 1-based line number in the source.
    pub line: u32,
    /// 1-based column number in the source.
    pub column: u32,
}

impl RawToken {
    /// Convert the raw C `Token` fields into a `RawToken` by
    /// computing the byte offset from the buffer base pointer.
    ///
    /// # Safety
    ///
    /// `raw.start` must be a valid pointer into `buffer_base`, and
    /// `buffer_base` must be the start of the tokenizer's buffer.
    unsafe fn from_raw(raw: &lime_sys::Token, buffer_base: *const u8) -> Self {
        let offset = if raw.start.is_null() {
            0
        } else {
            // SAFETY: both pointers are into the same buffer.
            unsafe { raw.start.cast::<u8>().offset_from(buffer_base) as usize }
        };
        Self {
            kind: TokenKind::from_raw(raw.type_),
            offset,
            length: raw.length,
            line: raw.line,
            column: raw.column,
        }
    }

    /// The raw integer token code.
    #[must_use]
    pub fn raw_code(&self) -> i32 {
        self.kind.to_raw()
    }
}

/// A SIMD-accelerated SQL tokenizer.
///
/// The tokenizer splits SQL input into tokens, using the `TokenTable` to
/// classify keywords. The input buffer is copied internally with 32 bytes
/// of zero-padding for SIMD safety.
///
/// The lifetime `'t` ties the tokenizer to its `TokenTable`, which must
/// remain alive for the duration of tokenization.
pub struct Tokenizer<'t> {
    inner: *mut lime_sys::Tokenizer,
    /// Owned padded copy of the input buffer.
    buffer: Vec<u8>,
    /// Ensures the `TokenTable` outlives this tokenizer.
    _table: PhantomData<&'t TokenTable>,
}

impl<'t> Tokenizer<'t> {
    /// Create a new tokenizer for the given SQL input.
    ///
    /// `table` provides keyword lookup. Pass `None` for identifier-only
    /// mode (all identifiers, no keyword classification).
    ///
    /// The input is copied into a zero-padded buffer internally; the
    /// original string need not remain alive.
    ///
    /// # Errors
    ///
    /// Returns `Error::NullPointer` if the C tokenizer allocation fails.
    pub fn new(table: Option<&'t TokenTable>, input: &str) -> Result<Self, Error> {
        let mut buffer = Vec::with_capacity(input.len() + SIMD_PAD);
        buffer.extend_from_slice(input.as_bytes());
        buffer.resize(input.len() + SIMD_PAD, 0);

        let table_ptr = table.map_or(std::ptr::null_mut(), TokenTable::as_ptr);

        // SAFETY: table_ptr is either null or a valid TokenTable pointer.
        // buffer is a valid byte slice with at least 32 bytes of
        // zero-padding past the input content.
        let inner =
            unsafe { lime_sys::tokenizer_create(table_ptr, buffer.as_ptr().cast(), input.len()) };

        if inner.is_null() {
            return Err(Error::NullPointer {
                function: "tokenizer_create",
            });
        }

        Ok(Self {
            inner,
            buffer,
            _table: PhantomData,
        })
    }

    /// Get the next token as a zero-copy `RawToken`, or `None` at
    /// end-of-input.
    ///
    /// This avoids the `String` allocation of `next_token` by
    /// returning byte offsets into the internal buffer instead.
    pub fn next_raw_token(&mut self) -> Option<RawToken> {
        let mut raw = MaybeUninit::<lime_sys::Token>::uninit();
        // SAFETY: inner is a valid tokenizer. raw is a valid out-param.
        let has_token = unsafe { lime_sys::tokenizer_next(self.inner, raw.as_mut_ptr()) };
        if has_token {
            // SAFETY: tokenizer_next returned true, so raw is
            // initialized. raw.start points into buffer.
            Some(unsafe { RawToken::from_raw(&raw.assume_init(), self.buffer.as_ptr()) })
        } else {
            None
        }
    }

    /// Get the next token, or `None` at end-of-input.
    pub fn next_token(&mut self) -> Option<Token> {
        let mut raw = MaybeUninit::<lime_sys::Token>::uninit();
        // SAFETY: inner is a valid tokenizer. raw is a valid out-param.
        // tokenizer_next writes to raw before returning true.
        let has_token = unsafe { lime_sys::tokenizer_next(self.inner, raw.as_mut_ptr()) };
        if has_token {
            // SAFETY: tokenizer_next returned true, so raw is fully
            // initialized. raw.start points into our buffer which is
            // alive.
            Some(unsafe { Token::from_raw(&raw.assume_init()) })
        } else {
            None
        }
    }

    /// Peek at the next token without consuming it.
    ///
    /// Returns `None` at end-of-input.
    pub fn peek_token(&mut self) -> Option<Token> {
        let mut raw = MaybeUninit::<lime_sys::Token>::uninit();
        // SAFETY: same as next_token — inner is valid, raw is valid.
        let has_token = unsafe { lime_sys::tokenizer_peek(self.inner, raw.as_mut_ptr()) };
        if has_token {
            // SAFETY: tokenizer_peek returned true, raw is initialized.
            Some(unsafe { Token::from_raw(&raw.assume_init()) })
        } else {
            None
        }
    }

    /// Current byte offset in the input.
    #[must_use]
    pub fn position(&self) -> usize {
        // SAFETY: inner is a valid tokenizer pointer.
        unsafe { lime_sys::tokenizer_position(self.inner) }
    }

    /// Current 1-based line number.
    #[must_use]
    pub fn line(&self) -> u32 {
        // SAFETY: inner is a valid tokenizer pointer.
        unsafe { lime_sys::tokenizer_line(self.inner) }
    }

    /// Current 1-based column number.
    #[must_use]
    pub fn column(&self) -> u32 {
        // SAFETY: inner is a valid tokenizer pointer.
        unsafe { lime_sys::tokenizer_column(self.inner) }
    }
}

impl Iterator for Tokenizer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        self.next_token()
    }
}

impl Drop for Tokenizer<'_> {
    fn drop(&mut self) {
        // SAFETY: inner is a valid tokenizer pointer (or null, which
        // tokenizer_destroy handles safely).
        unsafe { lime_sys::tokenizer_destroy(self.inner) };
    }
}

impl std::fmt::Debug for Tokenizer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokenizer")
            .field("position", &self.position())
            .field("line", &self.line())
            .field("column", &self.column())
            .finish()
    }
}
