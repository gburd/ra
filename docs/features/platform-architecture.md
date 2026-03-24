# Platform Architecture

This document provides a high-level overview of the Relational Algebra
Rule System as an integrated platform, covering how the crates
connect and the data flows between them.

## System Overview

The platform codifies database query optimization knowledge into a
single system that can parse, validate, optimize, execute, and
translate SQL across database engines.

```mermaid
graph TD
    CLI["CLI (ra-cli)"]
    Web["Web (ra-web)"]
    WASM["WASM Playground<br/>(ra-wasm)"]

    Engine["Query Optimization<br/>(ra-engine)"]
    Dialect["Dialect Translation<br/>(ra-dialect)"]
    Isolation["Isolation Testing<br/>(ra-isolation)"]

    PipelineDesc["Parser (ra-parser) → Compiler (ra-compiler)<br/>147 rules: logical, physical, hardware,<br/>distributed, multi-model"]

    CoreDesc["RelExpr, Expr, Rule, Cost, Statistics"]

    HW["ra-hardware<br/>GPU / FPGA<br/>cost model"]
    ML["ra-ml<br/>ML cost<br/>estimation"]
    Adapt["ra-adaptive<br/>Runtime<br/>reoptimization"]
    Codegen["ra-codegen<br/>Cranelift<br/>WASM / JIT"]

    PGExt["PostgreSQL Extension<br/>(ra-pg-extension)"]

    CLI --> Engine
    CLI --> Dialect
    Web --> Engine
    Web --> Isolation
    WASM --> Engine
    PGExt --> Engine
    PGExt --> HW
    Engine --> PipelineDesc
    PipelineDesc --> CoreDesc
    CoreDesc --> HW
    CoreDesc --> ML
    CoreDesc --> Adapt
    CoreDesc --> Codegen

    style CLI fill:#e1f5fe
    style Web fill:#e1f5fe
    style WASM fill:#e1f5fe
    style PGExt fill:#e1f5fe
    style Engine fill:#fff3e0
    style Dialect fill:#fff3e0
    style Isolation fill:#fff3e0
    style PipelineDesc fill:#f3e5f5
    style CoreDesc fill:#e8f5e9
    style HW fill:#fce4ec
    style ML fill:#fce4ec
    style Adapt fill:#fce4ec
    style Codegen fill:#fce4ec
```

## Crate Dependency Graph

```mermaid
graph TD
    Core[ra-core] --> Parser[ra-parser]
    Core --> Compiler[ra-compiler]
    Core --> Engine[ra-engine]
    Core --> Codegen[ra-codegen]
    Core --> Hardware[ra-hardware]
    Core --> ML[ra-ml]
    Core --> Adaptive[ra-adaptive]
    Core --> Dialect[ra-dialect]
    Core --> Wasm[ra-wasm]

    Parser --> Compiler
    Compiler --> Engine
    Engine --> Adaptive

    Engine --> CLI[ra-cli]
    Dialect --> CLI
    Engine --> Web[ra-web]
    Wasm --> Web
```

```mermaid
graph BT
    Core["ra-core<br/>(foundation)"]
    Parser["ra-parser"] --> Core
    Compiler["ra-compiler"] --> Core
    Compiler --> Parser
    Engine["ra-engine"] --> Compiler
    Codegen["ra-codegen"] --> Core
    Hardware["ra-hardware"] --> Core
    ML["ra-ml"] --> Core
    Adaptive["ra-adaptive"] --> Engine
    Dialect["ra-dialect"] --> Core
    WasmCrate["ra-wasm"] --> Core
    Isolation["ra-isolation"] -.-> WasmCrate
    Synthesis["ra-synthesis"] --> Core
    Discovery["ra-discovery"] --> Core
    Multimodel["ra-multimodel"] --> Core
    PGExt["ra-pg-extension"] --> Engine
    PGExt --> Hardware
    PGExt --> Core
    CLI["ra-cli"] --> Engine
    CLI --> Dialect
    WebCrate["ra-web"] --> Engine
    WebCrate --> WasmCrate

    style Core fill:#e8f5e9
    style CLI fill:#e1f5fe
    style WebCrate fill:#e1f5fe
    style PGExt fill:#e1f5fe
```

## Data Flow: Query Optimization

A SQL query flows through the system as follows:

```mermaid
graph TD
    S1["1. SQL text"] --> S2["2. SQL parser"]
    S2 -->|"RelExpr (logical plan)"| S3["3. ra-parser"]
    S3 -->|"Load .rra rule files"| S4["4. ra-compiler"]
    S4 -->|"Compile rules into egg rewrites"| S5["5. ra-engine: e-graph"]
    S5 -->|"Add logical plan<br/>Apply all rules (equality saturation)<br/>Saturate until fixed point or timeout"| S6["6. ra-engine: extract"]
    HW["ra-hardware<br/>hardware-aware cost model"] --> S6
    MLCost["ra-ml<br/>ML cardinality estimation"] --> S6
    S6 -->|"Lowest-cost physical plan"| S7["7. ra-codegen"]
    S7 -->|"Executable code<br/>(JIT / WASM / bytecode)"| S8["8. Execute against database"]

    style S1 fill:#e1f5fe
    style S2 fill:#e1f5fe
    style S3 fill:#f3e5f5
    style S4 fill:#f3e5f5
    style S5 fill:#fff3e0
    style S6 fill:#fff3e0
    style HW fill:#fce4ec
    style MLCost fill:#fce4ec
    style S7 fill:#e8f5e9
    style S8 fill:#e8f5e9
```

## Data Flow: Dialect Translation

```mermaid
graph TD
    D1["1. SQL text<br/>(source dialect)"] --> D2["2. sqlparser: parse"]
    D2 -->|AST| D3["3. ra-dialect: rewrite passes"]
    D3 -->|"Function rewriting (CONCAT vs &#124;&#124;)<br/>Operator rewriting (:: vs CAST)<br/>LIMIT/OFFSET syntax<br/>Date/time functions<br/>Type mappings"| D4["4. sqlparser: render"]
    D4 -->|"SQL text (target dialect)"| D5["5. Warnings about<br/>semantic differences"]

    style D1 fill:#e1f5fe
    style D2 fill:#e1f5fe
    style D3 fill:#fff3e0
    style D4 fill:#e8f5e9
    style D5 fill:#fce4ec
```

## Data Flow: Isolation Testing

```mermaid
graph TD
    I1["1. .spec file"] --> I2["2. spec_parser"]
    I2 -->|SpecFile| I3["3. scheduler"]
    I3 -->|"Generate step orderings<br/>(permutations)"| I4["4. For each permutation"]

    I4 --> SM["session manager<br/>Open N database sessions"]
    I4 --> EX["executor<br/>Execute steps in scheduled order"]
    I4 --> LM["lock monitor<br/>Track locks, detect deadlocks"]
    I4 --> SN["snapshot<br/>Verify visibility rules"]
    I4 --> EV["events<br/>Record what happened"]

    SM --> I5["5. TestResult<br/>anomalies detected,<br/>pass/fail per permutation"]
    EX --> I5
    LM --> I5
    SN --> I5
    EV --> I5

    style I1 fill:#e1f5fe
    style I2 fill:#e1f5fe
    style I3 fill:#fff3e0
    style I4 fill:#f3e5f5
    style SM fill:#fce4ec
    style EX fill:#fce4ec
    style LM fill:#fce4ec
    style SN fill:#fce4ec
    style EV fill:#fce4ec
    style I5 fill:#e8f5e9
```

## Data Flow: PostgreSQL Planner Hook

When loaded as a PostgreSQL extension via `shared_preload_libraries`,
Ra intercepts SELECT queries through the planner hook mechanism:

```mermaid
graph TD
    PG1["1. PostgreSQL receives SQL"] --> PG2["2. Parse/Analyze<br/>(standard PostgreSQL)"]
    PG2 -->|"Query parse tree"| PG3["3. planner_hook<br/>(ra_planner_hook)"]

    PG3 -->|"SELECT with ≤12 relations"| RA1["4. query_parser<br/>Query → RelExpr"]
    PG3 -->|"DML / utility / too many relations"| PGSTD["standard_planner()"]

    RA1 --> RA2["5. stats_bridge<br/>pg_class, pg_statistic,<br/>pg_constraint → Statistics"]
    RA2 --> RA3["6. ra-engine<br/>e-graph optimization"]
    RA3 --> RA4["7. plan_converter<br/>RelExpr → PlanAdviceSet"]
    RA4 --> RA5["8. cost_mapper<br/>Ra Cost → PG Cost"]
    RA5 -->|"confidence ≥ threshold"| RA6["9. Apply advice via GUCs<br/>enable_hashjoin, random_page_cost, ..."]
    RA5 -->|"confidence < threshold"| PGSTD
    RA6 --> PGSTD
    PGSTD --> PG4["10. PlannedStmt<br/>returned to executor"]

    style PG1 fill:#e1f5fe
    style PG2 fill:#e1f5fe
    style PG3 fill:#f3e5f5
    style RA1 fill:#fff3e0
    style RA2 fill:#fff3e0
    style RA3 fill:#fff3e0
    style RA4 fill:#fff3e0
    style RA5 fill:#fce4ec
    style RA6 fill:#fce4ec
    style PGSTD fill:#e8f5e9
    style PG4 fill:#e8f5e9
```

The extension reads PostgreSQL catalog statistics (via syscache, not
SPI) and uses hardware detection to tune cost parameters. On any error,
it falls back to the standard planner. See
[PostgreSQL Integration](../integrations/postgresql.md) for full details.

## Crate Summaries

### Foundation

| Crate       | Purpose                                    |
|-------------|--------------------------------------------|
| ra-core     | Shared types: RelExpr, Expr, Cost, Rule    |
| ra-parser   | Parse .rra literate rule files             |
| ra-compiler | Compile and index rules, type checking     |

### Optimization

| Crate       | Purpose                                    |
|-------------|--------------------------------------------|
| ra-engine   | E-graph optimization (egg), extraction     |
| ra-hardware | GPU/FPGA/SIMD/NUMA cost models             |
| ra-ml       | Neural network cardinality estimation      |
| ra-adaptive | Runtime reoptimization, plan switching     |
| ra-codegen  | JIT compilation (Cranelift), WASM, bytecode|

### Translation and Testing

| Crate        | Purpose                                   |
|--------------|-------------------------------------------|
| ra-dialect   | SQL dialect translation (6 dialects)      |
| ra-isolation | Cross-database isolation testing          |
| ra-wasm      | WASM database adapters (SQLite, DuckDB)   |

### Rule Discovery

| Crate        | Purpose                                   |
|--------------|-------------------------------------------|
| ra-synthesis | Natural language to SQL generation        |
| ra-discovery | Automatic rule mining from execution logs |
| ra-multimodel| Graph, document, time-series rules        |

### Applications

| Crate             | Purpose                                      |
|-------------------|----------------------------------------------|
| ra-cli            | Command-line interface                       |
| ra-web            | Web explorer backend (Rocket.rs)             |
| ra-pg-extension   | PostgreSQL planner hook (pgrx, PG 13--18)    |

## Rule Categories

The 147 rules are organized into 5 major categories:

### Logical Rules (20 rules)

Transform query plans while preserving semantics:
- Predicate pushdown (5 rules)
- Join reordering (5 rules)
- Projection pushdown (3 rules)
- Expression simplification (5 rules)
- Set operations (2 rules)

### Hardware Rules (21 rules)

Accelerate operators using specialized hardware:
- GPU operators (8 rules) -- parallel scan, hash join, aggregation
- FPGA streaming (4 rules) -- filter, compression, regex
- CPU acceleration (5 rules) -- SIMD, NUMA, cache, prefetch
- Data placement (4 rules) -- transfer, caching, memory management

### Distributed Rules (36 rules)

Optimize queries across multiple nodes:
- Exchange placement (4 rules)
- Data movement minimization (5 rules)
- Distributed joins (7 rules) -- broadcast, shuffle, co-located
- Partial aggregation (3 rules)
- Partition pruning (3 rules)
- Distributed sort/topN (3 rules)
- Co-location strategies (5 rules)
- Stage planning (3 rules)

### Multi-Model Rules (30 rules)

Optimize non-relational query patterns:
- Graph traversal (10 rules) -- path, pattern matching
- Document queries (10 rules) -- nested pushdown, pipelines
- Time-series (10 rules) -- range pruning, downsampling

### Physical and Database-Specific

Physical operator selection, index strategies, and engine-specific
optimizations (categories defined but not yet populated with rules).

## Configuration

### Optimization Budget

The engine respects time and iteration budgets:

```rust
OptimizationConfig {
    timeout_ms: 1000,      // max wall-clock time
    max_iterations: 100,   // max e-graph iterations
    cost_model: Box::new(HardwareCostModel::new(profile)),
}
```

### Hardware Profiles

Preset hardware profiles configure cost models:

- `HardwareProfile::gpu_server()` -- NVIDIA A100 80 GB, PCIe 4.0
- `HardwareProfile::fpga_appliance()` -- Xilinx Alveo U280
- `HardwareProfile::cpu_only()` -- Dual Xeon, DDR5

## Extension Points

1. **Custom Rules** -- Add `.rra` files to `rules/` directory
2. **Custom Cost Models** -- Implement `CostModel` trait
3. **Custom Backends** -- Implement code generation for new targets
4. **Custom Dialects** -- Add `Dialect` variants and translation rules
5. **Custom Adapters** -- Implement `DatabaseAdapter` for new engines

## References

- [Architecture details](architecture.md)
- [Rule authoring guide](rule-authoring.md)
- [API reference](api-reference.md)
- [Cost models](cost-models.md)
- [Hardware acceleration](hardware-acceleration.md)
- [Dialect translation](dialect-translation.md)
- [Isolation testing](isolation-testing.md)
- [WASM databases](wasm-databases.md)
- [Execution models](execution-models.md)
- [PostgreSQL integration](../integrations/postgresql.md)
