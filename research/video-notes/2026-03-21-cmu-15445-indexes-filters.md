# CMU 15-445 Lecture 8-9: Indexes and Filters

**Source:** CMU 15-445 Fall 2024, Lectures 8-9
**Speaker:** Andy Pavlo
**Topic:** Index Structures and Filter Optimizations

## Key Concepts

### Index Types for Optimization
1. **B+Tree**: Range scans, point lookups, ordered access
2. **Hash Index**: Point lookups only, O(1) average
3. **Bloom Filters**: Probabilistic membership test, no false negatives
4. **Zone Maps (Min/Max Indexes)**: Per-page/block min/max for pruning
5. **Bitmap Indexes**: Per-value bitmaps, efficient for low-cardinality
6. **Covering Indexes**: Include all needed columns, avoid heap access

### Index Selection in Optimization
- Optimizer must choose: SeqScan vs IndexScan vs BitmapScan
- Decision depends on selectivity:
  - Low selectivity (< 1%): IndexScan wins
  - Medium selectivity (1-20%): BitmapScan often wins
  - High selectivity (> 20%): SeqScan usually wins
- Correlation matters: ordered data makes IndexScan cheaper

### Multi-Index Access (Bitmap Scans)
- Multiple indexes combined via bitmap AND/OR
- Bitmap represents matching page/row IDs
- Access heap in physical page order (sequential I/O)
- Much better than multiple independent IndexScans for OR predicates
- Example: `WHERE age > 30 AND city = 'NYC'`
  - BitmapIndexScan(age_idx, age > 30) AND BitmapIndexScan(city_idx, city = 'NYC')
  - Then BitmapHeapScan with recheck

### Index Skip Scan
- Use multi-column index for queries on non-leading columns
- Skip through distinct values of leading column
- Effective when leading column has few distinct values
- Example: Index on (gender, age), query on age only
  - Scan: gender='M',age=X then gender='F',age=X

### Partial Indexes
- Index only rows matching a WHERE clause
- Smaller index, faster maintenance
- Optimizer must prove query implies index predicate
- Example: `CREATE INDEX ON orders(total) WHERE status = 'pending'`
  - Used when query has `WHERE status = 'pending' AND total > 100`

## Applicable to Ra

### New Rule Ideas
1. **Selectivity-Based Access Path Selection**: Choose SeqScan vs IndexScan
   vs BitmapScan based on estimated selectivity and correlation.
2. **Multi-Index Bitmap Combination**: When multiple predicates each have
   indexes, combine via BitmapAnd/BitmapOr instead of filtering.
3. **Partial Index Matching**: Check if query WHERE clause implies a partial
   index's predicate; if so, prefer the smaller partial index.
4. **Zone Map Pruning**: Skip data blocks where min/max range doesn't overlap
   with predicate range.
5. **Index Skip Scan Cost Model**: Cost model for skip scan considering
   distinct count of leading columns.
6. **Bloom Filter Index**: Use bloom filter indexes for point lookups on
   columns without B-tree indexes.

### Gap Analysis
- Ra has BitmapIndexScan, BitmapAnd, BitmapOr, BitmapHeapScan (recently added)
- Ra has IndexOnlyScan for covering indexes
- Missing: selectivity-based access path selection rules
- Missing: partial index matching logic
- Missing: zone map utilization rules
- Missing: bloom filter index rules
