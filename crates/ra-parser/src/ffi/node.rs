//! Tagged pointer and arena-based parse state for FFI callbacks.
//!
//! The Lime parser calls back into Rust via `extern "C"` builder functions.
//! Each callback receives `*mut RaParseState` and returns `*mut RaNode`.
//!
//! `RaNode` is a zero-sized opaque type; the pointer itself encodes both the
//! arena index and the node kind via tag bits in the low 2 bits:
//!
//! ```text
//! | 63 ..................... 2 | 1  0 |
//! |       arena index          | tag  |
//! ```

use ra_core::algebra::{RelExpr, SortKey};
use ra_core::expr::Expr;

/// Opaque node handle returned to the Lime parser. Never dereferenced.
pub enum RaNode {}

/// Tag bits indicating which arena a pointer indexes into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NodeTag {
    /// A relational expression node.
    Rel = 0b00,
    /// A scalar expression node.
    Expr = 0b01,
    /// A list of arena indices.
    List = 0b10,
    /// A sort key node.
    SortKey = 0b11,
}

/// Number of bits used for the tag.
const TAG_BITS: usize = 2;
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
        0b00 => NodeTag::Rel,
        0b01 => NodeTag::Expr,
        0b10 => NodeTag::List,
        0b11 => NodeTag::SortKey,
        // All 2-bit patterns are covered above; this is unreachable.
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
    /// Accumulated parse errors.
    errors: Vec<String>,
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
            errors: Vec::new(),
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

    /// Record a parse error.
    pub fn push_error(&mut self, msg: String) {
        self.errors.push(msg);
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

    /// Consume the parse state and return the final result.
    ///
    /// On success (no errors and at least one rel node), returns the last
    /// relational expression pushed — which is the root of the parse tree.
    /// On failure, returns the accumulated error messages.
    ///
    /// # Errors
    /// Returns the accumulated error messages if any errors were recorded
    /// or if no relational expression was pushed.
    pub fn take_result(self) -> Result<RelExpr, Vec<String>> {
        if !self.errors.is_empty() {
            return Err(self.errors);
        }
        self.rel_nodes
            .into_iter()
            .last()
            .ok_or_else(|| vec!["no relational expression produced".to_owned()])
    }
}

impl Default for RaParseState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::{NullOrdering, SortDirection};
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
        assert_eq!(errs, vec!["syntax error"]);
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
}
