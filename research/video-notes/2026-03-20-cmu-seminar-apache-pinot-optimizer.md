# CMU Seminar: Apache Pinot Query Optimizer

**Source:** https://db.cs.cmu.edu/seminar2025/ (SQL or Death? series)
**Date:** 2025-02-24
**Speaker:** Yash Mayya, Gonzalo Ortiz

## Key Points
- Apache Pinot is a real-time OLAP datastore for user-facing analytics
- Optimizer focuses on distributed query optimization
- Multi-stage query engine with cost-based optimization
- Segment-level pruning and indexing are key optimizations

## Optimization Techniques
- **Segment pruning**: skip data segments based on partition metadata
- **Index selection**: choose between sorted, inverted, range, and text indexes
- **Star-tree index**: pre-aggregated data for common query patterns
- **Multi-stage optimization**: break query into distributed stages
- **Predicate pushdown**: push filters to storage layer

## Applicable to RA
- Gap: No segment/partition pruning cost model
- Gap: No star-tree / pre-aggregated index modeling
- Gap: No multi-stage distributed query planning rules

## References
- Apache Pinot documentation
- Real-time analytics architecture patterns
