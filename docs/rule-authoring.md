# Rule Authoring Guide

This guide explains how to write transformation rules in `.rra` (Relational Rule Algebra) format.

## Overview

Each rule is a literate markdown file that documents a query transformation. The `.rra` format combines:

1. **YAML frontmatter** - Structured metadata
2. **Markdown documentation** - Human-readable explanation
3. **Code blocks** - Implementation and examples

## File Structure

```markdown
---
# YAML frontmatter (required)
id: rule-identifier
name: Human-Readable Name
category: logical/subcategory
...
---

# Rule Title

## Description
[Explanation of what the rule does]

## Relational Algebra
[Formal mathematical notation]

## Implementation
[Rust code using egg]

## Preconditions
[When the rule applies]

## Cost Model
[Estimated benefit]

## Test Cases
[SQL examples]

## References
[Source code and papers]
```

## Frontmatter Schema

Required fields:

```yaml
id: filter-through-join              # Unique identifier (kebab-case)
name: Filter Pushdown Through Join   # Display name
category: logical/predicate-pushdown # Category path
```

Optional fields:

```yaml
databases: [postgresql, mysql, duckdb]  # Databases implementing this
standard: sql:1992                      # SQL standard if applicable
execution_models: [volcano, vectorized] # Applicable execution models
version: 1.0.0                          # Rule version
authors: ["Name", "Database Team"]      # Authors/contributors
tags: [optimization, join, filter]      # Tags for searching
complexity: O(1)                        # Time complexity
benefit_range: [0.1, 0.9]               # Min/max benefit (0-1 scale)
```

Valid categories:
- `logical/predicate-pushdown`
- `logical/join-reordering`
- `logical/join-elimination`
- `logical/subquery-unnesting`
- `logical/projection-pushdown`
- `logical/aggregate-pushdown`
- `logical/expression-simplification`
- `logical/limit-pushdown`
- `logical/set-operations`
- `physical/join-algorithms`
- `physical/index-selection`
- `physical/aggregation-strategies`
- `physical/parallelization`
- `physical/materialization`
- `database-specific/{database-name}`
- `execution-models/{model-name}`
- `cost-models`
- `experimental`

Valid databases:
- `postgresql`, `mysql`, `duckdb`, `sqlite`, `oracle`, `mssql`, `datafusion`, `materialize`, `derby`, `monetdb`, `influxdb`

## Description Section

Explain what the rule does in plain English:

```markdown
## Description

Pushes selection predicates through join operators when the predicate only
references columns from one side of the join. This reduces the size of the
intermediate join result, improving performance.

**When to apply**: When a filter appears above a join and the filter predicate
only references columns from the left or right input.

**Why it works**: Filtering before joining reduces the number of tuples that
participate in the join operation, which is typically expensive.
```

## Relational Algebra Section

Use mathematical notation to formally specify the transformation:

```markdown
## Relational Algebra

\`\`\`algebra
σ[p](R ⋈[c] S) → (σ[p](R)) ⋈[c] S
  where attrs(p) ⊆ attrs(R)

σ[p](R ⋈[c] S) → R ⋈[c] (σ[p](S))
  where attrs(p) ⊆ attrs(S)
\`\`\`

Where:
- σ[p] is selection with predicate p
- ⋈[c] is join with condition c
- R, S are relations
- attrs(p) returns the set of attributes referenced by p
- ⊆ means "is a subset of"
```

Notation guide:
- `σ[p]` - Selection (filter)
- `π[A]` - Projection
- `⋈[c]` - Join
- `⋉` - Semi-join
- `⋊` - Anti-join
- `γ[G,A]` - Aggregation (group by G, aggregates A)
- `τ[O]` - Sort (order by O)
- `∪` - Union
- `∩` - Intersect
- `−` - Except/difference

## Implementation Section

Provide Rust code using egg rewrite rules:

```markdown
## Implementation (egg rewrite rule)

\`\`\`rust
use egg::{rewrite as rw, *};

rw!("filter-through-join-left";
    "(filter ?pred (join ?kind ?cond ?left ?right))" =>
    "(join ?kind ?cond (filter ?pred ?left) ?right)"
    if references_only(?pred, ?left)
),

rw!("filter-through-join-right";
    "(filter ?pred (join ?kind ?cond ?left ?right))" =>
    "(join ?kind ?cond ?left (filter ?pred ?right))"
    if references_only(?pred, ?right)
),
\`\`\`
```

Guard conditions (the `if` clause) ensure correctness:

```rust
fn references_only(pred: &Expr, rel: &Relation) -> bool {
    pred.referenced_columns()
        .is_subset(&rel.output_schema().columns())
}
```

## Preconditions Section

Document when the rule is applicable:

```markdown
## Preconditions

\`\`\`rust
fn applicable(join_type: JoinType, pred: &Expr) -> bool {
    // Only applies to INNER joins
    if !matches!(join_type, JoinType::Inner) {
        return false;
    }

    // Predicate must not reference the join condition
    if pred.references_join_columns() {
        return false;
    }

    // Predicate must be deterministic
    if !pred.is_deterministic() {
        return false;
    }

    true
}
\`\`\`

**Restrictions:**
- Only applies to INNER joins (not LEFT/RIGHT/FULL OUTER)
- Predicate must be deterministic (no random(), now(), etc.)
- Predicate must not reference join condition columns
```

## Cost Model Section

Estimate the benefit of applying the rule:

```markdown
## Cost Model

\`\`\`rust
fn estimated_benefit(
    left_stats: &Statistics,
    right_stats: &Statistics,
    pred_selectivity: f64,
) -> f64 {
    let left_card = left_stats.cardinality;
    let right_card = right_stats.cardinality;

    // Cost without pushdown: full join then filter
    let join_cost_before = left_card * right_card;
    let filter_cost = join_cost_before * FILTER_CPU_COST;
    let total_before = join_cost_before + filter_cost;

    // Cost with pushdown: filter then join
    let filtered_card = left_card * pred_selectivity;
    let join_cost_after = filtered_card * right_card;
    let filter_cost_pushdown = left_card * FILTER_CPU_COST;
    let total_after = join_cost_after + filter_cost_pushdown;

    // Return benefit (reduction in cost)
    (total_before - total_after) / total_before
}
\`\`\`

**Assumptions:**
- Join cost is proportional to product of cardinalities
- Filter cost is proportional to input size
- Selectivity is known or estimated

**Typical benefit**: 0.5-0.9 (50-90% cost reduction)
```

## Test Cases Section

Provide SQL examples demonstrating the transformation:

```markdown
## Test Cases

### Positive Case 1: Basic Filter Pushdown

\`\`\`sql
-- Input (before optimization)
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.amount > 1000;

-- Expected output (after optimization)
SELECT * FROM (
    SELECT * FROM orders WHERE amount > 1000
) o
JOIN customers c ON o.customer_id = c.id;
\`\`\`

### Positive Case 2: Multiple Predicates

\`\`\`sql
-- Input
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.amount > 1000 AND o.status = 'pending';

-- Expected output
SELECT * FROM (
    SELECT * FROM orders
    WHERE amount > 1000 AND status = 'pending'
) o
JOIN customers c ON o.customer_id = c.id;
\`\`\`

### Negative Case 1: Predicate References Both Sides

\`\`\`sql
-- Input (should NOT apply rule)
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.amount > c.credit_limit;  -- References both tables!

-- Output (unchanged)
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.amount > c.credit_limit;
\`\`\`

### Negative Case 2: Outer Join

\`\`\`sql
-- Input (should NOT apply rule for LEFT JOIN)
SELECT * FROM orders o
LEFT JOIN customers c ON o.customer_id = c.id
WHERE c.country = 'US';  -- Would change semantics!

-- Output (unchanged - pushing would eliminate NULL rows)
SELECT * FROM orders o
LEFT JOIN customers c ON o.customer_id = c.id
WHERE c.country = 'US';
\`\`\`
```

## References Section

Link to source code and academic papers:

```markdown
## References

**Implementation in databases:**
- PostgreSQL: `src/backend/optimizer/plan/initsplan.c:distribute_restrictinfo_to_rels()`
  - Git: https://github.com/postgres/postgres/blob/master/src/backend/optimizer/plan/initsplan.c
  - Lines: 2547-2623
- MySQL: `sql/sql_optimizer.cc:make_join_select()`
  - Git: https://github.com/mysql/mysql-server/blob/8.0/sql/sql_optimizer.cc
- DuckDB: `src/optimizer/filter_pushdown.cpp`
  - Git: https://github.com/duckdb/duckdb/blob/main/src/optimizer/filter_pushdown.cpp

**Academic papers:**
- Selinger, P. G., et al. "Access Path Selection in a Relational Database Management System."
  SIGMOD 1979. https://dl.acm.org/doi/10.1145/582095.582099
- Smith, J. M., & Chang, P. Y. T. "Optimizing the Performance of a Relational Algebra Database Interface."
  CACM 1975. https://dl.acm.org/doi/10.1145/361219.361220

**Textbooks:**
- Garcia-Molina et al., "Database Systems: The Complete Book", Section 16.2
- Ramakrishnan & Gehrke, "Database Management Systems", Chapter 15
```

## History Section (Optional)

Document rule evolution:

```markdown
## History

- **v1.0.0** (2026-03-17): Initial implementation
  - Based on System R optimizer design from 1979
  - Covers INNER joins only

- **v1.1.0** (2026-04-15): Extended support
  - Added support for semi-joins and anti-joins
  - Improved cost model with correlation statistics

- **v2.0.0** (2026-06-01): Major refactoring
  - Rewrote using egg rewrite rules
  - Added formal verification with TLA+
```

## Tips for Writing Good Rules

1. **Be Specific**: Clearly state when the rule applies and when it doesn't
2. **Provide Examples**: Include both positive and negative test cases
3. **Reference Sources**: Link to actual database implementations
4. **Explain Benefits**: Help users understand why the rule matters
5. **Test Thoroughly**: Include edge cases (NULLs, empty relations, etc.)
6. **Document Limitations**: Be honest about when the rule fails
7. **Use Standard Notation**: Follow mathematical conventions
8. **Keep It Simple**: One transformation per rule (compose rules for complex optimizations)

## Validation

Before submitting a rule, validate it:

```bash
# Validate syntax and schema
ra-cli validate rules/logical/predicate-pushdown/filter-through-join.rra

# Run test cases
ra-cli test rules/logical/predicate-pushdown/filter-through-join.rra

# Check against real database
ra-cli compare --database postgresql filter-through-join
```

## Example: Complete Rule

See [filter-through-join.rra](../rules/logical/predicate-pushdown/filter-through-join.rra) for a complete example following all guidelines.

## Contributing

When adding rules:

1. Fork the repository
2. Create a new `.rra` file in the appropriate category
3. Follow this authoring guide
4. Test the rule
5. Submit a pull request
6. Reference issues or discussions

See [CONTRIBUTING.md](../CONTRIBUTING.md) for details.
