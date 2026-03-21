# Rule: Compound Index Selection for Nested Fields

**Category:** multi-model/document
**File:** `rules/multi-model/document/compound-index-selection.rra`

## Metadata

- **ID:** `compound-index-selection`
- **Version:** "1.0.0"
- **Databases:** mongodb, couchbase, cosmosdb
- **Tags:** document, index, compound, nested, selection
- **SQL Standard:** "mql:5"
- **Authors:** "RA Contributors"


# Compound Index Selection for Nested Fields

## Description

Selects the optimal compound index when a query filters on multiple
nested document fields. Document databases support compound indexes
that span dotted paths (e.g., `{address.city: 1, address.zip: 1}`).
Choosing the best index prefix match avoids collection scans and
minimizes the number of index entries examined.

**When to apply**: A query has conjunctive predicates on two or more
nested fields, and one or more compound indexes cover a prefix of those
fields.

**Why it works**: A compound index stores entries sorted by the
concatenation of its key fields. The query planner can use an index
scan with a bounded key range when the query predicates match a
prefix of the index key order.

## Relational Algebra

```algebra
sigma[a.x = v1 AND a.y > v2](scan(collection))
  -> index_scan(collection, idx_{a.x, a.y}, [v1, v2..])
  where idx_{a.x, a.y} covers both predicates as a prefix
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("compound-index-selection";
    "(filter (and ?p1 ?p2) (doc-scan ?coll))" =>
    "(compound-index-scan ?coll ?p1 ?p2)"
    if matching_compound_index("?coll", "?p1", "?p2")
),
```

## Preconditions

```rust
fn applicable(
    collection: &str,
    preds: &[Expr],
    catalog: &IndexCatalog,
) -> bool {
    let fields: Vec<_> = preds.iter()
        .filter_map(|p| p.indexed_field())
        .collect();
    catalog.has_compound_index_prefix(collection, &fields)
}
```

**Restrictions:**
- Predicates must match a prefix of the compound index key order
- Range predicates should come after equality predicates in the index
- Multikey indexes (on array fields) have special intersection rules

## Cost Model

```rust
fn estimated_benefit(
    total_docs: f64,
    selectivity_combined: f64,
    index_scan_cost_factor: f64,
) -> f64 {
    let collection_scan_cost = total_docs;
    let index_scan_cost =
        total_docs * selectivity_combined * index_scan_cost_factor;
    (collection_scan_cost - index_scan_cost) / collection_scan_cost
}
```

**Typical benefit**: 0.8-0.99 for selective compound predicates.

## Test Cases

```javascript
// Positive: compound index on {address.city, address.zip}
db.users.find({
  "address.city": "Seattle",
  "address.zip": { $gte: "98100", $lte: "98199" }
});
// Uses compound index scan with bounded key range

// Negative: predicates don't match index prefix
// Index is on {city, zip}, query filters on {zip, state}
db.users.find({
  "address.zip": "98101",
  "address.state": "WA"
});
// Cannot use compound index; zip is not the first key
```

## References

MongoDB: src/mongo/db/query/query_planner.cpp - planFromCache()
MongoDB docs: "Compound Indexes" - docs.mongodb.com/manual/core/index-compound
Graefe "Implementing Sorting in Database Systems" (ACM Computing Surveys 2006)
