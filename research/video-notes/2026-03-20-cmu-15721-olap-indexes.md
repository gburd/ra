# CMU 15-721 Lecture 4: OLAP Indexes

**Source:** https://15721.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- OLAP indexes differ fundamentally from OLTP B+Trees
- Zone maps, bitmap indexes, and bloom filters dominate
- Data skipping is the primary optimization strategy
- Column-specific indexing for analytical queries

## OLAP Index Types

### Zone Maps (Min-Max Indexes)
- Store min/max per column per row group/page
- Skip entire blocks that cannot match predicates
- Zero maintenance: computed from data layout
- Effective when data is clustered on query predicates
- Parquet, ORC, Delta Lake all use zone maps

### Bitmap Indexes
- One bitmap per distinct value per column
- AND/OR operations via bitwise operations
- Compressed bitmaps: Roaring, EWAH, WAH
- Good for low-cardinality columns (status, category)
- Not suitable for high-cardinality columns

### Bloom Filters
- Probabilistic membership test (false positives, no false negatives)
- Useful for point lookups: "is value X in this block?"
- Compact representation: bits per element
- Used in Parquet, HBase, Cassandra row groups

### Learned Indexes
- Replace traditional index with ML model
- Model predicts position of key in sorted data
- Recursive Model Index (RMI): hierarchy of models
- Faster lookup but harder to update
- Research area: SageDB, ALEX, PGM-index

## Applicable to RA
- RA has physical/index-selection/ (36 rules)
- Gap: No zone map utilization rules
- Gap: No bitmap index selection and combination rules
- Gap: No bloom filter index rules
- Gap: No learned index cost modeling
- Gap: No data-skipping optimization framework
- Gap: No row group pruning cost model

## References
- O'Neil & Quass. "Improved Query Performance with Variant Indexes" (1997)
- Kraska et al. "The Case for Learned Index Structures" (2018)
- Chambi et al. "Better bitmap performance with Roaring bitmaps" (2016)
