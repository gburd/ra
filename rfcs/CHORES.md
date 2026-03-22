# RA Optimizer Chores & Small Tasks

Tasks too small for an RFC but necessary for project completion, organized by subsystem and priority.

**Priority Levels:**
- **P0**: Blocking / Critical
- **P1**: High Priority
- **P2**: Medium Priority
- **P3**: Low Priority / Nice to Have

---

## ra-cli (CLI Tool)

### P0 - Critical
- [ ] Fix 3 failing tests in `migrate_commands.rs` (float_threshold_narrowing, narrowed_threshold_detected, optional_to_required_is_data_loss_risk)
- [ ] Re-enable or properly implement regression commands (currently stubbed out)

### P1 - High Priority
- [ ] Add `--explain` flag to show optimizer decisions
- [ ] Add `--stats` output format (JSON, YAML, text)
- [ ] Implement `ra-cli analyze` command for query analysis without optimization
- [ ] Add `--version` with build info (git commit, build date, features)

### P2 - Medium Priority
- [ ] Add shell completion generation (`ra-cli completions bash/zsh/fish`)
- [ ] Improve error messages with suggestions (did you mean...?)
- [ ] Add `--dry-run` mode for optimization preview
- [ ] Color output for terminal (use `termcolor` crate)

### P3 - Nice to Have
- [ ] Interactive mode (`ra-cli interactive`) - REPL for queries
- [ ] Add `--benchmark` mode for performance testing
- [ ] Progress bar for long-running optimizations

---

## ra-pg-extension (PostgreSQL Extension)

### P0 - Critical
- [ ] Run integration tests with real PostgreSQL: `cargo pgrx test pg17`
- [ ] Test extension with TPC-H queries
- [ ] Verify MVCC/HOT statistics gathering works correctly
- [ ] Test crash recovery (extension unload/reload)

### P1 - High Priority
- [ ] Add GUC variables for tuning (enable_ra_optimizer, ra_cost_threshold, etc.)
- [ ] Implement EXPLAIN ANALYZE integration (show RA costs vs PostgreSQL costs)
- [ ] Add logging with configurable levels (DEBUG, INFO, WARN, ERROR)
- [ ] Performance benchmarking against PostgreSQL planner (TPC-H, TPC-DS)

### P2 - Medium Priority
- [ ] Add statistics staleness detection (warn if ANALYZE needed)
- [ ] Implement plan caching (avoid re-optimization for identical queries)
- [ ] Add monitoring view: `pg_stat_ra_optimizer` (queries optimized, time spent, improvements)
- [ ] Support parameterized queries ($1, $2, ...) properly

### P3 - Nice to Have
- [ ] Web UI for viewing optimizer decisions
- [ ] Integration with pg_stat_statements
- [ ] Add regression test suite matching PostgreSQL's

---

## ra-core (Core Library)

### P0 - Critical
- [ ] Verify all `RelExpr` variants are handled in pattern matches (no `_` wildcards)
- [ ] Add validation: ensure `Statistics` fields are consistent
- [ ] Document public API with examples

### P1 - High Priority
- [ ] Add `RelExpr::validate()` method (check query tree invariants)
- [ ] Implement `Display` trait for `RelExpr` (pretty-print queries)
- [ ] Add `RelExpr::size()` method (count nodes in tree)
- [ ] Add `RelExpr::complexity()` metric for cost estimation

### P2 - Medium Priority
- [ ] Add builder pattern for complex `RelExpr` construction
- [ ] Implement `Clone` more efficiently (use Rc/Arc for large subtrees?)
- [ ] Add `RelExpr::walk()` iterator for tree traversal
- [ ] Add `RelExpr::transform()` method for bottom-up rewrites

### P3 - Nice to Have
- [ ] Implement `serde` for `RelExpr` serialization
- [ ] Add `RelExpr::fingerprint()` for plan caching
- [ ] Support for custom operators (user-defined RelExpr nodes)

---

## ra-engine (Optimization Engine)

### P0 - Critical
- [ ] Verify cost model parameters are sensible (CPU_TUPLE_COST, IO_COST, etc.)
- [ ] Test e-graph extraction with various node limits
- [ ] Ensure rules don't create infinite loops (test with --iter-limit 1000)

### P1 - High Priority
- [ ] Add rule statistics tracking (which rules fire most often, which improve cost most)
- [ ] Implement timeout handling (return best-so-far plan if optimization times out)
- [ ] Add checkpointing (save e-graph state periodically for long optimizations)
- [ ] Better error messages when rules fail to apply

### P2 - Medium Priority
- [ ] Parallelize rule application (use rayon for independent rewrites)
- [ ] Implement adaptive iteration limit (stop early if no improvements for N iterations)
- [ ] Add debug mode that explains why rules didn't fire
- [ ] Optimize e-graph memory usage (current implementation uses ~100MB for complex queries)

### P3 - Nice to Have
- [ ] Visualization of e-graph evolution (export to GraphViz DOT format)
- [ ] Rule conflict detection (warn if rules interfere with each other)
- [ ] Machine learning for rule ordering (learn which rules to try first)

---

## ra-parser (SQL Parser)

### P0 - Critical
- [ ] Test parser with 1000+ real-world queries from TPC-H, TPC-DS
- [ ] Handle all PostgreSQL syntax (currently missing some CTEs, window functions)
- [ ] Add error recovery (don't fail entire parse on syntax error)

### P1 - High Priority
- [ ] Support all SQL dialects (MySQL, Oracle, SQL Server differences)
- [ ] Add parser for DDL (CREATE TABLE, CREATE INDEX for metadata inference)
- [ ] Better error messages with line/column numbers and suggestions
- [ ] Add parser benchmarks (queries per second)

### P2 - Medium Priority
- [ ] Support vendor-specific extensions (PostgreSQL :: casting, MySQL backticks)
- [ ] Parse optimizer hints (/*+ INDEX(t idx) */)
- [ ] Add parser for EXPLAIN output (for comparing with other systems)
- [ ] Fuzzing with cargo-fuzz to find parser bugs

### P3 - Nice to Have
- [ ] Auto-formatting of SQL queries (pretty-printer)
- [ ] SQL syntax highlighting for errors
- [ ] Support for embedded SQL (EXEC SQL ...)

---

## ra-dialect (Dialect Translation)

### P0 - Critical
- [ ] Test translation round-trips (PostgreSQL → RA → MySQL → RA → PostgreSQL)
- [ ] Verify all function mappings are correct
- [ ] Add validation tests for unsupported features

### P1 - High Priority
- [ ] Add more database dialects (Oracle, SQL Server, DB2, Snowflake, BigQuery)
- [ ] Document dialect differences (what works, what doesn't)
- [ ] Add translation quality metrics (how many queries translate perfectly?)
- [ ] Support for dialect-specific optimizations

### P2 - Medium Priority
- [ ] Add dialect auto-detection from query syntax
- [ ] Translation warnings (this feature is not supported in target dialect)
- [ ] Add SQL Server T-SQL specifics (@@ROWCOUNT, OUTPUT clause)
- [ ] Add Oracle PL/SQL specifics

### P3 - Nice to Have
- [ ] Translation UI (paste query, select dialects, show translation)
- [ ] Community-contributed dialect translations

---

## ra-stats (Statistics System)

### P0 - Critical
- [ ] Test timeline playback with 100+ timeline files
- [ ] Verify statistics interpolation is correct
- [ ] Add validation for statistics files (detect corruption)

### P1 - High Priority
- [ ] Add statistics export/import (for sharing benchmarks)
- [ ] Support for streaming statistics updates (live data)
- [ ] Add statistics decay (older stats count less in decisions)
- [ ] Performance: optimize statistics lookup (currently O(n) for n timelines)

### P2 - Medium Priority
- [ ] Add statistics visualization (charts of cardinality over time)
- [ ] Automatic outlier detection (warn about unusual statistics)
- [ ] Statistics compression (delta encoding for timelines)
- [ ] Add statistics diff tool (compare two timeline files)

### P3 - Nice to Have
- [ ] Machine learning for statistics forecasting
- [ ] Integration with real database statistics (pg_stats, MySQL information_schema)

---

## ra-hardware (Hardware Detection)

### P0 - Critical
- [ ] Test on various hardware (Intel, AMD, ARM, Apple Silicon)
- [ ] Verify CPU cache detection is accurate
- [ ] Test storage type detection (SSD vs HDD vs NVMe)

### P1 - High Priority
- [ ] Add calibration tool (measure actual hardware performance)
- [ ] Support for custom hardware profiles (user-defined)
- [ ] Add GPU detection (for future GPU-accelerated queries)
- [ ] Better CPU topology detection (NUMA, hyperthreading)

### P2 - Medium Priority
- [ ] Add network bandwidth detection (for distributed queries)
- [ ] Detect virtualization (VM vs bare metal) and adjust costs
- [ ] Add memory bandwidth detection (important for OLAP)
- [ ] Profile disk latency (random vs sequential I/O)

### P3 - Nice to Have
- [ ] Auto-tuning based on hardware (set optimal parallelism, buffer sizes)
- [ ] Hardware performance regression detection (disk getting slower?)

---

## ra-web (Web UI)

### P0 - Critical
- [ ] Fix web UI (currently showing blank page)
- [ ] Test with 100+ queries
- [ ] Verify query comparison works correctly

### P1 - High Priority
- [ ] Add query history (save optimized queries)
- [ ] Export optimization report (PDF, HTML)
- [ ] Add dark mode
- [ ] Responsive design (mobile-friendly)

### P2 - Medium Priority
- [ ] Add query templates (common patterns)
- [ ] Real-time optimization progress (WebSocket updates)
- [ ] Add query sharing (generate shareable links)
- [ ] Syntax highlighting for SQL

### P3 - Nice to Have
- [ ] Query builder UI (visual query construction)
- [ ] Integration with BI tools
- [ ] Collaborative query editing (multiple users)

---

## ra-tui (Terminal UI)

### P0 - Critical
- [ ] Verify ASCII recording works (`cargo run -- tui --record test.cast`)
- [ ] Test timeline playback with various speeds
- [ ] Fix any rendering glitches

### P1 - High Priority
- [ ] Add keyboard shortcuts help (press '?' to show help)
- [ ] Better error display (show full error messages, not just first line)
- [ ] Add query history navigation (up/down arrows)
- [ ] Export current view to text file

### P2 - Medium Priority
- [ ] Add zoom controls for large query plans
- [ ] Configurable color schemes
- [ ] Add search functionality (find text in plan)
- [ ] Mouse support (click to expand/collapse nodes)

### P3 - Nice to Have
- [ ] Split-pane mode (compare two queries side-by-side)
- [ ] Animation of optimization steps (show rule applications)

---

## Documentation

### P0 - Critical
- [ ] Fix all broken links in documentation
- [ ] Verify code examples compile and run
- [ ] Add "Getting Started" tutorial

### P1 - High Priority
- [ ] Document all public APIs with examples
- [ ] Add architecture diagram (how components interact)
- [ ] Create maintainer's guide (for contributors)
- [ ] Add troubleshooting section (common errors and fixes)

### P2 - Medium Priority
- [ ] Add video tutorials
- [ ] Create FAQ page
- [ ] Add performance tuning guide
- [ ] Document all configuration options

### P3 - Nice to Have
- [ ] Interactive API explorer
- [ ] Case studies (real-world usage)
- [ ] Add search functionality to docs

---

## Build System & CI

### P0 - Critical
- [ ] Set up continuous benchmarking (track performance over time)
- [ ] Add coverage tracking (aim for >80% coverage)
- [ ] Set up automated releases (tag → build → publish)

### P1 - High Priority
- [ ] Add cargo-deny to CI (check dependencies for vulnerabilities)
- [ ] Add cargo-outdated to CI (warn about outdated dependencies)
- [ ] Set up nightly builds
- [ ] Add build time tracking (detect slow compile times)

### P2 - Medium Priority
- [ ] Add cross-compilation (Linux, macOS, Windows)
- [ ] Docker images for easy deployment
- [ ] Add benchmarking in CI (fail if performance regresses >10%)
- [ ] Add mutation testing (cargo-mutants)

### P3 - Nice to Have
- [ ] Binary size optimization
- [ ] Compile time optimization
- [ ] Add automated security audits

---

## Testing

### P0 - Critical
- [ ] Run TPC-H benchmark (all 22 queries)
- [ ] Run TPC-DS benchmark (all 99 queries)
- [ ] Add fuzzing for parser and optimizer
- [ ] Test with 1M+ row tables

### P1 - High Priority
- [ ] Add property-based tests (proptest) for optimizer correctness
- [ ] Add performance regression tests (fail if 10%+ slower)
- [ ] Test error handling (what happens when out of memory?)
- [ ] Add stress tests (1000 concurrent queries)

### P2 - Medium Priority
- [ ] Add integration tests with real databases (PostgreSQL, MySQL, SQLite)
- [ ] Test with malformed/adversarial queries
- [ ] Add memory leak detection (valgrind, heaptrack)
- [ ] Test distributed execution with network failures

### P3 - Nice to Have
- [ ] Add chaos engineering tests (random failures)
- [ ] Test with different data distributions (uniform, skewed, zipfian)

---

## Rules & Optimizations

### P0 - Critical
- [ ] Verify all 1,354 rules have tests
- [ ] Check for rule conflicts (rules that undo each other)
- [ ] Add precondition validation for all rules

### P1 - High Priority
- [ ] Add rule categories to INDEX.md
- [ ] Document rule interaction (which rules work together?)
- [ ] Add rule performance metrics (which rules are slow?)
- [ ] Verify all rules have correct cost models

### P2 - Medium Priority
- [ ] Add rule templates (make it easy to add new rules)
- [ ] Add rule linting (check for common mistakes)
- [ ] Add rule visualization (show rule application order)
- [ ] Group similar rules (filter pushdown family, join reordering family)

### P3 - Nice to Have
- [ ] Machine learning for rule selection
- [ ] User-defined rules (custom optimizations)
- [ ] Rule marketplace (share rules with community)

---

## Missing RFCs

These gaps need RFCs (not small chores):

### High Priority Missing RFCs
1. **Semi-Join Reduction** (Gap #2 from analysis)
   - For distributed queries, reduce network traffic
   - Estimated effort: 2-3 weeks

2. **Distinct Aggregation Rewrite** (Gap #4 from analysis)
   - Optimize multiple COUNT(DISTINCT ...) in one query
   - Estimated effort: 1-2 weeks

3. **Skip Scan (Loose Index Scan)** (Gap #9 from analysis)
   - Use composite index when leading column not filtered
   - Already has RFC 0038, needs verification

4. **Partial Aggregation (Two-Phase)** (Gap #6 from analysis)
   - Verify if already implemented in parallel execution
   - If not, estimated effort: 2-3 weeks

### Medium Priority Missing RFCs
5. **Decorrelation Improvements**
   - Verify gap #10 (nested aggregates), may need RFC

6. **CMU Video Research Extraction**
   - Create RFCs from Andy Pavlo lectures (20-30 notes → 5-10 RFCs)

7. **pg_plan_advice Integration** (RFC 0003 exists, needs implementation plan)

8. **Monitoring System Completion** (RFC 0012 exists, partial implementation)

---

## Next Steps

1. **Address P0 items** in each subsystem (critical blockers)
2. **Create missing RFCs** for semi-join reduction, distinct aggregation rewrite
3. **Fix ra-web** (blank page issue)
4. **Run integration tests** for ra-pg-extension
5. **Fix failing tests** in ra-cli

---

Last Updated: 2026-03-22
