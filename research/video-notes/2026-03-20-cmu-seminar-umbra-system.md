# CMU Seminar: Umbra - Disk-Based System with In-Memory Performance

**Source:** https://db.cs.cmu.edu/seminar2022/ (Databases! series)
**Date:** 2022-09-12
**Speaker:** Thomas Neumann

## Key Points
- Umbra is the successor to HyPer from TUM
- Combines disk-based durability with in-memory performance
- Uses adaptive morsel-driven parallelism
- Compiles queries to machine code via custom IR (Flying Start)

## Optimization Techniques
- **Flying Start compilation**: custom IR avoids LLVM overhead
  - Compile time: microseconds vs milliseconds for LLVM
  - Quality: within 10% of LLVM-generated code
- **Adaptive morsel sizing**: adjust morsel size based on operator cost
- **Buffer management**: pointer-based access with transparent paging
- **Pipeline fusion**: fuse operators into tight loops
- **Vectorized fallback**: interpret short-running queries

## Key Insights for RA
- Compilation overhead matters: need adaptive JIT threshold
- Morsel size should be operator-dependent, not fixed
- Pipeline fusion is the key optimization for compiled queries
- Disk-based systems can match in-memory performance with right design

## Applicable to RA
- Gap: No adaptive compilation threshold rules
- Gap: No pipeline fusion cost model
- Gap: No custom IR generation (only Cranelift/LLVM)
- Gap: No morsel size adaptation based on operator type

## References
- Neumann & Leis. "Umbra: A Disk-Based System with In-Memory Performance" (2020)
- Neumann. "Efficiently Compiling Efficient Query Plans" (2011)
