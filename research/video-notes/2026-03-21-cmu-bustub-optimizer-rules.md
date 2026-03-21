# BusTub: CMU Teaching Database Optimizer Rules

**Source:** CMU BusTub source code (github.com/cmu-db/bustub)
**Topic:** Optimization rules implemented in CMU's teaching database

## Implemented Rules

### 1. Column Pruning (column_pruning.cpp)
Eliminate columns from projections that are not referenced by
downstream operators. Reduces I/O and memory usage.

### 2. Eliminate True Filter (eliminate_true_filter.cpp)
Remove filter nodes where the predicate is always true (constant folding
has reduced it to `true`). Avoids unnecessary predicate evaluation overhead.

### 3. Merge Filter with Nested Loop Join (merge_filter_nlj.cpp)
When a filter sits directly above a nested loop join, merge the filter
predicate into the join's condition. Reduces one operator.

### 4. Merge Filter with Scan (merge_filter_scan.cpp)
When a filter sits directly above a table scan, merge the filter predicate
into the scan as a scan-time predicate. Enables predicate evaluation
during I/O, reducing row processing overhead.

### 5. Merge Projection (merge_projection.cpp)
When two projections are adjacent, merge them into a single projection.
The inner projection's expressions are substituted into the outer.

### 6. Nested Loop Join to Hash Join (nlj_as_hash_join.cpp)
Convert nested loop join to hash join when:
- Join condition contains equality predicates
- Inner side is large enough to benefit from hashing
Hash join: O(n + m) vs nested loop: O(n * m)

### 7. Nested Loop Join to Index Join (nlj_as_index_join.cpp)
Convert nested loop join to index nested loop join when:
- Inner side has an index on the join key
- Index lookup: O(log n) per outer row vs O(n) per row

### 8. Order By with Index Scan (order_by_index_scan.cpp)
When ORDER BY matches an available index's sort order, use index scan
instead of sequential scan + sort. Eliminates the sort entirely.

### 9. Sequential Scan to Index Scan (seqscan_as_indexscan.cpp)
When filter predicate matches an index's key, replace sequential scan
with index scan. Reduces rows read from O(n) to O(matching).

### 10. Sort + Limit to Top-N (sort_limit_as_topn.cpp)
When Sort is immediately followed by Limit, replace with Top-N sort.
Top-N uses heap: O(n log k) instead of O(n log n) for LIMIT k.

## Teaching Insights

BusTub's rules represent the "minimum viable optimizer" for a teaching
database. They cover the most impactful optimizations:
1. Access path selection (index vs sequential)
2. Join algorithm selection (NLJ -> hash/index)
3. Unnecessary operator elimination (true filters, merge projections)
4. Physical operator selection (sort -> top-N, order by -> index)

## Applicable to Ra

### Verification Against Ra
Most of these basic rules exist in Ra. However, checking for completeness:

1. Column pruning -> Ra has column-pruning.md
2. True filter elimination -> Ra has expression-simplification/
3. Filter-join merge -> Ra has predicate-pushdown/
4. Filter-scan merge -> Ra has predicate-pushdown/
5. Projection merge -> Ra has projection-pushdown/
6. NLJ -> Hash Join -> Ra has join-algorithms/
7. NLJ -> Index Join -> Ra has access-path-selection/
8. ORDER BY -> Index scan -> Need to verify
9. SeqScan -> Index scan -> Ra has access-path-selection/
10. Sort+Limit -> Top-N -> Need to verify

### Potential Gaps
- **Top-N Sort**: Ra may not have explicit Sort+Limit -> Top-N rule
- **ORDER BY Index Elimination**: Ra may not have explicit rule for
  eliminating ORDER BY when index provides sort order
- **Filter-Join Merge**: Ra has predicate pushdown but verify the
  specific merge (filter into join condition) exists
