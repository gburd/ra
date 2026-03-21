# Rule: Volcano Iterator Model - Iterator Protocol

**Category:** execution-models
**File:** `rules/execution-models/volcano/volcano-iterator-protocol.rra`

## Metadata

- **ID:** `volcano-iterator-protocol`
- **Version:** 1.0.0
- **Databases:** postgresql, mysql, oracle, sqlite, mssql, duckdb
- **Tags:** execution, iterator, volcano, protocol, open-next-close, contract
- **SQL Standard:** Volcano model
- **Authors:** Goetz Graefe


# Volcano Iterator Model - Iterator Protocol

## Description

The Volcano iterator protocol defines the universal contract for query
execution operators: `open()`, `next()`, `close()`. Every operator in a
query plan implements this three-method interface, enabling arbitrary
composition of operators into execution trees. The parent operator
pulls tuples from its children by calling `next()`, creating a
demand-driven (pull-based) execution model.

**When to apply:** This is the foundational execution model for all
traditional row-at-a-time database engines. Every query plan tree is
an instantiation of this protocol.

**Why it works:** The uniform interface decouples operator
implementation from operator composition. A filter does not need to
know whether its child is a scan, a join, or a subquery -- it only
calls `next()`. This composability enables the optimizer to freely
rearrange operators without changing their implementations.

**Protocol contract:**
- `open()` initializes operator state, recursively opens children
- `next()` returns the next tuple or `None` when exhausted
- `close()` releases resources, recursively closes children
- Calls must follow the sequence: open, next*, close
- After `close()`, the operator may be reopened (correlated subqueries)

**Key properties:**
- **Pull-based**: Control flow originates at the root (consumer)
- **Lazy**: No work happens until a tuple is requested
- **Pipelined**: Tuples flow through operators without materialization
  (unless a pipeline breaker intervenes)
- **Single-threaded**: The classic model uses one thread per query
- **Synchronous**: Each `next()` call blocks until a tuple is available

## Relational Algebra

```
interface Iterator<T> {
  open()  → void          // Initialize state, acquire resources
  next()  → T | None      // Produce next tuple or signal exhaustion
  close() → void          // Release resources, finalize

  // Invariants:
  // 1. open() called exactly once before first next()
  // 2. next() returns None permanently after first None
  // 3. close() called exactly once after last next()
  // 4. After close(), open() may be called again (rescan)
}

// Query plan is a tree of iterators:
//
//   Root(next)
//     ↑ pulls from
//   Join(next)
//     ↑          ↑
//   Scan(A)    Filter(next)
//                ↑
//              Scan(B)
//
// Execution trace for SELECT * FROM A JOIN B ON ... WHERE ...:
//
//   root.open()           → join.open()  → scanA.open(), filter.open() → scanB.open()
//   root.next()           → join.next()  → scanA.next(), filter.next() → scanB.next()
//   ... repeats until None ...
//   root.close()          → join.close() → scanA.close(), filter.close() → scanB.close()

// State machine per operator:
//   CREATED → [open()] → OPENED → [next()→Some]* → [next()→None] → EXHAUSTED → [close()] → CLOSED
//                                                                                            ↓
//                                                                                      [open()] → OPENED (rescan)
```

## Implementation

```rust
use std::fmt;

/// The Volcano iterator trait.
///
/// All query execution operators implement this trait.
/// The lifetime parameter ties tuple references to the
/// iterator's internal buffers.
pub trait VolcanoIterator: fmt::Debug {
    /// Initialize operator state and recursively open children.
    ///
    /// Acquires resources (file handles, locks, buffers).
    /// Must be called before the first `next()`.
    fn open(&mut self) -> Result<()>;

    /// Return the next tuple, or `None` if exhausted.
    ///
    /// After returning `None` once, all subsequent calls
    /// must also return `None` (monotonic exhaustion).
    fn next_tuple(&mut self) -> Result<Option<Tuple>>;

    /// Release resources and recursively close children.
    ///
    /// After `close()`, `open()` may be called again for
    /// rescanning (used by correlated subqueries and
    /// nested loop joins on the inner side).
    fn close(&mut self) -> Result<()>;

    /// Returns the output schema of this operator.
    fn schema(&self) -> &Schema;

    /// Estimated row count for cost-based decisions.
    fn estimated_cardinality(&self) -> f64;
}

/// Operator lifecycle states for debug assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IteratorState {
    Created,
    Opened,
    Exhausted,
    Closed,
}

/// Wrapper that enforces the iterator protocol contract
/// via runtime checks. Used in debug builds.
pub struct ProtocolGuard<I: VolcanoIterator> {
    inner: I,
    state: IteratorState,
    tuples_produced: u64,
}

impl<I: VolcanoIterator> ProtocolGuard<I> {
    pub fn new(inner: I) -> Self {
        Self {
            inner,
            state: IteratorState::Created,
            tuples_produced: 0,
        }
    }
}

impl<I: VolcanoIterator> VolcanoIterator for ProtocolGuard<I> {
    fn open(&mut self) -> Result<()> {
        debug_assert!(
            self.state == IteratorState::Created
                || self.state == IteratorState::Closed,
            "open() called in state {:?}",
            self.state
        );
        self.state = IteratorState::Opened;
        self.tuples_produced = 0;
        self.inner.open()
    }

    fn next_tuple(&mut self) -> Result<Option<Tuple>> {
        debug_assert!(
            self.state == IteratorState::Opened,
            "next() called in state {:?}",
            self.state
        );
        match self.inner.next_tuple()? {
            Some(tuple) => {
                self.tuples_produced += 1;
                Ok(Some(tuple))
            }
            None => {
                self.state = IteratorState::Exhausted;
                Ok(None)
            }
        }
    }

    fn close(&mut self) -> Result<()> {
        debug_assert!(
            self.state == IteratorState::Opened
                || self.state == IteratorState::Exhausted,
            "close() called in state {:?}",
            self.state
        );
        self.state = IteratorState::Closed;
        self.inner.close()
    }

    fn schema(&self) -> &Schema {
        self.inner.schema()
    }

    fn estimated_cardinality(&self) -> f64 {
        self.inner.estimated_cardinality()
    }
}

/// Build a query execution tree from a logical plan.
///
/// Each RelExpr node is translated into a VolcanoIterator.
/// The tree structure mirrors the logical plan.
pub fn build_iterator_tree(
    plan: &RelExpr,
) -> Box<dyn VolcanoIterator> {
    match plan {
        RelExpr::Scan { table, filter } => {
            Box::new(ScanIterator::new(
                table.clone(),
                filter.clone(),
            ))
        }
        RelExpr::Filter { input, predicate } => {
            let child = build_iterator_tree(input);
            Box::new(FilterIterator::new(child, predicate.clone()))
        }
        RelExpr::Project { input, columns } => {
            let child = build_iterator_tree(input);
            Box::new(ProjectIterator::new(
                child,
                columns.clone(),
            ))
        }
        RelExpr::Join {
            left,
            right,
            condition,
            join_type,
        } => {
            let left_iter = build_iterator_tree(left);
            let right_iter = build_iterator_tree(right);
            Box::new(NestedLoopJoinIterator::new(
                left_iter,
                right_iter,
                condition.clone(),
                *join_type,
            ))
        }
        RelExpr::Sort { input, order } => {
            let child = build_iterator_tree(input);
            Box::new(SortIterator::new(child, order.clone()))
        }
        RelExpr::Aggregate {
            input,
            group_by,
            aggregates,
        } => {
            let child = build_iterator_tree(input);
            Box::new(AggregateIterator::new(
                child,
                group_by.clone(),
                aggregates.clone(),
            ))
        }
        RelExpr::Limit { input, count } => {
            let child = build_iterator_tree(input);
            Box::new(LimitIterator::new(child, *count))
        }
    }
}

/// Execute a query plan to completion using the iterator protocol.
pub fn execute_plan(
    plan: &RelExpr,
) -> Result<Vec<Tuple>> {
    let mut root = build_iterator_tree(plan);
    let mut results = Vec::new();

    root.open()?;

    loop {
        match root.next_tuple()? {
            Some(tuple) => results.push(tuple),
            None => break,
        }
    }

    root.close()?;

    Ok(results)
}
```

## Preconditions

- Query plan has been optimized (join order, predicate pushdown)
- Table metadata and statistics are available
- Sufficient memory for pipeline breaker buffers
- Single-threaded execution context (classic Volcano)

## Cost Model

**Per-tuple overhead:**
- Virtual function dispatch: ~5 ns per `next()` call
- Iterator state check: ~1-2 ns
- Function call stack frame: ~2-3 ns
- **Total per-tuple overhead: ~8-10 ns**

**For a plan with depth D and N tuples passing through:**
- Total `next()` calls: `N × D` (each tuple traverses D operators)
- Total protocol overhead: `N × D × 10 ns`
- For 1M rows, depth 5: ~50 ms of pure protocol overhead

**Comparison to alternatives:**
- Compiled (HyPer): eliminates virtual dispatch, ~1 ns/tuple
- Vectorized (MonetDB): amortizes call over ~1024 tuples, ~0.01 ns/tuple
- Volcano overhead is 10-1000x higher per tuple than alternatives

**Memory:**
- O(D) stack frames during execution
- Each operator: O(1) state (cursor, current tuple)
- Pipeline breakers add O(N) materialization cost

**Rescan cost (correlated subqueries):**
- Inner side reopened per outer tuple
- Cost multiplied by outer cardinality
- Critical to decorrelate when possible

## Test Cases

```sql
-- Test 1: Protocol lifecycle - simple query
SELECT name FROM users WHERE age > 30;
-- Expected execution trace:
--   project.open() → filter.open() → scan.open()
--   project.next() → filter.next() → scan.next() [loops until match]
--   ... repeat until scan exhausted ...
--   project.close() → filter.close() → scan.close()
-- Verify: open/close called exactly once each

-- Test 2: Protocol with pipeline breaker
SELECT name FROM users ORDER BY age;
-- Expected: sort.open() consumes ALL tuples from scan
--   sort.next() then produces sorted tuples one-by-one
-- Verify: scan exhausted during sort.open(), not during sort.next()

-- Test 3: Rescan semantics (correlated subquery)
SELECT * FROM orders o
WHERE EXISTS (
  SELECT 1 FROM items i WHERE i.order_id = o.id
);
-- Expected: inner scan opened/closed once per outer tuple
-- Verify: inner open() count = outer row count

-- Test 4: Early termination with LIMIT
SELECT * FROM large_table LIMIT 5;
-- Expected: limit.next() returns None after 5 tuples
--   limit.close() propagates to scan.close()
-- Verify: scan produces only 5 tuples, not full table

-- Test 5: Empty input handling
SELECT * FROM empty_table WHERE id > 0;
-- Expected: scan.next() returns None immediately
--   filter.next() returns None immediately
-- Verify: no errors on empty pipeline
```

## References

1. **Graefe, Goetz**. "Volcano: An Extensible and Parallel Query
   Evaluation System." IEEE TKDE 6(1), 1994.
   - Defines the open/next/close iterator protocol
   - Exchange operator for parallelism

2. **Graefe, Goetz**. "Query Evaluation Techniques for Large
   Databases." ACM Computing Surveys 25(2), 1993.
   - Comprehensive survey of iterator-based execution
   - Cost model analysis for iterator overhead

3. **Neumann, Thomas**. "Efficiently Compiling Efficient Query Plans
   for Modern Hardware." PVLDB 4(9), 2011.
   - Analyzes Volcano overhead (~10 ns per next() call)
   - Proposes compiled alternative (HyPer)

4. **PostgreSQL Source**: `src/backend/executor/execProcnode.c`
   - `ExecProcNode()` dispatches to operator-specific next()
   - `ExecInitNode()` / `ExecEndNode()` for open/close

5. **MySQL Source**: `sql/iterators/row_iterator.h`
   - Modern iterator interface (MySQL 8.0+)
   - `Init()`, `Read()`, `~RowIterator()` mapping to open/next/close
