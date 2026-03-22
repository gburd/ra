# CMU 15-721 Lectures 2-3: Data Formats and Encoding for Optimization

**Source:** CMU 15-721 Spring 2024, Lectures 2-3
**Date:** 2024-01-24, 2024-01-29
**Topic:** Columnar storage formats, compression, and their impact on query optimization
**Key Papers:** FastLanes (VLDB 2023), BtrBlocks (SIGMOD 2023), BitWeaving (SIGMOD 2013)

## Key Points

These lectures cover how data encoding and storage format choices affect query
optimization. The optimizer must be format-aware to generate efficient plans.

### Columnar Format Optimization Opportunities

1. **Zone maps / min-max indexes**: Per-column-chunk min/max values enable chunk pruning
   - Optimizer rule: skip entire column chunks when predicate falls outside min/max range
   - No index needed -- metadata is stored alongside the data

2. **Dictionary encoding**: Columns with low cardinality stored as integer codes
   - Optimizer rule: evaluate predicates on dictionary codes instead of decoded values
   - Predicate `col = 'value'` becomes `col_code = dict_lookup('value')`
   - Sorting on dictionary-encoded column = sorting on integer codes (faster)

3. **Run-length encoding (RLE)**: Sorted columns with repeated values
   - Optimizer rule: COUNT/SUM/MIN/MAX on RLE columns can operate on runs, not individual values
   - GROUP BY on RLE column is nearly free (runs = groups)
   - Merge join on RLE columns can skip entire runs

4. **Bit-packing**: Store integers in minimum bits needed
   - Optimizer rule: filter evaluation on bit-packed data using SIMD bitwise operations
   - BitWeaving: horizontal (HBP) and vertical (VBP) bit-packing layouts
   - Predicate evaluation without full decompression

5. **Frame of reference (FOR)**: Store deltas from a base value
   - Optimizer rule: range predicates on FOR-encoded data can check base + delta bounds
   - Combined with bit-packing for very compact representation

### FastLanes Compression Layout

Key insight: design compression layout for SIMD-friendly decompression.

**Optimization implications:**
- Interleave data across SIMD lanes for maximum throughput
- 100+ billion integers per second decompression on modern hardware
- Optimizer should prefer scans over index lookups when decompression is this fast
- Cost model must account for decompression speed when comparing access paths

### BtrBlocks: Adaptive Compression

Cascading compression that tries multiple schemes per block:
1. Try dictionary encoding first (if low cardinality)
2. Try RLE (if sorted/clustered)
3. Try FOR + bit-packing (if numeric with small range)
4. Fall back to general-purpose compression (LZ4, ZSTD)

**Optimization implications:**
- Different blocks of the same column may have different encodings
- Optimizer must track per-block encoding metadata
- Predicate pushdown effectiveness varies by encoding
- Cost model needs per-encoding cost multipliers

### Late Materialization

Defer row reconstruction until after predicate evaluation:
1. Evaluate all predicates on compressed columnar data
2. Build a selection vector (bitmap of qualifying rows)
3. Only materialize (decompress and reconstruct) qualifying rows
4. Particularly effective when selectivity is low (few rows qualify)

**Optimizer rule:** Insert late materialization boundaries to minimize decompression work.
Order: evaluate most selective predicates first, materialize columns needed for output last.

## Optimization Rules for Ra

### New Rules Identified

1. **dictionary-predicate-rewrite** - Rewrite string/enum predicates to operate on
   dictionary codes when column is dictionary-encoded
2. **zone-map-chunk-pruning** - Use column chunk min/max to skip entire chunks during scan
3. **rle-aggregate-acceleration** - Compute aggregates on RLE runs instead of individual values
4. **late-materialization-insertion** - Defer column materialization until after predicate evaluation
5. **encoding-aware-sort-cost** - Adjust sort cost based on column encoding (integer sort on
   dict codes is cheaper than string sort)
6. **compression-ratio-in-io-cost** - Adjust I/O cost model based on compression ratio
   (fewer bytes read from disk, but CPU cost for decompression)
7. **bitweaving-predicate-evaluation** - Use SIMD bitwise operations for predicate evaluation
   on bit-packed columns
8. **format-aware-access-path-selection** - Choose scan strategy based on storage format
   (Parquet, Arrow, ORC each have different pruning capabilities)

### Ra Gap Analysis

Ra currently has:
- `crates/ra-codegen/` - Code generation (could support encoding-aware code)
- `rules/physical/materialization/` - Materialization rules
- `rules/physical/hardware/` - Hardware-aware rules
- Parquet predicate pushdown (recently implemented)
- No dictionary-encoding-aware optimization
- No zone map / min-max pruning rules
- No compression-aware cost model

**Missing capabilities:**
- Storage format metadata in the catalog (encoding type per column/chunk)
- Dictionary encoding awareness in predicate evaluation
- Zone map metadata propagation to optimizer
- Late materialization boundary optimization
- Compression-ratio-adjusted I/O cost model

## Relevance to Ra

**Priority:** High for analytics workloads - columnar format optimization is the
single biggest performance differentiator for OLAP systems. Every major analytical
database (DuckDB, Snowflake, Databricks, Redshift) has format-aware optimization.

**Proposed RFC:** Format-Aware Query Optimization - extend the cost model and add
rules for dictionary-encoding rewrite, zone map pruning, late materialization, and
compression-aware I/O costing. This would significantly improve Ra's performance
on Parquet/Arrow/ORC data.
