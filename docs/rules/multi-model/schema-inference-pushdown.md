# Rule: Schema Inference Pushdown

**Category:** multi-model/document
**File:** `rules/multi-model/document/schema-inference-pushdown.rra`

## Metadata

- **ID:** `schema-inference-pushdown`
- **Version:** "1.0.0"
- **Databases:** mongodb, couchbase, cosmosdb
- **Tags:** document, schema, inference, pushdown, type
- **SQL Standard:** "mql:5"
- **Authors:** "RA Contributors"


# Schema Inference Pushdown

## Description

Uses inferred schema information to push type-aware predicates into
document scans. Schema-flexible databases allow heterogeneous field
types within a collection. By inferring the dominant type for a field,
the optimizer can add an implicit type check to avoid runtime type
errors and enable index usage.

**When to apply**: A predicate on a field that has mixed types in the
collection, and the query implies a specific type (e.g., numeric
comparison on a field that sometimes contains strings).

**Why it works**: Without type awareness, the storage engine may skip
the index (indexes are type-specific in MongoDB) and fall back to a
collection scan. By adding an explicit type constraint, the optimizer
enables index usage and prunes mistyped documents early.

## Relational Algebra

```algebra
sigma[field > value](scan(collection))
  -> sigma[type(field) = numeric AND field > value](scan(collection))
  where schema_inference shows field has mixed types
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("schema-inference-pushdown";
    "(filter (gt (field ?doc ?f) ?val) (doc-scan ?coll))" =>
    "(filter (and (type-eq (field ?doc ?f) numeric) \
                  (gt (field ?doc ?f) ?val)) \
       (doc-scan ?coll))"
    if has_mixed_types("?coll", "?f")
),
```

## Preconditions

```rust
fn applicable(
    collection: &str,
    field: &str,
    schema: &InferredSchema,
) -> bool {
    let types = schema.field_types(collection, field);
    types.len() > 1
}
```

**Restrictions:**
- Only beneficial when the field has multiple types in the collection
- Schema inference must be recent enough to be accurate
- Adding type checks changes query semantics if the user intended
  cross-type comparison

## Cost Model

```rust
fn estimated_benefit(
    total_docs: f64,
    correct_type_fraction: f64,
) -> f64 {
    let without_type = total_docs;
    let with_type = total_docs * correct_type_fraction;
    (without_type - with_type) / without_type
}
```

**Typical benefit**: 0.1-0.4 depending on type heterogeneity.

## Test Cases

```javascript
// Positive: field 'age' has mixed string/number types
db.users.find({ age: { $gt: 25 } });
// Adds type check: { age: { $type: "number", $gt: 25 } }
// Enables use of numeric index on 'age'

// Negative: field has uniform type
db.users.find({ name: "Alice" });
// No type ambiguity; no additional check needed
```

## References

MongoDB: docs.mongodb.com/manual/reference/operator/query/type
Wang et al. "Schema Management for Document Stores" (SIGMOD 2015)
Klettke et al. "Schema Extraction and Structural Outlier Detection for JSON Data" (BTW 2015)
