# Changelog

## [0.4.0] - 2026-05-17

### Added
- Post-extraction ordering propagation pass (RFC 0025) — eliminates redundant
  Sort nodes, converts Sort to IncrementalSort when input provides prefix ordering
- Updated Ra vs PostgreSQL 18.4 benchmark: 89x planning speedup (21/21 queries)

### Fixed
- Missing doc comments on TxnStmt struct fields (ra-core)
- 16 clippy warnings in ra-parser (unnested or-patterns, doc_markdown, etc.)
- Incorrect hardware profile names in CLI help text

### Changed
- Optimization pipeline now includes ordering pass after extraction on all paths
- Removed 37K benchmark run artifacts from repository tracking

### Performance
- Planning overhead from ordering pass: +0.3% (simple) to +18% (complex queries)
- Net benefit: eliminates Sort operators at execution time (ms-to-seconds saved)

---

## [0.2.1] - 2026-03-27

Phase 21: CLI Enhancements and Web UI Demonstrations

Recent commits focusing on developer experience improvements, enhanced CLI output,
and interactive web demonstrations.

### Added

**CLI Output Enhancements** (`ra-cli`)
- Smart header text that detects unlimited vs bounded resource budgets
- Real-time system metrics display (CPU utilization, load average, memory usage)
- Reorganized output order for better readability: hardware first, then formatted SQL
- SQL pretty-printing with proper formatting
- Enhanced optimization step visualization
- Rust-compiler-style error messages with context

**System Metrics Module** (`ra-hardware`)
- `SystemMetrics` struct collecting CPU, memory, and load average data
- CPU utilization sampled from `/proc/stat` on Linux
- Memory usage from `/proc/meminfo`
- Formatted output: `CPU: 15.3% | Load: 1.42 | Memory: 68.5% (4096 / 12288 MB)`
- Integration with CLI verbose mode

**Proxy Command Foundation** (`ra-cli`)
- Command structure for database proxy functionality
- Argument handling for connection strings, ports, and backends
- Foundation for transparent query interception and optimization

**EXPLAIN Formatters** (`ra-dialect`)
- Database-specific EXPLAIN output formatters for PostgreSQL, MySQL, SQLite
- Integration with CLI for formatted query plan output
- Support for multiple EXPLAIN formats (text, JSON, XML)

**Interactive Web Demonstrations** (`ra-web`)
- 10 fully functional demos with backend API endpoints
- Statistics staleness impact visualization
- Hardware-specific plan comparison across 12 profiles
- Join algorithm selection with cost breakdown
- Aggregation strategy selection
- Index selection based on selectivity
- Subquery unnesting demonstration
- Parallel query execution scaling
- GPU offloading decision analysis
- Distributed query planning (broadcast vs shuffle)
- Cost model calibration interface

**Documentation** (`docs/`)
- Comprehensive Ra Web UI quickstart guide with demo walkthroughs
- Detailed descriptions of all 10 interactive demonstrations
- API endpoint documentation for programmatic access
- Troubleshooting and architecture sections

### Changed

**CLI Output Order** (`ra-cli`)
- Moved hardware information to always display (not just verbose mode)
- System metrics now shown in verbose mode after hardware info
- SQL query formatted and displayed before plans
- Optimization steps better separated visually

**Improved Verbose Mode** (`ra-cli`)
- Better diff visualization with `--diff` flag
- Enhanced rule application tracking
- Clearer cost delta reporting

### Fixed

**Build Issues**
- Added unixODBC to flake.nix for ODBC database support
- Fixed proxy command argument type conversions
- Removed unused imports across multiple crates
- Added missing OptimizerConfig fields to differential_timeline benchmark
- Completed exhaustive pattern matching for FieldAccess and SubQuery expressions
- Cleaned up clippy warnings with thread safety bounds

**Documentation**
- Fixed VitePress dead links
- Corrected CLI documentation mismatches
- Removed duplicate /ra prefix from RFC cross-links
- Added language aliases for VitePress syntax highlighting

**Expression Handling** (`ra-core`)
- Added FieldAccess and SubQuery variants to Expression enum
- Updated all pattern matches to handle new expression types
- Fixed type errors in budget passing

### Metrics

| Metric                    | Value                              |
|---------------------------|------------------------------------|
| Commits                   | 32                                 |
| CLI improvements          | 8 commits                          |
| Web demos implemented     | 10 (fully functional)              |
| API endpoints added       | 14 (4 core + 10 demo)              |
| Documentation pages       | 1 major guide (quickstart)         |
| Build fixes               | 7 commits                          |
| Breaking changes          | None                               |

---

## [0.2.0] - 2026-03-21

Phase 20: Documentation, SQL Coverage, and Engine Hardening

38 commits (`a536fa8..a19a398`) spanning documentation infrastructure,
SQL test coverage, engine compilation fixes, and deployment automation.

### Added

**SQL Query Encyclopedia** (`docs/`)
- 30 encyclopedia pages covering query patterns (OLTP, OLAP, analytical,
  joins, subqueries, recursive, temporal), schema patterns (star schema),
  dataset characteristics (cardinality), workload patterns, distributed
  patterns (shuffle joins), and index structures (B-tree)
- Each page includes relational algebra notation (LaTeX), cost model
  formulas, statistics API usage, Ra optimization rules, and performance
  characteristics

**SQL Test Suite** (`tests/`)
- 251 total SQL queries tested against Ra's parser and optimizer
- Book queries: 181 queries from 10 database textbooks (99.45% pass rate,
  180/181); sources include SQL Performance Explained (Winand), High
  Performance MySQL (Schwartz), Database System Concepts (Silberschatz),
  Designing Data-Intensive Applications (Kleppmann), and 6 others
- Real-world queries: 70+ queries from production codebases including
  Django migrations, Rails ActiveRecord, dbt models, TimescaleDB
  time-series, PostGIS geospatial, and Airflow ETL pipelines
- Test runners in Python, Shell, and Rust for CI integration

**LaTeX Math Rendering** (`docs/.vitepress/`)
- KaTeX integration via `@mdit/plugin-katex` for inline and block math
- Converted 1,096 `.rra` rule files from ASCII algebra to LaTeX notation
- Automated conversion script (`scripts/convert_algebra_to_latex.py`)
- LaTeX conversion candidates report identifying 400+ files with
  mathematical formulas

**Documentation Link Validation** (`crates/ra-test-utils/`)
- Automated link validator scanning 2,854+ documentation files
- Covers `docs/`, `rules/`, `research/`, root markdown, and Rust doc
  comments
- Regex-based extraction with relative/absolute path resolution
- Line-number reporting for broken links
- Dedicated GitHub Actions workflow for doc-change CI

**Deployment Infrastructure**
- GitHub Pages: automated VitePress + rustdoc deployment on push to main
- Netlify: `netlify.toml` with Node 22, SPA routing, security headers,
  aggressive caching, WASM content-type support
- Codeberg Pages: Forgejo Actions workflow for VitePress + WASM + rustdoc

**Cardinality-Aware Cost Model** (`ra-engine`)
- `CardinalityAwareCostFn` using `ra-ml` `HeuristicEstimator`
- Hardware-adjusted base costs per operator type
- Staleness-aware confidence (Fresh=1.0, VeryStale=1.5, Unknown=2.0)
- `extract_best_with_cardinality()` extraction function

**Rule Pre-Condition Filtering** (`ra-engine`)
- `.rra` YAML frontmatter parsing with `serde_yaml` and `walkdir`
- Precondition types: pattern, predicate, hardware, database, feature
- Runtime filtering via `Optimizer::optimize_with_facts()`

**Polyglot SQL Transpiler** (`ra-dialect`)
- Dual backend system (Native + Polyglot) via `Backend` trait
- Polyglot backend integrates `polyglot-sql` for 32+ SQL dialects
  (BigQuery, Snowflake, Databricks, Redshift, ClickHouse, Trino, etc.)
- Feature-gated with `polyglot-backend`; 26 new dialect variants

**Migration Validation** (`ra-engine`)
- 821 lines of validation logic in `migrate_commands.rs`
- Validates metadata identity, precondition safety, constraint narrowing
- Actionable `ValidationError` messages with suggested fixes

**Stoolap Adapter** (`ra-adapters`)
- `stoolap` v0.3 dependency with feature flag
- `StoolapFacts` with table/column stats and schema storage

### Changed

**E-Graph Integration** (`ra-engine`)
- IndexScan and IndexOnlyScan fully integrated into e-graph
- Added `index-scan` to `RelLang` enum, conversion/extraction functions
- Updated `ra-pg-advisor`, `ra-wasm`, `ra-cli` pattern matches
- All 870 `ra-engine` tests passing including 6/6 min_max_index tests

**Large Join Optimizer** (`ra-engine`)
- Fixed CTE/RecursiveCTE field names (`base_case`/`recursive_case`/`body`)
- Added 8 bitmap/parallel operator variants to all match statements
- Cost model switched from `IntegratedCostModel` to `HardwareCostModel`

**Dependency Fixes**
- Parquet downgraded to 53.4 (DataFusion compatibility)
- Chrono downgraded to 0.4.38 (Arrow compatibility)
- Added `anyhow` to `ra-engine`

### Fixed

- 7 broken documentation links updated for reorganized directory structure
  (`guides/`, `features/`, `integrations/`)
- Netlify build: excluded 15 unnest rule files causing Vue parser failures
  on SQL patterns like `AS t(col)`
- `HardwareProfile` field access corrected in `ra-test-utils`
- `count_metadata` tests decoupled from `all_rules()` to avoid transitive
  load of broken min_max_index rules
- Duplicate `IndexScan`/`IndexOnlyScan` match arms removed from
  `federated_optimizer.rs`, `memo.rs`, `estimator.rs`
- Non-exhaustive pattern matches in `large_join.rs` (Unnest, MultiUnnest,
  TableFunction, IncrementalSort, ParallelScan, ParallelHashJoin,
  ParallelAggregate, Gather)

### Documentation

**Massive Documentation Restructure**
- 1,335 individual rule documentation pages generated across 15 categories
- INDEX.md (1,397 lines) cataloging all 1,327+ transformation rules
- REFERENCES.md bibliography with research paper citations
- Cucumber.io-style hierarchy: `guides/`, `features/`, `integrations/`
- GETTING_STARTED.md (547 lines) covering all major features
- New README.md (114 lines) with quick start

**Implementation Architecture Guide** (`docs/`)
- egg (e-graph equality saturation), sqlparser 0.52, DataFusion,
  Cranelift/Wasmtime, Timely/Differential dataflow, proptest
- Apache Calcite influence on rule organization and Volcano/Cascades

**Ledger Example** (`docs/examples/ledger/`)
- Progressive 7-part guide: introduction, schema, basic queries,
  aggregations, statistics impact, dialect translation, hardware awareness

**API Documentation** (`docs/`)
- Statistics API: table, column, index, partition, distribution stats
- Facts API: unified interface for statistics, schema, hardware, runtime
- Cost model parameters, workload profiles, complete examples

**RFC System** (`rfcs/`)
- RFC process documentation and template
- 3 new RFCs: Incremental View Maintenance (0022), Adaptive Query
  Execution (0023), Query Result Caching (0024)
- INDEX.md tracking all RFCs by status with lifecycle management

**Research**
- CMU Database Group and PostgreSQL optimization knowledge mining
- 15 high-value missing optimization techniques identified
- 5 new RFCs proposed (0035-0039): Genetic Query Optimizer, Multi-Query
  Optimization, Interesting Orders, Loose Index Scan, Operator Class
  Aware Indexing
- Hybrid OLAP/OLTP hot/cold data tiering (Iceberg, Hudi, Delta Lake,
  ClickHouse, TimescaleDB)
- Columnar file format optimizations (Parquet, ORC, Arrow, Avro) with
  RFC 0033
- Database optimization shortcuts (COUNT(*), MIN/MAX, materialized views,
  approximate query processing)
- Distributed query patterns: 10 patterns documented with cost formulas
- Production workload modeling guide (PostgreSQL, MySQL statistics to Ra
  facts)

### Test Coverage

**Large Join Optimizer** (`large_join.rs`): 39 to 80 tests
- `count_tables` coverage for RecursiveCTE, CTE, RowPattern,
  BitmapIndexScan, BitmapAnd/Or, BitmapHeapScan, Unnest, MultiUnnest,
  TableFunction, IncrementalSort, ParallelScan, ParallelHashJoin,
  ParallelAggregate, Gather, IndexOnlyScan
- `extract_joins` coverage for pass-through arms, set operators, CTE
  variants, bitmap operators, parallel operators, leaf nodes
- 4-table annealing test exercising inner loop

**Cardinality Cost** (`cardinality_cost.rs`): 5 to 22 tests
- Direct `CostFunction<RelLang>::cost` tests for Scan, ScanAlias, Filter,
  Project, Join, Aggregate, Sort, IncrementalSort, Limit, Union, Window,
  DistinctRel, IndexOnlyScan, BitmapIndexScan, BitmapHeapScan,
  MetadataLookup

**Rule Metadata** (`rule_metadata.rs`): 42 to 51 tests
- `parse_rra_file` and `load_rules_from_directory` edge cases

**Covering Index** (`facts.rs`): 3 new tests
- Empty provider, key column match, INCLUDE-style column detection

### Metrics

| Metric                    | Value                              |
|---------------------------|------------------------------------|
| Commits                   | 38                                 |
| Features                  | 11                                 |
| Fixes                     | 10                                 |
| Documentation commits     | 13                                 |
| Test/CI commits           | 3                                  |
| Research commits          | 1                                  |
| Rule files converted      | 1,096 (to LaTeX)                   |
| Rule docs generated       | 1,335 pages                        |
| SQL queries tested        | 251 (99.6% pass rate)              |
| Book query pass rate      | 180/181 (99.45%)                   |
| Encyclopedia pages        | 30                                 |
| Doc files validated       | 2,854+                             |
| Broken links fixed        | 7                                  |
| Deployment platforms      | 3 (GitHub Pages, Netlify, Codeberg)|
| SQL dialects supported    | 32+ (via polyglot backend)         |
| New test cases            | 60+ (across 4 modules)             |
| Crates modified           | 8+                                 |
| Breaking changes          | None                               |

---

## Phase 19: Distributed Query Optimization

### Added

**Network Cost Modeling** (`ra-hardware`, `ra-engine`)
- `NetworkTopology` struct modeling cluster connectivity with bandwidth,
  latency, and cloud billing costs per link
- `LinkType` enum: IntraRack, IntraDatacenter, CrossDatacenter,
  CrossRegion, Internet with realistic default parameters
- 5 predefined topology profiles: single datacenter cluster,
  multi-datacenter, cloud federation (AWS+GCP+Azure), edge+cloud,
  and data warehouse (Snowflake-style)
- `NetworkCostModel` integrating topology with table placement for
  transfer cost estimation
- `DistributionStrategy` with Broadcast, Shuffle, and CoLocated options
- `recommend_join_strategy()` for automatic broadcast vs shuffle selection
- 118 unit tests

**Distribution Strategies** (`ra-core`, `ra-engine`)
- `DataDistribution` modeling for hash, range, broadcast, and replicated
  partitioning
- `DistributedOptimizer` for rewriting plans with distribution-aware
  operators
- 34 optimization rules for join distribution, filter pushdown, partition
  pruning, locality awareness, and skew handling
- 84 unit tests
- Network cost integration: `DistributedOptimizer` uses `NetworkCostModel`
  for topology-aware broadcast vs shuffle decisions
- 26 integration tests

**Distributed Aggregation** (`ra-core`, `ra-stats`)
- Two-phase aggregation: local pre-aggregation + global merge for
  decomposable aggregates (SUM, COUNT, MIN, MAX)
- Three-phase aggregation: adds redistribution phase for high-cardinality
  GROUP BY
- `SkewDetector` identifying skewed key distributions using histograms
  and coefficient of variation
- 25 optimization rules for aggregation pushdown, phase selection,
  and skew-aware strategies
- 170 unit tests
- Integration with `DistributedOptimizer` for automatic two-phase/three-phase
  selection with skew detection
- 35 integration tests

**Federated Queries** (`ra-core`, `ra-engine`)
- `FederatedCostModel` estimating cost of pushing operations to remote
  databases vs pulling data locally
- `FederatedOptimizer` rewriting plans for multi-database execution
- Capability-aware optimization respecting per-database SQL support
- 24 optimization rules for federated pushdown
- 89 unit tests
- Network topology integration: `FederatedCostModel` uses real network costs
  for ShipQuery vs ShipData decisions
- 33 integration tests

**TPC-H Distributed Benchmarks** (`ra-engine`)
- 7 TPC-H queries adapted for distributed execution
- 4 network topologies (single DC, multi-DC, cloud federation, edge+cloud)
- 36 benchmark measurement points

### Metrics

| Metric              | Value     |
|---------------------|-----------|
| Lines of Rust       | ~8,550    |
| Base tests          | 461       |
| Integration tests   | 94        |
| Benchmarks          | 36        |
| New .rra rules      | 59        |
| Documentation pages | 2 new     |
| Crates modified     | 4         |
| Breaking changes    | None      |
