# Rule: Audit Trail Predicate Injection

**Category:** logical/security
**File:** `rules/logical/security/audit-predicate-injection.rra`

## Metadata

- **ID:** `audit-predicate-injection`
- **Version:** "1.0.0"
- **Databases:** oracle, postgresql, mssql
- **Tags:** logical, security, audit, fine-grained, vpd, predicate-injection
- **Authors:** "Oracle Corporation"


# Audit Trail Predicate Injection

## Description

Automatically injects audit-related predicates or side-effect functions
into query plans for fine-grained auditing (FGA) and Virtual Private
Database (VPD) policies. The optimizer must place audit hooks at the
correct position in the plan to capture all accessed rows while
minimizing performance overhead.

**When to apply**: Tables with fine-grained audit policies or VPD
policies that require per-row access tracking or filtering.

## Relational Algebra

```algebra
-- Before: plain query on audited table
pi[name, salary](sigma[dept = 'HR'](employees))

-- After: VPD predicate + audit hook injected
pi[name, salary](
    audit_hook(
        sigma[dept = 'HR' AND vpd_policy(current_user)](employees)
    )
)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("vpd-predicate-injection";
    "(tablescan ?table)" =>
    "(filter (vpd-policy ?table) (tablescan ?table))"
    if has_vpd_policy("?table")
),

rw!("fga-audit-injection";
    "(tablescan ?table)" =>
    "(audit-hook ?table (tablescan ?table))"
    if has_fga_policy("?table")
),
```

## Preconditions

```rust
fn applicable(table: &Table, catalog: &Catalog) -> bool {
    catalog.has_vpd_policy(table)
        || catalog.has_fga_policy(table)
}
```

**Restrictions:**
- VPD predicates must be injected before any other optimization
- Audit hooks must not be reordered past security barriers
- Policy functions must be deterministic within a statement

## Cost Model

```rust
fn estimated_benefit(rows: f64, policy_eval_cost: f64) -> f64 {
    // This is a correctness transformation, not optimization
    // Cost is the overhead of policy evaluation
    -(rows * policy_eval_cost)
}
```

**Typical benefit**: Negative (adds overhead), but required for compliance.

## Test Cases

```sql
-- Positive: VPD policy restricts access
-- Policy function returns: dept_id = get_user_dept()
SELECT * FROM employees;
-- Becomes: SELECT * FROM employees WHERE dept_id = get_user_dept()

-- Positive: FGA tracks access to salary column
SELECT name, salary FROM employees WHERE dept = 'Engineering';
-- Audit record generated for each row accessed
```

## References

- Oracle Virtual Private Database (VPD) documentation
- Oracle Fine-Grained Auditing (FGA) documentation
