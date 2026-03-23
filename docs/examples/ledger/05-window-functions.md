# Chapter 5: Window Functions and Advanced Analytics

## Beyond Simple Aggregations

Alice's business has grown. She needs sophisticated analytics: running balances, transaction rankings, moving averages, and year-over-year comparisons. Window functions enable these calculations without complex self-joins. Let's see how RA optimizes them.

## Window Function Fundamentals

Window functions operate on a "window" of rows:

```sql-interactive
-- Running balance for cash account
SELECT
    transaction_date,
    debit_amount,
    credit_amount,
    SUM(debit_amount - credit_amount) OVER (
        ORDER BY transaction_date, id
        ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
    ) as running_balance
FROM ledger_transactions
WHERE debit_account_code = '1010'
   OR credit_account_code = '1010'
ORDER BY transaction_date, id;
```

## Window Function Execution Model

### The Window Sort

```sql-interactive
-- RA must sort data for window processing
EXPLAIN SELECT
    transaction_date,
    debit_amount,
    ROW_NUMBER() OVER (ORDER BY transaction_date) as row_num,
    RANK() OVER (ORDER BY debit_amount DESC) as amount_rank
FROM ledger_transactions
WHERE debit_account_code = '5010';
```

**Execution Plan**:
```
WindowAgg
  `---- Sort (transaction_date)
      `---- WindowAgg
          `---- Sort (debit_amount DESC)
              `---- SeqScan
                  `---- Filter: debit_account_code = '5010'
```

Notice: Multiple sorts for different windows!

### Window Optimization: Shared Sorts

```sql-interactive
-- RA can share sorts when windows are compatible
SELECT
    transaction_date,
    debit_amount,
    -- These three windows share the same sort
    ROW_NUMBER() OVER w as row_num,
    SUM(debit_amount) OVER w as cumulative_sum,
    AVG(debit_amount) OVER w as cumulative_avg
FROM ledger_transactions
WHERE debit_account_code = '5010'
WINDOW w AS (ORDER BY transaction_date)
ORDER BY transaction_date;
```

**Optimized Plan**:
```
WindowAgg (all three functions)
  `---- Sort (transaction_date)  -- Single sort!
      `---- SeqScan
```

## Common Window Patterns

### Pattern 1: Running Totals

```sql-interactive
-- Daily cash position
WITH daily_cash AS (
    SELECT
        transaction_date,
        SUM(CASE
            WHEN debit_account_code = '1010' THEN debit_amount
            WHEN credit_account_code = '1010' THEN -credit_amount
            ELSE 0
        END) as daily_change
    FROM ledger_transactions
    GROUP BY transaction_date
)
SELECT
    transaction_date,
    daily_change,
    SUM(daily_change) OVER (
        ORDER BY transaction_date
        ROWS UNBOUNDED PRECEDING
    ) as cash_balance
FROM daily_cash
ORDER BY transaction_date;
```

### Pattern 2: Moving Averages

```sql-interactive
-- 7-day moving average of sales
WITH daily_sales AS (
    SELECT
        transaction_date,
        SUM(credit_amount) as daily_total
    FROM ledger_transactions
    WHERE credit_account_code = '4010'  -- Sales account
    GROUP BY transaction_date
)
SELECT
    transaction_date,
    daily_total,
    AVG(daily_total) OVER (
        ORDER BY transaction_date
        ROWS BETWEEN 6 PRECEDING AND CURRENT ROW
    ) as moving_avg_7day,
    AVG(daily_total) OVER (
        ORDER BY transaction_date
        ROWS BETWEEN 29 PRECEDING AND CURRENT ROW
    ) as moving_avg_30day
FROM daily_sales
ORDER BY transaction_date;
```

### Pattern 3: Rankings and Percentiles

```sql-interactive
-- Rank transactions by size within each account
SELECT
    debit_account_code,
    transaction_date,
    debit_amount,
    ROW_NUMBER() OVER w as transaction_number,
    RANK() OVER w as amount_rank,
    DENSE_RANK() OVER w as amount_dense_rank,
    PERCENT_RANK() OVER w as amount_percentile,
    NTILE(4) OVER w as amount_quartile
FROM ledger_transactions
WHERE debit_amount > 0
WINDOW w AS (
    PARTITION BY debit_account_code
    ORDER BY debit_amount DESC
)
ORDER BY debit_account_code, amount_rank;
```

### Pattern 4: Lead and Lag

```sql-interactive
-- Compare each transaction to previous
SELECT
    transaction_date,
    debit_account_code,
    debit_amount,
    LAG(debit_amount, 1) OVER w as prev_amount,
    debit_amount - LAG(debit_amount, 1) OVER w as change_from_prev,
    LEAD(debit_amount, 1) OVER w as next_amount,
    FIRST_VALUE(debit_amount) OVER w as first_in_month,
    LAST_VALUE(debit_amount) OVER w as last_in_month
FROM ledger_transactions
WHERE debit_account_code = '5010'
WINDOW w AS (
    PARTITION BY DATE_TRUNC('month', transaction_date)
    ORDER BY transaction_date
    ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING
)
ORDER BY transaction_date;
```

## Frame Specifications

Understanding frames is crucial for optimization:

```sql-interactive
-- Different frame types have different costs
SELECT
    transaction_date,
    debit_amount,

    -- ROWS: Physical row count (fast)
    SUM(debit_amount) OVER (
        ORDER BY transaction_date
        ROWS BETWEEN 10 PRECEDING AND CURRENT ROW
    ) as sum_last_11_rows,

    -- RANGE: Logical range (slower, needs comparison)
    SUM(debit_amount) OVER (
        ORDER BY transaction_date
        RANGE BETWEEN INTERVAL '7 days' PRECEDING AND CURRENT ROW
    ) as sum_last_7_days,

    -- GROUPS: Peer groups (PostgreSQL 11+)
    SUM(debit_amount) OVER (
        ORDER BY DATE_TRUNC('week', transaction_date)
        GROUPS BETWEEN 1 PRECEDING AND CURRENT ROW
    ) as sum_last_2_weeks

FROM ledger_transactions
WHERE debit_account_code = '1010'
ORDER BY transaction_date;
```

**Performance Comparison**:
- ROWS: O(1) frame boundary calculation
- RANGE: O(log n) binary search for boundaries
- GROUPS: O(n) scan for group boundaries

## Optimization Strategies

### Strategy 1: Partition Pruning

```sql-interactive
-- Partition by account for parallel processing
SELECT
    debit_account_code,
    transaction_date,
    debit_amount,
    SUM(debit_amount) OVER (
        PARTITION BY debit_account_code
        ORDER BY transaction_date
    ) as account_running_total
FROM ledger_transactions
WHERE debit_account_code IN ('1010', '1020', '5010')
ORDER BY debit_account_code, transaction_date;
```

RA can process each partition independently!

### Strategy 2: Index-Aided Windows

```sql-interactive
-- Index provides pre-sorted data
CREATE INDEX idx_account_date
ON ledger_transactions(debit_account_code, transaction_date);

-- No sort needed!
SELECT
    transaction_date,
    debit_amount,
    ROW_NUMBER() OVER (ORDER BY transaction_date) as rn
FROM ledger_transactions
WHERE debit_account_code = '1010'
ORDER BY transaction_date;
```

### Strategy 3: Materialized Window Results

```sql-interactive
-- Pre-compute common window calculations
CREATE MATERIALIZED VIEW account_running_balances AS
SELECT
    account_code,
    transaction_date,
    SUM(amount) OVER (
        PARTITION BY account_code
        ORDER BY transaction_date
        ROWS UNBOUNDED PRECEDING
    ) as running_balance,
    AVG(amount) OVER (
        PARTITION BY account_code
        ORDER BY transaction_date
        ROWS BETWEEN 29 PRECEDING AND CURRENT ROW
    ) as moving_avg_30day
FROM (
    SELECT debit_account_code as account_code,
           transaction_date,
           debit_amount as amount
    FROM ledger_transactions
    UNION ALL
    SELECT credit_account_code as account_code,
           transaction_date,
           -credit_amount as amount
    FROM ledger_transactions
) combined;

CREATE INDEX idx_mrb_lookup
ON account_running_balances(account_code, transaction_date);
```

## Complex Analytics Examples

### Year-Over-Year Growth

```sql-interactive
-- YoY growth by account type
WITH monthly_totals AS (
    SELECT
        DATE_TRUNC('month', t.transaction_date) as month,
        a.account_type,
        SUM(t.debit_amount) as total
    FROM ledger_transactions t
    JOIN chart_of_accounts a ON t.debit_account_code = a.account_code
    GROUP BY DATE_TRUNC('month', t.transaction_date), a.account_type
)
SELECT
    month,
    account_type,
    total as current_month,
    LAG(total, 12) OVER (
        PARTITION BY account_type
        ORDER BY month
    ) as same_month_last_year,
    ROUND(
        100.0 * (total - LAG(total, 12) OVER (
            PARTITION BY account_type
            ORDER BY month
        )) / NULLIF(LAG(total, 12) OVER (
            PARTITION BY account_type
            ORDER BY month
        ), 0),
        2
    ) as yoy_growth_percent
FROM monthly_totals
WHERE month >= '2023-01-01'
ORDER BY account_type, month;
```

### Cohort Analysis

```sql-interactive
-- Customer cohort retention (if customer data existed)
WITH cohorts AS (
    SELECT
        DATE_TRUNC('month', first_transaction) as cohort_month,
        customer_id,
        DATE_TRUNC('month', transaction_date) as transaction_month,
        ROW_NUMBER() OVER (
            PARTITION BY customer_id, DATE_TRUNC('month', transaction_date)
            ORDER BY transaction_date
        ) as month_transaction_num
    FROM (
        SELECT
            journal_entry_id as customer_id,  -- Pretend this is customer
            transaction_date,
            FIRST_VALUE(transaction_date) OVER (
                PARTITION BY journal_entry_id
                ORDER BY transaction_date
            ) as first_transaction
        FROM ledger_transactions
    ) t
)
SELECT
    cohort_month,
    COUNT(DISTINCT CASE
        WHEN transaction_month = cohort_month
        THEN customer_id
    END) as month_0,
    COUNT(DISTINCT CASE
        WHEN transaction_month = cohort_month + INTERVAL '1 month'
        THEN customer_id
    END) as month_1,
    COUNT(DISTINCT CASE
        WHEN transaction_month = cohort_month + INTERVAL '2 months'
        THEN customer_id
    END) as month_2
FROM cohorts
GROUP BY cohort_month
ORDER BY cohort_month;
```

## Interactive Window Function Explorer

```window-explorer
{
  "sample_query": "SELECT *, SUM(amount) OVER (...) FROM transactions",
  "window_options": {
    "partition_by": ["account", "date", "type", "none"],
    "order_by": ["date", "amount", "account"],
    "frame_type": ["ROWS", "RANGE", "GROUPS"],
    "frame_start": ["UNBOUNDED PRECEDING", "n PRECEDING", "CURRENT ROW"],
    "frame_end": ["CURRENT ROW", "n FOLLOWING", "UNBOUNDED FOLLOWING"]
  },
  "functions": [
    "ROW_NUMBER()",
    "RANK()",
    "DENSE_RANK()",
    "SUM(amount)",
    "AVG(amount)",
    "MIN(amount)",
    "MAX(amount)",
    "FIRST_VALUE(amount)",
    "LAST_VALUE(amount)",
    "LAG(amount, 1)",
    "LEAD(amount, 1)"
  ]
}
```

## Performance Comparison

### Window Function vs Self-Join

```sql-interactive
-- Method 1: Window Function (Efficient)
SELECT
    transaction_date,
    debit_amount,
    SUM(debit_amount) OVER (
        ORDER BY transaction_date
        ROWS UNBOUNDED PRECEDING
    ) as running_total
FROM ledger_transactions
WHERE debit_account_code = '1010';

-- Method 2: Self-Join (Inefficient)
SELECT
    a.transaction_date,
    a.debit_amount,
    SUM(b.debit_amount) as running_total
FROM ledger_transactions a
JOIN ledger_transactions b
  ON b.transaction_date <= a.transaction_date
  AND b.debit_account_code = '1010'
WHERE a.debit_account_code = '1010'
GROUP BY a.transaction_date, a.debit_amount
ORDER BY a.transaction_date;
```

**Cost Analysis**:
- Window: O(n log n) for sort + O(n) for window
- Self-Join: O(n$^2$) for join + O(n log n) for group

## Common Pitfalls

### Pitfall 1: Unnecessary Sorts

```sql-interactive
-- Bad: Multiple incompatible windows
SELECT
    ROW_NUMBER() OVER (ORDER BY transaction_date) as date_order,
    ROW_NUMBER() OVER (ORDER BY amount) as amount_order,
    ROW_NUMBER() OVER (ORDER BY account) as account_order
FROM ledger_transactions;
-- Requires 3 separate sorts!

-- Better: Use one primary sort
SELECT
    ROW_NUMBER() OVER (ORDER BY transaction_date) as date_order,
    DENSE_RANK() OVER (ORDER BY transaction_date, amount) as amount_rank,
    DENSE_RANK() OVER (ORDER BY transaction_date, account) as account_rank
FROM ledger_transactions;
-- Can share the transaction_date sort
```

### Pitfall 2: Wrong Frame Clause

```sql-interactive
-- Incorrect: Default frame excludes peers
SELECT
    amount,
    LAST_VALUE(amount) OVER (ORDER BY date) as last_amount
FROM transactions;
-- Default frame: RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW

-- Correct: Specify full frame
SELECT
    amount,
    LAST_VALUE(amount) OVER (
        ORDER BY date
        ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING
    ) as last_amount
FROM transactions;
```

## Practice Exercises

### Exercise 1: Optimize Multiple Windows

```sql-interactive
-- This query has redundant sorting. Optimize it:
SELECT
    transaction_date,
    debit_account_code,
    debit_amount,
    RANK() OVER (
        PARTITION BY debit_account_code
        ORDER BY debit_amount DESC
    ) as amount_rank,
    SUM(debit_amount) OVER (
        PARTITION BY debit_account_code
        ORDER BY transaction_date
    ) as running_total,
    AVG(debit_amount) OVER (
        PARTITION BY debit_account_code
        ORDER BY transaction_date
        ROWS BETWEEN 10 PRECEDING AND CURRENT ROW
    ) as moving_avg
FROM ledger_transactions;

-- Optimize by reordering/combining windows
-- Your solution here...
```

### Exercise 2: Convert Join to Window

```sql-interactive
-- Rewrite this self-join using window functions:
SELECT
    a.account_code,
    a.transaction_date,
    a.amount,
    a.amount - COALESCE(b.amount, 0) as change_from_previous
FROM transactions a
LEFT JOIN transactions b
  ON a.account_code = b.account_code
  AND b.transaction_date = (
      SELECT MAX(transaction_date)
      FROM transactions c
      WHERE c.account_code = a.account_code
        AND c.transaction_date < a.transaction_date
  )
ORDER BY a.account_code, a.transaction_date;

-- Your window function solution here...
```

## Key Takeaways

1. **Window functions avoid expensive self-joins**
   - O(n log n) vs O(n$^2$) complexity
   - Single table scan vs multiple

2. **Frame specifications matter**
   - ROWS is fastest
   - RANGE requires value comparison
   - Default frames can be surprising

3. **Compatible windows share sorts**
   - Same PARTITION BY and ORDER BY
   - RA optimizes sort sharing

4. **Indexes can eliminate sorts**
   - Pre-sorted data from indexes
   - Huge performance gain

5. **Materialized views for complex windows**
   - Pre-compute expensive calculations
   - Trade storage for speed

## Next Steps

We've seen individual optimizations. Now let's watch RA optimize a complex query step-by-step. In [Chapter 6: Optimization Journey](06-optimization-journey.md), we'll trace the complete transformation from naive SQL to optimized execution plan.

---

* Pro Tip: When multiple window functions need different sorts, consider if you can restructure to share sorting costs. Even partial sort sharing significantly improves performance.*