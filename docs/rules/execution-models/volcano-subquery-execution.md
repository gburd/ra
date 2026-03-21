# Rule: Volcano Iterator Model - Subquery Execution

**Category:** execution-models
**File:** `rules/execution-models/volcano/volcano-subquery-execution.rra`

## Metadata

- **ID:** `volcano-subquery-execution`
- **Version:** 1.0.0
- **Databases:** postgresql, mysql, oracle, sqlite, mssql
- **Tags:** execution, iterator, volcano, subquery, correlated, decorrelation
- **SQL Standard:** Volcano model
- **Authors:** Goetz Graefe, Won Kim


# Volcano Iterator Model - Subquery Execution

## Description

Subquery execution in the Volcano model handles nested queries that
appear in WHERE, SELECT, and FROM clauses. Correlated subqueries --
which reference columns from the outer query -- are the most
challenging case: in the naive iterator model, the inner query is
re-executed (reopened) for every outer tuple, causing O(N x M)
performance.

**When to apply:** Every query with subqueries must choose an
execution strategy. Correlated subqueries are the primary target
for optimization via decorrelation, caching, or semi-join
transformation.

**Why it matters:** A correlated subquery on a 1M-row outer table
that scans a 100K-row inner table executes 1M x 100K = 100 billion
tuple comparisons in the worst case. Decorrelating this into a hash
semi-join reduces it to ~1.1M operations -- a 90,000x improvement.

**Subquery categories:**
- **Scalar subquery**: Returns single value, used in SELECT or WHERE
- **EXISTS subquery**: Returns boolean, tests for row existence
- **IN subquery**: Tests membership in a set
- **ANY/ALL subquery**: Compares against set with operator
- **Derived table (FROM subquery)**: Materializes as a temporary
  relation, evaluated once
- **Lateral join**: Correlated FROM subquery, re-evaluated per row

## Relational Algebra

```
Subquery execution strategies:

1. Naive correlated execution (nested iteration):
   for each outer_tuple in R:
     result = execute(subquery, bindings=outer_tuple)
     if condition(outer_tuple, result):
       emit(outer_tuple)

   Cost: O(|R| x cost(subquery))
   Each iteration: open → next* → close on inner plan

2. Decorrelation to join:
   -- Correlated:
   SELECT * FROM R WHERE EXISTS (
     SELECT 1 FROM S WHERE S.fk = R.pk AND S.x > 10
   )

   -- Decorrelated to semi-join:
   R ⋉ (σ_{x>10}(S))

   Cost: O(|R| + |S|) with hash semi-join

3. Decorrelation with aggregation:
   -- Correlated scalar subquery:
   SELECT *, (SELECT SUM(amount) FROM orders
              WHERE orders.cust_id = c.id) AS total
   FROM customers c

   -- Decorrelated to left outer join + aggregate:
   customers ⟕ (GROUP BY cust_id: SUM(amount) FROM orders)

4. Result caching (memoization):
   Cache subquery results keyed by correlation parameters.
   On cache hit, skip re-execution.
   Effective when outer has many duplicate correlation values.

   cache = {}
   for each outer_tuple in R:
     key = outer_tuple[corr_columns]
     if key in cache:
       result = cache[key]
     else:
       result = execute(subquery, key)
       cache[key] = result

5. Semi-join / Anti-semi-join transformation:
   EXISTS → SemiJoin
   NOT EXISTS → AntiSemiJoin
   IN → SemiJoin (with null handling)
   NOT IN → AntiSemiJoin (with null handling)
```

## Implementation

```rust
/// Correlated subquery iterator: re-executes inner plan
/// for each outer tuple by passing correlation bindings.
pub struct CorrelatedSubqueryIterator {
    outer: Box<dyn VolcanoIterator>,
    inner_plan: RelExpr,
    correlation_columns: Vec<(usize, usize)>,
    subquery_type: SubqueryType,
    current_outer: Option<Tuple>,
    inner_iter: Option<Box<dyn VolcanoIterator>>,
}

#[derive(Debug, Clone, Copy)]
pub enum SubqueryType {
    Exists,
    NotExists,
    ScalarAggregate,
    InList,
}

impl VolcanoIterator for CorrelatedSubqueryIterator {
    fn open(&mut self) -> Result<()> {
        self.outer.open()
    }

    fn next_tuple(&mut self) -> Result<Option<Tuple>> {
        loop {
            // Get next outer tuple
            let outer_tuple = match self.outer.next_tuple()? {
                Some(t) => t,
                None => return Ok(None),
            };

            // Bind correlation parameters
            let bindings = self.extract_bindings(&outer_tuple);

            // Build and execute inner plan with bindings
            let bound_plan = self.inner_plan.bind(&bindings);
            let mut inner = build_iterator_tree(&bound_plan);
            inner.open()?;

            let result = match self.subquery_type {
                SubqueryType::Exists => {
                    // Just check if any row exists
                    let exists = inner.next_tuple()?.is_some();
                    inner.close()?;
                    exists
                }
                SubqueryType::NotExists => {
                    let exists = inner.next_tuple()?.is_some();
                    inner.close()?;
                    !exists
                }
                SubqueryType::ScalarAggregate => {
                    // Consume all inner rows for aggregate
                    // (handled by aggregate operator inside)
                    let scalar = inner.next_tuple()?;
                    inner.close()?;
                    // Attach scalar result to outer tuple
                    return Ok(Some(
                        outer_tuple.append_scalar(scalar),
                    ));
                }
                SubqueryType::InList => {
                    let mut found = false;
                    loop {
                        match inner.next_tuple()? {
                            None => break,
                            Some(inner_t) => {
                                if outer_tuple.matches(
                                    &inner_t,
                                    &self.correlation_columns,
                                ) {
                                    found = true;
                                    break;
                                }
                            }
                        }
                    }
                    inner.close()?;
                    found
                }
            };

            if result {
                return Ok(Some(outer_tuple));
            }
        }
    }

    fn close(&mut self) -> Result<()> {
        self.outer.close()
    }

    fn schema(&self) -> &Schema {
        self.outer.schema()
    }

    fn estimated_cardinality(&self) -> f64 {
        self.outer.estimated_cardinality() * 0.5
    }
}

/// Memoized subquery: caches results by correlation key
/// to avoid redundant re-execution.
pub struct MemoizedSubqueryIterator {
    outer: Box<dyn VolcanoIterator>,
    inner_plan: RelExpr,
    correlation_columns: Vec<usize>,
    cache: HashMap<Vec<Value>, SubqueryResult>,
    cache_hits: u64,
    cache_misses: u64,
}

impl MemoizedSubqueryIterator {
    fn execute_or_cache(
        &mut self,
        key: Vec<Value>,
    ) -> Result<SubqueryResult> {
        if let Some(cached) = self.cache.get(&key) {
            self.cache_hits += 1;
            return Ok(cached.clone());
        }

        self.cache_misses += 1;

        let bound_plan = self.inner_plan.bind_values(&key);
        let mut inner = build_iterator_tree(&bound_plan);
        inner.open()?;

        let mut rows = Vec::new();
        loop {
            match inner.next_tuple()? {
                Some(t) => rows.push(t),
                None => break,
            }
        }
        inner.close()?;

        let result = SubqueryResult::Rows(rows);
        self.cache.insert(key, result.clone());
        Ok(result)
    }
}

/// Decorrelation: transform correlated subquery to join.
pub fn decorrelate_exists_to_semi_join(
    plan: &RelExpr,
) -> Option<RelExpr> {
    match plan {
        RelExpr::Filter {
            input,
            predicate:
                Expr::Exists {
                    subquery,
                    correlation,
                },
        } => {
            // Extract correlation predicate
            let join_cond =
                build_join_condition(correlation);
            // Extract non-correlated filters from subquery
            let (inner_filters, inner_scan) =
                extract_filters(subquery);

            Some(RelExpr::SemiJoin {
                left: Box::new(*input.clone()),
                right: Box::new(apply_filters(
                    inner_scan,
                    inner_filters,
                )),
                condition: join_cond,
            })
        }
        _ => None,
    }
}

/// Decorrelation: scalar subquery to left outer join + agg.
pub fn decorrelate_scalar_subquery(
    plan: &RelExpr,
) -> Option<RelExpr> {
    match plan {
        RelExpr::Project {
            input,
            columns,
        } if columns.iter().any(|c| c.is_scalar_subquery()) => {
            let (scalar_subquery, corr_cols) =
                extract_scalar_subquery(columns)?;

            // Build left outer join with group-by aggregate
            let agg_subquery = RelExpr::Aggregate {
                input: Box::new(scalar_subquery.scan.clone()),
                group_by: corr_cols.clone(),
                aggregates: vec![scalar_subquery.aggregate],
            };

            let join_cond =
                build_equijoin_on(&corr_cols);

            Some(RelExpr::LeftOuterJoin {
                left: input.clone(),
                right: Box::new(agg_subquery),
                condition: join_cond,
            })
        }
        _ => None,
    }
}

/// Estimate the benefit of decorrelation.
pub fn decorrelation_benefit(
    outer_rows: f64,
    inner_rows: f64,
    distinct_correlation_values: f64,
) -> f64 {
    // Correlated: outer_rows x inner_scan
    let correlated_cost = outer_rows * inner_rows;
    // Decorrelated semi-join: outer + inner (hash)
    let decorrelated_cost = outer_rows + inner_rows;
    // Memoized: distinct_values x inner_scan + cache_lookups
    let memoized_cost = distinct_correlation_values * inner_rows
        + outer_rows;

    let best_alternative =
        decorrelated_cost.min(memoized_cost);
    correlated_cost / best_alternative
}
```

## Preconditions

- Subquery identified during plan optimization
- Correlation columns identified (outer references in inner query)
- For decorrelation: inner query structure is compatible (no LIMIT,
  no volatile functions, no lateral dependencies beyond correlation)
- For memoization: sufficient memory for cache
- NULL semantics handled correctly (IN vs EXISTS)

## Cost Model

**Naive correlated execution:**
- Cost: `outer_rows x (inner_open + inner_rows x per_tuple)`
- For EXISTS: early exit on first match saves inner_rows/2 on average
- For scalar: must always consume full inner result
- Catastrophic for large outer x large inner

**Memoized execution:**
- Cost: `distinct_keys x inner_cost + (outer_rows - distinct_keys) x cache_lookup`
- Cache lookup: O(1) hash map access
- Effective when: `distinct_keys << outer_rows`
- Memory: `distinct_keys x avg_result_size`
- Break-even: memoization wins when `cache_hit_rate > inner_cost / (inner_cost + cache_cost)`

**Decorrelated semi-join:**
- Cost: `outer_rows + inner_rows + hash_build + hash_probe`
- Hash build: O(inner_rows) time and memory
- Hash probe: O(outer_rows) time
- Total: O(outer_rows + inner_rows) -- linear, not quadratic

**Decorrelation speedup:**
- 1M outer x 100K inner: correlated = 100B ops, semi-join = 1.1M ops
- Speedup: ~90,000x
- Even with overhead, decorrelation is nearly always profitable

## Test Cases

```sql
-- Test 1: EXISTS decorrelation
SELECT * FROM customers c
WHERE EXISTS (
  SELECT 1 FROM orders o
  WHERE o.cust_id = c.id AND o.amount > 1000
);
-- Naive: rescan orders for each customer
-- Decorrelated: hash semi-join customers ⋉ orders
-- Verify: same results, O(N+M) instead of O(N*M)

-- Test 2: NOT EXISTS to anti-semi-join
SELECT * FROM customers c
WHERE NOT EXISTS (
  SELECT 1 FROM orders o WHERE o.cust_id = c.id
);
-- Decorrelated: anti-semi-join customers ▷ orders
-- Returns customers with no orders

-- Test 3: Scalar subquery decorrelation
SELECT c.name,
       (SELECT SUM(o.amount)
        FROM orders o
        WHERE o.cust_id = c.id) AS total
FROM customers c;
-- Decorrelated: customers LEFT JOIN (SELECT cust_id, SUM(amount)
--   FROM orders GROUP BY cust_id) ON cust_id
-- Verify: NULL total for customers with no orders

-- Test 4: IN subquery
SELECT * FROM products
WHERE id IN (SELECT product_id FROM order_items
             WHERE quantity > 10);
-- Decorrelated: semi-join products ⋉ order_items
-- Verify: handles NULL product_id correctly

-- Test 5: Memoization benefit
SELECT * FROM events e
WHERE EXISTS (
  SELECT 1 FROM rules r
  WHERE r.category = e.category AND r.active = true
);
-- If events has 10M rows but only 50 distinct categories:
-- Memoized: 50 subquery executions + 10M cache lookups
-- vs naive: 10M subquery executions

-- Test 6: Lateral join (non-decorrelatable)
SELECT c.*, LATERAL (
  SELECT * FROM orders o
  WHERE o.cust_id = c.id
  ORDER BY o.created_at DESC
  LIMIT 3
) AS recent_orders
FROM customers c;
-- LIMIT inside lateral prevents full decorrelation
-- Must use correlated or memoized execution
-- Verify: exactly 3 most recent orders per customer

-- Negative test: NOT IN with NULLs
SELECT * FROM products
WHERE id NOT IN (SELECT product_id FROM order_items);
-- If order_items.product_id contains NULL:
--   NOT IN returns no rows (SQL NULL semantics)
-- Verify: correct NULL handling, prefer NOT EXISTS
```

## References

1. **Kim, Won**. "On Optimizing an SQL-like Nested Query."
   ACM TODS 7(3), 1982.
   - First formal treatment of subquery decorrelation
   - Classification of correlated subquery types

2. **Seshadri, Praveen et al**. "Cost-Based Optimization for
   Magic: Algebra and Implementation." SIGMOD 1996.
   - Magic sets for recursive subquery optimization

3. **Neumann, Thomas; Kemper, Alfons**. "Unnesting Arbitrary
   Queries." BTW 2015.
   - General decorrelation framework
   - Handles complex subquery patterns

4. **PostgreSQL Source**: `src/backend/optimizer/plan/subselect.c`
   - Subquery optimization and decorrelation logic
   - `convert_EXISTS_to_join()`, `convert_IN_to_join()`

5. **Galindo-Legaria, Cesar; Joshi, Milind**. "Orthogonal
   Optimization of Subqueries and Aggregation." SIGMOD 2001.
   - Apply operators for correlated evaluation
   - Systematic decorrelation rules

6. **MySQL Source**: `sql/sql_optimizer.cc`
   - `Item_subselect::select_transformer()` for IN→semi-join
