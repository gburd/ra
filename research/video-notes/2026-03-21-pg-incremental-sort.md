# PostgreSQL v13-16: Incremental Sort and Key Reordering

**Source:** PostgreSQL v13/v16 Release Notes and Documentation
**Topic:** Incremental sort, GROUP BY reordering, DISTINCT reordering

## Key Concepts

### Incremental Sort (v13)
When data is already sorted on a prefix of the required sort key,
only sort within each prefix group instead of full re-sort.

**Cost model:**
- Full sort: O(n * log(n))
- Incremental sort: O(n * log(m)) where m = average group size
- When prefix covers most of sort key, m << n
- Memory: only buffer one group at a time

**Example:**
- Index on (a) provides sort on column a
- Query needs ORDER BY a, b, c
- Incremental sort: for each distinct value of a, sort by (b, c)
- If avg 100 rows per distinct a value: O(n * log(100)) vs O(n * log(n))

**When to use:**
- Index provides partial sort order
- Previous sort/merge operation provides partial order
- Particularly effective with index scans that provide ordering

### GROUP BY Key Reordering (v16)
Reorder GROUP BY keys to maximize prefix match with available
input ordering, avoiding unnecessary sort.

**Example:**
- Index provides sort on (a, b)
- Query: GROUP BY (c, b, a)
- Reorder to: GROUP BY (a, b, c)
- Now incremental sort only needs to sort by c within each (a, b) group

**Rules:**
- Reordering GROUP BY does not change query semantics
- Any permutation of GROUP BY keys produces same result
- Choose permutation maximizing pathkey prefix match

### DISTINCT Key Reordering (v16)
Same principle applied to SELECT DISTINCT.

**Example:**
- Index provides sort on (x, y)
- Query: SELECT DISTINCT z, y, x FROM t
- Reorder evaluation to process x, y first (from index), then only sort z

### Presorted Aggregate (v16)
When aggregate function has ORDER BY or DISTINCT, provide presorted
input to avoid internal sort.

**Example:**
```sql
SELECT array_agg(val ORDER BY val) FROM t GROUP BY grp;
```
If input sorted by (grp, val), aggregate receives presorted data.

## Applicable to Ra

### New Rules
1. **Incremental Sort Selection**:
   ```
   Pattern: Sort(keys=[k1, k2, ..., kN], input=X)
   Condition: X provides ordering on prefix [k1, ..., kP] where P < N
   Result: IncrementalSort(prefix=[k1,...,kP], suffix=[kP+1,...,kN], input=X)
   ```

2. **GROUP BY Key Reordering**:
   ```
   Pattern: Aggregate(group_by=[g1,...,gN], input=X)
   Condition: X provides ordering on some permutation prefix
   Result: Aggregate(group_by=reordered_keys, input=X)
   ```

3. **DISTINCT Key Reordering**:
   ```
   Pattern: Distinct(input=Sort(keys=[k1,...,kN], input=X))
   Condition: X provides ordering on some permutation prefix
   Result: Distinct(input=IncrementalSort(...))
   ```

4. **Presorted Aggregate**:
   ```
   Pattern: Aggregate(aggs=[agg(DISTINCT col)], input=Sort(keys=..., X))
   Condition: sort keys match aggregate ordering needs
   Result: Aggregate(aggs=[agg(col, presorted=true)], input=X)
   ```

### Prerequisites
- Physical property tracking (know what ordering input provides)
- IncrementalSort as a physical operator in Ra's algebra (already exists)
- Pathkey/ordering metadata on plan nodes

### Impact
- Eliminates full sorts in 15-25% of GROUP BY queries
- Reduces sort memory from O(n) to O(max_group_size)
- Proven production value in PostgreSQL v13-16
