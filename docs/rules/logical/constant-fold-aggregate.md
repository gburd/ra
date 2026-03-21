# Rule: Constant Fold Aggregate on Constants

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/constant-fold-aggregate.rra`

## Metadata

- **ID:** `constant-fold-aggregate`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** logical, function, constant-folding, aggregate, values
- **Authors:** "RA Contributors"


# Constant Fold Aggregate on Constants

## Description

Evaluates aggregate functions over constant VALUES at plan time.
COUNT(*) over a known-size VALUES clause, SUM/MIN/MAX over constant
tuples can all be computed during optimization.

**When to apply**: An aggregate operates on a VALUES node (all rows
are constant literals).

## Implementation

```rust
rw!("constant-fold-count-values";
    "(aggregate empty-group (count-star) (values ?rows))" =>
    "(values (literal (count-rows ?rows)))"
),
rw!("constant-fold-sum-values";
    "(aggregate empty-group (sum ?col) (values ?rows))" =>
    "(values (literal (sum-column ?col ?rows)))"
    if all_values_constant("?rows")
),
rw!("constant-fold-min-values";
    "(aggregate empty-group (min ?col) (values ?rows))" =>
    "(values (literal (min-column ?col ?rows)))"
    if all_values_constant("?rows")
),
```

## Test Cases

```sql
-- Positive: COUNT over VALUES
SELECT COUNT(*) FROM (VALUES (1), (2), (3)) AS t(x);
-- Folded to: SELECT 3

-- Positive: SUM over VALUES
SELECT SUM(x) FROM (VALUES (10), (20), (30)) AS t(x);
-- Folded to: SELECT 60

-- Negative: aggregate over table
SELECT COUNT(*) FROM orders;
-- Cannot fold: table size unknown at plan time
```

## References

- Calcite: AggregateValuesRule
