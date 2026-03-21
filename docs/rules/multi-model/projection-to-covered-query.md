# Rule: Projection to Covered Query

**Category:** multi-model/document
**File:** `rules/multi-model/document/projection-to-covered-query.rra`

## Metadata

- **ID:** `projection-to-covered-query`
- **Version:** "1.0.0"
- **Databases:** mongodb, couchbase, cosmosdb
- **Tags:** document, projection, covered, index, optimization
- **SQL Standard:** "mql:5"
- **Authors:** "RA Contributors"


# Projection to Covered Query

## Description

Converts a document query with projection into a covered query when all
projected fields are included in an existing index. A covered query
returns results directly from the index without fetching the full
document from storage, eliminating random I/O to the document store.

**When to apply**: All fields in the projection list (and the filter,
if present) are part of a compound index.

**Why it works**: Compound indexes in document databases store copies of
indexed field values in sorted order. When the query only needs those
fields, the storage engine can satisfy the query entirely from the index,
avoiding document deserialization and random disk reads.

## Relational Algebra

```algebra
pi[f1, f2, ..., fn](sigma[p](scan(collection)))
  -> index_only_scan(collection, index, f1..fn, p)
  where index covers {f1..fn} union attrs(p)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("projection-to-covered-query";
    "(project ?fields (filter ?pred (doc-scan ?coll)))" =>
    "(index-only-scan ?coll ?fields ?pred)"
    if index_covers("?coll", "?fields", "?pred")
),
```

## Preconditions

```rust
fn applicable(
    fields: &[String],
    pred: &Expr,
    collection: &str,
    catalog: &IndexCatalog,
) -> bool {
    let needed: HashSet<_> = fields.iter()
        .chain(pred.referenced_fields().iter())
        .collect();
    catalog.has_covering_index(collection, &needed)
}
```

**Restrictions:**
- All projected and filtered fields must be in one index
- Index must not be a partial or sparse index that excludes documents
- Array fields in the projection may prevent covered queries

## Cost Model

```rust
fn estimated_benefit(
    doc_count: f64,
    avg_doc_size: f64,
    avg_index_entry_size: f64,
) -> f64 {
    let doc_scan_cost = doc_count * avg_doc_size;
    let index_scan_cost = doc_count * avg_index_entry_size;
    (doc_scan_cost - index_scan_cost) / doc_scan_cost
}
```

**Typical benefit**: 0.7-0.95 when index entries are much smaller than documents.

## Test Cases

```javascript
// Positive: compound index on {status, amount}
db.orders.find(
  { status: "shipped" },
  { status: 1, amount: 1, _id: 0 }
);
// Uses index-only scan; no document fetch needed

// Negative: projection includes field not in index
db.orders.find(
  { status: "shipped" },
  { status: 1, amount: 1, customer_name: 1, _id: 0 }
);
// customer_name not in index; must fetch documents
```

## References

MongoDB: src/mongo/db/query/planner_analysis.cpp - analyzeGeo()
MongoDB docs: "Covered Queries" - docs.mongodb.com/manual/core/query-optimization
Chodorow "MongoDB: The Definitive Guide" (O'Reilly, 3rd ed.) Chapter 5
