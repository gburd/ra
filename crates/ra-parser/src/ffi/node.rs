//! Tagged pointer and arena-based parse state for FFI callbacks.
//!
//! The Lime parser calls back into Rust via `extern "C"` builder functions.
//! Each callback receives `*mut RaParseState` and returns `*mut RaNode`.
//!
//! `RaNode` is a zero-sized opaque type; the pointer itself encodes both the
//! arena index and the node kind via tag bits in the low 3 bits:
//!
//! ```text
//! | 63 ..................... 3 | 2  1  0 |
//! |       arena index          |   tag   |
//! ```

use std::fmt;

use ra_core::algebra::{
    AggregateExpr, GraphPatternElement, MergeWhen, RelExpr, SortKey, WindowExpr,
};
use ra_core::expr::Expr;

/// A structured syntax error captured from the Lime parser.
///
/// Carries precise position, token length, and expected-token hints
/// from the LALR parser state, enabling exact caret widths and
/// helpful "expected one of ..." messages.
#[derive(Debug, Clone)]
pub struct StructuredParseError {
    /// Byte offset in the source string where the rejected token starts.
    pub position: usize,
    /// Length of the rejected token in bytes.
    pub token_length: usize,
    /// The raw text of the rejected token, if available.
    pub token_text: Option<String>,
    /// The grammar name of the rejected token (e.g. "IDENT", "FROM").
    pub token_name: String,
    /// Human-readable error message.
    pub message: String,
    /// Grammar names of tokens that would have been valid at this point.
    pub expected_tokens: Vec<String>,
}

impl fmt::Display for StructuredParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if !self.expected_tokens.is_empty() {
            write!(
                f,
                " (expected one of: {})",
                self.expected_tokens.join(", ")
            )?;
        }
        Ok(())
    }
}

/// Error type returned by `RaParseState::take_result()`.
///
/// Distinguishes structured syntax errors (from `%syntax_error`) from
/// plain string errors (from builder/semantic actions).
#[derive(Debug)]
pub enum ParseErrors {
    /// Rich syntax errors with position, token, and expected-token info.
    Structured(Vec<StructuredParseError>),
    /// Plain string errors from builder functions or fallback paths.
    Strings(Vec<String>),
}

impl fmt::Display for ParseErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Structured(errs) => {
                for (i, err) in errs.iter().enumerate() {
                    if i > 0 {
                        f.write_str("; ")?;
                    }
                    write!(f, "{err}")?;
                }
                Ok(())
            }
            Self::Strings(errs) => f.write_str(&errs.join("; ")),
        }
    }
}

impl ParseErrors {
    /// Return the string error messages (for builder/semantic errors).
    ///
    /// Returns `None` if this is a `Structured` variant.
    #[must_use]
    pub fn as_strings(&self) -> Option<&[String]> {
        match self {
            Self::Strings(ss) => Some(ss),
            Self::Structured(_) => None,
        }
    }

    /// Return the structured parse errors.
    ///
    /// Returns `None` if this is a `Strings` variant.
    #[must_use]
    pub fn as_structured(&self) -> Option<&[StructuredParseError]> {
        match self {
            Self::Structured(se) => Some(se),
            Self::Strings(_) => None,
        }
    }
}

/// Opaque node handle returned to the Lime parser. Never dereferenced.
pub enum RaNode {}

/// Tag bits indicating which arena a pointer indexes into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NodeTag {
    /// A relational expression node.
    Rel = 0b000,
    /// A scalar expression node.
    Expr = 0b001,
    /// A list of arena indices.
    List = 0b010,
    /// A sort key node.
    SortKey = 0b011,
    /// An aggregate expression node.
    Agg = 0b100,
    /// A window expression node.
    Window = 0b101,
    /// A `MERGE` `WHEN` clause node.
    MergeWhen = 0b110,
    /// A `GRAPH_TABLE` `MATCH` pattern element node.
    GraphElem = 0b111,
}

/// Number of bits used for the tag.
const TAG_BITS: usize = 3;
/// Mask to extract the tag from a tagged pointer.
const TAG_MASK: usize = (1 << TAG_BITS) - 1;

/// Encode an arena index and tag into an opaque `*mut RaNode`.
///
/// Indices are offset by 1 so that `(index=0, tag=Rel)` does not produce a
/// null pointer (which would be indistinguishable from an error).
///
/// # Safety
/// The returned pointer is never dereferenced; it is purely a numeric encoding.
#[must_use]
pub fn encode(index: usize, tag: NodeTag) -> *mut RaNode {
    let tagged = ((index + 1) << TAG_BITS) | (tag as usize);
    tagged as *mut RaNode
}

/// Encode a relational expression arena index.
#[must_use]
pub fn encode_rel(index: usize) -> *mut RaNode {
    encode(index, NodeTag::Rel)
}

/// Encode a scalar expression arena index.
#[must_use]
pub fn encode_expr(index: usize) -> *mut RaNode {
    encode(index, NodeTag::Expr)
}

/// Encode a list arena index.
#[must_use]
pub fn encode_list(index: usize) -> *mut RaNode {
    encode(index, NodeTag::List)
}

/// Encode a sort key arena index.
#[must_use]
pub fn encode_sort_key(index: usize) -> *mut RaNode {
    encode(index, NodeTag::SortKey)
}

/// Encode an aggregate expression arena index.
#[must_use]
pub fn encode_agg(index: usize) -> *mut RaNode {
    encode(index, NodeTag::Agg)
}

/// Encode a window expression arena index.
#[must_use]
pub fn encode_window(index: usize) -> *mut RaNode {
    encode(index, NodeTag::Window)
}

/// Encode a MERGE WHEN-clause arena index.
#[must_use]
pub fn encode_merge_when(index: usize) -> *mut RaNode {
    encode(index, NodeTag::MergeWhen)
}

/// Encode a `GRAPH_TABLE` pattern-element arena index.
#[must_use]
pub fn encode_graph_elem(index: usize) -> *mut RaNode {
    encode(index, NodeTag::GraphElem)
}

/// Decode a tagged pointer into its tag and arena index.
///
/// Returns `None` if the pointer is null.
#[must_use]
pub fn decode(ptr: *mut RaNode) -> Option<(NodeTag, usize)> {
    if ptr.is_null() {
        return None;
    }
    let bits = ptr as usize;
    let tag_raw = bits & TAG_MASK;
    let offset_index = bits >> TAG_BITS;
    // Offset-by-1 encoding: a zero shifted value means invalid
    if offset_index == 0 {
        return None;
    }
    let index = offset_index - 1;
    let tag = match tag_raw {
        0b000 => NodeTag::Rel,
        0b001 => NodeTag::Expr,
        0b010 => NodeTag::List,
        0b011 => NodeTag::SortKey,
        0b100 => NodeTag::Agg,
        0b101 => NodeTag::Window,
        0b110 => NodeTag::MergeWhen,
        0b111 => NodeTag::GraphElem,
        _ => return None,
    };
    Some((tag, index))
}

/// Arena-based parse state shared across all FFI builder callbacks.
///
/// Each `push_*` method appends to the appropriate arena and returns a
/// tagged pointer encoding the arena index. The Lime parser passes these
/// opaque pointers back to subsequent builder calls, which decode them to
/// retrieve the stored AST nodes.
pub struct RaParseState {
    /// Arena for relational expression nodes.
    rel_nodes: Vec<RelExpr>,
    /// Arena for scalar expression nodes.
    expr_nodes: Vec<Expr>,
    /// Arena for sort key nodes.
    sort_keys: Vec<SortKey>,
    /// Arena for generic lists (stores indices into other arenas).
    lists: Vec<Vec<usize>>,
    /// Arena for aggregate expression nodes.
    agg_exprs: Vec<AggregateExpr>,
    /// Arena for window expression nodes.
    window_exprs: Vec<WindowExpr>,
    /// Arena for MERGE WHEN-clause nodes.
    merge_whens: Vec<MergeWhen>,
    /// Arena for `GRAPH_TABLE` pattern-element nodes.
    graph_elems: Vec<GraphPatternElement>,
    /// Output-column aliases for SELECT-list items, keyed by expr arena
    /// index (`SELECT expr AS alias`). Consulted by `ra_project`.
    expr_aliases: std::collections::HashMap<usize, String>,
    /// Accumulated parse errors (from builder/semantic actions).
    errors: Vec<String>,
    /// Structured syntax errors (from `%syntax_error` hook).
    structured_errors: Vec<StructuredParseError>,
}

impl RaParseState {
    /// Create a new empty parse state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rel_nodes: Vec::new(),
            expr_nodes: Vec::new(),
            sort_keys: Vec::new(),
            lists: Vec::new(),
            agg_exprs: Vec::new(),
            window_exprs: Vec::new(),
            merge_whens: Vec::new(),
            graph_elems: Vec::new(),
            expr_aliases: std::collections::HashMap::new(),
            errors: Vec::new(),
            structured_errors: Vec::new(),
        }
    }

    /// Push a relational expression and return its tagged pointer.
    pub fn push_rel(&mut self, expr: RelExpr) -> *mut RaNode {
        let index = self.rel_nodes.len();
        self.rel_nodes.push(expr);
        encode_rel(index)
    }

    /// Push a scalar expression and return its tagged pointer.
    pub fn push_expr(&mut self, expr: Expr) -> *mut RaNode {
        let index = self.expr_nodes.len();
        self.expr_nodes.push(expr);
        encode_expr(index)
    }

    /// Push a sort key and return its tagged pointer.
    pub fn push_sort_key(&mut self, key: SortKey) -> *mut RaNode {
        let index = self.sort_keys.len();
        self.sort_keys.push(key);
        encode_sort_key(index)
    }

    /// Create a new empty list and return its tagged pointer.
    pub fn push_list(&mut self) -> *mut RaNode {
        let index = self.lists.len();
        self.lists.push(Vec::new());
        encode_list(index)
    }

    /// Push an aggregate expression and return its tagged pointer.
    pub fn push_agg_expr(&mut self, agg: AggregateExpr) -> *mut RaNode {
        let index = self.agg_exprs.len();
        self.agg_exprs.push(agg);
        encode_agg(index)
    }

    /// Push a window expression and return its tagged pointer.
    pub fn push_window_expr(&mut self, win: WindowExpr) -> *mut RaNode {
        let index = self.window_exprs.len();
        self.window_exprs.push(win);
        encode_window(index)
    }

    /// Push a MERGE WHEN-clause and return its tagged pointer.
    pub fn push_merge_when(&mut self, when: MergeWhen) -> *mut RaNode {
        let index = self.merge_whens.len();
        self.merge_whens.push(when);
        encode_merge_when(index)
    }

    /// Take (clone) a MERGE WHEN-clause by arena index.
    #[must_use]
    pub fn take_merge_when(&self, index: usize) -> Option<MergeWhen> {
        self.merge_whens.get(index).cloned()
    }

    /// Push a `GRAPH_TABLE` pattern element and return its tagged pointer.
    pub fn push_graph_elem(&mut self, elem: GraphPatternElement) -> *mut RaNode {
        let index = self.graph_elems.len();
        self.graph_elems.push(elem);
        encode_graph_elem(index)
    }

    /// Take (clone) a `GRAPH_TABLE` pattern element by arena index.
    #[must_use]
    pub fn take_graph_elem(&self, index: usize) -> Option<GraphPatternElement> {
        self.graph_elems.get(index).cloned()
    }

    /// Append an item index to an existing list.
    ///
    /// Returns `false` if `list_idx` is out of bounds.
    pub fn list_push(&mut self, list_idx: usize, item_idx: usize) -> bool {
        if let Some(list) = self.lists.get_mut(list_idx) {
            list.push(item_idx);
            true
        } else {
            false
        }
    }

    /// Record a parse error (from builder/semantic actions).
    pub fn push_error(&mut self, msg: String) {
        self.errors.push(msg);
    }

    /// Record a structured syntax error (from `%syntax_error` hook).
    pub fn push_structured_error(&mut self, err: StructuredParseError) {
        self.structured_errors.push(err);
    }

    /// Return the accumulated structured syntax errors.
    #[must_use]
    pub fn structured_errors(&self) -> &[StructuredParseError] {
        &self.structured_errors
    }

    /// Retrieve a relational expression by arena index.
    #[must_use]
    pub fn get_rel(&self, index: usize) -> Option<&RelExpr> {
        self.rel_nodes.get(index)
    }

    /// Take (clone) a relational expression by arena index.
    #[must_use]
    pub fn take_rel(&self, index: usize) -> Option<RelExpr> {
        self.rel_nodes.get(index).cloned()
    }

    /// Retrieve a scalar expression by arena index.
    #[must_use]
    pub fn get_expr(&self, index: usize) -> Option<&Expr> {
        self.expr_nodes.get(index)
    }

    /// Take (clone) a scalar expression by arena index.
    #[must_use]
    pub fn take_expr(&self, index: usize) -> Option<Expr> {
        self.expr_nodes.get(index).cloned()
    }

    /// Record an output-column alias for the expr at `index`.
    pub fn set_expr_alias(&mut self, index: usize, alias: String) {
        self.expr_aliases.insert(index, alias);
    }

    /// Output-column alias previously recorded for the expr at `index`.
    #[must_use]
    pub fn expr_alias(&self, index: usize) -> Option<String> {
        self.expr_aliases.get(&index).cloned()
    }

    /// Retrieve a sort key by arena index.
    #[must_use]
    pub fn get_sort_key(&self, index: usize) -> Option<&SortKey> {
        self.sort_keys.get(index)
    }

    /// Take (clone) a sort key by arena index.
    #[must_use]
    pub fn take_sort_key(&self, index: usize) -> Option<SortKey> {
        self.sort_keys.get(index).cloned()
    }

    /// Retrieve a list by arena index.
    #[must_use]
    pub fn get_list(&self, index: usize) -> Option<&[usize]> {
        self.lists.get(index).map(Vec::as_slice)
    }

    /// Take (clone) an aggregate expression by arena index.
    #[must_use]
    pub fn take_agg_expr(&self, index: usize) -> Option<AggregateExpr> {
        self.agg_exprs.get(index).cloned()
    }

    /// Take (clone) a window expression by arena index.
    #[must_use]
    pub fn take_window_expr(&self, index: usize) -> Option<WindowExpr> {
        self.window_exprs.get(index).cloned()
    }

    /// Consume the parse state and return the final result.
    ///
    /// On success (no errors and at least one rel node), returns the last
    /// relational expression pushed — which is the root of the parse tree.
    /// On failure, returns structured errors if available, otherwise
    /// the accumulated string error messages.
    ///
    /// # Errors
    /// Returns `ParseResult::Err` with structured or string errors if any
    /// errors were recorded or if no relational expression was pushed.
    pub fn take_result(self) -> Result<RelExpr, ParseErrors> {
        if !self.structured_errors.is_empty() {
            return Err(ParseErrors::Structured(self.structured_errors));
        }
        if !self.errors.is_empty() {
            return Err(ParseErrors::Strings(self.errors));
        }
        self.rel_nodes
            .into_iter()
            .last()
            .ok_or_else(|| {
                ParseErrors::Strings(
                    vec!["no relational expression produced".to_owned()],
                )
            })
    }
}

impl Default for RaParseState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test code uses expect for unwrapping")]
mod tests {
    use super::*;
    use ra_core::algebra::{AggregateFunction, NullOrdering, SortDirection, WindowFunction};
    use ra_core::expr::{ColumnRef, Const};

    #[test]
    fn encode_decode_roundtrip_rel() {
        let ptr = encode_rel(42);
        let (tag, idx) = decode(ptr).expect("should decode");
        assert_eq!(tag, NodeTag::Rel);
        assert_eq!(idx, 42);
    }

    #[test]
    fn encode_decode_roundtrip_expr() {
        let ptr = encode_expr(7);
        let (tag, idx) = decode(ptr).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        assert_eq!(idx, 7);
    }

    #[test]
    fn encode_decode_roundtrip_list() {
        let ptr = encode_list(100);
        let (tag, idx) = decode(ptr).expect("should decode");
        assert_eq!(tag, NodeTag::List);
        assert_eq!(idx, 100);
    }

    #[test]
    fn encode_decode_roundtrip_sort_key() {
        let ptr = encode_sort_key(3);
        let (tag, idx) = decode(ptr).expect("should decode");
        assert_eq!(tag, NodeTag::SortKey);
        assert_eq!(idx, 3);
    }

    #[test]
    fn encode_decode_roundtrip_agg() {
        let ptr = encode_agg(5);
        let (tag, idx) = decode(ptr).expect("should decode");
        assert_eq!(tag, NodeTag::Agg);
        assert_eq!(idx, 5);
    }

    #[test]
    fn encode_decode_roundtrip_window() {
        let ptr = encode_window(9);
        let (tag, idx) = decode(ptr).expect("should decode");
        assert_eq!(tag, NodeTag::Window);
        assert_eq!(idx, 9);
    }

    #[test]
    fn decode_null_returns_none() {
        assert!(decode(std::ptr::null_mut()).is_none());
    }

    #[test]
    fn parse_state_push_and_take_rel() {
        let mut state = RaParseState::new();
        let ptr = state.push_rel(RelExpr::scan("users"));
        let (tag, idx) = decode(ptr).expect("should decode");
        assert_eq!(tag, NodeTag::Rel);
        let rel = state.take_rel(idx).expect("should exist");
        assert!(matches!(rel, RelExpr::Scan { table, .. } if table == "users"));
    }

    #[test]
    fn parse_state_push_and_take_expr() {
        let mut state = RaParseState::new();
        let ptr = state.push_expr(Expr::Const(Const::Int(42)));
        let (tag, idx) = decode(ptr).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let expr = state.take_expr(idx).expect("should exist");
        assert_eq!(expr, Expr::Const(Const::Int(42)));
    }

    #[test]
    fn parse_state_list_operations() {
        let mut state = RaParseState::new();
        let list_ptr = state.push_list();
        let (tag, list_idx) = decode(list_ptr).expect("should decode");
        assert_eq!(tag, NodeTag::List);

        assert!(state.list_push(list_idx, 10));
        assert!(state.list_push(list_idx, 20));

        let items = state.get_list(list_idx).expect("should exist");
        assert_eq!(items, &[10, 20]);
    }

    #[test]
    fn parse_state_list_push_out_of_bounds() {
        let mut state = RaParseState::new();
        assert!(!state.list_push(999, 0));
    }

    #[test]
    fn parse_state_take_result_success() {
        let mut state = RaParseState::new();
        state.push_rel(RelExpr::scan("t1"));
        state.push_rel(RelExpr::scan("t2"));
        let result = state.take_result();
        assert!(result.is_ok());
        let rel = result.expect("should be ok");
        assert!(matches!(rel, RelExpr::Scan { table, .. } if table == "t2"));
    }

    #[test]
    fn parse_state_take_result_with_errors() {
        let mut state = RaParseState::new();
        state.push_rel(RelExpr::scan("t"));
        state.push_error("syntax error".to_owned());
        let result = state.take_result();
        assert!(result.is_err());
        let errs = result.expect_err("should be err");
        let strings = errs.as_strings().expect("should be string errors");
        assert_eq!(strings, &["syntax error"]);
    }

    #[test]
    fn parse_state_take_result_empty() {
        let state = RaParseState::new();
        let result = state.take_result();
        assert!(result.is_err());
    }

    #[test]
    fn parse_state_sort_key() {
        let mut state = RaParseState::new();
        let key = SortKey {
            expr: Expr::Column(ColumnRef::new("id")),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        };
        let ptr = state.push_sort_key(key.clone());
        let (tag, idx) = decode(ptr).expect("should decode");
        assert_eq!(tag, NodeTag::SortKey);
        let retrieved = state.take_sort_key(idx).expect("should exist");
        assert_eq!(retrieved, key);
    }

    #[test]
    fn parse_state_push_and_take_agg_expr() {
        let mut state = RaParseState::new();
        let agg = AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(Expr::Column(ColumnRef::new("amount"))),
            distinct: false,
            alias: None,
        };
        let ptr = state.push_agg_expr(agg.clone());
        let (tag, idx) = decode(ptr).expect("should decode");
        assert_eq!(tag, NodeTag::Agg);
        let retrieved = state.take_agg_expr(idx).expect("should exist");
        assert_eq!(retrieved, agg);
    }

    #[test]
    fn parse_state_push_and_take_window_expr() {
        let mut state = RaParseState::new();
        let win = WindowExpr {
            function: WindowFunction::RowNumber,
            arg: None,
            partition_by: vec![],
            order_by: vec![],
            frame: None,
            alias: Some("rn".to_owned()),
        };
        let ptr = state.push_window_expr(win.clone());
        let (tag, idx) = decode(ptr).expect("should decode");
        assert_eq!(tag, NodeTag::Window);
        let retrieved = state.take_window_expr(idx).expect("should exist");
        assert_eq!(retrieved, win);
    }
}
