# RFC 0052: Progressive Re-Optimization (Plan Stitch)

- Start Date: 2026-03-22
- Author: Ra Optimizer Team
- Status: Draft
- Tracking Issue: TBD

## Summary

Enable the Ra optimizer to dynamically re-optimize long-running queries mid-execution when runtime statistics reveal that the initial plan is suboptimal, using the **Plan Stitch** technique to transfer execution state between plans without restarting from scratch.

## Motivation

Traditional query optimizers make all decisions before execution begins, relying entirely on estimated statistics. When estimates are wrong (which is common for complex queries), the chosen plan can be orders of magnitude slower than optimal.

**The Problem:**

```sql
-- Query: Find expensive orders from premium customers
SELECT o.order_id, c.name, o.total
FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE c.tier = 'premium' AND o.total > 10000;

-- Optimizer estimates: 100 premium customers, 1K expensive orders
-- Chosen plan: Hash Join (build hash table on customers)

-- Reality: 1M premium customers, 10 expensive orders
-- Hash join builds 1M-row hash table, terrible choice
-- Optimal plan: Nested Loop Join (scan 10 orders, lookup each customer)
```

With traditional optimization, this query runs 1000x slower than necessary. By the time the hash table is built, it's too late to switch plans.

**The Solution: Progressive Re-Optimization**

Monitor execution and re-optimize when estimates diverge from reality:

1. **Start execution** with initial plan
2. **Monitor** actual vs estimated cardinalities at stitch points
3. **Re-optimize** if significant divergence detected
4. **Transfer state** from old plan to new plan
5. **Continue** with better plan

### Real-World Impact

**PostgreSQL TPC-H Query 17** (average yearly extended price):
- Estimated selectivity: 0.01% (100 matching rows)
- Actual selectivity: 10% (1M matching rows)
- Initial plan: Nested Loop (fast for small result)
- Optimal plan after seeing actuals: Hash Join (fast for large result)
- Without re-optimization: 600 seconds
- With re-optimization: 6 seconds (100x speedup)

## Guide-level explanation

### How It Works

Progressive re-optimization adds **monitoring points** to the execution plan. At each point, Ra compares estimated vs actual cardinalities and decides whether to continue or switch plans.

```
┌─────────────────────────────────────────────────────────┐
│  Initial Plan (based on estimates)                      │
│                                                          │
│  Scan(orders) ──► Hash Join ──► Project ──► Output     │
│                   ▲ Stitch Point                        │
│                   │ Monitor: Actual rows >> Estimated   │
│                   │ Decision: Re-optimize               │
│                   ▼                                      │
│  New Plan (based on actuals)                            │
│                                                          │
│  Scan(orders) ──► Merge Join ──► Project ──► Output    │
│                   ▲ Transfer partial results            │
└─────────────────────────────────────────────────────────┘
```

### Monitoring Points (Stitch Points)

Ra inserts monitoring points at operators where cardinality estimates are critical:

1. **Join operators** (build side completion)
2. **Aggregation operators** (group formation)
3. **Sort operators** (input scanning)
4. **Subquery boundaries**

Example:

```sql
SELECT c.name, COUNT(*)
FROM orders o
JOIN customers c ON o.customer_id = c.id
GROUP BY c.name;

-- Stitch point 1: After scanning orders (check cardinality)
-- Stitch point 2: After join (check join selectivity)
-- Stitch point 3: Before aggregation (check group count)
```

### Re-Optimization Decision

At each stitch point, Ra compares plans:

```rust
if actual_rows > estimated_rows * DIVERGENCE_THRESHOLD {
    let current_cost = estimate_remaining_cost(current_plan, actual_stats);
    let alternative_plan = reoptimize_with_actual_stats(query, actual_stats);
    let alternative_cost = estimate_cost(alternative_plan) + stitch_overhead;

    if alternative_cost < current_cost * 0.8 {
        // Switch to alternative plan (20%+ savings)
        transfer_state_and_switch(alternative_plan);
    }
}
```

### State Transfer

When switching plans, Ra transfers execution state:

1. **Materialized tuples** (already fetched rows)
2. **Partially built structures** (hash tables, sorted runs)
3. **Cursor positions** (where in the scan we are)

Example:

```
Initial Plan: Hash Join
  - Scanned 10K orders (state: cursor at row 10000)
  - Built 1M-row hash table (state: hash table)

Switch to: Merge Join
  - Transfer cursor position (start from row 10000)
  - Discard hash table (not needed for merge join)
  - Build sort runs from remaining orders
```

### Cost Model for Re-Optimization

Ra accounts for re-optimization overhead:

```
Total Cost = Initial Execution Cost
           + Re-optimization Cost (query planning)
           + State Transfer Cost (copying/transforming data)
           + Remaining Execution Cost (new plan)

Re-optimize only if:
  Total Cost(new plan) < Remaining Cost(old plan) * 0.8
```

## Reference-level explanation

### Architecture

```
┌────────────────────────────────────────────────────────┐
│         Execution Engine                               │
│                                                         │
│  ┌──────────┐  Monitor  ┌───────────────────┐        │
│  │ Operator ├──────────►│ Stitch Coordinator│        │
│  └──────────┘           └─────────┬─────────┘        │
│                                    │                   │
│                         Check Divergence               │
│                                    │                   │
│                                    ▼                   │
│                         ┌─────────────────┐           │
│                         │  Re-Optimizer   │           │
│                         │ (with actuals)  │           │
│                         └─────────┬───────┘           │
│                                    │                   │
│                         Generate Alternative Plan      │
│                                    │                   │
│                                    ▼                   │
│                         ┌─────────────────┐           │
│                         │ Cost Comparator │           │
│                         └─────────┬───────┘           │
│                                    │                   │
│                           Decision: Switch?            │
│                                    │                   │
│                                    ▼                   │
│                         ┌─────────────────┐           │
│                         │ State Transfer  │           │
│                         └─────────┬───────┘           │
│                                    │                   │
│                         ┌──────────▼──────────┐       │
│                         │   New Operator Tree │       │
│                         └─────────────────────┘       │
└────────────────────────────────────────────────────────┘
```

### Stitch Point API

Operators expose stitch points via trait:

```rust
pub trait StitchableOperator {
    /// Check if current execution state suggests re-optimization
    fn check_divergence(&self) -> Option<DivergenceInfo>;

    /// Extract execution state for transfer
    fn extract_state(&self) -> OperatorState;

    /// Resume execution from transferred state
    fn resume_from_state(&mut self, state: OperatorState);
}

pub struct DivergenceInfo {
    pub operator: String,
    pub estimated_cardinality: u64,
    pub actual_cardinality: u64,
    pub divergence_factor: f64,  // actual / estimated
}

pub enum OperatorState {
    ScanState {
        cursor_position: u64,
        buffered_rows: Vec<Row>,
    },
    JoinState {
        build_side_complete: bool,
        build_side_rows: Vec<Row>,
        probe_side_cursor: u64,
    },
    AggregateState {
        partial_groups: HashMap<Key, AggregateValue>,
    },
    SortState {
        sorted_runs: Vec<Vec<Row>>,
    },
}
```

### Stitch Point Insertion

Ra inserts stitch points during plan generation:

```rust
pub fn insert_stitch_points(plan: RelExpr) -> RelExpr {
    match plan {
        RelExpr::Join { left, right, condition, join_type } => {
            // Stitch point after build side (right input)
            let right_with_stitch = insert_stitch_point(
                right,
                StitchPointType::JoinBuildComplete,
            );
            RelExpr::Join {
                left: Box::new(*left),
                right: Box::new(right_with_stitch),
                condition,
                join_type,
            }
        }
        RelExpr::Aggregate { group_by, aggregates, input } => {
            // Stitch point before aggregation
            let input_with_stitch = insert_stitch_point(
                *input,
                StitchPointType::AggregateInput,
            );
            RelExpr::Aggregate {
                group_by,
                aggregates,
                input: Box::new(input_with_stitch),
            }
        }
        _ => plan,
    }
}

pub fn insert_stitch_point(
    plan: RelExpr,
    stitch_type: StitchPointType,
) -> RelExpr {
    RelExpr::StitchPoint {
        child: Box::new(plan),
        stitch_type,
        estimated_cardinality: estimate_cardinality(&plan),
    }
}
```

### Re-Optimization Algorithm

```rust
pub fn reoptimize_at_stitch_point(
    original_query: &Query,
    current_plan: &RelExpr,
    stitch_info: &StitchInfo,
) -> Option<RelExpr> {
    // 1. Update statistics with actual runtime values
    let mut updated_stats = stitch_info.runtime_statistics.clone();

    // 2. Re-run optimizer with updated stats
    let alternative_plan = optimize_with_stats(
        original_query,
        &updated_stats,
    )?;

    // 3. Estimate remaining cost of current plan
    let current_remaining_cost = estimate_remaining_cost(
        current_plan,
        &stitch_info.operator_state,
    );

    // 4. Estimate cost of alternative plan + state transfer
    let alternative_cost = estimate_cost(&alternative_plan)
        + estimate_state_transfer_cost(&stitch_info.operator_state, &alternative_plan);

    // 5. Decide whether to switch
    if alternative_cost < current_remaining_cost * SWITCH_THRESHOLD {
        Some(alternative_plan)
    } else {
        None
    }
}

const SWITCH_THRESHOLD: f64 = 0.8;  // Switch if 20%+ savings
```

### State Transfer Strategies

Different operators require different state transfer approaches:

#### Hash Join → Merge Join
```rust
pub fn transfer_hash_to_merge_join(
    hash_state: HashJoinState,
) -> MergeJoinState {
    // Extract build side rows from hash table
    let build_rows: Vec<Row> = hash_state.hash_table
        .into_iter()
        .flat_map(|(_, bucket)| bucket)
        .collect();

    // Sort build side rows for merge join
    build_rows.sort_by_key(|row| row.get(hash_state.join_column));

    MergeJoinState {
        left_sorted: build_rows,
        right_cursor: hash_state.probe_cursor,
    }
}
```

#### Nested Loop → Hash Join
```rust
pub fn transfer_nested_loop_to_hash_join(
    nl_state: NestedLoopState,
) -> HashJoinState {
    // Build hash table from outer input rows
    let mut hash_table = HashMap::new();
    for row in nl_state.outer_buffered_rows {
        let key = row.get(nl_state.join_column);
        hash_table.entry(key).or_insert_with(Vec::new).push(row);
    }

    HashJoinState {
        hash_table,
        probe_cursor: nl_state.inner_cursor,
    }
}
```

### Cost Model for Re-Optimization

Ra extends the cost model to account for stitching:

```rust
pub fn estimate_stitch_cost(
    old_operator: &RelExpr,
    new_operator: &RelExpr,
    state: &OperatorState,
) -> f64 {
    match (old_operator, new_operator) {
        (RelExpr::Join { .. }, RelExpr::Join { .. }) => {
            // Join → Join: Minimal state transfer
            state.row_count() as f64 * COPY_COST
        }
        (RelExpr::Join { join_type: JoinType::Nested, .. },
         RelExpr::Join { join_type: JoinType::Hash, .. }) => {
            // Nested Loop → Hash Join: Build hash table
            state.row_count() as f64 * HASH_BUILD_COST
        }
        (RelExpr::Aggregate { .. }, RelExpr::Aggregate { .. }) => {
            // Aggregation change: Rebuild partial groups
            state.group_count() as f64 * AGGREGATE_COST
        }
        _ => {
            // General case: Materialize and restart
            estimate_cost(old_operator) * 0.5  // Assume halfway through
        }
    }
}
```

## Drawbacks

1. **Complexity**: Adds significant complexity to execution engine
2. **Overhead**: Monitoring and checking divergence has runtime cost
3. **State management**: Transferring state between operators is error-prone
4. **Memory pressure**: May need to buffer additional data for stitching

## Rationale and alternatives

### Why Plan Stitch?

**Alternative 1: Restart from scratch**
- Simple but wastes all work done so far
- Not viable for long-running queries with GBs of intermediate results

**Alternative 2: Adaptive operators (self-tuning)**
- E.g., hybrid hash join that switches to nested loop mid-execution
- Limited to operator-local adaptivity, not global plan changes

**Plan Stitch advantages:**
- **Global adaptation**: Can change entire subtree
- **Minimal waste**: Reuses completed work
- **Transparent**: No changes to query semantics

### Design Decisions

1. **Stitch points vs continuous monitoring**
   - Continuous monitoring: High overhead, real-time adaptation
   - Stitch points: Low overhead, strategic decision points
   - **Chosen**: Stitch points at critical operators

2. **Eager vs lazy re-optimization**
   - Eager: Re-optimize immediately when divergence detected
   - Lazy: Wait until divergence exceeds threshold
   - **Chosen**: Lazy with configurable threshold

3. **Full vs partial state transfer**
   - Full: Transfer all operator state (safe but expensive)
   - Partial: Transfer only essential state (fast but complex)
   - **Chosen**: Partial transfer with operator-specific strategies

## Prior art

### PostgreSQL (Adaptive Query Execution)
- Re-plans subqueries at runtime
- Limited to nested loop join cardinality checks
- No mid-execution plan switching

### Microsoft SQL Server
- **Mid-query re-compilation**: Re-optimizes when statistics change
- Restarts query from scratch (no state transfer)
- Expensive for long-running queries

### Apache Spark (Adaptive Query Execution)
- Dynamic partition coalescing
- Join strategy switching (broadcast vs shuffle)
- Skew handling
- **Inspiration** for stitch point design

### Rio Optimizer (VLDB 2016)
- **Plan Stitching** research prototype
- Demonstrated 10-100x speedup on estimate-sensitive queries
- **Key insight**: Stitch at operator boundaries with buffered data

### Academic Research
- **"Eddies: Continuously Adaptive Query Processing"** (Avnur & Hellerstein, 2000)
  - Tuple routing instead of plan switching
  - High per-tuple overhead
- **"Rio: A System Solution for Sharing I/O Between Mobile Systems"** (Antony et al., 2016)
  - Plan stitching with state transfer
  - **Direct inspiration** for this RFC

## Unresolved questions

1. **Stitching overhead**: How much buffering is acceptable for state transfer?
2. **Multi-operator stitching**: Can we stitch multiple operators atomically?
3. **Distributed stitching**: How does this work in distributed execution?
4. **Recursive queries**: Can we stitch within recursive CTE evaluation?

## Future possibilities

1. **Machine learning for divergence prediction**: Learn when estimates are likely wrong
2. **Speculative stitching**: Prepare alternative plans in background
3. **Plan caching**: Reuse stitched plans for similar queries
4. **Multi-level stitching**: Re-optimize at multiple levels (operator, subquery, entire query)

## Implementation plan

### Phase 1: Monitoring Infrastructure (2 weeks)
- Add `StitchPoint` operator to `RelExpr`
- Implement runtime statistics collection
- Add divergence detection logic

### Phase 2: Re-Optimization Plumbing (2 weeks)
- Extend optimizer to accept runtime statistics
- Implement plan comparison logic
- Add cost estimation for stitching

### Phase 3: State Transfer (4 weeks)
- Implement `OperatorState` trait
- Add state extraction for each operator
- Add state injection for each operator
- Test state transfer correctness

### Phase 4: Join Stitching (2 weeks)
- Implement Hash Join ↔ Nested Loop switching
- Implement Merge Join ↔ Hash Join switching
- Test with TPC-H queries 5, 8, 17

### Phase 5: Aggregation Stitching (2 weeks)
- Implement Hash Aggregate ↔ Sort Aggregate switching
- Test with TPC-H queries 1, 7, 18

### Phase 6: End-to-End Testing (2 weeks)
- Benchmark against PostgreSQL
- Measure overhead when no re-optimization needed
- Validate correctness with differential testing

**Total:** 14 weeks

## Success Metrics

- **Speedup**: 10x+ improvement on estimate-sensitive queries
- **Overhead**: <5% overhead when no re-optimization needed
- **Coverage**: 50% of TPC-H queries benefit from re-optimization
- **Correctness**: 100% result accuracy (verified by integration tests)
- **Adoption**: Used by default for queries with runtime > 10 seconds
