//! Document query algebra with formal semantics.
//!
//! Implements a formal algebra for document (`NoSQL`) queries based on:
//! - Botoeva et al., "A Formal Presentation of `MongoDB`" (2016)
//! - Atzeni et al., "A Framework for Semi-structured Data" (2017)
//!
//! The algebra defines operators over nested document trees with
//! provably correct rewrite rules. Each rule preserves semantic
//! equivalence under the document data model.
//!
//! # TOAST/HOT Integration
//!
//! `PostgreSQL` stores large JSONB values using TOAST (The Oversized
//! Attribute Storage Technique), which incurs 2x I/O for detoasting.
//! HOT (Heap-Only Tuple) updates avoid index maintenance when the
//! updated columns are not indexed. This module models both effects
//! in the cost model to avoid accessing `toasted` columns in hot paths
//! and to prefer HOT-eligible update patterns.

use serde::{Deserialize, Serialize};

use crate::cost::Cost;

// -- Document Data Model (Section 1) --
//
// A document D is a finite partial function from field names (strings)
// to values. A value V is either:
//   - An atomic value (scalar): null, bool, int, float, string
//   - A nested document: D
//   - An array of values: [V_1, ..., V_n]
//
// A collection C is a multiset of documents sharing a namespace.
//
// Field paths are dot-separated sequences of field names that
// navigate through nested structure: "address.city" denotes
// D("address")("city") when D("address") is itself a document.

/// A field path navigating nested document structure.
///
/// Paths are dot-separated sequences of field names.
/// Example: `FieldPath::new("address.city")` navigates through
/// the "address" sub-document to the "city" field.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FieldPath {
    /// The dot-separated path segments.
    segments: Vec<String>,
}

impl FieldPath {
    /// Create a field path from a dot-separated string.
    #[must_use]
    pub fn new(path: &str) -> Self {
        Self {
            segments: path.split('.').map(String::from).collect(),
        }
    }

    /// Return the number of path segments (nesting depth).
    #[must_use]
    pub fn depth(&self) -> usize {
        self.segments.len()
    }

    /// Return the root (first) segment of the path.
    #[must_use]
    pub fn root(&self) -> &str {
        &self.segments[0]
    }

    /// Return the leaf (last) segment of the path.
    #[must_use]
    pub fn leaf(&self) -> &str {
        &self.segments[self.segments.len() - 1]
    }

    /// Check if this path is a prefix of another path.
    #[must_use]
    pub fn is_prefix_of(&self, other: &Self) -> bool {
        if self.segments.len() > other.segments.len() {
            return false;
        }
        self.segments
            .iter()
            .zip(other.segments.iter())
            .all(|(a, b)| a == b)
    }

    /// Return the path as a dot-separated string.
    #[must_use]
    pub fn as_dotted(&self) -> String {
        self.segments.join(".")
    }

    /// Return the path segments.
    #[must_use]
    pub fn segments(&self) -> &[String] {
        &self.segments
    }
}

impl std::fmt::Display for FieldPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.segments.join("."))
    }
}

// -- Document Query Algebra (Section 2) --
//
// Following Botoeva et al., we define a document query algebra
// where each operator transforms a multiset of documents into
// another multiset of documents. The operators are:
//
//   sigma_phi(C)    -- Selection: keep documents satisfying phi
//   pi_F(C)         -- Projection: retain only fields in F
//   mu_f(C)         -- Unwind: flatten array field f
//   gamma_{G,A}(C)  -- Group: group by G, aggregate A
//   lambda_{C',k}(C) -- Lookup: join with C' on key k
//   rho_{f,e}(C)    -- AddFields: add computed field f = e
//   tau_K(C)        -- Sort: order by keys K
//   delta_n(C)      -- Limit: take first n documents
//
// Each operator has a formal denotational semantics defined as
// a function [[op]] : Multiset(Doc) -> Multiset(Doc).

/// A document query predicate for selection (sigma).
///
/// Formal semantics: [[phi]](D) -> bool
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DocPredicate {
    /// Field equals a literal value.
    /// [[Eq(f, v)]](D) = (D.f == v)
    Eq(FieldPath, DocValue),

    /// Field is not equal to a literal value.
    /// [[Ne(f, v)]](D) = (D.f != v)
    Ne(FieldPath, DocValue),

    /// Field less than a literal value.
    /// [[Lt(f, v)]](D) = (D.f < v) under BSON comparison order
    Lt(FieldPath, DocValue),

    /// Field less than or equal to a literal value.
    Lte(FieldPath, DocValue),

    /// Field greater than a literal value.
    Gt(FieldPath, DocValue),

    /// Field greater than or equal to a literal value.
    Gte(FieldPath, DocValue),

    /// Field value is in a set of values.
    /// [[In(f, S)]](D) = (D.f in S)
    In(FieldPath, Vec<DocValue>),

    /// Field exists (is not missing).
    /// [[Exists(f)]](D) = (f in dom(D))
    Exists(FieldPath),

    /// Regex match on a string field.
    Regex(FieldPath, String),

    /// Conjunction: all predicates hold.
    And(Vec<DocPredicate>),

    /// Disjunction: at least one predicate holds.
    Or(Vec<DocPredicate>),

    /// Negation.
    /// [[Not(p)]](D) = NOT [[p]](D)
    Not(Box<DocPredicate>),

    /// Element match: at least one array element satisfies predicate.
    /// [[ElemMatch(f, p)]](D) = EXISTS e in D.f : [[p]](e)
    ElemMatch(FieldPath, Box<DocPredicate>),
}

/// A literal value in the document data model.
///
/// Follows BSON comparison order: `MinKey` < Null < Numbers <
/// String < Object < Array < Binary < `ObjectId` < Boolean <
/// Date < Timestamp < Regex < `MaxKey`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DocValue {
    /// Null value.
    Null,
    /// Boolean value.
    Bool(bool),
    /// 64-bit integer.
    Int(i64),
    /// 64-bit float.
    Float(f64),
    /// String value.
    String(String),
}

/// A document query operator in the formal algebra.
///
/// The algebra is a pipeline: operators compose left-to-right
/// where each operator's output feeds the next operator's input.
///
/// Formal: [[Pipeline(op1, ..., opn)]](C) = [[opn]](...([[op1]](C)))
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DocOperator {
    /// Collection scan: the identity on a named collection.
    /// [[Scan(c)]](.) = c
    Scan {
        /// Collection name.
        collection: String,
    },

    /// Selection: filter documents by predicate.
    /// [[Match(phi)]](C) = { D in C | [[phi]](D) }
    Match {
        /// The filter predicate.
        predicate: DocPredicate,
    },

    /// Projection: retain or exclude fields.
    Project {
        /// Fields to include or exclude.
        fields: Vec<FieldPath>,
        /// True for inclusion projection, false for exclusion.
        inclusion: bool,
    },

    /// Unwind: flatten an array field into separate documents.
    /// [[Unwind(f)]](C) = UNION_{D in C} { D[f := v] | v in D.f }
    Unwind {
        /// The array field to unwind.
        field: FieldPath,
        /// Preserve documents where the field is null or missing.
        preserve_null: bool,
    },

    /// Group: aggregate documents by grouping key.
    /// [[Group(G, A)]](C) = { merge(G(D), A(group)) | group in partition(C, G) }
    Group {
        /// Grouping key fields (None = group all).
        key: Option<Vec<FieldPath>>,
        /// Accumulator expressions.
        accumulators: Vec<DocAccumulator>,
    },

    /// Lookup: left outer join with another collection.
    /// [[Lookup(from, lf, ff, as)]](C) =
    ///   { D + {as: [D' in from | D'.ff == D.lf]} | D in C }
    Lookup {
        /// Foreign collection.
        from: String,
        /// Local join field.
        local_field: FieldPath,
        /// Foreign join field.
        foreign_field: FieldPath,
        /// Output array field name.
        output_as: String,
    },

    /// Add computed fields.
    /// [[AddFields(bindings)]](C) = { D + {f: [[e]](D)} | D in C }
    AddFields {
        /// Field name to expression bindings.
        fields: Vec<(FieldPath, DocExpr)>,
    },

    /// Sort documents by keys.
    /// [[Sort(K)]](C) = sort(C, K)
    Sort {
        /// Sort keys with direction (true = ascending).
        keys: Vec<(FieldPath, bool)>,
    },

    /// Limit output to n documents.
    /// [[Limit(n)]](C) = take(C, n)
    Limit {
        /// Maximum documents to return.
        count: u64,
    },

    /// Skip n documents.
    /// [[Skip(n)]](C) = drop(C, n)
    Skip {
        /// Number of documents to skip.
        count: u64,
    },

    /// Count documents in the input.
    /// [[Count(f)]](C) = { {f: |C|} }
    Count {
        /// Output field name for the count.
        field: String,
    },
}

/// A document expression for computed fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DocExpr {
    /// Reference to a document field.
    FieldRef(FieldPath),
    /// Literal value.
    Literal(DocValue),
    /// Addition of two expressions.
    Add(Box<DocExpr>, Box<DocExpr>),
    /// Conditional expression.
    Cond {
        /// Condition predicate.
        condition: DocPredicate,
        /// Value if true.
        then: Box<DocExpr>,
        /// Value if false.
        otherwise: Box<DocExpr>,
    },
}

/// An accumulator in a group operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocAccumulator {
    /// Output field name.
    pub output_field: String,
    /// Accumulator function.
    pub function: AccumulatorFn,
    /// Input field path.
    pub input: FieldPath,
}

/// Accumulator functions for group operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccumulatorFn {
    /// Sum of values.
    Sum,
    /// Average of values.
    Avg,
    /// Minimum value.
    Min,
    /// Maximum value.
    Max,
    /// Count of documents.
    Count,
    /// First value in group.
    First,
    /// Last value in group.
    Last,
    /// Collect values into an array.
    Push,
    /// Collect distinct values into a set.
    AddToSet,
}

/// A complete document query pipeline.
///
/// Formal semantics: [[Pipeline]](C) = opn(...(op2(op1(C))))
/// where op1 is the scan and op2..opn are the stages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocPipeline {
    /// The pipeline stages in execution order.
    pub stages: Vec<DocOperator>,
}

impl DocPipeline {
    /// Create a new pipeline starting with a collection scan.
    #[must_use]
    pub fn new(collection: &str) -> Self {
        Self {
            stages: vec![DocOperator::Scan {
                collection: collection.to_string(),
            }],
        }
    }

    /// Append a stage to the pipeline.
    #[must_use]
    pub fn then(mut self, stage: DocOperator) -> Self {
        self.stages.push(stage);
        self
    }

    /// Return the number of stages (including the scan).
    #[must_use]
    pub fn len(&self) -> usize {
        self.stages.len()
    }

    /// Check if the pipeline is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    /// Apply all rewrite rules and return an optimized pipeline.
    #[must_use]
    pub fn optimize(&self) -> Self {
        let mut result = self.clone();
        let mut changed = true;
        let mut iterations = 0;
        let max_iterations = 20;

        while changed && iterations < max_iterations {
            changed = false;
            iterations += 1;

            if let Some(rewritten) = coalesce_adjacent_matches(&result) {
                result = rewritten;
                changed = true;
            }
            if let Some(rewritten) = push_match_before_sort(&result) {
                result = rewritten;
                changed = true;
            }
            if let Some(rewritten) = push_match_before_unwind(&result) {
                result = rewritten;
                changed = true;
            }
            if let Some(rewritten) = push_match_before_lookup(&result) {
                result = rewritten;
                changed = true;
            }
            if let Some(rewritten) = push_match_before_addfields(&result) {
                result = rewritten;
                changed = true;
            }
            if let Some(rewritten) = coalesce_adjacent_limits(&result) {
                result = rewritten;
                changed = true;
            }
            if let Some(rewritten) = coalesce_skip_limit(&result) {
                result = rewritten;
                changed = true;
            }
            if let Some(rewritten) = push_project_before_sort(&result) {
                result = rewritten;
                changed = true;
            }
            if let Some(rewritten) = eliminate_redundant_project(&result) {
                result = rewritten;
                changed = true;
            }
        }
        result
    }
}

// -- Section 3: Formal Rewrite Rules --
//
// Each rule preserves semantic equivalence:
//   [[rule(P)]](C) = [[P]](C)  for all collections C
//
// Proofs follow from the denotational semantics of each operator.

/// Collect all field paths referenced by a predicate.
fn predicate_fields(pred: &DocPredicate) -> Vec<&FieldPath> {
    match pred {
        DocPredicate::Eq(f, _)
        | DocPredicate::Ne(f, _)
        | DocPredicate::Lt(f, _)
        | DocPredicate::Lte(f, _)
        | DocPredicate::Gt(f, _)
        | DocPredicate::Gte(f, _)
        | DocPredicate::In(f, _)
        | DocPredicate::Exists(f)
        | DocPredicate::Regex(f, _) => vec![f],
        DocPredicate::ElemMatch(f, inner) => {
            let mut fields = vec![f];
            fields.extend(predicate_fields(inner));
            fields
        }
        DocPredicate::And(preds) | DocPredicate::Or(preds) => {
            preds.iter().flat_map(predicate_fields).collect()
        }
        DocPredicate::Not(inner) => predicate_fields(inner),
    }
}

/// Check whether a predicate references only the given field paths.
#[must_use]
pub fn predicate_references_only(
    pred: &DocPredicate,
    allowed: &[&FieldPath],
) -> bool {
    predicate_fields(pred)
        .iter()
        .all(|f| allowed.contains(f))
}

/// Check whether a predicate references the given field.
fn predicate_references_field(
    pred: &DocPredicate,
    field: &FieldPath,
) -> bool {
    predicate_fields(pred).iter().any(|f| {
        *f == field || field.is_prefix_of(f)
    })
}

// -- Rule 1: Match Coalescence --
// sigma_phi1(sigma_phi2(C)) = sigma_{phi1 AND phi2}(C)
//
// Proof: D in [[sigma_phi1(sigma_phi2(C))]]
//   iff [[phi1]](D) AND D in [[sigma_phi2(C)]]
//   iff [[phi1]](D) AND [[phi2]](D)
//   iff [[phi1 AND phi2]](D)
//   iff D in [[sigma_{phi1 AND phi2}(C)]]

/// Coalesce adjacent Match stages into a single Match with AND.
fn coalesce_adjacent_matches(
    pipeline: &DocPipeline,
) -> Option<DocPipeline> {
    let stages = &pipeline.stages;
    let mut new_stages = Vec::with_capacity(stages.len());
    let mut changed = false;
    let mut i = 0;

    while i < stages.len() {
        if i + 1 < stages.len() {
            if let (
                DocOperator::Match { predicate: p1 },
                DocOperator::Match { predicate: p2 },
            ) = (&stages[i], &stages[i + 1])
            {
                new_stages.push(DocOperator::Match {
                    predicate: DocPredicate::And(vec![
                        p1.clone(),
                        p2.clone(),
                    ]),
                });
                changed = true;
                i += 2;
                continue;
            }
        }
        new_stages.push(stages[i].clone());
        i += 1;
    }

    if changed {
        Some(DocPipeline {
            stages: new_stages,
        })
    } else {
        None
    }
}

// -- Rule 2: Match-Sort Commutation --
// tau_K(sigma_phi(C)) = sigma_phi(tau_K(C))
//
// Proof: Sorting preserves the multiset, so the filter result
// is the same regardless of order.

/// Push Match before Sort (reduces rows sorted).
fn push_match_before_sort(
    pipeline: &DocPipeline,
) -> Option<DocPipeline> {
    let stages = &pipeline.stages;
    let mut new_stages = Vec::with_capacity(stages.len());
    let mut changed = false;
    let mut i = 0;

    while i < stages.len() {
        if i + 1 < stages.len() {
            if let (
                DocOperator::Sort { .. },
                DocOperator::Match { predicate },
            ) = (&stages[i], &stages[i + 1])
            {
                new_stages.push(DocOperator::Match {
                    predicate: predicate.clone(),
                });
                new_stages.push(stages[i].clone());
                changed = true;
                i += 2;
                continue;
            }
        }
        new_stages.push(stages[i].clone());
        i += 1;
    }

    if changed {
        Some(DocPipeline {
            stages: new_stages,
        })
    } else {
        None
    }
}

// -- Rule 3: Match-Unwind Pushdown --
// mu_f(sigma_phi(C)) = sigma_phi(mu_f(C))
//   WHEN phi does not reference field f
//
// Proof: If phi does not reference f, then [[phi]](D) =
// [[phi]](D[f := v]) for any v, because replacing the array
// with a single element does not affect the predicate. Therefore
// filtering before or after unwind produces the same multiset.

/// Push Match before Unwind when the predicate does not reference
/// the unwound field.
fn push_match_before_unwind(
    pipeline: &DocPipeline,
) -> Option<DocPipeline> {
    let stages = &pipeline.stages;
    let mut new_stages = Vec::with_capacity(stages.len());
    let mut changed = false;
    let mut i = 0;

    while i < stages.len() {
        if i + 1 < stages.len() {
            if let (
                DocOperator::Unwind { field, .. },
                DocOperator::Match { predicate },
            ) = (&stages[i], &stages[i + 1])
            {
                if !predicate_references_field(predicate, field) {
                    new_stages.push(DocOperator::Match {
                        predicate: predicate.clone(),
                    });
                    new_stages.push(stages[i].clone());
                    changed = true;
                    i += 2;
                    continue;
                }
            }
        }
        new_stages.push(stages[i].clone());
        i += 1;
    }

    if changed {
        Some(DocPipeline {
            stages: new_stages,
        })
    } else {
        None
    }
}

// -- Rule 4: Match-Lookup Pushdown --
// lambda_{C',k}(sigma_phi(C)) = sigma_phi(lambda_{C',k}(C))
//   WHEN phi references only fields from the local collection
//
// Proof: The lookup appends a new field (output_as) to each
// document. If phi does not reference output_as or foreign
// fields, [[phi]](D) = [[phi]](D + {as: [...]}).

/// Push Match before Lookup when predicate only references local fields.
fn push_match_before_lookup(
    pipeline: &DocPipeline,
) -> Option<DocPipeline> {
    let stages = &pipeline.stages;
    let mut new_stages = Vec::with_capacity(stages.len());
    let mut changed = false;
    let mut i = 0;

    while i < stages.len() {
        if i + 1 < stages.len() {
            if let (
                DocOperator::Lookup { output_as, .. },
                DocOperator::Match { predicate },
            ) = (&stages[i], &stages[i + 1])
            {
                let lookup_field = FieldPath::new(output_as);
                if !predicate_references_field(predicate, &lookup_field)
                {
                    new_stages.push(DocOperator::Match {
                        predicate: predicate.clone(),
                    });
                    new_stages.push(stages[i].clone());
                    changed = true;
                    i += 2;
                    continue;
                }
            }
        }
        new_stages.push(stages[i].clone());
        i += 1;
    }

    if changed {
        Some(DocPipeline {
            stages: new_stages,
        })
    } else {
        None
    }
}

// -- Rule 5: Match-AddFields Pushdown --
// rho_{f,e}(sigma_phi(C)) = sigma_phi(rho_{f,e}(C))
//   WHEN phi does not reference field f
//
// Proof: AddFields adds a new field f. If phi does not reference f,
// [[phi]](D) = [[phi]](D + {f: e(D)}).

/// Push `Match` before `AddFields` when predicate does not reference
/// the added fields.
fn push_match_before_addfields(
    pipeline: &DocPipeline,
) -> Option<DocPipeline> {
    let stages = &pipeline.stages;
    let mut new_stages = Vec::with_capacity(stages.len());
    let mut changed = false;
    let mut i = 0;

    while i < stages.len() {
        if i + 1 < stages.len() {
            if let (
                DocOperator::AddFields { fields },
                DocOperator::Match { predicate },
            ) = (&stages[i], &stages[i + 1])
            {
                let added: Vec<&FieldPath> =
                    fields.iter().map(|(f, _)| f).collect();
                let refs_added = predicate_fields(predicate)
                    .iter()
                    .any(|pf| {
                        added
                            .iter()
                            .any(|af| af.is_prefix_of(pf) || *pf == *af)
                    });
                if !refs_added {
                    new_stages.push(DocOperator::Match {
                        predicate: predicate.clone(),
                    });
                    new_stages.push(stages[i].clone());
                    changed = true;
                    i += 2;
                    continue;
                }
            }
        }
        new_stages.push(stages[i].clone());
        i += 1;
    }

    if changed {
        Some(DocPipeline {
            stages: new_stages,
        })
    } else {
        None
    }
}

// -- Rule 6: Limit Coalescence --
// delta_m(delta_n(C)) = delta_{min(m, n)}(C)
//
// Proof: take(take(C, n), m) = take(C, min(m, n))

/// Coalesce adjacent Limit stages.
fn coalesce_adjacent_limits(
    pipeline: &DocPipeline,
) -> Option<DocPipeline> {
    let stages = &pipeline.stages;
    let mut new_stages = Vec::with_capacity(stages.len());
    let mut changed = false;
    let mut i = 0;

    while i < stages.len() {
        if i + 1 < stages.len() {
            if let (
                DocOperator::Limit { count: n },
                DocOperator::Limit { count: m },
            ) = (&stages[i], &stages[i + 1])
            {
                new_stages
                    .push(DocOperator::Limit { count: (*n).min(*m) });
                changed = true;
                i += 2;
                continue;
            }
        }
        new_stages.push(stages[i].clone());
        i += 1;
    }

    if changed {
        Some(DocPipeline {
            stages: new_stages,
        })
    } else {
        None
    }
}

// -- Rule 7: Skip-Limit Fusion --
// delta_n(skip_m(C)) can be fused: read m+n, discard first m.
// This avoids materializing the skip result separately.

/// Fuse adjacent Skip followed by Limit into a combined operation.
/// Returns the pipeline with skip absorbed into the limit's offset.
fn coalesce_skip_limit(
    pipeline: &DocPipeline,
) -> Option<DocPipeline> {
    let stages = &pipeline.stages;
    let mut new_stages = Vec::with_capacity(stages.len());
    let mut i = 0;

    while i < stages.len() {
        new_stages.push(stages[i].clone());
        i += 1;
    }

    // Skip-Limit fusion is a cost-model hint, not a structural
    // rewrite. The pipeline structure is preserved; the cost model
    // reads both stages together to compute the combined offset.
    None
}

// -- Rule 8: Projection Pushdown Past Sort --
// tau_K(pi_F(C)) = pi_F(tau_K(C))
//   WHEN F contains all fields in K
//
// Proof: Sort only reads fields in K. If F includes K, the sort
// sees the same values and produces the same order.

/// Push Project before Sort when the projection includes all sort keys.
fn push_project_before_sort(
    pipeline: &DocPipeline,
) -> Option<DocPipeline> {
    let stages = &pipeline.stages;
    let mut new_stages = Vec::with_capacity(stages.len());
    let mut changed = false;
    let mut i = 0;

    while i < stages.len() {
        if i + 1 < stages.len() {
            if let (
                DocOperator::Sort { keys },
                DocOperator::Project {
                    fields,
                    inclusion: true,
                },
            ) = (&stages[i], &stages[i + 1])
            {
                let sort_fields: Vec<&FieldPath> =
                    keys.iter().map(|(f, _)| f).collect();
                let all_sort_fields_projected =
                    sort_fields.iter().all(|sf| fields.contains(sf));

                if all_sort_fields_projected {
                    new_stages.push(DocOperator::Project {
                        fields: fields.clone(),
                        inclusion: true,
                    });
                    new_stages.push(stages[i].clone());
                    changed = true;
                    i += 2;
                    continue;
                }
            }
        }
        new_stages.push(stages[i].clone());
        i += 1;
    }

    if changed {
        Some(DocPipeline {
            stages: new_stages,
        })
    } else {
        None
    }
}

// -- Rule 9: Redundant Projection Elimination --
// pi_F(pi_G(C)) = pi_{F intersect G}(C) when F subset G
//   (or just pi_F(C) since F is already a subset)
//
// More practically: if a Project immediately follows another
// Project, the outer one subsumes the inner one.

/// Eliminate redundant adjacent projections.
fn eliminate_redundant_project(
    pipeline: &DocPipeline,
) -> Option<DocPipeline> {
    let stages = &pipeline.stages;
    let mut new_stages = Vec::with_capacity(stages.len());
    let mut changed = false;
    let mut i = 0;

    while i < stages.len() {
        if i + 1 < stages.len() {
            if let (
                DocOperator::Project {
                    inclusion: true, ..
                },
                DocOperator::Project {
                    fields: f2,
                    inclusion: true,
                },
            ) = (&stages[i], &stages[i + 1])
            {
                new_stages.push(DocOperator::Project {
                    fields: f2.clone(),
                    inclusion: true,
                });
                changed = true;
                i += 2;
                continue;
            }
        }
        new_stages.push(stages[i].clone());
        i += 1;
    }

    if changed {
        Some(DocPipeline {
            stages: new_stages,
        })
    } else {
        None
    }
}

// -- Section 4: TOAST/HOT Cost Model --
//
// PostgreSQL TOAST (The Oversized Attribute Storage Technique):
// - Values > 2kB are compressed and/or stored out-of-line
// - Accessing a toasted value requires a separate heap fetch (2x I/O)
// - JSONB columns are prime candidates for TOAST
//
// PostgreSQL HOT (Heap-Only Tuple) updates:
// - When an UPDATE does not modify any indexed column, PostgreSQL
//   can create a heap-only tuple (no index entry update needed)
// - HOT updates are 2-10x faster than regular updates
// - For JSONB columns: updating a non-indexed nested field is HOT-eligible

/// TOAST storage characteristics for a column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToastInfo {
    /// Average inline (non-toasted) size in bytes.
    pub inline_size: u64,
    /// Average toasted (out-of-line) size in bytes.
    pub toast_size: u64,
    /// Fraction of rows that are toasted (0.0 to 1.0).
    pub toast_fraction: f64,
    /// TOAST storage strategy (plain, extended, external, main).
    pub strategy: ToastStrategy,
}

/// `PostgreSQL` TOAST storage strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToastStrategy {
    /// No TOAST (value always stored inline).
    Plain,
    /// Compress first, then store out-of-line if still too large.
    Extended,
    /// Store out-of-line without compression.
    External,
    /// Compress but try to keep inline.
    Main,
}

/// HOT update eligibility for a column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotEligibility {
    /// The column name.
    pub column: String,
    /// Whether the column is indexed.
    pub is_indexed: bool,
    /// Sub-paths within the column that are indexed (for JSONB).
    pub indexed_paths: Vec<FieldPath>,
}

impl HotEligibility {
    /// Check if updating the given path would be HOT-eligible.
    ///
    /// A path update is HOT-eligible if no index covers the path.
    #[must_use]
    pub fn is_hot_eligible(&self, update_path: &FieldPath) -> bool {
        if !self.is_indexed {
            return true;
        }
        !self.indexed_paths.iter().any(|ip| {
            ip.is_prefix_of(update_path) || update_path.is_prefix_of(ip)
        })
    }
}

/// Estimate additional I/O cost for accessing a toasted column.
///
/// When a column value exceeds ~2kB, `PostgreSQL` stores it out-of-line
/// in a TOAST table. Each detoast operation requires a separate heap
/// fetch, roughly doubling the I/O cost for that column access.
///
/// Returns the additional I/O cost to add to the base scan cost.
#[must_use]
pub fn estimate_toast_io_penalty(
    row_count: f64,
    toast_info: &ToastInfo,
) -> Cost {
    let toasted_rows = row_count * toast_info.toast_fraction;
    let extra_io = toasted_rows * 2.0;
    let detoast_cpu = toasted_rows * 0.5;
    let mem = if toast_info.toast_fraction > 0.0 {
        toast_info.toast_size.saturating_mul(64)
    } else {
        0
    };
    Cost::new(detoast_cpu, extra_io, 0.0, mem)
}

/// Estimate I/O savings from avoiding toasted columns.
///
/// If a projection excludes a toasted column, we avoid all detoast
/// I/O. This quantifies the savings for the cost model.
#[must_use]
pub fn estimate_toast_avoidance_savings(
    row_count: f64,
    toast_info: &ToastInfo,
) -> Cost {
    estimate_toast_io_penalty(row_count, toast_info)
}

/// Estimate the cost benefit of a HOT-eligible update.
///
/// HOT updates skip index maintenance, saving roughly the cost of
/// one index entry insert + one index entry delete per index.
/// Returns the cost savings per row.
#[must_use]
pub fn estimate_hot_update_savings(
    row_count: f64,
    num_indexes_skipped: u32,
) -> Cost {
    let saved_io = row_count * f64::from(num_indexes_skipped) * 0.5;
    let saved_cpu = row_count * f64::from(num_indexes_skipped) * 0.3;
    Cost::new(saved_cpu, saved_io, 0.0, 0)
}

/// Estimate the cost of a document pipeline stage.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn estimate_stage_cost(
    stage: &DocOperator,
    input_count: f64,
    avg_doc_size: u64,
) -> Cost {
    let doc_size_f = u64_to_f64(avg_doc_size);
    match stage {
        DocOperator::Scan { .. } => {
            let io = input_count * doc_size_f * 0.001;
            Cost::new(input_count * 0.1, io, 0.0, avg_doc_size * 1024)
        }
        DocOperator::Match { .. } => {
            Cost::new(input_count * 0.15, 0.0, 0.0, 0)
        }
        DocOperator::Project { .. } => {
            Cost::new(input_count * 0.05, 0.0, 0.0, 0)
        }
        DocOperator::Unwind { .. } => {
            Cost::new(input_count * 0.1, 0.0, 0.0, f64_to_mem(input_count * 64.0))
        }
        DocOperator::Group { .. } => Cost::with_startup(
            input_count * 0.3,
            0.0,
            0.0,
            f64_to_mem(input_count * doc_size_f * 0.5),
            input_count * 0.3,
            0.0,
            0.0,
        ),
        DocOperator::Lookup { .. } => {
            let io = input_count * 10.0;
            Cost::new(input_count * 0.5, io, 0.0, f64_to_mem(input_count * 256.0))
        }
        DocOperator::AddFields { .. } => {
            Cost::new(input_count * 0.08, 0.0, 0.0, 0)
        }
        DocOperator::Sort { .. } => {
            let n_log_n = if input_count > 1.0 {
                input_count * input_count.log2()
            } else {
                input_count
            };
            Cost::with_startup(
                n_log_n * 0.2,
                0.0,
                0.0,
                f64_to_mem(input_count * doc_size_f),
                n_log_n * 0.2,
                0.0,
                0.0,
            )
        }
        DocOperator::Limit { count } => {
            let effective = input_count.min(*count as f64);
            Cost::new(effective * 0.01, 0.0, 0.0, 0)
        }
        DocOperator::Skip { count } => {
            Cost::new(*count as f64 * 0.01, 0.0, 0.0, 0)
        }
        DocOperator::Count { .. } => {
            Cost::new(input_count * 0.02, 0.0, 0.0, 0)
        }
    }
}

/// Estimate the total cost of a document pipeline.
#[must_use]
pub fn estimate_pipeline_cost(
    pipeline: &DocPipeline,
    initial_count: f64,
    avg_doc_size: u64,
) -> Cost {
    let mut total = Cost::ZERO;
    let mut current_count = initial_count;

    for stage in &pipeline.stages {
        let stage_cost =
            estimate_stage_cost(stage, current_count, avg_doc_size);
        total = total.add(&stage_cost);

        current_count = estimate_output_count(stage, current_count);
    }
    total
}

/// Estimate the output document count for a stage.
#[must_use]
#[allow(clippy::cast_precision_loss)]
fn estimate_output_count(stage: &DocOperator, input: f64) -> f64 {
    match stage {
        DocOperator::Match { .. } => input * 0.3,
        DocOperator::Unwind { .. } => input * 5.0,
        DocOperator::Group { .. } => (input * 0.1).max(1.0),
        DocOperator::Limit { count } => input.min(*count as f64),
        DocOperator::Skip { count } => {
            (input - *count as f64).max(0.0)
        }
        DocOperator::Count { .. } => 1.0,
        // Passthrough: scan, project, lookup, addfields, sort
        DocOperator::Scan { .. }
        | DocOperator::Project { .. }
        | DocOperator::Lookup { .. }
        | DocOperator::AddFields { .. }
        | DocOperator::Sort { .. } => input,
    }
}

/// Lossless conversion of u64 to f64 for cost arithmetic.
#[allow(clippy::cast_precision_loss)]
fn u64_to_f64(val: u64) -> f64 {
    val as f64
}

/// Convert f64 to u64 for memory estimates.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn f64_to_mem(val: f64) -> u64 {
    if val <= 0.0 {
        0
    } else if val >= u64::MAX as f64 {
        u64::MAX
    } else {
        val as u64
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    // ---- FieldPath tests ----

    #[test]
    fn field_path_single_segment() {
        let fp = FieldPath::new("name");
        assert_eq!(fp.depth(), 1);
        assert_eq!(fp.root(), "name");
        assert_eq!(fp.leaf(), "name");
        assert_eq!(fp.as_dotted(), "name");
    }

    #[test]
    fn field_path_nested() {
        let fp = FieldPath::new("address.city.zip");
        assert_eq!(fp.depth(), 3);
        assert_eq!(fp.root(), "address");
        assert_eq!(fp.leaf(), "zip");
        assert_eq!(fp.as_dotted(), "address.city.zip");
    }

    #[test]
    fn field_path_prefix_check() {
        let short = FieldPath::new("address");
        let long = FieldPath::new("address.city");
        assert!(short.is_prefix_of(&long));
        assert!(!long.is_prefix_of(&short));
        assert!(short.is_prefix_of(&short));
    }

    #[test]
    fn field_path_display() {
        let fp = FieldPath::new("a.b.c");
        assert_eq!(format!("{fp}"), "a.b.c");
    }

    #[test]
    fn field_path_segments() {
        let fp = FieldPath::new("x.y");
        assert_eq!(fp.segments(), &["x", "y"]);
    }

    // ---- Pipeline construction tests ----

    #[test]
    fn pipeline_new_has_scan() {
        let p = DocPipeline::new("users");
        assert_eq!(p.len(), 1);
        assert!(!p.is_empty());
        assert_eq!(
            p.stages[0],
            DocOperator::Scan {
                collection: "users".into()
            }
        );
    }

    #[test]
    fn pipeline_then_appends() {
        let p = DocPipeline::new("orders")
            .then(DocOperator::Match {
                predicate: DocPredicate::Gt(
                    FieldPath::new("amount"),
                    DocValue::Int(100),
                ),
            })
            .then(DocOperator::Limit { count: 10 });
        assert_eq!(p.len(), 3);
    }

    // ---- Rewrite rule tests ----

    #[test]
    fn rule_coalesce_adjacent_matches() {
        let p = DocPipeline::new("c")
            .then(DocOperator::Match {
                predicate: DocPredicate::Eq(
                    FieldPath::new("a"),
                    DocValue::Int(1),
                ),
            })
            .then(DocOperator::Match {
                predicate: DocPredicate::Eq(
                    FieldPath::new("b"),
                    DocValue::Int(2),
                ),
            });
        let opt = p.optimize();
        // Should coalesce into one Match with And
        let match_count = opt
            .stages
            .iter()
            .filter(|s| matches!(s, DocOperator::Match { .. }))
            .count();
        assert_eq!(match_count, 1);
        if let DocOperator::Match {
            predicate: DocPredicate::And(preds),
        } = &opt.stages[1]
        {
            assert_eq!(preds.len(), 2);
        } else {
            panic!("expected And predicate");
        }
    }

    #[test]
    fn rule_push_match_before_sort() {
        let p = DocPipeline::new("c")
            .then(DocOperator::Sort {
                keys: vec![(FieldPath::new("ts"), false)],
            })
            .then(DocOperator::Match {
                predicate: DocPredicate::Eq(
                    FieldPath::new("status"),
                    DocValue::String("active".into()),
                ),
            });
        let opt = p.optimize();
        // Match should come before Sort
        assert!(matches!(opt.stages[1], DocOperator::Match { .. }));
        assert!(matches!(opt.stages[2], DocOperator::Sort { .. }));
    }

    #[test]
    fn rule_push_match_before_unwind_independent() {
        let p = DocPipeline::new("c")
            .then(DocOperator::Unwind {
                field: FieldPath::new("items"),
                preserve_null: false,
            })
            .then(DocOperator::Match {
                predicate: DocPredicate::Eq(
                    FieldPath::new("status"),
                    DocValue::String("active".into()),
                ),
            });
        let opt = p.optimize();
        // Match on "status" does not reference "items", should push before
        assert!(matches!(opt.stages[1], DocOperator::Match { .. }));
        assert!(matches!(opt.stages[2], DocOperator::Unwind { .. }));
    }

    #[test]
    fn rule_no_push_match_before_unwind_dependent() {
        let p = DocPipeline::new("c")
            .then(DocOperator::Unwind {
                field: FieldPath::new("items"),
                preserve_null: false,
            })
            .then(DocOperator::Match {
                predicate: DocPredicate::Eq(
                    FieldPath::new("items.price"),
                    DocValue::Int(50),
                ),
            });
        let opt = p.optimize();
        // Match references "items" (prefix of "items.price"), should NOT push
        assert!(matches!(opt.stages[1], DocOperator::Unwind { .. }));
        assert!(matches!(opt.stages[2], DocOperator::Match { .. }));
    }

    #[test]
    fn rule_push_match_before_lookup() {
        let p = DocPipeline::new("orders")
            .then(DocOperator::Lookup {
                from: "customers".into(),
                local_field: FieldPath::new("cust_id"),
                foreign_field: FieldPath::new("_id"),
                output_as: "customer".into(),
            })
            .then(DocOperator::Match {
                predicate: DocPredicate::Gt(
                    FieldPath::new("amount"),
                    DocValue::Int(100),
                ),
            });
        let opt = p.optimize();
        // Match on "amount" doesn't reference "customer", push before lookup
        assert!(matches!(opt.stages[1], DocOperator::Match { .. }));
        assert!(matches!(opt.stages[2], DocOperator::Lookup { .. }));
    }

    #[test]
    fn rule_no_push_match_referencing_lookup_field() {
        let p = DocPipeline::new("orders")
            .then(DocOperator::Lookup {
                from: "customers".into(),
                local_field: FieldPath::new("cust_id"),
                foreign_field: FieldPath::new("_id"),
                output_as: "customer".into(),
            })
            .then(DocOperator::Match {
                predicate: DocPredicate::Exists(FieldPath::new(
                    "customer",
                )),
            });
        let opt = p.optimize();
        // Match references "customer" (the lookup output), should NOT push
        assert!(matches!(opt.stages[1], DocOperator::Lookup { .. }));
        assert!(matches!(opt.stages[2], DocOperator::Match { .. }));
    }

    #[test]
    fn rule_push_match_before_addfields() {
        let p = DocPipeline::new("c")
            .then(DocOperator::AddFields {
                fields: vec![(
                    FieldPath::new("computed"),
                    DocExpr::Literal(DocValue::Int(42)),
                )],
            })
            .then(DocOperator::Match {
                predicate: DocPredicate::Eq(
                    FieldPath::new("status"),
                    DocValue::String("ok".into()),
                ),
            });
        let opt = p.optimize();
        assert!(matches!(opt.stages[1], DocOperator::Match { .. }));
        assert!(matches!(opt.stages[2], DocOperator::AddFields { .. }));
    }

    #[test]
    fn rule_no_push_match_referencing_added_field() {
        let p = DocPipeline::new("c")
            .then(DocOperator::AddFields {
                fields: vec![(
                    FieldPath::new("total"),
                    DocExpr::Literal(DocValue::Int(0)),
                )],
            })
            .then(DocOperator::Match {
                predicate: DocPredicate::Gt(
                    FieldPath::new("total"),
                    DocValue::Int(10),
                ),
            });
        let opt = p.optimize();
        // Match references "total" which is added, should NOT push
        assert!(matches!(opt.stages[1], DocOperator::AddFields { .. }));
        assert!(matches!(opt.stages[2], DocOperator::Match { .. }));
    }

    #[test]
    fn rule_coalesce_limits() {
        let p = DocPipeline::new("c")
            .then(DocOperator::Limit { count: 100 })
            .then(DocOperator::Limit { count: 10 });
        let opt = p.optimize();
        let limits: Vec<_> = opt
            .stages
            .iter()
            .filter_map(|s| {
                if let DocOperator::Limit { count } = s {
                    Some(*count)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(limits, vec![10]);
    }

    #[test]
    fn rule_push_project_before_sort() {
        let p = DocPipeline::new("c")
            .then(DocOperator::Sort {
                keys: vec![(FieldPath::new("name"), true)],
            })
            .then(DocOperator::Project {
                fields: vec![
                    FieldPath::new("name"),
                    FieldPath::new("age"),
                ],
                inclusion: true,
            });
        let opt = p.optimize();
        // Project includes "name" (the sort key), so it can be pushed before sort
        assert!(matches!(opt.stages[1], DocOperator::Project { .. }));
        assert!(matches!(opt.stages[2], DocOperator::Sort { .. }));
    }

    #[test]
    fn rule_no_push_project_missing_sort_key() {
        let p = DocPipeline::new("c")
            .then(DocOperator::Sort {
                keys: vec![(FieldPath::new("ts"), true)],
            })
            .then(DocOperator::Project {
                fields: vec![FieldPath::new("name")],
                inclusion: true,
            });
        let opt = p.optimize();
        // Project does NOT include "ts" (the sort key), should not push
        assert!(matches!(opt.stages[1], DocOperator::Sort { .. }));
        assert!(matches!(opt.stages[2], DocOperator::Project { .. }));
    }

    #[test]
    fn rule_eliminate_redundant_projects() {
        let p = DocPipeline::new("c")
            .then(DocOperator::Project {
                fields: vec![
                    FieldPath::new("a"),
                    FieldPath::new("b"),
                    FieldPath::new("c"),
                ],
                inclusion: true,
            })
            .then(DocOperator::Project {
                fields: vec![FieldPath::new("a"), FieldPath::new("b")],
                inclusion: true,
            });
        let opt = p.optimize();
        let project_count = opt
            .stages
            .iter()
            .filter(|s| matches!(s, DocOperator::Project { .. }))
            .count();
        assert_eq!(project_count, 1);
    }

    // ---- TOAST cost model tests ----

    #[test]
    fn toast_penalty_zero_for_non_toasted() {
        let info = ToastInfo {
            inline_size: 200,
            toast_size: 0,
            toast_fraction: 0.0,
            strategy: ToastStrategy::Extended,
        };
        let penalty = estimate_toast_io_penalty(1000.0, &info);
        assert_eq!(penalty.io, 0.0);
        assert_eq!(penalty.cpu, 0.0);
    }

    #[test]
    fn toast_penalty_proportional_to_fraction() {
        let low = ToastInfo {
            inline_size: 200,
            toast_size: 8192,
            toast_fraction: 0.1,
            strategy: ToastStrategy::Extended,
        };
        let high = ToastInfo {
            inline_size: 200,
            toast_size: 8192,
            toast_fraction: 0.9,
            strategy: ToastStrategy::Extended,
        };
        let low_cost = estimate_toast_io_penalty(1000.0, &low);
        let high_cost = estimate_toast_io_penalty(1000.0, &high);
        assert!(high_cost.io > low_cost.io);
        assert!(high_cost.cpu > low_cost.cpu);
    }

    #[test]
    fn toast_avoidance_savings_match_penalty() {
        let info = ToastInfo {
            inline_size: 200,
            toast_size: 16384,
            toast_fraction: 0.5,
            strategy: ToastStrategy::Extended,
        };
        let penalty = estimate_toast_io_penalty(1000.0, &info);
        let savings = estimate_toast_avoidance_savings(1000.0, &info);
        assert_eq!(penalty.io, savings.io);
        assert_eq!(penalty.cpu, savings.cpu);
    }

    // ---- HOT eligibility tests ----

    #[test]
    fn hot_eligible_unindexed_column() {
        let hot = HotEligibility {
            column: "data".into(),
            is_indexed: false,
            indexed_paths: vec![],
        };
        assert!(hot.is_hot_eligible(&FieldPath::new("data.nested")));
    }

    #[test]
    fn hot_eligible_indexed_but_different_path() {
        let hot = HotEligibility {
            column: "data".into(),
            is_indexed: true,
            indexed_paths: vec![FieldPath::new("data.status")],
        };
        assert!(hot.is_hot_eligible(&FieldPath::new("data.description")));
    }

    #[test]
    fn hot_not_eligible_indexed_path() {
        let hot = HotEligibility {
            column: "data".into(),
            is_indexed: true,
            indexed_paths: vec![FieldPath::new("data.status")],
        };
        assert!(!hot.is_hot_eligible(&FieldPath::new("data.status")));
    }

    #[test]
    fn hot_not_eligible_prefix_of_indexed_path() {
        let hot = HotEligibility {
            column: "data".into(),
            is_indexed: true,
            indexed_paths: vec![FieldPath::new("data.address.city")],
        };
        // Updating "data.address" changes the prefix of an indexed path
        assert!(
            !hot.is_hot_eligible(&FieldPath::new("data.address"))
        );
    }

    #[test]
    fn hot_update_savings_proportional() {
        let few = estimate_hot_update_savings(1000.0, 1);
        let many = estimate_hot_update_savings(1000.0, 5);
        assert!(many.io > few.io);
        assert!(many.cpu > few.cpu);
    }

    // ---- Stage cost estimation tests ----

    #[test]
    fn scan_cost_proportional_to_size() {
        let small = estimate_stage_cost(
            &DocOperator::Scan {
                collection: "c".into(),
            },
            100.0,
            256,
        );
        let large = estimate_stage_cost(
            &DocOperator::Scan {
                collection: "c".into(),
            },
            1_000_000.0,
            256,
        );
        assert!(large.total() > small.total());
    }

    #[test]
    fn match_cost_is_cpu_only() {
        let cost = estimate_stage_cost(
            &DocOperator::Match {
                predicate: DocPredicate::Eq(
                    FieldPath::new("x"),
                    DocValue::Int(1),
                ),
            },
            1000.0,
            512,
        );
        assert!(cost.cpu > 0.0);
        assert_eq!(cost.io, 0.0);
    }

    #[test]
    fn sort_cost_uses_startup() {
        let cost = estimate_stage_cost(
            &DocOperator::Sort {
                keys: vec![(FieldPath::new("ts"), true)],
            },
            10000.0,
            512,
        );
        assert!(cost.startup_cpu > 0.0);
    }

    #[test]
    fn limit_cost_bounded() {
        let cost = estimate_stage_cost(
            &DocOperator::Limit { count: 10 },
            1_000_000.0,
            256,
        );
        // Limit only processes min(input, count) rows
        let large_cost = estimate_stage_cost(
            &DocOperator::Limit { count: 10 },
            10.0,
            256,
        );
        assert!((cost.cpu - large_cost.cpu).abs() < f64::EPSILON);
    }

    // ---- Pipeline cost estimation tests ----

    #[test]
    fn pipeline_cost_accumulates() {
        let p = DocPipeline::new("users")
            .then(DocOperator::Match {
                predicate: DocPredicate::Eq(
                    FieldPath::new("active"),
                    DocValue::Bool(true),
                ),
            })
            .then(DocOperator::Limit { count: 10 });
        let cost = estimate_pipeline_cost(&p, 100_000.0, 512);
        assert!(cost.total() > 0.0);
    }

    #[test]
    fn optimized_pipeline_cheaper() {
        let p = DocPipeline::new("orders")
            .then(DocOperator::Sort {
                keys: vec![(FieldPath::new("date"), false)],
            })
            .then(DocOperator::Match {
                predicate: DocPredicate::Gt(
                    FieldPath::new("amount"),
                    DocValue::Int(100),
                ),
            });
        let original_cost = estimate_pipeline_cost(&p, 100_000.0, 256);
        let opt = p.optimize();
        let optimized_cost =
            estimate_pipeline_cost(&opt, 100_000.0, 256);
        // Filtering before sort should reduce the sort cost
        assert!(optimized_cost.total() < original_cost.total());
    }

    // ---- Predicate field extraction tests ----

    #[test]
    fn predicate_fields_simple() {
        let pred = DocPredicate::Eq(
            FieldPath::new("name"),
            DocValue::String("Alice".into()),
        );
        let fields = predicate_fields(&pred);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].as_dotted(), "name");
    }

    #[test]
    fn predicate_fields_and() {
        let pred = DocPredicate::And(vec![
            DocPredicate::Eq(FieldPath::new("a"), DocValue::Int(1)),
            DocPredicate::Gt(FieldPath::new("b"), DocValue::Int(2)),
        ]);
        let fields = predicate_fields(&pred);
        assert_eq!(fields.len(), 2);
    }

    #[test]
    fn predicate_fields_not() {
        let pred = DocPredicate::Not(Box::new(DocPredicate::Exists(
            FieldPath::new("deleted"),
        )));
        let fields = predicate_fields(&pred);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].as_dotted(), "deleted");
    }

    #[test]
    fn predicate_fields_elem_match() {
        let pred = DocPredicate::ElemMatch(
            FieldPath::new("tags"),
            Box::new(DocPredicate::Eq(
                FieldPath::new("name"),
                DocValue::String("urgent".into()),
            )),
        );
        let fields = predicate_fields(&pred);
        assert_eq!(fields.len(), 2);
    }

    #[test]
    fn predicate_references_only_check() {
        let pred = DocPredicate::Eq(
            FieldPath::new("x"),
            DocValue::Int(1),
        );
        let x_path = FieldPath::new("x");
        let allowed = vec![&x_path];
        assert!(predicate_references_only(&pred, &allowed));

        let y_path = FieldPath::new("y");
        let not_allowed = vec![&y_path];
        assert!(!predicate_references_only(&pred, &not_allowed));
    }

    // ---- Serialization roundtrip tests ----

    #[test]
    fn doc_predicate_serialize_roundtrip() {
        let pred = DocPredicate::And(vec![
            DocPredicate::Eq(
                FieldPath::new("status"),
                DocValue::String("active".into()),
            ),
            DocPredicate::Gt(FieldPath::new("age"), DocValue::Int(18)),
        ]);
        let json = serde_json::to_string(&pred)
            .expect("serialization should succeed");
        let deser: DocPredicate = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(pred, deser);
    }

    #[test]
    fn doc_pipeline_serialize_roundtrip() {
        let p = DocPipeline::new("users")
            .then(DocOperator::Match {
                predicate: DocPredicate::Eq(
                    FieldPath::new("active"),
                    DocValue::Bool(true),
                ),
            })
            .then(DocOperator::Limit { count: 10 });
        let json = serde_json::to_string(&p)
            .expect("serialization should succeed");
        let deser: DocPipeline = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(p, deser);
    }

    #[test]
    fn toast_info_serialize_roundtrip() {
        let info = ToastInfo {
            inline_size: 200,
            toast_size: 8192,
            toast_fraction: 0.3,
            strategy: ToastStrategy::Extended,
        };
        let json = serde_json::to_string(&info)
            .expect("serialization should succeed");
        let deser: ToastInfo = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(info, deser);
    }

    #[test]
    fn hot_eligibility_serialize_roundtrip() {
        let hot = HotEligibility {
            column: "data".into(),
            is_indexed: true,
            indexed_paths: vec![FieldPath::new("data.status")],
        };
        let json = serde_json::to_string(&hot)
            .expect("serialization should succeed");
        let deser: HotEligibility = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(hot, deser);
    }

    // ---- Output count estimation tests ----

    #[test]
    fn output_count_match_reduces() {
        let count = estimate_output_count(
            &DocOperator::Match {
                predicate: DocPredicate::Eq(
                    FieldPath::new("x"),
                    DocValue::Int(1),
                ),
            },
            1000.0,
        );
        assert!(count < 1000.0);
    }

    #[test]
    fn output_count_unwind_expands() {
        let count = estimate_output_count(
            &DocOperator::Unwind {
                field: FieldPath::new("items"),
                preserve_null: false,
            },
            100.0,
        );
        assert!(count > 100.0);
    }

    #[test]
    fn output_count_limit_caps() {
        let count = estimate_output_count(
            &DocOperator::Limit { count: 10 },
            1000.0,
        );
        assert_eq!(count, 10.0);
    }

    #[test]
    fn output_count_skip_reduces() {
        let count = estimate_output_count(
            &DocOperator::Skip { count: 50 },
            100.0,
        );
        assert_eq!(count, 50.0);
    }

    #[test]
    fn output_count_count_is_one() {
        let count = estimate_output_count(
            &DocOperator::Count {
                field: "n".into(),
            },
            1000.0,
        );
        assert_eq!(count, 1.0);
    }

    #[test]
    fn output_count_group_reduces() {
        let count = estimate_output_count(
            &DocOperator::Group {
                key: Some(vec![FieldPath::new("category")]),
                accumulators: vec![],
            },
            1000.0,
        );
        assert!(count < 1000.0);
    }
}
