# Rule: Aggregate Selectivity Estimation

**Category:** logical/aggregate-pushdown
**File:** `rules/logical/aggregate-pushdown/aggregate-selectivity-estimation.rra`

## Metadata

- **ID:** `aggregate-selectivity-estimation`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb
- **Tags:** aggregation, statistics, cardinality
- **Authors:** "RA Contributors"


# Aggregate Selectivity Estimation

## Description

Uses NDV (number of distinct values) statistics to estimate aggregate output
size and choose optimal algorithm.

**When to apply**: When deciding between hash vs sort aggregation.

**Why it works**: Accurate cardinality estimates enable better algorithm selection.

## Relational Algebra

```algebra
aggregate[group_cols, agg](R)
  -> cost_based_aggregate[algorithm, group_cols, agg](R)
  where algorithm = choose_algorithm(NDV(group_cols), input_size)

If NDV(group_cols) is small: streaming or sorting
If NDV(group_cols) is large: hashing
```

## Implementation

```rust
fn choose_aggregate_algorithm(
    ndv: u64,
    input_rows: u64,
    memory_limit: u64,
) -> AggAlgorithm {
    let hash_table_size = ndv * 200; // Estimate per-group overhead

    if ndv < 100 {
        AggAlgorithm::Streaming // Very few groups
    } else if hash_table_size < memory_limit {
        AggAlgorithm::HashAgg // Fits in memory
    } else {
        AggAlgorithm::SortAgg // Spill-safe
    }
}
```

## Cost Model

```rust
fn benefit() -> f64 {
    0.0 // Not a rewrite: enables better cost-based decisions
}
```

**Typical benefit**: Indirect (0-20% from better algorithm choice)

## Test Cases

### Positive: Low NDV -> Streaming

```sql
SELECT status, COUNT(*) FROM orders GROUP BY status;

-- status has 5 distinct values
-- Use streaming aggregation
```

### Positive: High NDV -> Hash

```sql
SELECT user_id, COUNT(*) FROM events GROUP BY user_id;

-- user_id has 10M distinct values
-- Use hash aggregation if memory allows
```

### Positive: Very high NDV -> Sort

```sql
SELECT session_id, MAX(timestamp) FROM logs GROUP BY session_id;

-- session_id has 1B distinct values
-- Use sort-based aggregation (external sort)
```

## References

- PostgreSQL: estimate_num_groups for cardinality estimation
- DuckDB: Perfect hash aggregation for low NDV
- Calcite: Statistics-based aggregate algorithm selection
