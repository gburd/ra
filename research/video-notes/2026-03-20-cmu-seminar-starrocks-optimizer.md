# CMU Seminar: StarRocks Query Optimizer

**Source:** https://db.cs.cmu.edu/seminar2025/ (SQL or Death? series)
**Date:** 2025-03-31
**Speaker:** Kaisen Kang

## Key Points
- StarRocks uses a Cascades-based query optimizer
- CBO with column-level statistics and multi-column statistics
- Supports both OLAP and real-time analytics
- Adaptive execution with runtime filter generation

## Optimization Techniques
- **Cascades framework**: top-down rule-based optimization
- **Statistics-driven**: column histograms, NDV, null count
- **Runtime filters**: bloom filters generated during hash join build
  phase, pushed to scan operators
- **Materialized view rewriting**: transparent query rewriting to MVs
- **Cost-based join reordering**: handles star and snowflake schemas
- **Adaptive MPP**: adjust parallelism based on data distribution

## Applicable to RA
- Gap: No runtime filter generation rules (bloom filters from join builds)
- Gap: No materialized view rewriting / matching rules
- Gap: No star/snowflake schema-specific join ordering
- Gap: Limited Cascades-style optimization

## References
- StarRocks documentation
- Graefe. "The Cascades Framework" (1995)
