# Chapter 3: Basic Query Optimization

## From Simple to Sophisticated

Alice starts each morning with simple questions: What's in the cash register? How much did we sell yesterday? These basic queries are where optimization begins. Let's see how RA transforms simple SQL into efficient execution plans.

## The Anatomy of a Query Plan

Before we optimize, let's understand what we're looking at:

```sql-interactive
-- A simple query
EXPLAIN SELECT account_name, balance
FROM account_balances
WHERE account_code = '1010';
```

### Plan Components

```
Scan (account_balances)
  |---- Filter: account_code = '1010'
  |---- Cost: 10.5 units
  |---- Rows: 1 (estimated)
  `---- Width: 48 bytes
```

Each node shows:
- **Operation**: What it does (Scan, Filter, Join, etc.)
- **Cost**: Estimated resource usage
- **Rows**: Expected output size
- **Width**: Bytes per row

## Progressive Optimization Examples

### Level 1: Table Scan

Alice's first query - checking cash balance:

```sql-interactive
-- Without any indexes
SELECT
    account_name,
    SUM(debit_amount - credit_amount) as balance
FROM ledger_transactions
WHERE debit_account_code = '1010'
   OR credit_account_code = '1010'
GROUP BY account_name;
```

**RA's Initial Plan**:
```
Aggregate
  `---- Filter (OR condition)
      `---- SeqScan (ledger_transactions)
```

**Cost Breakdown**:
- SeqScan: 1000 units (read all 50,000 rows)
- Filter: 100 units (check each row)
- Aggregate: 10 units (sum matching rows)
- **Total: 1110 units**

### Level 2: Index Scan

Add an index and watch the transformation:

```sql-interactive
-- With index on debit_account_code
CREATE INDEX idx_debit_account ON ledger_transactions(debit_account_code);

-- Same query, different plan
SELECT
    account_name,
    SUM(debit_amount - credit_amount) as balance
FROM ledger_transactions
WHERE debit_account_code = '1010'
   OR credit_account_code = '1010'
GROUP BY account_name;
```

**RA's Optimized Plan**:
```
Aggregate
  `---- BitmapOr
      |---- BitmapIndexScan (idx_debit_account)
      `---- BitmapIndexScan (idx_credit_account)
```

**Cost Breakdown**:
- BitmapIndexScan: 20 units (each)
- BitmapOr: 5 units
- Aggregate: 10 units
- **Total: 55 units** (20X improvement!)

### Level 3: Covering Index

The ultimate optimization - read no table data:

```sql-interactive
-- Covering index includes all needed columns
CREATE INDEX idx_debit_covering ON ledger_transactions
(debit_account_code) INCLUDE (account_name, debit_amount, credit_amount);

-- Now RA can skip the table entirely
SELECT
    account_name,
    SUM(debit_amount - credit_amount) as balance
FROM ledger_transactions
WHERE debit_account_code = '1010'
GROUP BY account_name;
```

**RA's Best Plan**:
```
Aggregate
  `---- IndexOnlyScan (idx_debit_covering)
```

**Cost Breakdown**:
- IndexOnlyScan: 15 units
- Aggregate: 10 units
- **Total: 25 units** (44X improvement from original!)

## Join Optimization

Alice needs to see account names with transactions:

### The Naive Join

```sql-interactive
-- Straightforward but slow
SELECT
    a.account_name,
    t.transaction_date,
    t.debit_amount
FROM chart_of_accounts a
JOIN ledger_transactions t ON a.account_code = t.debit_account_code
WHERE t.transaction_date = CURRENT_DATE;
```

**Initial Plan**:
```
NestedLoopJoin
  |---- SeqScan (chart_of_accounts)
  `---- SeqScan (ledger_transactions)
      `---- Filter: transaction_date = CURRENT_DATE
```

### Join Order Matters

RA tries different join orders:

```sql-interactive
-- RA considers both options:
-- Option 1: Accounts -> Transactions
-- Option 2: Transactions -> Accounts

-- Let's hint at the better order
SELECT /*+ LEADING(t a) */
    a.account_name,
    t.transaction_date,
    t.debit_amount
FROM ledger_transactions t
JOIN chart_of_accounts a ON a.account_code = t.debit_account_code
WHERE t.transaction_date = CURRENT_DATE;
```

**Optimized Plan**:
```
HashJoin
  |---- Filter: transaction_date = CURRENT_DATE
  |   `---- IndexScan (ledger_transactions)
  `---- Hash
      `---- SeqScan (chart_of_accounts)
```

Why is this better?
1. Filter reduces transactions from 50,000 to ~100
2. Small hash table (150 accounts)
3. Single pass through filtered transactions

## Filter Pushdown

RA pushes filters as close to the data as possible:

```sql-interactive
-- Before optimization
SELECT *
FROM (
    SELECT
        account_code,
        account_name,
        SUM(debit_amount) as total
    FROM ledger_transactions
    JOIN chart_of_accounts USING (account_code)
    GROUP BY account_code, account_name
) summary
WHERE total > 1000;
```

**Before Pushdown**:
```
Filter: total > 1000
  `---- Aggregate
      `---- Join
          |---- Scan (ledger_transactions)
          `---- Scan (chart_of_accounts)
```

**After Pushdown**:
```
Aggregate
  `---- Join
      |---- Scan (ledger_transactions)
      |   `---- Filter: debit_amount > 0  -- Partial pushdown
      `---- Scan (chart_of_accounts)
Having: SUM(debit_amount) > 1000
```

## Predicate Simplification

RA simplifies complex conditions:

```sql-interactive
-- Complex WHERE clause
SELECT *
FROM ledger_transactions
WHERE (debit_account_code = '1010' AND transaction_date = '2024-01-15')
   OR (debit_account_code = '1010' AND transaction_date = '2024-01-16')
   OR (debit_account_code = '1010' AND transaction_date = '2024-01-17');

-- RA rewrites to:
SELECT *
FROM ledger_transactions
WHERE debit_account_code = '1010'
  AND transaction_date IN ('2024-01-15', '2024-01-16', '2024-01-17');
```

## Subquery Optimization

### Correlated Subquery

```sql-interactive
-- Inefficient correlated subquery
SELECT
    a.account_name,
    (SELECT SUM(debit_amount)
     FROM ledger_transactions t
     WHERE t.debit_account_code = a.account_code) as total_debits
FROM chart_of_accounts a
WHERE a.account_type = 'ASSET';
```

### RA's Decorrelation

```sql-interactive
-- RA transforms to efficient join
SELECT
    a.account_name,
    COALESCE(t.total_debits, 0) as total_debits
FROM chart_of_accounts a
LEFT JOIN (
    SELECT debit_account_code, SUM(debit_amount) as total_debits
    FROM ledger_transactions
    GROUP BY debit_account_code
) t ON t.debit_account_code = a.account_code
WHERE a.account_type = 'ASSET';
```

## Interactive Query Tuner

```query-tuner
{
  "query": "SELECT * FROM ledger_transactions WHERE debit_account_code = ?",
  "parameters": {
    "account_code": "1010",
    "date_range": "1 day",
    "limit": 100
  },
  "available_indexes": [
    "btree(debit_account_code)",
    "btree(transaction_date)",
    "btree(debit_account_code, transaction_date)",
    "hash(journal_entry_id)"
  ],
  "statistics": {
    "table_rows": 50000,
    "account_selectivity": 0.02,
    "date_selectivity": 0.003
  }
}
```

Adjust parameters and see how RA chooses different plans!

## Common Patterns and Anti-Patterns

### [x] Good: Sargable Predicates

```sql-interactive
-- Searchable argument - can use index
SELECT * FROM ledger_transactions
WHERE transaction_date = '2024-01-15';
```

### [FAIL] Bad: Non-Sargable Predicates

```sql-interactive
-- Function on column prevents index use
SELECT * FROM ledger_transactions
WHERE DATE_TRUNC('month', transaction_date) = '2024-01-01';

-- RA cannot optimize this effectively
```

### [x] Better: Rewrite for Sargability

```sql-interactive
-- Range condition allows index use
SELECT * FROM ledger_transactions
WHERE transaction_date >= '2024-01-01'
  AND transaction_date < '2024-02-01';
```

## Cost Model Deep Dive

Let's understand how RA calculates costs:

```cost-model
// Cost calculation for different operations

const costs = {
  seqScan: (rows, width) => {
    const pageCost = 1.0;
    const cpuTupleCost = 0.01;
    const pages = Math.ceil((rows * width) / 8192);
    return (pages * pageCost) + (rows * cpuTupleCost);
  },

  indexScan: (rows, selectivity) => {
    const indexPageCost = 0.5;
    const randomPageCost = 4.0;
    const cpuIndexCost = 0.005;
    const matchingRows = rows * selectivity;
    return (indexPageCost * Math.log2(rows)) +
           (randomPageCost * matchingRows) +
           (cpuIndexCost * matchingRows);
  },

  hashJoin: (outerRows, innerRows) => {
    const hashCost = 0.001;
    const probeCost = 0.002;
    return (innerRows * hashCost) + (outerRows * probeCost);
  }
};

// Compare plans for your query
```

## Rule Application Trace

See which optimization rules fire:

```sql-interactive
-- Enable rule tracing
SET ra.trace_rules = true;

SELECT
    a.account_name,
    SUM(t.debit_amount) as total
FROM chart_of_accounts a
JOIN ledger_transactions t ON a.account_code = t.debit_account_code
WHERE a.account_type = 'EXPENSE'
  AND t.transaction_date >= '2024-01-01'
GROUP BY a.account_name;
```

**Rules Applied**:
1. `PushFilterBeforeJoin`: Move date filter to transactions
2. `UseIndexForEquality`: Use account_code index
3. `ChooseJoinAlgorithm`: HashJoin for small$\times$large
4. `PushdownProjection`: Only read needed columns

## Practice Exercises

### Exercise 1: Optimize the Slow Query

```sql-interactive
-- This query is slow. Can you fix it?
SELECT DISTINCT
    a.account_name,
    a.account_type
FROM chart_of_accounts a
WHERE EXISTS (
    SELECT 1
    FROM ledger_transactions t
    WHERE t.debit_account_code = a.account_code
      AND t.transaction_date >= CURRENT_DATE - 30
);
```

*Hint: Think about the EXISTS subquery...*

### Exercise 2: Choose the Right Index

For this query workload, which index would help most?

```sql-interactive
-- Query 1 (60% of workload)
SELECT * FROM ledger_transactions
WHERE journal_entry_id = ?;

-- Query 2 (30% of workload)
SELECT * FROM ledger_transactions
WHERE transaction_date = ?
  AND debit_account_code = ?;

-- Query 3 (10% of workload)
SELECT * FROM ledger_transactions
WHERE credit_amount > 10000;
```

Options:
1. `CREATE INDEX ON ledger_transactions(journal_entry_id)`
2. `CREATE INDEX ON ledger_transactions(transaction_date, debit_account_code)`
3. `CREATE INDEX ON ledger_transactions(credit_amount)`
4. Both 1 and 2

### Exercise 3: Rewrite for Performance

```sql-interactive
-- Current slow version
SELECT
    account_code,
    account_name,
    (SELECT COUNT(*)
     FROM ledger_transactions
     WHERE debit_account_code = account_code) +
    (SELECT COUNT(*)
     FROM ledger_transactions
     WHERE credit_account_code = account_code) as transaction_count
FROM chart_of_accounts;

-- Rewrite this query for better performance
-- Your solution here...
```

## Key Takeaways

1. **Index selection dramatically impacts performance**
   - Right index can be 10-100x improvement
   - Wrong index might not help at all

2. **Join order matters**
   - Start with most selective table
   - Build smallest intermediate results

3. **Filter early, project late**
   - Reduce rows as soon as possible
   - But keep columns until needed

4. **Subqueries often need rewriting**
   - Correlated -> Join
   - EXISTS -> Semi-join
   - IN -> Hash lookup

5. **Cost models are estimates**
   - Based on statistics
   - Can be wrong with skewed data

## Next Steps

Basic queries are the foundation, but Alice's business needs summaries and reports. In [Chapter 4: Aggregations](04-aggregations.md), we'll explore how RA optimizes GROUP BY, HAVING, and aggregate functions.

---

* Pro Tip: Watch the cost numbers! A 10x cost difference usually means 10x performance difference in practice.*