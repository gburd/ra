# Chapter 10: Advanced Optimization Features

## Beyond the Basics

Alice's ledger has grown into a sophisticated financial system. She needs advanced optimization features: covering indexes for instant lookups, bitmap scans for complex filters, partition-wise joins for time-series data, and adaptive query execution. Let's explore RA's cutting-edge optimization capabilities.

## Covering Indexes (Index-Only Scans)

A covering index contains all columns needed by a query, eliminating table access:

### Creating the Perfect Covering Index

```sql-interactive
-- Without covering index: Must read table
EXPLAIN (ANALYZE, BUFFERS) SELECT
    transaction_date,
    debit_amount,
    credit_amount
FROM ledger_transactions
WHERE debit_account_code = '1010'
  AND transaction_date >= '2024-01-01';

-- Plan: Index Scan + Heap Fetch
-- Buffers: shared hit=245 (index) + 892 (table)

-- Create covering index
CREATE INDEX idx_covering_debit
ON ledger_transactions (debit_account_code, transaction_date)
INCLUDE (debit_amount, credit_amount);

-- With covering index: No table access!
EXPLAIN (ANALYZE, BUFFERS) SELECT
    transaction_date,
    debit_amount,
    credit_amount
FROM ledger_transactions
WHERE debit_account_code = '1010'
  AND transaction_date >= '2024-01-01';

-- Plan: Index Only Scan
-- Buffers: shared hit=245 (only index!)
-- 78% fewer I/O operations!
```

### When Covering Indexes Win

```sql-interactive
-- Perfect for aggregations
CREATE INDEX idx_covering_summary
ON ledger_transactions (debit_account_code)
INCLUDE (debit_amount, transaction_date);

-- This query never touches the table!
SELECT
    debit_account_code,
    COUNT(*) as transactions,
    SUM(debit_amount) as total,
    MAX(transaction_date) as last_transaction
FROM ledger_transactions
WHERE debit_account_code LIKE '10%'
GROUP BY debit_account_code;
```

## Bitmap Index Scans

Bitmap scans combine multiple index conditions efficiently:

### Understanding Bitmap Operations

```sql-interactive
-- Complex OR condition
EXPLAIN (ANALYZE, BUFFERS) SELECT *
FROM ledger_transactions
WHERE (debit_account_code = '1010' AND transaction_date = '2024-01-15')
   OR (debit_account_code = '5010' AND debit_amount > 1000)
   OR (credit_account_code = '4010' AND credit_amount > 500);
```

**Bitmap Execution Plan**:
```
Bitmap Heap Scan
  └── BitmapOr
      ├── BitmapAnd
      │   ├── Bitmap Index Scan (debit_account_code = '1010')
      │   └── Bitmap Index Scan (transaction_date = '2024-01-15')
      ├── BitmapAnd
      │   ├── Bitmap Index Scan (debit_account_code = '5010')
      │   └── Bitmap Index Scan (debit_amount > 1000)
      └── BitmapAnd
          ├── Bitmap Index Scan (credit_account_code = '4010')
          └── Bitmap Index Scan (credit_amount > 500)
```

### Bitmap vs Regular Index Scan

```sql-interactive
-- When bitmap wins: Multiple conditions, moderate selectivity
SELECT COUNT(*)
FROM ledger_transactions
WHERE debit_account_code IN ('1010', '1020', '1030', '1040', '1050')
  AND transaction_date >= '2024-01-01'
  AND debit_amount BETWEEN 100 AND 1000;

-- Bitmap: Combines three indexes efficiently
-- Regular: Would need multiple passes or ignore indexes
```

## Partition-Wise Operations

Alice partitions by month for better performance:

### Setting Up Partitioning

```sql-interactive
-- Create partitioned table
CREATE TABLE ledger_transactions_partitioned (
    LIKE ledger_transactions INCLUDING ALL
) PARTITION BY RANGE (transaction_date);

-- Create monthly partitions
CREATE TABLE ledger_transactions_2024_01
PARTITION OF ledger_transactions_partitioned
FOR VALUES FROM ('2024-01-01') TO ('2024-02-01');

CREATE TABLE ledger_transactions_2024_02
PARTITION OF ledger_transactions_partitioned
FOR VALUES FROM ('2024-02-01') TO ('2024-03-01');

-- And so on...
```

### Partition Pruning

```sql-interactive
-- RA automatically prunes irrelevant partitions
EXPLAIN (ANALYZE) SELECT *
FROM ledger_transactions_partitioned
WHERE transaction_date = '2024-01-15';

-- Plan: Only scans January partition!
Append
  └── Seq Scan on ledger_transactions_2024_01
      -- Other partitions pruned!
```

### Partition-Wise Joins

```sql-interactive
-- Both tables partitioned by date
EXPLAIN SELECT *
FROM orders_partitioned o
JOIN payments_partitioned p
  ON o.order_id = p.order_id
  AND o.order_date = p.payment_date
WHERE o.order_date >= '2024-01-01'
  AND o.order_date < '2024-02-01';

-- RA performs join per partition!
Append
  ├── Hash Join
  │   ├── Seq Scan on orders_2024_01
  │   └── Hash
  │       └── Seq Scan on payments_2024_01
  └── (Other months pruned)
```

## Adaptive Query Execution

RA can change plans mid-execution based on actual data:

### Adaptive Join Selection

```sql-interactive
-- Enable adaptive execution
SET enable_adaptive_execution = true;

-- Query with uncertain selectivity
PREPARE adaptive_query AS
SELECT *
FROM ledger_transactions t1
JOIN ledger_transactions t2
  ON t1.reference_id = t2.id
WHERE t1.amount > $1;

-- RA's adaptive strategy:
EXECUTE adaptive_query(100);    -- Starts with nested loop
EXECUTE adaptive_query(10);     -- Might switch to hash join
EXECUTE adaptive_query(1);      -- Might switch to merge join
```

### Runtime Statistics Collection

```sql-interactive
-- RA collects statistics during execution
EXPLAIN (ANALYZE, BUFFERS) SELECT
    account_type,
    COUNT(*) FILTER (WHERE debit_amount > 1000) as large_transactions,
    COUNT(*) FILTER (WHERE debit_amount <= 1000) as small_transactions
FROM ledger_transactions t
JOIN chart_of_accounts a ON t.debit_account_code = a.account_code
GROUP BY account_type;

-- RA learns:
-- - Actual vs estimated rows
-- - Filter selectivity
-- - Join selectivity
-- Uses this for future queries!
```

## Incremental View Maintenance

Keep materialized views fresh efficiently:

```sql-interactive
-- Create materialized view with incremental refresh
CREATE MATERIALIZED VIEW account_daily_summary AS
SELECT
    account_code,
    transaction_date,
    SUM(debit_amount) as daily_debit,
    SUM(credit_amount) as daily_credit,
    COUNT(*) as transaction_count
FROM ledger_transactions
GROUP BY account_code, transaction_date;

-- Traditional refresh (recomputes everything)
REFRESH MATERIALIZED VIEW account_daily_summary;

-- Incremental refresh (only new data)
REFRESH MATERIALIZED VIEW CONCURRENTLY account_daily_summary;

-- RA tracks changes and updates only affected rows!
```

## Join Elimination

RA removes unnecessary joins:

```sql-interactive
-- Query with potentially unnecessary join
SELECT
    t.transaction_id,
    t.amount,
    t.transaction_date
FROM ledger_transactions t
JOIN chart_of_accounts a ON t.debit_account_code = a.account_code
WHERE t.transaction_date = '2024-01-15';

-- RA realizes: No columns from 'a' in output
-- RA realizes: Foreign key guarantees join won't filter
-- Optimized plan: Join eliminated!

SELECT
    transaction_id,
    amount,
    transaction_date
FROM ledger_transactions
WHERE transaction_date = '2024-01-15';
```

## Predicate Inference

RA infers additional predicates:

```sql-interactive
-- Original query
SELECT *
FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE c.country = 'USA'
  AND o.order_date = '2024-01-15';

-- RA infers transitivity
SELECT *
FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE c.country = 'USA'
  AND o.order_date = '2024-01-15'
  AND c.id IN (SELECT id FROM customers WHERE country = 'USA');  -- Inferred!

-- Enables better join order and filtering
```

## Parallel Aggregation Strategies

### Parallel Hash Aggregation

```sql-interactive
-- Large aggregation parallelizes well
SET max_parallel_workers_per_gather = 4;

EXPLAIN (ANALYZE) SELECT
    debit_account_code,
    EXTRACT(YEAR FROM transaction_date) as year,
    COUNT(*) as transactions,
    SUM(debit_amount) as total
FROM ledger_transactions
GROUP BY debit_account_code, EXTRACT(YEAR FROM transaction_date);

-- Plan:
Finalize HashAggregate
  └── Gather Merge
      └── Partial HashAggregate  -- Each worker aggregates
          └── Parallel Seq Scan
              Workers: 4
```

### Two-Phase Aggregation

```sql-interactive
-- RA uses two-phase for high-cardinality groups
SELECT
    transaction_id,  -- Unique values!
    SUM(amount) as total
FROM large_table
GROUP BY transaction_id;

-- Phase 1: Local pre-aggregation
-- Phase 2: Final aggregation
-- Reduces memory pressure
```

## Query Result Caching

RA can cache frequent query results:

```sql-interactive
-- Enable result caching
SET enable_result_cache = on;
SET result_cache_size = '256MB';

-- First execution: Computes and caches
SELECT
    account_type,
    COUNT(*) as count,
    SUM(debit_amount) as total
FROM ledger_transactions t
JOIN chart_of_accounts a ON t.debit_account_code = a.account_code
WHERE transaction_date >= DATE_TRUNC('month', CURRENT_DATE)
GROUP BY account_type;
-- Time: 234ms

-- Second execution: From cache!
-- Time: 2ms (100x faster!)

-- Cache invalidated on:
-- - Table updates
-- - Statistics changes
-- - TTL expiration
```

## Lateral Joins and Apply

Advanced join patterns:

```sql-interactive
-- LATERAL allows correlated subqueries in FROM
SELECT
    a.account_code,
    a.account_name,
    latest.transaction_date,
    latest.amount
FROM chart_of_accounts a
CROSS JOIN LATERAL (
    SELECT
        transaction_date,
        debit_amount as amount
    FROM ledger_transactions t
    WHERE t.debit_account_code = a.account_code
    ORDER BY transaction_date DESC
    LIMIT 3
) latest
WHERE a.account_type = 'ASSET';

-- Efficiently gets top-3 transactions per account
```

## Index Skip Scan

For low-cardinality leading columns:

```sql-interactive
-- Index on (account_type, account_code, transaction_date)
-- But querying without account_type:

SELECT DISTINCT account_code
FROM ledger_transactions
WHERE transaction_date = '2024-01-15';

-- RA uses Index Skip Scan:
-- Skips through account_type values
-- More efficient than full index scan
```

## Merge Append Optimization

For sorted union operations:

```sql-interactive
-- Union of sorted sources
(SELECT * FROM transactions_2024_01 ORDER BY transaction_date)
UNION ALL
(SELECT * FROM transactions_2024_02 ORDER BY transaction_date)
ORDER BY transaction_date;

-- RA uses Merge Append:
-- Maintains sort order without re-sorting!
MergeAppend
  ├── Index Scan on transactions_2024_01
  └── Index Scan on transactions_2024_02
```

## Advanced Statistics

### Multi-Column Statistics

```sql-interactive
-- Create statistics on correlated columns
CREATE STATISTICS account_correlation (dependencies, ndistinct, mcv)
ON account_type, normal_balance
FROM chart_of_accounts;

-- RA now knows:
-- - ASSET accounts always have DEBIT normal_balance
-- - Improves selectivity estimates
```

### Expression Statistics

```sql-interactive
-- Statistics on expressions
CREATE STATISTICS date_extract_stats
ON EXTRACT(YEAR FROM transaction_date),
   EXTRACT(MONTH FROM transaction_date)
FROM ledger_transactions;

-- Improves estimates for:
SELECT * FROM ledger_transactions
WHERE EXTRACT(YEAR FROM transaction_date) = 2024;
```

## Optimization Hints

Fine-tune RA's decisions:

```sql-interactive
-- Force specific join order
SELECT /*+ LEADING(t a) USE_HASH(t a) */
    a.account_name,
    SUM(t.debit_amount)
FROM ledger_transactions t
JOIN chart_of_accounts a ON t.debit_account_code = a.account_code
GROUP BY a.account_name;

-- Available hints:
-- Join order: LEADING(table_list)
-- Join method: USE_HASH, USE_MERGE, USE_NL
-- Access path: INDEX(table index_name), NO_INDEX
-- Parallelism: PARALLEL(n), NO_PARALLEL
```

## Practice Exercises

### Exercise 1: Design Covering Index

```sql-interactive
-- This query runs frequently. Design the optimal covering index:
SELECT
    customer_id,
    order_date,
    status,
    total_amount
FROM orders
WHERE status IN ('PENDING', 'PROCESSING')
  AND order_date >= CURRENT_DATE - 7
ORDER BY order_date DESC, total_amount DESC
LIMIT 50;

-- Your covering index:
-- CREATE INDEX ... ON orders (...) INCLUDE (...);
```

### Exercise 2: Optimize Complex Query

```sql-interactive
-- Apply advanced optimizations to this slow query:
WITH monthly_accounts AS (
    SELECT DISTINCT
        DATE_TRUNC('month', transaction_date) as month,
        debit_account_code as account_code
    FROM ledger_transactions
    WHERE transaction_date >= '2023-01-01'
),
account_totals AS (
    SELECT
        DATE_TRUNC('month', transaction_date) as month,
        debit_account_code as account_code,
        SUM(debit_amount) as total
    FROM ledger_transactions
    WHERE transaction_date >= '2023-01-01'
    GROUP BY 1, 2
)
SELECT
    ma.month,
    COUNT(DISTINCT ma.account_code) as active_accounts,
    SUM(at.total) as month_total,
    AVG(at.total) as avg_per_account
FROM monthly_accounts ma
LEFT JOIN account_totals at
    ON ma.month = at.month
    AND ma.account_code = at.account_code
GROUP BY ma.month
ORDER BY ma.month;

-- Identify optimization opportunities:
-- [ ] Eliminate redundant CTE
-- [ ] Add covering index
-- [ ] Use window functions
-- [ ] Partition by month
```

## Key Takeaways

1. **Covering indexes eliminate table access**
   - Include all needed columns
   - Dramatic I/O reduction
   - Perfect for hot queries

2. **Bitmap scans handle complex filters**
   - Combine multiple indexes
   - Efficient OR conditions
   - Better than multiple scans

3. **Partitioning scales with data**
   - Automatic partition pruning
   - Partition-wise joins
   - Parallel partition processing

4. **Adaptive execution handles uncertainty**
   - Runtime plan changes
   - Learn from execution
   - Better than static plans

5. **Advanced features compound benefits**
   - Combine techniques
   - Layer optimizations
   - Massive speedups possible

## Conclusion

You've completed the RA Query Optimizer interactive guide! Through Alice's ledger system, you've learned:

- ✅ How query optimizers transform SQL into efficient plans
- ✅ The role of statistics in optimization decisions
- ✅ Cost-based optimization principles
- ✅ Rule-based transformations
- ✅ Hardware-aware planning
- ✅ Cross-database translation
- ✅ Advanced optimization techniques

## Your Optimization Toolkit

You now have the knowledge to:
1. Read and understand query plans
2. Identify optimization opportunities
3. Design effective indexes
4. Tune queries for specific hardware
5. Diagnose performance problems
6. Apply advanced techniques

## Next Steps

- 📚 Explore RA's source code
- 🔬 Experiment with your own queries
- 🎯 Apply these techniques to real databases
- 🤝 Contribute to RA's development

---

*🎉 Congratulations! You've mastered query optimization with RA. Remember: optimization is both art and science. Keep experimenting, measuring, and learning. Happy optimizing!*