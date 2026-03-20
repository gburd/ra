# CMU 15-445 Lecture 8: Tree Indexes

**Source:** https://15445.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- B+Tree is the dominant index structure for OLTP workloads
- Index selection is critical to query performance
- Covering indexes avoid heap table access
- Partial indexes reduce index size and maintenance cost

## Index Types and Techniques

### B+Tree
- Self-balancing, O(log n) lookups
- Supports range scans, ORDER BY
- Prefix compression for space efficiency
- Bulk loading for initial construction

### Hash Index
- O(1) average lookup
- No range scan support
- Good for equality predicates only

### Covering Index (Index-Only Scan)
- Include all columns needed by query in index
- Avoids random I/O to heap table
- PostgreSQL: INCLUDE clause in CREATE INDEX

### Partial Index
- Index only rows matching a predicate
- Smaller, faster, cheaper to maintain
- PostgreSQL: WHERE clause in CREATE INDEX

### Multi-Column Index
- Composite index on (a, b, c)
- Useful for leftmost prefix queries
- Order matters: (a, b, c) helps WHERE a=1 AND b=2 but not WHERE b=2 alone

### Index Skip Scan
- Scan multi-column index skipping leading column values
- For queries that don't use leftmost prefix
- Not universal - PostgreSQL added in v18

## Applicable to RA
- RA has physical/index-selection/ (36 rules) covering basics
- Gap: No partial index matching rules (match query predicate to partial index WHERE)
- Gap: No covering index detection and promotion rules
- Gap: No index skip scan cost modeling
- Gap: No composite index ordering analysis
- Gap: No index intersection/union optimization rules

## References
- Bayer & McCreight. "Organization and Maintenance of Large Ordered Indices" (1970)
- Graefe. "Modern B-Tree Techniques" (2011)
