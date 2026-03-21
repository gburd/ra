# CMU 15-721 Lecture 22: System Analysis - Amazon Redshift

**Source:** CMU 15-721 Spring 2024, Lecture 22
**Speaker:** Andy Pavlo (with guest from Amazon)
**Topic:** Amazon Redshift Architecture and Optimization

## Key Concepts

### Redshift Optimizer
- Based on ParAccel, which used Cascades framework
- Evolved significantly with Redshift-specific optimizations
- Distribution-aware optimization is core to the system
- Zone maps used extensively for scan pruning

### Distribution-Aware Optimization
Redshift tables are distributed across compute nodes using:
1. **KEY distribution**: Rows distributed by hash of specified column
2. **ALL distribution**: Full copy on every node (for small dimension tables)
3. **EVEN distribution**: Round-robin across nodes
4. **AUTO distribution**: System chooses based on table size

**Optimizer must consider distribution for joins:**
- Co-located join: both tables distributed on join key -> no data movement
- Broadcast join: small table broadcast to all nodes
- Redistribution join: one or both tables redistributed on join key

### Sort Key Optimization
Redshift uses SORTKEY (compound or interleaved):
- **Compound sort key**: Data sorted by first key, then second, etc.
  - Efficient for range scans on leading columns
  - Zone maps highly effective (min/max per block tightly bounded)
- **Interleaved sort key**: Equal weight to all key columns
  - Effective for any column combination in predicates
  - Zone maps less effective (wider ranges per block)

### Zone Map Exploitation
- Each 1MB block stores min/max for each column
- Scan skips blocks where predicate range doesn't overlap
- Very effective when data is sorted on predicate column
- Column correlation = zone map effectiveness

### Late Materialization
- In columnar storage, only read needed columns
- Evaluate predicates on individual columns first
- Only materialize full rows for matching tuples
- Saves I/O proportional to (columns_used / total_columns)

### Query Compilation
- Compiles SQL to C++ code, then to native machine code
- Entire pipeline compiled as single function
- Eliminates virtual function call overhead
- Cached compiled code reused for similar queries

## Applicable to Ra

### New Rule Ideas
1. **Distribution-Aware Join Planning**: Choose join strategy based on
   data distribution. Co-located tables need no data movement.
2. **Broadcast vs Redistribute Decision**: If one side < broadcast_threshold,
   broadcast; otherwise redistribute on join key.
3. **Sort Key Exploitation**: Use sort key metadata to estimate zone map
   effectiveness and adjust scan cost accordingly.
4. **Late Materialization Rule**: For columnar scans, evaluate predicates
   before materializing unused columns.
5. **Distribution Key Selection Advisor**: Recommend optimal distribution
   key based on join patterns in workload.
6. **Compound vs Interleaved Sort Key Selection**: Choose sort key type
   based on predicate patterns (single-column range -> compound,
   multi-column point -> interleaved).

### Gap Analysis
- Ra has distributed/ rules (58 total) covering some distribution concepts
- Missing: distribution-aware join cost model
- Missing: zone map effectiveness estimation
- Missing: late materialization as optimization rule
- Missing: sort key / cluster key exploitation in cost model
