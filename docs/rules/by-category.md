# Rules by Category

Hierarchical organization of all 1,354 optimization rules.

## Logical Optimizations (352 rules)

Transform query structure while preserving semantics.

### Predicate Pushdown (27 rules)
Push filters closer to data sources to reduce intermediate results.
- Filter through join
- Filter through aggregate
- Filter through union
- Filter into subquery
- Partition pruning

### Join Optimization (41 rules)
- **Join Elimination** (25 rules) - Remove unnecessary joins
- **Join Reordering** (16 rules) - Find optimal join order

### Subquery Optimization (26 rules)
Transform subqueries into more efficient forms.
- EXISTS to semi-join
- IN to semi-join
- Scalar subquery unnesting
- Correlated subquery decorrelation

### Aggregate Optimization (28 rules)
- Aggregate pushdown through join
- Aggregate elimination
- Distinct optimization
- Group-by simplification

### Function Optimization (58 rules)
- Function inlining
- Constant folding
- Expression simplification
- Dead code elimination

### Other Logical (172 rules)
- CTE optimization (22 rules)
- Projection pushdown (19 rules)
- Expression simplification (17 rules)
- Limit pushdown (16 rules)
- Semantic rewriting (16 rules)
- Set operations (14 rules)
- Window functions (13 rules)
- Distinct elimination (11 rules)
- View rewriting (10 rules)
- Sideways information passing (10 rules)

## Physical Optimizations (145 rules)

Select algorithms and access methods.

### Join Algorithms (31 rules)
- Hash join variants
- Sort-merge join
- Nested loop join
- Index nested loop join
- Adaptive join selection

### Access Path Selection (28 rules)
- Index scan vs sequential scan
- Index selection
- Multi-index strategies
- Covering index usage

### Aggregation Strategies (22 rules)
- Hash aggregation
- Sort aggregation
- Streaming aggregation
- Partial aggregation

### Sort Optimization (18 rules)
- Sort elimination
- Sort reuse
- Top-K optimization
- External sorting

### Parallelization (15 rules)
- Intra-operator parallelism
- Inter-operator parallelism
- Partition-wise operations

### Other Physical (31 rules)
- Materialization strategies
- Pipelining decisions
- Memory management

## Database-Specific (403 rules)

System-specific optimizations for 33+ database systems.

### Major Systems
- PostgreSQL (58 rules)
- MySQL (45 rules)
- Oracle (52 rules)
- SQL Server (48 rules)
- DuckDB (38 rules)
- ClickHouse (35 rules)
- CockroachDB (28 rules)
- MongoDB (25 rules)

### Frameworks
- Apache Calcite (42 rules)
- Apache Spark (18 rules)
- Apache Flink (14 rules)

## Distributed Optimizations (118 rules)

### Data Movement (22 rules)
- Shuffle optimization
- Broadcast vs partition
- Co-location strategies

### Distributed Joins (18 rules)
- Broadcast join
- Shuffle join
- Co-located join
- Semi-join reduction

### Distributed Aggregation (15 rules)
- Two-phase aggregation
- Partial aggregation pushdown
- Combiner optimization

### Other Distributed (63 rules)
- Partition pruning
- Exchange placement
- Stage planning
- Locality optimization

## Execution Models (124 rules)

Different paradigms for query execution.

### Vectorized Execution (28 rules)
- Batch processing
- SIMD optimization
- Cache-conscious algorithms

### Adaptive Execution (22 rules)
- Runtime re-optimization
- Adaptive join selection
- Memory grant feedback

### Compiled Execution (18 rules)
- Query compilation
- JIT optimization
- Code generation

### Streaming Execution (15 rules)
- Incremental computation
- Window processing
- Event-time handling

### Other Models (41 rules)
- Pipeline execution
- Morsel-driven execution
- Push-based execution
- Volcano model

## Cost Models (50 rules)

### Cardinality Estimation (15 rules)
- Histogram-based
- Sampling-based
- Learning-based
- Correlation-aware

### Cost Formulas (12 rules)
- I/O cost
- CPU cost
- Network cost
- Memory cost

### Selectivity Estimation (8 rules)
- Predicate selectivity
- Join selectivity
- Correlation detection

### System-R Model (11 rules)
- Classic cost formulas
- Independence assumptions
- Catalog statistics

## Experimental (56 rules)

Cutting-edge research techniques.

### Machine Learning (12 rules)
- Learned cardinality estimation
- Learned join ordering
- Learned index selection

### Approximate Query Processing (10 rules)
- Sampling techniques
- Sketch-based aggregation
- Online aggregation

### Hardware Acceleration (8 rules)
- GPU query processing
- FPGA acceleration
- Persistent memory

### Other Experimental (26 rules)
- Worst-case optimal joins
- Differential dataflow
- Semantic optimization

## Multi-Model (30 rules)

### Document Stores (10 rules)
- Document shredding
- Path indexes
- Schema inference

### Graph Databases (8 rules)
- Graph traversal optimization
- Pattern matching
- Reachability queries

### Time-Series (7 rules)
- Time-based partitioning
- Downsampling
- Gap filling

### Other Multi-Model (5 rules)
- Polyglot persistence
- Cross-model optimization

## Hardware Optimizations (21 rules)

### GPU Acceleration (8 rules)
- Kernel selection
- Memory coalescing
- Warp divergence

### NUMA Awareness (5 rules)
- Socket-local execution
- Memory placement
- Cache optimization

### Storage Optimization (8 rules)
- SSD-aware algorithms
- Persistent memory
- Tiered storage

## Other Categories

### Federated Queries (24 rules)
- Source selection
- Capability matching
- Cost estimation

### RPR - Robust Plan Reuse (19 rules)
- Plan caching
- Parametric optimization
- Plan stability

### Unnest Operations (5 rules)
- Array unnesting
- Lateral unnesting
- Multi-unnest

### Templates (4 rules)
- Rule patterns
- Generic transformations

### Parallel Execution (3 rules)
- Parallel operators
- Synchronization
- Load balancing