# Rule: Factorized Join Representation

**Category:** experimental/wcoj
**File:** `rules/experimental/wcoj/factorized-join.rra`

## Metadata

- **ID:** `factorized-join`
- **Version:** "1.0.0"
- **Databases:** duckdb
- **Tags:** wcoj, factorized, compact-representation, redundancy-elimination
- **Authors:** "Olteanu & Zavodny 2015", "RA Contributors"


# Factorized Join Representation

## Description

Instead of materializing the flat join result (which can be exponentially
large), Factorized Joins represent the output as a factorized d-representation
that avoids redundancy. The key idea is that join results have structure:
values repeat across tuples in predictable patterns. A factorized
representation stores each value once and uses a tree structure to encode
the Cartesian product structure implicitly.

**When to apply**: Multi-way joins where the flat output is much larger
than the factorized representation. Common when joins produce many-to-many
results with repeated attribute values, or when downstream operations
(aggregations, projections) can operate directly on factorized form.

**Why it works**: A flat join result of size M can often be represented
in O(N^fhtw) space where fhtw is the fractional hypertree width (fhtw <= rho*).
Downstream aggregations and projections can operate on the factorized form
without ever materializing the flat result.

## Relational Algebra

```algebra
join[R.a=S.a, S.b=T.b](R(a,x), S(a,b), T(b,y))
  -> factorized_join(
       tree: a -> [R.x, S.b -> [T.y]],
       relations: {R(a,x), S(a,b), T(b,y)}
     )

-- Flat result size: |R| * |S| * |T| (worst case)
-- Factorized size: |domain(a)| * (|R.x| + |domain(b)| * |T.y|)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("factorized-join";
    "(join ?pred1 (join ?pred2 ?r1 ?r2) ?r3)" =>
    "(factorized_join
       (ftree (build_factorization_tree ?r1 ?r2 ?r3
               ?pred1 ?pred2))
       (relations ?r1 ?r2 ?r3))"
    if factorized_size_smaller()
    if downstream_supports_factorized()
),
```

## Preconditions

```rust
fn applicable(
    relations: &[RelExpr],
    predicates: &[JoinPredicate],
    downstream: &[Operator],
) -> bool {
    // Compute factorized vs flat size
    let flat_size = estimate_flat_join_size(
        relations, predicates,
    );
    let fhtw = compute_fhtw(relations, predicates);
    let n_max = relations.iter()
        .map(|r| r.row_count)
        .max()
        .unwrap_or(0);
    let factorized_size = (n_max as f64).powf(fhtw);

    // Factorized form must be significantly smaller
    if factorized_size >= flat_size as f64 * 0.5 {
        return false;
    }

    // Downstream must support factorized input
    downstream.iter().all(|op| match op {
        Operator::Aggregate(_) => true,
        Operator::Project(_) => true,
        Operator::Count => true,
        _ => false,
    })
}
```

**Restrictions:**
- Downstream operators must support factorized input
- Factorization tree computation is NP-hard in general (use heuristics)
- Not beneficial when flat output is small
- Requires custom operators for factorized aggregation/projection

## Cost Model

```rust
fn estimated_benefit(
    relations: &[Statistics],
    predicates: &[JoinPredicate],
) -> f64 {
    let flat_size = estimate_flat_join_size(
        relations, predicates,
    );
    let fhtw = compute_fhtw(relations, predicates);
    let n = relations.iter()
        .map(|r| r.row_count as f64)
        .reduce(f64::max)
        .unwrap_or(1.0);

    let factorized_size = n.powf(fhtw);

    // Factorized construction cost
    let build_cost = factorized_size * 2.0;

    // Flat materialization cost
    let flat_cost = flat_size as f64 * 1.0;

    if flat_cost > build_cost {
        (flat_cost - build_cost) / flat_cost
    } else {
        0.0
    }
}
```

**Typical benefit**: 10x-1000x space reduction for queries with many-to-many
joins. Enables aggregate queries over large join results without materialization.

## Test Cases

### Positive: Many-to-many join with aggregation

```sql
SELECT a, COUNT(*)
FROM R(a, b) JOIN S(b, c) ON R.b = S.b
GROUP BY a;

-- Flat result: |R| * avg_fanout(b) * |S|
-- Factorized: for each a, store list of b values, for each b store c count
-- Aggregation on factorized form: sum counts per a
```

### Positive: Star join with wide fact table

```sql
SELECT d1.name, d2.name, SUM(f.amount)
FROM fact f
JOIN dim1 d1 ON f.k1 = d1.id
JOIN dim2 d2 ON f.k2 = d2.id
GROUP BY d1.name, d2.name;

-- Factorized: tree rooted at (d1.name, d2.name), leaves are f.amount
-- SUM computed bottom-up without materializing full join
```

### Negative: One-to-one join

```sql
SELECT * FROM users u JOIN profiles p ON u.id = p.user_id;

-- No redundancy to factor out
-- Flat = factorized, standard join is better
```

## References

**Academic papers:**
- Olteanu, Zavodny, "Size Bounds for Factorised Representations of Query Results", TODS 2015
- Bakibayev et al., "Aggregation and Ordering in Factorised Databases", VLDB 2013
- Olteanu, Schleich, "Factorized Databases", SIGMOD Record 2016

**Key insights:**
- Fractional hypertree width (fhtw) determines factorized size
- fhtw <= rho* (fractional edge cover number)
- For acyclic queries: fhtw = 1, factorized size = O(N)
- Factorized databases (FDB) enable ML directly on join results
