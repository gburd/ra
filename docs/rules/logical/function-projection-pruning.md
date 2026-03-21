# Rule: Prune Unused Function Calls from Projections

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/function-projection-pruning.rra`

## Metadata

- **ID:** `function-projection-pruning`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** logical, function, pruning, projection, dead-code
- **Authors:** "RA Contributors"


# Prune Unused Function Calls from Projections

## Description

Removes function calls from intermediate projections when their
results are not referenced by any upstream operator. If a subquery
computes MD5(col) but the outer query never uses that column, the
expensive function call can be eliminated entirely.

**When to apply**: A projection contains a function call whose
output column is not referenced by any parent operator.

**Why it works**: Eliminating unreferenced function computations
saves per-row CPU cost, especially for expensive functions.

## Implementation

```rust
// Remove unreferenced function column from projection
rw!("prune-unused-func-in-project";
    "(project ?used_cols
       (project [?cols.. (?f ?args) as ?unused] ?input))" =>
    "(project ?used_cols
       (project [?cols..] ?input))"
    if not_referenced_above("?unused", "?used_cols")
),

// Remove function from subquery when outer doesn't use it
rw!("prune-unused-func-in-subquery";
    "(project ?outer_cols
       (filter ?pred
         (project [?cols.. (?f ?args) as ?unused] ?input)))" =>
    "(project ?outer_cols
       (filter ?pred
         (project [?cols..] ?input)))"
    if not_referenced_in("?unused", "?outer_cols")
    if not_referenced_in("?unused", "?pred")
),

// Remove function from view expansion
rw!("prune-unused-func-from-view";
    "(project [?needed..] (view ?name [?vcols.. (?f ?args) as ?extra]))" =>
    "(project [?needed..] (view ?name [?vcols..]))"
    if not_in_list("?extra", "?needed")
),
```

## Preconditions

- Function output must not be referenced by any ancestor operator
- Must trace through all references: SELECT, WHERE, GROUP BY,
  ORDER BY, HAVING, and join conditions
- Side-effecting functions must never be pruned

## Test Cases

```sql
-- Positive: outer query ignores function column
SELECT id, name FROM (
    SELECT id, name, MD5(document) AS hash
    FROM files
) sub;
-- Prune MD5(document): never referenced by outer SELECT

-- Positive: view with unused computed column
CREATE VIEW emp_v AS
    SELECT id, name, ENCRYPT(ssn) AS enc_ssn FROM emp;
SELECT id, name FROM emp_v;
-- Prune ENCRYPT(ssn): not in outer projection

-- Positive: CTE with unused expensive function
WITH enriched AS (
    SELECT id, REGEXP_REPLACE(body, '\\s+', ' ') AS clean
    FROM articles
)
SELECT id FROM enriched;
-- Prune REGEXP_REPLACE: clean column unused

-- Negative: function result used in WHERE
SELECT id FROM (
    SELECT id, LENGTH(name) AS name_len FROM users
) sub WHERE name_len > 10;
-- Cannot prune: name_len referenced in WHERE

-- Negative: function with side effects
SELECT id FROM (
    SELECT id, LOG_ACCESS(user_id) AS logged FROM resources
) sub;
-- Cannot prune: LOG_ACCESS has side effects
```

## References

- Calcite: ProjectRemoveRule (column pruning)
- functions.toml: pure property (side-effect-free functions)
- Dead code elimination in compiler optimization
