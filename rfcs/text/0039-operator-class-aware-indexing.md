# RFC 0039: Operator Class Aware Index Selection

## Status
PROPOSED

## Summary
Enhance index selection to understand operator classes, enabling proper use of specialized indexes like GiST, GIN, and custom operator classes for user-defined types.

## Motivation
RA currently assumes all indexes support standard comparison operators. PostgreSQL's operator class system allows indexes to support different operations (e.g., GiST for spatial queries, GIN for text search). Without operator class awareness, RA cannot utilize these specialized indexes, missing significant optimization opportunities.

## Design

### Operator Class Model

```rust
pub struct OperatorClass {
    name: String,
    index_type: IndexType,
    supported_operators: Vec<OperatorSignature>,
    strategy_numbers: HashMap<Operator, StrategyNumber>,
}

pub enum IndexType {
    BTree,
    Hash,
    GiST,
    GIN,
    BRIN,
    Custom(String),
}

pub struct IndexCapabilities {
    operator_class: OperatorClass,
    can_order: bool,
    can_unique: bool,
    can_multicolumn: bool,
    supports_bitmap_scan: bool,
}
```

### Operator Matching

```rust
impl IndexSelector {
    fn matches_predicate(
        &self,
        index: &Index,
        predicate: &Predicate,
    ) -> bool {
        let op_class = &index.operator_class;

        // Check if operator is supported
        op_class.supports_operator(predicate.operator)
            && self.type_compatible(index.column_type, predicate.value_type)
    }
}
```

### Specialized Index Patterns

1. **Text Search (GIN)**:
   - `@@` text search operator
   - `@>` array contains
   - `?` jsonb key exists

2. **Spatial (GiST)**:
   - `&&` bounding box overlap
   - `@` contained by
   - `<->` distance operator

3. **Range Types (GiST/SP-GiST)**:
   - `&&` range overlap
   - `@>` contains range
   - `<@` contained by range

### Cost Adjustments

```rust
fn index_scan_cost_with_opclass(
    index: &Index,
    operator: &Operator,
    selectivity: f64,
) -> Cost {
    let base_cost = match index.index_type {
        IndexType::BTree => btree_cost(selectivity),
        IndexType::GIN => gin_cost(selectivity),
        IndexType::GiST => gist_cost(selectivity),
        _ => default_cost(selectivity),
    };

    // Adjust for operator complexity
    base_cost * operator_cost_factor(operator)
}
```

## Implementation Plan

1. Define operator class abstraction
2. Catalog standard operator classes
3. Modify index selection logic
4. Add operator class to index metadata
5. Implement specialized cost models
6. Add support for custom operator classes

## Alternatives Considered

- **Hard-code Common Cases**: Not extensible
- **Ignore Specialized Indexes**: Miss optimizations
- **Full Operator Family Model**: Too complex initially

## Success Criteria

- Support all PostgreSQL standard operator classes
- Correctly select GiST/GIN indexes when beneficial
- 10x+ speedup for text search and spatial queries
- Extensible to custom operator classes