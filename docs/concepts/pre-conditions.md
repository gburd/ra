# Formal Pre-Condition System for RRA Rules

## Overview

The RA optimizer includes a formal pre-condition system that allows optimization rules to declaratively specify when they are applicable. Pre-conditions can reference system facts (statistics, hardware, schema, runtime state) and are evaluated before attempting to apply a rule.

## Purpose

**Problem:** With 1,327+ rules, the optimizer needs an efficient way to filter rules based on their applicability. Previously, pre-conditions were expressed informally in prose or embedded as Rust code guards.

**Solution:** A declarative pre-condition language that:
- Is machine-readable and evaluable
- References system facts (statistics, hardware, schema)
- Can be serialized to YAML in rule frontmatter
- Enables automatic rule filtering

## Pre-Condition Types

### 1. Pattern Constraints

Structural patterns that must (or must not) match:

```yaml
preconditions:
  - type: pattern
    must_match: "(filter ?pred (join inner ?cond ?left ?right))"
    description: "Filter above an inner join"
```

### 2. Predicates

Boolean conditions on pattern variables:

```yaml
preconditions:
  - type: predicate
    condition: "is_deterministic(?pred)"
    description: "Predicate must be deterministic"
```

### 3. System Facts

Comparisons against system facts:

```yaml
preconditions:
  - type: fact
    fact_type: statistics.cardinality
    table: "?left"
    comparator: ">"
    threshold: 10000
    confidence: 0.8
    optional: true
    description: "Left table should have >10k rows (optimization hint)"
```

### 4. Database Capabilities

Feature requirements:

```yaml
preconditions:
  - type: capability
    database: "current"
    requires: "lateral_join"
    description: "Requires LATERAL JOIN support"
```

### 5. Composite Conditions

Combine conditions with AND/OR/NOT:

```yaml
preconditions:
  - type: composite
    operator: or
    conditions:
      - type: predicate
        condition: "references_only(?pred, ?left)"
      - type: predicate
        condition: "references_only(?pred, ?right)"
```

## Fact Types

The system recognizes these fact categories:

### Statistics Facts
- `statistics.cardinality` - Row count
- `statistics.ndv` - Number of distinct values
- `statistics.selectivity` - Predicate selectivity
- `statistics.null_fraction` - Fraction of null values
- `statistics.correlation` - Column correlation

### Hardware Facts
- `hardware.memory` - Available memory (bytes)
- `hardware.cpu_cores` - Number of CPU cores
- `hardware.simd_width` - SIMD width (bits)
- `hardware.has_gpu` - GPU availability (boolean)
- `hardware.cache_size` - Cache size (bytes)

### Schema Facts
- `schema.column_type` - Column data type
- `schema.index_exists` - Index existence check
- `schema.has_primary_key` - Primary key existence
- `schema.foreign_keys` - Foreign key constraints
- `schema.table_size` - Table size (bytes)

### Runtime Facts
- `runtime.cardinality_error` - Estimation error ratio
- `runtime.execution_time` - Operator execution time
- `runtime.memory_usage` - Memory usage (bytes)
- `runtime.skew_detected` - Data skew detection

### Database Capabilities
- `database.feature` - Feature support check
- `database.dialect` - SQL dialect
- `database.version` - Database version

## Comparison Operators

- `>`, `>=`, `<`, `<=` - Numeric comparisons
- `==`, `=`, `!=` - Equality/inequality
- `contains` - String containment
- `starts_with`, `ends_with` - String prefix/suffix

## Optional Pre-Conditions

Pre-conditions marked as `optional: true` are treated as optimization hints. If they fail, the rule is still considered applicable, but may be deprioritized.

```yaml
preconditions:
  - type: fact
    fact_type: statistics.cardinality
    table: "?table"
    comparator: ">"
    threshold: 100000
    optional: true
    description: "Rule is more effective on large tables"
```

## Examples

### Basic Filter Pushdown

```yaml
---
id: filter-through-join
name: Filter Pushdown Through Join
preconditions:
  - type: pattern
    must_match: "(filter ?pred (join inner ?cond ?left ?right))"
  - type: predicate
    condition: "is_deterministic(?pred)"
  - type: predicate
    condition: "references_only(?pred, ?left) OR references_only(?pred, ?right)"
---
```

### Join Commutativity with Statistics

```yaml
---
id: join-commutativity
name: Join Commutativity
preconditions:
  - type: pattern
    must_match: "(join inner ?cond ?left ?right)"
  - type: fact
    fact_type: statistics.cardinality
    table: "?right"
    comparator: "<"
    threshold:
      expression: "cardinality(?left)"
    confidence: 0.7
    optional: true
    description: "Prefer smaller table as build side"
---
```

### Hardware-Aware Rule

```yaml
---
id: gpu-hash-join
name: GPU Hash Join
preconditions:
  - type: pattern
    must_match: "(join inner ?cond ?left ?right)"
  - type: fact
    fact_type: hardware.has_gpu
    comparator: "=="
    threshold: true
  - type: fact
    fact_type: statistics.cardinality
    table: "?left"
    comparator: ">"
    threshold: 1000000
    description: "GPU join beneficial for large tables"
---
```

## Implementation

### Rule Metadata

Pre-conditions are stored in the `RuleMetadata` struct:

```rust
pub struct RuleMetadata {
    pub id: String,
    pub name: String,
    // ...
    pub preconditions: Vec<PreCondition>,
}
```

### Pre-Condition Types

```rust
pub enum PreCondition {
    Pattern {
        must_match: Option<String>,
        must_not_match: Option<String>,
        description: Option<String>,
        optional: bool,
    },
    Predicate {
        condition: String,
        description: Option<String>,
        optional: bool,
    },
    Fact {
        fact_type: String,
        table: Option<String>,
        column: Option<String>,
        comparator: String,
        threshold: FactValue,
        confidence: Option<f64>,
        description: Option<String>,
        optional: bool,
    },
    Capability {
        database: String,
        requires: String,
        description: Option<String>,
        optional: bool,
    },
    Composite {
        operator: LogicalOperator,
        conditions: Vec<PreCondition>,
        description: Option<String>,
        optional: bool,
    },
}
```

## Migration Guide

### Converting Existing Rules

**Before:**

```markdown
## Preconditions

```rust
fn applicable(join_type: JoinType, pred: &Expr) -> bool {
    matches!(join_type, JoinType::Inner) && pred.is_deterministic()
}
```

**After:**

```yaml
preconditions:
  - type: pattern
    must_match: "(join inner ?cond ?left ?right)"
  - type: predicate
    condition: "is_deterministic(?pred)"
```

### Semi-Automated Migration

Use the CLI tool to assist with migration:

```bash
# Extract preconditions from existing rules
ra-cli migrate-preconditions \
    --input rules/logical/predicate-pushdown/*.rra \
    --output rules-migrated/ \
    --validate

# Validate migration
ra-cli validate-preconditions \
    --baseline rules/ \
    --migrated rules-migrated/
```

## Best Practices

1. **Be Specific:** Use pattern constraints to narrow matches early
2. **Mark Optional Facts:** If statistics are helpful but not required, mark as optional
3. **Document Intent:** Always include description fields
4. **Test Coverage:** Add test cases that verify pre-conditions work correctly
5. **Confidence Thresholds:** Specify minimum confidence for statistics-based conditions

## Integration with Optimizer

The optimizer uses pre-conditions to filter applicable rules:

```rust
// Filter rules based on available facts
let applicable = optimizer.applicable_rules(expr, facts);

// Run optimization with filtered rules
let optimized = optimizer.optimize_with_facts(expr, facts)?;
```

## Future Work

- **Phase 3:** Implement `FactsContext` aggregator
- **Phase 4:** Implement `PreConditionEvaluator`
- **Phase 6:** Integrate with optimizer
- **Phase 9-12:** Migrate remaining ~1200 rules
- **Phase 13-16:** Build database adapters (Stoolap, PostgreSQL)

## See Also

- [FactsProvider API](FACTS_PROVIDER.md)
- [Database Integration Guide](DATABASE_INTEGRATION.md)
- [Rule Format Specification](RULE_FORMAT.md)
