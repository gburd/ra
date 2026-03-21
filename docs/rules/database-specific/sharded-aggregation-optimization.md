# Rule: MongoDB Sharded Aggregation Optimization

**Category:** database-specific/mongodb
**File:** `rules/database-specific/mongodb/sharded-aggregation-optimization.rra`

## Metadata

- **ID:** `mongodb-sharded-aggregation`
- **Version:** "1.0.0"
- **Databases:** mongodb
- **Tags:** sharding, aggregation, distributed, merge
- **Authors:** "MongoDB Inc."


# MongoDB Sharded Aggregation Optimization

## Description

Optimizes aggregation pipelines in sharded clusters by determining which stages
can execute on shards (parallel) vs. primary shard (merge). Pushes computation
to shards when possible, minimizing data transfer and enabling parallelism.

**When to apply**: Aggregation queries on sharded collections. The optimizer
splits pipelines into shard-executable and merge-required portions based on
stage characteristics.

**Why it works**: Shard-local operations (filter, project, sort, limit) can
run in parallel on all shards. Only stages requiring global view ($group on
non-shard-key, $sort without limit) need merge on primary shard.

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("mongodb-push-aggregation-to-shards";
    "(pipeline ?stages)" =>
    "(merge-on-primary
       (map-on-shards (shard-executable-stages ?stages))
       (merge-required-stages ?stages))"
    if is-sharded-collection("?stages")
),
```

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let n_shards = stats.n_shards as f64;
    let docs_per_shard = stats.total_docs / n_shards;

    // Serial execution on primary
    let serial_cost = stats.total_docs * 0.00001;

    // Parallel on shards + merge
    let parallel_cost = (docs_per_shard * 0.00001) + (stats.merge_cost * 0.001);

    if serial_cost > parallel_cost {
        (serial_cost - parallel_cost) / serial_cost
    } else {
        0.0
    }
}
```

## Test Cases

### Positive: $match and $project push to shards

```javascript
db.orders.aggregate([
  {$match: {status: "completed"}},  // Runs on all shards
  {$project: {total: 1, date: 1}},  // Runs on all shards
  {$group: {_id: null, sum: {$sum: "$total"}}}  // Merge on primary
])
```

## References

**Documentation:**
- MongoDB Manual: "Aggregation Pipeline and Sharded Collections"
- https://docs.mongodb.com/manual/core/aggregation-pipeline-sharded-collections/
