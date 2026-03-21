# Rule: MongoDB Slot-Based Execution Engine (SBE)

**Category:** database-specific/mongodb
**File:** `rules/database-specific/mongodb/sbe-slot-based-execution.rra`

## Metadata

- **ID:** `mongodb-sbe-slot-based-execution`
- **Version:** "1.0.0"
- **Databases:** mongodb
- **Tags:** execution, sbe, slot-based, vectorized, performance
- **Authors:** "MongoDB Inc."


# MongoDB Slot-Based Execution Engine (SBE)

## Description

MongoDB's Slot-Based Execution engine (SBE) replaces the classic document-at-a-time
execution model with a slot-based approach where each operator passes individual
values through named slots rather than full BSON documents. This reduces
deserialization overhead and enables more efficient data flow between operators.

**When to apply**: SBE is used for eligible queries in MongoDB 5.0+ as the
default execution engine. The query planner selects SBE for find() queries,
aggregation pipelines, and certain update/delete operations when the query shape
is supported.

**Why it works**: The classic engine passes full BSON documents between operators,
requiring repeated deserialization of fields. SBE passes only the fields needed
by each operator through typed slots, eliminating redundant parsing and reducing
memory allocation. This is analogous to the transition from row-at-a-time to
columnar processing in analytical databases.

## Relational Algebra

```algebra
-- Classic engine: full document passing
pi[name, age](sigma[age > 21](scan(users)))
  Each operator receives/emits full BSON document

-- SBE: slot-based value passing
pi[slot_name, slot_age](sigma[slot_age > 21](scan(users, slots=[name, age])))
  Scan populates only needed slots; operators work on slots
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("mongodb-classic-to-sbe";
    "(classic-exec
       (project ?fields
          (filter ?pred
             (collection-scan ?coll))))" =>
    "(sbe-exec
       (sbe-project (fields-to-slots ?fields)
          (sbe-filter (pred-to-slot-expr ?pred)
             (sbe-scan ?coll (required-slots ?fields ?pred)))))"
    if sbe_eligible("?coll", "?pred", "?fields")
),

rw!("mongodb-sbe-slot-pruning";
    "(sbe-project ?out-slots
       (sbe-filter ?pred-expr
          (sbe-scan ?coll ?all-slots)))" =>
    "(sbe-project ?out-slots
       (sbe-filter ?pred-expr
          (sbe-scan ?coll (union-slots ?out-slots (pred-slots ?pred-expr)))))"
    if has_unused_slots("?all-slots", "?out-slots", "?pred-expr")
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> bool {
    stats.mongodb_version >= Version(5, 0)
        && stats.query_is_sbe_eligible
        && !stats.uses_unsupported_sbe_operator
}
```

**Restrictions:**
- Not all aggregation stages are SBE-compatible (e.g., some $lookup variants)
- Queries with JavaScript expressions ($where) fall back to classic engine
- Text search queries use classic engine
- SBE requires query plan caching for best performance

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let doc_count = stats.input_document_count as f64;
    let avg_doc_size = stats.avg_document_size_bytes as f64;
    let needed_fields = stats.projected_field_count as f64;
    let total_fields = stats.avg_field_count as f64;

    // Classic: deserialize full document per operator
    let classic_cost = doc_count * avg_doc_size * 0.000001
        * stats.operator_count as f64;

    // SBE: deserialize only needed fields, once
    let sbe_cost = doc_count * avg_doc_size
        * (needed_fields / total_fields)
        * 0.000001;

    if classic_cost > sbe_cost {
        (classic_cost - sbe_cost) / classic_cost
    } else {
        0.0
    }
}
```

**Assumptions:**
- BSON deserialization: ~1us per KB
- Slot access: ~10ns per slot read
- Average operator chain length: 3-5 operators
- Field utilization ratio: typically 20-40% of document fields

**Typical benefit**: 20% to 3x improvement for queries touching few fields
from large documents.

## Test Cases

### Positive: Projection on large documents

```javascript
// Large documents with many fields
db.profiles.find(
  {status: "active"},
  {name: 1, email: 1}
)

// SBE: scan populates only status, name, email slots
// Classic: passes full document (50+ fields) through filter and project
// SBE wins when documents are large and projection is narrow
```

### Positive: Multi-stage aggregation

```javascript
db.orders.aggregate([
  {$match: {status: "shipped"}},
  {$group: {_id: "$region", total: {$sum: "$amount"}}},
  {$sort: {total: -1}},
  {$limit: 10}
])

// SBE: slots for status, region, amount flow through pipeline
// Each stage only accesses its required slots
// No full-document deserialization between stages
```

### Negative: $where with JavaScript

```javascript
// Falls back to classic engine
db.collection.find({
  $where: function() { return this.a + this.b > 100; }
})

// JavaScript expressions require full document context
// SBE cannot optimize this query shape
```

## References

**Implementation:**
- MongoDB source: `src/mongo/db/exec/sbe/`
- Stage builders: `src/mongo/db/query/sbe_stage_builder.cpp`
- Slot management: `src/mongo/db/exec/sbe/values/slot.h`

**Documentation:**
- MongoDB Blog: "An Introduction to the MongoDB SBE"
- MongoDB Jira: SERVER-52892 (SBE rollout tracking)

**Related papers:**
- Neumann, T., "Efficiently Compiling Efficient Query Plans", PVLDB 2011
  - Slot-based execution draws from compiled query execution concepts
