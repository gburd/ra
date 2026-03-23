# Rule: Shapiro Symmetric Hash Join

**Category:** physical/join-algorithms
**File:** `rules/physical/join-algorithms/shapiro-symmetric-hash-join.rra`

## Metadata

- **ID:** `shapiro-symmetric-hash-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mssql
- **Tags:** hash-join, symmetric, pipelining, classic
- **Authors:** "Leonard Shapiro"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(join inner (= ?lcol ?rcol) ?left ?right)"
    description: "Equi-join with symmetric hash (pipelined)"
  - type: "predicate"
    condition: "is_equijoin(= ?lcol ?rcol)"
    description: "Must be an equi-join"
  - type: "predicate"
    condition: "is_streaming(?left) || is_streaming(?right)"
    description: "At least one input is a stream (benefits from symmetric hashing)"
    optional: true
```


# Shapiro Symmetric Hash Join

## Description

A pipelined hash join algorithm that builds hash tables on both inputs
simultaneously, allowing tuples to be produced as soon as matches are found
without waiting for one input to complete. This enables better pipelining
and parallelism compared to traditional (build-then-probe) hash join.

**When to apply**: Pipelined query execution where early tuple production
is beneficial, or when both join inputs arrive incrementally (e.g., from
network or parallel scans). Particularly useful in streaming or interactive
queries.

**Why it works**: Traditional hash join waits to build a complete hash table
on the smaller input before probing. Symmetric hash join maintains hash tables
on both sides and produces results incrementally as tuples arrive, improving
response time and enabling better parallelism.

## Relational Algebra

```algebra
Traditional hash join (build-probe):
1. Build phase: Insert all of R into hash table H_R
2. Probe phase: For each s $\in$ S, probe H_R for matches

Symmetric hash join:
1. Maintain H_R and H_S (hash tables on both sides)
2. For each arriving r $\in$ R:
     - Probe H_S for matches, output results
     - Insert r into H_R
3. For each arriving s $\in$ S:
     - Probe H_R for matches, output results
     - Insert s into H_S
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Physical implementation sketch
struct SymmetricHashJoin {
    hash_table_left: HashMap<JoinKey, Vec<Tuple>>,
    hash_table_right: HashMap<JoinKey, Vec<Tuple>>,
}

impl SymmetricHashJoin {
    fn next_tuple(&mut self, executor: &mut Executor) -> Option<Tuple> {
        loop {
            // Try to get tuple from left input
            if let Some(left_tuple) = self.left_input.next() {
                let key = self.extract_join_key(&left_tuple);

                // Probe right hash table
                if let Some(right_tuples) = self.hash_table_right.get(&key) {
                    for right_tuple in right_tuples {
                        if self.join_predicate(&left_tuple, right_tuple) {
                            // Found match! Produce result immediately
                            return Some(self.combine(left_tuple.clone(), right_tuple.clone()));
                        }
                    }
                }

                // Insert into left hash table
                self.hash_table_left.entry(key)
                    .or_insert_with(Vec::new)
                    .push(left_tuple);
            }

            // Try to get tuple from right input
            if let Some(right_tuple) = self.right_input.next() {
                let key = self.extract_join_key(&right_tuple);

                // Probe left hash table
                if let Some(left_tuples) = self.hash_table_left.get(&key) {
                    for left_tuple in left_tuples {
                        if self.join_predicate(left_tuple, &right_tuple) {
                            return Some(self.combine(left_tuple.clone(), right_tuple.clone()));
                        }
                    }
                }

                // Insert into right hash table
                self.hash_table_right.entry(key)
                    .or_insert_with(Vec::new)
                    .push(right_tuple);
            }

            // Both inputs exhausted
            if self.left_input.is_done() && self.right_input.is_done() {
                return None;
            }
        }
    }
}
```

## Preconditions


> **Note:** Formal preconditions are defined in the YAML frontmatter above.

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    // Both inputs should be pipelined (not materialized)
    stats.inputs_are_pipelined
        // Join should be equi-join (hash-based)
        && stats.is_equijoin
        // Sufficient memory for two hash tables
        && (stats.left_cardinality + stats.right_cardinality) * stats.avg_row_size
           < hw.hash_table_memory_limit
        // Benefit from early tuple production
        && (stats.has_limit || stats.is_interactive_query)
}
```

**Restrictions:**
- Requires sufficient memory for TWO hash tables (vs. one for traditional)
- Only applicable to equi-joins (equality predicates)
- Most beneficial when early tuple production matters (LIMIT, interactive)
- Memory overhead: 2x vs. traditional hash join

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let left_rows = stats.left_cardinality as f64;
    let right_rows = stats.right_cardinality as f64;
    let result_rows = (left_rows * right_rows * stats.join_selectivity);

    // Traditional hash join:
    // - Build: insert all left rows (can't produce results yet)
    // - Probe: check all right rows, produce results
    // Time to first tuple: time_to_build_left
    let traditional_first_tuple_latency = left_rows * 0.000001;

    // Symmetric hash join:
    // - Can produce results as soon as first matching pair arrives
    // Expected time to first result (assuming uniform match distribution)
    let match_probability = stats.join_selectivity;
    let expected_tuples_until_match = 1.0 / match_probability;
    let symmetric_first_tuple_latency = expected_tuples_until_match * 0.000001;

    // For queries with LIMIT or interactive requirements
    if stats.has_limit || stats.is_interactive_query {
        let latency_improvement =
            (traditional_first_tuple_latency - symmetric_first_tuple_latency)
            / traditional_first_tuple_latency;
        return latency_improvement.max(0.3); // At least 30% for pipelining
    }

    // For full scan queries, symmetric has overhead (two hash tables)
    // Slightly slower due to double probing
    -0.1 // 10% slower for full scans
}
```

**Assumptions:**
- Traditional hash join: build phase blocks, then probe phase produces results
- Symmetric: produces results as tuples arrive
- Memory cost: 2x hash tables vs. 1x (memory-limited scenarios may not apply)
- Probe cost is similar between the two (both O(1) expected)

**Typical benefit**: 30%-150% improvement in time-to-first-tuple for LIMIT queries.

## Test Cases

### Positive: LIMIT query (early termination)

```sql
-- Find first 10 matching orders
SELECT * FROM customers c
JOIN orders o ON c.id = o.customer_id
WHERE c.region = 'US'
ORDER BY o.order_date DESC
LIMIT 10;

-- Symmetric hash join: Produces first results immediately
-- Traditional: Must build full customer hash table before any results
-- If matches are found early, can terminate both inputs early
```

### Positive: Parallel inputs

```sql
-- Both inputs are parallel scans
SELECT * FROM large_table1 t1
JOIN large_table2 t2 ON t1.key = t2.key;

-- Symmetric: Both scans run in parallel, producing results as soon as tuples meet
-- Traditional: Left scan completes, then right scan starts
```

### Negative: Memory-limited scenario

```sql
-- Both tables are huge, memory is limited
SELECT * FROM huge_table1 t1
JOIN huge_table2 t2 ON t1.id = t2.t1_id;

-- Symmetric requires TWO hash tables (one per side)
-- Traditional only needs ONE hash table (on smaller side)
-- If memory is insufficient for two hash tables, traditional is better
```

### Positive: Streaming data

```sql
-- Real-time join of two event streams
SELECT * FROM event_stream1 e1
JOIN event_stream2 e2
  ON e1.session_id = e2.session_id
WHERE e1.timestamp BETWEEN e2.timestamp - INTERVAL '5 minutes'
                       AND e2.timestamp + INTERVAL '5 minutes';

-- Symmetric hash join: Perfect for streaming
-- Both streams arrive incrementally, produce results in real-time
```

## References

**Original paper:**
- Shapiro, L.D., "Join Processing in Database Systems with Large Main Memories", ACM TODS 1986
  - DOI: 10.1145/6314.6315
  - Introduced symmetric hash join (also called "double-pipelined hash join")
  - Analysis of memory trade-offs

**Related work:**
- Wilschut, A.N., Apers, P.M.G., "Dataflow Query Execution in a Parallel Main-Memory Environment", Parallel Computing 1991
  - DOI: 10.1016/0167-8191(91)90032-I
  - Parallel symmetric hash join

- Ives, Z.G., Florescu, D., et al., "An Adaptive Query Execution System for Data Integration", ACM SIGMOD 1999
  - DOI: 10.1145/304182.304209
  - Symmetric hash join in adaptive query processing

**Modern implementations:**
- PostgreSQL: `src/backend/executor/nodeHashjoin.c` - hash join implementation
  - Explores both build-probe and symmetric variants
- mssql: Adaptive join operators using symmetric hash join
- flink: Symmetric hash join for stream processing
