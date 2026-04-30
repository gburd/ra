# Rule: Full-Text Index for LIKE Patterns

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/full-text-index-for-like.rra`

## Metadata

- **ID:** `full-text-index-for-like`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, mssql
- **Tags:** index, full-text, like, pattern-matching, search
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter (like ?col ?pattern) (scan ?table))"
    description: "LIKE predicate eligible for full-text index"
  - type: "predicate"
    condition: "has_fulltext_index(?table, ?col)"
    description: "Full-text index must exist on the column"
  - type: "capability"
    database: "current"
    requires: "fulltext_index"
    description: "Database supports full-text indexes"
```


# Full-Text Index for LIKE Patterns

## Description

Uses a full-text index instead of a sequential scan for LIKE
patterns that B-tree indexes cannot accelerate, specifically
infix patterns like `%pattern%`. Full-text indexes use inverted
indexes on tokenized text, enabling sub-linear search.

**When to apply**: A LIKE predicate uses a leading wildcard
(`%pattern%` or `%pattern`) and a full-text index exists on the
target column.

**Why it works**: B-tree indexes require a fixed prefix for range
scans, so `%pattern%` triggers a full table scan. Full-text indexes
tokenize text and build inverted indexes, enabling word-level
lookups regardless of position.

## Relational Algebra

```algebra
filter[body LIKE '%optimization%'](scan[T])
  -> full_text_search[I](body, 'optimization')
  where I is a full-text index on column body
```

## Implementation

```rust
rw!("full-text-index-for-like";
    "(filter (like ?col (pattern-infix ?term)) (scan ?table))" =>
    "(full-text-search ?index ?col ?term)"
    if has_fulltext_index_on("?table", "?col")
),
```

## Cost Model

```rust
fn cost(
    matching_docs: u64,
    total_docs: u64,
    index_entries: u64,
) -> f64 {
    let index_lookup = (index_entries as f64).log2();
    let doc_fetch = matching_docs as f64 * 1.5; // Random I/O
    index_lookup + doc_fetch
}

fn benefit_over_full_scan(
    matching_docs: u64,
    total_docs: u64,
) -> f64 {
    1.0 - (matching_docs as f64 / total_docs as f64)
}
```

**Typical benefit**: 50-99% for text search on large tables.

## Test Cases

### Positive: Infix LIKE with full-text index

```sql
CREATE FULLTEXT INDEX idx_articles_body ON articles(body);

SELECT * FROM articles WHERE body LIKE '%database optimization%';

-- Full-text index lookup instead of sequential scan
```

### Positive: PostgreSQL tsvector search

```sql
CREATE INDEX idx_docs_search ON documents USING GIN(to_tsvector('english', content));

SELECT * FROM documents
WHERE to_tsvector('english', content) @@ to_tsquery('optimization');

-- GIN index on tsvector
```

### Negative: Prefix LIKE (B-tree handles this)

```sql
SELECT * FROM users WHERE name LIKE 'John%';

-- B-tree index on name handles prefix patterns efficiently
```

## References

- PostgreSQL: Full-text search with GIN indexes
- MySQL: FULLTEXT indexes on InnoDB/MyISAM
- IndexType::FullText in ra-stats-advanced/src/index_types.rs
