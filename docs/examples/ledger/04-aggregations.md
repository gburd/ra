# Chapter 4: Aggregation Optimization

## The Heart of Business Intelligence

Every month, Alice needs financial reports: profit & loss statements, balance sheets, cash flow analysis. These queries aggregate thousands of transactions into meaningful summaries. Let's see how RA optimizes aggregations for speed and efficiency.

## Understanding Aggregation Costs

Aggregations are expensive because they must:
1. Read all relevant rows
2. Group by key columns
3. Compute aggregate functions
4. Sort results (sometimes)

```sql-interactive
-- A typical monthly summary
SELECT
    a.account_type,
    COUNT(*) as transaction_count,
    SUM(t.debit_amount) as total_debits,
    SUM(t.credit_amount) as total_credits,
    AVG(t.debit_amount) as avg_debit
FROM chart_of_accounts a
JOIN ledger_transactions t ON a.account_code = t.debit_account_code
WHERE t.transaction_date >= DATE_TRUNC('month', CURRENT_DATE)
GROUP BY a.account_type
ORDER BY total_debits DESC;
```

## Aggregation Strategies

### Strategy 1: Hash Aggregation

Best for unsorted data with moderate group count:

```sql-interactive
-- Hash aggregation example
EXPLAIN SELECT
    account_type,
    COUNT(*) as cnt
FROM chart_of_accounts
GROUP BY account_type;
```

**Plan**:
```
HashAggregate
  Group Key: account_type
  └── SeqScan (chart_of_accounts)
```

**Cost Model**:
- Build hash table: O(n)
- Memory needed: groups × tuple_size
- Good for: < 10,000 groups

### Strategy 2: Sort Aggregation

Best when data is pre-sorted or needs sorting anyway:

```sql-interactive
-- Sort aggregation (when ORDER BY matches GROUP BY)
EXPLAIN SELECT
    transaction_date,
    COUNT(*) as daily_transactions,
    SUM(debit_amount) as daily_total
FROM ledger_transactions
GROUP BY transaction_date
ORDER BY transaction_date;
```

**Plan**:
```
GroupAggregate
  Group Key: transaction_date
  └── Sort
      Sort Key: transaction_date
      └── SeqScan (ledger_transactions)
```

### Strategy 3: Index-Aided Aggregation

When index provides sorted order:

```sql-interactive
-- Index eliminates sort step
CREATE INDEX idx_transaction_date ON ledger_transactions(transaction_date);

EXPLAIN SELECT
    transaction_date,
    COUNT(*) as daily_transactions
FROM ledger_transactions
GROUP BY transaction_date
ORDER BY transaction_date;
```

**Optimized Plan**:
```
GroupAggregate
  Group Key: transaction_date
  └── IndexScan (idx_transaction_date)  -- Already sorted!
```

## Aggregate Pushdown

RA pushes aggregations through joins when possible:

### Before Optimization

```sql-interactive
-- Naive approach: Join everything, then aggregate
SELECT
    je.entry_date,
    COUNT(lt.id) as transaction_count,
    SUM(lt.debit_amount) as total_amount
FROM journal_entries je
JOIN ledger_transactions lt ON je.id = lt.journal_entry_id
GROUP BY je.entry_date;
```

### After Aggregate Pushdown

```sql-interactive
-- RA's optimization: Aggregate before joining
WITH transaction_summary AS (
    SELECT
        journal_entry_id,
        COUNT(*) as transaction_count,
        SUM(debit_amount) as total_amount
    FROM ledger_transactions
    GROUP BY journal_entry_id
)
SELECT
    je.entry_date,
    SUM(ts.transaction_count) as transaction_count,
    SUM(ts.total_amount) as total_amount
FROM journal_entries je
JOIN transaction_summary ts ON je.id = ts.journal_entry_id
GROUP BY je.entry_date;
```

**Why This Helps**:
- Smaller join (fewer rows after aggregation)
- Less memory usage
- Parallel aggregation possible

## Multi-Level Grouping

Alice needs hierarchical summaries:

```sql-interactive
-- Rollup: Multiple grouping levels
SELECT
    COALESCE(account_type, 'TOTAL') as level,
    COALESCE(DATE_TRUNC('month', transaction_date)::text, 'All Months') as month,
    COUNT(*) as transactions,
    SUM(debit_amount) as total_debits
FROM ledger_transactions t
JOIN chart_of_accounts a ON t.debit_account_code = a.account_code
WHERE transaction_date >= '2024-01-01'
GROUP BY ROLLUP(account_type, DATE_TRUNC('month', transaction_date))
ORDER BY account_type NULLS LAST, month NULLS LAST;
```

**RA's Optimization**:
```
Sort
  └── MixedAggregate  -- Multiple grouping sets
      ├── GroupingSet: (account_type, month)
      ├── GroupingSet: (account_type)
      └── GroupingSet: ()
      └── Join
          └── ...
```

## Distinct Optimization

DISTINCT has special optimization paths:

```sql-interactive
-- COUNT(DISTINCT ...) is expensive
SELECT
    account_type,
    COUNT(DISTINCT debit_account_code) as unique_accounts,
    COUNT(DISTINCT transaction_date) as active_days
FROM ledger_transactions t
JOIN chart_of_accounts a ON t.debit_account_code = a.account_code
GROUP BY account_type;
```

**RA's Strategies**:

1. **Hash-based DISTINCT**: For high cardinality
2. **Sort-based DISTINCT**: For low memory
3. **Bitmap DISTINCT**: For integer keys
4. **HyperLogLog**: For approximate counts

## Having Clause Optimization

HAVING filters after aggregation:

```sql-interactive
-- Find accounts with unusual activity
SELECT
    debit_account_code,
    COUNT(*) as transaction_count,
    AVG(debit_amount) as avg_amount,
    STDDEV(debit_amount) as stddev_amount
FROM ledger_transactions
WHERE transaction_date >= CURRENT_DATE - 30
GROUP BY debit_account_code
HAVING COUNT(*) > 100
   AND STDDEV(debit_amount) > AVG(debit_amount) * 2;
```

**Optimization Note**: RA cannot push HAVING filters before aggregation, but it can:
- Use partial aggregation
- Prune groups early if possible
- Combine multiple aggregate computations

## Materialized Aggregates

For frequently-needed summaries:

```sql-interactive
-- Create materialized view for daily summaries
CREATE MATERIALIZED VIEW daily_account_summary AS
SELECT
    transaction_date,
    debit_account_code as account_code,
    COUNT(*) as transaction_count,
    SUM(debit_amount) as total_amount,
    MIN(debit_amount) as min_amount,
    MAX(debit_amount) as max_amount,
    AVG(debit_amount) as avg_amount
FROM ledger_transactions
GROUP BY transaction_date, debit_account_code;

-- Create index for fast lookups
CREATE INDEX idx_daily_summary
ON daily_account_summary(account_code, transaction_date);

-- Now queries are instant
SELECT *
FROM daily_account_summary
WHERE account_code = '1010'
  AND transaction_date >= '2024-01-01';
```

## Parallel Aggregation

RA can parallelize aggregations:

```sql-interactive
-- Large aggregation benefits from parallelism
SET max_parallel_workers_per_gather = 4;

EXPLAIN SELECT
    DATE_TRUNC('month', transaction_date) as month,
    account_type,
    COUNT(*) as transactions,
    SUM(debit_amount) as total
FROM ledger_transactions t
JOIN chart_of_accounts a ON t.debit_account_code = a.account_code
GROUP BY DATE_TRUNC('month', transaction_date), account_type;
```

**Parallel Plan**:
```
Finalize HashAggregate
  └── Gather
      Workers Planned: 4
      └── Partial HashAggregate  -- Each worker aggregates subset
          └── Parallel SeqScan
```

## Window Functions vs Aggregations

Sometimes window functions are better than GROUP BY:

```sql-interactive
-- Using GROUP BY (requires self-join for running total)
WITH daily_totals AS (
    SELECT
        transaction_date,
        SUM(debit_amount) as daily_total
    FROM ledger_transactions
    WHERE debit_account_code = '1010'
    GROUP BY transaction_date
)
SELECT
    a.transaction_date,
    a.daily_total,
    SUM(b.daily_total) as running_total
FROM daily_totals a
JOIN daily_totals b ON b.transaction_date <= a.transaction_date
GROUP BY a.transaction_date, a.daily_total
ORDER BY a.transaction_date;

-- Using window function (more efficient)
SELECT
    transaction_date,
    SUM(debit_amount) as daily_total,
    SUM(SUM(debit_amount)) OVER (
        ORDER BY transaction_date
        ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
    ) as running_total
FROM ledger_transactions
WHERE debit_account_code = '1010'
GROUP BY transaction_date
ORDER BY transaction_date;
```

## Interactive Aggregation Analyzer

```aggregation-analyzer
{
  "query": "SELECT account_type, COUNT(*), SUM(amount) FROM ... GROUP BY account_type",
  "options": {
    "strategy": ["hash", "sort", "mixed"],
    "parallelism": [1, 2, 4, 8],
    "work_mem": ["4MB", "16MB", "64MB", "256MB"],
    "enable_hashagg": true,
    "enable_sort": true
  },
  "data_profile": {
    "total_rows": 50000,
    "distinct_groups": 5,
    "average_group_size": 10000,
    "data_distribution": "uniform"
  }
}
```

## Common Aggregation Patterns

### Financial Period Comparisons

```sql-interactive
-- Year-over-year comparison
WITH monthly_summary AS (
    SELECT
        DATE_TRUNC('month', transaction_date) as month,
        account_type,
        SUM(debit_amount) as total
    FROM ledger_transactions t
    JOIN chart_of_accounts a ON t.debit_account_code = a.account_code
    GROUP BY DATE_TRUNC('month', transaction_date), account_type
)
SELECT
    account_type,
    month,
    total as current_month,
    LAG(total, 12) OVER (
        PARTITION BY account_type
        ORDER BY month
    ) as same_month_last_year,
    total - LAG(total, 12) OVER (
        PARTITION BY account_type
        ORDER BY month
    ) as yoy_change
FROM monthly_summary
ORDER BY account_type, month;
```

### Top-N per Group

```sql-interactive
-- Top 3 expense accounts by month
WITH ranked_expenses AS (
    SELECT
        DATE_TRUNC('month', transaction_date) as month,
        debit_account_code,
        SUM(debit_amount) as total,
        ROW_NUMBER() OVER (
            PARTITION BY DATE_TRUNC('month', transaction_date)
            ORDER BY SUM(debit_amount) DESC
        ) as rank
    FROM ledger_transactions t
    JOIN chart_of_accounts a ON t.debit_account_code = a.account_code
    WHERE a.account_type = 'EXPENSE'
    GROUP BY DATE_TRUNC('month', transaction_date), debit_account_code
)
SELECT month, debit_account_code, total
FROM ranked_expenses
WHERE rank <= 3
ORDER BY month, rank;
```

## Optimization Exercises

### Exercise 1: Optimize Slow Aggregation

```sql-interactive
-- This query is slow. How would you optimize it?
SELECT
    a.account_name,
    EXTRACT(YEAR FROM t.transaction_date) as year,
    EXTRACT(MONTH FROM t.transaction_date) as month,
    COUNT(*) as transactions,
    SUM(
        CASE
            WHEN t.debit_account_code = a.account_code
            THEN t.debit_amount
            ELSE t.credit_amount
        END
    ) as total
FROM chart_of_accounts a
CROSS JOIN ledger_transactions t
WHERE (t.debit_account_code = a.account_code
    OR t.credit_account_code = a.account_code)
GROUP BY a.account_name,
         EXTRACT(YEAR FROM t.transaction_date),
         EXTRACT(MONTH FROM t.transaction_date)
HAVING COUNT(*) > 10;
```

### Exercise 2: Choose Aggregation Strategy

For each scenario, which aggregation strategy would RA choose?

1. GROUP BY user_id (1M distinct users, 10M rows)
2. GROUP BY transaction_date ORDER BY transaction_date (2 years of dates)
3. GROUP BY country_code (195 countries, 1M rows)
4. GROUP BY uuid_column (all unique, 100K rows)

### Exercise 3: Rewrite for Performance

```sql-interactive
-- Current version with subqueries
SELECT
    account_type,
    (SELECT COUNT(*)
     FROM ledger_transactions t
     WHERE t.debit_account_code IN (
         SELECT account_code
         FROM chart_of_accounts
         WHERE account_type = a.account_type
     )) as debit_count,
    (SELECT SUM(debit_amount)
     FROM ledger_transactions t
     WHERE t.debit_account_code IN (
         SELECT account_code
         FROM chart_of_accounts
         WHERE account_type = a.account_type
     )) as debit_total
FROM (SELECT DISTINCT account_type FROM chart_of_accounts) a;

-- Rewrite using JOIN and GROUP BY
-- Your solution here...
```

## Key Takeaways

1. **Choose the right aggregation strategy**
   - Hash for moderate groups
   - Sort when order needed
   - Index-aided when possible

2. **Push aggregations early**
   - Aggregate before joins
   - Use partial aggregation
   - Leverage materialized views

3. **Memory matters**
   - Hash aggregation needs memory
   - Spilling to disk is slow
   - Tune work_mem appropriately

4. **Parallel aggregation scales**
   - Linear speedup with workers
   - Best for large datasets
   - Partial aggregation per worker

5. **Window functions for complex analytics**
   - Running totals
   - Rankings
   - Period comparisons

## Next Steps

Aggregations summarize data, but window functions provide even more analytical power. In [Chapter 5: Window Functions](05-window-functions.md), we'll explore running totals, rankings, and advanced analytics.

---

*💡 Performance Tip: Pre-aggregate frequently used summaries in materialized views. Trading storage for speed is often worth it for dashboards and reports.*