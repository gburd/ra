//! Hand-written recursive-descent parser for plan advice.
//!
//! This module is a clean-room reimplementation of the
//! `pgpa_scanner.l` + `pgpa_parser.y` pair from
//! `contrib/pg_plan_advice/`. The grammar is small and unambiguous,
//! so we don't need a generator — a single-pass tokeniser combined
//! with one mutually-recursive parser produces a typed AST in one
//! traversal of the input.
//!
//! The grammar this implements is reproduced here (matching
//! `pgpa_parser.y` lines 65-272):
//!
//! ```text
//! advice_item_list := advice_item*
//! advice_item      := JOIN_ORDER '(' join_order_target_list ')'
//!                  |  INDEX_TAG  '(' index_target_list ')'
//!                  |  SIMPLE_TAG '(' simple_target_list ')'
//!                  |  GENERIC_TAG '(' generic_target_list ')'
//!
//! join_order_target_list := (relation_identifier | join_order_sublist)*
//! join_order_sublist     := '(' join_order_target_list ')'
//!                        |  '{' simple_target_list '}'
//!
//! index_target_list      := (relation_identifier index_name)*
//! index_name             := identifier ( '.' identifier )?
//!
//! simple_target_list     := relation_identifier*
//! generic_target_list    := (relation_identifier | generic_sublist)*
//! generic_sublist        := '(' simple_target_list ')'
//!
//! relation_identifier    := identifier opt_occurrence opt_partition opt_plan
//! opt_occurrence         := '#' integer | <empty>
//! opt_partition          := '/' identifier ( '.' identifier )? | <empty>
//! opt_plan               := '@' identifier | <empty>
//! identifier             := simple_identifier | quoted_identifier
//! simple_identifier      := [A-Za-z_\u0080-\uFFFF][A-Za-z0-9_$\u0080-\uFFFF]*
//! quoted_identifier      := '"' ( '""' | [^"] )+ '"'
//! integer                := [0-9](_?[0-9])*
//! ```
//!
//! Whitespace and `/* ... */` C-style comments are skipped between
//! all tokens, matching `pgpa_scanner.l`. Identifier downcasing
//! applies to bare identifiers (matching `downcase_identifier(...,
//! false, false)` in PG); quoted identifiers preserve case.

use thiserror::Error;

use crate::ast::{
    Advice, AdviceItem, AdviceTag, AdviceTarget, AdviceTargetKind,
    IndexTarget, RelationIdentifier,
};

/// Parse failure with a position and message.
///
/// The phrasing of `message` matches PG's `pgpa_yyerror`
/// formatting where applicable (`"<problem> at or near \"<token>\""`
/// or `"<problem> at end of input"`).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("plan-advice parse error: {message}")]
pub struct ParseError {
    /// Byte offset into the input where the error was detected.
    pub position: usize,
    /// Human-readable message.
    pub message: String,
}

/// Top-level entry point.
///
/// Parses a plan-advice string and returns the AST. Whitespace
/// between tokens is ignored. `/* ... */` C-style comments are
/// also ignored. The input may be empty (returns `Ok(vec![])`).
///
/// # Errors
///
/// Returns [`ParseError`] when the input does not parse.
///
/// # Examples
///
/// ```
/// use ra_plan_advice::parse_advice;
///
/// let advice = parse_advice("SEQ_SCAN(t)").unwrap();
/// assert_eq!(advice.len(), 1);
/// ```
pub fn parse_advice(input: &str) -> Result<Advice, ParseError> {
    let mut p = Parser::new(input);
    let mut items = Vec::new();
    loop {
        p.skip_ws_or_err()?;
        if p.pos >= p.src.len() {
            break;
        }
        items.push(p.parse_item()?);
    }
    Ok(items)
}

// ────────────────────────────────────────────────────────────────
// Lexer-fused parser
// ────────────────────────────────────────────────────────────────

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    /// Current sublist nesting depth, guarded against unbounded
    /// recursion. The plan-advice string comes from an untrusted
    /// GUC (`ra_planner.plan_advice`), so a malicious value like
    /// `JOIN_ORDER(((((...)))))` with deep nesting could overflow
    /// the stack and crash the backend (a stack overflow is not a
    /// catchable panic). We cap nesting well below any legitimate
    /// use.
    depth: usize,
}

/// Maximum sublist nesting depth. Real advice nests at most a
/// couple of levels (`FOREIGN_JOIN((a b))`); 64 is far beyond any
/// legitimate input while keeping recursion safely shallow.
const MAX_NESTING_DEPTH: usize = 64;

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            src: input.as_bytes(),
            pos: 0,
            depth: 0,
        }
    }

    /// Enter a nested sublist, erroring if it would exceed
    /// [`MAX_NESTING_DEPTH`]. Pair every successful call with
    /// [`Self::exit_sublist`].
    fn enter_sublist(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_NESTING_DEPTH {
            return Err(self.error(format!(
                "plan advice nested too deeply (max {MAX_NESTING_DEPTH})"
            )));
        }
        Ok(())
    }

    fn exit_sublist(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Build a [`ParseError`] anchored at the current position.
    /// Mirrors `pgpa_yyerror`'s "<msg> at or near \"<token>\""
    /// formatting where possible.
    fn error(&self, message: impl Into<String>) -> ParseError {
        let mut msg: String = message.into();
        if self.pos < self.src.len() {
            // PG quotes the offending byte/token. We approximate
            // by quoting one ASCII byte; UTF-8 multi-byte
            // sequences are uncommon in advice strings.
            let ch = self.src[self.pos] as char;
            msg = format!("{msg} at or near \"{ch}\"");
        } else {
            msg = format!("{msg} at end of input");
        }
        ParseError { position: self.pos, message: msg }
    }

    /// Skip whitespace and `/* ... */` C-style comments. Returns
    /// true if there's more input afterwards. Mirrors
    /// `pgpa_scanner.l` whitespace + the `xc` exclusive state. An
    /// unterminated comment yields a parse error.
    fn skip_whitespace_and_comments(&mut self) -> bool {
        self.skip_ws_or_err().is_ok() && self.pos < self.src.len()
    }

    /// Same as [`Self::skip_whitespace_and_comments`] but surfaces
    /// unterminated comments. Used by the public entry point and
    /// the inter-token skip points where the position is required
    /// to be at the start of a token.
    fn skip_ws_or_err(&mut self) -> Result<(), ParseError> {
        loop {
            // Skip whitespace.
            while self.pos < self.src.len() {
                let c = self.src[self.pos];
                if matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0c | 0x0b) {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            // Look for `/* ... */`. PG does not support nested
            // comments here, matching the comment in
            // `pgpa_scanner.l`.
            if self.pos + 1 < self.src.len()
                && self.src[self.pos] == b'/'
                && self.src[self.pos + 1] == b'*'
            {
                let comment_start = self.pos;
                self.pos += 2;
                while self.pos + 1 < self.src.len()
                    && !(self.src[self.pos] == b'*' && self.src[self.pos + 1] == b'/')
                {
                    self.pos += 1;
                }
                if self.pos + 1 < self.src.len() {
                    self.pos += 2; // past `*/`
                    continue;
                }
                return Err(ParseError {
                    position: comment_start,
                    message: "unterminated comment".to_string(),
                });
            }
            return Ok(());
        }
    }

    /// Peek the next byte without advancing.
    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    /// Consume `b` if present; otherwise leave position alone.
    fn eat(&mut self, b: u8) -> bool {
        if self.peek() == Some(b) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Require the next byte to be `b`; advance past it. Build
    /// a parse error if not.
    fn expect(&mut self, b: u8) -> Result<(), ParseError> {
        if self.eat(b) {
            Ok(())
        } else {
            Err(self.error(format!(
                "syntax error: expected '{}'",
                b as char
            )))
        }
    }

    // -- Tokens -------------------------------------------------------

    /// Parse one identifier — either a simple identifier or a
    /// double-quoted identifier. Mirrors the `{identifier}` and
    /// `{xdstart}…{xdstop}` rules in `pgpa_scanner.l`.
    ///
    /// Bare identifiers are downcased on the way out (PG calls
    /// `downcase_identifier(yytext, yyleng, false, false)`);
    /// quoted identifiers preserve case.
    fn parse_identifier(&mut self) -> Result<String, ParseError> {
        if self.peek() == Some(b'"') {
            self.parse_quoted_identifier()
        } else {
            self.parse_bare_identifier()
        }
    }

    fn parse_bare_identifier(&mut self) -> Result<String, ParseError> {
        let start = self.pos;
        let Some(c) = self.peek() else {
            return Err(self.error("syntax error: expected identifier"));
        };
        if !is_ident_start(c) {
            return Err(self.error("syntax error: expected identifier"));
        }
        self.pos += 1;
        while let Some(c) = self.peek() {
            if is_ident_cont(c) {
                self.pos += 1;
            } else {
                break;
            }
        }
        // SAFETY: we only advance past bytes that pass
        // is_ident_{start,cont}, both of which include 0x80..=0xFF
        // unconditionally; multi-byte UTF-8 sequences therefore
        // pass through intact. ASCII bytes are valid UTF-8.
        let bytes = &self.src[start..self.pos];
        let s = std::str::from_utf8(bytes)
            .map_err(|_| self.error("identifier contains invalid UTF-8"))?;
        Ok(downcase_identifier(s))
    }

    /// Mirrors the `xd` exclusive state in `pgpa_scanner.l`. A
    /// quoted identifier may contain `""` to embed a literal
    /// double-quote; everything else between the surrounding
    /// quotes is taken verbatim.
    fn parse_quoted_identifier(&mut self) -> Result<String, ParseError> {
        debug_assert_eq!(self.peek(), Some(b'"'));
        self.pos += 1;
        let mut buf = String::new();
        loop {
            match self.peek() {
                None => return Err(self.error("unterminated quoted identifier")),
                Some(b'"') => {
                    // Either the close quote, or `""` -> literal `"`.
                    self.pos += 1;
                    if self.peek() == Some(b'"') {
                        buf.push('"');
                        self.pos += 1;
                    } else {
                        if buf.is_empty() {
                            return Err(self.error(
                                "zero-length delimited identifier",
                            ));
                        }
                        return Ok(buf);
                    }
                }
                Some(c) => {
                    buf.push(c as char);
                    self.pos += 1;
                }
            }
        }
    }

    /// Parse a positive integer: `[0-9](_?[0-9])*`. Mirrors PG's
    /// `decinteger` rule. Underscores between digits are
    /// permitted but ignored.
    fn parse_integer(&mut self) -> Result<u32, ParseError> {
        let start = self.pos;
        let Some(c) = self.peek() else {
            return Err(self.error("syntax error: expected integer"));
        };
        if !c.is_ascii_digit() {
            return Err(self.error("syntax error: expected integer"));
        }
        let mut buf = String::new();
        buf.push(c as char);
        self.pos += 1;
        loop {
            match self.peek() {
                Some(b'_') => {
                    self.pos += 1;
                    let Some(d) = self.peek() else {
                        return Err(self.error(
                            "integer literal cannot end in '_'",
                        ));
                    };
                    if !d.is_ascii_digit() {
                        return Err(self.error(
                            "integer literal cannot have '_' before non-digit",
                        ));
                    }
                    buf.push(d as char);
                    self.pos += 1;
                }
                Some(d) if d.is_ascii_digit() => {
                    buf.push(d as char);
                    self.pos += 1;
                }
                _ => break,
            }
        }
        buf.parse::<u32>().map_err(|_| {
            ParseError {
                position: start,
                message: "integer out of range".to_string(),
            }
        })
    }

    // -- Grammar productions -----------------------------------------

    /// `advice_item := tag '(' targets ')'`. Dispatches on tag
    /// flavor (`join_order` / `index` / `simple` / `generic`) the same
    /// way `pgpa_parser.y::advice_item` does.
    fn parse_item(&mut self) -> Result<AdviceItem, ParseError> {
        // Identifier first; then we resolve it to a tag.
        let raw = self.parse_identifier()?;
        let Some(tag) = AdviceTag::from_lowercase(&raw) else {
            return Err(ParseError {
                position: self.pos.saturating_sub(raw.len()),
                message: format!(
                    "unrecognized advice tag at or near \"{raw}\""
                ),
            });
        };

        self.skip_whitespace_and_comments();
        self.expect(b'(')?;
        let targets = if tag.is_join_order() {
            self.parse_join_order_target_list()?
        } else if tag.requires_index_argument() {
            self.parse_index_target_list()?
        } else if tag.is_simple() {
            self.parse_simple_target_list()?
        } else {
            self.parse_generic_target_list()?
        };
        self.skip_whitespace_and_comments();
        self.expect(b')')?;

        // Validation borrowed from pgpa_parser.y.
        if tag == AdviceTag::JoinOrder && targets.is_empty() {
            return Err(self.error(
                "JOIN_ORDER must have at least one target",
            ));
        }
        if tag == AdviceTag::ForeignJoin {
            for t in &targets {
                if matches!(t.kind, AdviceTargetKind::Identifier)
                    || t.children.len() <= 1
                {
                    return Err(self.error(
                        "FOREIGN_JOIN targets must contain more than one relation identifier",
                    ));
                }
            }
        }

        Ok(AdviceItem { tag, targets })
    }

    /// `simple_target_list := relation_identifier*` (possibly empty).
    fn parse_simple_target_list(&mut self) -> Result<Vec<AdviceTarget>, ParseError> {
        let mut out = Vec::new();
        while self.skip_whitespace_and_comments() && self.peek() != Some(b')') {
            // Closing brace for `{...}` is also a stop signal,
            // since simple_target_list is reused inside `{}` too.
            if self.peek() == Some(b'}') {
                break;
            }
            out.push(AdviceTarget::identifier(self.parse_relation_identifier()?));
        }
        Ok(out)
    }

    /// `generic_target_list := (relation_identifier | generic_sublist)*`.
    fn parse_generic_target_list(&mut self) -> Result<Vec<AdviceTarget>, ParseError> {
        let mut out = Vec::new();
        while self.skip_whitespace_and_comments() && self.peek() != Some(b')') {
            if self.peek() == Some(b'(') {
                self.pos += 1;
                self.enter_sublist()?;
                let children = self.parse_simple_target_list()?;
                self.exit_sublist();
                self.skip_whitespace_and_comments();
                self.expect(b')')?;
                out.push(AdviceTarget::ordered(children));
            } else {
                out.push(AdviceTarget::identifier(self.parse_relation_identifier()?));
            }
        }
        Ok(out)
    }

    /// `join_order_target_list := (relation_identifier | join_order_sublist)*`.
    fn parse_join_order_target_list(&mut self) -> Result<Vec<AdviceTarget>, ParseError> {
        let mut out = Vec::new();
        while self.skip_whitespace_and_comments() && self.peek() != Some(b')') {
            if self.peek() == Some(b'(') {
                self.pos += 1;
                self.enter_sublist()?;
                let children = self.parse_join_order_target_list()?;
                self.exit_sublist();
                self.skip_whitespace_and_comments();
                self.expect(b')')?;
                out.push(AdviceTarget::ordered(children));
            } else if self.peek() == Some(b'{') {
                self.pos += 1;
                let children = self.parse_simple_target_list()?;
                self.skip_whitespace_and_comments();
                self.expect(b'}')?;
                // Curly-brace sublists themselves cannot contain
                // sublists (per pgpa_parser.y::join_order_sublist).
                out.push(AdviceTarget::unordered(children));
            } else {
                out.push(AdviceTarget::identifier(self.parse_relation_identifier()?));
            }
        }
        Ok(out)
    }

    /// `index_target_list := (relation_identifier index_name)*`.
    fn parse_index_target_list(&mut self) -> Result<Vec<AdviceTarget>, ParseError> {
        let mut out = Vec::new();
        while self.skip_whitespace_and_comments() && self.peek() != Some(b')') {
            let rid = self.parse_relation_identifier()?;
            self.skip_whitespace_and_comments();
            let index = self.parse_index_name()?;
            let mut t = AdviceTarget::identifier(rid);
            t.index = Some(index);
            out.push(t);
        }
        Ok(out)
    }

    /// `index_name := identifier ('.' identifier)?`.
    fn parse_index_name(&mut self) -> Result<IndexTarget, ParseError> {
        let first = self.parse_identifier()?;
        // PG's flex tokenizer skips whitespace before each token,
        // so we must look past whitespace to see if there's a
        // schema-qualified `.` continuation.
        self.skip_ws_or_err()?;
        if self.eat(b'.') {
            self.skip_ws_or_err()?;
            let second = self.parse_identifier()?;
            Ok(IndexTarget {
                schema: Some(first),
                name: second,
            })
        } else {
            Ok(IndexTarget { schema: None, name: first })
        }
    }

    /// `relation_identifier := identifier opt_occurrence opt_partition opt_plan`.
    fn parse_relation_identifier(&mut self) -> Result<RelationIdentifier, ParseError> {
        let alias_name = self.parse_identifier()?;
        // PG's flex tokenizer skips whitespace and comments
        // between every token, so all four punctuation lookahead
        // points need to skip too.
        self.skip_ws_or_err()?;
        // opt_occurrence := '#' integer | empty
        let occurrence = if self.peek() == Some(b'#') {
            self.pos += 1;
            self.skip_ws_or_err()?;
            let n = self.parse_integer()?;
            if n == 0 {
                return Err(self.error(
                    "only positive occurrence numbers are permitted",
                ));
            }
            self.skip_ws_or_err()?;
            n
        } else {
            1
        };
        // opt_partition := '/' identifier ('.' identifier)? | empty
        let (partition_schema, partition_name) = if self.peek() == Some(b'/') {
            self.pos += 1;
            self.skip_ws_or_err()?;
            let first = self.parse_identifier()?;
            self.skip_ws_or_err()?;
            if self.eat(b'.') {
                self.skip_ws_or_err()?;
                let second = self.parse_identifier()?;
                self.skip_ws_or_err()?;
                (Some(first), Some(second))
            } else {
                (None, Some(first))
            }
        } else {
            (None, None)
        };
        // opt_plan := '@' identifier | empty
        let plan_name = if self.peek() == Some(b'@') {
            self.pos += 1;
            self.skip_ws_or_err()?;
            Some(self.parse_identifier()?)
        } else {
            None
        };
        Ok(RelationIdentifier {
            alias_name,
            occurrence,
            partition_schema,
            partition_name,
            plan_name,
        })
    }
}

// ────────────────────────────────────────────────────────────────
// Character classes
// ────────────────────────────────────────────────────────────────

/// PG's `ident_start` class: `[A-Za-z\200-\377_]`.
fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}

/// PG's `ident_cont` class: `[A-Za-z\200-\377_0-9\$]`.
fn is_ident_cont(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80
}

/// PG's `downcase_identifier(s, len, false /*warn*/, false /*truncate*/)`.
/// We don't truncate (matching the comment in `pgpa_scanner.l`) and we
/// don't emit warnings. Unicode bytes (>=0x80) pass through unchanged.
fn downcase_identifier(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_uppercase() { b + 32 } else { b } as char)
        .collect()
}
