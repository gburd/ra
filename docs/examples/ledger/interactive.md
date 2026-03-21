# Interactive Ledger: Query Optimization Lab

This page provides a hands-on query optimization lab using Alice's coffee shop ledger. Edit queries, adjust statistics, toggle indexes, and watch Ra's optimizer change its plans in real time.

## How This Works

Ra's WASM module runs entirely in your browser. When you click **Optimize**, Ra:

1. Parses your SQL into a relational algebra expression
2. Applies transformation rules (predicate pushdown, join reordering, etc.)
3. Estimates costs using the statistics you configure below
4. Returns both the original and optimized plans with cost breakdowns

No data leaves your browser. The statistics controls simulate what `ANALYZE` collects in a real database.

---

## Query Editor

Try any of the example queries below, or write your own against the [ledger schema](./02-schema.md).

```sql-interactive
SELECT
    a.account_name,
    a.account_type,
    t.category,
    COUNT(*) as transaction_count,
    SUM(t.amount) as total_amount,
    AVG(t.amount) as avg_amount
FROM accounts a
JOIN transactions t ON a.account_id = t.account_id
WHERE t.transaction_date >= '2024-01-01'
  AND t.transaction_date < '2024-07-01'
GROUP BY a.account_name, a.account_type, t.category
ORDER BY total_amount DESC;
```

---

## Statistics Controls

The controls below simulate what happens when table statistics change. In a real database, these come from `ANALYZE`. Here, adjusting them shows how the optimizer responds to different data profiles.

### Table Statistics

<div class="ra-stats-panel">
  <div class="ra-stats-group">
    <h4>transactions</h4>
    <div class="ra-stats-control">
      <label>Row count</label>
      <input type="range" id="stats-txn-rows" min="100" max="100000" value="1000" step="100">
      <output id="stats-txn-rows-val">1,000</output>
    </div>
    <div class="ra-stats-control">
      <label>Category cardinality</label>
      <input type="range" id="stats-cat-card" min="3" max="100" value="20" step="1">
      <output id="stats-cat-card-val">20</output>
    </div>
    <div class="ra-stats-control">
      <label>Date range (days)</label>
      <input type="range" id="stats-date-range" min="30" max="1095" value="365" step="30">
      <output id="stats-date-range-val">365</output>
    </div>
    <div class="ra-stats-control">
      <label>Amount skew</label>
      <select id="stats-amount-skew">
        <option value="uniform">Uniform</option>
        <option value="right-skew" selected>Right-skewed (many small, few large)</option>
        <option value="bimodal">Bimodal (sales + payroll)</option>
      </select>
    </div>
  </div>

  <div class="ra-stats-group">
    <h4>accounts</h4>
    <div class="ra-stats-control">
      <label>Row count</label>
      <input type="range" id="stats-acct-rows" min="5" max="1000" value="100" step="5">
      <output id="stats-acct-rows-val">100</output>
    </div>
    <div class="ra-stats-control">
      <label>account_type cardinality</label>
      <input type="range" id="stats-type-card" min="2" max="20" value="5" step="1">
      <output id="stats-type-card-val">5</output>
    </div>
  </div>
</div>

### Index Configuration

<div class="ra-index-panel">
  <div class="ra-index-toggle">
    <input type="checkbox" id="idx-txn-date" checked>
    <label for="idx-txn-date"><code>idx_txn_date</code> on <code>transactions(transaction_date)</code></label>
  </div>
  <div class="ra-index-toggle">
    <input type="checkbox" id="idx-txn-account" checked>
    <label for="idx-txn-account"><code>idx_txn_account</code> on <code>transactions(account_id)</code></label>
  </div>
  <div class="ra-index-toggle">
    <input type="checkbox" id="idx-txn-category">
    <label for="idx-txn-category"><code>idx_txn_category</code> on <code>transactions(category)</code></label>
  </div>
  <div class="ra-index-toggle">
    <input type="checkbox" id="idx-txn-amount">
    <label for="idx-txn-amount"><code>idx_txn_amount</code> on <code>transactions(amount)</code></label>
  </div>
  <div class="ra-index-toggle">
    <input type="checkbox" id="idx-txn-date-cat">
    <label for="idx-txn-date-cat"><code>idx_txn_date_category</code> on <code>transactions(transaction_date, category)</code> (composite)</label>
  </div>
  <div class="ra-index-toggle">
    <input type="checkbox" id="idx-acct-type">
    <label for="idx-acct-type"><code>idx_accounts_type</code> on <code>accounts(account_type)</code></label>
  </div>
</div>

---

## Example Queries

Each example highlights a different optimization concept. Click a query to load it into the editor above, then adjust the statistics to see how the plan changes.

### 1. Monthly Spending by Category

Demonstrates **predicate pushdown** and **aggregate optimization**. Watch how the filter on `transaction_date` moves closer to the table scan when statistics indicate high selectivity.

```sql-interactive
SELECT
    category,
    COUNT(*) as transaction_count,
    SUM(amount) as total_amount,
    ROUND(AVG(amount), 2) as avg_amount,
    MIN(amount) as min_amount,
    MAX(amount) as max_amount
FROM transactions
WHERE transaction_date >= '2024-01-01'
  AND transaction_date < '2024-02-01'
GROUP BY category
ORDER BY total_amount DESC;
```

**What to try:**
- Increase row count from 1,000 to 50,000 and observe whether Ra switches from sequential scan to index scan on `transaction_date`
- Toggle the `idx_txn_date` index off -- the optimizer falls back to a full table scan
- Enable the composite `idx_txn_date_category` index and notice the aggregate can partially use the index ordering

### 2. Top 10 Largest Transactions

Demonstrates **sort elimination** and **limit pushdown**. When an index provides the needed ordering, Ra can eliminate the explicit sort.

```sql-interactive
SELECT
    t.transaction_id,
    a.account_name,
    t.transaction_date,
    t.amount,
    t.category,
    t.description
FROM transactions t
JOIN accounts a ON t.account_id = a.account_id
WHERE t.amount > 500
ORDER BY t.amount DESC
LIMIT 10;
```

**What to try:**
- Enable `idx_txn_amount` and observe the sort operator disappear from the plan
- Change the `WHERE` threshold from 500 to 50 -- with more matching rows, the optimizer may prefer a hash join over nested loop
- Set transaction row count to 100,000 -- the cost difference between plans becomes dramatic

### 3. Running Account Balance

Demonstrates **window function optimization**. The `SUM() OVER (ORDER BY ...)` requires sorted input; Ra decides whether to sort explicitly or leverage an index.

```sql-interactive
SELECT
    t.transaction_date,
    t.description,
    t.amount,
    t.entry_type,
    SUM(
        CASE WHEN t.entry_type = 'DEBIT'
             THEN t.amount
             ELSE -t.amount
        END
    ) OVER (ORDER BY t.transaction_date, t.transaction_id) as running_balance
FROM transactions t
WHERE t.account_id = 1
ORDER BY t.transaction_date, t.transaction_id;
```

**What to try:**
- Toggle `idx_txn_account` to see how the access path for `account_id = 1` changes
- Increase row count -- with more rows, the window function cost dominates and Ra may choose incremental sort
- Notice how the CASE expression does not block predicate pushdown because the filter is on a different column

### 4. Category Aggregation with Join

Demonstrates **join reordering** based on cardinality estimates. Ra picks the smaller table as the build side of a hash join.

```sql-interactive
SELECT
    c.category_name,
    c.category_type,
    COUNT(t.transaction_id) as num_transactions,
    SUM(t.amount) as total_amount,
    ROUND(AVG(t.amount), 2) as avg_amount
FROM categories c
LEFT JOIN transactions t ON c.category_name = t.category
WHERE c.category_type = 'expense'
GROUP BY c.category_name, c.category_type
HAVING COUNT(t.transaction_id) > 0
ORDER BY total_amount DESC;
```

**What to try:**
- Categories table has 20 rows; transactions has 1,000+. Ra builds the hash table on categories (smaller side)
- Increase category cardinality to 100 and transaction rows to 100,000 -- watch the join method potentially change
- The LEFT JOIN prevents some optimizations that an INNER JOIN would allow; try changing it

### 5. Year-over-Year Comparison

Demonstrates **subquery decorrelation** and **self-join optimization**. The CTE materializes once and is scanned twice.

```sql-interactive
WITH monthly_totals AS (
    SELECT
        DATE_TRUNC('month', transaction_date) as month,
        category,
        SUM(amount) as total_amount,
        COUNT(*) as num_transactions
    FROM transactions
    WHERE transaction_date >= '2024-01-01'
    GROUP BY DATE_TRUNC('month', transaction_date), category
)
SELECT
    curr.month as current_month,
    curr.category,
    curr.total_amount as current_amount,
    prev.total_amount as previous_amount,
    ROUND(
        (curr.total_amount - COALESCE(prev.total_amount, 0))
        / NULLIF(prev.total_amount, 0) * 100,
        1
    ) as pct_change
FROM monthly_totals curr
LEFT JOIN monthly_totals prev
    ON curr.category = prev.category
    AND prev.month = curr.month - INTERVAL '1 month'
ORDER BY curr.month, curr.category;
```

**What to try:**
- This query scans `monthly_totals` twice. Ra materializes the CTE to avoid recomputation
- Increase date range to see more months and watch the self-join cost grow
- Change category cardinality -- with fewer categories the join is cheaper

---

## Plan Visualization Guide

When you click **Optimize**, Ra shows the query plan as a tree. Here is how to read it:

### Node Types

| Node | Meaning | When Used |
|------|---------|-----------|
| `Scan` | Full table scan | No useful index, or small table |
| `IndexScan` | B-tree index lookup | Selective filter with matching index |
| `IndexOnlyScan` | Index-only (covering) | All needed columns are in the index |
| `Filter` | Row-level predicate | After scan, removes non-matching rows |
| `Join` (nested loop) | Row-by-row join | Small outer table, indexed inner |
| `HashJoin` | Hash-based join | Large tables, equality predicates |
| `Sort` | Explicit sort | ORDER BY without matching index |
| `Aggregate` | Group-by computation | GROUP BY, COUNT, SUM, etc. |
| `Window` | Window function | OVER() clauses |
| `Limit` | Row count cap | LIMIT clause |

### Cost Components

Ra breaks cost into four components:

- **CPU**: Computation cost (expression evaluation, hashing, comparison)
- **I/O**: Disk access cost (sequential vs random reads)
- **Memory**: Working memory for sorts, hash tables, aggregation buffers
- **Network**: Transfer cost (relevant for distributed queries)

The optimizer minimizes **total cost**, which is a weighted sum of these components. The weights come from the hardware profile -- a laptop with fast SSD penalizes I/O less than a server with spinning disks.

---

## Why Plans Change

This section explains the optimizer's reasoning for common plan transitions.

### Sequential Scan to Index Scan

**When it happens:** A filter becomes more selective.

With 1,000 rows and a filter matching 50%, sequential scan wins because it reads pages in order. With the same filter matching 0.5% (5 rows), an index scan fetches only those 5 rows.

**Threshold:** Roughly when selectivity drops below 5-15% of the table, depending on row width and index structure.

### Nested Loop to Hash Join

**When it happens:** The outer table grows.

Nested loop join is O(N * lookup_cost). When the outer table is small and the inner has an index, this is fast. When both tables are large, hash join's O(N + M) build-and-probe dominates.

**Threshold:** Typically when the outer table exceeds a few hundred rows, though this depends on available memory for the hash table.

### Sort to Index Scan (Sort Elimination)

**When it happens:** An index provides the needed ordering.

If `ORDER BY transaction_date` is requested and `idx_txn_date` exists, Ra can scan the index in order and skip the sort entirely. This saves O(N log N) work.

**Threshold:** Always preferred when the index covers the required ordering and the query accesses a significant fraction of the table.

### Hash Aggregate to Sort Aggregate

**When it happens:** Input is already sorted on the GROUP BY key.

If a preceding index scan or sort already orders the data by the grouping columns, a streaming (sort-based) aggregate uses less memory than building a hash table.

**Threshold:** Depends on the number of groups relative to input size. Many groups favor hash aggregate; few groups favor sort aggregate when input is pre-sorted.

---

## WASM Integration Status

The interactive features on this page use Ra's `ra-wasm-docs` crate, which wraps the parser, optimizer, and formatter for browser use.

**Currently available:**
- SQL parsing to relational algebra (`parse_sql`)
- Query optimization with cost estimates (`optimize`)
- Dialect translation (`translate`)
- SQL formatting (`format`)

**Requires WASM build:**
- Statistics-aware optimization (the `WasmOptimizer` in `ra-wasm` supports `addTableStats` and hardware profiles)
- The statistics sliders on this page will connect to `WasmOptimizer.addTableStats()` once the full WASM build is integrated into the docs pipeline

To build the WASM module locally:

```bash
cd docs && npm run build:wasm
```

This runs `wasm-pack build` on `crates/ra-wasm-docs` and places output in `docs/static/wasm/`.

---

## Schema Reference

The interactive examples use a simplified ledger schema. See [data.sql](./data.sql) for the full dataset.

| Table | Rows | Key Columns |
|-------|------|-------------|
| `accounts` | 100 | `account_id`, `account_type`, `account_name` |
| `transactions` | 262+ | `account_id`, `transaction_date`, `amount`, `category` |
| `categories` | 20 | `category_name`, `category_type` |

For the full double-entry accounting schema with multi-currency support, see [ledger.sql](./ledger.sql) and [Chapter 2: Schema Design](./02-schema.md).

---

## Next Steps

- [Chapter 6: Optimization Journey](./06-optimization-journey.md) -- step-by-step walkthrough of Ra optimizing a real query
- [Chapter 7: Statistics Impact](./07-statistics-impact.md) -- deeper exploration of how statistics drive plan selection
- [Chapter 9: Hardware Awareness](./09-hardware-awareness.md) -- how CPU, memory, and storage characteristics influence cost models
