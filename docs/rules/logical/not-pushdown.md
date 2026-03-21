# Rule: NOT Pushdown (De Morgan's Laws)

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/not-pushdown.rra`

## Metadata

- **ID:** `not-pushdown`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** function, not, boolean, de-morgan, pushdown, simplification
- **Authors:** "RA Contributors"


# NOT Pushdown (De Morgan's Laws)

## Description

Pushes NOT through boolean connectives using De Morgan's laws to eliminate
NOT operators and expose simpler predicates for index usage and predicate
pushdown.

**When to apply**: NOT applied to compound boolean expressions (AND, OR)
or double-negated expressions.

## Relational Algebra

```algebra
NOT (A AND B) -> (NOT A) OR (NOT B)     -- De Morgan
NOT (A OR B)  -> (NOT A) AND (NOT B)    -- De Morgan
NOT (NOT A)   -> A                       -- double negation
NOT TRUE      -> FALSE
NOT FALSE     -> TRUE
NOT (a = b)   -> a != b
NOT (a > b)   -> a <= b
NOT (a IN S)  -> a NOT IN S
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("not-and-to-or"; "(not (and ?a ?b))" => "(or (not ?a) (not ?b))"),
rw!("not-or-to-and"; "(not (or ?a ?b))" => "(and (not ?a) (not ?b))"),
rw!("not-not-elimination"; "(not (not ?a))" => "?a"),
rw!("not-true"; "(not (literal true))" => "(literal false)"),
rw!("not-false"; "(not (literal false))" => "(literal true)"),
rw!("not-equals"; "(not (= ?a ?b))" => "(!= ?a ?b)"),
rw!("not-greater"; "(not (> ?a ?b))" => "(<= ?a ?b)"),
rw!("not-less"; "(not (< ?a ?b))" => "(>= ?a ?b)"),
rw!("not-gte"; "(not (>= ?a ?b))" => "(< ?a ?b)"),
rw!("not-lte"; "(not (<= ?a ?b))" => "(> ?a ?b)"),
```

## Cost Model

```rust
fn estimated_benefit(exposes_index: bool) -> f64 {
    if exposes_index { 0.3 } else { 0.05 }
}
```

## Test Cases

### Positive: NOT on AND (De Morgan)

```sql
SELECT * FROM t WHERE NOT (status = 'active' AND priority > 5);
-- Rewrite to: status != 'active' OR priority <= 5
```

### Positive: Double negation

```sql
SELECT * FROM t WHERE NOT NOT (active = true);
-- Rewrite to: WHERE active = true
```

### Positive: NOT on comparison

```sql
SELECT * FROM t WHERE NOT (price > 100);
-- Rewrite to: WHERE price <= 100
-- Enables index range scan
```

### Negative: NOT on non-boolean expression

```sql
SELECT * FROM t WHERE NOT custom_function(col);
-- Cannot push through opaque function
```

## References

**Implementation:**
- PostgreSQL: `negate_clause()` in `prepqual.c`
- All SQL engines apply De Morgan's laws during normalization
