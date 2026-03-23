# RFC 0031: Top-N Sort and Empty Result Propagation

- Start Date: 2026-03-21
- Author: RA Contributors
- Status: Accepted
- Tracking Issue: TBD

## Summary

Two complementary micro-optimizations: (1) replace Sort + Limit with a heap-based Top-N operator for O(n log k) performance instead of O(n log n), and (2) propagate empty results upward through the plan tree when inputs are provably empty.

## Motivation

**Top-N Sort**: `ORDER BY ... LIMIT k` is one of the most common query patterns (top-k queries, pagination, leaderboards). A full sort followed by limit wastes both time (O(n log n) vs O(n log k)) and memory (O(n) vs O(k)).

**Empty Result Propagation**: When a filter has contradictory predicates (`WHERE false`, `x > 5 AND x < 3`) or an input table has 0 rows, the entire subtree can be eliminated. Without this, the optimizer generates and executes plans for queries that provably return no rows.

## Guide-level explanation

### Top-N Sort

```sql
-- Common pattern: get the 10 most recent orders
SELECT * FROM orders ORDER BY created_at DESC LIMIT 10;
-- Optimizer replaces Sort(Limit(10)) with TopN(k=10)
-- Uses a min-heap: O(n log 10) instead of O(n log n)
```

### Empty Result Propagation

```sql
-- Contradictory predicate
SELECT * FROM t WHERE x > 5 AND x < 3;
-- Optimizer detects contradiction, replaces with EmptyRelation

-- Empty join input
SELECT * FROM empty_table t1 JOIN large_table t2 ON t1.id = t2.id;
-- Inner join with empty input -> entire join produces empty result
```

## Reference-level explanation

### Top-N Sort Implementation

```rust
pub struct TopN {
    pub k: usize,
    pub sort_keys: Vec<SortKey>,
    pub child: Box<PlanNode>,
}
```

Rule: `sort-limit-to-topn`
- Pattern: `Limit(k, Sort(keys, child))`
- Result: `TopN(k, keys, child)`
- Cost: `n * log2(k) * cpu_operator_cost` (vs `n * log2(n)` for full sort)
- Memory: `k * tuple_size` (vs `n * tuple_size`)

### Empty Result Propagation

Rule: `propagate-empty-relation`

Propagation rules:
- `Filter(false, X)` -> `Empty`
- `Empty INNER JOIN Y` -> `Empty`
- `X INNER JOIN Empty` -> `Empty`
- `Empty SEMI JOIN Y` -> `Empty`
- `X SEMI JOIN Empty` -> `Empty`
- `Empty UNION ALL Empty` -> `Empty`
- `Empty CROSS JOIN Y` -> `Empty`

Non-propagating cases (preserve semantics):
- `Empty LEFT JOIN Y` -> `Empty` (preserves left side, which is empty)
- `X LEFT JOIN Empty` -> `X` with NULLs (NOT empty)
- `Empty FULL JOIN Y` -> `Y` with NULLs (NOT empty)

Contradiction detection:
- `x > a AND x < b` where `a >= b`
- `x = a AND x = b` where `a != b`
- `WHERE false`
- `WHERE NULL`

## Drawbacks

- Top-N requires a new physical operator implementation
- Contradiction detection has limits (cannot detect all contradictions)
- Empty propagation adds analysis overhead to every plan node

## Rationale and alternatives

### Why This Design?

Both are well-understood, low-risk optimizations implemented by every major database system. Top-N provides 10x+ speedup for the most common pagination pattern.

### Alternative Approaches

- **Streaming Top-N**: More complex but handles ties; future extension
- **Partial sort + limit**: Less optimal than heap-based approach
- **Semantic analysis only**: Detects contradictions but not empty base tables

## Prior art

- BusTub: `sort_limit_as_topn.cpp`
- DataFusion: `PropagateEmptyRelation` rule
- DuckDB: Top-N optimization and empty result elimination
- PostgreSQL: Limit node with Sort optimization

## Unresolved questions

- Top-N with ties (`LIMIT k WITH TIES`): handle in initial implementation or defer?
- Empty propagation through window functions
- Interaction with OFFSET (Top-N for LIMIT k OFFSET m needs k+m heap)

## Future possibilities

- Approximate Top-N for very large k values
- Empty propagation through CTEs and subqueries
- Streaming Top-N for parallel execution
