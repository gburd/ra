# Rule: HAVING to Filter Separation

**Category:** logical/aggregate-pushdown
**File:** `rules/logical/aggregate-pushdown/having-to-filter-separation.rra`

## Metadata

- **ID:** `having-to-filter-separation`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb
- **Tags:** aggregation, having, filter, separation
- **Authors:** "RA Contributors"


# HAVING to Filter Separation

## Description

Separates HAVING conditions into pre-aggregate filters (WHERE) and post-aggregate
filters, reducing rows before aggregation.

**When to apply**: HAVING clause contains conditions on non-aggregated columns.

**Why it works**: Filtering before aggregation reduces input size, making
aggregation cheaper.

## Relational Algebra

```algebra
aggregate[group, agg] + having[cond_on_base AND cond_on_agg](R)
  -> having[cond_on_agg](aggregate[group, agg](filter[cond_on_base](R)))
```

## Implementation

```rust
rw!("having-to-filter-separation";
    "(having (and ?base_cond ?agg_cond) (aggregate ?group ?agg ?input))" =>
    "(having ?agg_cond (aggregate ?group ?agg (filter ?base_cond ?input)))"
    if is_base_column_condition("?base_cond")
    if is_aggregate_condition("?agg_cond")
),
```

## Cost Model

```rust
fn benefit(input_rows: u64, filter_selectivity: f64) -> f64 {
    let filtered_rows = (input_rows as f64 * filter_selectivity) as u64;
    let without = input_rows; // Aggregate all rows
    let with = filtered_rows; // Aggregate filtered rows
    (without - with) as f64 / without as f64
}
```

**Typical benefit**: 30-70% when base filters are selective

## Test Cases

### Positive: Separate base and aggregate conditions

```sql
SELECT dept_id, COUNT(*)
FROM employees
GROUP BY dept_id
HAVING dept_id > 100 AND COUNT(*) > 10;

-- Rewrite to:
SELECT dept_id, COUNT(*)
FROM employees
WHERE dept_id > 100
GROUP BY dept_id
HAVING COUNT(*) > 10;
```

### Positive: Multiple base conditions

```sql
SELECT category, SUM(price)
FROM products
GROUP BY category
HAVING category IN ('electronics', 'books') AND SUM(price) > 10000;
```

### Negative: Only aggregate conditions

```sql
SELECT dept_id, AVG(salary)
FROM employees
GROUP BY dept_id
HAVING AVG(salary) > 50000;

-- Cannot separate: no base column conditions
```

## References

- PostgreSQL: preprocess_expression for HAVING optimization
- SQL Standard: HAVING clause semantics
