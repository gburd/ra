# CMU 15-721 Lecture 20: System Analysis - DuckDB

**Source:** CMU 15-721 Spring 2024, Lecture 20
**Speaker:** Andy Pavlo (with guest from DuckDB)
**Topic:** DuckDB Architecture and Optimization

## Key Concepts

### DuckDB Optimizer Architecture
- Rule-based + cost-based hybrid optimizer
- Heuristic rules applied first (always beneficial)
- Cost-based optimization for join ordering and access path selection
- Custom optimizer framework (not Cascades or System R)
- Focus: analytical workloads on single-node systems

### Key Optimization Rules in DuckDB
1. **Filter pushdown**: Push predicates below joins and aggregates
2. **Projection pushdown**: Eliminate unused columns early
3. **Common subexpression elimination**: Compute shared expressions once
4. **Cross join elimination**: Convert CROSS JOIN to INNER JOIN when predicate exists
5. **Outer join elimination**: LEFT/RIGHT JOIN -> INNER when WHERE rejects nulls
6. **Self-join elimination**: Detect and remove unnecessary self-joins
7. **Empty result propagation**: Short-circuit when any input is known empty
8. **Constant folding**: Evaluate constant expressions at plan time
9. **Top-N optimization**: ORDER BY + LIMIT -> heap-based top-N sort
10. **Aggregate pushdown**: Push aggregates below joins when valid

### External Aggregation (Unique to DuckDB)
- Two-phase approach for out-of-memory aggregation:
  - Phase 1: Thread-local pre-aggregation with small hash tables
  - Phase 2: Partition-wise aggregation after exchange
- Over-partitioning: create more partitions than threads
- Unified buffer manager handles spill-to-disk transparently
- Pointer recomputation avoids serialization overhead

### Vectorized Execution
- Process data in vectors (1024-2048 tuples)
- SIMD for predicate evaluation and hash computation
- Morsel-driven parallelism for work distribution
- Pipeline-based execution model

### Compression-Aware Processing
- Operate on compressed data where possible
- Dictionary compression: filter on dictionary codes, not values
- Constant compression: entire column is same value -> skip scan
- Run-length encoding: aggregate over runs efficiently
- Delta encoding: range predicates on deltas

## Applicable to Ra

### New Rule Ideas
1. **Empty Result Propagation**: When any input to a join/union is provably
   empty (e.g., WHERE false, or contradictory predicates), propagate empty
   result upward, eliminating unnecessary computation.
2. **Top-N Sort Selection**: When ORDER BY + LIMIT detected, rewrite to
   heap-based top-N sort (O(n log k) instead of O(n log n)).
3. **Dictionary-Aware Filtering**: For dictionary-compressed columns, evaluate
   filter on dictionary, then apply bitmap to column.
4. **Constant Column Elimination**: When column has single distinct value,
   replace with constant in expressions.
5. **Two-Phase Aggregation with Spill**: Model cost of external aggregation
   when hash table exceeds memory budget.
6. **Cross Join to Inner Conversion**: Detect implicit join predicates in
   WHERE clause and convert CROSS to INNER JOIN.

### Gap Analysis
- Ra has some of these (predicate pushdown, projection pushdown)
- Missing: empty result propagation
- Missing: top-N sort as distinct physical operator
- Missing: compression-aware query processing rules
- Missing: external aggregation cost modeling
