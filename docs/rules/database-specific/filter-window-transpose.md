# Rule: Filter Window Transpose

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/filter-window-transpose.rra`

## Metadata

- **ID:** `filter-window-transpose`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, oracle
- **Tags:** filter, window, transpose, pushdown
- **Authors:** "Apache Calcite Contributors"


# Filter Window Transpose

## Description

Pushes filters through window functions when the filter predicate only
references non-window columns (the input columns, not the computed window
function results). This reduces the input cardinality to the window operation,
improving window function performance.

**When to apply**: A filter appears above a window operation, and the filter
predicate only references the original input columns, not the window function
results. The filter can be safely pushed below the window.

**Why it works**: Window functions compute over their input rows, preserving
the input schema and adding computed columns. Filters on input columns can
be evaluated before window computation without affecting semantics, reducing
the window's working set.

## Relational Algebra

```algebra
σ_p(WINDOW_f(R)) where p references only R columns ->
  WINDOW_f(σ_p(R))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("filter-window-transpose";
    "(filter ?pred (window ?specs ?input))" =>
    "(window ?specs
       (filter ?pred ?input))"
    if filter-references-only-input("?pred", "?input", "?specs")
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> bool {
    // Filter must reference only input columns (not window function results)
    stats.filter_columns.is_disjoint(&stats.window_function_columns)
        // Filter should be selective
        && stats.filter_selectivity < 0.7
        // Window function should be present
        && !stats.window_specs.is_empty()
}
```

**Restrictions:**
- Filter must not reference window function result columns
- Filter must reference only original input columns
- Window function must not depend on filtered rows (e.g., ROW_NUMBER would change)

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let input_rows = stats.input_row_count as f64;
    let selectivity = stats.filter_selectivity;
    let filtered_rows = input_rows * selectivity;
    let rows_eliminated = input_rows - filtered_rows;

    // Window function cost is typically O(n log n) for sorting
    // or O(n) for simple aggregates within partitions
    let window_cost_per_row = if stats.window_needs_sort {
        0.00001 // 10μs per row for sort-based window
    } else {
        0.000005 // 5μs per row for partition-based window
    };

    // Cost savings from smaller window input
    let window_savings = rows_eliminated * window_cost_per_row;

    // Filter evaluation cost
    let filter_cost_per_row = 0.000001; // 1μs per row
    let filter_cost = input_rows * filter_cost_per_row;

    // Normalize
    let total_query_cost = input_rows * (window_cost_per_row + filter_cost_per_row);
    (window_savings / total_query_cost).min(0.55)
}
```

**Assumptions:**
- Window functions with ORDER BY require sorting: O(n log n)
- Window functions with PARTITION BY only: O(n)
- Filter evaluation: ~1μs per row
- Selective filters significantly reduce window working set

**Typical benefit**: 20-55% for selective filters on large window inputs.

## Test Cases

### Positive: Filter on input column

```sql
-- Window function with filter on input column
SELECT
  employee_id,
  salary,
  AVG(salary) OVER (PARTITION BY department) as avg_dept_salary
FROM employees
WHERE hire_date >= '2020-01-01';

-- Before:
-- Filter(hire_date >= '2020-01-01')
--   Window[AVG(salary) PARTITION BY department]
--     Scan(employees)

-- After filter-window-transpose:
-- Window[AVG(salary) PARTITION BY department]
--   Filter(hire_date >= '2020-01-01')
--     Scan(employees)

-- Filter reduces input from 10M to 2M employees before window
```

### Positive: Multiple filters on input

```sql
-- Multiple filters, all on input columns
SELECT
  order_id,
  amount,
  ROW_NUMBER() OVER (PARTITION BY customer_id ORDER BY order_date) as row_num
FROM orders
WHERE order_date >= '2024-01-01'
  AND status = 'completed';

-- Both filters push through window
```

### Negative: Filter on window function result

```sql
-- Filter references window function result
SELECT *
FROM (
  SELECT
    employee_id,
    salary,
    ROW_NUMBER() OVER (ORDER BY salary DESC) as rank
  FROM employees
) t
WHERE rank <= 10;

-- Cannot push 'rank' filter through window - rank is computed by window!
```

### Positive: Selective filter on partition key

```sql
-- Filter on partition key
SELECT
  product_id,
  sale_date,
  SUM(amount) OVER (PARTITION BY product_id ORDER BY sale_date) as running_total
FROM sales
WHERE product_id IN (100, 200, 300);

-- Filter eliminates most partitions, dramatically reducing window cost
```

### Negative: Filter affects window semantics

```sql
-- ROW_NUMBER depends on all rows
SELECT
  order_id,
  ROW_NUMBER() OVER (ORDER BY order_date) as row_num
FROM orders
WHERE amount > 100;

-- Careful: pushing filter changes ROW_NUMBER assignments
-- (This is actually OK semantically, but optimizer must verify)
```

## References

**Implementation in databases:**
- Apache Calcite: `FilterWindowTransposeRule.java`
- PostgreSQL: Window function with filter pushdown (window.c)
- Oracle: Analytical function optimization
- mssql: Window function filter optimization

**Academic papers:**
- Cao et al., "Optimization of Analytic Window Functions", VLDB 2012
  - DOI: 10.14778/2367502.2367534
  - Window function execution and optimization strategies
- Leis et al., "How Good Are Query Optimizers, Really?", VLDB 2015
  - DOI: 10.14778/2850583.2850594
  - Window function challenges in modern optimizers
- Bellamkonda et al., "Enhanced Subquery Optimizations in Oracle", VLDB 2009
  - Window function and predicate interaction
