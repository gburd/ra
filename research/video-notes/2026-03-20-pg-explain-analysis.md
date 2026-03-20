# PostgreSQL EXPLAIN Analysis and Plan Diagnosis

**Source:** https://www.postgresql.org/docs/current/using-explain.html
**Date:** Reference documentation (current)
**Speaker:** PostgreSQL documentation

## Key Points
- EXPLAIN ANALYZE shows estimated vs actual values
- Cost estimation errors are the primary cause of suboptimal plans
- Plan node types indicate optimizer decisions
- Buffer statistics reveal I/O patterns

## Plan Analysis Techniques

### Identifying Bad Estimates
- Compare "rows=estimated" with "actual rows=actual"
- Orders-of-magnitude difference suggests missing/stale statistics
- Run ANALYZE on affected tables
- Consider extended statistics for correlated columns

### Identifying Wrong Join Methods
- Hash join chosen when nested loop would be better (small outer)
- Nested loop chosen when hash join better (large outer, no index)
- Disable methods to test: SET enable_hashjoin = off

### Identifying Missing Indexes
- Sequential scan with high "Rows Removed by Filter"
- Index scan with high random_page_cost penalty
- Bitmap scan combining multiple low-selectivity indexes

### Red Flags
| Signal | Likely Problem | Fix |
|--------|---------------|-----|
| Estimated rows >> actual | Overestimate, too-expensive plan chosen | ANALYZE, extended stats |
| Estimated rows << actual | Underestimate, may OOM on hash join | ANALYZE, increase work_mem |
| Seq Scan on filtered table | Missing index | CREATE INDEX |
| Sort with high startup cost | Missing index for ORDER BY | CREATE INDEX matching sort |
| Nested Loop with large outer | Wrong join method | Check statistics, join_collapse_limit |
| Hash Join building on large side | Wrong build side | Check statistics |
| "Rows Removed by Filter" high | Filter not pushed to scan | Check predicate form |

## Applicable to RA
- Gap: No plan quality analysis / diagnosis framework
- Gap: No "plan vs actual" comparison mechanism
- Gap: No automatic diagnosis of estimation errors
- Gap: No recommendation engine for missing indexes or statistics
- Gap: No plan visualization with actual execution metrics

## References
- PostgreSQL documentation: Chapter 14 - Performance Tips
- pgMustard, EXPLAIN.dalibo.com - plan visualization tools
