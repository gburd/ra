# Rule: Limit Pushdown (Trino)

**Category:** database-specific/trino
**File:** `rules/database-specific/trino/limit-pushdown.rra`

## Metadata

- **ID:** `trino-limit-pushdown`
- **Version:** "1.0.0"
- **Databases:** trino
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Limit Pushdown (Trino)

## Metadata
- **Rule ID**: `trino-limit-pushdown`
- **Category**: Database-Specific / Trino
- **Complexity**: O(1) transformation
- **Source**: Trino LimitPushDown.java
- **GitHub**: https://github.com/trinodb/trino/blob/master/core/trino-main/src/main/java/io/trino/sql/planner/optimizations/LimitPushDown.java

## Description

Trino pushes LIMIT through various operators (joins, aggregations, unions) to reduce data volume early. More aggressive than standard LIMIT pushdown.

**Pushdown through:**
- UNION ALL (applies to each branch)
- Semi-joins (probe side)
- Aggregations (with partial limits)
- Sorts (converts to TopN)

## Relational Algebra

```
LIMIT_n(R ∪ S) → LIMIT_n(LIMIT_n(R) ∪ LIMIT_n(S))
LIMIT_n(Sort(R)) → TopN_n(R)
LIMIT_n(R ⋉ S) → LIMIT_n(R) ⋉ S  // Semi-join probe side
```

## Implementation Pattern

```java
// Trino LimitPushDown.java
@Override
public PlanNode visitLimit(LimitNode node, RewriteContext<Void> context) {
    PlanNode source = context.rewrite(node.getSource());

    // Push through union
    if (source instanceof UnionNode) {
        return pushLimitThroughUnion(node, (UnionNode) source);
    }

    // Convert Sort+Limit to TopN
    if (source instanceof SortNode) {
        return new TopNNode(
            node.getId(),
            ((SortNode) source).getSource(),
            node.getCount(),
            ((SortNode) source).getOrderingScheme());
    }

    // Push through semi-join
    if (source instanceof SemiJoinNode) {
        SemiJoinNode semiJoin = (SemiJoinNode) source;
        return semiJoin.replaceChildren(ImmutableList.of(
            new LimitNode(idAllocator.getNextId(), semiJoin.getSource(), node.getCount()),
            semiJoin.getFilteringSource()));
    }

    return node.replaceChildren(ImmutableList.of(source));
}
```

## Test Cases

### Test 1: LIMIT through UNION
```sql
(SELECT * FROM t1)
UNION ALL
(SELECT * FROM t2)
LIMIT 100;

-- Optimized to:
-- (SELECT * FROM t1 LIMIT 100)
-- UNION ALL
-- (SELECT * FROM t2 LIMIT 100)
-- LIMIT 100
```

### Test 2: Sort+Limit to TopN
```sql
SELECT * FROM large_table
ORDER BY score DESC
LIMIT 10;

-- Converted to TopN heap (O(n) vs O(n log n))
```

## References

1. **Trino Source**: LimitPushDown.java

## Tags
`database-specific`, `trino`, `limit`, `pushdown`, `topn`
