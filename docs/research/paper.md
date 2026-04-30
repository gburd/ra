# RA: A Literate, Equality-Saturating System for Database Query Optimization

**Draft -- March 2026**

## Abstract

We present RA, an open-source system that codifies database query
optimization knowledge into a unified, formally specified repository.
RA represents 147 transformation rules spanning logical, physical,
hardware-accelerated, distributed, and multi-model optimizations as
literate documents that combine formal relational algebra, executable
egg rewrite rules, cost models, and test cases. The system uses
equality saturation via e-graphs to explore the full space of
equivalent query plans, avoiding the phase-ordering problem inherent
in traditional rule-based optimizers. We describe the architecture,
rule representation format, hardware-aware cost models, and
cross-database validation methodology. Preliminary evaluation shows
that RA's rule set covers the major optimizations implemented across
six production database systems while providing a single composable
framework for experimentation and education.

## 1. Introduction

Query optimization is one of the most studied problems in database
systems, with a lineage stretching from System R's dynamic
programming approach [Selinger et al. 1979] through Volcano/Cascades
[Graefe 1993, 1995] to modern equality saturation [Willsey et al.
2021]. Despite decades of research, optimization knowledge remains
fragmented across database codebases, academic papers, and tribal
knowledge. A rule implemented in PostgreSQL may be absent from MySQL;
an optimization described in a 1990s paper may have no public
implementation.

RA addresses this fragmentation by providing:

1. **A literate rule format** (.rra) that combines formal algebra,
   implementation, cost models, and test cases in a single document.
2. **Equality saturation** via the egg library for exhaustive plan
   space exploration without phase ordering.
3. **Hardware-aware cost models** that account for GPU, FPGA, SIMD,
   and NUMA characteristics.
4. **Cross-database validation** against PostgreSQL, MySQL, SQLite,
   DuckDB, Oracle, and MSSQL.
5. **An interactive web explorer** for visualization and education.

### 1.1 Contributions

- A literate programming format for query transformation rules with
  embedded formal specifications, implementations, and tests.
- A catalog of 147 rules extracted from production databases and
  academic literature, organized into five categories.
- Hardware-aware cost models for heterogeneous CPU/GPU/FPGA systems.
- An interactive platform for exploring optimization strategies across
  database dialects.

## 2. Background

### 2.1 Equality Saturation

Traditional optimizers apply rules in a fixed order, risking local
optima. Equality saturation [Tate et al. 2009, Willsey et al. 2021]
avoids this by maintaining an e-graph -- a data structure that
compactly represents equivalence classes of expressions. Rules are
applied exhaustively until saturation (no new expressions) or a
budget is exceeded, then cost-based extraction selects the optimal
plan.

The egg library [Willsey et al. 2021] provides an efficient Rust
implementation of e-graphs with support for conditional rewrites
and analysis passes.

### 2.2 Query Optimization Taxonomy

We organize optimizations into five categories:

| Category     | Count | Examples                                |
|--------------|-------|-----------------------------------------|
| Logical      | 20    | Predicate pushdown, join reordering     |
| Hardware     | 21    | GPU scan, FPGA filter, SIMD vectorize   |
| Distributed  | 36    | Broadcast join, partition pruning        |
| Multi-model  | 30    | Graph traversal, time-series pruning    |
| Physical     | 40    | Join algorithms, index selection        |

### 2.3 Hardware Heterogeneity

Modern query processing must consider heterogeneous hardware. GPU
databases (HeavyDB, BlazingSQL, PG-Strom) offload compute-intensive
operations; FPGA appliances (Netezza, Alveo) accelerate streaming
filters; CPU features (AVX-512, NUMA topology) affect scan and join
performance. A unified cost model must weigh compute throughput
against data transfer overhead.

## 3. System Architecture

### 3.1 Overview

RA consists of 16 Rust crates organized in layers:

```
Applications: ra-cli
Optimization: ra-engine (egg), ra-codegen (Cranelift)
Rules: ra-parser (.rra), ra-compiler (indexing)
Cost models: ra-hardware, ra-ml, ra-adaptive
Translation: ra-dialect (6 dialects)
Testing: ra-isolation
Discovery: ra-synthesis, ra-discovery, ra-multimodel
Foundation: ra-core (RelExpr, Expr, Cost, Rule)
```

### 3.2 Rule Format

Each rule is an .rra file -- a markdown document with YAML
frontmatter:

```yaml
id: filter-through-join
name: Filter Pushdown Through Join
category: logical/predicate-pushdown
databases: [postgresql, mysql, duckdb]
```

The document body contains:
- **Description**: Plain-English explanation of the transformation
- **Relational algebra**: Formal notation (sigma, pi, join symbols)
- **Implementation**: egg rewrite rule in Rust
- **Preconditions**: Guard conditions for correctness
- **Cost model**: Estimated benefit as a function of statistics
- **Test cases**: SQL examples (positive and negative)
- **References**: Database source code links and academic papers

### 3.3 Optimization Pipeline

1. Parse SQL to `RelExpr` (relational algebra AST)
2. Load .rra rules and compile to egg rewrites
3. Build e-graph from query plan
4. Apply rules via equality saturation
5. Extract lowest-cost plan using hardware-aware cost model
6. Generate executable code (Cranelift JIT, WASM, or bytecode)

### 3.4 Hardware Cost Model

The `HardwareCostModel` estimates execution cost per operator per
device:

```
Cost(op, device) = compute_time(op, device) + transfer_time(data, path)
```

Where `transfer_time` accounts for PCIe bandwidth (host to GPU),
BRAM capacity (FPGA), and NUMA hop latency (multi-socket CPU).

Key finding: for pure bandwidth-bound scans, CPU memory bandwidth
(DDR5 at 50 GB/s) exceeds PCIe 4.0 (25 GB/s), so GPU acceleration
only benefits compute-intensive operations (compound predicates,
hash joins, aggregations). The cost model correctly captures this
by comparing total time including transfer overhead.

### 3.5 Hardware Profiles

Three preset profiles model common configurations:

| Profile        | Device          | Memory    | Bandwidth |
|----------------|-----------------|-----------|-----------|
| GPU Server     | NVIDIA A100     | 80 GB HBM | 2 TB/s   |
| FPGA Appliance | Xilinx Alveo    | 32 GB DDR | 460 GB/s  |
| CPU Only       | 2x Xeon         | 512 GB DDR| 100 GB/s  |

## 4. Rule Catalog

### 4.1 Logical Rules (20 rules)

Standard relational algebra transformations:
- **Predicate pushdown** (5 rules): Push filters through joins,
  projections, unions, and into scans.
- **Join reordering** (5 rules): Commutativity, associativity,
  bushy trees, Cartesian-to-join conversion.
- **Projection pushdown** (3 rules): Column pruning, merge, and
  push through joins.
- **Expression simplification** (5 rules): Constant folding,
  boolean simplification, null propagation.
- **Set operations** (2 rules): Union merge, intersect-to-join.

### 4.2 Hardware Rules (21 rules)

Rules for heterogeneous execution:
- **GPU** (8 rules): Parallel scan, hash join, aggregation, sort,
  predicate evaluation, string operations, window functions,
  distinct aggregation.
- **FPGA** (4 rules): Stream filter, compression scan, hash join,
  regex filter.
- **CPU acceleration** (5 rules): SIMD vectorized scan, NUMA-aware
  partitioning, prefetch-aware join, cache-conscious partitioning,
  heterogeneous operator placement.
- **Data placement** (4 rules): Host-device transfer, device memory
  caching, columnar conversion, unified memory management.

### 4.3 Distributed Rules (36 rules)

Rules for multi-node query execution:
- Exchange placement and merging (4 rules)
- Data movement minimization (5 rules)
- Distributed joins: broadcast, shuffle, co-located, lookup,
  semi-join reduction, skew-aware (7 rules)
- Two/three-phase aggregation (3 rules)
- Partition pruning: static, dynamic, partition-wise join (3 rules)
- Distributed sort and top-N (3 rules)
- Co-location strategies (5 rules)
- Stage planning (3 rules)

### 4.4 Multi-Model Rules (30 rules)

Rules extending relational optimization to non-relational patterns:
- **Graph** (10 rules): Join-to-traversal, bidirectional search,
  path materialization, pattern decomposition.
- **Document** (10 rules): Nested predicate pushdown, array unwind
  pushdown, pipeline coalescence.
- **Time-series** (10 rules): Time range pruning, downsampling
  pushdown, last-point optimization.

## 5. Cross-Database Validation

### 5.1 Methodology

Each rule references source code locations in production databases
where the optimization is implemented. We validate correctness by:

1. **Semantic equivalence**: Run before/after SQL against PostgreSQL,
   DuckDB, and SQLite; verify identical result sets.
2. **Cost improvement**: Verify that the optimized plan has lower
   estimated cost using the database's own EXPLAIN output.
3. **Negative cases**: Confirm that guard conditions prevent
   incorrect application (e.g., filter pushdown through outer joins).

### 5.2 Coverage Matrix

| Database   | Logical | Physical | Hardware | Distributed |
|------------|---------|----------|----------|-------------|
| PostgreSQL | 18/20   | --       | --       | --          |
| MySQL      | 15/20   | --       | --       | --          |
| DuckDB     | 19/20   | --       | 3/21     | --          |
| SQLite     | 12/20   | --       | --       | --          |
| HeavyDB    | --      | --       | 6/21     | --          |
| Citus      | --      | --       | --       | 15/36       |

### 5.3 Isolation Testing

The ra-isolation crate adapts PostgreSQL's `isolationtester`
infrastructure to verify transaction isolation guarantees across
databases. Spec files define concurrent transaction scenarios;
the framework executes all step permutations and detects anomalies
(dirty reads, phantom reads, write skew).

## 6. Related Work

**Cascades/Volcano** [Graefe 1993, 1995]: Top-down rule-based
optimizer using memo tables. RA's e-graph approach subsumes memo
tables while avoiding rule ordering issues.

**Apache Calcite** [Begoli et al. 2018]: Shared optimization
framework used by Hive, Flink, and others. Calcite uses a
Volcano-style optimizer; RA uses equality saturation for more
complete plan space exploration.

**Tengu** [Zhang et al. 2022]: Learned query optimizer using
reinforcement learning. RA's ra-ml crate provides ML cardinality
estimation as a complement to rule-based optimization.

**egg** [Willsey et al. 2021]: The e-graph library underlying
RA's optimization engine. RA contributes a domain-specific rule
catalog and hardware-aware cost model on top of egg's
infrastructure.

**Cockroach/Optgen** [CockroachDB]: DSL for defining optimizer
rules. RA's .rra format is broader, including formal algebra,
cost models, test cases, and references alongside the
implementation.

## 7. Future Work

1. **CXL Memory**: Rules for CXL-attached memory pooling that
   extend the NUMA cost model to disaggregated memory.
2. **Learned Index Selection**: ML-based index structure selection
   integrated with the cost model.
3. **Formal Verification**: TLA+ specifications for critical
   properties (termination, equivalence, cost monotonicity).
4. **Adaptive Rule Discovery**: Mining execution logs to
   automatically synthesize new optimization rules.
5. **Disaggregated Storage**: Rules for compute-storage separation
   in cloud-native architectures.

## 8. Conclusion

RA demonstrates that query optimization knowledge can be captured
in a literate, composable, formally specified format. By using
equality saturation, the system avoids the phase-ordering problem
and enables exhaustive plan space exploration. Hardware-aware cost
models extend traditional optimization to heterogeneous CPU/GPU/FPGA
environments. The interactive web explorer makes this knowledge
accessible for both research and education.

The rule catalog, covering 147 transformations across five categories,
represents a concrete step toward a shared, open-source repository
of database optimization knowledge.

## References

1. Selinger, P.G., et al. "Access Path Selection in a Relational
   Database Management System." SIGMOD 1979.

2. Graefe, G. "The Volcano Optimizer Generator: Extensibility and
   Efficient Search." ICDE 1993.

3. Graefe, G. "The Cascades Framework for Query Optimization."
   IEEE Data Engineering Bulletin, 18(3), 1995.

4. Tate, R., et al. "Equality Saturation: A New Approach to
   Optimization." POPL 2009.

5. Willsey, M., et al. "egg: Fast and Extensible Equality
   Saturation." POPL 2021.

6. Begoli, E., et al. "Apache Calcite: A Foundational Framework
   for Optimized Query Processing Over Heterogeneous Data Sources."
   SIGMOD 2018.

7. Neumann, T. "Efficiently Compiling Efficient Query Plans for
   Modern Hardware." VLDB 2011.

8. Boncz, P.A., et al. "MonetDB/X100: Hyper-Pipelining Query
   Execution." CIDR 2005.

9. Leis, V., et al. "Morsel-Driven Parallelism: A NUMA-Aware
   Query Evaluation Framework for the Many-Core Age." SIGMOD 2014.

10. McSherry, F., et al. "Differential Dataflow." CIDR 2013.

11. Idreos, S., et al. "Database Cracking." CIDR 2007.

12. Berenson, H., et al. "A Critique of ANSI SQL Isolation Levels."
    SIGMOD 1995.

13. He, B., et al. "Relational Joins on Graphics Processors."
    SIGMOD 2008.

14. Mueller, R., et al. "Data Processing on FPGAs." VLDB 2009.

15. Ragan-Kelley, J., et al. "Halide: A Language and Compiler for
    Optimizing Parallelism, Locality, and Recomputation in Image
    Processing Pipelines." PLDI 2013.
