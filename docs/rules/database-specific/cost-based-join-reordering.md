# Rule: Cost-Based Join Reordering (Presto)

**Category:** database-specific/presto
**File:** `rules/database-specific/presto/cost-based-join-reordering.rra`

## Metadata

- **ID:** `presto-cost-based-join-reordering`
- **Version:** "1.0.0"
- **Databases:** presto
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Cost-Based Join Reordering (Presto)

## Metadata
- **Rule ID**: `presto-cost-based-join-reordering`
- **Category**: Database-Specific / Presto
- **Source**: Presto/Trino CostBasedOptimizer
- **Complexity**: O(n\!) bounded by heuristics

## Description

Presto uses table statistics and cost estimates to reorder joins, choosing the optimal join tree using dynamic programming with pruning.

## Relational Algebra

```
(R ⋈ S) ⋈ T
→ (R ⋈ T) ⋈ S  if cost((R ⋈ T) ⋈ S) < cost((R ⋈ S) ⋈ T)
```

## References
1. **Presto Source**: CostBasedOptimizer.java

## Tags
`database-specific`, `presto`, `join-reordering`, `cost-based`, `statistics`
