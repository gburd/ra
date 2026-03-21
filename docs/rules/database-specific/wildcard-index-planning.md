# Rule: MongoDB Wildcard Index Query Planning

**Category:** database-specific/mongodb
**File:** `rules/database-specific/mongodb/wildcard-index-planning.rra`

## Metadata

- **ID:** `mongodb-wildcard-index-planning`
- **Version:** "1.0.0"
- **Databases:** mongodb
- **Tags:** index, wildcard, schema-flexible, query-planning
- **Authors:** "MongoDB Inc."


# MongoDB Wildcard Index Query Planning

## Description

Plans query execution using wildcard indexes that cover arbitrary field paths in
schema-flexible documents. Wildcard indexes (`{$**: 1}`) index all scalar fields
in a document (or a subtree), enabling index scans for ad-hoc queries on fields
not known at index creation time.

**When to apply**: Queries on collections with highly variable schemas where
traditional compound indexes cannot cover all query patterns. The planner selects
a wildcard index when no specific index covers the queried field but a wildcard
index does.

**Why it works**: Schema-flexible collections may have hundreds of distinct field
paths. Creating individual indexes for each is impractical. A single wildcard
index covers all scalar fields, and the planner selects the relevant index subtree
for each query's field path, providing logarithmic lookup without per-field indexes.

## Relational Algebra

```algebra
-- Traditional index: explicit field
sigma[metadata.sensor_type = "temperature"](
  index-scan(idx_metadata_sensor_type))

-- Wildcard index: any field path
sigma[metadata.sensor_type = "temperature"](
  wildcard-index-scan($**, path="metadata.sensor_type"))

-- Planner extracts field path from predicate, looks up wildcard index subtree
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("mongodb-wildcard-index-select";
    "(filter (eq ?path ?val)
       (collection-scan ?coll))" =>
    "(filter (eq ?path ?val)
       (wildcard-index-scan ?coll ?path ?val))"
    if has_wildcard_index("?coll")
    if field_covered_by_wildcard("?path", "?coll")
    if is_scalar_field("?path")
),

rw!("mongodb-wildcard-over-collscan";
    "(collection-scan ?coll)" =>
    "(wildcard-index-scan ?coll ?queried-path ?bounds)"
    if no_specific_index("?coll", "?queried-path")
    if has_wildcard_index("?coll")
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> bool {
    stats.has_wildcard_index
        && stats.queried_field_is_scalar
        && !stats.has_specific_index_for_field
        && stats.collection_size > 1000
}
```

**Restrictions:**
- Wildcard indexes do not support compound index behavior (multi-field)
- Cannot index array elements for multikey-like semantics
- `$exists: false` queries cannot use wildcard indexes
- Slower than purpose-built indexes for known query patterns
- Cannot cover sort operations (no ordering guarantee across fields)

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let collection_size = stats.total_documents as f64;
    let selectivity = stats.predicate_selectivity;

    // Full collection scan cost
    let collscan_cost = collection_size * 0.001; // 1ms per doc

    // Wildcard index scan cost (slightly higher than regular index)
    let index_scan_cost = (collection_size * selectivity).log2()
        * 0.00001 // index traversal
        + collection_size * selectivity * 0.001; // fetch matching docs

    if collscan_cost > index_scan_cost {
        (collscan_cost - index_scan_cost) / collscan_cost
    } else {
        0.0
    }
}
```

**Typical benefit**: 20% to 5x for selective queries on schema-flexible collections.

## Test Cases

### Positive: Ad-hoc query on dynamic field

```javascript
// Wildcard index covers all fields under metadata
db.events.createIndex({"metadata.$**": 1})

// Query on arbitrary metadata field
db.events.find({"metadata.device.firmware_version": "2.1.0"})

// Planner uses wildcard index subtree for metadata.device.firmware_version
// explain shows: IXSCAN { $**: 1 }
// Without wildcard: COLLSCAN
```

### Positive: Multiple queries, single index

```javascript
db.logs.createIndex({"$**": 1})

// All these queries can use the wildcard index:
db.logs.find({level: "error"})
db.logs.find({"request.method": "POST"})
db.logs.find({"response.status": 500})

// One index serves all three query patterns
```

### Negative: Query requiring sort on wildcard field

```javascript
db.events.createIndex({"$**": 1})

// Wildcard index cannot provide sort order
db.events.find({level: "error"}).sort({timestamp: -1})

// IXSCAN for filter, but still needs SORT stage for ordering
// A specific index {level: 1, timestamp: -1} would be better
```

## References

**Implementation:**
- MongoDB source: `src/mongo/db/query/planner_wildcard_helpers.cpp`
- Wildcard index internals: `src/mongo/db/index/wildcard_key_generator.cpp`
- Query planning: `src/mongo/db/query/get_executor.cpp`

**Documentation:**
- MongoDB Manual: "Wildcard Indexes"
  - https://docs.mongodb.com/manual/core/index-wildcard/
- MongoDB Blog: "Wildcard Indexes in MongoDB 4.2"

**Related papers:**
- Mior, M.J., et al., "NoSE: Schema Design for NoSQL Applications", IEEE TKDE 2017
