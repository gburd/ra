# CMU Seminar: Apache DataFusion Query Engine

**Source:** https://db.cs.cmu.edu/seminar2024/ (Database Building Blocks series)
**Date:** 2024-09-23
**Speaker:** Andrew Lamb

## Key Points
- DataFusion is a modular, embeddable analytic query engine in Rust
- Built on Apache Arrow for columnar in-memory processing
- Extensible optimizer with user-defined rules
- Used as foundation by many systems (InfluxDB, Ballista, etc.)

## Optimization Techniques
- **Rule-based optimizer**: ordered list of optimization passes
- **Predicate pushdown**: through projections, aggregations, joins
- **Projection pushdown**: eliminate unused columns
- **Common subexpression elimination**: reuse repeated expressions
- **Filter pushdown through joins**: push WHERE into join sides
- **Limit pushdown**: push LIMIT through sorts and projections
- **Join reordering**: heuristic-based for simple cases
- **Type coercion**: automatic type casting optimization
- **Simplification**: constant folding, expression simplification
- **Subquery decorrelation**: convert correlated subqueries to joins

## Architecture
- **LogicalPlan**: tree of logical operators
- **OptimizerRule trait**: extensible optimization rules
- **PhysicalPlanner**: converts logical to physical plan
- **ExecutionPlan trait**: actual execution operators

## Applicable to RA
- RA already has 20 DataFusion-specific rules
- Gap: No common subexpression elimination rules
- Gap: No type coercion optimization rules
- Gap: Limited subquery decorrelation (17 rules but may lack advanced cases)
- Gap: No extensible rule API for user-defined optimizations

## References
- Apache DataFusion documentation
- Arrow Columnar Format specification
