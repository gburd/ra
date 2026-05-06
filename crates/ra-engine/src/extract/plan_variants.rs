//! Plan variant generation for multi-candidate neural re-ranking.
//!
//! `extract_best_with_neural` extracts a single best plan from the e-graph,
//! but the neural model may predict a different plan structure to be cheaper.
//! This module generates lightweight plan *perturbations* that represent
//! physically different execution strategies without re-running the e-graph.
//!
//! # Generated variants
//!
//! 1. **Original** — the plan extracted by `IntegratedCostFn` (always included)
//! 2. **Swapped joins** — outermost join has left/right children swapped;
//!    changes the probe/build side for hash joins
//! 3. **Join order reversal** — for plans with ≥2 joins, reverses the
//!    associativity of the leftmost pair; tests right-deep vs left-deep shape
//!
//! # Scoring
//!
//! [`NeuralPlanScorer::score`] is called for each variant.  The variant with
//! the lowest neural cost is selected when the model confidence exceeds
//! `MIN_CONFIDENCE_FOR_VARIANT_SELECTION` (default 0.3).  Below this threshold
//! the original plan is returned unchanged to avoid regressing on untrained queries.

use ra_core::algebra::{JoinType, RelExpr};

/// Minimum confidence before neural variants replace the standard plan.
pub const MIN_CONFIDENCE_FOR_VARIANT_SELECTION: f32 = 0.3;

/// A plan candidate with its source tag.
#[derive(Debug, Clone)]
pub struct PlanCandidate {
    /// The plan as a `RelExpr` tree.
    pub plan: RelExpr,
    /// Human-readable tag describing how this variant was generated.
    pub source: &'static str,
}

/// Generate up to 3 plan variants from a base `RelExpr`.
///
/// Always includes the original plan.  Additional variants are added only
/// when the plan contains join nodes.
pub fn generate_variants(base: &RelExpr) -> Vec<PlanCandidate> {
    let mut variants = vec![PlanCandidate { plan: base.clone(), source: "integrated" }];

    // Variant 2: swap outermost join children
    if let Some(swapped) = swap_outermost_join(base) {
        variants.push(PlanCandidate { plan: swapped, source: "join_swapped" });
    }

    // Variant 3: reverse the leftmost join pair (right-deep alternative)
    if let Some(reversed) = reverse_left_join_pair(base) {
        variants.push(PlanCandidate { plan: reversed, source: "join_order_reversed" });
    }

    variants
}

/// Swap the left and right children of the outermost join node.
///
/// This changes which table is used as the build side in a hash join,
/// which can be significant when table sizes differ greatly.
pub fn swap_outermost_join(expr: &RelExpr) -> Option<RelExpr> {
    match expr {
        RelExpr::Join { join_type, condition, left, right } => {
            // Only swap commutative joins (inner and full outer)
            if matches!(join_type, JoinType::Inner | JoinType::FullOuter) {
                Some(RelExpr::Join {
                    join_type: *join_type,
                    condition: condition.clone(),
                    left: right.clone(),
                    right: left.clone(),
                })
            } else {
                None
            }
        }
        // Propagate through unary operators to find the outermost join
        RelExpr::Sort { keys, input } => {
            swap_outermost_join(input).map(|swapped| RelExpr::Sort {
                keys: keys.clone(),
                input: Box::new(swapped),
            })
        }
        RelExpr::Limit { count, offset, input } => {
            swap_outermost_join(input).map(|swapped| RelExpr::Limit {
                count: *count,
                offset: *offset,
                input: Box::new(swapped),
            })
        }
        RelExpr::Project { columns, input } => {
            swap_outermost_join(input).map(|swapped| RelExpr::Project {
                columns: columns.clone(),
                input: Box::new(swapped),
            })
        }
        RelExpr::Filter { predicate, input } => {
            swap_outermost_join(input).map(|swapped| RelExpr::Filter {
                predicate: predicate.clone(),
                input: Box::new(swapped),
            })
        }
        RelExpr::Aggregate { group_by, aggregates, input } => {
            swap_outermost_join(input).map(|swapped| RelExpr::Aggregate {
                group_by: group_by.clone(),
                aggregates: aggregates.clone(),
                input: Box::new(swapped),
            })
        }
        _ => None,
    }
}

/// Reverse the associativity of the leftmost join pair.
///
/// Transforms `(A ⋈ B) ⋈ C` → `A ⋈ (B ⋈ C)`, or vice versa.
/// Only applied when both join types are Inner (associativity is valid).
pub fn reverse_left_join_pair(expr: &RelExpr) -> Option<RelExpr> {
    // Look for the pattern: Join(Join(A, B, cond_ab), C, cond_bc)
    if let RelExpr::Join { join_type: outer_type, condition: outer_cond, left, right } = expr {
        if !matches!(outer_type, JoinType::Inner) {
            return None;
        }
        if let RelExpr::Join {
            join_type: inner_type,
            condition: inner_cond,
            left: a,
            right: b,
        } = left.as_ref()
        {
            if !matches!(inner_type, JoinType::Inner) {
                return None;
            }
            let c = right;
            // Rewrite to: A ⋈ (B ⋈ C)
            let new_right = RelExpr::Join {
                join_type: JoinType::Inner,
                condition: outer_cond.clone(), // approximate: reuse condition
                left: b.clone(),
                right: c.clone(),
            };
            return Some(RelExpr::Join {
                join_type: JoinType::Inner,
                condition: inner_cond.clone(),
                left: a.clone(),
                right: Box::new(new_right),
            });
        }
    }
    // Propagate through unary operators
    match expr {
        RelExpr::Sort { keys, input } => {
            reverse_left_join_pair(input).map(|r| RelExpr::Sort {
                keys: keys.clone(),
                input: Box::new(r),
            })
        }
        RelExpr::Limit { count, offset, input } => {
            reverse_left_join_pair(input).map(|r| RelExpr::Limit {
                count: *count,
                offset: *offset,
                input: Box::new(r),
            })
        }
        RelExpr::Project { columns, input } => {
            reverse_left_join_pair(input).map(|r| RelExpr::Project {
                columns: columns.clone(),
                input: Box::new(r),
            })
        }
        RelExpr::Filter { predicate, input } => {
            reverse_left_join_pair(input).map(|r| RelExpr::Filter {
                predicate: predicate.clone(),
                input: Box::new(r),
            })
        }
        RelExpr::Aggregate { group_by, aggregates, input } => {
            reverse_left_join_pair(input).map(|r| RelExpr::Aggregate {
                group_by: group_by.clone(),
                aggregates: aggregates.clone(),
                input: Box::new(r),
            })
        }
        _ => None,
    }
}

/// Select the best candidate based on neural scores.
///
/// Returns `(best_plan_index, neural_cost)`.
/// Falls back to the first candidate (original) when confidence is too low
/// or all variants score identically.
pub fn select_best_by_neural(
    candidates: &[PlanCandidate],
    scorer: &crate::extract::neural_cost::NeuralPlanScorer,
) -> (usize, f64) {
    assert!(!candidates.is_empty(), "must have at least one candidate");

    let scored: Vec<(f64, f32)> =
        candidates.iter().map(|c| scorer.score(&c.plan)).collect();

    // If confidence is too low, return the original (index 0)
    let max_confidence = scored.iter().map(|(_, c)| *c).fold(0.0_f32, f32::max);
    if max_confidence < MIN_CONFIDENCE_FOR_VARIANT_SELECTION {
        return (0_usize, scored[0].0);
    }

    // Return the index with the lowest neural cost
    let best_idx: usize = scored
        .iter()
        .enumerate()
        .min_by(|(_, (a, _)), (_, (b, _))| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0);

    (best_idx, scored[best_idx].0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{ColumnRef, Const, Expr};

    fn scan(name: &str) -> RelExpr {
        RelExpr::Scan { table: name.to_string(), alias: None }
    }

    fn inner_join(left: RelExpr, right: RelExpr) -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn test_generate_variants_single_scan() {
        let plan = scan("orders");
        let variants = generate_variants(&plan);
        // No joins → only original
        assert_eq!(variants.len(), 1);
        assert_eq!(variants[0].source, "integrated");
    }

    #[test]
    fn test_generate_variants_single_join() {
        let plan = inner_join(scan("orders"), scan("customers"));
        let variants = generate_variants(&plan);
        // Original + swapped = 2 (no pair to reverse with only 1 join)
        assert!(variants.len() >= 2);
        assert_eq!(variants[0].source, "integrated");
        assert!(variants.iter().any(|v| v.source == "join_swapped"));
    }

    #[test]
    fn test_generate_variants_two_joins() {
        let plan = inner_join(
            inner_join(scan("orders"), scan("customers")),
            scan("products"),
        );
        let variants = generate_variants(&plan);
        // Original + swapped + reversed = 3
        assert_eq!(variants.len(), 3);
        assert!(variants.iter().any(|v| v.source == "join_swapped"));
        assert!(variants.iter().any(|v| v.source == "join_order_reversed"));
    }

    #[test]
    fn test_swap_preserves_plan_structure() {
        let plan = inner_join(scan("a"), scan("b"));
        let swapped = swap_outermost_join(&plan).expect("should swap");
        if let RelExpr::Join { left, right, .. } = &swapped {
            if let (RelExpr::Scan { table: lt, .. }, RelExpr::Scan { table: rt, .. }) =
                (left.as_ref(), right.as_ref())
            {
                assert_eq!(lt, "b");
                assert_eq!(rt, "a");
            }
        }
    }

    #[test]
    fn test_reverse_join_pair_depth_two() {
        let plan = inner_join(inner_join(scan("a"), scan("b")), scan("c"));
        let reversed = reverse_left_join_pair(&plan).expect("should reverse");
        // Should produce a ⋈ (b ⋈ c) structure
        if let RelExpr::Join { left, right, .. } = &reversed {
            assert!(matches!(left.as_ref(), RelExpr::Scan { table, .. } if table == "a"));
            assert!(matches!(right.as_ref(), RelExpr::Join { .. }));
        }
    }

    #[test]
    fn test_left_join_not_swapped() {
        let plan = RelExpr::Join {
            join_type: JoinType::LeftOuter,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
        };
        // Left outer joins should NOT be swapped (not commutative)
        assert!(swap_outermost_join(&plan).is_none());
    }

    #[test]
    fn test_generate_variants_through_sort() {
        use ra_core::algebra::{NullOrdering, SortDirection, SortKey};
        let plan = RelExpr::Sort {
            keys: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("id")),
                direction: SortDirection::Asc,
                nulls: NullOrdering::Last,
            }],
            input: Box::new(inner_join(scan("orders"), scan("customers"))),
        };
        let variants = generate_variants(&plan);
        // Should find the join through the Sort wrapper
        assert!(variants.len() >= 2);
    }
}
