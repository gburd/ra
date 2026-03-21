# Rule: Use Partition-Aligned Index Scan

**Category:** physical/rpr
**File:** `rules/rpr/partition-index-scan.rra`

## Metadata

- **ID:** `rpr-partition-index-scan`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle, duckdb, mssql
- **Tags:** rpr, index, partition, scan, sort-elimination
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: pattern
    must_match: "(row-pattern ?input ?partition ?order ?defines ?measures ?pattern)"
  - type: fact
    fact_type: schema.index_exists
    table: "?input_table"
    columns: "?partition_and_order_cols"
    description: "Composite index exists on (PARTITION BY cols, ORDER BY cols)"
```


# Use Partition-Aligned Index Scan

## Description

When a composite index covers both PARTITION BY and ORDER BY columns,
use an index scan that delivers rows already grouped by partition and
sorted within each partition. This eliminates both the explicit sort
and the partition grouping step.

**When to apply**: A composite index on `(partition_cols, order_cols)`
exists for the scanned table.

**Why it works**: A B-tree index on `(symbol, trade_date)` delivers
rows grouped by `symbol` and sorted by `trade_date` within each
group. This exactly matches the input requirement of MATCH_RECOGNIZE
with `PARTITION BY symbol ORDER BY trade_date`, eliminating both
the hash-based partition grouping and the sort.

## Relational Algebra

```algebra
-- Before: hash partition + sort
RowPattern(
  Sort(trade_date,
    HashPartition(symbol,
      SeqScan(stock_prices))),
  partition: [symbol],
  order: [trade_date], ...
)

-- After: index scan provides both grouping and ordering
RowPattern(
  IndexScan(stock_prices, idx_symbol_date),
  partition: [symbol],
  order: [trade_date], ...
)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Composite index eliminates sort and partition hash
rw!("rpr-partition-index-scan";
    "(row-pattern
       (sort ?order (hash-partition ?partition (scan ?table)))
       ?partition ?order ?defines ?measures ?pattern)" =>
    "(row-pattern
       (index-scan ?table
         (lookup-composite-index ?table ?partition ?order))
       ?partition ?order ?defines ?measures ?pattern)"
    if composite_index_exists("?table", "?partition", "?order")
),

// Index scan with pushed filter
rw!("rpr-partition-index-scan-with-filter";
    "(row-pattern
       (sort ?order
         (hash-partition ?partition
           (filter ?pred (scan ?table))))
       ?partition ?order ?defines ?measures ?pattern)" =>
    "(row-pattern
       (index-scan ?table
         (lookup-composite-index ?table ?partition ?order)
         (index-filter ?pred))
       ?partition ?order ?defines ?measures ?pattern)"
    if composite_index_exists("?table", "?partition", "?order")
),
```

## Preconditions

```rust
fn composite_index_exists(
    table: &str,
    partition: &[Expr],
    order: &[OrderByExpr],
) -> bool {
    let mut key_cols: Vec<String> = partition.iter()
        .filter_map(|e| e.as_column_ref())
        .map(|c| c.to_string())
        .collect();
    key_cols.extend(order.iter().map(|o| o.column.clone()));
    schema::find_index_with_prefix(table, &key_cols).is_some()
}
```

**Restrictions:**
- Index must have partition columns as prefix, followed by order columns.
- Index direction must match ORDER BY direction (ASC vs DESC).
- If partition columns are not a prefix of the index, this rule cannot apply.
- For partial indexes, the index predicate must be compatible with any pushed filters.

## Cost Model

```rust
fn estimated_benefit(
    table_card: f64,
    num_partitions: f64,
) -> f64 {
    // Sort cost: O(n log n) for full sort
    let sort_cost = table_card * table_card.log2() * 0.001;
    // Hash partition cost: O(n) hash + O(n) redistribute
    let hash_cost = table_card * 0.002;
    // Index scan cost: O(n) sequential read
    let index_cost = table_card * 0.005;

    let saved = sort_cost + hash_cost - index_cost;
    saved / (sort_cost + hash_cost)
}
```

**Typical benefit**: 30-70%. Eliminates O(n log n) sort and
O(n) hash partition overhead.

## Test Cases

### Positive: composite index on (symbol, trade_date)

```sql
-- Given: CREATE INDEX idx ON stock_prices(symbol, trade_date)
SELECT * FROM stock_prices
  MATCH_RECOGNIZE (
    PARTITION BY symbol
    ORDER BY trade_date
    PATTERN (A+ B+)
    DEFINE A AS price > PREV(price), B AS price < PREV(price)
  );
-- After: IndexScan(idx) provides grouped, sorted input
```

### Negative: index on ORDER BY only (no partition prefix)

```sql
-- Given: CREATE INDEX idx ON stock_prices(trade_date)
-- Cannot use: rows not grouped by symbol
-- Must still hash-partition by symbol
```

### Negative: reversed column order in index

```sql
-- Given: CREATE INDEX idx ON stock_prices(trade_date, symbol)
-- Cannot use: partition columns (symbol) are not the index prefix
```

## References

- Selinger et al. "Access Path Selection in a Relational Database Management System" (1979)
- PostgreSQL: build_index_paths in optimizer/path/indxpath.c
- Graefe, G. "Implementing Sorting in Database Systems" (2006)
