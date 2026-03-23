# Chapter 9: Hardware-Aware Optimization

## Adapting to Your Machine

Alice's ledger runs on different hardware: her laptop for development, a cloud VM for production, and even a Raspberry Pi for the point-of-sale system. RA adapts its optimization strategy based on available hardware resources. Let's explore how CPU, memory, disk, and network characteristics influence query plans.

## Hardware Profile Detection

```sql-interactive
-- RA detects hardware characteristics
SELECT
    current_setting('shared_buffers') as memory_cache,
    current_setting('work_mem') as operation_memory,
    current_setting('effective_cache_size') as os_cache,
    current_setting('max_parallel_workers_per_gather') as cpu_parallelism,
    current_setting('random_page_cost') as random_io_cost,
    current_setting('seq_page_cost') as sequential_io_cost,
    pg_size_pretty(pg_database_size(current_database())) as database_size
FROM (SELECT 1) x;  -- Dummy FROM for compatibility
```

## Interactive Hardware Simulator

```hardware-simulator
{
  "profiles": {
    "laptop": {
      "cpu_cores": 4,
      "memory_gb": 16,
      "disk_type": "SSD",
      "disk_iops": 50000,
      "network_mbps": 100
    },
    "cloud_vm": {
      "cpu_cores": 8,
      "memory_gb": 32,
      "disk_type": "NVMe",
      "disk_iops": 200000,
      "network_mbps": 10000
    },
    "raspberry_pi": {
      "cpu_cores": 4,
      "memory_gb": 4,
      "disk_type": "SD Card",
      "disk_iops": 500,
      "network_mbps": 100
    },
    "enterprise_server": {
      "cpu_cores": 64,
      "memory_gb": 512,
      "disk_type": "RAID 10 SSD",
      "disk_iops": 1000000,
      "network_mbps": 40000
    }
  },
  "current_profile": "laptop",
  "workload": "SELECT * FROM ledger_transactions WHERE amount > 1000"
}
```

Select different profiles and see how RA's plan changes!

## Scenario 1: Memory-Constrained Environment

### Raspberry Pi (4GB RAM)

```sql-interactive
-- Limited work_mem forces disk-based operations
SET work_mem = '4MB';

EXPLAIN (ANALYZE, BUFFERS) SELECT
    account_type,
    COUNT(*) as transactions,
    SUM(debit_amount) as total
FROM ledger_transactions t
JOIN chart_of_accounts a ON t.debit_account_code = a.account_code
GROUP BY account_type
ORDER BY total DESC;
```

**Plan with 4MB work_mem**:
```
Sort (External Merge Disk)  -- Spills to disk!
  `---- HashAggregate (Disk-based)  -- Also spills!
      `---- Hash Join
          |---- Hash (In-memory: 150 rows fit)
          `---- Seq Scan (ledger_transactions)
```

### Cloud VM (32GB RAM)

```sql-interactive
-- Plenty of memory for in-memory operations
SET work_mem = '256MB';

EXPLAIN (ANALYZE, BUFFERS) SELECT
    account_type,
    COUNT(*) as transactions,
    SUM(debit_amount) as total
FROM ledger_transactions t
JOIN chart_of_accounts a ON t.debit_account_code = a.account_code
GROUP BY account_type
ORDER BY total DESC;
```

**Plan with 256MB work_mem**:
```
Sort (In-memory Quicksort)  -- Everything in RAM!
  `---- HashAggregate (In-memory)
      `---- Hash Join (In-memory)
          |---- Hash (In-memory)
          `---- Seq Scan (ledger_transactions)
```

**Performance Impact**:
- Disk-based: 847ms
- Memory-based: 124ms (7x faster!)

## Scenario 2: CPU Parallelism

### Single Core Constraint

```sql-interactive
-- Disable parallel execution
SET max_parallel_workers_per_gather = 0;

EXPLAIN SELECT
    DATE_TRUNC('month', transaction_date) as month,
    COUNT(*) as transactions,
    SUM(debit_amount) as total
FROM ledger_transactions
GROUP BY DATE_TRUNC('month', transaction_date);
```

**Sequential Plan**:
```
HashAggregate
  `---- Seq Scan (ledger_transactions)
      Workers: 0
      Time: 423ms
```

### Multi-Core Power

```sql-interactive
-- Enable parallel execution
SET max_parallel_workers_per_gather = 4;

EXPLAIN SELECT
    DATE_TRUNC('month', transaction_date) as month,
    COUNT(*) as transactions,
    SUM(debit_amount) as total
FROM ledger_transactions
GROUP BY DATE_TRUNC('month', transaction_date);
```

**Parallel Plan**:
```
Finalize HashAggregate
  `---- Gather
      Workers Planned: 4
      `---- Partial HashAggregate
          `---- Parallel Seq Scan
              Workers: 4
              Time: 127ms (3.3x speedup)
```

## Scenario 3: Disk I/O Characteristics

### SD Card (Slow Random I/O)

```sql-interactive
-- High random I/O cost discourages index use
SET random_page_cost = 40;  -- Very expensive random reads
SET seq_page_cost = 1;

EXPLAIN SELECT *
FROM ledger_transactions
WHERE debit_account_code IN ('1010', '1020', '1030')
ORDER BY transaction_date;
```

**Plan for slow random I/O**:
```
Sort
  `---- Seq Scan  -- Avoids index due to random I/O cost!
      Filter: debit_account_code IN (...)
```

### NVMe SSD (Fast Random I/O)

```sql-interactive
-- Low random I/O cost encourages index use
SET random_page_cost = 1.1;  -- Almost as fast as sequential
SET seq_page_cost = 1;

EXPLAIN SELECT *
FROM ledger_transactions
WHERE debit_account_code IN ('1010', '1020', '1030')
ORDER BY transaction_date;
```

**Plan for fast random I/O**:
```
Index Scan  -- Now index is worth it!
  `---- Index: idx_account_date
      Filter: debit_account_code IN (...)
```

## Cost Model Calibration

RA's cost model adapts to hardware:

```javascript
// Hardware-aware cost calculation
class HardwareAwareCostModel {
  constructor(hardware) {
    this.hardware = hardware;
    this.calibrate();
  }

  calibrate() {
    // CPU costs
    this.cpu_tuple_cost = 0.01 / this.hardware.cpu_speed_ghz;
    this.cpu_operator_cost = 0.0025 / this.hardware.cpu_speed_ghz;

    // I/O costs
    if (this.hardware.disk_type === 'SSD') {
      this.random_page_cost = 1.5;
    } else if (this.hardware.disk_type === 'HDD') {
      this.random_page_cost = 4.0;
    } else if (this.hardware.disk_type === 'SD Card') {
      this.random_page_cost = 40.0;
    }

    // Memory costs
    this.hash_mem_cost = 1.0 / this.hardware.memory_bandwidth_gbps;

    // Network costs (for distributed queries)
    this.network_tuple_cost = 0.1 / this.hardware.network_mbps;
  }

  estimateIndexScan(rows, selectivity) {
    const pages = Math.ceil(rows * selectivity / 100);
    const random_io = pages * this.random_page_cost;
    const cpu = rows * selectivity * this.cpu_tuple_cost;
    return random_io + cpu;
  }

  estimateHashJoin(outerRows, innerRows) {
    const buildCost = innerRows * this.hash_mem_cost;
    const probeCost = outerRows * this.cpu_operator_cost;
    const memoryNeeded = innerRows * 64; // bytes per row

    if (memoryNeeded > this.hardware.work_mem) {
      // Disk-based hash join
      return buildCost + probeCost + (memoryNeeded / 1048576) * this.random_page_cost;
    } else {
      // In-memory hash join
      return buildCost + probeCost;
    }
  }
}
```

## Memory Management Strategies

### Strategy 1: Work Memory Allocation

```sql-interactive
-- Check current memory settings
SHOW work_mem;  -- Per-operation memory
SHOW shared_buffers;  -- Shared cache
SHOW effective_cache_size;  -- Total cache estimate

-- Adjust for large aggregation
SET work_mem = '256MB';  -- Just for this session

-- Complex aggregation query
WITH monthly_summary AS (
    SELECT
        DATE_TRUNC('month', transaction_date) as month,
        debit_account_code,
        SUM(debit_amount) as total,
        COUNT(*) as count,
        AVG(debit_amount) as average,
        STDDEV(debit_amount) as stddev
    FROM ledger_transactions
    GROUP BY DATE_TRUNC('month', transaction_date), debit_account_code
)
SELECT
    month,
    COUNT(DISTINCT debit_account_code) as accounts,
    SUM(total) as grand_total,
    AVG(average) as avg_transaction
FROM monthly_summary
GROUP BY month
ORDER BY month;
```

### Strategy 2: Buffer Pool Management

```sql-interactive
-- Monitor buffer usage
SELECT
    schemaname,
    tablename,
    pg_size_pretty(pg_relation_size(schemaname||'.'||tablename)) as size,
    heap_blks_read as disk_reads,
    heap_blks_hit as cache_hits,
    ROUND(100.0 * heap_blks_hit / NULLIF(heap_blks_hit + heap_blks_read, 0), 2) as cache_hit_ratio
FROM pg_statio_user_tables
ORDER BY heap_blks_read + heap_blks_hit DESC;
```

### Strategy 3: Parallel Memory Distribution

```sql-interactive
-- Parallel workers share memory
SET max_parallel_workers_per_gather = 4;
SET min_parallel_table_scan_size = '8MB';
SET parallel_setup_cost = 100;  -- Lower for faster CPUs

-- Each worker gets work_mem
-- Total memory = work_mem * (1 + parallel_workers)
```

## CPU Optimization Techniques

### SIMD Operations

```sql-interactive
-- RA can use SIMD for batch operations
SELECT
    SUM(debit_amount),  -- Vectorized sum
    AVG(debit_amount),  -- Vectorized average
    MIN(debit_amount),  -- Vectorized min
    MAX(debit_amount)   -- Vectorized max
FROM ledger_transactions
WHERE debit_amount > 0;  -- Simple filter for SIMD
```

### CPU Cache Optimization

```sql-interactive
-- Column-wise better than row-wise for cache
-- Bad: Causes cache misses
SELECT *  -- Loads entire row
FROM ledger_transactions
WHERE debit_amount > 1000;

-- Good: Better cache utilization
SELECT debit_amount, transaction_date  -- Only needed columns
FROM ledger_transactions
WHERE debit_amount > 1000;
```

## Network-Aware Optimization

For distributed databases:

```sql-interactive
-- Minimize network transfer
-- Bad: Transfer all data then filter
SELECT *
FROM remote_transactions
WHERE amount > 1000;

-- Good: Filter remotely
SELECT transaction_id, amount
FROM remote_transactions
WHERE amount > 1000;  -- Pushed to remote

-- Best: Aggregate remotely
SELECT
    account_code,
    SUM(amount) as total
FROM remote_transactions
WHERE amount > 1000
GROUP BY account_code;  -- Aggregation at source
```

## Hardware Profiling Queries

### CPU Profiling

```sql-interactive
-- Find CPU-intensive queries
SELECT
    query,
    calls,
    total_exec_time,
    mean_exec_time,
    stddev_exec_time,
    rows,
    100.0 * total_exec_time / SUM(total_exec_time) OVER () as percent_cpu
FROM pg_stat_statements
WHERE query NOT LIKE '%pg_stat%'
ORDER BY total_exec_time DESC
LIMIT 10;
```

### I/O Profiling

```sql-interactive
-- Find I/O-intensive operations
SELECT
    query,
    blk_read_time as disk_read_ms,
    blk_write_time as disk_write_ms,
    shared_blks_hit as cache_hits,
    shared_blks_read as cache_misses,
    ROUND(100.0 * shared_blks_hit / NULLIF(shared_blks_hit + shared_blks_read, 0), 2) as cache_hit_ratio
FROM pg_stat_statements
ORDER BY blk_read_time + blk_write_time DESC
LIMIT 10;
```

## Adaptive Execution

RA can adapt plans during execution:

```sql-interactive
-- Enable adaptive execution
SET enable_adaptive_execution = on;

-- Query with uncertain selectivity
SELECT *
FROM ledger_transactions t1
JOIN ledger_transactions t2 ON t1.ref_id = t2.id
WHERE t1.amount > ?;  -- Unknown parameter

-- RA's adaptive strategy:
-- 1. Start with nested loop (good for small result)
-- 2. If too many rows, switch to hash join
-- 3. If memory exhausted, switch to merge join
```

## Hardware-Specific Optimizations

### For SSDs

```sql
-- Optimize for SSDs
random_page_cost = 1.1
effective_io_concurrency = 200  -- SSDs handle parallel I/O well
```

### For HDDs

```sql
-- Optimize for spinning disks
random_page_cost = 4.0
effective_io_concurrency = 2  -- Limited by disk heads
```

### For Cloud Storage

```sql
-- Optimize for network-attached storage
random_page_cost = 2.0
effective_io_concurrency = 10
-- Consider latency in cost model
```

## Practice Exercises

### Exercise 1: Tune for Hardware

Given this hardware profile, what settings would you use?

```
Hardware:
- CPU: 16 cores @ 3.5GHz
- RAM: 64GB
- Disk: RAID 10 SSDs
- Network: 10Gbps

Workload:
- 80% analytical queries (aggregations)
- 20% transactional queries
- Database size: 100GB
```

Your settings:
```sql
-- shared_buffers = ?
-- work_mem = ?
-- maintenance_work_mem = ?
-- effective_cache_size = ?
-- random_page_cost = ?
-- max_parallel_workers = ?
```

### Exercise 2: Diagnose Hardware Bottleneck

```sql-interactive
-- Query runs slowly. Which hardware component is the bottleneck?
EXPLAIN (ANALYZE, BUFFERS) SELECT
    a.account_type,
    DATE_TRUNC('week', t.transaction_date) as week,
    COUNT(DISTINCT t.id) as transactions,
    COUNT(DISTINCT t.debit_account_code) as unique_accounts,
    PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY t.debit_amount) as median
FROM ledger_transactions t
JOIN chart_of_accounts a ON t.debit_account_code = a.account_code
WHERE t.transaction_date >= CURRENT_DATE - 90
GROUP BY a.account_type, DATE_TRUNC('week', t.transaction_date);

-- Analyze output:
-- Buffers: shared hit=1203 read=45123  --> Disk I/O issue?
-- Sort Method: external merge Disk: 24576kB  --> Memory issue?
-- Execution Time: 8234.123 ms  --> CPU issue?
```

## Key Takeaways

1. **Hardware characteristics drive optimization**
   - Memory determines join strategy
   - CPU cores enable parallelism
   - Disk type affects index decisions

2. **Cost models must match hardware**
   - Calibrate for your specific setup
   - Random vs sequential I/O costs
   - CPU and memory speeds

3. **Memory is often the constraint**
   - In-memory operations are 10-100x faster
   - Spilling to disk kills performance
   - Buffer cache hit ratio is critical

4. **Parallelism scales with cores**
   - Near-linear speedup for scans
   - Aggregations parallelize well
   - Joins are harder to parallelize

5. **I/O patterns matter**
   - Sequential reads are fast
   - Random reads vary by disk type
   - Minimize I/O when possible

## Next Steps

We've covered individual optimization techniques. Now let's explore advanced features that combine everything we've learned. In [Chapter 10: Advanced Features](10-advanced-features.md), we'll see covering indexes, bitmap scans, and cutting-edge optimizations.

---

* Performance Tip: The fastest I/O is no I/O. Use covering indexes and increase memory to avoid disk access entirely. When you must read from disk, sequential is 10-100x faster than random on HDDs.*