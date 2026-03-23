# Rule: Semi-Join Reduction Programs

**Category:** experimental/semantic
**File:** `rules/experimental/semantic/semijoin-reduction.rra`

## Metadata

- **ID:** `semijoin-reduction`
- **Version:** "1.0.0"
- **Databases:** postgresql, cockroachdb, duckdb
- **Tags:** semantic, semi-join, reduction, distributed, bloom-filter
- **Authors:** "Bernstein & Chiu 1981", "Stocker et al. 2001", "RA Contributors"


# Semi-Join Reduction Programs

## Description

Replaces full joins with a sequence of semi-join reductions that pre-filter
relations before the actual join. A semi-join R $\ltimes$ S returns only the rows
of R that have a matching row in S, without producing the join result.
By applying semi-joins first, each relation is reduced to only the rows
that will contribute to the final join, dramatically reducing the cost
of the subsequent full join.

**When to apply**: Multi-way joins in distributed settings where shipping
reduced relations is cheaper than shipping full relations. Also beneficial
in single-node settings when semi-joins use bloom filters to reduce probe
input to hash joins.

**Why it works**: A semi-join R $\ltimes$ S ships only the distinct join key
values of S (or a bloom filter), which is much smaller than S itself.
Applying this to R filters out non-matching rows early. In a multi-way
join, a sequence of semi-joins can cascade reductions across all relations.

## Relational Algebra

```algebra
R join[R.a = S.a] S join[S.b = T.b] T
  -> (R $\ltimes$[a] S') join[R.a = S'.a] S' join[S'.b = T'.b] T'
  where S' = S $\ltimes$[b] T, T' = T $\ltimes$[b] S

-- Bloom filter variant:
R join[R.a = S.a] S
  -> R bloom_filter[R.a, bf(S.a)] join[R.a = S.a] S
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("semijoin-reduction";
    "(join ?p1 ?r (join ?p2 ?s ?t))" =>
    "(join ?p1
       (semi_join ?p1 ?r ?s)
       (join ?p2
         (semi_join ?p2 ?s ?t)
         (semi_join (swap_pred ?p2) ?t ?s)))"
    if multi_way_join_reducible()
    if estimated_reduction_ratio() > 0.3
),

rw!("bloom-filter-reduction";
    "(join ?pred ?left ?right)" =>
    "(join ?pred
       (bloom_filter_probe ?pred ?left
         (bloom_filter_build ?pred ?right))
       ?right)"
    if bloom_filter_beneficial("?left", "?right")
),
```

## Preconditions

```rust
fn applicable(
    relations: &[RelExpr],
    predicates: &[JoinPredicate],
) -> bool {
    // At least 2 joins (semi-join reduction sequence)
    if predicates.len() < 2 {
        return false;
    }

    // Estimate reduction ratio (fraction of rows eliminated)
    let reduction = estimate_reduction_ratio(
        relations, predicates,
    );

    // Semi-join reduction must eliminate significant rows
    reduction > 0.3
}

fn estimate_reduction_ratio(
    relations: &[RelExpr],
    predicates: &[JoinPredicate],
) -> f64 {
    // For each relation, estimate how many rows survive
    // after all applicable semi-joins
    let mut total_original = 0.0;
    let mut total_reduced = 0.0;

    for rel in relations {
        let original = rel.estimated_rows();
        let reduced = estimate_after_semijoin(
            rel, relations, predicates,
        );
        total_original += original;
        total_reduced += reduced;
    }

    1.0 - (total_reduced / total_original)
}
```

**Restrictions:**
- Semi-join computation has overhead (hash/bloom filter construction)
- Reduction ratio must be significant to overcome overhead
- Optimal semi-join program selection is NP-hard (use heuristic)
- Bloom filters have false positive rate (1-5% typical)

## Cost Model

```rust
fn estimated_benefit(
    relations: &[Statistics],
    predicates: &[JoinPredicate],
) -> f64 {
    // Cost of semi-join reductions
    let semijoin_cost: f64 = predicates.iter()
        .map(|p| {
            let build_rows = p.smaller_side_rows(relations);
            build_rows * 0.5 // Bloom filter build
        })
        .sum();

    // Original join cost (full data)
    let original_cost = estimate_join_cost(relations, predicates);

    // Reduced join cost (after semi-join filtering)
    let reduced_relations = apply_reductions(
        relations, predicates,
    );
    let reduced_cost = estimate_join_cost(
        &reduced_relations, predicates,
    );

    let total_with_semijoin = semijoin_cost + reduced_cost;

    if original_cost > total_with_semijoin {
        (original_cost - total_with_semijoin) / original_cost
    } else {
        0.0
    }
}
```

**Typical benefit**: 2x-10x for distributed joins where network transfer
dominates. 20-50% for single-node bloom filter reduction.

## Test Cases

### Positive: Distributed multi-way join

```sql
-- Three tables on different nodes
SELECT *
FROM orders o        -- Node 1: 100M rows
JOIN customers c     -- Node 2: 10M rows
  ON o.cust_id = c.id
JOIN products p      -- Node 3: 1M rows
  ON o.prod_id = p.id
WHERE p.category = 'electronics';

-- Semi-join program:
-- 1. Ship bloom filter of p.id (where category='electronics') to Node 1
-- 2. Filter orders using bloom filter (reduces 100M -> 5M)
-- 3. Ship bloom filter of filtered orders.cust_id to Node 2
-- 4. Filter customers (reduces 10M -> 2M)
-- 5. Perform full join on reduced data
```

### Positive: Bloom filter before hash join

```sql
SELECT * FROM lineitem l
JOIN orders o ON l.orderkey = o.orderkey
WHERE o.orderpriority = '1-URGENT';

-- Build bloom filter on o.orderkey (where urgent)
-- Probe lineitem against bloom filter first
-- Reduces lineitem probe input by ~80%
```

### Negative: Small tables with high join selectivity

```sql
SELECT * FROM small_a a
JOIN small_b b ON a.id = b.aid;

-- Tables are small, semi-join overhead > benefit
```

## References

**Academic papers:**
- Bernstein, Chiu, "Using Semi-Joins to Solve Relational Queries", JACM 1981
- Stocker et al., "Integrating Semi-Join Reducers into State-of-the-Art Query Processors", ICDE 2001
- Zhu et al., "Looking Ahead Makes Query Plans Robust", VLDB 2017 (bloom filter runtime filters)

**Implementation:**
- Spark SQL: broadcast hash join with bloom filter
- CockroachDB: semi-join reduction in distributed planner
- DuckDB: dynamic filter pushdown (runtime bloom filters)
- Trino: dynamic filtering with bloom filters

**Key insights:**
- Bernstein-Chiu algorithm finds optimal semi-join programs for acyclic queries
- Bloom filters are the modern implementation of semi-join reduction
- Runtime filter generation (DuckDB, Spark) applies reductions dynamically
- Cascading reductions: filtering one relation enables filtering others
