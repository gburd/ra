# CMU 15-721 Lecture 5: Database Compression

**Source:** https://15721.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- Compression reduces I/O and memory footprint
- Columnar compression achieves higher ratios than row-based
- Some queries can operate directly on compressed data
- Compression scheme affects query processing strategy

## Compression Techniques

### Dictionary Encoding
- Replace values with integer codes
- Small dictionary = fast lookups
- Enables integer operations on string data
- Order-preserving dictionaries enable range scans on codes

### Run-Length Encoding (RLE)
- Store (value, count) pairs for repeated values
- Excellent for sorted, low-cardinality columns
- Enables run-aware aggregation (sum = value * count)

### Bit Packing
- Use minimum bits needed per value
- Frame-of-reference (FOR): store offset + delta
- PFOR: Patched frame-of-reference for outliers

### Delta Encoding
- Store differences between consecutive values
- Effective for sorted or timestamp columns
- Combine with bit packing for maximum compression

### Compression-Aware Query Processing
- Filter on compressed data (dictionary code comparison)
- Aggregate on compressed data (RLE-aware sum/count)
- Join on dictionary codes instead of original values
- Late decompression: only decompress output columns

## Applicable to RA
- RA has limited compression awareness
- Gap: No compression-aware scan rules
- Gap: No dictionary-encoded comparison optimization
- Gap: No RLE-aware aggregation rules
- Gap: No late decompression optimization
- Gap: No compression ratio estimation in cost model
- Gap: No rules for choosing operators based on data encoding

## References
- Abadi, Madden, Ferreira. "Integrating Compression and Execution in Column-Oriented Database Systems" (2006)
- Zukowski et al. "Super-Scalar RAM-CPU Cache Compression" (2006)
