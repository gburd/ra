# CMU 15-721 Lecture 9: Query Compilation

**Source:** https://15721.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- Query compilation generates native machine code for query plans
- Eliminates interpretation overhead entirely
- Two approaches: transpile to C/C++ or generate LLVM IR directly
- Compilation time is a real concern for short-running queries

## Compilation Techniques

### Code Generation Approaches
1. **Transpilation**: generate C/C++ source, compile with system compiler
   - PostgreSQL JIT: generates LLVM IR for expression evaluation
   - HyPer/Umbra: generate LLVM IR for entire pipelines
2. **Direct IR generation**: emit LLVM IR or similar
   - Faster compilation than transpilation
   - More control over generated code
3. **Adaptive compilation**: interpret first, compile hot queries
   - PostgreSQL: JIT kicks in above cost threshold (jit_above_cost = 100000)
   - Reduces compilation overhead for simple queries

### Pipeline-Based Compilation (Neumann 2011)
- Fuse operators within a pipeline into a single tight loop
- Pipeline breakers: sorts, hash builds, materializations
- "Produce/consume" paradigm: each operator generates code
- Eliminates tuple-at-a-time overhead

### JIT Cost Thresholds (PostgreSQL)
| Parameter | Default | Purpose |
|-----------|---------|---------|
| jit | on | Enable JIT compilation |
| jit_above_cost | 100000 | Minimum plan cost for JIT |
| jit_inline_above_cost | 500000 | Cost threshold for function inlining |
| jit_optimize_above_cost | 500000 | Cost threshold for expensive optimizations |

## Applicable to RA
- RA has ra-codegen crate and experimental/compilation/ (2 rules)
- Gap: No pipeline fusion rules (identify fusible operator sequences)
- Gap: No JIT cost threshold decision rules
- Gap: No adaptive compilation strategy rules
- Gap: No pipeline breaker analysis and minimization
- Gap: Only 2 compilation rules - needs significant expansion

## References
- Neumann. "Efficiently Compiling Efficient Query Plans for Modern Hardware" (2011)
- Shaikhha et al. "How to Architect a Query Compiler, Revisited" (2018)
- Kersten et al. "Compiled vs Vectorized" (2018)
