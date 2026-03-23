# Rule: Preaggregation

**Category:** physical/aggregation-strategies
**File:** `rules/physical/aggregation-strategies/preaggregation.rra`

## Metadata

- **ID:** `preaggregation`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, clickhouse, cockroachdb, mssql, oracle
- **Tags:** aggregation, optimization, early
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(aggregate (join ?type ?cond ?left ?right) ?groups ?aggs)"
    description: "Aggregation that can be pre-aggregated before join"
  - type: "predicate"
    condition: "all_decomposable(?aggs)"
    description: "Aggregate functions must be decomposable for pre-aggregation"
  - type: "predicate"
    condition: "references_only(?groups, ?left) || references_only(?groups, ?right)"
    description: "Grouping columns from one join side"
```


# Preaggregation

## Metadata
- **Rule ID**: `preaggregation`
- **Category**: Physical / Aggregation Strategies
- **Complexity**: O(n) with early reduction
- **Introduced**: Distributed systems (1990s)
- **Prerequisites**: Decomposable aggregates
- **Alternatives**: Direct aggregation

## Description

Preaggregation performs partial aggregation early in the query plan (before joins, before network transfer) to reduce data volume. Also called "partial push-down" or "early aggregation."

**When to use:**
- Before expensive joins
- Before network shuffle (distributed)
- Before sorting
- High reduction factor expected

**Advantages:**
- Reduces data volume early
- Less network traffic
- Smaller join input
- Better cache efficiency

**Disadvantages:**
- Extra CPU overhead if no reduction
- Memory pressure from maintaining partial state
- Not beneficial for high cardinality

## Relational Algebra

```
$\gamma$_{g; AGG(v)}(R $\bowtie$ S)
-> $\gamma$_{g; FINAL_AGG(p)}($\gamma$_{g; PARTIAL_AGG(v)}(R) $\bowtie$ S)
  :if R.g = join_key and reduction_factor > 0.5

Cost savings = |R| * (1 - reduction_factor) * join_cost
```

## Implementation

```rust
pub struct PreaggregationPushdown;

impl OptimizationRule for PreaggregationPushdown {
    fn apply(&self, plan: &LogicalPlan) -> Option<LogicalPlan> {
        match plan {
            // Pattern: Aggregate(Join(R, S))
            LogicalPlan::Aggregate { input, group_by, aggs } => {
                if let LogicalPlan::Join { left, right, .. } = input.as_ref() {
                    // Check if group_by columns from left side only
                    if self.can_pushdown(left, group_by) {
                        // Insert partial aggregation before join
                        let partial = LogicalPlan::PartialAggregate {
                            input: left.clone(),
                            group_by: group_by.clone(),
                            aggs: aggs.clone(),
                        };

                        return Some(LogicalPlan::Aggregate {
                            input: Box::new(LogicalPlan::Join {
                                left: Box::new(partial),
                                right: right.clone(),
                                ..
                            }),
                            group_by: group_by.clone(),
                            aggs: self.make_final_aggs(aggs),
                        });
                    }
                }
            }
            _ => {}
        }
        None
    }
}
```

## Cost Model

```rust
pub fn benefit_of_preaggregation(
    input_card: u64,
    group_card: u64,
    downstream_cost_per_row: f64,
) -> f64 {
    let reduction_factor = group_card as f64 / input_card as f64;

    let preagg_cost = input_card as f64 * 10.0; // Hash aggregation
    let savings = (input_card - group_card) as f64 * downstream_cost_per_row;

    savings - preagg_cost
}
```

## Test Cases

### Test 1: Aggregate before join
```sql
-- Without preaggregation:
-- 1M sales $\bowtie$ 100 products -> 1M rows -> aggregate -> 100 groups

-- With preaggregation:
-- 1M sales -> aggregate -> 100 groups $\bowtie$ 100 products -> 100 rows

SELECT p.name, SUM(s.amount)
FROM sales s JOIN products p ON s.product_id = p.id
GROUP BY p.name;

-- Reduction: 1M -> 100 (99.99% reduction)
```

### Test 2: Distributed query with network shuffle
```sql
-- Without: 1B rows shuffled across network
-- With: 1M groups shuffled (99.9% reduction)

SELECT user_id, COUNT(*)
FROM distributed_events
GROUP BY user_id;

-- Preaggregation on each node before network transfer
```

## References

1. **Presto/Trino**: Partial aggregation pushdown
2. **Spark**: Map-side combining before shuffle
3. **Cascades Optimizer**: Eager aggregation rules

## Tags
`physical`, `aggregation`, `pushdown`, `optimization`, `distributed`
