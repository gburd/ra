# Rule: Index Join Optimizer (Trino)

**Category:** database-specific/trino
**File:** `rules/database-specific/trino/index-join-optimizer.rra`

## Metadata

- **ID:** `trino-index-join-optimizer`
- **Version:** "1.0.0"
- **Databases:** trino
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Index Join Optimizer (Trino)

## Metadata
- **Rule ID**: `trino-index-join-optimizer`
- **Category**: Database-Specific / Trino
- **Complexity**: O(n log m) with index
- **Source**: Trino IndexJoinOptimizer.java
- **GitHub**: https://github.com/trinodb/trino/blob/master/core/trino-main/src/main/java/io/trino/sql/planner/optimizations/IndexJoinOptimizer.java

## Description

Trino converts cross joins with post-filters into index joins when connector supports index lookups. Leverages connector-specific indexes.

**Key features:**
- Cross join + filter -> Index join
- Connector-agnostic (uses IndexSource interface)
- Supports multi-column indexes
- Cost-based decision

## Relational Algebra

```
$\sigma$_$\theta$(R $\times$ S) -> R $\bowtie$_{index} S
  where $\theta$ = equality predicate on indexed columns
```

## Implementation Pattern

```java
@Override
public PlanNode visitJoin(JoinNode node, RewriteContext<Void> context) {
    if (node.getType() \!= INNER) {
        return node;
    }

    // Check if join is actually a cross join with filter
    if (\!node.getCriteria().isEmpty()) {
        return node;
    }

    // Extract equality predicates from filter
    List<Expression> conjuncts = extractConjuncts(node.getFilter().orElse(TRUE));
    List<EquiJoinClause> joinClauses = extractJoinClauses(conjuncts);

    // Check if connector supports index
    if (joinClauses.isEmpty() || \!supportsIndexJoin(node.getRight())) {
        return node;
    }

    // Convert to index join
    return new IndexJoinNode(
        node.getId(),
        IndexJoinNode.Type.INNER,
        node.getLeft(),
        node.getRight(),
        joinClauses,
        Optional.empty());
}
```

## Test Cases

### Test 1: Cross join to index join
```sql
-- Cross join with filter
SELECT *
FROM small_table s, large_indexed_table l
WHERE s.id = l.foreign_key;

-- Converted to index join using l.foreign_key index
```

## References

1. **Trino Source**: IndexJoinOptimizer.java

## Tags
`database-specific`, `trino`, `index`, `join`, `connector-aware`
