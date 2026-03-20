# CMU 15-721 Lecture 8: Vectorized Execution

**Source:** https://15721.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- Vectorized execution processes batches of tuples (vectors) instead of one at a time
- Eliminates per-tuple virtual function call overhead of Volcano model
- Enables SIMD, cache-friendly access, and compiler auto-vectorization
- DuckDB and Velox are leading implementations

## Vectorized Execution Techniques

### Vector Size Selection
- Typical vector sizes: 1024 or 2048 tuples
- Must fit in L1/L2 cache for best performance
- Too small: amortization insufficient; too large: cache pollution
- Some systems use adaptive vector sizing

### Selection Vectors
- Bitmap or list of valid tuple indices within a vector
- Avoid copying/compacting data after selection
- Pass selection vector to subsequent operators
- Enables branch-free selection with SIMD

### Late Materialization
- Keep data in columnar format as long as possible
- Only materialize rows when needed (e.g., for output)
- Reduces memory bandwidth for selective queries
- Critical for column stores

### SIMD Integration
- Process multiple values per CPU instruction
- Filter: compare 4/8/16 values simultaneously
- Aggregate: parallel summation
- Hash: compute multiple hash values at once
- Gather/scatter for non-contiguous access patterns

## Applicable to RA
- RA has execution-models/vectorized/ (12 rules)
- Gap: No vector size selection rules based on cache hierarchy
- Gap: No selection vector propagation rules
- Gap: No late materialization optimization rules
- Gap: No SIMD-specific operator selection rules
- Gap: No adaptive vector sizing rules

## References
- Boncz, Zukowski, Nes. "MonetDB/X100: Hyper-Pipelining Query Execution" (2005)
- Kersten et al. "Everything You Always Wanted to Know About Compiled and Vectorized Queries But Were Afraid to Ask" (2018)
