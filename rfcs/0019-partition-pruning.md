# RFC 0019: Partition Pruning and Partition-Wise Operations

- Start Date: 2026-03-20
- Author: System
- Status: Draft

## Summary

Add partition pruning (eliminate partitions at planning time), partition-wise joins, and partition-wise aggregation for partitioned tables.

## Motivation

Partitioned tables are common in analytics and time-series workloads:
```sql
CREATE TABLE events (
    timestamp TIMESTAMPTZ,
    user_id INT,
    event_type TEXT
) PARTITION BY RANGE (timestamp);
```

**Without partition pruning:**
```sql
SELECT * FROM events WHERE timestamp > '2024-01-01';
-- Scans ALL partitions (2023-01 through 2024-12)
```

**With partition pruning:**
```sql
-- Only scans partitions >= 2024-01 (12 partitions instead of 24)
```

## Technical design

### Partition Pruning

```rust
pub struct PartitionInfo {
    pub table: String,
    pub partition_key: Vec<String>,
    pub partitions: Vec<PartitionBounds>,
}

pub enum PartitionBounds {
    Range { min: Const, max: Const },
    List { values: Vec<Const> },
    Hash { modulus: u32, remainder: u32 },
}

impl PartitionPruner {
    pub fn prune(&self, info: &PartitionInfo, filter: &Expr) -> Vec<usize> {
        // For each partition, check if filter contradicts bounds
        info.partitions
            .iter()
            .enumerate()
            .filter_map(|(idx, bounds)| {
                if self.filter_overlaps(filter, bounds) {
                    Some(idx)
                } else {
                    None  // Prune this partition
                }
            })
            .collect()
    }
}
```

### Partition-Wise Join

When joining two tables partitioned on the same key:
```sql
SELECT * FROM events_2024 e JOIN users u ON e.user_id = u.user_id
WHERE u.active = true
```

Can be rewritten as:
```sql
UNION ALL
  SELECT * FROM events_2024_01 e JOIN users_partition_1 u ON e.user_id = u.user_id
  SELECT * FROM events_2024_02 e JOIN users_partition_2 u ON e.user_id = u.user_id
  ...
```

Each partition join can be executed in parallel.

### Partition-Wise Aggregation

```sql
SELECT date_trunc('day', timestamp), COUNT(*)
FROM events
WHERE timestamp > '2024-01-01'
GROUP BY 1
```

Rewrite as:
```sql
SELECT agg_combine(partial_aggs) FROM (
  SELECT partial_agg FROM events_2024_01 GROUP BY date_trunc('day', timestamp)
  UNION ALL
  SELECT partial_agg FROM events_2024_02 GROUP BY date_trunc('day', timestamp)
  ...
)
```

## Implementation plan

- Week 1-2: Partition metadata and pruning logic
- Week 3-4: Partition-wise join transformation
- Week 5-6: Partition-wise aggregation
- Week 7: Cost model and testing

## Gap addressed

Gaps #5.1, #5.2, #5.3 (Medium severity) from postgres-planner-gaps.md
