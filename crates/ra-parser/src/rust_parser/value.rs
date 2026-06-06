//! Semantic value type for the native-Rust Lime parser path.
//!
//! Lime's Rust output threads a single `Value` type through every grammar
//! symbol (there is no per-symbol union as on the C side). `Value` therefore
//! has to carry both:
//!
//! - **terminal payloads** — the token text/literal fields the lexer produces
//!   (`text`, `int_val`, `float_val`, `location`), and
//! - **non-terminal results** — a tagged arena index identical in meaning to
//!   the C path's `*mut RaNode` (see [`crate::ffi::node`]).
//!
//! Reduction actions read RHS values and the parse-state arenas
//! (`ctx.user: &mut RaParseState`) and write the LHS value. The builder layer
//! in [`crate::rust_parser::builders`] consumes/produces `Value`s, reusing the
//! exact arena + tagged-index logic the C builders use.

use std::rc::Rc;

use crate::ffi::node::{decode, encode, NodeTag, RaNode};

/// A single semantic value flowing through the Rust LALR parser.
///
/// Cheap to clone: the token text is reference-counted and the node handle is
/// a plain integer. Reductions clone RHS values out of the stack, so `Clone`
/// must stay inexpensive.
#[derive(Clone, Default)]
pub struct Value {
    /// Tagged arena index, encoded exactly like the C path's `*mut RaNode`
    /// (`(index + 1) << 3 | tag`). Zero means "no node" (a terminal, or an
    /// epsilon/passthrough that carries no AST node). Private: grammar actions
    /// reach it via builder calls; the translator maps the C idioms `X = 0`
    /// and `if (X)` to [`Value::default`] / [`Value::is_present`].
    node: usize,
    /// Token text for identifiers and literals; `None` for non-terminals.
    /// Private: read via [`Value::text`]; the C write idiom `A.text = 0` is
    /// dropped by the translator (a fresh `Value` already has no text).
    text: Option<Rc<str>>,
    /// Integer literal value (`ICONST`) or a small enum payload for
    /// `%type {RaToken}` non-terminals (join kind, NULLS ordering, …). Public
    /// so the C field idioms `A.int_val = N;` / `B.int_val` translate verbatim.
    pub int_val: i64,
    /// Float literal value (`FCONST`), also reused as a presence flag by the
    /// LIMIT/OFFSET encoding. Public for verbatim field-idiom translation.
    pub float_val: f64,
    /// Byte offset of the token, reused as the OFFSET slot by LIMIT. Public.
    pub location: i32,
    /// Byte length of the token text. Public.
    pub length: i32,
}

impl Value {
    /// Build a terminal value from the lexer's token fields.
    #[must_use]
    pub fn from_token(text: &str, int_val: i64, float_val: f64, location: i32) -> Self {
        Self {
            node: 0,
            text: Some(Rc::from(text)),
            int_val,
            float_val,
            location,
            length: i32::try_from(text.len()).unwrap_or(0),
        }
    }

    /// Wrap a tagged arena handle (as produced by `RaParseState::push_*`,
    /// re-encoded to `usize`) as a non-terminal value.
    #[must_use]
    pub fn from_node(handle: *mut RaNode) -> Self {
        Self {
            node: handle as usize,
            ..Self::default()
        }
    }

    /// The token text, or `""` for non-terminals / token-less values.
    #[must_use]
    pub fn text(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }

    /// Whether this value carries an AST node (non-null handle). Mirrors the C
    /// idiom `if (X) { ... }` used to test optional clauses for presence.
    #[must_use]
    pub fn is_present(&self) -> bool {
        self.node != 0
    }

    /// The tagged node handle for passing to a builder, as a raw `*mut RaNode`
    /// (the same numeric encoding the C path uses; never dereferenced).
    #[must_use]
    pub fn handle(&self) -> *mut RaNode {
        self.node as *mut RaNode
    }

    /// Decode this value's node into `(tag, arena_index)`, or `None` when it
    /// carries no node.
    #[must_use]
    pub fn decode(&self) -> Option<(NodeTag, usize)> {
        decode(self.handle())
    }
}

/// Convenience: wrap an arena index + tag as a non-terminal value.
#[must_use]
pub fn node_value(index: usize, tag: NodeTag) -> Value {
    Value::from_node(encode(index, tag))
}
