# RFC 0025: Physical Property Tracking Framework

- Start Date: 2026-03-21
- Author: RA Contributors
- Status: Implemented (MVP, 2026-05-29) — see INDEX.md for scope boundaries
- Tracking Issue: TBD

## Summary

Extend the e-graph extraction phase with a physical property tracking layer that propagates sort ordering, data partitioning, and data distribution requirements through plan nodes, enabling the optimizer to reason about when sorts and exchanges are redundant.

## Motivation

RA's e-graph optimizer does not track physical properties (sort ordering, data partitioning, data distribution) through plan nodes. This means the optimizer cannot reason about when a sort is redundant, when a merge join is free because data is already sorted, or when an exchange operator is needed in distributed plans.

Without physical property tracking:
- Redundant sorts are inserted after merge joins on the same key
- Interesting orderings from indexes are not exploited
- Distributed query planning lacks correctness guarantees for data placement
- GROUP BY and DISTINCT cannot be reordered to match available orderings

## Guide-level explanation

Physical properties describe characteristics of the data flowing through a plan node:

```sql
-- Index on orders(customer_id, order_date)
-- The optimizer knows the scan produces data sorted on (customer_id, order_date)
SELECT customer_id, SUM(total)
FROM orders
GROUP BY customer_id;
-- No extra sort needed: index already provides the required ordering
```

The optimizer tracks three kinds of physical properties:
1. **Ordering**: which columns the data is sorted on, and in which direction
2. **Partitioning**: how data is distributed across partitions (hash, range, round-robin)
3. **Distribution**: whether data is replicated, partitioned, or on a single node

## Reference-level explanation

### Implementation Details

```rust
pub trait PhysicalProperties {
    fn ordering(&self) -> &[OrderColumn];
    fn partitioning(&self) -> &Partitioning;
    fn distribution(&self) -> &Distribution;
}

pub struct OrderColumn {
    pub column: ColumnRef,
    pub direction: SortDirection,
    pub nulls_first: bool,
}

pub enum Partitioning {
    Hash { columns: Vec<ColumnRef>, buckets: usize },
    Range { column: ColumnRef, boundaries: Vec<Value> },
    RoundRobin,
    Single,
}

pub enum Distribution {
    Replicated,
    Partitioned,
    Singleton,
}
```

Each physical operator declares:
- Required input properties (what ordering/partitioning the child must provide)
- Provided output properties (what ordering/partitioning this operator produces)
- Enforcer cost (cost to add Sort/Exchange if the child does not satisfy requirements)

During extraction, property requirements propagate top-down while available properties propagate bottom-up. Enforcers (Sort, Exchange) are inserted only when needed.

### Integration Points

- E-graph extraction phase: property-aware cost comparison
- Cost model: enforcer insertion costs
- Merge join selection: requires sorted inputs
- GROUP BY optimization: key reordering based on available ordering
- Distributed planning: exchange operator placement

### Performance Considerations

- Property propagation adds O(n) overhead per plan node during extraction
- Enforcer cost computation is constant time per operator
- Net effect is reduced plan cost by eliminating unnecessary sorts

## Drawbacks

- Increases complexity of the extraction phase
- Property propagation must be correct for all operator combinations
- Partial ordering matches require careful prefix-matching logic

## Rationale and alternatives

### Why This Design?

The Cascades/Volcano approach of separating logical and physical optimization is well-proven. Physical properties are the standard mechanism for this separation in production optimizers.

### Alternative Approaches

- **Post-optimization sort elimination**: Simpler but misses opportunities during planning
- **Rule-based sort removal**: Fragile and incomplete coverage
- **Ignore physical properties**: Current state; leaves performance on the table

## Prior art

- Graefe, "The Cascades Framework for Query Optimization" (1995)
- Selinger et al., "Access Path Selection" (1979) -- interesting orderings
- PostgreSQL pathkeys system
- Apache Calcite trait system for physical properties
- CockroachDB required/provided physical properties

## Unresolved questions

- How to represent functional dependencies that affect ordering?
- Interaction with e-graph equality: do equivalent e-nodes share properties?
- Granularity of partitioning representation for distributed queries

## Future possibilities

- Interesting orderings for merge join selection
- Automatic exchange operator placement in distributed mode
- Property-aware materialized view matching
