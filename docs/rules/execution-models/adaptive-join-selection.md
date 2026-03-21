# Rule: Adaptive Runtime Join Selection

**Category:** execution-models
**File:** `rules/execution-models/adaptive/adaptive-join-selection.rra`

## Metadata

- **ID:** `adaptive-join-selection`
- **Version:** 1.0.0
- **Databases:** mssql, Oracle, Spark, DuckDB
- **Tags:** execution, adaptive, join, runtime, hash-join, merge-join, nested-loop
- **SQL Standard:** Adaptive join processing
- **Authors:** Anisoara Nica, Rainer Gemulla


# Adaptive Runtime Join Selection

## Description

Adaptive join selection defers the choice of join algorithm (hash join, sort-merge join, nested-loop join) until runtime, when actual input sizes, sort order, and memory availability are known. The executor begins with a lightweight probing phase that samples the build input to estimate its cardinality and value distribution. Based on these runtime observations, it selects the most efficient join algorithm and commits to it.

mssql's Adaptive Join (introduced in 2017) is the canonical production implementation: it builds a hash table until a threshold row count is reached, then decides whether to continue with hash join or switch to a nested-loop index join.

**Key characteristics:**
- **Deferred commitment**: Join algorithm chosen after observing build-side rows
- **Threshold-based switching**: Row count threshold determines hash vs. nested-loop
- **Memory-aware**: Considers available memory grant for hash table sizing
- **Index-aware**: Nested-loop path requires suitable index on probe side
- **Zero overhead if threshold met early**: Quick decision, no wasted work

**Trade-offs:**
- Build-side rows processed twice if algorithm switches (hash table discarded)
- Only applicable when both hash and index-NL paths are viable
- Threshold must be calibrated per query/hardware

## Relational Algebra

```
AdaptiveJoin(build, probe, condition) -> Result

fn execute_adaptive_join(build, probe, cond):
  // Phase 1: Probe build input
  buffer = []
  for row in build:
    buffer.push(row)
    if buffer.len() > ADAPTIVE_THRESHOLD:
      // Large build side: commit to hash join
      return hash_join(buffer, build.remaining(), probe, cond)

  // Small build side: use nested-loop with index
  if probe.has_index(cond.probe_columns()):
    return index_nested_loop(buffer, probe, cond)
  else:
    // No suitable index: fall back to hash join anyway
    return hash_join_from_buffer(buffer, probe, cond)
```

## Implementation

```rust
/// Threshold for switching between hash join and index NL
pub struct AdaptiveJoinConfig {
    /// Row count threshold: below = NL, above = hash
    pub row_threshold: usize,
    /// Memory threshold: switch if hash table would exceed
    pub memory_threshold_bytes: usize,
}

impl Default for AdaptiveJoinConfig {
    fn default() -> Self {
        Self {
            row_threshold: 1000,
            memory_threshold_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Runtime join algorithm selection
pub enum JoinDecision {
    HashJoin,
    IndexNestedLoop,
    SortMergeJoin,
}

/// Adaptive join operator
pub struct AdaptiveJoin {
    build_input: Box<dyn Iterator<Item = Row>>,
    probe_input: Box<dyn Iterator<Item = Row>>,
    condition: JoinCondition,
    config: AdaptiveJoinConfig,
    probe_index: Option<IndexHandle>,
}

impl AdaptiveJoin {
    pub fn execute(&mut self) -> Result<Vec<Row>> {
        let mut build_buffer = Vec::new();
        let mut estimated_row_size = 0;

        // Phase 1: Buffer build rows until threshold
        while let Some(row) = self.build_input.next() {
            if estimated_row_size == 0 {
                estimated_row_size = row.size_bytes();
            }
            build_buffer.push(row);

            // Check row count threshold
            if build_buffer.len() > self.config.row_threshold {
                return self.execute_hash_join(build_buffer);
            }

            // Check memory threshold
            let mem_used = build_buffer.len()
                * estimated_row_size;
            if mem_used > self.config.memory_threshold_bytes {
                return self.execute_hash_join(build_buffer);
            }
        }

        // Phase 2: Build side fully consumed, choose algorithm
        self.choose_and_execute(build_buffer)
    }

    fn choose_and_execute(
        &mut self,
        build_buffer: Vec<Row>,
    ) -> Result<Vec<Row>> {
        let decision = self.decide(&build_buffer);
        match decision {
            JoinDecision::IndexNestedLoop => {
                self.execute_index_nl(build_buffer)
            }
            JoinDecision::HashJoin => {
                self.execute_hash_join(build_buffer)
            }
            JoinDecision::SortMergeJoin => {
                self.execute_sort_merge(build_buffer)
            }
        }
    }

    fn decide(&self, buffer: &[Row]) -> JoinDecision {
        let build_rows = buffer.len();

        // If probe index exists and build is small, use NL
        if self.probe_index.is_some()
            && build_rows <= self.config.row_threshold
        {
            return JoinDecision::IndexNestedLoop;
        }

        // If both sides are sorted on join key, use merge
        if self.build_sorted() && self.probe_sorted() {
            return JoinDecision::SortMergeJoin;
        }

        JoinDecision::HashJoin
    }

    fn build_sorted(&self) -> bool {
        // Check if build input delivers sorted output
        false // placeholder
    }

    fn probe_sorted(&self) -> bool {
        false // placeholder
    }

    fn execute_hash_join(
        &mut self,
        buffer: Vec<Row>,
    ) -> Result<Vec<Row>> {
        let mut ht = HashTable::new();
        for row in &buffer {
            ht.insert(&self.condition.build_key(row), row);
        }
        // Continue consuming remaining build input
        while let Some(row) = self.build_input.next() {
            ht.insert(&self.condition.build_key(&row), &row);
        }
        // Probe phase
        let mut results = Vec::new();
        while let Some(probe_row) = self.probe_input.next() {
            let key = self.condition.probe_key(&probe_row);
            for build_row in ht.get(&key) {
                results.push(Row::join(build_row, &probe_row));
            }
        }
        Ok(results)
    }

    fn execute_index_nl(
        &mut self,
        buffer: Vec<Row>,
    ) -> Result<Vec<Row>> {
        let idx = self.probe_index.as_ref()
            .expect("index required for NL");
        let mut results = Vec::new();
        for build_row in &buffer {
            let key = self.condition.build_key(build_row);
            for probe_row in idx.lookup(&key) {
                results.push(Row::join(build_row, &probe_row));
            }
        }
        Ok(results)
    }

    fn execute_sort_merge(
        &mut self,
        buffer: Vec<Row>,
    ) -> Result<Vec<Row>> {
        // Standard sort-merge join on pre-sorted inputs
        let mut results = Vec::new();
        // ... merge logic ...
        Ok(results)
    }
}

/// Cost model for adaptive join decision
pub fn adaptive_join_threshold(
    probe_index_cost: f64,
    hash_build_cost_per_row: f64,
    hash_probe_cost: f64,
    probe_rows: f64,
) -> usize {
    // Threshold where hash join cost equals index NL cost
    // NL cost: build_rows * index_lookup_cost
    // Hash cost: build_rows * build_per_row + probe_rows * probe_per_row
    // Solve for build_rows where NL = Hash
    let threshold = hash_probe_cost * probe_rows
        / (probe_index_cost - hash_build_cost_per_row);
    threshold.max(1.0) as usize
}
```

## Cost Model

**Index Nested-Loop Join:**
- Cost: `build_rows * index_lookup_cost`
- Best when: build side small, probe index available
- Index lookup: ~0.01 ms per random I/O (cached), ~5 ms (disk)

**Hash Join:**
- Build cost: `build_rows * hash_insert_cost`
- Probe cost: `probe_rows * hash_lookup_cost`
- Best when: large build side, no useful index

**Crossover point (threshold):**
- `T = hash_probe_total / (index_lookup_cost - hash_build_per_row)`
- Typically 100-10,000 rows depending on index efficiency
- mssql default: ~1,000 rows (configurable per plan)

**Adaptive overhead:**
- Buffering build rows: negligible (already needed for hash build)
- Decision logic: < 1 microsecond
- Wasted work on algorithm switch: build rows processed twice

## Test Cases

```sql
-- Test 1: Small build side -> nested-loop with index
SELECT o.*, c.name
FROM orders o
JOIN customers c ON o.customer_id = c.customer_id
WHERE o.order_date = '2024-01-01';
-- ~50 orders on that date, customers has PK index
-- Adaptive: choose index NL (50 index lookups)

-- Test 2: Large build side -> hash join
SELECT o.*, l.*
FROM orders o
JOIN lineitem l ON o.order_id = l.order_id;
-- 1.5M orders, 6M lineitems
-- Adaptive: exceeds threshold quickly, commit to hash join

-- Test 3: Parameter-sensitive query
SELECT * FROM orders o
JOIN lineitem l ON o.order_id = l.order_id
WHERE o.status = ?;
-- status='pending': 100 rows -> index NL
-- status='shipped': 1.2M rows -> hash join
-- Adaptive plan handles both correctly

-- Test 4: Both sides sorted -> merge join
SELECT * FROM sorted_a a
JOIN sorted_b b ON a.key = b.key;
-- Both inputs sorted on join key (from index scan or prior sort)
-- Adaptive: detect sorted order, choose merge join
```

## References

1. **mssql Documentation**. "Adaptive Joins in mssql." Microsoft Docs.
   - Production adaptive join implementation since mssql 2017

2. **Avnur, Ron and Joseph Hellerstein**. "Eddies: Continuously Adaptive Query Processing." SIGMOD 2000.
   - Runtime routing of tuples to join operators

3. **Nica, Anisoara et al**. "Statisticum: Data Statistics Management in sap-hana." VLDB 2017.
   - Runtime statistics for adaptive decisions

4. **Neumann, Thomas and Bernhard Radke**. "Adaptive Optimization of Very Large Join Queries." SIGMOD 2018.
   - Adaptive join enumeration for complex queries
