# CMU 15-445 Lectures 12-13: Query Execution I & II

**Source:** https://15445.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- Execution model determines how the plan tree is evaluated
- Three main models: iterator (Volcano), materialization, vectorized
- Parallel execution adds exchange operators and partitioning

## Execution Models

### Iterator / Volcano Model
- Each operator implements Open(), Next(), Close()
- Tuples flow upward one at a time via Next() calls
- Advantages: simple, pipelined, low memory
- Disadvantages: virtual function overhead per tuple, cache unfriendly

### Materialization Model
- Each operator processes entire input, produces entire output
- Advantages: can optimize per-operator, avoids iterator overhead
- Disadvantages: high memory usage, no pipelining

### Vectorized / Batch Model
- Like iterator but passes batches of tuples (vectors)
- Advantages: amortizes iterator overhead, SIMD-friendly, cache-friendly
- Best balance of pipelining and per-batch optimization
- Used by DuckDB, Velox, DataFusion, ClickHouse

## Parallel Execution
- Inter-operator: pipeline parallelism (different operators on different threads)
- Intra-operator: same operator processes different data partitions
- Exchange operators: gather, redistribute, broadcast
- Morsel-driven: divide work into morsels assigned to threads dynamically

## Applicable to RA
- RA has execution-models/ (99 rules) covering volcano, vectorized, push-based, morsel-driven
- Gap: No detailed exchange operator placement optimization rules
- Gap: Limited rules for choosing between execution models based on query shape
- Gap: No adaptive batch size tuning for vectorized execution
- Gap: No pipeline breaker analysis rules

## References
- Graefe. "Volcano - An Extensible and Parallel Query Evaluation System" (1994)
- Neumann. "Efficiently Compiling Efficient Query Plans for Modern Hardware" (2011)
- Leis et al. "Morsel-Driven Parallelism" (2014)
