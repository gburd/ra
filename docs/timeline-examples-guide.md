# Timeline Examples Guide

Walkthroughs of the plan evolution timelines in `timelines/`. Each timeline tells a story about how the optimizer adapts to changing data characteristics.

## Join Reordering Cascade

**File:** `timelines/join-reordering-cascade.toml`
**Duration:** 6 hours, 10 snapshots

### Story

An e-commerce analytics query joins 5 tables: `fact_transactions`, `dim_products`, `dim_stores`, `dim_customers`, and `promotions`. The `promotions` table starts tiny (50 rows), so the optimizer uses it as the first join input in a left-deep plan -- joining the smallest table first eliminates the most rows early.

A seasonal campaign causes the promotions table to grow 10,000x (50 -> 500K rows). This invalidates the join ordering assumption:

1. **t=0h:** Left-deep, promotions-first (50 rows). Q-error: 1.01
2. **t=0.75h:** Campaign starts. Stats still show 50 promos, but 5K exist. Estimates start diverging.
3. **t=1.5h:** 50K promotions. Optimizer still uses old join order. Q-error: 1.78
4. **t=2h:** ANALYZE detects growth. Switches to products-first join order. Q-error drops to 1.03.
5. **t=3h:** Campaign peak (500K promos). Products-first also suboptimal. Q-error: 1.77
6. **t=3.5h:** ANALYZE + reoptimize -> bushy join tree: `(fact x stores) JOIN (promos x products)`. Q-error: 1.02
7. **t=4.5h-5h:** Campaign winds down. 495K promos purged. Bushy plan now wasteful.
8. **t=5.5h:** ANALYZE detects cleanup. Reverts to left-deep promotions-first. Q-error: 1.03

### What to observe

- The promotions row count in the statistics panel traces a mountain shape
- Three distinct join orderings appear in feedback operator names
- Stale periods (high Q-error) precede each reoptimize event
- The final plan matches the initial plan -- full cycle

## IndexScan vs SeqScan Transitions

**File:** `timelines/index-vs-seqscan.toml`
**Duration:** 2 hours, 8 snapshots

### Story

A web analytics `http_requests` table has a B-tree index on the `status` column. The query `WHERE status = 'error'` initially matches only 0.5% of rows, making the IndexScan highly efficient.

A production incident causes the error rate to spike to 15%. At this selectivity, the index becomes a liability -- random I/O from index lookups costs more than a sequential scan.

1. **t=0:** 100K rows, 0.5% errors -> IndexScan. 8ms execution.
2. **t=15m:** 500K rows, 1% errors -> IndexScan still optimal. 35ms.
3. **t=30m:** 2M rows, 8% error rate spike. Stats stale, still using IndexScan. 850ms -- IndexScan doing massive random I/O.
4. **t=45m:** 4M rows, 15% errors. IndexScan with stale stats: 3200ms (catastrophically slow).
5. **t=1h:** ANALYZE confirms 15% selectivity. Switches to SeqScan. 1800ms -- much better for this selectivity.
6. **t=1h15m:** Bug fix deployed, old errors purged. SeqScan using stale stats (overestimates errors). 1500ms.
7. **t=1h30m:** VACUUM + ANALYZE -> error rate confirmed at 2%. Back to IndexScan. 45ms.
8. **t=2h:** Stable at 1.5% errors. IndexScan confirmed optimal. 40ms.

### What to observe

- Execution time (actual_time_ms) in feedback shows the performance cliff when IndexScan is wrong
- The Q-error metric jumps to 240x during the stale IndexScan period (estimated 2500 rows, actual 600K)
- After ANALYZE, Q-error drops back to ~1.0
- The operator name in feedback explicitly tracks scan type transitions

## Aggregation Strategy Evolution

**File:** `timelines/aggregation-strategy-evolution.toml`
**Duration:** 3 hours, 11 snapshots

### Story

An IoT platform runs hourly aggregations grouped by `(device_id, hour)`. The number of groups determines whether HashAgg or GroupAgg (sort-based) is more efficient. PostgreSQL's `work_mem` setting controls the hash table budget.

1. **t=0:** 1K devices, 24K groups. HashAgg fits easily in 64MB work_mem. 2.8s.
2. **t=15m:** 5K devices, 120K groups. HashAgg nearing limit. 8.5s.
3. **t=30m:** 20K devices, 480K groups. HashAgg spilling to disk! 25s.
4. **t=45m:** ANALYZE -> optimizer switches to GroupAgg (sort-based). 18s -- sort is cheaper than disk-spilling hash.
5. **t=1h:** 30K devices. GroupAgg stable. 22s.
6. **t=1.5h:** DBA increases work_mem to 256MB. ANALYZE + reoptimize -> HashAgg returns. 15s.
7. **t=2h:** Fleet scales to 100K devices. Even 256MB insufficient. HashAgg spilling again. 45s.
8. **t=2h15m:** ANALYZE -> 2-phase partial GroupAgg: workers pre-aggregate, then final HashAgg merges. 28s.
9. **t=2.5h:** Old data archived (85M -> 30M rows). 2-phase now wasteful.
10. **t=2h45m:** ANALYZE -> back to HashAgg (smaller table fits). 10s.
11. **t=3h:** Steady state. HashAgg with 256MB work_mem. 10.5s.

### What to observe

- Device count growth drives group count growth, which drives strategy changes
- Three distinct aggregation strategies: HashAgg, GroupAgg, 2-phase partial
- The work_mem increase (schema_change event) enables a strategy switch without data changes
- Archive deletion shows that data reduction can restore simpler plans

## Partition Pruning Effectiveness

**File:** `timelines/partition-pruning-effectiveness.toml`
**Duration:** 4 hours, 8 snapshots

### Story

A time-partitioned event log stores data in monthly partitions. A query filters for October 2025 events. With accurate statistics, the optimizer prunes all non-matching partitions. As the October partition grows 6x (5M -> 30M rows) without ANALYZE, the cost estimate becomes wildly inaccurate.

1. **t=0:** 5 uniform partitions (5M each). Query scans only October. Q-error: 1.0. 3.5s.
2. **t=30m:** October grows to 15M (3x). Stats still say 5M. Q-error: 3.0. 9.8s.
3. **t=1h:** October at 25M (5x). Stats catastrophically stale. Q-error: 5.0. 16s.
4. **t=1.5h:** ANALYZE on October. Fresh stats fix the cost model. Q-error: 1.0. 18.5s (accurate estimate, just a big partition).
5. **t=2h:** October split into 4 weekly sub-partitions (w1-w4). Now a query for "October 8-14" can prune to a single week.
6. **t=2.5h:** Old partitions (January, June) archived. Fewer total partitions to consider.
7. **t=3h:** ANALYZE on all weekly sub-partitions. Optimal pruning restored.
8. **t=4h:** Steady state. Weekly partitions enable fine-grained pruning.

### What to observe

- The schema_change event represents the partition split -- a structural optimization
- Partition names change mid-timeline: `event_log_2025_10` splits into `event_log_2025_10_w1` through `w4`
- Tables appearing/disappearing between snapshots reflects the partitioning evolution
- Q-error peaks at 5.0x during the stale period, which represents 5x more rows scanned than estimated
- After the partition split + ANALYZE, Q-error stabilizes near 1.0

## Viewing Timelines

Load any timeline in the TUI:

```bash
cargo run --bin ra-cli -- tui --demo
```

Or play through snapshots on the command line:

```bash
cargo run --bin ra-cli -- stats-timeline play \
    --timeline timelines/join-reordering-cascade.toml
```

In the TUI, use arrow keys to step through snapshots. The statistics panel shows per-table row counts and the events panel shows what happened between snapshots.
