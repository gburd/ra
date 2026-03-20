# CMU Research: UDF Compilation Magic

**Source:** https://db.cs.cmu.edu/projects/udf/
**Date:** Ongoing research
**Speaker:** Andy Pavlo, Jignesh Patel

## Key Points
- UDFs are traditionally "black boxes" to the optimizer
- Strategic code transformation across SQL/UDF boundaries
- PRISM system: "outline before inline" approach
- Batching UDF invocations improves efficiency

## Techniques

### PRISM: Outline Before Inline
- Don't immediately inline UDFs into SQL
- First outline: extract and restructure procedural logic
- Expose optimization opportunities to SQL optimizer
- Then selectively inline optimizable portions

### UDF Batching
- Instead of per-row UDF invocation, batch multiple rows
- Reduces context switching overhead
- Enables compiler to optimize batched code
- Better vectorization and cache behavior

### Cross-Boundary Optimization
- SQL optimizer knows about set operations (joins, aggregations)
- UDF compiler knows about procedural code optimization
- Transfer information across boundary for holistic optimization
- Example: push filter from UDF into SQL scan

## Applicable to RA
- RA has limited function optimization (58 rules in logical/function-optimization/)
- Gap: No UDF inlining/outlining decision rules
- Gap: No UDF batching optimization rules
- Gap: No cross-boundary (SQL/UDF) optimization
- Gap: No UDF cost estimation (treats functions as black boxes)
- Gap: No function decomposition for partial pushdown

## References
- CMU UDF Compilation project
- Ramachandra et al. "Froid: Optimization of Imperative Programs in a Relational Database" (2017)
