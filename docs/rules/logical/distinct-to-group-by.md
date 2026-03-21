# Rule: DISTINCT to GROUP BY

**Category:** logical/aggregate-pushdown
**File:** `rules/logical/aggregate-pushdown/distinct-to-group-by.rra`

## Metadata

- **ID:** `distinct-to-group-by`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb
- **Tags:** distinct, group-by, normalization
- **Authors:** "RA Contributors"


# DISTINCT to GROUP BY

## Description

Normalizes SELECT DISTINCT to GROUP BY for uniform optimization. Enables
applying standard aggregation optimizations to DISTINCT queries.

**When to apply**: Always normalize DISTINCT to GROUP BY internally.

**Why it works**: GROUP BY and DISTINCT have same semantics but GROUP BY
enables more optimization opportunities.

## Relational Algebra

```algebra
distinct[cols](R)
  -> aggregate[group_by: cols, agg: none](R)
```

## Implementation

```rust
rw!("distinct-to-group-by";
    "(distinct ?cols ?input)" =>
    "(aggregate ?cols (list) ?input)"
),
```

## Cost Model

```rust
// No runtime benefit: normalization for optimization
fn benefit() -> f64 {
    0.0 // Enables other optimizations
}
```

**Typical benefit**: 0-20% (indirect through enabled optimizations)

## Test Cases

### Positive: Simple DISTINCT

```sql
SELECT DISTINCT category FROM products;

-- Normalize to:
SELECT category FROM products GROUP BY category;
```

### Positive: Multi-column DISTINCT

```sql
SELECT DISTINCT dept_id, job_title FROM employees;

-- GROUP BY dept_id, job_title
```

### Negative: DISTINCT with aggregates (already GROUP BY)

```sql
SELECT DISTINCT dept_id, COUNT(*) FROM employees GROUP BY dept_id;

-- Already using GROUP BY
```

## References

- SQL Standard: DISTINCT semantics
- All major databases: Internal normalization to GROUP BY
