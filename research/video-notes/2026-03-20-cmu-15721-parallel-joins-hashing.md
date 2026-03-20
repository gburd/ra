# CMU 15-721 Lecture 11: Parallel Join Algorithms (Hashing)

**Source:** https://15721.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- Hash joins dominate in analytical workloads
- Parallelism in both build and probe phases
- Partitioning strategy critically affects performance
- NUMA-awareness is essential for multi-socket systems

## Parallel Hash Join Techniques

### Partitioned Hash Join
- Phase 1: Partition both relations using hash function h1
- Phase 2: Build hash table per partition using hash function h2
- Phase 3: Probe hash tables in parallel
- Advantage: each partition fits in cache
- Disadvantage: partitioning pass adds cost

### Shared Hash Table (Non-Partitioned)
- Build single shared hash table with concurrent inserts
- Use lock-free hash table (CAS operations)
- Probe phase is embarrassingly parallel
- Advantage: no partitioning overhead
- Disadvantage: cache contention on large tables

### Radix Hash Join
- Multi-pass radix partitioning for cache efficiency
- First pass: partition into cache-line-sized chunks
- Each subsequent pass refines partitioning
- Best performance on modern hardware with large L2/L3 caches

### NUMA-Aware Hash Join
- Partition data by NUMA node affinity
- Schedule build/probe on local NUMA node
- Minimize cross-socket memory access
- Can achieve near-linear scalability

### Skew Handling
- Heavy hitters: detect and handle frequent values separately
- Partition splitting: split oversized partitions further
- Work stealing: balance load across threads dynamically

## Applicable to RA
- RA has physical/hardware/cache-conscious-join.rra and physical/hardware/numa-aware-partitioning.rra
- Gap: No radix hash join cost modeling
- Gap: No parallel hash table selection rules (partitioned vs shared)
- Gap: No skew detection and handling rules for hash joins
- Gap: No work-stealing scheduling rules
- Gap: No NUMA-aware hash join build/probe rules

## References
- Kim et al. "Sort vs. Hash Revisited: Fast Join Implementation on Modern Multi-Core CPUs" (2009)
- Balkesen et al. "Main-Memory Hash Joins on Multi-Core CPUs: Tuning to the Underlying Hardware" (2013)
- Schuh et al. "An Experimental Comparison of Thirteen Relational Equi-Joins in Main Memory" (2016)
