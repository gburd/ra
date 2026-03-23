# Chapter 7: How Statistics Shape Optimization

## The Power of Accurate Statistics

RA's decisions depend on statistics: row counts, cardinalities, distributions. When statistics are wrong, even the best optimizer makes poor choices. Let's explore how statistics influence query plans and how to diagnose statistics-related performance problems.

## Understanding Database Statistics

### What RA Tracks

```sql-interactive
-- View current statistics
SELECT
    schemaname,
    tablename,
    attname as column_name,
    n_distinct as distinct_values,
    null_frac as null_fraction,
    avg_width as avg_bytes,
    correlation as physical_correlation
FROM pg_stats
WHERE tablename IN ('ledger_transactions', 'chart_of_accounts')
ORDER BY tablename, attname;
```

Key statistics:
- **n_distinct**: Number of unique values (-1 means unique)
- **null_frac**: Fraction of NULL values
- **histogram_bounds**: Value distribution
- **most_common_vals**: Frequent values and their frequencies
- **correlation**: Physical vs logical order (-1 to 1)

## Interactive Statistics Editor

```statistics-editor
{
  "ledger_transactions": {
    "row_count": 50000,
    "columns": {
      "transaction_date": {
        "distinct": 730,
        "min": "2023-01-01",
        "max": "2024-12-31",
        "null_fraction": 0.0,
        "correlation": 0.95
      },
      "debit_account_code": {
        "distinct": 120,
        "null_fraction": 0.0,
        "most_common": [
          {"value": "5010", "frequency": 0.15},
          {"value": "1010", "frequency": 0.12},
          {"value": "4010", "frequency": 0.10}
        ]
      },
      "debit_amount": {
        "distinct": -1,
        "min": 0.01,
        "max": 999999.99,
        "avg": 542.31,
        "stddev": 2341.55
      }
    }
  },
  "chart_of_accounts": {
    "row_count": 150,
    "columns": {
      "account_type": {
        "distinct": 5,
        "histogram": {
          "ASSET": 30,
          "LIABILITY": 25,
          "EQUITY": 15,
          "REVENUE": 35,
          "EXPENSE": 45
        }
      }
    }
  }
}
```

Modify these values and watch how RA's plan changes!

## Scenario 1: Row Count Impact

### Small Table Assumption

```sql-interactive
-- When RA thinks ledger_transactions has 100 rows
SET ra.statistics.ledger_transactions.row_count = 100;

EXPLAIN SELECT
    a.account_name,
    t.debit_amount
FROM chart_of_accounts a
JOIN ledger_transactions t ON a.account_code = t.debit_account_code
WHERE t.transaction_date = '2024-01-15';
```

**Plan with 100 rows**:
```
NestedLoopJoin  -- Good for small tables
  |---- SeqScan (ledger_transactions)
  `---- IndexSeek (chart_of_accounts)
```

### Large Table Reality

```sql-interactive
-- When RA knows the real count: 50,000 rows
SET ra.statistics.ledger_transactions.row_count = 50000;

EXPLAIN SELECT
    a.account_name,
    t.debit_amount
FROM chart_of_accounts a
JOIN ledger_transactions t ON a.account_code = t.debit_account_code
WHERE t.transaction_date = '2024-01-15';
```

**Plan with 50,000 rows**:
```
HashJoin  -- Better for large tables
  |---- IndexScan (ledger_transactions)
  `---- Hash
      `---- SeqScan (chart_of_accounts)
```

## Scenario 2: Cardinality Impact

### Low Cardinality

```sql-interactive
-- Account codes have low cardinality (5 distinct values)
SET ra.statistics.ledger_transactions.debit_account_code.distinct = 5;

EXPLAIN SELECT *
FROM ledger_transactions
WHERE debit_account_code = '1010';
```

**Plan with low cardinality**:
```
SeqScan  -- Index not worth it for 20% of table
  `---- Filter: debit_account_code = '1010'
Expected rows: 10,000 (20% selectivity)
```

### High Cardinality

```sql-interactive
-- Account codes have high cardinality (5000 distinct values)
SET ra.statistics.ledger_transactions.debit_account_code.distinct = 5000;

EXPLAIN SELECT *
FROM ledger_transactions
WHERE debit_account_code = '1010';
```

**Plan with high cardinality**:
```
IndexScan  -- Index very selective
  `---- Index: idx_debit_account
Expected rows: 10 (0.02% selectivity)
```

## Scenario 3: Correlation Impact

Correlation measures how well logical order matches physical order:

### High Correlation (Ordered Data)

```sql-interactive
-- Transaction dates are inserted in order (correlation = 0.95)
SET ra.statistics.ledger_transactions.transaction_date.correlation = 0.95;

EXPLAIN SELECT *
FROM ledger_transactions
WHERE transaction_date BETWEEN '2024-01-01' AND '2024-01-31'
ORDER BY transaction_date;
```

**Plan with high correlation**:
```
IndexScan  -- Sequential I/O, fast!
  `---- Index: idx_transaction_date
Cost: 150 (low because of sequential reads)
```

### Low Correlation (Random Data)

```sql-interactive
-- Transaction dates are randomly distributed (correlation = 0.1)
SET ra.statistics.ledger_transactions.transaction_date.correlation = 0.1;

EXPLAIN SELECT *
FROM ledger_transactions
WHERE transaction_date BETWEEN '2024-01-01' AND '2024-01-31'
ORDER BY transaction_date;
```

**Plan with low correlation**:
```
Sort  -- Index would cause random I/O
  `---- SeqScan
      `---- Filter: date BETWEEN ...
Cost: 500 (high because must sort)
```

## Scenario 4: Histogram Impact

Histograms help with range queries:

### Uniform Distribution

```sql-interactive
-- Amounts uniformly distributed
SET ra.statistics.ledger_transactions.debit_amount.histogram = 'uniform';

EXPLAIN SELECT *
FROM ledger_transactions
WHERE debit_amount BETWEEN 100 AND 200;
```

**Estimate**: 10% of rows (linear interpolation)

### Skewed Distribution

```sql-interactive
-- Most amounts are small, few are large
SET ra.statistics.ledger_transactions.debit_amount.histogram = [
  1, 5, 10, 20, 50, 100, 500, 1000, 10000, 100000
];

EXPLAIN SELECT *
FROM ledger_transactions
WHERE debit_amount BETWEEN 100 AND 200;
```

**Estimate**: 2% of rows (histogram bucket calculation)

## Real-World Statistics Problems

### Problem 1: Stale Statistics

```sql-interactive
-- Statistics say 1,000 rows, reality is 100,000
-- Last ANALYZE: 6 months ago

SELECT
    account_type,
    COUNT(*) as cnt
FROM ledger_transactions t
JOIN chart_of_accounts a ON t.debit_account_code = a.account_code
GROUP BY account_type;

-- RA chooses: NestedLoop (good for 1K rows)
-- Should choose: HashJoin (good for 100K rows)
-- Performance: 50x slower than optimal
```

**Solution**:
```sql
ANALYZE ledger_transactions;
ANALYZE chart_of_accounts;
```

### Problem 2: Correlated Columns

```sql-interactive
-- RA doesn't know that account_type determines normal_balance
SELECT *
FROM chart_of_accounts
WHERE account_type = 'ASSET'
  AND normal_balance = 'CREDIT';  -- This combination never exists!

-- RA estimates: 30 * 0.5 = 15 rows
-- Reality: 0 rows
```

**Solution**: Multi-column statistics
```sql
CREATE STATISTICS account_correlation
ON account_type, normal_balance
FROM chart_of_accounts;
```

### Problem 3: Parameter Sniffing

```sql-interactive
-- Plan cached for account '1010' (very common)
PREPARE account_query AS
SELECT * FROM ledger_transactions
WHERE debit_account_code = $1;

-- Execute with rare account
EXECUTE account_query('9999');
-- Uses SeqScan optimized for common value!
```

## Statistics Diagnostic Queries

### Check Statistics Freshness

```sql-interactive
SELECT
    schemaname,
    tablename,
    last_analyze,
    last_autoanalyze,
    analyze_count,
    n_live_tup as row_count,
    n_dead_tup as dead_rows
FROM pg_stat_user_tables
WHERE schemaname = 'public'
ORDER BY last_analyze NULLS FIRST;
```

### Find Selectivity Problems

```sql-interactive
-- Compare estimated vs actual rows
EXPLAIN (ANALYZE, BUFFERS) SELECT *
FROM ledger_transactions
WHERE debit_account_code = '1010';

-- Look for:
-- Planned rows: 100
-- Actual rows: 5000  <-- Big discrepancy!
```

### Identify Missing Indexes

```sql-interactive
-- Queries doing sequential scans
SELECT
    schemaname,
    tablename,
    seq_scan,
    seq_tup_read,
    idx_scan,
    idx_tup_fetch,
    CASE
        WHEN seq_scan > 0
        THEN ROUND(100.0 * idx_scan / (seq_scan + idx_scan), 2)
        ELSE 100
    END as index_use_percent
FROM pg_stat_user_tables
WHERE seq_scan > 1000
ORDER BY seq_tup_read DESC;
```

## Interactive Statistics Experiment

```statistics-lab
// Experiment with different statistics scenarios

const scenarios = [
  {
    name: "Black Friday",
    description: "Sudden 100x increase in transactions",
    changes: {
      "ledger_transactions.row_count": "multiply by 100",
      "transaction_date.distinct": "set to 1",
      "debit_amount.avg": "multiply by 5"
    }
  },
  {
    name: "After Archival",
    description: "Old data moved to archive",
    changes: {
      "ledger_transactions.row_count": "divide by 10",
      "transaction_date.min": "set to 30 days ago",
      "account_code.distinct": "divide by 2"
    }
  },
  {
    name: "New Account System",
    description: "Migrated to fine-grained accounts",
    changes: {
      "chart_of_accounts.row_count": "multiply by 10",
      "account_code.distinct": "multiply by 10",
      "account_type.histogram": "redistribute"
    }
  }
];

// Apply scenario and see plan changes
```

## Cost Model Calibration

RA's cost model uses statistics to estimate:

```javascript
// Selectivity calculation
function estimateSelectivity(column, operator, value) {
  const stats = getStatistics(column);

  switch(operator) {
    case '=':
      if (stats.most_common_vals.includes(value)) {
        return stats.most_common_freqs[value];
      }
      return 1.0 / stats.n_distinct;

    case 'BETWEEN':
      if (stats.histogram) {
        return histogramSelectivity(stats.histogram, value);
      }
      return 0.1; // Default 10%

    case 'IN':
      return Math.min(
        value.length / stats.n_distinct,
        1.0
      );
  }
}

// Join cardinality estimation
function estimateJoinRows(left, right, join_column) {
  const leftRows = left.row_count;
  const rightRows = right.row_count;
  const leftDistinct = left.stats[join_column].n_distinct;
  const rightDistinct = right.stats[join_column].n_distinct;

  // Assumes uniform distribution
  return (leftRows * rightRows) / Math.max(leftDistinct, rightDistinct);
}
```

## Forcing Statistics Updates

### Manual ANALYZE

```sql-interactive
-- Update all statistics
ANALYZE;

-- Update specific table
ANALYZE ledger_transactions;

-- Update specific columns
ANALYZE ledger_transactions (transaction_date, debit_account_code);

-- Verbose output
ANALYZE VERBOSE ledger_transactions;
```

### Auto-Analyze Configuration

```sql
-- Check auto-analyze settings
SHOW autovacuum_analyze_threshold;  -- Default: 50
SHOW autovacuum_analyze_scale_factor;  -- Default: 0.1

-- More aggressive auto-analyze for volatile tables
ALTER TABLE ledger_transactions
SET (autovacuum_analyze_scale_factor = 0.01);
```

### Statistics Target

```sql-interactive
-- Increase statistics precision (default 100)
ALTER TABLE ledger_transactions
ALTER COLUMN debit_amount
SET STATISTICS 1000;  -- More histogram buckets

-- Check current target
SELECT attname, attstattarget
FROM pg_attribute
WHERE attrelid = 'ledger_transactions'::regclass
  AND attstattarget > 0;
```

## Practice Exercises

### Exercise 1: Diagnose the Problem

```sql-interactive
-- This query plan is suboptimal. What statistics issue causes it?
EXPLAIN SELECT
    a.account_name,
    COUNT(*)
FROM chart_of_accounts a
JOIN ledger_transactions t ON a.account_code = t.debit_account_code
WHERE a.account_type = 'EXPENSE'
GROUP BY a.account_name;

-- Plan shows:
-- NestedLoop (expecting 10 rows)
-- Actual: 15,000 rows

-- What's wrong?
-- A) Stale row count
-- B) Wrong cardinality for account_type
-- C) Missing correlation between tables
-- D) Incorrect join selectivity
```

### Exercise 2: Fix with Statistics

```sql-interactive
-- Query uses wrong plan. Fix with statistics commands:
SELECT *
FROM ledger_transactions
WHERE transaction_date >= CURRENT_DATE - 7
  AND debit_amount > (
    SELECT AVG(debit_amount) * 2
    FROM ledger_transactions
  );

-- Currently: Sequential scan
-- Should be: Index scan

-- Your solution:
-- ANALYZE ...
-- CREATE STATISTICS ...
-- ALTER TABLE ... SET STATISTICS ...
```

## Key Takeaways

1. **Statistics drive optimization decisions**
   - Wrong statistics -> wrong plans
   - Regular ANALYZE is critical

2. **Cardinality estimates cascade**
   - Early misestimates compound
   - Join estimates multiply errors

3. **Correlation matters for index scans**
   - High correlation -> index preferred
   - Low correlation -> sequential scan

4. **Histograms improve range estimates**
   - Default assumes uniform distribution
   - Real data is rarely uniform

5. **Monitor and update statistics**
   - After bulk loads
   - After major changes
   - Regularly for volatile tables

## Next Steps

Statistics work within a single database. But what if you need to run the same query on different databases? In [Chapter 8: Dialect Translation](08-dialect-translation.md), we'll see how RA adapts queries for PostgreSQL, MySQL, SQLite, and more.

---

* Pro Tip: If EXPLAIN ANALYZE shows large row estimate errors (off by 10x+), update statistics immediately. Bad statistics are the #1 cause of poor query performance.*