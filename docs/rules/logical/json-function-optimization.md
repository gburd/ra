# Rule: JSON Function Optimization

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/json-function-optimization.rra`

## Metadata

- **ID:** `json-function-optimization`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, duckdb
- **Tags:** function, json, jsonb, extraction, optimization
- **Authors:** "RA Contributors"


# JSON Function Optimization

## Description

Optimizes chains of JSON function calls by combining nested extractions
into a single path lookup, eliminating redundant parsing, and matching
JSON predicates to GIN indexes.

**When to apply**: Multiple JSON extractions on the same document, nested
JSON function calls that can be collapsed, or JSON predicates that match
a GIN index on the column.

**Why it works**: Each JSON extraction parses the document (or traverses
its binary representation). Combining nested extractions into a single
path avoids repeated traversal. GIN indexes on JSONB columns enable
index scans instead of sequential scans for containment checks.

## Relational Algebra

```algebra
-- Collapse nested extractions
pi[json_extract(json_extract(doc, '$.a'), '$.b')](R)
  -> pi[json_extract(doc, '$.a.b')](R)

-- Match GIN index for containment
sigma[doc @> '{"status":"active"}'](R)
  -> gin_index_scan[I_gin](doc @> '{"status":"active"}')
```

## Implementation

```rust
rw!("collapse-json-path";
    "(json-extract (json-extract ?doc ?path1) ?path2)" =>
    "(json-extract ?doc (concat-path ?path1 ?path2))"
),

rw!("json-gin-index-match";
    "(filter (jsonb-contains ?col ?pattern) (scan ?table))" =>
    "(gin-index-scan ?idx ?col ?pattern)"
    if has_gin_index("?table", "?col")
),
```

## Test Cases

### Positive: Nested extraction collapse

```sql
-- Before
SELECT doc->'address'->>'city' FROM profiles;
-- After: single path extraction $.address.city
```

### Positive: GIN index for containment

```sql
-- GIN index on data column
SELECT * FROM events WHERE data @> '{"type": "click"}';
-- Uses GIN index scan
```

### Positive: Multiple extractions from same document

```sql
SELECT doc->>'name', doc->>'email', doc->>'phone' FROM contacts;
-- Parse document once, extract all three fields
```

### Negative: Dynamic path

```sql
SELECT doc->>column_name FROM t;
-- Path is not a constant; cannot optimize at plan time
```

## References

- PostgreSQL: JSONB indexing with GIN
- MySQL: JSON indexing via generated columns
- DuckDB: Native JSON extraction optimization
