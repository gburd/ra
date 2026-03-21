# Rule: Sort-Based Aggregation

**Category:** physical/aggregation-strategies
**File:** `rules/physical/aggregation-strategies/sort-based-aggregation.rra`

## Metadata

- **ID:** `sort-based-aggregation`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** aggregation, sort, grouping
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(aggregate ?input ?groups ?aggs)"
    description: "Sort-based aggregation implementation"
  - type: "predicate"
    condition: "is_sorted_by(?input, ?groups) || sort_cost(?input, ?groups) < hash_cost(?input, ?groups)"
    description: "Sort-based preferred when input is sorted or sort is cheaper than hash"
    optional: true
```


# Sort-Based Aggregation

## Description

Sorts input by group keys, then aggregates consecutive groups with streaming scan.

**When to apply**: High cardinality GROUP BY or input already sorted.

**Why it works**: Sorted groups are consecutive; can aggregate with minimal memory (no hash table).

## Relational Algebra

```algebra
aggregate[group_keys, agg_funcs](R)
  -> sort_aggregate:
       sorted = sort(R, group_keys)
       current_group = null
       current_aggs = null
       for each r in sorted:
         if r.group_keys != current_group:
           emit (current_group, current_aggs)
           current_group = r.group_keys
           current_aggs = initialize_aggregates(r)
         else:
           update_aggregates(current_aggs, r)
       emit (current_group, current_aggs)
```

## Implementation

```rust
rw!("use-sort-aggregation";
    "(aggregate ?groups ?aggs ?input)" =>
    "(sort-aggregate ?groups ?aggs ?input)"
    if high_cardinality("?groups") || is_sorted("?input", "?groups")
),
```

## Cost Model

```rust
fn cost(input_size: u64, is_presorted: bool) -> f64 {
    let sort_cost = if is_presorted {
        0.0
    } else {
        input_size as f64 * (input_size as f64).log2()
    };
    let scan = input_size as f64;
    sort_cost + scan
}
```

**Typical benefit**: 30-60% vs hash when high cardinality or pre-sorted

## Test Cases

### Positive: Very high cardinality

```sql
SELECT user_id, session_id, COUNT(*)
FROM events
GROUP BY user_id, session_id;

-- 500M distinct groups: hash table too large
-- Sort then stream aggregate
```

### Positive: Input already sorted

```sql
SELECT date, SUM(amount)
FROM sales
GROUP BY date;

-- Sales partitioned by date: already sorted
-- Skip sort, stream aggregate directly
```

### Positive: DISTINCT aggregation

```sql
SELECT category, COUNT(DISTINCT user_id)
FROM purchases
GROUP BY category;

-- DISTINCT requires sort anyway: combine with group aggregation
```

### Negative: Low cardinality

```sql
SELECT status, COUNT(*)
FROM orders
GROUP BY status;

-- 5 statuses: hash aggregation faster (no sort overhead)
```

## References

- PostgreSQL: Group aggregate with presort
- Oracle: Sort-based GROUP BY
- MySQL: Filesort with grouping
- mssql: Stream aggregate operator
