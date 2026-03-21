# Rule: Nested Predicate Pushdown

**Category:** multi-model/document
**File:** `rules/multi-model/document/nested-predicate-pushdown.rra`

## Metadata

- **ID:** `nested-predicate-pushdown`
- **Version:** "1.0.0"
- **Databases:** mongodb, couchbase, cosmosdb, dynamodb
- **Tags:** document, nested, predicate, pushdown, filter
- **SQL Standard:** "mql:5"
- **Authors:** "RA Contributors"


# Nested Predicate Pushdown

## Description

Pushes predicates on nested document fields into the document scan
operator, allowing the storage engine to skip documents that do not
match at the storage layer. Document databases store hierarchical data;
filtering on nested paths (e.g., `address.city`) early avoids
deserializing and materializing entire documents.

**When to apply**: A filter references a dotted path into a nested
document field, and the storage engine supports predicate evaluation
during scan.

**Why it works**: Without pushdown, the engine scans all documents,
deserializes them, then filters. With pushdown, the storage layer
evaluates the predicate inline, skipping non-matching documents
before they enter the query pipeline.

## Relational Algebra

```algebra
sigma[doc.nested.field op value](scan(collection))
  -> scan_with_filter(collection, nested.field, op, value)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("nested-predicate-pushdown";
    "(filter ?pred (doc-scan ?collection))" =>
    "(doc-scan-filtered ?collection ?pred)"
    if is_nested_field_predicate("?pred")
),
```

## Preconditions

```rust
fn applicable(pred: &Expr, collection: &str) -> bool {
    pred.references_nested_field()
    && pred.is_deterministic()
    && storage_supports_inline_filter(collection)
}
```

**Restrictions:**
- Predicate must be on a deterministic nested path
- Array element predicates may require `$elemMatch` semantics
- Computed fields (aggregation expressions) cannot be pushed

## Cost Model

```rust
fn estimated_benefit(
    total_docs: f64,
    selectivity: f64,
    avg_doc_size_bytes: f64,
) -> f64 {
    let full_scan_cost = total_docs * avg_doc_size_bytes;
    let filtered_cost =
        total_docs * PREDICATE_EVAL_COST
        + total_docs * selectivity * avg_doc_size_bytes;
    (full_scan_cost - filtered_cost) / full_scan_cost
}
```

**Typical benefit**: 0.5-0.95 for selective predicates on large documents.

## Test Cases

```javascript
// Positive: filter on nested field
db.users.find({ "address.city": "Seattle" });
// Pushes address.city predicate into scan

// Positive: deep nesting
db.orders.find({ "items.product.category": "electronics" });
// Pushes nested path predicate into scan

// Negative: computed expression
db.users.find({ $expr: { $gt: [{ $size: "$orders" }, 5] } });
// Cannot push: $size is a computed expression
```

## References

MongoDB: src/mongo/db/query/query_planner.cpp - QueryPlanner::plan()
CouchDB: src/mango/src/mango_selector.erl
Cattell "Scalable SQL and NoSQL Data Stores" (SIGMOD Record 2010)
