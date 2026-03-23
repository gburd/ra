# RFC 0022: Incremental View Maintenance

- Start Date: 2026-03-21
- Author: RA Contributors
- Status: Draft
- Tracking Issue: TBD

## Summary

Implement incremental view maintenance (IVM) to efficiently update materialized views when base tables change, avoiding full recomputation. This enables real-time analytics on frequently updated data by maintaining pre-computed aggregations and joins.

## Motivation

Materialized views accelerate query performance but become stale when underlying data changes. Full refresh is expensive for large views. IVM updates only the affected portions, enabling:
- Real-time dashboards with sub-second updates
- Continuous aggregations over streaming data
- Efficient maintenance of complex join results
- Reduced compute costs for view maintenance

## Guide-level explanation

Users define incrementally maintained views with a new clause:

```sql
CREATE MATERIALIZED VIEW sales_summary
WITH (incremental = true) AS
SELECT
    product_id,
    date_trunc('day', order_date) as day,
    SUM(quantity) as total_quantity,
    SUM(amount) as total_amount
FROM orders
GROUP BY product_id, date_trunc('day', order_date);
```

The optimizer automatically:
1. Captures changes to base tables
2. Computes deltas for the view
3. Merges deltas into the materialized result
4. Maintains consistency across transactions

## Reference-level explanation

### Delta Computation

For each DML operation on base tables:
- **INSERT**: Compute view deltas from new rows
- **DELETE**: Compute inverse deltas from deleted rows
- **UPDATE**: Treat as DELETE + INSERT

### Supported View Types

Phase 1:
- Single-table aggregations (SUM, COUNT, AVG, MIN, MAX)
- Simple projections and filters
- Inner joins with foreign key relationships

Phase 2:
- Outer joins
- DISTINCT aggregations
- Window functions with PARTITION BY

Phase 3:
- Recursive CTEs
- Set operations (UNION, EXCEPT)
- User-defined aggregates

### Maintenance Strategies

1. **Eager**: Update view immediately in same transaction
2. **Deferred**: Batch updates and apply periodically
3. **Hybrid**: Eager for small changes, deferred for bulk operations

### Delta Storage

Deltas stored in auxiliary tables:
- `_view_name_delta_ins`: Insertions to apply
- `_view_name_delta_del`: Deletions to apply
- Periodic merge consolidates deltas into main view

## Drawbacks

- Storage overhead for delta tables
- Write amplification on base table updates
- Complex correctness proofs for all SQL constructs
- Not all views can be incrementally maintained
- Transaction overhead for eager maintenance

## Rationale and alternatives

### Why This Design?

- Proven approach in academic literature (DBToaster, Noria)
- Composable with existing optimizer infrastructure
- Graceful degradation to full refresh when needed

### Alternative Approaches

1. **Trigger-based maintenance**: More flexible but slower
2. **Log-based CDC**: Requires external infrastructure
3. **Approximate IVM**: Trading accuracy for performance

## Prior art

- **PostgreSQL**: pg_ivm extension (limited functionality)
- **Oracle**: Materialized view logs and fast refresh
- **SQL Server**: Indexed views with automatic maintenance
- **Materialize**: Streaming dataflow for incremental computation
- **DBToaster**: Generates specialized C++ code for IVM
- **Differential Dataflow**: Generalized incremental computation

## Unresolved questions

- Optimal delta merge frequency?
- How to handle schema evolution?
- Support for distributed views?
- Integration with query result caching?

## Future possibilities

- Incremental maintenance of recursive queries
- Multi-version concurrency for read-heavy workloads
- Automatic view selection based on workload
- Incremental maintenance across database restarts
- Federation with external streaming systems