# Rule: Shard Index Predicate Prefix Addition

**Category:** database-specific/tidb
**File:** `rules/database-specific/tidb/predicate-push-down-shard-index.rra`

## Metadata

- **ID:** `tidb-predicate-push-down-shard-index`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** distributed, predicate, pushdown, shard, index, tidb
- **Authors:** "RA Contributors"


# Shard Index Predicate Prefix Addition

## Description

Automatically adds a `tidb_shard(x) = val` predicate prefix when the
query filters on a column that is part of a shard index. TiDB uses
shard indexes to distribute hotspot writes across ranges. The shard
function computes a hash prefix that becomes the first column of the
index. This rule infers the shard value from the equality predicate
and adds it, enabling the optimizer to use the shard index.

**When to apply**: A table has a unique shard index (where the first
column is `tidb_shard(col)`), and the query has an equality predicate
on `col`. The shard value can be computed at plan time from the constant
in the equality predicate.

**Why it works**: Without the shard prefix, the optimizer cannot use
the shard index because the first column of the index is
`tidb_shard(col)`, not `col` itself. By computing and adding the shard
value, the optimizer can generate a point lookup on the shard index,
which is a single-range read instead of a full index scan.

## Relational Algebra

```algebra
sigma[a = 10](Scan(T))
  -> sigma[tidb_shard(a) = hash(10) AND a = 10](Scan(T))
  where T has index (tidb_shard(a), a)

sigma[a IN (10, 20)](Scan(T))
  -> sigma[(tidb_shard(a) = hash(10) AND a = 10)
        OR (tidb_shard(a) = hash(20) AND a = 20)](Scan(T))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("add-shard-prefix-eq";
    "(filter [(eq ?col ?val)] (scan ?table))" =>
    "(filter [(eq (tidb_shard ?col) (shard_hash ?val))
              (eq ?col ?val)]
        (scan ?table))"
    if has_shard_index("?table", "?col")
    if is_constant("?val")
),

rw!("add-shard-prefix-in";
    "(filter [(in ?col ?vals)] (scan ?table))" =>
    "(filter (expand_shard_in ?col ?vals)
        (scan ?table))"
    if has_shard_index("?table", "?col")
    if all_constants("?vals")
),
```

## Preconditions

```rust
fn applicable(
    table: &DataSource,
    conds: &[Expression],
) -> bool {
    // Table must have a shard index
    table.has_expr_prefix_unique_key()
    // Conditions must contain equality or IN on the sharded column
    && conds.iter().any(|c| {
        c.is_equality_on(table.shard_column())
        || c.is_in_list_on(table.shard_column())
    })
}
```

**Restrictions:**
- Only works with equality predicates and IN lists (not ranges)
- The shard function value must be computable at plan time
  (requires constant values, not column references)
- If the predicate is a range (a > 10), the shard prefix cannot
  be inferred (the hash is not order-preserving)
- Only applies to unique shard indexes (tidb_shard prefix)

## Cost Model

```rust
fn shard_prefix_benefit(
    total_rows: f64,
    matching_rows: f64,
    full_scan_cost: f64,
    point_lookup_cost: f64,
) -> f64 {
    // Without shard prefix: cannot use shard index, full scan
    let without = total_rows * full_scan_cost;
    // With shard prefix: point lookup on shard index
    let with = matching_rows * point_lookup_cost;
    without - with
}
```

## Test Cases

```sql
-- Positive: equality on sharded column
-- CREATE TABLE t (a INT, UNIQUE INDEX uk(tidb_shard(a), a))
SELECT * FROM t WHERE a = 10;

-- Rewritten to:
-- SELECT * FROM t WHERE tidb_shard(a) = <hash(10)> AND a = 10
-- Enables index point lookup on uk
```

```sql
-- Positive: IN list on sharded column
SELECT * FROM t WHERE a IN (10, 20, 30);

-- Rewritten to:
-- SELECT * FROM t WHERE
--   (tidb_shard(a) = hash(10) AND a = 10) OR
--   (tidb_shard(a) = hash(20) AND a = 20) OR
--   (tidb_shard(a) = hash(30) AND a = 30)
```

```sql
-- Negative: range predicate on sharded column
SELECT * FROM t WHERE a > 10;
-- Cannot compute shard prefix for range; hash not order-preserving
```

```sql
-- Negative: non-constant predicate
SELECT * FROM t1 JOIN t2 ON t1.a = t2.b;
-- t2.b is not a constant; shard value unknown at plan time
```

## References

TiDB: pkg/planner/core/rule_predicate_push_down.go:54 - addPrefix4ShardIndexes (commit e2184a2)
TiDB: pkg/planner/core/rule_predicate_push_down.go:62 - addExprPrefixCond
TiDB docs: "Shard Index" for hotspot avoidance
