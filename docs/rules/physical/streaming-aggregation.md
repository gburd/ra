# Rule: Streaming Aggregation

**Category:** physical/aggregation-strategies
**File:** `rules/physical/aggregation-strategies/streaming-aggregation.rra`

## Metadata

- **ID:** `streaming-aggregation`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, clickhouse, cockroachdb, mssql, oracle
- **Tags:** aggregation, streaming, sorted
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(aggregate ?input ?groups ?aggs)"
    description: "Aggregation on streaming/sorted input"
  - type: "predicate"
    condition: "is_sorted_by(?input, ?groups)"
    description: "Input must be sorted by grouping columns for streaming"
```


# Streaming Aggregation

## Metadata
- **Rule ID**: `streaming-aggregation`
- **Category**: Physical / Aggregation Strategies
- **Complexity**: O(n) single-pass, O(1) memory per group
- **Introduced**: Stream processing systems (2000s)
- **Prerequisites**: Input sorted by GROUP BY columns
- **Alternatives**: hash-aggregation, sort-aggregation

## Description

Streaming aggregation assumes input is pre-sorted by GROUP BY columns, allowing O(1) memory aggregation with early result emission. Ideal for pipel ined execution.

**When to use:**
- Input pre-sorted (index scan, previous sort)
- Memory extremely limited
- Early result emission needed (LIMIT)
- GROUP BY is prefix of ORDER BY

**Advantages:**
- O(1) memory per group
- Single-pass algorithm
- Early result emission
- Perfect for pipelined execution

**Disadvantages:**
- Requires sorted input
- Cannot be used if input unsorted

## Relational Algebra

```
$\gamma$_{group_cols; agg_funcs}(R)
-> StreamingAggregation(R, group_cols, agg_funcs)
  :if Sorted(R, group_cols)

Cost = n * (compare + update)
```

## Implementation

```rust
pub struct StreamingAggregation {
    input: Box<dyn Operator>,
    group_cols: Vec<usize>,
    agg_funcs: Vec<AggregateFunction>,
    current_group: Option<GroupKey>,
    current_state: AggregateState,
    pending_result: Option<Tuple>,
}

impl Operator for StreamingAggregation {
    fn next(&mut self) -> Option<Tuple> {
        // Emit pending result first
        if let Some(result) = self.pending_result.take() {
            return Some(result);
        }

        while let Some(tuple) = self.input.next() {
            let key = self.extract_group_key(&tuple);

            if self.current_group.as_ref() \!= Some(&key) {
                // New group: emit previous, start new
                let result = self.finalize_group();
                self.current_group = Some(key);
                self.current_state = self.init_aggregate_state();
                self.update_aggregates(&tuple);

                if result.is_some() {
                    return result;
                }
            } else {
                self.update_aggregates(&tuple);
            }
        }

        // Emit final group
        self.finalize_group()
    }
}
```

## Cost Model

```rust
pub fn cost_streaming_aggregation(
    input_card: u64,
    group_card: u64,
) -> Cost {
    // Minimal CPU: just comparisons and updates
    Cost::cpu(input_card * 3) + Cost::memory(group_card * 64)
}
```

## Test Cases

### Test 1: Index scan provides sorted input
```sql
CREATE INDEX idx_sales_product ON sales(product);

SELECT product, SUM(amount)
FROM sales
GROUP BY product;

-- Expected: StreamingAggregation
-- Index scan provides sorted input
```

### Test 2: Early emission with LIMIT
```sql
SELECT product, COUNT(*)
FROM sales
GROUP BY product
ORDER BY product
LIMIT 10;

-- Expected: StreamingAggregation with early stop
-- Emit first 10 groups without scanning all input
```

## References

1. **ClickHouse**: Streaming aggregation for pre-sorted data
2. **Timely Dataflow**: Streaming aggregation in differential dataflow

## Tags
`physical`, `aggregation`, `streaming`, `sorted-input`, `pipelined`
