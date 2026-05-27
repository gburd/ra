//! Typed AST for plan advice, port of `pgpa_ast.h`.
//!
//! Every type in this module mirrors a PG-side counterpart:
//!
//! | This module | `pgpa_ast.h` |
//! |---|---|
//! | [`AdviceTag`] | `pgpa_advice_tag_type` |
//! | [`AdviceTargetKind`] | `pgpa_target_type` |
//! | [`RelationIdentifier`] | `pgpa_identifier` |
//! | [`IndexTarget`] | `pgpa_index_target` |
//! | [`AdviceTarget`] | `pgpa_advice_target` |
//! | [`AdviceItem`] | `pgpa_advice_item` |
//! | [`Advice`] | `List *` of `pgpa_advice_item` |

use serde::{Deserialize, Serialize};

/// One of the 20 plan-advice tags PG knows about.
///
/// The order of variants and the spelling returned by
/// [`AdviceTag::as_keyword`] match `pgpa_cstring_advice_tag` in
/// `contrib/pg_plan_advice/pgpa_ast.c` exactly. New tags must be
/// kept in sync with PG's `pgpa_parse_advice_tag` lookup table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AdviceTag {
    BitmapHeapScan,
    DoNotScan,
    ForeignJoin,
    Gather,
    GatherMerge,
    HashJoin,
    IndexOnlyScan,
    IndexScan,
    JoinOrder,
    MergeJoinMaterialize,
    MergeJoinPlain,
    NestedLoopMaterialize,
    NestedLoopMemoize,
    NestedLoopPlain,
    NoGather,
    Partitionwise,
    SemijoinNonUnique,
    SemijoinUnique,
    SeqScan,
    TidScan,
}

impl AdviceTag {
    /// The uppercase keyword PG renders for this tag (matches
    /// `pgpa_cstring_advice_tag`).
    #[must_use]
    pub const fn as_keyword(self) -> &'static str {
        match self {
            Self::BitmapHeapScan => "BITMAP_HEAP_SCAN",
            Self::DoNotScan => "DO_NOT_SCAN",
            Self::ForeignJoin => "FOREIGN_JOIN",
            Self::Gather => "GATHER",
            Self::GatherMerge => "GATHER_MERGE",
            Self::HashJoin => "HASH_JOIN",
            Self::IndexOnlyScan => "INDEX_ONLY_SCAN",
            Self::IndexScan => "INDEX_SCAN",
            Self::JoinOrder => "JOIN_ORDER",
            Self::MergeJoinMaterialize => "MERGE_JOIN_MATERIALIZE",
            Self::MergeJoinPlain => "MERGE_JOIN_PLAIN",
            Self::NestedLoopMaterialize => "NESTED_LOOP_MATERIALIZE",
            Self::NestedLoopMemoize => "NESTED_LOOP_MEMOIZE",
            Self::NestedLoopPlain => "NESTED_LOOP_PLAIN",
            Self::NoGather => "NO_GATHER",
            Self::Partitionwise => "PARTITIONWISE",
            Self::SemijoinNonUnique => "SEMIJOIN_NON_UNIQUE",
            Self::SemijoinUnique => "SEMIJOIN_UNIQUE",
            Self::SeqScan => "SEQ_SCAN",
            Self::TidScan => "TID_SCAN",
        }
    }

    /// Parse a tag from its lowercase keyword. Mirrors
    /// `pgpa_parse_advice_tag` in `pgpa_ast.c`. Returns `None` on
    /// unknown input; the parser converts that to a syntax error.
    #[must_use]
    pub fn from_lowercase(s: &str) -> Option<Self> {
        // The first-character switch + strcmp pattern PG uses
        // translates directly to a Rust match. We keep the same
        // structure so adding new tags requires updating both
        // sides.
        Some(match s.as_bytes().first()? {
            b'b' if s == "bitmap_heap_scan" => Self::BitmapHeapScan,
            b'd' if s == "do_not_scan" => Self::DoNotScan,
            b'f' if s == "foreign_join" => Self::ForeignJoin,
            b'g' if s == "gather" => Self::Gather,
            b'g' if s == "gather_merge" => Self::GatherMerge,
            b'h' if s == "hash_join" => Self::HashJoin,
            b'i' if s == "index_scan" => Self::IndexScan,
            b'i' if s == "index_only_scan" => Self::IndexOnlyScan,
            b'j' if s == "join_order" => Self::JoinOrder,
            b'm' if s == "merge_join_materialize" => Self::MergeJoinMaterialize,
            b'm' if s == "merge_join_plain" => Self::MergeJoinPlain,
            b'n' if s == "nested_loop_materialize" => Self::NestedLoopMaterialize,
            b'n' if s == "nested_loop_memoize" => Self::NestedLoopMemoize,
            b'n' if s == "nested_loop_plain" => Self::NestedLoopPlain,
            b'n' if s == "no_gather" => Self::NoGather,
            b'p' if s == "partitionwise" => Self::Partitionwise,
            b's' if s == "semijoin_non_unique" => Self::SemijoinNonUnique,
            b's' if s == "semijoin_unique" => Self::SemijoinUnique,
            b's' if s == "seq_scan" => Self::SeqScan,
            b't' if s == "tid_scan" => Self::TidScan,
            _ => return None,
        })
    }

    /// Tags that the PG scanner returns as `TOK_TAG_INDEX` —
    /// i.e. their argument list is a sequence of
    /// `relation_identifier index_name` pairs (with optional
    /// `schema.name` for the index).
    #[must_use]
    pub const fn requires_index_argument(self) -> bool {
        matches!(self, Self::IndexScan | Self::IndexOnlyScan)
    }

    /// Tags that the PG scanner returns as `TOK_TAG_SIMPLE` —
    /// their argument list is a flat sequence of relation
    /// identifiers, no nesting allowed.
    #[must_use]
    pub const fn is_simple(self) -> bool {
        matches!(
            self,
            Self::BitmapHeapScan
                | Self::NoGather
                | Self::SeqScan
                | Self::TidScan
        )
    }

    /// Tags that the PG scanner returns as `TOK_TAG_JOIN_ORDER` —
    /// only `JOIN_ORDER` itself, which is special-cased to allow
    /// both ordered `(...)` and unordered `{...}` sublists.
    #[must_use]
    pub const fn is_join_order(self) -> bool {
        matches!(self, Self::JoinOrder)
    }
}

/// Discriminator for [`AdviceTarget`].
///
/// Mirrors `pgpa_target_type` in `pgpa_ast.h`. The three variants
/// represent the three forms a target can take in the grammar:
/// a bare relation identifier, a parenthesised ordered list, or
/// a curly-brace unordered list. Only `JOIN_ORDER` admits the
/// unordered variant; all other tags reject it during parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AdviceTargetKind {
    /// `alias#n/schema.name@plan` — a single relation reference.
    Identifier,
    /// `( ... )` — order matters (driving table first).
    OrderedList,
    /// `{ ... }` — order is undefined (planner-decided).
    UnorderedList,
}

/// Disambiguated reference to a single relation.
///
/// Mirrors `pgpa_identifier` in `pgpa_identifier.h`. Generated
/// advice always omits whatever components aren't needed; the
/// parser fills `occurrence` with `1` when the user omitted `#n`.
///
/// The full grammar production from `pgpa_parser.y`:
///
/// ```text
/// relation_identifier:
///     identifier opt_ri_occurrence opt_partition opt_plan_name
/// opt_ri_occurrence: '#' INTEGER | <empty>      // default 1
/// opt_partition:     '/' identifier '.' identifier
///                  | '/' identifier
///                  | <empty>
/// opt_plan_name:     '@' identifier | <empty>
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RelationIdentifier {
    /// Required. The alias name from the original SQL (or the
    /// relation's own name when no alias was supplied).
    pub alias_name: String,
    /// 1-based occurrence number. Generated advice writes `#2`,
    /// `#3`, ... when the alias appears multiple times in the
    /// same subquery. Default is 1.
    pub occurrence: u32,
    /// Partition schema (optional). When `partition_name` is
    /// `Some`, generated advice always supplies this too;
    /// user-written advice may omit it.
    pub partition_schema: Option<String>,
    /// Partition relation name. `Some` means the identifier
    /// refers to a partition of the parent table.
    pub partition_name: Option<String>,
    /// Subquery name the identifier resolves within. `None` means
    /// the top-level subquery.
    pub plan_name: Option<String>,
}

impl RelationIdentifier {
    /// Convenience constructor for the simple case `alias`
    /// with default occurrence and no partition/plan.
    #[must_use]
    pub fn simple(alias_name: impl Into<String>) -> Self {
        Self {
            alias_name: alias_name.into(),
            occurrence: 1,
            partition_schema: None,
            partition_name: None,
            plan_name: None,
        }
    }
}

/// An index reference attached to an `INDEX_SCAN` /
/// `INDEX_ONLY_SCAN` target. Mirrors `pgpa_index_target`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IndexTarget {
    /// Optional `schema.` prefix. `None` means the index is
    /// looked up on the search path.
    pub schema: Option<String>,
    /// Required index relation name.
    pub name: String,
}

/// A single argument to an advice tag — either a relation
/// identifier or a sublist that itself contains targets.
///
/// Mirrors `pgpa_advice_target`. The `index` field is `Some`
/// only when the parent tag is `INDEX_SCAN` / `INDEX_ONLY_SCAN`
/// and the target is a bare identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdviceTarget {
    pub kind: AdviceTargetKind,
    /// Set when `kind == Identifier`. Mirrors
    /// `pgpa_advice_target.rid`.
    pub identifier: Option<RelationIdentifier>,
    /// Set when `kind == OrderedList` or `kind == UnorderedList`.
    /// Mirrors `pgpa_advice_target.children`.
    pub children: Vec<AdviceTarget>,
    /// Set when this target is the argument to `INDEX_SCAN` /
    /// `INDEX_ONLY_SCAN`. Mirrors `pgpa_advice_target.itarget`.
    pub index: Option<IndexTarget>,
}

impl AdviceTarget {
    /// Build a bare-identifier target. Used by parser/render
    /// callers and by callers constructing advice
    /// programmatically.
    #[must_use]
    pub fn identifier(rid: RelationIdentifier) -> Self {
        Self {
            kind: AdviceTargetKind::Identifier,
            identifier: Some(rid),
            children: Vec::new(),
            index: None,
        }
    }

    /// Build an ordered-list target `(child1 child2 ...)`.
    #[must_use]
    pub fn ordered(children: Vec<AdviceTarget>) -> Self {
        Self {
            kind: AdviceTargetKind::OrderedList,
            identifier: None,
            children,
            index: None,
        }
    }

    /// Build an unordered-list target `{child1 child2 ...}`.
    /// Only legal under `JOIN_ORDER`.
    #[must_use]
    pub fn unordered(children: Vec<AdviceTarget>) -> Self {
        Self {
            kind: AdviceTargetKind::UnorderedList,
            identifier: None,
            children,
            index: None,
        }
    }
}

/// One advice instruction: a tag plus the list of targets it
/// applies to. Mirrors `pgpa_advice_item`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdviceItem {
    pub tag: AdviceTag,
    pub targets: Vec<AdviceTarget>,
}

/// A complete plan-advice document: zero or more advice items.
/// Mirrors the `List *` of `pgpa_advice_item` returned by
/// `pgpa_parse`.
pub type Advice = Vec<AdviceItem>;
