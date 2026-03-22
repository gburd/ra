# CMU 15-721 Lecture 6: SIMD and Vectorized Query Execution

**Source:** CMU 15-721 Spring 2024, Lecture 6
**Date:** 2024-02-12
**Topic:** Leveraging SIMD instructions for query processing
**Key Papers:** "Make the Most out of Your SIMD Investments" (VLDB 2023),
"Rethinking SIMD Vectorization for In-Memory Databases" (SIGMOD 2015),
"Filter Representation in Vectorized Query Execution" (DaMoN 2021)

## Key Points

This lecture covers how modern query engines exploit SIMD (Single Instruction,
Multiple Data) instructions for parallel processing within a single core.
The optimizer must generate plans that are amenable to SIMD execution.

### SIMD Fundamentals for Query Processing

1. **Data-level parallelism**: Process 4-64 values simultaneously per instruction
2. **Key SIMD operations for databases:**
   - Predicate evaluation (compare 8 values at once)
   - Hash computation (hash 4-8 keys simultaneously)
   - Gather/scatter (collect non-contiguous values)
   - Selection (compact qualifying rows using mask)
3. **Width**: SSE (128-bit/4 int32), AVX2 (256-bit/8 int32), AVX-512 (512-bit/16 int32)

### Optimizer Implications

**1. Selection Vector vs Branch-Based Filtering:**
- Traditional: `if (pred(row)) emit(row)` (branch per row)
- Vectorized: evaluate predicate on batch, produce selection vector (bitmap)
- SIMD: evaluate predicate on multiple rows simultaneously
- Optimizer choice: branch-based for high selectivity (few qualifying rows),
  selection vector for moderate selectivity, SIMD for low selectivity

**Optimization rule:** selection-strategy-by-selectivity - choose filtering
strategy based on estimated predicate selectivity.

**2. Batch Size Selection:**
- Batch must fit in L1/L2 cache for vectorized execution
- Too large: cache thrashing, too small: per-batch overhead
- Typical: 1024-4096 rows per batch
- For SIMD: batch size should be multiple of SIMD width

**Optimization rule:** batch-size-selection - choose batch size based on
cache hierarchy and SIMD width of target hardware.

**3. Columnar vs Row Layout for Vectorized Execution:**
- Columnar layout maximizes SIMD efficiency (contiguous values)
- Row layout requires gather operations (expensive on SIMD)
- Optimizer should prefer columnar intermediate representations

**4. Filter Representation:**
Three representations for intermediate filter results:
- **Bitmap**: 1 bit per row, compact but requires bit manipulation
- **Selection vector**: array of qualifying row indices, good for sparse results
- **Validity mask**: per-column null bitmap, reused for filter results

Optimizer must choose representation based on selectivity:
- Bitmap: best for selectivity > 50% (most rows qualify)
- Selection vector: best for selectivity < 20% (few rows qualify)

### SIMD-Friendly Operator Design

**Hash Join:**
- Build: compute hash of multiple keys simultaneously (SIMD hash)
- Probe: batch lookup of multiple keys (SIMD gather from hash table)
- Linear probing benefits from SIMD (check multiple slots at once)

**Sort:**
- Bitonic sort for small arrays (fully SIMD-parallelizable)
- SIMD-accelerated comparison for merge sort
- Key extraction for indirect sort (sort keys, permute rows)

**Aggregation:**
- SIMD accumulation for SUM (add 8 values at once)
- SIMD hash for hash-based grouping
- Partial aggregation within SIMD registers

### Micro Adaptivity in Vectorwise

Runtime micro-optimization within operators:
1. Monitor per-batch selectivity
2. Switch between branch-based and selection vector based on observed selectivity
3. No optimizer involvement needed -- happens within operator execution
4. Example: string comparison switches between SIMD and scalar based on string length

## Optimization Rules for Ra

### New Rules Identified

1. **simd-width-aware-batch-sizing** - Set vector batch size based on target SIMD width
   (AVX2: 8-wide, AVX-512: 16-wide) and data types involved

2. **selection-strategy-selection** - Choose between bitmap, selection vector, and
   branching based on estimated selectivity:
   - selectivity > 0.5: bitmap
   - 0.05 < selectivity < 0.5: selection vector
   - selectivity < 0.05: branch-based

3. **simd-friendly-hash-function** - Select hash function that supports SIMD evaluation
   (CRC32C, multiply-shift) over complex hashes (MurmurHash) for hash joins/aggregates

4. **columnar-intermediate-preference** - When generating intermediate results (temp
   tables, materialization points), prefer columnar layout for SIMD-friendly downstream
   processing

5. **bitonic-sort-for-small-arrays** - For sorts with < 1024 elements, use SIMD bitonic
   sort instead of comparison-based sort

6. **gather-avoidance** - When planning row-format access, restructure to avoid SIMD
   gather instructions (expensive on most architectures). Prefer sequential access.

### Ra Gap Analysis

Ra currently has:
- `rules/execution-models/vectorized/` - Vectorized execution rules
- `rules/hardware/` - Hardware-aware rules (including SIMD-related)
- `crates/ra-hardware/` - Hardware detection crate

**Likely already covered:**
- Basic vectorized execution model selection
- Some hardware-aware optimization

**Missing capabilities:**
- Selection strategy selection based on selectivity
- SIMD-width-aware batch sizing
- Filter representation selection (bitmap vs selection vector)
- SIMD-friendly operator variant selection
- Micro-adaptivity within operators

## Relevance to Ra

**Priority:** Medium - SIMD optimization is primarily relevant at the execution
engine level, not the logical optimizer level. However, Ra generates execution
plans and should annotate operators with SIMD-relevant hints (batch size, filter
representation, hash function choice) when targeting vectorized execution engines.

**Key insight:** The most impactful optimizer-level decision is choosing between
materialization strategies (columnar vs row) at pipeline boundaries. Getting this
right enables or prevents SIMD optimization in downstream operators.
