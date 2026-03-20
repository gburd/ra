# Architecture

This document describes the architecture of the Relational Algebra Rule System.

## Overview

The system consists of several layers:

```
┌─────────────────────────────────────────────────────────────────┐
│                         Applications                             │
│          CLI (ra-cli)  ·  Web (ra-web)  ·  WASM (ra-wasm)       │
└────────────────┬────────────────┬────────────────┬──────────────┘
                 ↓                ↓                ↓
┌────────────────────┐  ┌──────────────────┐  ┌────────────────┐
│ Optimization Engine│  │  Code Generation │  │  SQL Dialect    │
│    (ra-engine)     │  │   (ra-codegen)   │  │  (ra-dialect)   │
│  E-graph + egg     │  │ Cranelift + WASM │  │  Translation    │
└────────┬───────────┘  └──────────────────┘  └────────────────┘
         ↓
┌──────────────────────────────────────────────────────────────┐
│                    Rule Repository                             │
│       Parser (ra-parser)  ·  Compiler (ra-compiler)           │
│  100+ rules: logical · physical · hardware · distributed      │
└────────┬──────────────────────────────────────────────────────┘
         ↓
┌──────────────────────────────────────────────────────────────┐
│                      Core Types (ra-core)                      │
│     RelExpr · Expression · Rule · Cost · Statistics            │
└───────┬────────────┬────────────┬────────────┬───────────────┘
        ↓            ↓            ↓            ↓
┌────────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────┐
│ ra-hardware│ │  ra-ml   │ │ra-adaptive│ │ ra-isolation │
│ GPU/FPGA   │ │ ML cost  │ │ Runtime  │ │  Isolation   │
│ cost model │ │ estimator│ │ reopt    │ │  testing     │
└────────────┘ └──────────┘ └──────────┘ └──────────────┘
```

## Components

### ra-core

The foundation layer providing core types and traits:

- **RelExpr**: Relational algebra AST (Scan, Filter, Join, Project, etc.)
- **Expr**: Expression types (Column, Const, BinOp, Function, etc.)
- **Rule**: Rule trait and metadata types
- **Pattern**: Pattern matching for rules
- **Cost**: Cost model traits and types
- **Statistics**: Cardinality, selectivity, histograms
- **Properties**: Physical properties (ordering, partitioning)

All types are designed to be:
- Serializable (for network transport and caching)
- Cloneable (for e-graph operations)
- Well-documented (literate programming philosophy)

### ra-parser

Parses `.rra` (Relational Rule Algebra) literate format files:

- **parser.rs**: Main parser combining YAML frontmatter and markdown
- **extractor.rs**: Extracts code blocks from markdown
- **validator.rs**: Validates frontmatter schema
- **lexer.rs**: Tokenization (if needed for custom syntax)

Input: `.rra` files
Output: `RuleFile` structs with metadata and code sections

### ra-compiler

Compiles and indexes rules:

- **index.rs**: Builds searchable index of rules
- **analyzer.rs**: Analyzes rule dependencies and conflicts
- **checker.rs**: Type checks rule patterns and applications
- **registry.rs**: Manages loaded rules

Input: Parsed `RuleFile` structs
Output: Compiled rule registry with metadata

### ra-engine

The optimization engine:

- **egraph.rs**: E-graph construction using `egg` library
- **rewrite.rs**: Converts rules to egg rewrite rules
- **extract.rs**: Cost-based plan extraction from e-graph
- **analysis.rs**: E-graph analysis passes
- **differential.rs**: Differential dataflow for incremental updates
- **timely_integration.rs**: Timely dataflow integration

Key algorithms:
1. **Equality Saturation**: Uses egg's e-graph to explore equivalent plans
2. **Cost-Based Extraction**: Finds lowest-cost plan from e-graph
3. **Incremental Maintenance**: Differential dataflow for efficient updates

### ra-codegen

Generates executable code:

- **ir.rs**: Internal intermediate representation
- **cranelift_backend.rs**: JIT compilation using Cranelift
- **wasm.rs**: WASM compilation using wasmtime
- **bytecode.rs**: Simple bytecode interpreter
- **volcano.rs**: Volcano-style iterator code generation

Input: Optimized physical plan
Output: Executable query code

### ra-cli

Command-line interface:

```bash
ra-cli validate <path>     # Validate .rra files
ra-cli test <path>         # Run rule test cases
ra-cli list                # List available rules
ra-cli show <rule-id>      # Show rule details
ra-cli optimize <query>    # Optimize a SQL query
ra-cli explain <query>     # Explain transformations
ra-cli benchmark           # Run benchmarks
```

### ra-web

Web explorer backend API:

- REST API for optimization and exploration
- WebSocket for real-time updates
- URL shortening for sharing
- Static file serving

Endpoints:
- `POST /api/optimize` - Optimize a query
- `POST /api/explain` - Explain transformations
- `POST /api/share` - Generate shareable URL
- `GET /api/rules` - List available rules

## Data Flow

### Query Optimization Flow

```
SQL Query
    ↓
[Parse to RelExpr]
    ↓
[Build E-graph with rules]
    ↓
[Equality Saturation]
    ↓
[Cost-based Extraction]
    ↓
Optimized Physical Plan
    ↓
[Code Generation]
    ↓
Executable Code
```

### Rule Update Flow (Incremental)

```
Rule Change
    ↓
[Parse and Validate]
    ↓
[Update Differential Collection]
    ↓
[Recompute Affected Plans]
    ↓
Updated Registry
```

## Equality Saturation with egg

The optimization engine uses `egg` (e-graphs good) for equality saturation:

1. **E-graph Construction**: Convert query to e-graph representation
2. **Rule Application**: Apply all rules exhaustively
3. **Saturation**: Continue until no new expressions added
4. **Extraction**: Find lowest-cost equivalent expression

Benefits:
- Explores all equivalent plans simultaneously
- Avoids local optima
- Composable rules without dependencies
- Terminates with saturation or timeout

## Differential Dataflow

For incremental maintenance when rules change:

1. **Initial Computation**: Build optimization graph
2. **Change Detection**: Detect rule additions/removals/changes
3. **Incremental Update**: Recompute only affected parts
4. **Consolidation**: Merge results

Benefits:
- Efficient rule updates
- Minimal recomputation
- Scales to large rule sets

## Cost Model

Cost estimation considers:

- **I/O Cost**: Disk reads, sequential vs random access
- **CPU Cost**: Row processing, expression evaluation
- **Memory Cost**: Hash tables, sorts, buffers
- **Network Cost**: Data transfer for distributed queries

Cost formula:
```
TotalCost = IO_Cost + CPU_Cost + Memory_Cost + Network_Cost
```

Statistics used:
- Table cardinality (row count)
- Column cardinality (distinct values)
- Data distribution (histograms)
- Correlation between columns

## Formal Verification

### TLA+ Specifications

Critical properties verified:

1. **Termination**: Optimization always terminates
2. **Equivalence**: Transformations preserve semantics
3. **Cost Monotonicity**: Logical rules never increase cost
4. **Confluence**: Rule order doesn't affect final result

### Property-Based Testing

Using `proptest` to verify:

- Semantic equivalence of transformations
- Cost model consistency
- Termination guarantees
- Idempotence of rules

### Differential Testing

Compare against reference databases:
- PostgreSQL
- DuckDB
- SQLite

Verify that:
- Results match reference implementation
- Plans are at least as good (lower cost)

## Performance Considerations

### Optimization Time

Target: <100ms for typical queries

Strategies:
- Timeout after fixed time budget
- Incremental optimization (return best so far)
- Rule prioritization (apply most beneficial first)
- Memoization of subplans

### Memory Usage

E-graphs can grow large. Mitigations:
- Node limits
- Garbage collection of unreachable nodes
- Compression of equivalent nodes

### Parallelization

Opportunities:
- Parallel rule application
- Parallel cost estimation
- Parallel code generation

## Extension Points

The system is designed to be extensible:

1. **Custom Rules**: Add new `.rra` files
2. **Custom Cost Models**: Implement `CostModel` trait
3. **Custom Backends**: Implement code generation for new targets
4. **Custom Statistics**: Extend statistics types

### ra-hardware

Hardware-aware cost models and operator placement:

- **device.rs**: Device enum (CPU, GPU, FPGA) and transfer path modeling
- **profile.rs**: HardwareProfile describing system capabilities with
  preset profiles for GPU servers (A100), FPGA appliances (Alveo), and
  CPU-only systems
- **cost.rs**: HardwareCostModel implementing CostModel trait, estimating
  execution cost on each device including PCIe transfer overhead

See [hardware-acceleration.md](features/hardware-acceleration.md) for details on
the 21 hardware-specific optimization rules covering GPU scans/joins/
aggregations, FPGA streaming filters, SIMD vectorization, and NUMA-aware
partitioning.

### ra-adaptive

Runtime reoptimization and adaptive execution:

- **runtime_stats.rs**: Runtime statistics collection
- **triggers.rs**: Reoptimization trigger conditions
- **plan_switch.rs**: Mid-execution plan switching
- **executor.rs**: Adaptive query executor
- **checkpoint.rs**: Checkpoint/restart for plan transitions

### ra-ml

ML-based cardinality estimation:

- **features.rs**: Feature extraction from query plans
- **nn.rs**: Neural network model for cardinality prediction
- **estimator.rs**: ML-enhanced cost estimator
- **training.rs**: Online model training from execution feedback

### ra-synthesis

Query synthesis from natural language:

- **intent.rs**: Intent parser for natural language queries
- **generator.rs**: Query generator from parsed intent to RelExpr
- **validator.rs**: Query validation
- **render.rs**: SQL rendering from RelExpr

### ra-discovery

Automatic rule discovery from execution logs:

- **log.rs**: Execution log parsing
- **fingerprint.rs**: Query fingerprinting
- **mining.rs**: Pattern mining from log data
- **synthesis.rs**: Rule synthesis from discovered patterns
- **validation.rs**: Discovered rule validation

## Future Enhancements

- **CXL Memory**: Rules for CXL-attached memory pooling
- **Disaggregated Storage**: Rules for compute-storage separation
- **Learned Indexes**: ML-based index structure selection

## References

- [egg: Fast and Extensible Equality Saturation](https://arxiv.org/abs/2004.03082)
- [Differential Dataflow](https://github.com/TimelyDataflow/differential-dataflow)
- [The Volcano Optimizer Generator](https://dl.acm.org/doi/10.1109/69.273032)
- [Access Path Selection in a Relational Database (System R)](https://dl.acm.org/doi/10.1145/582095.582099)
