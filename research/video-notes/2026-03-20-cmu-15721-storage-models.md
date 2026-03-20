# CMU 15-721 Lecture 3: Storage Models & Data Layout

**Source:** https://15721.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- Storage layout fundamentally affects query optimization strategies
- NSM (row store) vs DSM (column store) vs PAX (hybrid)
- Column stores enable late materialization, compression, and vectorized execution
- Storage model should inform optimizer decisions

## Storage Model Implications for Optimization

### Row Store (NSM) Optimization
- Favor index scans for selective queries
- Covering indexes avoid full row reads
- Sequential scan efficient for full-row access
- UPDATE-friendly: single page write

### Column Store (DSM) Optimization
- Column pruning eliminates entire columns from I/O
- Late materialization: keep column references, reconstruct rows late
- Compression-aware scanning: operate on compressed data
- Vectorized execution natural fit
- SCAN-friendly: sequential column reads

### Hybrid (PAX) Optimization
- Row groups contain column chunks
- Zone maps (min/max per column chunk) enable chunk skipping
- Hybrid of row and column benefits
- Used by Parquet, ORC, Arrow

### Zone Maps / Min-Max Indexes
- Store min/max per column per data block
- Skip blocks that cannot contain matching values
- No maintenance cost (embedded in storage format)
- Effective for range predicates on sorted/clustered data

## Applicable to RA
- RA has some storage-aware rules in physical/materialization/
- Gap: No zone map / min-max index utilization rules
- Gap: No storage-layout-aware scan selection rules
- Gap: No late materialization optimization rules
- Gap: No compression-aware query processing rules
- Gap: No rules for choosing scan strategy based on column selectivity

## References
- Abadi, Madden, Hachem. "Column-Stores vs. Row-Stores: How Different Are They Really?" (2008)
- Zukowski et al. "Super-Scalar RAM-CPU Cache Compression" (2006)
