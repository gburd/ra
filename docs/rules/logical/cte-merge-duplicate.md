# Rule: CTE Merge Duplicate Definitions

**Category:** logical/cte-optimization
**File:** `rules/logical/cte-optimization/cte-merge-duplicate.rra`

## Metadata

- **ID:** `cte-merge-duplicate`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, oracle
- **Tags:** cte, deduplication, merge, common-subexpression
- **Authors:** "RA Contributors"


# CTE Merge Duplicate Definitions

## Description

Detects and merges CTEs with identical definitions. When two CTEs compute the same query, one can be eliminated and references redirected.

**When to apply**: Multiple CTEs have structurally identical definitions.

**Why it works**: Avoids redundant computation and materialization.

## Relational Algebra

```algebra
CTE[a, def1](CTE[b, def2](body))
  -> CTE[a, def1](body[b := a])
  where def1 $\equiv$ def2
```

## Implementation

```rust
rw!("cte-merge-duplicate";
    "(cte ?a ?def (cte ?b ?def ?body))" =>
    "(cte ?a ?def (substitute ?b ?a ?body))"
),
```

## Cost Model

```rust
fn benefit(def_cost: f64) -> f64 {
    def_cost / (2.0 * def_cost)  // Eliminate half the computation
}
```

**Typical benefit**: 20-60% when duplicate CTEs exist

## Test Cases

### Positive: Identical CTE definitions

```sql
WITH
  a AS (SELECT dept_id, AVG(salary) FROM emp GROUP BY dept_id),
  b AS (SELECT dept_id, AVG(salary) FROM emp GROUP BY dept_id)
SELECT * FROM a JOIN b ON a.dept_id <> b.dept_id;

-- Merge b into a
```

### Negative: Different definitions

```sql
WITH
  a AS (SELECT * FROM users WHERE active),
  b AS (SELECT * FROM users WHERE premium)
SELECT * FROM a JOIN b ON a.id = b.id;

-- Different predicates; cannot merge
```

## References

- PostgreSQL: Common subexpression elimination
- Oracle: CTE deduplication
