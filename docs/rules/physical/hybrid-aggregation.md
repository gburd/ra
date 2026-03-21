# Rule: Hybrid Aggregation

**Category:** physical/aggregation-strategies
**File:** `rules/physical/aggregation-strategies/hybrid-aggregation.rra`

## Metadata

- **ID:** `hybrid-aggregation`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, clickhouse, cockroachdb, mssql, oracle
- **Tags:** aggregation, adaptive, hybrid
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(aggregate ?input ?groups ?aggs)"
    description: "Aggregation combining hash and sort strategies"
  - type: "fact"
    fact_type: "statistics.distinct_count"
    table: "?input"
    comparator: ">"
    threshold: 100000
    optional: true
    description: "High enough cardinality to benefit from hybrid approach"
```


# Hybrid Aggregation

## Metadata
- **Rule ID**: `hybrid-aggregation`
- **Category**: Physical / Aggregation Strategies
- **Complexity**: O(n) adaptive
- **Introduced**: Modern OLAP systems (2010s)
- **Prerequisites**: Runtime monitoring
- **Alternatives**: hash-aggregation, sort-aggregation

## Description

Hybrid aggregation starts with hash-based aggregation and dynamically switches to sort-based if memory pressure detected. Best of both worlds with adaptive behavior.

**Strategy:**
1. Begin with hash aggregation (fast path)
2. Monitor memory usage
3. If threshold exceeded, spill and switch to sort
4. Merge hash table with sorted spills

**When to use:**
- Unknown cardinality at planning time
- Variable workload characteristics
- Memory-constrained environments
- Production systems requiring robustness

## Relational Algebra

```
γ_{g; AGG(v)}(R)
→ HybridAggregation(R, g, AGG)
  where strategy = if memory_ok then Hash else Sort
```

## Implementation

```rust
pub struct HybridAggregation {
    input: Box<dyn Operator>,
    strategy: Strategy,
    hash_table: HashMap<GroupKey, AggState>,
    spill_files: Vec<SpillFile>,
    memory_limit: usize,
}

enum Strategy {
    Hash,
    SpillingToSort,
    MergingSpills,
}

impl Operator for HybridAggregation {
    fn next(&mut self) -> Option<Tuple> {
        match self.strategy {
            Strategy::Hash => {
                while let Some(tuple) = self.input.next() {
                    if self.memory_usage() > self.memory_limit {
                        // Switch strategy
                        self.spill_hash_table();
                        self.strategy = Strategy::SpillingToSort;
                        return self.next();
                    }

                    let key = self.extract_group_key(&tuple);
                    self.hash_table.entry(key)
                        .or_insert_with(|| self.init_state())
                        .update(&tuple);
                }

                // All fits in memory
                self.emit_from_hash_table()
            }
            Strategy::SpillingToSort => {
                // Continue with sort-based aggregation
                self.external_sort_and_aggregate()
            }
            Strategy::MergingSpills => {
                // Merge sorted spill files
                self.merge_spills()
            }
        }
    }
}
```

## Cost Model

```rust
pub fn cost_hybrid_aggregation(
    input_card: u64,
    group_card: u64,
    memory_limit: usize,
    spill_probability: f64,
) -> Cost {
    // Expected cost: weighted average
    let hash_cost = cost_hash_aggregation(input_card, group_card, memory_limit);
    let sort_cost = cost_sort_aggregation(input_card, group_card, memory_limit);

    hash_cost * (1.0 - spill_probability) + sort_cost * spill_probability
}
```

## Test Cases

### Test 1: Low cardinality (no spill)
```sql
SELECT category, COUNT(*)
FROM products
GROUP BY category;

-- Groups: 10 categories
-- Result: Pure hash aggregation (fast path)
```

### Test 2: High cardinality (spills)
```sql
SELECT user_id, COUNT(*)
FROM events
GROUP BY user_id;

-- Groups: 10M users
-- Result: Starts hash, detects pressure, switches to sort
```

## References

1. **DuckDB**: Adaptive aggregation with spilling
2. **mssql**: Adaptive query processing
3. **Oracle**: Automatic workload management

## Tags
`physical`, `aggregation`, `hybrid`, `adaptive`, `spilling`
