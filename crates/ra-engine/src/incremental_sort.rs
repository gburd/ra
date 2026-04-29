//! Incremental sort optimization.
//!
//! When input data is already sorted by a prefix of the required sort
//! keys, an incremental sort only re-sorts within each prefix group.
//! This is much cheaper than a full sort:
//!
//! - Full sort:        O(n log n)
//! - Incremental sort: O(n log m), where m = average group size
//!
//! `PostgreSQL` added this in v13 and reported significant speedups for
//! queries where an index provides partial ordering.

use ra_core::algebra::{RelExpr, SortKey};

/// Result of detecting whether an input's existing ordering matches
/// a prefix of the required sort keys.
#[derive(Debug, Clone, PartialEq)]
pub struct PrefixMatch {
    /// The sort keys that the input already provides (the prefix).
    pub prefix_keys: Vec<SortKey>,
    /// The remaining sort keys that need sorting within groups.
    pub suffix_keys: Vec<SortKey>,
}

/// Detect whether the input's existing ordering forms a prefix of the
/// required sort keys.
///
/// Returns `Some(PrefixMatch)` when at least one key matches and at
/// least one key remains (otherwise there's nothing to optimize: a
/// full match means the sort is redundant, and no match means a full
/// sort is needed).
///
/// Two sort keys match when their expressions are structurally equal
/// and they share the same direction and null ordering.
#[must_use]
pub fn detect_prefix_match(
    required_keys: &[SortKey],
    input_keys: &[SortKey],
) -> Option<PrefixMatch> {
    if required_keys.is_empty() || input_keys.is_empty() {
        return None;
    }

    let prefix_len = required_keys
        .iter()
        .zip(input_keys.iter())
        .take_while(|(req, inp)| sort_keys_match(req, inp))
        .count();

    // Need at least one prefix match AND at least one remaining key.
    if prefix_len == 0 || prefix_len >= required_keys.len() {
        return None;
    }

    Some(PrefixMatch {
        prefix_keys: required_keys[..prefix_len].to_vec(),
        suffix_keys: required_keys[prefix_len..].to_vec(),
    })
}

/// Check whether two sort keys are equivalent (same expression,
/// direction, and null ordering).
fn sort_keys_match(a: &SortKey, b: &SortKey) -> bool {
    a.expr == b.expr && a.direction == b.direction && a.nulls == b.nulls
}

/// Estimate the cost of an incremental sort vs a full sort.
///
/// Returns `(incremental_cost, full_sort_cost)`. When the incremental
/// cost is lower, the optimizer should prefer the incremental sort.
///
/// # Parameters
/// - `row_count`: total number of rows in the input
/// - `prefix_ndv`: number of distinct values in the prefix columns
///   (determines the number of groups)
///
/// The model assumes uniform group sizes: each group has approximately
/// `row_count / prefix_ndv` rows.
#[must_use]
pub fn estimate_costs(row_count: f64, prefix_ndv: f64) -> IncrementalSortCost {
    let n = row_count.max(1.0);
    let groups = prefix_ndv.max(1.0).min(n);
    let avg_group_size = n / groups;

    let full_sort_cost = n * n.log2().max(1.0);

    // Each group sorted independently: groups * (m * log(m))
    let group_sort = avg_group_size * avg_group_size.log2().max(1.0);
    let incremental_cost = groups * group_sort;

    IncrementalSortCost {
        incremental_cost,
        full_sort_cost,
        estimated_groups: groups,
        avg_group_size,
    }
}

/// Cost comparison between incremental and full sort.
#[derive(Debug, Clone)]
pub struct IncrementalSortCost {
    /// Estimated cost of the incremental sort.
    pub incremental_cost: f64,
    /// Estimated cost of a full sort over the same data.
    pub full_sort_cost: f64,
    /// Estimated number of prefix groups.
    pub estimated_groups: f64,
    /// Average number of rows per group.
    pub avg_group_size: f64,
}

impl IncrementalSortCost {
    /// Whether incremental sort is cheaper than full sort.
    #[must_use]
    pub fn is_beneficial(&self) -> bool {
        self.incremental_cost < self.full_sort_cost
    }

    /// The ratio of incremental cost to full sort cost.
    /// Values < 1.0 indicate incremental sort is cheaper.
    #[must_use]
    pub fn cost_ratio(&self) -> f64 {
        if self.full_sort_cost == 0.0 {
            return 1.0;
        }
        self.incremental_cost / self.full_sort_cost
    }
}

/// Attempt to rewrite a `Sort` node into an `IncrementalSort` when
/// the input provides a partial ordering.
///
/// Returns `None` if no prefix match is found or if incremental sort
/// would not be beneficial.
///
/// # Parameters
/// - `sort_keys`: the keys the Sort operator requires
/// - `input_keys`: the keys the input already provides (from physical
///   property tracking or known index ordering)
/// - `input`: the input `RelExpr`
/// - `row_count`: estimated input row count
/// - `prefix_ndv`: estimated NDV for the prefix columns
#[must_use]
pub fn try_incremental_sort(
    sort_keys: &[SortKey],
    input_keys: &[SortKey],
    input: RelExpr,
    row_count: f64,
    prefix_ndv: f64,
) -> Option<RelExpr> {
    let prefix_match = detect_prefix_match(sort_keys, input_keys)?;

    let costs = estimate_costs(row_count, prefix_ndv);
    if !costs.is_beneficial() {
        return None;
    }

    Some(RelExpr::IncrementalSort {
        prefix_keys: prefix_match.prefix_keys,
        suffix_keys: prefix_match.suffix_keys,
        input: Box::new(input),
    })
}

#[cfg(test)]
#[expect(clippy::float_cmp, reason = "legacy allow")]
#[expect(clippy::panic, clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;
    use ra_core::algebra::{NullOrdering, SortDirection, SortKey};
    use ra_core::expr::{ColumnRef, Expr};

    fn col_key(name: &str) -> SortKey {
        SortKey {
            expr: Expr::Column(ColumnRef {
                table: None,
                column: name.to_string(),
            }),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }
    }

    fn desc_key(name: &str) -> SortKey {
        SortKey {
            expr: Expr::Column(ColumnRef {
                table: None,
                column: name.to_string(),
            }),
            direction: SortDirection::Desc,
            nulls: NullOrdering::Last,
        }
    }

    // ---- detect_prefix_match ----

    #[test]
    fn prefix_match_single_key_match() {
        let required = vec![col_key("a"), col_key("b"), col_key("c")];
        let input = vec![col_key("a")];

        let result = detect_prefix_match(&required, &input);
        assert!(result.is_some());
        let pm = result.unwrap();
        assert_eq!(pm.prefix_keys.len(), 1);
        assert_eq!(pm.suffix_keys.len(), 2);
    }

    #[test]
    fn prefix_match_two_keys() {
        let required = vec![col_key("a"), col_key("b"), col_key("c")];
        let input = vec![col_key("a"), col_key("b")];

        let result = detect_prefix_match(&required, &input);
        assert!(result.is_some());
        let pm = result.unwrap();
        assert_eq!(pm.prefix_keys.len(), 2);
        assert_eq!(pm.suffix_keys.len(), 1);
    }

    #[test]
    fn no_match_when_first_key_differs() {
        let required = vec![col_key("a"), col_key("b")];
        let input = vec![col_key("x")];

        assert!(detect_prefix_match(&required, &input).is_none());
    }

    #[test]
    fn no_match_when_direction_differs() {
        let required = vec![col_key("a"), col_key("b")];
        let input = vec![desc_key("a")];

        assert!(detect_prefix_match(&required, &input).is_none());
    }

    #[test]
    fn no_match_when_full_prefix() {
        // All keys match: sort is redundant, not incremental
        let required = vec![col_key("a"), col_key("b")];
        let input = vec![col_key("a"), col_key("b")];

        assert!(detect_prefix_match(&required, &input).is_none());
    }

    #[test]
    fn no_match_when_input_has_more() {
        // Input sorted on (a, b, c) but we only need (a, b)
        // => full match, no incremental sort needed
        let required = vec![col_key("a"), col_key("b")];
        let input = vec![col_key("a"), col_key("b"), col_key("c")];

        assert!(detect_prefix_match(&required, &input).is_none());
    }

    #[test]
    fn no_match_empty_required() {
        let input = vec![col_key("a")];
        assert!(detect_prefix_match(&[], &input).is_none());
    }

    #[test]
    fn no_match_empty_input() {
        let required = vec![col_key("a")];
        assert!(detect_prefix_match(&required, &[]).is_none());
    }

    #[test]
    fn prefix_match_preserves_key_details() {
        let required = vec![col_key("a"), desc_key("b"), col_key("c")];
        let input = vec![col_key("a")];

        let pm = detect_prefix_match(&required, &input).unwrap();
        assert_eq!(pm.prefix_keys[0].direction, SortDirection::Asc);
        assert_eq!(pm.suffix_keys[0].direction, SortDirection::Desc);
        assert_eq!(pm.suffix_keys[1].direction, SortDirection::Asc);
    }

    // ---- estimate_costs ----

    #[test]
    fn incremental_cheaper_for_many_groups() {
        // 1M rows, 10K groups => avg group size 100
        let costs = estimate_costs(1_000_000.0, 10_000.0);
        assert!(costs.is_beneficial());
        assert!(costs.cost_ratio() < 1.0);
    }

    #[test]
    fn incremental_cheaper_for_high_cardinality() {
        // 100K rows, 50K groups => avg group size 2
        let costs = estimate_costs(100_000.0, 50_000.0);
        assert!(costs.is_beneficial());
        assert!(costs.cost_ratio() < 0.5);
    }

    #[test]
    fn single_group_not_beneficial() {
        // 1M rows, 1 group => equivalent to full sort
        let costs = estimate_costs(1_000_000.0, 1.0);
        assert!(!costs.is_beneficial());
    }

    #[test]
    fn small_input_still_works() {
        let costs = estimate_costs(10.0, 5.0);
        assert!(costs.is_beneficial());
        assert!(costs.estimated_groups == 5.0);
        assert!(costs.avg_group_size == 2.0);
    }

    #[test]
    fn ndv_capped_at_row_count() {
        // NDV cannot exceed row count
        let costs = estimate_costs(100.0, 1000.0);
        assert_eq!(costs.estimated_groups, 100.0);
        assert_eq!(costs.avg_group_size, 1.0);
    }

    #[test]
    fn zero_row_count_safe() {
        let costs = estimate_costs(0.0, 0.0);
        assert!(costs.incremental_cost.is_finite());
        assert!(costs.full_sort_cost.is_finite());
    }

    #[test]
    fn cost_ratio_full_sort_zero() {
        // Edge case: 1 row, 1 group
        let costs = estimate_costs(1.0, 1.0);
        // Both should be equal (1 * log2(1) = 0 -> max(1) = 1)
        assert!(costs.cost_ratio().is_finite());
    }

    // ---- try_incremental_sort ----

    #[test]
    fn try_incremental_sort_produces_node() {
        let sort_keys = vec![col_key("a"), col_key("b"), col_key("c")];
        let input_keys = vec![col_key("a")];
        let input = RelExpr::Scan {
            table: "t".to_string(),
            alias: None,
        };

        let result = try_incremental_sort(&sort_keys, &input_keys, input, 100_000.0, 10_000.0);
        assert!(result.is_some());

        let expr = result.unwrap();
        match &expr {
            RelExpr::IncrementalSort {
                prefix_keys,
                suffix_keys,
                ..
            } => {
                assert_eq!(prefix_keys.len(), 1);
                assert_eq!(suffix_keys.len(), 2);
            }
            other => panic!("expected IncrementalSort, got {other:?}"),
        }
    }

    #[test]
    fn try_incremental_sort_none_when_no_prefix() {
        let sort_keys = vec![col_key("a"), col_key("b")];
        let input_keys = vec![col_key("x")];
        let input = RelExpr::Scan {
            table: "t".to_string(),
            alias: None,
        };

        assert!(
            try_incremental_sort(&sort_keys, &input_keys, input, 100_000.0, 10_000.0,).is_none()
        );
    }

    #[test]
    fn try_incremental_sort_none_when_single_group() {
        let sort_keys = vec![col_key("a"), col_key("b")];
        let input_keys = vec![col_key("a")];
        let input = RelExpr::Scan {
            table: "t".to_string(),
            alias: None,
        };

        // 1 group: incremental sort won't be cheaper
        assert!(try_incremental_sort(&sort_keys, &input_keys, input, 100_000.0, 1.0,).is_none());
    }

    // ---- IncrementalSortCost methods ----

    #[test]
    fn cost_ratio_less_than_one_when_beneficial() {
        let costs = estimate_costs(1_000_000.0, 10_000.0);
        assert!(costs.cost_ratio() < 1.0);
        assert!(costs.is_beneficial());
    }

    #[test]
    fn cost_ratio_monotonic_with_groups() {
        // More groups => lower cost ratio
        let few = estimate_costs(100_000.0, 10.0);
        let many = estimate_costs(100_000.0, 10_000.0);
        assert!(many.cost_ratio() < few.cost_ratio());
    }

    #[test]
    fn incremental_sort_relexpr_children() {
        let expr = RelExpr::IncrementalSort {
            prefix_keys: vec![col_key("a")],
            suffix_keys: vec![col_key("b")],
            input: Box::new(RelExpr::Scan {
                table: "t".to_string(),
                alias: None,
            }),
        };
        assert_eq!(expr.children().len(), 1);
    }

    #[test]
    fn incremental_sort_referenced_columns() {
        let expr = RelExpr::IncrementalSort {
            prefix_keys: vec![col_key("a")],
            suffix_keys: vec![col_key("b")],
            input: Box::new(RelExpr::Scan {
                table: "t".to_string(),
                alias: None,
            }),
        };
        let cols = expr.referenced_columns();
        assert_eq!(cols.len(), 2);
    }
}
