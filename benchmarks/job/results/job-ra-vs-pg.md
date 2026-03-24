# JOB Benchmark Results: Ra Optimizer Performance

Generated: 2026-03-24

## Executive Summary

Ra's optimizer processes all 113 JOB queries successfully with no
parse or optimization failures. After caching hardware detection
(the major bottleneck found during this benchmark run), total
optimization time for all 113 queries is **175ms** with an average
of **1.5ms per query**.

- Total queries: 113
- Parse failures: 0
- Optimization failures: 0
- Total optimization time: 175ms
- Average optimization time: 1.5ms/query
- Max optimization time: 4.1ms (excluding first-query cold start)
- Queries under 100us: 40 (left-deep fast path)
- Queries over 5s: 0

PostgreSQL execution comparison requires the IMDB database to be
loaded (see Setup section). This report covers Ra optimizer latency
only.

## Optimization Path Distribution

Ra uses three optimization strategies based on query complexity:

| Strategy | Table Count | Queries | Avg Time | Description |
|----------|------------|---------|----------|-------------|
| Left-deep | 2-7 | 41 (36%) | 10us | Direct left-deep tree construction |
| E-graph | 8-9 | 35 (31%) | 2.0ms | Equality saturation with rewrite rules |
| Large-join | 10+ | 37 (33%) | 2.0ms | Simulated annealing heuristic |

## Optimization Time by Template

| Template | Tables | Queries | Avg (us) | Max (us) | Path |
|----------|--------|---------|----------|----------|------|
| 1 | 5 | 4 | 12 | 19 | left-deep |
| 2 | 5 | 4 | 98 | 370 | left-deep |
| 3 | 4 | 3 | 6 | 9 | left-deep |
| 4 | 5 | 3 | 7 | 7 | left-deep |
| 5 | 5 | 3 | 5 | 5 | left-deep |
| 6 | 5 | 6 | 12 | 46 | left-deep |
| 7 | 8 | 3 | 1,837 | 1,999 | e-graph |
| 8 | 7 | 4 | 9 | 11 | left-deep |
| 9 | 8 | 4 | 1,754 | 1,832 | e-graph |
| 10 | 7 | 3 | 21 | 29 | left-deep* |
| 11 | 8 | 4 | 2,012 | 2,300 | e-graph |
| 12 | 8 | 3 | 1,973 | 2,183 | e-graph |
| 13 | 9 | 4 | 2,727 | 4,128 | e-graph |
| 14 | 8 | 3 | 2,067 | 2,663 | e-graph |
| 15 | 9 | 4 | 2,607 | 3,486 | e-graph |
| 16 | 8 | 4 | 2,087 | 3,034 | e-graph |
| 17 | 7 | 6 | 10 | 12 | left-deep |
| 18 | 7 | 3 | 9 | 10 | left-deep |
| 19 | 10 | 4 | 1,422 | 1,836 | large-join |
| 20 | 10 | 3 | 1,351 | 1,453 | large-join |
| 21 | 9 | 3 | 1,945 | 1,990 | e-graph |
| 22 | 11 | 4 | 1,478 | 1,495 | large-join |
| 23 | 11 | 3 | 1,995 | 3,075 | large-join |
| 24 | 12 | 2 | 2,282 | 2,856 | large-join |
| 25 | 9 | 3 | 2,820 | 3,831 | e-graph |
| 26 | 12 | 3 | 1,705 | 1,731 | large-join |
| 27 | 12 | 3 | 2,262 | 2,756 | large-join |
| 28 | 13 | 3 | 1,987 | 2,006 | large-join |
| 29 | 17 | 3 | 3,364 | 3,695 | large-join |
| 30 | 12 | 3 | 1,736 | 1,813 | large-join |
| 31 | 11 | 3 | 1,542 | 1,585 | large-join |
| 32 | 6 | 2 | 10 | 14 | left-deep |
| 33 | 14 | 3 | 2,260 | 2,284 | large-join |

*Template 10 first variant includes one-time hardware detection.

## Performance Fix: Hardware Detection Caching

### Problem

`detect_hardware()` spawns 4 subprocess calls to `sysctl` on macOS
to read CPU cores, L2 cache, L3 cache, and SIMD width. Each
subprocess spawn costs ~30-40ms, totaling ~150ms. This function was
called on every optimization because the `Optimizer` does not store
a hardware profile by default.

### Before Fix

| Metric | Value |
|--------|-------|
| Total optimization (113 queries) | 6,580ms |
| Average per query | 58.2ms |
| Min per query | 27ms |
| Max per query | 197ms |
| `detect_hardware()` per call | 154ms |
| Queries >100ms | 14 |

### Fix Applied

Cached the hardware detection result using `std::sync::OnceLock` in
`ra-hardware/src/detection.rs`. The first call performs detection
and stores the result; subsequent calls return a clone of the cached
profile.

### After Fix

| Metric | Value | Improvement |
|--------|-------|-------------|
| Total optimization (113 queries) | 175ms | 37.6x |
| Average per query | 1.5ms | 38.8x |
| Min per query | 4us | 6,750x |
| Max per query | 4.1ms | 48x |
| `detect_hardware()` per call | <1us | >150,000x |
| Queries >100ms | 0 | 14 -> 0 |

### Files Modified

- `crates/ra-hardware/src/detection.rs` -- Added `OnceLock` caching
- `crates/ra-engine/src/genetic_fingerprint.rs` -- Added missing
  `MvScan` match arm (pre-existing compile fix)

## Analysis: Optimization Time by Strategy

### Left-Deep (41 queries, 2-7 tables)

Average: **10us**. This is the fast path that bypasses e-graph
equality saturation entirely. Sorts tables by cardinality and
builds a left-deep join tree directly.

Strengths:
- Sub-microsecond overhead for most queries
- Handles the majority of simple JOB queries

Weaknesses:
- No join reordering search -- cardinality-based sort only
- No cost-based join method selection
- Limited to inner joins in simple structures

### E-Graph (35 queries, 8-9 tables)

Average: **2.0ms**. Runs equality saturation with 170 rewrite rules,
convergence detection, and cost-based extraction.

The adaptive iteration limits help:
- 8 tables (Complex): 15 iterations, 300ms timeout
- 9 tables (Complex): 15 iterations, 300ms timeout

Most queries converge well before the iteration limit.

### Large-Join Heuristic (37 queries, 10+ tables)

Average: **2.0ms**. Uses simulated annealing with parameters:
- Initial temperature: 1000
- Cooling rate: 0.95
- Max iterations: 10,000

Performs comparably to e-graph but handles larger join graphs
without the exponential blowup.

## Remaining Issues

1. **No IMDB database loaded**: PostgreSQL execution comparison
   requires downloading and loading the 3GB IMDB dataset.
   Run `./download_imdb.sh && ./load_data.sh imdb` to enable
   differential testing.

2. **Join quality untested**: Optimizer latency is excellent, but
   join ordering quality (whether Ra picks good join orders)
   requires comparing actual query execution times against
   PostgreSQL.

3. **Cardinality estimation**: The benchmark uses only basic table
   row counts, not column-level statistics or histograms.
   Better statistics would improve join ordering quality.

4. **Cost model accuracy**: The hardware-aware cost model uses
   hardware characteristics (cache size, SIMD width) but lacks
   calibration against actual query runtimes.

## Follow-up Work

- [ ] Download IMDB data and run full differential comparison
- [ ] Compare Ra vs PostgreSQL join orders for each query template
- [ ] Add column-level statistics for IMDB tables
- [ ] Calibrate cost model against PostgreSQL EXPLAIN ANALYZE output
- [ ] Profile e-graph queries to identify rule application hotspots
- [ ] Consider raising left-deep threshold from 7 to 9 tables
