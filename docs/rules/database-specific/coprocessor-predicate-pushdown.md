# Rule: TiDB Coprocessor Predicate Pushdown

**Category:** database-specific/tidb
**File:** `rules/database-specific/tidb/coprocessor-predicate-pushdown.rra`

## Metadata

- **ID:** `tidb-coprocessor-predicate-pushdown`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** coprocessor, pushdown, filter, tikv, distributed
- **Authors:** "PingCAP TiDB Team", "RA Contributors"


# TiDB Coprocessor Predicate Pushdown

## Description

Pushes filter predicates down to TiKV coprocessor tasks, executing filters
at the storage layer before data is transferred to TiDB server. This is a
core optimization in TiDB's distributed architecture, dramatically reducing
network traffic and improving query performance.

**When to apply**: Filters on columns stored in TiKV that can be evaluated
without TiDB server-side functions or complex expressions.

**Why it works**: TiKV coprocessors can evaluate predicates directly on
stored data, filtering out non-matching rows before sending results over
the network. For highly selective predicates, this reduces data transfer
by orders of magnitude.

## Relational Algebra

```algebra
Filter[pred](Scan[table])
  -> CopTask(Filter[pred](Scan[table]))
  where is_pushable_to_tikv(pred)

Pushable predicates:
- Simple comparisons: =, <>, <, >, <=, >=
- Logical: AND, OR, NOT
- IN, BETWEEN
- IS NULL, IS NOT NULL
- Pattern matching: LIKE (with leading literal)
- Arithmetic expressions on columns
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("tidb-cop-predicate-pushdown";
    "(filter ?pred (scan ?table))" =>
    "(cop_task (filter ?pred (scan ?table)))"
    if is_pushable_to_coprocessor("?pred")
    if table_stored_in_tikv("?table")
),

// Coprocessor pushdown decision logic
fn is_pushable_to_coprocessor(pred: &Predicate) -> bool {
    match pred {
        // Simple column comparisons always pushable
        Predicate::Comparison {
            op: CompOp::Eq
                | CompOp::Ne
                | CompOp::Lt
                | CompOp::Le
                | CompOp::Gt
                | CompOp::Ge,
            left,
            right,
        } => is_pushable_expr(left) && is_pushable_expr(right),

        // Logical operators: recursively check children
        Predicate::And(preds) | Predicate::Or(preds) => {
            preds.iter().all(is_pushable_to_coprocessor)
        }
        Predicate::Not(inner) => is_pushable_to_coprocessor(inner),

        // Range predicates
        Predicate::In { col, values } => {
            is_column_ref(col) && values.iter().all(is_constant)
        }
        Predicate::Between { col, low, high } => {
            is_column_ref(col) && is_constant(low) && is_constant(high)
        }

        // NULL checks
        Predicate::IsNull(col) | Predicate::IsNotNull(col) => is_column_ref(col),

        // Pattern matching with limitations
        Predicate::Like { col, pattern } => {
            is_column_ref(col) && has_leading_literal(pattern)
        }

        // Not pushable: UDFs, correlated subqueries, window functions
        Predicate::Subquery(_)
        | Predicate::WindowFunc(_)
        | Predicate::UserDefinedFunc(_) => false,
    }
}

fn is_pushable_expr(expr: &Expression) -> bool {
    match expr {
        // Column references and constants always pushable
        Expression::ColumnRef(_) | Expression::Constant(_) => true,

        // Arithmetic on pushable expressions
        Expression::BinaryOp { op, left, right } => {
            matches!(
                op,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod
            ) && is_pushable_expr(left)
                && is_pushable_expr(right)
        }

        // Cast operations (if target type is supported by TiKV)
        Expression::Cast { expr, target_type } => {
            is_pushable_expr(expr) && is_tikv_supported_type(target_type)
        }

        // Aggregations, subqueries, UDFs not pushable
        Expression::Aggregate(_)
        | Expression::Subquery(_)
        | Expression::ScalarUDF(_) => false,
    }
}

// TiKV coprocessor task representation
struct CopTask {
    table: TableRef,
    filters: Vec<Predicate>,
    projections: Vec<Expression>,
    limit: Option<u64>,
    // Execution happens in parallel across TiKV nodes
    regions: Vec<RegionId>,
}

impl CopTask {
    fn estimate_data_transfer(&self, full_scan_bytes: u64, selectivity: f64) -> u64 {
        // Without pushdown: transfer all data
        // With pushdown: transfer only matching rows
        (full_scan_bytes as f64 * selectivity) as u64
    }

    fn estimate_network_cost(&self, bytes: u64, network_bandwidth_gbps: f64) -> f64 {
        let bandwidth_bytes = network_bandwidth_gbps * 1e9;
        bytes as f64 / bandwidth_bytes
    }
}
```

**Restrictions:**
- Predicate must use only TiKV-supported functions (no UDFs)
- No correlated subqueries (require server-side evaluation)
- LIKE patterns must have leading literal (no `LIKE '%suffix'`)
- Generated columns may block pushdown if complex
- Collation differences between TiDB and TiKV

## Cost Model

```rust
fn estimated_benefit(
    scan_bytes: u64,
    selectivity: f64,
    network_bandwidth_gbps: f64,
) -> f64 {
    // Without pushdown: Transfer all data, filter at TiDB server
    let transfer_all = scan_bytes as f64 / (network_bandwidth_gbps * 1e9);
    let server_filter_cost = (scan_bytes as f64 / 1e6) * 10.0; // 10ns per KB
    let cost_without = transfer_all + server_filter_cost;

    // With pushdown: Filter at TiKV, transfer only matching rows
    let cop_filter_cost = (scan_bytes as f64 / 1e6) * 5.0; // 5ns per KB (faster local)
    let filtered_bytes = scan_bytes as f64 * selectivity;
    let transfer_filtered = filtered_bytes / (network_bandwidth_gbps * 1e9);
    let cost_with = cop_filter_cost + transfer_filtered;

    if cost_without > cost_with {
        (cost_without - cost_with) / cost_without
    } else {
        0.0
    }
}
```

**Assumptions:**
- TiKV coprocessor can evaluate predicates at near-memory-speed
- Network bandwidth is the primary bottleneck (10-100 Gbps in production)
- Predicate selectivity significantly reduces data transfer (< 0.5)
- TiKV regions can execute coprocessor tasks in parallel

**Typical benefit**: 50-95% for highly selective predicates (selectivity < 0.1),
especially on large tables where network transfer dominates query time.

## Test Cases

### Positive: Simple equality predicate

```sql
SELECT * FROM orders WHERE status = 'shipped';

-- Without pushdown:
-- 1. Scan all orders from TiKV (1TB)
-- 2. Transfer 1TB over network to TiDB
-- 3. Filter at TiDB server
-- Time: ~100s (network bound)

-- With coprocessor pushdown:
-- 1. Push filter to TiKV coprocessor
-- 2. TiKV filters locally (status = 'shipped')
-- 3. Transfer only matching rows (100GB, 10% selectivity)
-- Time: ~10s (10x faster)
```

### Positive: Range predicate with arithmetic

```sql
SELECT * FROM sales
WHERE sale_date >= '2024-01-01'
  AND sale_date < '2024-02-01'
  AND amount * quantity > 1000;

-- All predicates pushable to TiKV coprocessor
-- Reduces data transfer significantly (e.g., 1TB -> 50GB)
```

### Negative: UDF not pushable

```sql
CREATE FUNCTION calculate_discount(price DECIMAL) RETURNS DECIMAL AS ...;

SELECT * FROM products
WHERE calculate_discount(price) < 100;

-- UDF not available in TiKV coprocessor
-- Must evaluate at TiDB server (no pushdown)
```

### Negative: Correlated subquery

```sql
SELECT * FROM orders o
WHERE o.total > (
  SELECT AVG(total) FROM orders WHERE customer_id = o.customer_id
);

-- Correlated subquery requires server-side evaluation
-- Cannot push to TiKV coprocessor
```

## References

**Source code:**
- File: `pkg/planner/core/find_best_task.go`
- Function: `copTaskBuilder.buildCopTask()`
- File: `pkg/planner/core/exhaust_physical_plans.go`
- Repository: https://github.com/pingcap/tidb

**TiDB Documentation:**
- Coprocessor: https://docs.pingcap.com/tidb/stable/tidb-operator-pushdown
- Expression Pushdown List: https://docs.pingcap.com/tidb/stable/expressions-pushed-down

**TiKV Documentation:**
- Coprocessor Framework: https://tikv.org/docs/latest/concepts/architecture/#coprocessor
- Expression Evaluation: https://github.com/tikv/tikv/tree/master/components/tidb_query_expr

**Related concepts:**
- Compute pushdown in distributed databases
- Filter early principle (Volcano optimizer)
- Storage-level predicate evaluation (Parquet, ORC)
