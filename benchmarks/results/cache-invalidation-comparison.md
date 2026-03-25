# Cache Invalidation: Polling vs Differential (RFC 0059)

Benchmark results comparing polling-based (per-access) cache invalidation
against event-driven differential dataflow invalidation.

## Test Setup

- Machine: Apple Silicon (M-series), single-threaded
- Benchmark: `cargo bench --bench cache_invalidation_bench`
- Workload: N cached plans, each depending on one table

## Results

### Per-Access Polling Cost

Polling checks ALL plan dependencies on every cache access:

| Cached plans | Per-access check | Cost per 1M accesses |
|---|---|---|
| 10 | 203 ns | 203 ms |
| 100 | 3.8 us | 3.8 s |
| 1000 | 37.5 us | 37.5 s |

### Differential Invalidation Cost (One-Time)

Differential dataflow computes affected plans once per change event:

| Cached plans | One-time compute | Cost per 100 changes/hr |
|---|---|---|
| 10 | 65 us | 6.5 ms/hr |
| 100 | 141 us | 14.1 ms/hr |
| 1000 | 1.23 ms | 123 ms/hr |

### Cache Lookup (O(1) for both approaches)

| Cached plans | Exact hit latency |
|---|---|
| 10 | 67 ns |
| 100 | 94 ns |
| 1000 | 90 ns |

### End-to-End Pipeline: detect + compute + invalidate

| Cached plans | Full pipeline |
|---|---|
| 10 | 65 us |
| 100 | 141 us |

## Analysis

### OLTP Scenario: 1M queries/sec, 100 ANALYZE operations/hour

**Polling (current):**
- 1M plans x 37.5 us/check = 37.5 seconds of CPU per second
- That is 37.5x overhead on every query execution path

**Differential (RFC 0059):**
- 100 changes/hr x 1.23 ms/change = 123 ms/hr total overhead
- Per-query overhead: 0 (no per-access checking)
- Cache lookup remains O(1) at ~90 ns

**Overhead reduction:**

For a 1M q/s workload with 100 ANALYZE events/hour:
- Polling: ~37.5 CPU-seconds per real-time second
- Differential: 123 ms per hour = 0.000034 CPU-seconds per second
- Ratio: >1,000,000x reduction in invalidation overhead

### Key Properties

1. **O(1) lookup**: Both approaches have identical O(1) cache lookup time
2. **Zero per-access cost**: Differential moves all staleness checking
   off the hot path
3. **Precision**: Only plans depending on changed resources are
   invalidated, not the entire cache
4. **Soft invalidation**: Hot entries are marked stale instead of evicted,
   preserving cache warmth while signaling the need for reoptimization
