# Rule: Adaptive Plan Optimization (Trino)

**Category:** database-specific/trino
**File:** `rules/database-specific/trino/adaptive-plan-optimization.rra`

## Metadata

- **ID:** `trino-adaptive-plan-optimization`
- **Version:** "1.0.0"
- **Databases:** trino
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Adaptive Plan Optimization (Trino)

## Metadata
- **Rule ID**: `trino-adaptive-plan-optimization`
- **Category**: Database-Specific / Trino
- **Source**: Trino AdaptivePlanOptimizer.java

## Description

Trino re-optimizes query plans mid-execution based on actual runtime statistics, adjusting join strategies and partition counts dynamically.

## Tags
`database-specific`, `trino`, `adaptive`, `runtime-reoptimization`
