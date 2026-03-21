# CMU 15-445 Lecture 13-14: Query Execution Models

**Source:** CMU 15-445 Fall 2024, Lectures 13-14
**Speaker:** Andy Pavlo
**Topic:** Query Execution I & II

## Key Concepts

### Execution Models

#### Iterator (Volcano) Model
- Each operator implements `Open()`, `Next()`, `Close()`
- Tuple-at-a-time processing
- Natural pipelining: no materialization between operators
- Drawback: high per-tuple overhead (function calls)
- Used by: PostgreSQL, MySQL, SQLite

#### Materialization Model
- Each operator processes entire input, produces entire output
- Better for OLTP (small results)
- Worse for OLAP (large intermediate results)
- Natural for bottom-up execution

#### Vectorized Model
- Like iterator but `Next()` returns a batch of tuples
- Amortizes function call overhead across batch
- Enables SIMD for predicate evaluation
- Used by: DuckDB, Velox, DataFusion

### Pipeline Execution

#### Pipeline Breakers
Operators that must consume all input before producing output:
- **Sort**: Must see all tuples before first output
- **Hash join build**: Must build hash table before probing
- **Aggregate (hash)**: Must process all groups
- **Window function**: Must see partition before computing

Non-breakers (pipelined operators):
- **Filter**: Evaluate and pass through immediately
- **Project**: Compute and pass through immediately
- **Scan**: Read and emit immediately
- **Hash join probe**: Once table built, probe immediately
- **Limit**: Pass through up to N tuples

#### Pipeline Fusion
Combine adjacent pipelined operators into single compiled function:
- Filter + Scan -> fused predicate evaluation during scan
- Project + Filter -> fused projection and filtering
- Reduces function call overhead
- Enables register-level data passing

### Parallel Execution

#### Inter-Operator Parallelism
- Different operators run on different threads
- Limited by pipeline depth and operator dependencies

#### Intra-Operator Parallelism
- Single operator parallelized across threads
- **Parallel scan**: Divide table pages among workers
- **Parallel hash join**: Build shared hash table, parallel probe
- **Parallel aggregate**: Per-thread local aggregation, then combine
- **Parallel sort**: Per-thread sort, then merge

#### Exchange Operator
- Distributes/collects tuples between parallel pipelines
- Types: Gather, Redistribute (hash), Broadcast, Round-Robin
- Placed by optimizer between pipeline stages

### Worker Allocation
- Static: fixed workers per query (PostgreSQL)
- Dynamic: morsel-driven (worker steals work units) (DuckDB, Umbra)
- Adaptive: adjust workers based on system load

## Applicable to Ra

### New Rule Ideas
1. **Pipeline Breaker Analysis**: Annotate plan nodes as pipeline breakers
   or pipelined. Use for startup cost estimation.
2. **Pipeline Fusion**: Merge adjacent filter + scan or project + filter
   into fused operators for compilation.
3. **Exchange Placement**: Insert exchange operators at pipeline boundaries
   for parallel execution.
4. **Worker Allocation Rule**: Choose number of parallel workers based on
   table size, system load, and available cores.
5. **Parallel Sort Strategy**: Choose between parallel sort-merge and
   parallel partitioned sort based on data distribution.
6. **Vectorized vs Compiled Selection**: Choose vectorized execution for
   complex expressions, compiled for simple filters.

### Gap Analysis
- Ra has parallel operators (ParallelScan, ParallelHashJoin, etc.)
- Ra has Gather operator for collecting parallel results
- Missing: pipeline breaker annotation
- Missing: pipeline fusion rules
- Missing: exchange operator placement optimization
- Missing: adaptive worker allocation rules
