# Rule: Adaptive Code Generation (HyPer)

**Category:** database-specific/hyper
**File:** `rules/database-specific/hyper/adaptive-code-generation.rra`

## Metadata

- **ID:** `hyper-adaptive-code-generation`
- **Version:** "1.0.0"
- **Databases:** hyper
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Adaptive Code Generation (HyPer)

## Metadata
- **Rule ID**: `hyper-adaptive-codegen`
- **Category**: Database-Specific / HyPer/Umbra
- **Source**: HyPer/Umbra

## Description

HyPer compiles queries to LLVM IR, then adaptively switches between interpreted and compiled execution based on query complexity and data size.

## Implementation Pattern

```cpp
class AdaptiveExecutor {
    void execute(QueryPlan plan) {
        if (plan.estimated_cost() < COMPILATION_THRESHOLD) {
            // Interpret (fast startup)
            interpretedExecute(plan);
        } else {
            // Compile to native code (high throughput)
            CompiledQuery compiled = llvmCompile(plan);
            compiled.execute();
        }
    }
};
```

## References
1. **Paper**: "Efficiently Compiling Efficient Query Plans for Modern Hardware" (VLDB 2011)
   - DOI: 10.14778/2002938.2002940

## Tags
`database-specific`, `hyper`, `umbra`, `compilation`, `llvm`, `adaptive`, `jit`
