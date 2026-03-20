# PostgreSQL Join Processing Internals

**Source:** https://www.interdb.jp/pg/pgsql03/06.html
**Date:** Reference documentation
**Speaker:** Hironobu Suzuki

## Key Points
- Dynamic programming for join ordering under GEQO threshold
- Three-level progressive combination
- Preprocessing converts subqueries and outer joins where possible
- RelOptInfo structures track costs and paths at each level

## Join Ordering Algorithm

### Preprocessing
1. Handle WITH clause (CTE) processing
2. Convert FROM subqueries to joins when possible
   - Condition: no GROUP BY, HAVING, ORDER BY, LIMIT, DISTINCT
   - Controlled by from_collapse_limit (default 8)
3. Convert outer joins to inner joins when possible
   - Condition: WHERE clause makes outer join unnecessary
   - Example: LEFT JOIN with IS NOT NULL on right side

### Dynamic Programming Enumeration
1. **Level 1**: Enumerate single-table access paths
   - Sequential scan, index scans, bitmap scans
   - Store in RelOptInfo for each base relation

2. **Level 2**: Enumerate two-table combinations
   - For each pair with join predicate
   - Try: nested loop, merge join, hash join
   - Try: both table orderings (A join B, B join A)
   - Try: indexed nested loop with parameterized inner

3. **Level 3+**: Progressive combination
   - Combine level-2 results with single tables
   - Combine level-2 results with other level-2 results
   - Track cheapest path + "interesting orderings"

4. **Level N**: Select cheapest complete plan

### Interesting Orderings
- Some orderings are worth preserving even if locally more expensive
- Example: sort for merge join also satisfies ORDER BY
- System R innovation: track set of "interesting" sort orders
- PostgreSQL tracks pathkeys at each planning level

## Applicable to RA
- Gap: No DP-based join enumeration (RA uses e-graphs)
- Gap: No subquery-to-join conversion during preprocessing
- Gap: No outer-to-inner join conversion based on WHERE analysis
- Gap: No "interesting orderings" / pathkeys tracking
- Gap: No parameterized scan consideration for nested loop inners
- Gap: No progressive level-based cost tracking

## References
- Selinger et al. "Access Path Selection in a Relational Database Management System" (1979)
- Moerkotte & Neumann. "Dynamic Programming Strikes Back" (2008)
