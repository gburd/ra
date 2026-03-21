# Rule: JSON Path Expression Pushdown

**Category:** logical/multi-model
**File:** `rules/logical/multi-model/json-path-pushdown.rra`

## Metadata

- **ID:** `json-path-pushdown`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, mongodb, cockroachdb, duckdb
- **Tags:** logical, multi-model, json, jsonpath, pushdown, semi-structured
- **Authors:** "PostgreSQL Team"


# JSON Path Expression Pushdown

## Description

Pushes JSON path extraction and filtering below joins and aggregations
so that only needed JSON fields are extracted and unnested. When a JSON
column contains deeply nested structures, extracting specific paths
early reduces the data volume flowing through subsequent operators.

**When to apply**: Queries accessing specific paths in JSON/JSONB columns
where the full document is not needed downstream.

## Relational Algebra

```algebra
-- Before: extract JSON after join
pi[r.id, r.doc->>'name'](R join S)

-- After: extract JSON before join, narrow projection
pi[r.id, r.name](
    pi[id, doc->>'name' AS name](R)
    join S)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("json-extraction-pushdown";
    "(project (json-extract ?doc ?path)
        (join ?cond ?json_side ?other))" =>
    "(project ?path
        (join ?cond
            (project (json-extract ?doc ?path) ?json_side)
            ?other))"
    if json_path_independent_of_join("?path", "?cond")
),

rw!("json-predicate-to-index";
    "(filter (= (json-extract ?doc ?path) ?val) (scan ?table))" =>
    "(gin-index-scan ?table ?doc ?path ?val)"
    if has_gin_index("?table", "?doc")
),
```

## Preconditions

```rust
fn applicable(query: &Query) -> bool {
    query.has_json_extraction()
        && (query.json_paths_subset_of_document()
            || query.has_gin_index_on_json())
}
```

**Restrictions:**
- GIN index support required for predicate pushdown
- JSON extraction functions must be deterministic
- JSONB is more efficient than JSON for path extraction

## Cost Model

```rust
fn estimated_benefit(
    rows: f64,
    avg_doc_size: f64,
    extracted_field_size: f64,
) -> f64 {
    rows * (avg_doc_size - extracted_field_size)
}
```

**Typical benefit**: 10-60% for wide JSON documents with narrow queries.

## Test Cases

```sql
-- Positive: extract specific field from JSON before join
SELECT o.id, o.data->>'customer_name'
FROM orders o JOIN products p ON (o.data->>'product_id')::int = p.id;
-- Push JSON extraction into orders scan

-- Positive: GIN index for JSON predicate
CREATE INDEX idx_data ON orders USING GIN(data);
SELECT * FROM orders WHERE data @> '{"status": "pending"}';
-- Uses GIN index scan

-- Negative: full document needed
SELECT o.data FROM orders o;
```

## References

- PostgreSQL: JSON Functions and Operators, GIN Indexes for JSONB
- MongoDB: Covered Queries with JSON projections
