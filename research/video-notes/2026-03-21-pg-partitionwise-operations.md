# PostgreSQL: Partitionwise Join and Aggregation

**Source:** PostgreSQL Documentation and Source Analysis
**Topic:** Partition-aware join and aggregation optimizations

## Key Concepts

### Partitionwise Join
When two partitioned tables are joined on their partition key,
join matching partitions separately instead of joining the full tables.

**Example:**
```sql
-- Both tables partitioned by region
SELECT * FROM orders o JOIN customers c ON o.region = c.region;

-- Without partitionwise join:
--   HashJoin(SeqScan(orders), SeqScan(customers))

-- With partitionwise join:
--   Append(
--     HashJoin(SeqScan(orders_east), SeqScan(customers_east)),
--     HashJoin(SeqScan(orders_west), SeqScan(customers_west)),
--     HashJoin(SeqScan(orders_central), SeqScan(customers_central))
--   )
```

**Benefits:**
- Each partition join uses less memory (smaller hash tables)
- Enables parallel execution across partitions
- Partition pruning can eliminate entire join branches
- Data locality: matching partitions may be co-located

**Requirements:**
- Both tables partitioned on join key
- Partition schemes must be compatible (same number of partitions, same boundaries)
- Join condition includes all partition key columns

**Trade-offs:**
- Planning cost increases linearly with partition count
- Memory for plan tree increases linearly
- Not beneficial when few partitions or uneven partition sizes

### Partitionwise Aggregation
When aggregating a partitioned table and GROUP BY includes the partition key,
aggregate within each partition then combine results.

**Example:**
```sql
-- Table partitioned by date
SELECT date, SUM(amount) FROM sales GROUP BY date;

-- Without partitionwise aggregate:
--   HashAggregate(Append(Scan(jan), Scan(feb), Scan(mar)))

-- With partitionwise aggregate:
--   Append(
--     HashAggregate(Scan(jan)),
--     HashAggregate(Scan(feb)),
--     HashAggregate(Scan(mar))
--   )
```

**Benefits:**
- Each partition aggregate uses less memory
- Results are locally complete (no final combine needed when GROUP BY = partition key)
- Enables parallelism across partitions

**Two cases:**
1. **Full aggregation**: GROUP BY includes partition key -> each partition produces final result
2. **Partial aggregation**: GROUP BY does not include partition key -> partial results need final combine

### Runtime Partition Pruning
Eliminate partitions at execution time using parameter values unknown during planning.

**Example:**
```sql
PREPARE q AS SELECT * FROM sales WHERE date = $1;
EXECUTE q('2024-01-15');
-- At execution time, prune all partitions except January
```

**Also applies to:**
- Subquery results used as partition filters
- Join-derived filters (semi-join with dimension table)
- Expression evaluation with runtime values

## Applicable to Ra

### New Rules
1. **Partitionwise Join**:
   ```
   Pattern: Join(type, Scan(T1, partitioned_by=K), Scan(T2, partitioned_by=K), K=K)
   Condition: T1 and T2 have compatible partition schemes
   Result: Union(all=true,
     Join(type, Scan(T1_p1), Scan(T2_p1), cond),
     Join(type, Scan(T1_p2), Scan(T2_p2), cond),
     ...)
   ```

2. **Partitionwise Aggregation**:
   ```
   Pattern: Aggregate(group_by=[..., K, ...], input=Scan(T, partitioned_by=K))
   Result: Union(all=true,
     Aggregate(group_by=[...], input=Scan(T_p1)),
     Aggregate(group_by=[...], input=Scan(T_p2)),
     ...)
   ```

3. **Runtime Partition Pruning**:
   ```
   Pattern: Filter(pred(param), Append(Scan(T_p1), Scan(T_p2), ...))
   Result: Filter(pred(param), Append(Scan(T_matching_partitions)))
   -- Evaluated at execution time
   ```

### Prerequisites
- Partition metadata in catalog (partition key, boundaries, scheme type)
- Partition compatibility checking
- New algebra nodes or extensions for partition-aware operations

### Impact
- 2-10x speedup for partitioned table joins (memory and parallelism)
- 2-5x speedup for partitioned aggregation
- Critical for time-series and multi-tenant databases
