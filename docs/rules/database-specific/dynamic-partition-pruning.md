# Rule: Dynamic Partition Pruning (Presto)

**Category:** database-specific/presto
**File:** `rules/database-specific/presto/dynamic-partition-pruning.rra`

## Metadata

- **ID:** `presto-dynamic-partition-pruning`
- **Version:** "1.0.0"
- **Databases:** presto
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Dynamic Partition Pruning (Presto)

## Metadata
- **Rule ID**: `presto-dynamic-partition-pruning`
- **Category**: Database-Specific / Presto/Trino
- **Source**: Presto

## Description

Presto uses runtime information from join build side to prune partitions on probe side, similar to dynamic filtering but for partitioned tables.

## Tags
`database-specific`, `presto`, `dynamic-pruning`, `partitions`, `runtime`
