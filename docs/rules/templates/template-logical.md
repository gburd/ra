# Rule: Human-Readable Rule Name

**Category:** logical/SUBCATEGORY
**File:** `rules/templates/template-logical.rra`

## Metadata

- **ID:** `RULE-ID-HERE`
- **Version:** 1.0.0
- **Databases:** postgresql, mysql, duckdb, sqlite
- **Tags:** optimization
- **SQL Standard:** sql:1992
- **Authors:** "Your Name"


# Rule Name

## Description

Describe what the rule does in plain English. Explain:

**When to apply**: Under what conditions this transformation is beneficial.

**Why it works**: The theoretical justification for correctness and performance.

## Relational Algebra

```algebra
LHS_EXPRESSION -> RHS_EXPRESSION
  where CONDITION
```

Where:
- Define all symbols used above

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("RULE-ID-HERE";
    "(PATTERN)" =>
    "(REPLACEMENT)"
    if GUARD_CONDITION
),
```

## Preconditions

```rust
fn applicable(/* params */) -> bool {
    // Document structural and semantic preconditions
    true
}
```

**Restrictions:**
- List conditions under which the rule must NOT be applied

## Cost Model

```rust
fn estimated_benefit(/* stats */) -> f64 {
    // Estimate cost reduction
    0.5
}
```

**Assumptions:**
- List cost model assumptions

**Typical benefit**: X-Y% cost reduction

## Test Cases

### Positive Case 1: Basic Transformation

```sql
-- Input (before optimization)
SELECT ...;

-- Expected output (after optimization)
SELECT ...;
```

### Negative Case 1: Rule Should Not Apply

```sql
-- Input (should NOT apply rule)
SELECT ...;

-- Output (unchanged)
SELECT ...;
```

## References

**Implementation in databases:**
- PostgreSQL: `src/backend/optimizer/...`
- DuckDB: `src/optimizer/...`

**Academic papers:**
- Author, "Title", Conference Year. URL
