# CMU 15-721 Lecture 9: Hash Join Algorithms

**Source:** CMU 15-721 Spring 2024, Lecture 9
**Speaker:** Andy Pavlo
**Topic:** Hash Join Algorithms for Analytical Workloads

## Key Concepts

### Hash Join Variants

#### Simple Hash Join
- Build hash table from inner (smaller) relation
- Probe with each tuple from outer relation
- Memory: must fit inner relation in memory
- Time: O(n + m) for build + probe

#### Grace Hash Join
- Both relations too large for memory
- Partition both relations by hash of join key
- Join matching partitions independently
- Recursive partitioning if partition still too large
- I/O: 3x (read + write + read) each relation

#### Hybrid Hash Join
- Optimization of Grace: keep one partition in memory
- First partition never spilled, joined during build phase
- Reduces I/O compared to pure Grace hash join
- Memory-adaptive: adjust in-memory partition count

### Parallel Hash Join

#### Shared Hash Table
- All workers build into single shared hash table
- Requires concurrent hash table (lock-free or partitioned)
- Workers then parallel probe against shared table
- Build phase is pipeline breaker (startup cost)

#### Partitioned Hash Join
- Partition both sides by hash into P partitions
- Each worker handles subset of partitions
- No shared hash table needed
- Better cache locality
- Extra cost: partitioning pass

#### Radix Hash Join (Best for analytics)
- Multi-pass radix clustering to achieve cache alignment
- Pass 1: partition by high bits of hash (L2 cache sized)
- Pass 2: partition by low bits (L1 cache sized)
- Then build + probe entirely within cache
- Highest throughput for large analytical joins
- Higher latency (multiple partitioning passes)

### Skew Handling
- Partition skew: some partitions much larger than others
- Hot key problem: single join key value has millions of matches
- Solutions:
  1. **Histogram-based partitioning**: Use statistics to create even partitions
  2. **Overflow partitioning**: Spill oversized partitions to secondary
  3. **Replication**: Replicate small-side hot keys to all workers
  4. **Range partitioning**: Use value ranges instead of hash for known skew

### Runtime Filters (Bloom Filters from Hash Join)
- During build phase, create bloom filter on join keys
- Push bloom filter to probe-side scan
- Filter out non-matching tuples before they enter probe pipeline
- Especially effective for star schema joins
- Cost: ~1 bit per distinct key (very cheap)
- Benefit: 10-100x reduction in probe-side data

## Applicable to Ra

### New Rule Ideas
1. **Hash Join Variant Selection**: Choose simple vs Grace vs hybrid based
   on estimated inner relation size relative to memory budget.
2. **Parallel Hash Join Strategy**: Choose shared table vs partitioned
   based on partition count, cache size, and data distribution.
3. **Radix Join Selection**: For large analytical joins (> 10M rows),
   prefer radix hash join for cache efficiency.
4. **Skew Detection Rule**: When outer side has high-frequency join keys
   (from MCV list), add skew handling to hash join plan.
5. **Bloom Filter Runtime Filter**: Generate bloom filter during hash join
   build, push to probe-side scan operator.
6. **Hash Table Size Estimation**: Estimate hash table memory requirement
   to decide between in-memory and Grace hash join.

### Gap Analysis
- Ra has basic hash join cost model
- Missing: Grace/hybrid hash join modeling
- Missing: parallel hash join variant selection
- Missing: radix join as an option
- Missing: skew detection and handling
- Missing: bloom filter runtime filter generation
