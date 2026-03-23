# Rule: Volcano Iterator Model - Nested Loop Join

**Category:** execution-models
**File:** `rules/execution-models/volcano/volcano-nested-loop-join.rra`

## Metadata

- **ID:** `volcano-nested-loop-join`
- **Version:** 1.0.0
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** execution, iterator, volcano, join, nested-loop
- **SQL Standard:** Volcano model
- **Authors:** Goetz Graefe


# Volcano Iterator Model - Nested Loop Join

## Description

Nested loop join in the Volcano iterator model implements the classic doubly-nested loop algorithm using the iterator interface. For each tuple from the outer relation, it scans the entire inner relation to find matching tuples. This simple algorithm works for all join types and conditions but has O(N$\times$M) complexity.

**Algorithm:**
```
for each outer_tuple in outer_relation:
    for each inner_tuple in inner_relation:
        if join_condition(outer_tuple, inner_tuple):
            emit(outer_tuple $\bowtie$ inner_tuple)
```

**Advantages:**
- Works for any join predicate (equality, inequality, complex)
- Simple implementation
- Memory-efficient (no hash table)
- Good for small inner relations
- Can stop early (useful for LIMIT)

**Disadvantages:**
- O(N$\times$M) tuple comparisons
- Poor performance for large relations
- Inner relation scanned repeatedly
- High I/O cost if inner not cached

## Relational Algebra

```
NestedLoopJoin(R, S, $\theta$) -> Iterator<Tuple>

NestedLoopIterator implements Iterator {
  outer: Iterator
  inner: Iterator
  current_outer: Tuple | None
  condition: Predicate

  fn open() {
    outer.open()
    current_outer = outer.next()
    if current_outer != None {
      inner.open()
    }
  }

  fn next() -> Tuple | None {
    loop {
      if current_outer == None {
        return None
      }

      inner_tuple = inner.next()

      if inner_tuple != None {
        if condition(current_outer, inner_tuple) {
          return merge(current_outer, inner_tuple)
        }
      } else {
        // Inner exhausted, advance outer
        inner.close()
        current_outer = outer.next()
        if current_outer == None {
          return None
        }
        inner.open()
      }
    }
  }

  fn close() {
    inner.close()
    outer.close()
  }
}
```

## Implementation

```rust
use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::Expr;

/// Volcano-style nested loop join iterator
pub struct NestedLoopJoinIterator {
    outer: Box<dyn Iterator<Item = Tuple>>,
    inner: Box<dyn Iterator<Item = Tuple>>,
    condition: Expr,
    join_type: JoinType,
    current_outer: Option<Tuple>,
}

impl NestedLoopJoinIterator {
    pub fn new(
        outer: Box<dyn Iterator<Item = Tuple>>,
        inner: Box<dyn Iterator<Item = Tuple>>,
        condition: Expr,
        join_type: JoinType,
    ) -> Self {
        Self {
            outer,
            inner,
            condition,
            join_type,
            current_outer: None,
        }
    }
}

impl Iterator for NestedLoopJoinIterator {
    type Item = Tuple;

    fn open(&mut self) -> Result<()> {
        self.outer.open()?;
        self.current_outer = self.outer.next()?;

        if self.current_outer.is_some() {
            self.inner.open()?;
        }

        Ok(())
    }

    fn next(&mut self) -> Result<Option<Tuple>> {
        loop {
            // Check if we have an outer tuple
            let outer_tuple = match &self.current_outer {
                None => return Ok(None), // No more outer tuples
                Some(t) => t,
            };

            // Try to get next inner tuple
            if let Some(inner_tuple) = self.inner.next()? {
                // Evaluate join condition
                if eval_join_condition(&self.condition, outer_tuple, &inner_tuple)? {
                    return Ok(Some(merge_tuples(outer_tuple, &inner_tuple)));
                }
                // Condition failed, continue to next inner tuple
                continue;
            }

            // Inner exhausted, advance outer
            self.inner.close()?;
            self.current_outer = self.outer.next()?;

            if self.current_outer.is_none() {
                return Ok(None); // No more outer tuples
            }

            // Reopen inner for new outer tuple
            self.inner.open()?;
        }
    }

    fn close(&mut self) -> Result<()> {
        self.inner.close()?;
        self.outer.close()?;
        Ok(())
    }
}

/// Cost model for nested loop join
pub fn nested_loop_join_cost(
    outer_rows: f64,
    inner_rows: f64,
    outer_row_size: usize,
    inner_row_size: usize,
    selectivity: f64,
) -> f64 {
    // Tuple comparison cost
    let comparison_cost = 0.0001; // ms per comparison
    let tuple_merge_cost = 0.0005; // ms per merge

    // Total comparisons
    let comparisons = outer_rows * inner_rows;
    let matches = comparisons * selectivity;

    // CPU cost
    let cpu_cost = comparisons * comparison_cost + matches * tuple_merge_cost;

    // I/O cost - inner relation scanned once per outer tuple
    let page_size = 8192;
    let inner_pages = ((inner_rows * inner_row_size as f64) / page_size as f64).ceil();
    let io_cost_per_scan = inner_pages * 0.1; // ms per page
    let total_io = outer_rows * io_cost_per_scan;

    cpu_cost + total_io
}
```

## Cost Model

**CPU Cost:**
- Tuple comparisons: `outer_rows $\times$ inner_rows $\times$ comparison_cost`
- Predicate evaluation: depends on condition complexity
- Tuple merging: `output_rows $\times$ merge_cost`
- **Total CPU:** `O(N $\times$ M)`

**I/O Cost:**
- Outer scan: `outer_pages` (once)
- Inner scan: `inner_pages $\times$ outer_rows` (repeated)
- Cache behavior critical - inner should fit in buffer pool
- **Total I/O:** `O(N) $\times$ O(M)` if uncached

**Memory:**
- O(1) - only current tuples
- No hash table or sort buffer needed
- Minimal memory footprint

**Selectivity Impact:**
- Low selectivity: mostly CPU waste
- High selectivity: many output tuples

**When to Use:**
- Small inner relation (fits in cache)
- Index on inner relation (index nested loop)
- Non-equijoin conditions
- Early termination needed (LIMIT queries)

## Test Cases

```sql
-- Test 1: Small inner relation
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE c.region = 'US';
-- Expected: Nested loop if customers is small
-- Cost: O(orders $\times$ customers_in_US)

-- Test 2: Index nested loop
SELECT * FROM orders o
JOIN products p ON o.product_id = p.id;
-- Expected: Use index on products.id for inner lookups
-- Cost: O(orders $\times$ log(products))

-- Test 3: Non-equijoin
SELECT * FROM events e1
JOIN events e2 ON e1.timestamp < e2.timestamp AND e1.user = e2.user;
-- Expected: Nested loop (can't use hash join)
-- Cost: O(events$^2$) - expensive!

-- Test 4: Cross join with limit
SELECT * FROM a CROSS JOIN b LIMIT 10;
-- Expected: Can stop after 10 results
-- Cost: Minimal if stopped early
```

## References

1. **Graefe, Goetz**. "Query Evaluation Techniques for Large Databases." ACM Computing Surveys, 1993.
   - Comprehensive join algorithm survey
   - Nested loop variations

2. **PostgreSQL Source**: `src/backend/executor/nodeNestloop.c`
   - Production nested loop implementation
   - Shows optimizations and edge cases

3. **Ramakrishnan & Gehrke**. "Database Management Systems", 3rd Ed., Chapter 14.
   - Textbook treatment of join algorithms
   - Cost analysis

4. **Selinger et al**. "Access Path Selection in a Relational Database System." SIGMOD 1979.
   - Original cost-based optimizer
   - Nested loop cost model

5. **MySQL Source**: `sql/sql_executor.cc` - Nested loop join implementation
