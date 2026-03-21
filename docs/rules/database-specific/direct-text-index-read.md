# Rule: Direct Read from Text/Full-Text Index

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/direct-text-index-read.rra`

## Metadata

- **ID:** `clickhouse-direct-text-index-read`
- **Version:** 1.0.0
- **Databases:** clickhouse
- **Tags:** database-specific, clickhouse, full-text, text-index, search, mergetree
- **Authors:** "RA Contributors"


# Direct Read from Text/Full-Text Index

## Description

Reads results directly from a full-text or specialized text index (tokenbf_v1, ngrambf_v1, or experimental full-text indexes) when the query consists only of text search predicates. Avoids reading the base table entirely.

**When to apply**: Query with text search predicates (LIKE, hasToken, etc.) on columns with text indexes, where no other columns are needed.

**Why it works**: Text indexes store document IDs that match search terms. For simple existence checks or counting, reading only the index is sufficient.

**Database version**: ClickHouse v22.3+

## Relational Algebra

```algebra
Project[count(*)](Filter[hasToken(text_col, 'term')](Scan[MergeTree](T)))
  -> CountFromTextIndex('term', text_index)
  where only_uses_text_index_predicates
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("clickhouse-direct-text-index-read";
    "(agg count (filter ?text_pred (scan ?table ?props)))" =>
    "(count_from_text_index ?text_pred (find_text_index ?table ?text_pred))"
    if is_database("clickhouse")
    if has_text_index("?table", "?text_pred")
    if only_text_predicates("?text_pred")
),
```

## Preconditions

```rust
fn applicable(
    filter: &Expr,
    table: &TableRef,
) -> bool {
    // Extract text predicates
    let text_preds = extract_text_predicates(filter);
    if text_preds.is_empty() {
        return false;
    }

    // All predicates must be on indexed text columns
    text_preds.iter().all(|pred| {
        table.has_text_index_on(pred.column())
    })
}
```

**Restrictions:**
- Only applies to ClickHouse with text indexes (tokenbf_v1, ngrambf_v1)
- Query must use only text search predicates
- Most beneficial for COUNT queries
- Experimental for full-text search indexes

## Cost Model

```rust
fn estimated_benefit(
    table_size: f64,
    index_size: f64,
) -> f64 {
    let table_scan_cost = table_size;
    let index_only_cost = index_size;
    (table_scan_cost - index_only_cost) / table_scan_cost
}
```

**Typical benefit**: 60-95% for pure text search queries

## Test Cases

```sql
CREATE TABLE docs (
  id UInt64,
  content String,
  INDEX content_idx content TYPE tokenbf_v1(32768, 3, 0) GRANULARITY 1
) ENGINE = MergeTree()
ORDER BY id;

-- Direct index read
SELECT count() FROM docs
WHERE hasToken(content, 'important');

-- Index returns matching granules, no need to read content column
```

## References

**Source code:**
- ClickHouse: `src/Processors/QueryPlan/Optimizations/optimizeDirectReadFromTextIndex.cpp`
  - Commit: 35f2d31186cca2f8c50f7ba4bd93817da490da85
