# Rule: Hash Aggregation

**Category:** physical/aggregation-strategies
**File:** `rules/physical/aggregation-strategies/hash-aggregation.rra`

## Metadata

- **ID:** `hash-aggregation`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, clickhouse, cockroachdb, mssql, oracle
- **Tags:** aggregation, hash, in-memory, olap
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(aggregate ?input ?groups ?aggs)"
    description: "Aggregation suitable for hash-based strategy"
  - type: "fact"
    fact_type: "statistics.distinct_count"
    table: "?input"
    comparator: "<"
    threshold: 1000000
    description: "Group cardinality should fit in memory (<1M groups)"
    optional: true
  - type: "fact"
    fact_type: "hardware.memory"
    comparator: ">="
    threshold: 67108864
    optional: true
    description: "Sufficient memory for hash table (>=64MB)"
```


# Hash Aggregation

## Metadata
- **Rule ID**: `hash-aggregation`
- **Category**: Physical / Aggregation Strategies
- **Complexity**: O(n) average, O(n²) worst-case with hash collisions
- **Introduced**: Early 1980s (various systems)
- **Prerequisites**: Sufficient memory for hash table
- **Alternatives**: sort-aggregation, streaming-aggregation

## Description

Hash aggregation builds an in-memory hash table keyed by GROUP BY columns, accumulating aggregate values as rows are scanned. Most efficient for moderate cardinality grouping.

**When to use:**
- GROUP BY cardinality fits in memory
- Random access to groups needed
- Unsorted input data
- Single-pass aggregation desired

**Advantages:**
- O(n) single-pass algorithm
- No sorting required
- Efficient for moderate group counts (<1M groups)
- Supports partial aggregation

**Disadvantages:**
- Memory-bound (spills to disk if exceeded)
- Poor cache locality for high cardinality
- Sensitive to hash collisions
- Cannot produce sorted output

## Relational Algebra

```
γ_{group_cols; agg_funcs}(R)
→ HashAggregation(R, group_cols, agg_funcs)

Cost = n * (hash + update) + |groups| * emit
  where n = |R|
        groups = distinct(R.group_cols)
```

## Implementation (egg rewrite rules)

```lisp
;; Select hash aggregation for moderate cardinality
(rewrite (aggregate ?input ?groups ?aggs)
  (hash-aggregate ?input ?groups ?aggs)
  :if (< (group-cardinality ?input ?groups) 1000000)
  :if (< (memory-required ?input ?groups) (available-memory)))

;; Prefer hash over sort when input is unsorted
(rewrite (aggregate ?input ?groups ?aggs)
  (hash-aggregate ?input ?groups ?aggs)
  :if (not (sorted-by ?input ?groups)))

;; Switch to sort-based when memory insufficient
(rewrite (hash-aggregate ?input ?groups ?aggs)
  (sort-aggregate ?input ?groups ?aggs)
  :if (> (memory-required ?input ?groups) (available-memory)))
```

## Implementation Pattern (Volcano Iterator)

```rust
pub struct HashAggregation {
    input: Box<dyn Operator>,
    group_cols: Vec<usize>,
    agg_funcs: Vec<AggregateFunction>,
    hash_table: HashMap<GroupKey, AggregateState>,
    iterator: Option<hash_map::IntoIter<GroupKey, AggregateState>>,
    phase: Phase,
}

enum Phase {
    Building,
    Emitting,
}

impl Operator for HashAggregation {
    fn next(&mut self) -> Option<Tuple> {
        match self.phase {
            Phase::Building => {
                // Build phase: consume all input
                while let Some(tuple) = self.input.next() {
                    let key = self.extract_group_key(&tuple);
                    let entry = self.hash_table.entry(key)
                        .or_insert_with(|| self.init_aggregate_state());

                    for (func, value) in self.agg_funcs.iter().zip(tuple.values()) {
                        func.update(entry, value);
                    }
                }

                // Switch to emit phase
                self.phase = Phase::Emitting;
                self.iterator = Some(std::mem::take(&mut self.hash_table).into_iter());
                self.next()
            }
            Phase::Emitting => {
                // Emit phase: return groups one by one
                self.iterator.as_mut()?.next().map(|(key, state)| {
                    let mut tuple = key.into_tuple();
                    for (func, agg_state) in self.agg_funcs.iter().zip(&state.accumulators) {
                        tuple.push(func.finalize(agg_state));
                    }
                    tuple
                })
            }
        }
    }
}
```

## Preconditions


> **Note:** Formal preconditions are defined in the YAML frontmatter above.

```rust
fn can_use_hash_aggregation(
    input: &RelNode,
    group_cols: &[Column],
    agg_funcs: &[AggFunc],
    context: &OptimizerContext,
) -> bool {
    let group_cardinality = estimate_group_cardinality(input, group_cols);
    let bytes_per_group = estimate_group_size(group_cols, agg_funcs);
    let memory_required = group_cardinality * bytes_per_group;

    // Check memory availability (with safety margin)
    let available_memory = context.query_memory_limit();
    memory_required < (available_memory as f64 * 0.8) as u64
}
```

## Cost Model

```rust
pub fn cost_hash_aggregation(
    input_card: u64,
    group_card: u64,
    group_width: usize,
    agg_count: usize,
    hardware: &HardwareModel,
) -> Cost {
    let bytes_per_group = group_width + agg_count * 8;
    let hash_table_size = group_card * bytes_per_group as u64;

    // Scan input
    let scan_cost = Cost::io(
        (input_card as f64 / hardware.tuples_per_page()) * hardware.sequential_page_read_cost()
    );

    // Hash and update costs
    let hash_cost = Cost::cpu(input_card * 10); // Hash computation
    let update_cost = Cost::cpu(input_card * agg_count as u64 * 5); // Aggregate updates

    // Memory cost (check for spill)
    let memory_cost = if hash_table_size > hardware.cache_size_l3() {
        // Spill to disk
        Cost::io(hash_table_size as f64 / hardware.page_size() * 2.0) // Write + read
    } else {
        Cost::memory(hash_table_size)
    };

    scan_cost + hash_cost + update_cost + memory_cost
}
```

## Test Cases

### Test 1: Simple GROUP BY with COUNT
```sql
CREATE TABLE sales (product TEXT, amount DECIMAL, date DATE);
INSERT INTO sales VALUES
  ('Widget', 100, '2024-01-01'),
  ('Gadget', 150, '2024-01-01'),
  ('Widget', 200, '2024-01-02'),
  ('Widget', 50, '2024-01-03');

SELECT product, COUNT(*), SUM(amount)
FROM sales
GROUP BY product;

-- Expected: HashAggregation
-- Groups: 2 (Widget, Gadget)
-- Passes: 1 (single scan)
```

### Test 2: Multiple aggregates
```sql
SELECT product,
       COUNT(*) as cnt,
       SUM(amount) as total,
       AVG(amount) as avg_amt,
       MIN(amount) as min_amt,
       MAX(amount) as max_amt
FROM sales
GROUP BY product;

-- Expected: HashAggregation with 5 aggregates
-- All aggregates computed in single pass
```

### Test 3: Composite GROUP BY key
```sql
SELECT product, date, COUNT(*), SUM(amount)
FROM sales
GROUP BY product, date;

-- Expected: HashAggregation
-- Groups: 3 (Widget/2024-01-01, Gadget/2024-01-01, Widget/2024-01-02)
-- Composite key hashing
```

### Test 4: Should spill for high cardinality
```sql
CREATE TABLE events (user_id INT, event TEXT, timestamp TIMESTAMP);
INSERT INTO events SELECT generate_series(1, 10000000), 'click', now();

SELECT user_id, COUNT(*)
FROM events
GROUP BY user_id;

-- Expected: HashAggregation with spill-to-disk
-- Or: SortAggregation if memory too constrained
-- Groups: 10M (exceeds typical memory limits)
```

## References

1. **MonetDB X100**: Boncz et al. (2005) "MonetDB/X100: Hyper-Pipelining Query Execution"
   - Hash-based aggregation in columnar systems

2. **DuckDB Hash Aggregation**: https://duckdb.org/internals/vector.html
   - Vectorized hash aggregation implementation

3. **PostgreSQL Docs**: "Aggregate Functions and GROUP BY"
   - https://www.postgresql.org/docs/current/queries-table-expressions.html

4. **Database Internals** (Petrov, 2019), Chapter 9
   - Hash-based vs sort-based aggregation tradeoffs

## Tags
`physical`, `aggregation`, `hash`, `in-memory`, `olap`
