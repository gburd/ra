# CMU Research: Hustle - Hardware-Software Co-Design

**Source:** https://db.cs.cmu.edu/projects/hustle/
**Date:** Ongoing research
**Speaker:** Jignesh Patel, Andy Pavlo

## Key Points
- Moore's Law and Dennard scaling limits require co-design approach
- Processing-in-Memory (PiM) moves compute closer to data
- Bit-level data shredding may be next evolution beyond columnar
- Hardware accelerators (Intel IAX) for database operations

## Techniques

### Processing-in-Memory
- Avoid memory wall by computing at storage location
- Aggregation, filtering near memory chips
- Dramatically reduces data movement costs
- Requires rethinking cost models

### Bit-Level Data Organization
- Beyond column stores: shred to individual bits
- Enables new compression and SIMD patterns
- Question: "did we stop too soon" at columnar granularity?

### Hardware Accelerators
- Intel Analytics Accelerator (IAX) for decompression, filtering
- Associative processors for search operations
- GPU offloading for hash joins and aggregations
- FPGA-based custom operators

## Applicable to RA
- RA has hardware/ (21 rules) including GPU and accelerator rules
- Gap: No PiM cost model or operator placement rules
- Gap: No bit-level data access optimization rules
- Gap: No Intel IAX accelerator-specific rules
- Gap: No automatic hardware offloading decision rules
- Gap: Cost model doesn't account for memory bandwidth wall

## References
- Patel & Pavlo. Hustle project (CMU)
- Kim et al. Processing-in-Memory research
