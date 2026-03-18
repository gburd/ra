# Index Types

RA models 11 index types across major database systems. Each type has distinct characteristics that affect query optimizer access path selection.

## Supported Index Types

| Type | Databases | Equality | Range | Ordering | Use Case |
|------|-----------|----------|-------|----------|----------|
| Clustered | MySQL, MSSQL, Oracle | Yes | Yes | Yes | Primary key, range scans |
| NonClustered | All | Yes | Yes | Yes | General-purpose secondary index |
| Composite | All | Yes | Yes | Yes | Multi-column predicates |
| FullText | All | No | No | No | Natural language search |
| Unique | All | Yes | Yes | Yes | Uniqueness enforcement |
| Filtered | PostgreSQL, MSSQL | Yes | Yes | Yes | Subset indexing (partial) |
| Spatial | PostgreSQL, MySQL, MSSQL | No | Yes | No | Geometry, geography |
| Columnstore | MSSQL, ClickHouse, DuckDB | No | No | No | Analytical aggregation |
| Hash | PostgreSQL, MySQL | Yes | No | No | Exact-match lookups |
| GIN | PostgreSQL | Yes | No | No | JSONB, arrays, tsvector |
| GiST | PostgreSQL | Yes | Yes | No | Ranges, KNN, geometry |

## Cost Model

Each index type has associated cost factors:

- `random_page_cost` - Cost of random I/O (default: 4.0)
- `sequential_page_cost` - Cost of sequential I/O (default: 1.0)
- `cpu_index_tuple_cost` - Per-tuple index evaluation cost
- `cpu_heap_tuple_cost` - Per-tuple heap fetch cost
- `type_multiplier` - Relative cost vs standard B-tree

### Type Multipliers

| Index Type | Multiplier | Rationale |
|------------|-----------|-----------|
| B-tree (Clustered, NonClustered, etc.) | 1.0 | Baseline |
| Hash | 0.8 | O(1) lookup |
| GIN | 1.5 | Inverted index overhead |
| GiST | 2.0 | Generalized tree operations |
| Spatial | 2.5 | R-tree + geometric computation |
| FullText | 3.0 | Text parsing + ranking |
| Columnstore | 0.3 | Batch mode + compression |

## Index Selection Algorithm

The optimizer selects the best index based on:

1. **Predicate coverage** - Index key columns must cover the predicate columns
2. **Ordering match** - Index must provide required order (if ORDER BY present)
3. **Covering potential** - Covering indexes avoid heap access (50% cost reduction)
4. **Estimated scan cost** - Based on selectivity, clustering factor, and cost factors

## Implementation

Index types are defined in `crates/ra-stats/src/index_types.rs`:

- `IndexType` - Enum with 11 variants
- `IndexMetadata` - Instance metadata (size, levels, clustering factor)
- `IndexCostFactors` - Per-type cost parameters
- `IndexScanCost` - Scan cost breakdown (index I/O, heap I/O, CPU)
- `select_best_index()` - Index selection function

## Optimization Rules

15 index selection rules in `rules/physical/index-selection/`:

- Range scan optimization for clustered indexes
- Covering index with INCLUDE columns
- Full-text index for LIKE/CONTAINS patterns
- Spatial index for geometry operations
- Composite index column ordering
- Filtered/partial index predicate matching
- Columnstore for analytical aggregation
- Hash index for equality lookups
- GIN for containment queries
- GiST for range/overlap types
- Unique index for DISTINCT elimination
- Index for ORDER BY elimination
- Index for GROUP BY optimization
- Index for MIN/MAX optimization
- Index merge intersection
- Index-only COUNT optimization
