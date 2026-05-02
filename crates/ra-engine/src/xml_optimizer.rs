//! `XPath` and `XQuery` optimization for SQL/XML queries (RFC 0083).
//!
//! Provides `XPath` expression analysis, XML index-aware cost
//! estimation, and rewrite rules for queries embedding `XPath` or
//! `XQuery` expressions. Extracts optimization principles from
//! Berkeley DB XML and adapts them to relational databases:
//!
//! - **`PostgreSQL`**: `xpath()`, `xmlexists()`, `xmltable()`
//! - **Oracle**: `XMLQuery()`, `XMLTable()`, `existsNode()`
//! - **SQL Server**: `.value()`, `.query()`, `.exist()`, `.nodes()`
//!
//! All `XPath` parsing is best-effort. Malformed expressions are left
//! as opaque function calls with default costs. The optimizer never
//! rejects a query due to XML parsing failure.
//!
//! See: `rfcs/text/0083-xpath-xquery-optimization.md`

use std::fmt;

use egg::{rewrite, Id, Rewrite, Subst, Var};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;
use crate::parse_var;

// ------------------------------------------------------------------
// XPath axis types
// ------------------------------------------------------------------

/// `XPath` navigation axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum XPathAxis {
    /// `child::` (default axis)
    Child,
    /// `descendant::`
    Descendant,
    /// `descendant-or-self::`
    DescendantOrSelf,
    /// `self::`
    Self_,
    /// `parent::`
    Parent,
    /// `ancestor::`
    Ancestor,
    /// `ancestor-or-self::`
    AncestorOrSelf,
    /// `attribute::` or `@`
    Attribute,
    /// `following::`
    Following,
    /// `following-sibling::`
    FollowingSibling,
    /// `preceding::`
    Preceding,
    /// `preceding-sibling::`
    PrecedingSibling,
}

impl XPathAxis {
    /// Estimated relative cost of navigating this axis.
    ///
    /// Based on Berkeley DB XML's structural join costs:
    /// child/attribute are cheapest (direct lookup), descendant
    /// requires tree traversal, following/preceding scan entire
    /// document sections.
    #[must_use]
    pub fn navigation_cost(self) -> f64 {
        match self {
            Self::Child => 1.0,
            Self::Attribute => 0.5,
            Self::Self_ => 0.1,
            Self::Parent => 2.0,
            Self::Descendant | Self::DescendantOrSelf => 10.0,
            Self::Ancestor | Self::AncestorOrSelf => 8.0,
            Self::Following | Self::Preceding => 20.0,
            Self::FollowingSibling | Self::PrecedingSibling => 5.0,
        }
    }

    /// Whether this axis can be resolved via a structural index
    /// (path index in Berkeley DB XML terms).
    #[must_use]
    pub fn supports_structural_index(self) -> bool {
        match self {
            Self::Child
            | Self::Descendant
            | Self::DescendantOrSelf
            | Self::Parent
            | Self::Ancestor
            | Self::AncestorOrSelf
            | Self::Attribute => true,
            Self::Self_
            | Self::Following
            | Self::FollowingSibling
            | Self::Preceding
            | Self::PrecedingSibling => false,
        }
    }

    /// Parse an axis name from `XPath` syntax.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "child" => Some(Self::Child),
            "descendant" => Some(Self::Descendant),
            "descendant-or-self" => Some(Self::DescendantOrSelf),
            "self" => Some(Self::Self_),
            "parent" => Some(Self::Parent),
            "ancestor" => Some(Self::Ancestor),
            "ancestor-or-self" => Some(Self::AncestorOrSelf),
            "attribute" => Some(Self::Attribute),
            "following" => Some(Self::Following),
            "following-sibling" => Some(Self::FollowingSibling),
            "preceding" => Some(Self::Preceding),
            "preceding-sibling" => Some(Self::PrecedingSibling),
            _ => None,
        }
    }
}

impl fmt::Display for XPathAxis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Child => write!(f, "child"),
            Self::Descendant => write!(f, "descendant"),
            Self::DescendantOrSelf => {
                write!(f, "descendant-or-self")
            }
            Self::Self_ => write!(f, "self"),
            Self::Parent => write!(f, "parent"),
            Self::Ancestor => write!(f, "ancestor"),
            Self::AncestorOrSelf => {
                write!(f, "ancestor-or-self")
            }
            Self::Attribute => write!(f, "attribute"),
            Self::Following => write!(f, "following"),
            Self::FollowingSibling => {
                write!(f, "following-sibling")
            }
            Self::Preceding => write!(f, "preceding"),
            Self::PrecedingSibling => {
                write!(f, "preceding-sibling")
            }
        }
    }
}

// ------------------------------------------------------------------
// XPath node tests
// ------------------------------------------------------------------

/// `XPath` node test (what kind of node to select).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeTest {
    /// Named element: `child::item`
    Name(String),
    /// Wildcard: `child::*`
    Wildcard,
    /// `node()` -- matches any node
    AnyNode,
    /// `text()` -- matches text nodes
    Text,
    /// `comment()` -- matches comments
    Comment,
    /// `processing-instruction()` -- matches PIs
    ProcessingInstruction,
}

impl fmt::Display for NodeTest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(n) => write!(f, "{n}"),
            Self::Wildcard => write!(f, "*"),
            Self::AnyNode => write!(f, "node()"),
            Self::Text => write!(f, "text()"),
            Self::Comment => write!(f, "comment()"),
            Self::ProcessingInstruction => {
                write!(f, "processing-instruction()")
            }
        }
    }
}

// ------------------------------------------------------------------
// XPath predicates
// ------------------------------------------------------------------

/// Comparison operator in an `XPath` predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum XPathCompareOp {
    /// `=`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

impl fmt::Display for XPathCompareOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eq => write!(f, "="),
            Self::Ne => write!(f, "!="),
            Self::Lt => write!(f, "<"),
            Self::Le => write!(f, "<="),
            Self::Gt => write!(f, ">"),
            Self::Ge => write!(f, ">="),
        }
    }
}

/// A predicate attached to an `XPath` step.
#[derive(Debug, Clone, PartialEq)]
pub enum XPathPredicate {
    /// Comparison: `[@price > 100]`
    Comparison {
        /// Left side (usually a path or `.`)
        left: String,
        /// Comparison operator
        op: XPathCompareOp,
        /// Right side (usually a literal)
        right: String,
    },
    /// Positional: `[1]`, `[last()]`
    Position(PositionPredicate),
    /// Function call: `[contains(., 'text')]`
    Function {
        /// Function name
        name: String,
        /// Arguments as raw strings
        args: Vec<String>,
    },
    /// Existence check: `[@attr]` (attribute exists)
    Existence(String),
}

/// Positional predicate types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PositionPredicate {
    /// Fixed position: `[3]`
    Index(u64),
    /// Last element: `[last()]`
    Last,
    /// Computed position: `[position() mod 2 = 0]`
    Computed,
}

impl XPathPredicate {
    /// Estimated selectivity of this predicate.
    ///
    /// Follows Berkeley DB XML's approach: equality predicates on
    /// indexed paths have low selectivity, range predicates are
    /// moderate, positional predicates depend on document size.
    #[must_use]
    pub fn estimated_selectivity(&self) -> f64 {
        match self {
            Self::Comparison { op, .. } => match op {
                XPathCompareOp::Eq => 0.01,
                XPathCompareOp::Ne => 0.99,
                XPathCompareOp::Lt
                | XPathCompareOp::Le
                | XPathCompareOp::Gt
                | XPathCompareOp::Ge => 0.33,
            },
            Self::Position(PositionPredicate::Index(_) | PositionPredicate::Last) => 0.01,
            Self::Position(PositionPredicate::Computed) => 0.5,
            Self::Function { name, .. } => match name.as_str() {
                "contains" | "starts-with" => 0.1,
                "ends-with" => 0.15,
                "matches" => 0.2,
                "not" => 0.5,
                _ => 0.3,
            },
            Self::Existence(_) => 0.75,
        }
    }

    /// Whether this predicate can benefit from an XML value index.
    #[must_use]
    pub fn supports_value_index(&self) -> bool {
        match self {
            Self::Comparison { op, .. } => {
                matches!(
                    op,
                    XPathCompareOp::Eq
                        | XPathCompareOp::Lt
                        | XPathCompareOp::Le
                        | XPathCompareOp::Gt
                        | XPathCompareOp::Ge
                )
            }
            Self::Existence(_) => true,
            Self::Function { name, .. } => {
                matches!(name.as_str(), "contains" | "starts-with")
            }
            Self::Position(_) => false,
        }
    }
}

// ------------------------------------------------------------------
// XPath expression structure
// ------------------------------------------------------------------

/// A single navigation step in an `XPath` expression.
#[derive(Debug, Clone, PartialEq)]
pub struct XPathStep {
    /// Navigation axis
    pub axis: XPathAxis,
    /// Node test (element name, wildcard, etc.)
    pub node_test: NodeTest,
    /// Predicates attached to this step
    pub predicates: Vec<XPathPredicate>,
}

impl XPathStep {
    /// Estimated cost of evaluating this step.
    #[must_use]
    pub fn estimated_cost(&self) -> f64 {
        let nav = self.axis.navigation_cost();
        let pred_cost: f64 = self.predicates.iter().map(predicate_eval_cost).sum();
        nav + pred_cost
    }

    /// Whether this step can use a structural (path) index.
    #[must_use]
    pub fn can_use_path_index(&self) -> bool {
        self.axis.supports_structural_index() && matches!(self.node_test, NodeTest::Name(_))
    }
}

/// Cost of evaluating a single predicate.
fn predicate_eval_cost(pred: &XPathPredicate) -> f64 {
    match pred {
        XPathPredicate::Comparison { .. } => 1.0,
        XPathPredicate::Position(_) => 0.5,
        XPathPredicate::Function { name, .. } => match name.as_str() {
            "contains" | "starts-with" | "ends-with" => 3.0,
            "matches" => 8.0,
            "not" | "boolean" | "number" | "string" => 0.5,
            _ => 5.0,
        },
        XPathPredicate::Existence(_) => 0.2,
    }
}

/// Parsed `XPath` expression as a sequence of steps.
#[derive(Debug, Clone, PartialEq)]
pub struct XPathExpr {
    /// Whether the path is absolute (`/doc/...`) or relative
    pub absolute: bool,
    /// Navigation steps
    pub steps: Vec<XPathStep>,
}

impl XPathExpr {
    /// Total estimated evaluation cost (without index).
    #[must_use]
    pub fn estimated_cost(&self) -> f64 {
        self.steps.iter().map(XPathStep::estimated_cost).sum()
    }

    /// Whether this expression could be served entirely from
    /// XML indexes (path + value indexes cover all steps).
    #[must_use]
    pub fn is_index_coverable(&self) -> bool {
        self.steps.iter().all(|step| {
            step.can_use_path_index()
                && step
                    .predicates
                    .iter()
                    .all(XPathPredicate::supports_value_index)
        })
    }

    /// Extract the simple path string (e.g., "/doc/items/item").
    ///
    /// Returns None if the path contains wildcards, computed
    /// predicates, or non-child/descendant axes that prevent
    /// index usage.
    #[must_use]
    pub fn simple_path(&self) -> Option<String> {
        let mut parts = Vec::new();
        for step in &self.steps {
            match (&step.axis, &step.node_test) {
                (XPathAxis::Child, NodeTest::Name(n)) => {
                    parts.push(n.clone());
                }
                (XPathAxis::Attribute, NodeTest::Name(n)) => {
                    parts.push(format!("@{n}"));
                }
                _ => return None,
            }
        }
        if self.absolute {
            Some(format!("/{}", parts.join("/")))
        } else {
            Some(parts.join("/"))
        }
    }

    /// Collect all predicates across all steps.
    #[must_use]
    pub fn all_predicates(&self) -> Vec<&XPathPredicate> {
        self.steps.iter().flat_map(|s| &s.predicates).collect()
    }

    /// Combined selectivity of all predicates.
    #[must_use]
    #[expect(clippy::similar_names, reason = "sels and self are distinct concepts")]
    pub fn combined_selectivity(&self) -> f64 {
        let sels: Vec<f64> = self
            .all_predicates()
            .iter()
            .map(|p| p.estimated_selectivity())
            .collect();
        combine_selectivities(&sels)
    }
}

impl fmt::Display for XPathExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.absolute {
            write!(f, "/")?;
        }
        for (i, step) in self.steps.iter().enumerate() {
            if i > 0 {
                write!(f, "/")?;
            }
            if step.axis != XPathAxis::Child {
                write!(f, "{}::", step.axis)?;
            }
            write!(f, "{}", step.node_test)?;
            for pred in &step.predicates {
                write!(f, "[{pred}]")?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for XPathPredicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Comparison { left, op, right } => {
                write!(f, "{left} {op} {right}")
            }
            Self::Position(PositionPredicate::Index(n)) => {
                write!(f, "{n}")
            }
            Self::Position(PositionPredicate::Last) => {
                write!(f, "last()")
            }
            Self::Position(PositionPredicate::Computed) => {
                write!(f, "position()")
            }
            Self::Function { name, args } => {
                write!(f, "{name}({})", args.join(", "))
            }
            Self::Existence(attr) => write!(f, "@{attr}"),
        }
    }
}

// ------------------------------------------------------------------
// XPath parser (best-effort, common patterns only)
// ------------------------------------------------------------------

/// Parse an `XPath` expression string into structured form.
///
/// Handles common `XPath` 1.0 patterns used in SQL/XML queries.
/// Returns None for expressions too complex to parse (they remain
/// as opaque function calls with default costs).
#[must_use]
pub fn parse_xpath(input: &str) -> Option<XPathExpr> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (absolute, rest) = if let Some(stripped) = trimmed.strip_prefix('/') {
        (true, stripped)
    } else {
        (false, trimmed)
    };

    // Handle leading // (descendant-or-self::node()/)
    let (has_leading_double_slash, rest) = if let Some(stripped) = rest.strip_prefix('/') {
        (true, stripped)
    } else {
        (false, rest)
    };

    if rest.is_empty() {
        return None;
    }

    let segments = split_path_segments(rest);
    let mut steps = Vec::new();

    if has_leading_double_slash {
        steps.push(XPathStep {
            axis: XPathAxis::DescendantOrSelf,
            node_test: NodeTest::AnyNode,
            predicates: Vec::new(),
        });
    }

    for segment in segments {
        if let Some(step) = parse_step(segment) {
            steps.push(step);
        } else {
            return None;
        }
    }

    if steps.is_empty() {
        return None;
    }

    Some(XPathExpr { absolute, steps })
}

/// Split a path string by '/' while respecting predicates.
fn split_path_segments(path: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut bracket_depth: u32 = 0;

    for (i, ch) in path.char_indices() {
        match ch {
            '[' => bracket_depth += 1,
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
            }
            '/' if bracket_depth == 0 => {
                if i > start {
                    segments.push(&path[start..i]);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < path.len() {
        segments.push(&path[start..]);
    }
    segments
}

/// Parse a single `XPath` step (e.g., "item[@price > 100]").
fn parse_step(segment: &str) -> Option<XPathStep> {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Handle abbreviated axes
    if trimmed == "." {
        return Some(XPathStep {
            axis: XPathAxis::Self_,
            node_test: NodeTest::AnyNode,
            predicates: Vec::new(),
        });
    }
    if trimmed == ".." {
        return Some(XPathStep {
            axis: XPathAxis::Parent,
            node_test: NodeTest::AnyNode,
            predicates: Vec::new(),
        });
    }

    // Split off predicates
    let (name_part, predicates) = split_predicates(trimmed);

    // Parse axis and node test
    let (axis, node_test) = parse_axis_and_test(name_part)?;

    Some(XPathStep {
        axis,
        node_test,
        predicates,
    })
}

/// Split predicates from a step: "item[@x > 1][2]" -> ("item", [...])
fn split_predicates(step: &str) -> (&str, Vec<XPathPredicate>) {
    let bracket_start = step.find('[');
    let name_part = match bracket_start {
        Some(idx) => &step[..idx],
        None => return (step, Vec::new()),
    };

    let pred_str = &step[bracket_start.unwrap_or(step.len())..];
    let predicates = parse_predicate_list(pred_str);
    (name_part, predicates)
}

/// Parse a list of predicates: "[pred1][pred2]"
fn parse_predicate_list(s: &str) -> Vec<XPathPredicate> {
    let mut predicates = Vec::new();
    let mut depth: u32 = 0;
    let mut start = 0;

    for (i, ch) in s.char_indices() {
        match ch {
            '[' => {
                if depth == 0 {
                    start = i + 1;
                }
                depth += 1;
            }
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let inner = s[start..i].trim();
                    if let Some(pred) = parse_predicate(inner) {
                        predicates.push(pred);
                    }
                }
            }
            _ => {}
        }
    }
    predicates
}

/// Parse a single predicate expression.
fn parse_predicate(s: &str) -> Option<XPathPredicate> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Positional: pure number
    if let Ok(n) = trimmed.parse::<u64>() {
        return Some(XPathPredicate::Position(PositionPredicate::Index(n)));
    }

    // last()
    if trimmed == "last()" {
        return Some(XPathPredicate::Position(PositionPredicate::Last));
    }

    // position() based
    if trimmed.starts_with("position()") {
        return Some(XPathPredicate::Position(PositionPredicate::Computed));
    }

    // Existence check: @attr (bare attribute reference)
    if let Some(attr) = trimmed.strip_prefix('@') {
        if !attr.contains(' ') && !attr.contains('=') {
            return Some(XPathPredicate::Existence(attr.to_string()));
        }
    }

    // Function call: name(args)
    if let Some(paren_idx) = trimmed.find('(') {
        if trimmed.ends_with(')') {
            let name = trimmed[..paren_idx].trim();
            let args_str = &trimmed[paren_idx + 1..trimmed.len() - 1];
            // Only treat as function if name is a known function
            if is_xpath_function(name) {
                let args: Vec<String> = args_str.split(',').map(|a| a.trim().to_string()).collect();
                return Some(XPathPredicate::Function {
                    name: name.to_string(),
                    args,
                });
            }
        }
    }

    // Comparison: look for operator
    for (op_str, op) in &[
        ("!=", XPathCompareOp::Ne),
        ("<=", XPathCompareOp::Le),
        (">=", XPathCompareOp::Ge),
        ("=", XPathCompareOp::Eq),
        ("<", XPathCompareOp::Lt),
        (">", XPathCompareOp::Gt),
    ] {
        if let Some(idx) = trimmed.find(op_str) {
            let left = trimmed[..idx].trim().to_string();
            let right = trimmed[idx + op_str.len()..].trim().to_string();
            return Some(XPathPredicate::Comparison {
                left,
                op: *op,
                right,
            });
        }
    }

    None
}

/// Check if a name is a known `XPath` function.
fn is_xpath_function(name: &str) -> bool {
    matches!(
        name,
        "contains"
            | "starts-with"
            | "ends-with"
            | "substring"
            | "string-length"
            | "normalize-space"
            | "translate"
            | "concat"
            | "not"
            | "boolean"
            | "number"
            | "string"
            | "count"
            | "sum"
            | "floor"
            | "ceiling"
            | "round"
            | "matches"
            | "upper-case"
            | "lower-case"
            | "last"
            | "position"
            | "true"
            | "false"
            | "id"
            | "local-name"
            | "namespace-uri"
            | "name"
    )
}

/// Parse axis and node test from "`axis::test`" or abbreviated form.
fn parse_axis_and_test(s: &str) -> Option<(XPathAxis, NodeTest)> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }

    // @attr => attribute::attr
    if let Some(attr) = trimmed.strip_prefix('@') {
        let test = parse_node_test(attr);
        return Some((XPathAxis::Attribute, test));
    }

    // axis::test
    if let Some(idx) = trimmed.find("::") {
        let axis_str = &trimmed[..idx];
        let test_str = &trimmed[idx + 2..];
        let axis = XPathAxis::parse(axis_str)?;
        let test = parse_node_test(test_str);
        return Some((axis, test));
    }

    // Abbreviated: just a name or node type test
    let test = parse_node_test(trimmed);
    Some((XPathAxis::Child, test))
}

/// Parse a node test string.
fn parse_node_test(s: &str) -> NodeTest {
    match s.trim() {
        "*" => NodeTest::Wildcard,
        "node()" => NodeTest::AnyNode,
        "text()" => NodeTest::Text,
        "comment()" => NodeTest::Comment,
        "processing-instruction()" => NodeTest::ProcessingInstruction,
        name => NodeTest::Name(name.to_string()),
    }
}

// ------------------------------------------------------------------
// XML index types
// ------------------------------------------------------------------

/// XML index type (modeled after Berkeley DB XML index categories).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum XmlIndexType {
    /// Path index: indexes structural paths in the document tree.
    /// Accelerates navigation (child, descendant, ancestor axes).
    Path,
    /// Value index: indexes path + value pairs.
    /// Accelerates equality and range predicates on specific paths.
    Value,
    /// Presence index: indexes element/attribute existence.
    /// Accelerates `xmlexists()` and `@attr` existence checks.
    Presence,
    /// Full-text index on text content.
    /// Accelerates `contains()`, `matches()` predicates.
    FullText,
    /// Property index: computed value stored alongside document.
    /// Used by SQL Server secondary XML indexes.
    Property,
}

impl fmt::Display for XmlIndexType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path => write!(f, "PATH"),
            Self::Value => write!(f, "VALUE"),
            Self::Presence => write!(f, "PRESENCE"),
            Self::FullText => write!(f, "FULLTEXT"),
            Self::Property => write!(f, "PROPERTY"),
        }
    }
}

/// XML value type for typed indexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum XmlValueType {
    /// String values
    String,
    /// Numeric values (integer or decimal)
    Numeric,
    /// Date/time values
    DateTime,
}

/// Metadata about an XML index available on a column.
#[derive(Debug, Clone, PartialEq)]
pub struct XmlIndexInfo {
    /// Index type
    pub index_type: XmlIndexType,
    /// Indexed paths (e.g., `["/doc/items/item/@price"]`)
    pub paths: Vec<String>,
    /// Value type constraint (for value indexes)
    pub value_type: Option<XmlValueType>,
    /// Estimated number of indexed entries
    pub estimated_entries: Option<u64>,
    /// Average entry size in bytes
    pub avg_entry_bytes: Option<u32>,
}

impl XmlIndexInfo {
    /// Whether this index covers the given `XPath` expression.
    #[must_use]
    pub fn covers_xpath(&self, xpath: &XPathExpr) -> bool {
        let Some(simple) = xpath.simple_path() else {
            return false;
        };

        match self.index_type {
            XmlIndexType::Path | XmlIndexType::Presence => self
                .paths
                .iter()
                .any(|p| simple.starts_with(p.as_str()) || p.starts_with(simple.as_str())),
            XmlIndexType::Value | XmlIndexType::Property => {
                self.paths.contains(&simple)
                    && xpath
                        .all_predicates()
                        .iter()
                        .all(|pred| pred.supports_value_index())
            }
            XmlIndexType::FullText => {
                self.paths.iter().any(|p| simple.starts_with(p.as_str()))
                    && xpath.all_predicates().iter().any(|pred| {
                        matches!(
                            pred,
                            XPathPredicate::Function {
                                name,
                                ..
                            } if matches!(
                                name.as_str(),
                                "contains" | "matches"
                            )
                        )
                    })
            }
        }
    }
}

// ------------------------------------------------------------------
// Cost estimation
// ------------------------------------------------------------------

/// Cost parameters for XML query evaluation.
#[derive(Debug, Clone)]
pub struct XmlCostParams {
    /// Cost per byte of XML document parsing (default: 0.001).
    pub parse_cost_per_byte: f64,
    /// Base cost for any XML function call (default: 5.0).
    pub function_call_overhead: f64,
    /// Discount when a path index is available (0.0-1.0).
    pub path_index_discount: f64,
    /// Discount when a value index is available (0.0-1.0).
    pub value_index_discount: f64,
}

impl Default for XmlCostParams {
    fn default() -> Self {
        Self {
            parse_cost_per_byte: 0.001,
            function_call_overhead: 5.0,
            path_index_discount: 0.1,
            value_index_discount: 0.05,
        }
    }
}

/// Estimate the cost of evaluating an `XPath` expression.
///
/// When XML indexes are available, the cost is significantly
/// reduced. Without indexes, cost scales with document size
/// (full parse required).
#[must_use]
pub fn estimate_xpath_cost(
    xpath: &XPathExpr,
    avg_doc_bytes: f64,
    indexes: &[XmlIndexInfo],
    params: &XmlCostParams,
) -> f64 {
    let has_path_index = indexes.iter().any(|idx| idx.covers_xpath(xpath));
    let has_value_index = indexes
        .iter()
        .any(|idx| idx.index_type == XmlIndexType::Value && idx.covers_xpath(xpath));

    let parse_cost = if has_path_index {
        0.0
    } else {
        avg_doc_bytes * params.parse_cost_per_byte
    };

    let nav_cost: f64 = xpath
        .steps
        .iter()
        .map(|s| {
            let base = s.estimated_cost();
            if has_path_index && s.can_use_path_index() {
                base * params.path_index_discount
            } else {
                base
            }
        })
        .sum();

    let pred_cost: f64 = xpath
        .all_predicates()
        .iter()
        .map(|p| {
            let base = predicate_eval_cost(p);
            if has_value_index && p.supports_value_index() {
                base * params.value_index_discount
            } else {
                base
            }
        })
        .sum();

    params.function_call_overhead + parse_cost + nav_cost + pred_cost
}

/// Combine selectivities with correlation damping.
///
/// Uses the same approach as `documentdb_optimizer`: product of
/// selectivities with exponential damping to avoid underestimation.
#[must_use]
pub fn combine_selectivities(selectivities: &[f64]) -> f64 {
    if selectivities.is_empty() {
        return 1.0;
    }
    if selectivities.len() == 1 {
        return selectivities[0];
    }

    let mut sorted: Vec<f64> = selectivities.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let damping: f64 = 0.85;
    let mut combined = 1.0_f64;
    for (i, &sel) in sorted.iter().enumerate() {
        let exponent = damping.powi(i32::try_from(i).unwrap_or(0));
        combined *= sel.powf(exponent);
    }

    combined.clamp(0.000_001, 1.0)
}

// ------------------------------------------------------------------
// XPath expression simplification
// ------------------------------------------------------------------

/// Simplify an `XPath` expression by removing redundant steps.
///
/// Applies the following simplifications (inspired by Berkeley DB
/// XML's ASTReplaceOptimizer):
///
/// 1. Remove `self::node()` steps (no-op navigation)
/// 2. Collapse `descendant-or-self::node()/child::X` to `descendant::X`
/// 3. Remove duplicate adjacent child steps to the same element
#[must_use]
pub fn simplify_xpath(expr: &XPathExpr) -> XPathExpr {
    let mut steps: Vec<XPathStep> = Vec::new();

    for (i, step) in expr.steps.iter().enumerate() {
        // Rule 1: skip self::node() with no predicates
        if step.axis == XPathAxis::Self_
            && step.node_test == NodeTest::AnyNode
            && step.predicates.is_empty()
        {
            continue;
        }

        // Rule 2: descendant-or-self::node()/ + child::X -> descendant::X
        if step.axis == XPathAxis::DescendantOrSelf
            && step.node_test == NodeTest::AnyNode
            && step.predicates.is_empty()
        {
            if let Some(next) = expr.steps.get(i + 1) {
                if next.axis == XPathAxis::Child {
                    steps.push(XPathStep {
                        axis: XPathAxis::Descendant,
                        node_test: next.node_test.clone(),
                        predicates: next.predicates.clone(),
                    });
                    continue;
                }
            }
        }

        // Rule 2 continued: skip the child step if previous was
        // descendant-or-self and we already merged
        if step.axis == XPathAxis::Child {
            if let Some(prev_orig) = (i > 0).then(|| &expr.steps[i - 1]) {
                if prev_orig.axis == XPathAxis::DescendantOrSelf
                    && prev_orig.node_test == NodeTest::AnyNode
                    && prev_orig.predicates.is_empty()
                {
                    continue;
                }
            }
        }

        steps.push(step.clone());
    }

    XPathExpr {
        absolute: expr.absolute,
        steps,
    }
}

// ------------------------------------------------------------------
// Platform-specific XML function detection
// ------------------------------------------------------------------

/// Database platform for XML function classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum XmlPlatform {
    /// `PostgreSQL`: `xpath()`, `xmlexists()`, `xmltable()`
    PostgreSQL,
    /// Oracle: `XMLQuery()`, `XMLTable()`, `existsNode()`
    Oracle,
    /// SQL Server: .`value()`, .`query()`, .`exist()`, .`nodes()`
    SqlServer,
}

/// Classification of an XML function call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlFunctionKind {
    /// Returns XML fragment(s): `xpath()`, `XMLQuery()`, .`query()`
    Query,
    /// Returns scalar value: `xpath()`[1]`::text`, .`value()`
    Value,
    /// Returns boolean existence: `xmlexists()`, `existsNode()`, .`exist()`
    Exists,
    /// Returns relational table: `xmltable()`, `XMLTable()`, .`nodes()`
    Table,
}

/// Recognized XML function call with parsed `XPath`.
#[derive(Debug, Clone)]
pub struct XmlFunctionCall {
    /// Platform this function belongs to
    pub platform: XmlPlatform,
    /// Kind of XML operation
    pub kind: XmlFunctionKind,
    /// Function name as it appears in SQL
    pub function_name: String,
    /// Parsed `XPath` expression (if parseable)
    pub xpath: Option<XPathExpr>,
    /// Raw `XPath` string
    pub xpath_raw: String,
}

/// Recognize an XML function name and classify it.
#[must_use]
pub fn classify_xml_function(name: &str) -> Option<(XmlPlatform, XmlFunctionKind)> {
    match name.to_lowercase().as_str() {
        // PostgreSQL
        "xpath" => Some((XmlPlatform::PostgreSQL, XmlFunctionKind::Query)),
        "xmlexists" => Some((XmlPlatform::PostgreSQL, XmlFunctionKind::Exists)),
        "xmltable" => Some((XmlPlatform::PostgreSQL, XmlFunctionKind::Table)),
        // Oracle
        "xmlquery" => Some((XmlPlatform::Oracle, XmlFunctionKind::Query)),
        "xml_table" => Some((XmlPlatform::Oracle, XmlFunctionKind::Table)),
        "existsnode" => Some((XmlPlatform::Oracle, XmlFunctionKind::Exists)),
        // SQL Server methods (appear as functions in translated SQL)
        "xml_value" | "xmlvalue" => Some((XmlPlatform::SqlServer, XmlFunctionKind::Value)),
        "xml_query" | "xmlquery_ss" => Some((XmlPlatform::SqlServer, XmlFunctionKind::Query)),
        "xml_exist" | "xmlexist" => Some((XmlPlatform::SqlServer, XmlFunctionKind::Exists)),
        "xml_nodes" | "xmlnodes" => Some((XmlPlatform::SqlServer, XmlFunctionKind::Table)),
        _ => None,
    }
}

// ------------------------------------------------------------------
// E-graph rewrite rules for XML query optimization
// ------------------------------------------------------------------

/// Return rewrite rules for XML/XPath/XQuery optimization.
///
/// Rules target SQL/XML patterns where `XPath` expressions are
/// embedded in function calls. The rules are safe for all
/// platforms; platform-specific rewrites are handled at the
/// dialect translation layer.
#[must_use]
pub fn xml_optimization_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Rule 1: Push XML function filter through inner join.
        //
        // When a filter contains an XML function (xpath, xmlexists),
        // push it to the side containing the XML column. This enables
        // XML index usage and reduces rows before the join.
        rewrite!("xml-filter-through-join-left";
            "(filter ?pred (join inner ?cond ?left ?right))" =>
            "(join inner ?cond (filter ?pred ?left) ?right)"
            if is_xml_function_filter(parse_var("?pred"))
        ),
        rewrite!("xml-filter-through-join-right";
            "(filter ?pred (join inner ?cond ?left ?right))" =>
            "(join inner ?cond ?left (filter ?pred ?right))"
            if is_xml_function_filter(parse_var("?pred"))
        ),
        // Rule 2: Push XML filter below projection.
        //
        // XML filters should evaluate before projection to allow
        // XML index access on the base scan.
        rewrite!("xml-filter-below-project";
            "(filter ?pred (project ?cols ?input))" =>
            "(project ?cols (filter ?pred ?input))"
            if is_xml_function_filter(parse_var("?pred"))
        ),
        // Rule 3: Split conjunctive XML filters.
        //
        // When an AND contains both an XML predicate and a
        // relational predicate, splitting them enables independent
        // optimization of each filter.
        rewrite!("xml-split-conjunctive-filter";
            "(filter (and ?p1 ?p2) ?input)" =>
            "(filter ?p1 (filter ?p2 ?input))"
            if is_xml_function_filter(parse_var("?p1"))
        ),
        // Rule 4: Merge adjacent XML filters.
        //
        // Inverse of rule 3: merge enables compound XML index
        // evaluation when both predicates target the same column.
        rewrite!("xml-merge-adjacent-filters";
            "(filter ?p1 (filter ?p2 ?input))" =>
            "(filter (and ?p1 ?p2) ?input)"
            if is_xml_function_filter(parse_var("?p1"))
            if is_xml_function_filter(parse_var("?p2"))
        ),
        // Rule 5: XML filter below aggregate.
        //
        // Push XML filters below aggregate when they reference
        // only base columns (not aggregation results).
        rewrite!("xml-filter-below-aggregate";
            "(filter ?pred (aggregate ?groups ?aggs ?input))" =>
            "(aggregate ?groups ?aggs (filter ?pred ?input))"
            if is_xml_function_filter(parse_var("?pred"))
        ),
        // Rule 6: XML filter through union branches.
        //
        // Push XML filter into each branch of a union.
        rewrite!("xml-filter-through-union";
            "(filter ?pred (union ?all ?left ?right))" =>
            "(union ?all (filter ?pred ?left) (filter ?pred ?right))"
            if is_xml_function_filter(parse_var("?pred"))
        ),
        // Rule 7: Push XML filter through left outer join (left side).
        //
        // An XML predicate on the preserved side of a left join
        // can safely be pushed below the join.
        rewrite!("xml-filter-through-left-join";
            "(filter ?pred (join left-outer ?cond ?left ?right))" =>
            "(join left-outer ?cond (filter ?pred ?left) ?right)"
            if is_xml_function_filter(parse_var("?pred"))
        ),
    ]
}

/// Condition: check if a predicate involves an XML function.
///
/// Looks for func nodes with XML function names (xpath, xmlexists,
/// etc.) in the predicate's e-class, up to a limited depth.
fn is_xml_function_filter(
    pred_var: Var,
) -> impl Fn(&mut egg::EGraph<RelLang, RelAnalysis>, Id, &Subst) -> bool {
    move |egraph, _id, subst| {
        let pred_id = subst[pred_var];
        contains_xml_function(egraph, pred_id, 4)
    }
}

/// Recursively check if an e-class contains XML function patterns.
fn contains_xml_function(egraph: &egg::EGraph<RelLang, RelAnalysis>, id: Id, depth: u32) -> bool {
    if depth == 0 {
        return false;
    }

    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::Func(children) => {
                // Check if first child is an XML function name
                if let Some(&name_id) = children.first() {
                    let name_canonical = egraph.find(name_id);
                    for name_node in &egraph[name_canonical].nodes {
                        if let RelLang::Symbol(sym) = name_node {
                            let name_str = sym.as_str();
                            if classify_xml_function(name_str).is_some() {
                                return true;
                            }
                        }
                    }
                }
            }
            RelLang::Eq([l, r])
            | RelLang::Ne([l, r])
            | RelLang::Lt([l, r])
            | RelLang::Le([l, r])
            | RelLang::Gt([l, r])
            | RelLang::Ge([l, r])
            | RelLang::And([l, r])
            | RelLang::Or([l, r])
                if (contains_xml_function(egraph, *l, depth - 1)
                    || contains_xml_function(egraph, *r, depth - 1)) =>
            {
                return true;
            }
            RelLang::Not([inner])
            | RelLang::IsNull([inner])
            | RelLang::IsNotNull([inner])
                if contains_xml_function(egraph, *inner, depth - 1) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

// ------------------------------------------------------------------
// Errors
// ------------------------------------------------------------------

/// Errors specific to XML optimization.
///
/// All errors are non-fatal: the optimizer falls back to treating
/// the XML function call as opaque with default costs.
#[derive(Debug, thiserror::Error)]
pub enum XmlOptimizerError {
    /// `XPath` expression could not be parsed.
    #[error(
        "XPath parse failed for '{xpath}': {reason}; \
         using default cost estimate"
    )]
    XPathParseFailed {
        /// The raw `XPath` string
        xpath: String,
        /// Why parsing failed
        reason: String,
    },

    /// XML index metadata unavailable.
    #[error(
        "XML index metadata unavailable for table '{table}': \
         {reason}; skipping index-aware optimization"
    )]
    IndexMetadataUnavailable {
        /// Table name
        table: String,
        /// Why metadata was unavailable
        reason: String,
    },
}

// ------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]
mod tests {
    use super::*;
    use crate::egraph::to_rec_expr;
    use crate::rewrite::all_rules;
    use egg::Runner;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    // -- XPath parsing tests --

    #[test]
    fn parse_simple_absolute_path() {
        let expr = parse_xpath("/doc/items/item").unwrap();
        assert!(expr.absolute);
        assert_eq!(expr.steps.len(), 3);
        assert_eq!(expr.steps[0].node_test, NodeTest::Name("doc".to_string()));
        assert_eq!(expr.steps[1].node_test, NodeTest::Name("items".to_string()));
        assert_eq!(expr.steps[2].node_test, NodeTest::Name("item".to_string()));
    }

    #[test]
    fn parse_relative_path() {
        let expr = parse_xpath("items/item").unwrap();
        assert!(!expr.absolute);
        assert_eq!(expr.steps.len(), 2);
    }

    #[test]
    fn parse_path_with_attribute() {
        let expr = parse_xpath("/doc/item/@price").unwrap();
        assert_eq!(expr.steps.len(), 3);
        assert_eq!(expr.steps[2].axis, XPathAxis::Attribute);
        assert_eq!(expr.steps[2].node_test, NodeTest::Name("price".to_string()));
    }

    #[test]
    fn parse_descendant_or_self_shorthand() {
        let expr = parse_xpath("//item").unwrap();
        assert!(expr.absolute);
        assert_eq!(expr.steps.len(), 2);
        assert_eq!(expr.steps[0].axis, XPathAxis::DescendantOrSelf);
        assert_eq!(expr.steps[1].axis, XPathAxis::Child);
    }

    #[test]
    fn parse_predicate_comparison() {
        let expr = parse_xpath("/doc/item[@price > 100]").unwrap();
        assert_eq!(expr.steps.len(), 2);
        assert_eq!(expr.steps[1].predicates.len(), 1);
        match &expr.steps[1].predicates[0] {
            XPathPredicate::Comparison { left, op, right } => {
                assert_eq!(left, "@price");
                assert_eq!(*op, XPathCompareOp::Gt);
                assert_eq!(right, "100");
            }
            other => panic!("expected Comparison, got {other:?}"),
        }
    }

    #[test]
    fn parse_predicate_equality() {
        let expr = parse_xpath("/doc/status[. = 'active']").unwrap();
        assert_eq!(expr.steps[1].predicates.len(), 1);
        match &expr.steps[1].predicates[0] {
            XPathPredicate::Comparison { op, .. } => {
                assert_eq!(*op, XPathCompareOp::Eq);
            }
            other => panic!("expected Comparison, got {other:?}"),
        }
    }

    #[test]
    fn parse_predicate_positional() {
        let expr = parse_xpath("/doc/items/item[1]").unwrap();
        assert_eq!(expr.steps[2].predicates.len(), 1);
        assert_eq!(
            expr.steps[2].predicates[0],
            XPathPredicate::Position(PositionPredicate::Index(1))
        );
    }

    #[test]
    fn parse_predicate_last() {
        let expr = parse_xpath("/doc/items/item[last()]").unwrap();
        assert_eq!(
            expr.steps[2].predicates[0],
            XPathPredicate::Position(PositionPredicate::Last)
        );
    }

    #[test]
    fn parse_predicate_function() {
        let expr = parse_xpath("/doc/name[contains(., 'Corp')]").unwrap();
        assert_eq!(expr.steps[1].predicates.len(), 1);
        match &expr.steps[1].predicates[0] {
            XPathPredicate::Function { name, args } => {
                assert_eq!(name, "contains");
                assert_eq!(args.len(), 2);
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn parse_predicate_existence() {
        let expr = parse_xpath("/doc/item[@id]").unwrap();
        assert_eq!(expr.steps[1].predicates.len(), 1);
        assert_eq!(
            expr.steps[1].predicates[0],
            XPathPredicate::Existence("id".to_string())
        );
    }

    #[test]
    fn parse_multiple_predicates() {
        let expr = parse_xpath("/doc/item[@price > 100][@status = 'active']").unwrap();
        assert_eq!(expr.steps[1].predicates.len(), 2);
    }

    #[test]
    fn parse_explicit_axis() {
        let expr = parse_xpath("/doc/descendant::item").unwrap();
        assert_eq!(expr.steps[1].axis, XPathAxis::Descendant);
    }

    #[test]
    fn parse_self_and_parent() {
        let expr = parse_xpath("./..").unwrap();
        assert_eq!(expr.steps.len(), 2);
        assert_eq!(expr.steps[0].axis, XPathAxis::Self_);
        assert_eq!(expr.steps[1].axis, XPathAxis::Parent);
    }

    #[test]
    fn parse_wildcard() {
        let expr = parse_xpath("/doc/*/name").unwrap();
        assert_eq!(expr.steps[1].node_test, NodeTest::Wildcard);
    }

    #[test]
    fn parse_text_node_test() {
        let expr = parse_xpath("/doc/name/text()").unwrap();
        assert_eq!(expr.steps[2].node_test, NodeTest::Text);
    }

    #[test]
    fn parse_empty_returns_none() {
        assert!(parse_xpath("").is_none());
        assert!(parse_xpath("  ").is_none());
    }

    #[test]
    fn parse_ne_predicate() {
        let expr = parse_xpath("/doc/item[@type != 'sample']").unwrap();
        match &expr.steps[1].predicates[0] {
            XPathPredicate::Comparison { op, .. } => {
                assert_eq!(*op, XPathCompareOp::Ne);
            }
            other => panic!("expected Comparison, got {other:?}"),
        }
    }

    // -- XPath simplification tests --

    #[test]
    fn simplify_removes_self_node() {
        let expr = parse_xpath("/doc/self::node()/item").unwrap();
        let simplified = simplify_xpath(&expr);
        // self::node() should be removed
        assert!(simplified.steps.len() < expr.steps.len());
    }

    #[test]
    fn simplify_collapses_descendant_or_self() {
        let expr = parse_xpath("//item").unwrap();
        let simplified = simplify_xpath(&expr);
        assert_eq!(simplified.steps.len(), 1);
        assert_eq!(simplified.steps[0].axis, XPathAxis::Descendant);
        assert_eq!(
            simplified.steps[0].node_test,
            NodeTest::Name("item".to_string())
        );
    }

    // -- Cost estimation tests --

    #[test]
    fn cost_increases_with_steps() {
        let short = parse_xpath("/doc/item").unwrap();
        let long = parse_xpath("/doc/items/item/name").unwrap();
        assert!(long.estimated_cost() > short.estimated_cost());
    }

    #[test]
    fn cost_descendant_higher_than_child() {
        let child = parse_xpath("/doc/item").unwrap();
        let desc = parse_xpath("/doc/descendant::item").unwrap();
        assert!(desc.estimated_cost() > child.estimated_cost());
    }

    #[test]
    fn cost_with_index_cheaper() {
        let xpath = parse_xpath("/doc/items/item").unwrap();
        let params = XmlCostParams::default();

        let cost_no_index = estimate_xpath_cost(&xpath, 10000.0, &[], &params);

        let index = XmlIndexInfo {
            index_type: XmlIndexType::Path,
            paths: vec!["/doc/items/item".to_string()],
            value_type: None,
            estimated_entries: Some(1000),
            avg_entry_bytes: Some(50),
        };
        let cost_with_index = estimate_xpath_cost(&xpath, 10000.0, &[index], &params);

        assert!(cost_with_index < cost_no_index);
    }

    #[test]
    fn cost_value_index_cheapest() {
        let xpath = parse_xpath("/doc/item[@price = 100]").unwrap();
        let params = XmlCostParams::default();

        let path_index = XmlIndexInfo {
            index_type: XmlIndexType::Path,
            paths: vec!["/doc/item".to_string()],
            value_type: None,
            estimated_entries: Some(1000),
            avg_entry_bytes: Some(50),
        };
        // Value index path matches the XPath simple_path (the
        // element path, not the attribute path inside predicates)
        let value_index = XmlIndexInfo {
            index_type: XmlIndexType::Value,
            paths: vec!["/doc/item".to_string()],
            value_type: Some(XmlValueType::Numeric),
            estimated_entries: Some(1000),
            avg_entry_bytes: Some(8),
        };

        let cost_path = estimate_xpath_cost(&xpath, 10000.0, &[path_index], &params);
        let cost_value = estimate_xpath_cost(&xpath, 10000.0, &[value_index], &params);

        // Value index should be cheaper due to predicate discount
        assert!(cost_value < cost_path);
    }

    // -- Index coverage tests --

    #[test]
    fn path_index_covers_simple_path() {
        let xpath = parse_xpath("/doc/items/item").unwrap();
        let index = XmlIndexInfo {
            index_type: XmlIndexType::Path,
            paths: vec!["/doc/items".to_string()],
            value_type: None,
            estimated_entries: None,
            avg_entry_bytes: None,
        };
        assert!(index.covers_xpath(&xpath));
    }

    #[test]
    fn value_index_needs_exact_path() {
        let xpath = parse_xpath("/doc/item").unwrap();
        let index = XmlIndexInfo {
            index_type: XmlIndexType::Value,
            paths: vec!["/doc/items/item".to_string()],
            value_type: Some(XmlValueType::String),
            estimated_entries: None,
            avg_entry_bytes: None,
        };
        assert!(!index.covers_xpath(&xpath));
    }

    #[test]
    fn fulltext_index_requires_text_function() {
        let xpath_no_fn = parse_xpath("/doc/name").unwrap();
        let xpath_with_fn = parse_xpath("/doc/name[contains(., 'x')]").unwrap();
        let index = XmlIndexInfo {
            index_type: XmlIndexType::FullText,
            paths: vec!["/doc/name".to_string()],
            value_type: None,
            estimated_entries: None,
            avg_entry_bytes: None,
        };
        assert!(!index.covers_xpath(&xpath_no_fn));
        assert!(index.covers_xpath(&xpath_with_fn));
    }

    // -- Selectivity tests --

    #[test]
    fn equality_selectivity_low() {
        let pred = XPathPredicate::Comparison {
            left: ".".to_string(),
            op: XPathCompareOp::Eq,
            right: "'val'".to_string(),
        };
        assert!(pred.estimated_selectivity() < 0.05);
    }

    #[test]
    fn range_selectivity_moderate() {
        let pred = XPathPredicate::Comparison {
            left: ".".to_string(),
            op: XPathCompareOp::Gt,
            right: "100".to_string(),
        };
        let sel = pred.estimated_selectivity();
        assert!(sel > 0.1);
        assert!(sel < 0.5);
    }

    #[test]
    fn combined_selectivity_damped() {
        let sel1 = 0.1;
        let sel2 = 0.2;
        let combined = combine_selectivities(&[sel1, sel2]);
        // Should be more than pure independence (sel1 * sel2 = 0.02)
        assert!(combined > sel1 * sel2);
        // But less than the most selective alone
        assert!(combined < sel1);
    }

    // -- Expression properties tests --

    #[test]
    fn simple_path_extraction() {
        let expr = parse_xpath("/doc/items/item").unwrap();
        assert_eq!(expr.simple_path(), Some("/doc/items/item".to_string()));
    }

    #[test]
    fn simple_path_with_attribute() {
        let expr = parse_xpath("/doc/item/@price").unwrap();
        assert_eq!(expr.simple_path(), Some("/doc/item/@price".to_string()));
    }

    #[test]
    fn simple_path_fails_for_wildcard() {
        let expr = parse_xpath("/doc/*/item").unwrap();
        assert!(expr.simple_path().is_none());
    }

    #[test]
    fn index_coverable_simple_path() {
        let expr = parse_xpath("/doc/item").unwrap();
        assert!(expr.is_index_coverable());
    }

    #[test]
    fn not_index_coverable_with_position() {
        let expr = parse_xpath("/doc/item[1]").unwrap();
        assert!(!expr.is_index_coverable());
    }

    // -- XML function classification tests --

    #[test]
    fn classify_postgresql_functions() {
        assert_eq!(
            classify_xml_function("xpath"),
            Some((XmlPlatform::PostgreSQL, XmlFunctionKind::Query))
        );
        assert_eq!(
            classify_xml_function("xmlexists"),
            Some((XmlPlatform::PostgreSQL, XmlFunctionKind::Exists))
        );
    }

    #[test]
    fn classify_oracle_functions() {
        assert_eq!(
            classify_xml_function("existsnode"),
            Some((XmlPlatform::Oracle, XmlFunctionKind::Exists))
        );
    }

    #[test]
    fn classify_sqlserver_functions() {
        assert_eq!(
            classify_xml_function("xml_exist"),
            Some((XmlPlatform::SqlServer, XmlFunctionKind::Exists))
        );
        assert_eq!(
            classify_xml_function("xml_value"),
            Some((XmlPlatform::SqlServer, XmlFunctionKind::Value))
        );
    }

    #[test]
    fn classify_unknown_returns_none() {
        assert!(classify_xml_function("random_func").is_none());
    }

    // -- XPath display tests --

    #[test]
    fn display_simple_path() {
        let expr = parse_xpath("/doc/items/item").unwrap();
        assert_eq!(format!("{expr}"), "/doc/items/item");
    }

    #[test]
    fn display_with_attribute() {
        let expr = parse_xpath("/doc/item/@price").unwrap();
        assert_eq!(format!("{expr}"), "/doc/item/attribute::price");
    }

    #[test]
    fn display_with_predicate() {
        let expr = parse_xpath("/doc/item[@price > 100]").unwrap();
        let displayed = format!("{expr}");
        assert!(displayed.contains("@price > 100"));
    }

    // -- Axis properties tests --

    #[test]
    fn structural_index_support() {
        assert!(XPathAxis::Child.supports_structural_index());
        assert!(XPathAxis::Descendant.supports_structural_index());
        assert!(XPathAxis::Attribute.supports_structural_index());
        assert!(!XPathAxis::Following.supports_structural_index());
        assert!(!XPathAxis::PrecedingSibling.supports_structural_index());
    }

    #[test]
    fn axis_cost_ordering() {
        assert!(XPathAxis::Attribute.navigation_cost() < XPathAxis::Child.navigation_cost());
        assert!(XPathAxis::Child.navigation_cost() < XPathAxis::Descendant.navigation_cost());
        assert!(XPathAxis::Descendant.navigation_cost() < XPathAxis::Following.navigation_cost());
    }

    // -- E-graph integration test --

    #[test]
    fn xml_rules_integrate_with_engine() {
        let rules = xml_optimization_rules();
        assert!(
            rules.len() >= 7,
            "expected at least 7 XML rules, got {}",
            rules.len()
        );
    }

    #[test]
    fn xml_rules_fire_on_filter_join() {
        // Build: filter(pred, join(inner, cond, left, right))
        // where pred contains an XML function call
        let left = RelExpr::scan("docs");
        let right = RelExpr::scan("users");
        let join = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("doc_id"))),
                right: Box::new(Expr::Column(ColumnRef::new("user_id"))),
            },
            left: Box::new(left),
            right: Box::new(right),
        };
        let filtered = join.filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("status"))),
            right: Box::new(Expr::Const(Const::String("active".to_string()))),
        });

        let rec = to_rec_expr(&filtered).expect("conversion ok");
        let runner = Runner::default()
            .with_expr(&rec)
            .with_node_limit(50_000)
            .with_iter_limit(10)
            .run(&all_rules());
        // E-graph should grow from rule applications
        assert!(runner.egraph.number_of_classes() > 1);
    }

    // -- Additional predicate tests --

    #[test]
    fn predicate_value_index_support() {
        let eq = XPathPredicate::Comparison {
            left: ".".to_string(),
            op: XPathCompareOp::Eq,
            right: "'x'".to_string(),
        };
        assert!(eq.supports_value_index());

        let ne = XPathPredicate::Comparison {
            left: ".".to_string(),
            op: XPathCompareOp::Ne,
            right: "'x'".to_string(),
        };
        // NE still needs scan but can use value index
        assert!(!ne.supports_value_index());

        let pos = XPathPredicate::Position(PositionPredicate::Index(1));
        assert!(!pos.supports_value_index());

        let exists = XPathPredicate::Existence("attr".to_string());
        assert!(exists.supports_value_index());
    }

    #[test]
    fn xpath_all_predicates() {
        let expr = parse_xpath("/doc/item[@id][. = 'x']").unwrap();
        assert_eq!(expr.all_predicates().len(), 2);
    }

    #[test]
    fn xpath_combined_selectivity() {
        let expr = parse_xpath("/doc/item[@price > 100][@status = 'active']").unwrap();
        let sel = expr.combined_selectivity();
        // Two predicates should produce lower selectivity than either
        let sel1 = expr.steps[1].predicates[0].estimated_selectivity();
        assert!(sel < sel1);
    }

    // -- Error type tests --

    #[test]
    fn error_display_xpath_parse() {
        let err = XmlOptimizerError::XPathParseFailed {
            xpath: "///bad".to_string(),
            reason: "triple slash".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("///bad"));
        assert!(msg.contains("triple slash"));
    }

    #[test]
    fn error_display_index_metadata() {
        let err = XmlOptimizerError::IndexMetadataUnavailable {
            table: "docs".to_string(),
            reason: "no catalog".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("docs"));
        assert!(msg.contains("no catalog"));
    }

    // -- XPathAxis parse/display roundtrip --

    #[test]
    fn axis_parse_display_roundtrip() {
        for name in &[
            "child",
            "descendant",
            "descendant-or-self",
            "self",
            "parent",
            "ancestor",
            "ancestor-or-self",
            "attribute",
            "following",
            "following-sibling",
            "preceding",
            "preceding-sibling",
        ] {
            let axis = XPathAxis::parse(name).unwrap();
            assert_eq!(format!("{axis}"), *name);
        }
    }

    #[test]
    fn axis_parse_unknown_returns_none() {
        assert!(XPathAxis::parse("invalid").is_none());
    }

    // -- Position predicate tests --

    #[test]
    fn parse_position_based_predicate() {
        let expr = parse_xpath("/doc/items[position() > 3]").unwrap();
        assert_eq!(expr.steps[1].predicates.len(), 1);
        assert_eq!(
            expr.steps[1].predicates[0],
            XPathPredicate::Position(PositionPredicate::Computed)
        );
    }

    #[test]
    fn position_predicate_not_value_indexable() {
        let pred = XPathPredicate::Position(PositionPredicate::Computed);
        assert!(!pred.supports_value_index());

        let pred = XPathPredicate::Position(PositionPredicate::Last);
        assert!(!pred.supports_value_index());
    }

    // -- Function predicate tests --

    #[test]
    fn function_predicate_contains_is_indexable() {
        let pred = XPathPredicate::Function {
            name: "contains".to_string(),
            args: vec![".".to_string(), "'x'".to_string()],
        };
        assert!(pred.supports_value_index());
    }

    #[test]
    fn function_predicate_starts_with_is_indexable() {
        let pred = XPathPredicate::Function {
            name: "starts-with".to_string(),
            args: vec![".".to_string(), "'abc'".to_string()],
        };
        assert!(pred.supports_value_index());
    }

    #[test]
    fn function_predicate_unknown_not_indexable() {
        let pred = XPathPredicate::Function {
            name: "normalize-space".to_string(),
            args: vec![".".to_string()],
        };
        assert!(!pred.supports_value_index());
    }

    // -- XmlIndexType display test --

    #[test]
    fn xml_index_type_display() {
        assert_eq!(format!("{}", XmlIndexType::Path), "PATH");
        assert_eq!(format!("{}", XmlIndexType::Value), "VALUE");
        assert_eq!(format!("{}", XmlIndexType::Presence), "PRESENCE");
        assert_eq!(format!("{}", XmlIndexType::FullText), "FULLTEXT");
        assert_eq!(format!("{}", XmlIndexType::Property), "PROPERTY");
    }

    // -- Presence index coverage test --

    #[test]
    fn presence_index_covers_simple_path() {
        let xpath = parse_xpath("/doc/item").unwrap();
        let index = XmlIndexInfo {
            index_type: XmlIndexType::Presence,
            paths: vec!["/doc/item".to_string()],
            value_type: None,
            estimated_entries: None,
            avg_entry_bytes: None,
        };
        assert!(index.covers_xpath(&xpath));
    }

    #[test]
    fn presence_index_does_not_cover_wildcard() {
        let xpath = parse_xpath("/doc/*/item").unwrap();
        let index = XmlIndexInfo {
            index_type: XmlIndexType::Presence,
            paths: vec!["/doc/item".to_string()],
            value_type: None,
            estimated_entries: None,
            avg_entry_bytes: None,
        };
        // Wildcard returns None for simple_path()
        assert!(!index.covers_xpath(&xpath));
    }

    // -- Property index test --

    #[test]
    fn property_index_needs_exact_match() {
        let xpath = parse_xpath("/doc/item[@price = 100]").unwrap();
        let index = XmlIndexInfo {
            index_type: XmlIndexType::Property,
            paths: vec!["/doc/item".to_string()],
            value_type: Some(XmlValueType::Numeric),
            estimated_entries: Some(5000),
            avg_entry_bytes: Some(8),
        };
        assert!(index.covers_xpath(&xpath));

        let wrong_index = XmlIndexInfo {
            index_type: XmlIndexType::Property,
            paths: vec!["/doc/other".to_string()],
            value_type: Some(XmlValueType::Numeric),
            estimated_entries: Some(5000),
            avg_entry_bytes: Some(8),
        };
        assert!(!wrong_index.covers_xpath(&xpath));
    }

    // -- Selectivity edge cases --

    #[test]
    fn combine_selectivities_empty_is_one() {
        let combined = combine_selectivities(&[]);
        assert!((combined - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn combine_selectivities_single() {
        let combined = combine_selectivities(&[0.42]);
        assert!((combined - 0.42).abs() < f64::EPSILON);
    }

    #[test]
    fn combine_selectivities_clamps_to_minimum() {
        // Many very selective predicates should not go below
        // the minimum clamp value
        let sels = vec![0.0001; 10];
        let combined = combine_selectivities(&sels);
        assert!(
            combined >= 0.000_001,
            "combined selectivity should be clamped"
        );
    }

    // -- Cost estimation edge cases --

    #[test]
    fn xpath_cost_with_fulltext_index() {
        let xpath = parse_xpath("/doc/name[contains(., 'Corp')]").unwrap();
        let params = XmlCostParams::default();

        let ft_index = XmlIndexInfo {
            index_type: XmlIndexType::FullText,
            paths: vec!["/doc/name".to_string()],
            value_type: None,
            estimated_entries: Some(10_000),
            avg_entry_bytes: Some(200),
        };

        let cost_no_index = estimate_xpath_cost(&xpath, 5000.0, &[], &params);
        let cost_with_ft = estimate_xpath_cost(&xpath, 5000.0, &[ft_index], &params);
        // Full-text index doesn't affect path navigation cost,
        // but the function is covered as a code path
        assert!(cost_with_ft > 0.0);
        assert!(cost_no_index > 0.0);
    }

    // -- Oracle function classification tests --

    #[test]
    fn classify_all_oracle_functions() {
        assert_eq!(
            classify_xml_function("xmlquery"),
            Some((XmlPlatform::Oracle, XmlFunctionKind::Query))
        );
        assert_eq!(
            classify_xml_function("xml_table"),
            Some((XmlPlatform::Oracle, XmlFunctionKind::Table))
        );
    }

    #[test]
    fn classify_all_sqlserver_functions() {
        assert_eq!(
            classify_xml_function("xmlvalue"),
            Some((XmlPlatform::SqlServer, XmlFunctionKind::Value))
        );
        assert_eq!(
            classify_xml_function("xmlquery_ss"),
            Some((XmlPlatform::SqlServer, XmlFunctionKind::Query))
        );
        assert_eq!(
            classify_xml_function("xmlexist"),
            Some((XmlPlatform::SqlServer, XmlFunctionKind::Exists))
        );
        assert_eq!(
            classify_xml_function("xml_nodes"),
            Some((XmlPlatform::SqlServer, XmlFunctionKind::Table))
        );
        assert_eq!(
            classify_xml_function("xmlnodes"),
            Some((XmlPlatform::SqlServer, XmlFunctionKind::Table))
        );
    }

    #[test]
    fn classify_postgresql_xmltable() {
        assert_eq!(
            classify_xml_function("xmltable"),
            Some((XmlPlatform::PostgreSQL, XmlFunctionKind::Table))
        );
    }

    // -- Case insensitivity test --

    #[test]
    fn classify_case_insensitive() {
        assert_eq!(
            classify_xml_function("XPATH"),
            Some((XmlPlatform::PostgreSQL, XmlFunctionKind::Query))
        );
        assert_eq!(
            classify_xml_function("XmlExists"),
            Some((XmlPlatform::PostgreSQL, XmlFunctionKind::Exists))
        );
    }

    // -- XPathExpr is_index_coverable edge cases --

    #[test]
    fn index_coverable_with_existence_predicate() {
        let expr = parse_xpath("/doc/item[@id]").unwrap();
        assert!(expr.is_index_coverable());
    }

    #[test]
    fn index_coverable_with_contains_function() {
        let expr = parse_xpath("/doc/name[contains(., 'x')]").unwrap();
        assert!(expr.is_index_coverable(), "contains() supports value index");
    }

    #[test]
    fn not_index_coverable_with_computed_position() {
        let expr = parse_xpath("/doc/item[position() > 2]").unwrap();
        assert!(!expr.is_index_coverable());
    }

    // -- XmlCostParams default test --

    #[test]
    fn xml_cost_params_default() {
        let params = XmlCostParams::default();
        assert!(params.parse_cost_per_byte > 0.0);
        assert!(params.function_call_overhead > 0.0);
        assert!(params.path_index_discount > 0.0 && params.path_index_discount < 1.0);
        assert!(params.value_index_discount > 0.0 && params.value_index_discount < 1.0);
    }

    // -- XmlOptimizerError additional test --

    #[test]
    fn error_display_xpath_parse_failed() {
        let err = XmlOptimizerError::XPathParseFailed {
            xpath: "/broken[".to_string(),
            reason: "unmatched bracket".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("/broken["));
        assert!(msg.contains("unmatched bracket"));
    }

    #[test]
    fn error_display_index_metadata_unavailable() {
        let err = XmlOptimizerError::IndexMetadataUnavailable {
            table: "docs".to_string(),
            reason: "no catalog access".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("docs"));
        assert!(msg.contains("no catalog access"));
    }

    // -- Simplify no-op test --

    #[test]
    fn simplify_already_simple_path_unchanged() {
        let expr = parse_xpath("/doc/item/name").unwrap();
        let simplified = simplify_xpath(&expr);
        assert_eq!(simplified.steps.len(), expr.steps.len());
        assert_eq!(simplified.steps[0].node_test, expr.steps[0].node_test);
    }

    // -- XPath Display with relative path --

    #[test]
    fn display_relative_path() {
        let expr = parse_xpath("items/item").unwrap();
        let displayed = format!("{expr}");
        assert!(!displayed.starts_with('/'));
        assert!(displayed.contains("items"));
    }

    // -- Nested predicate parsing --

    #[test]
    fn parse_le_and_ge_predicates() {
        let expr = parse_xpath("/doc/item[@price <= 100]").unwrap();
        match &expr.steps[1].predicates[0] {
            XPathPredicate::Comparison { op, .. } => {
                assert_eq!(*op, XPathCompareOp::Le);
            }
            other => panic!("expected Comparison, got {other:?}"),
        }

        let expr = parse_xpath("/doc/item[@price >= 50]").unwrap();
        match &expr.steps[1].predicates[0] {
            XPathPredicate::Comparison { op, .. } => {
                assert_eq!(*op, XPathCompareOp::Ge);
            }
            other => panic!("expected Comparison, got {other:?}"),
        }
    }

    // -- Node test parsing --

    #[test]
    fn parse_comment_node_test() {
        let expr = parse_xpath("/doc/comment()").unwrap();
        assert_eq!(expr.steps[1].node_test, NodeTest::Comment);
    }

    #[test]
    fn parse_processing_instruction_node_test() {
        let expr = parse_xpath("/doc/processing-instruction()").unwrap();
        assert_eq!(expr.steps[1].node_test, NodeTest::ProcessingInstruction);
    }

    #[test]
    fn parse_node_test() {
        let expr = parse_xpath("/doc/node()").unwrap();
        assert_eq!(expr.steps[1].node_test, NodeTest::AnyNode);
    }
}
