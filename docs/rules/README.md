# RA Rule System Documentation

## Overview

The RA query optimizer employs a comprehensive rule-based optimization system with **1,354 transformation rules** organized into hierarchical categories. Each rule represents a specific query transformation technique derived from database research, industry practice, or system-specific optimizations.

## Rule Structure

Each rule file (`.rra` format) contains:
- **YAML frontmatter**: Metadata including ID, name, category, supported databases, preconditions
- **Markdown documentation**: Description, rationale, and examples
- **Formal specifications**: Relational algebra notation and implementation patterns
- **Cost model**: Benefit estimation and applicability conditions
- **Test cases**: SQL examples showing before/after transformations
- **References**: Links to papers, system implementations, and research

## Categories

| Category | Rules | Description |
|----------|-------|-------------|
| [Database-Specific](./database-specific/) | 403 | System-specific optimizations for PostgreSQL, MySQL, DuckDB, etc. |
| [Logical](./logical/) | 352 | Query logical transformations (predicate pushdown, join elimination, etc.) |
| [Physical](./physical/) | 145 | Physical operator selection and implementation strategies |
| [Execution Models](./execution-models/) | 124 | Different query execution paradigms (vectorized, compiled, etc.) |
| [Distributed](./distributed/) | 118 | Distributed query optimization and execution strategies |
| [Experimental](./experimental/) | 56 | Cutting-edge research techniques and experimental optimizations |
| [Cost Models](./cost-models/) | 50 | Cardinality estimation and cost calculation strategies |
| [Multi-Model](./multi-model/) | 30 | Optimizations for document, graph, and time-series queries |
| [Federated](./federated/) | 24 | Cross-database query optimization |
| [Hardware](./hardware/) | 21 | Hardware-specific optimizations (GPU, FPGA, NUMA) |
| [RPR](./rpr/) | 19 | Robust Plan Reuse optimizations |
| [Unnest](./unnest/) | 5 | Array and nested data unnesting |
| [Templates](./templates/) | 4 | Rule templates and patterns |
| [Parallel](./parallel/) | 3 | Parallel query execution strategies |

## Navigation

- **[Complete Rule Index](./INDEX.md)** - Alphabetical listing of all rules
- **[By Category](./BY_CATEGORY.md)** - Rules grouped by category
- **[By Database](./BY_DATABASE.md)** - Rules by database system support
- **[Research References](./REFERENCES.md)** - Academic papers and system documentation

## Rule Format

### Example: Filter Into Join Condition

```yaml
---
id: filter-into-join-condition
name: Filter Absorption Into Join Condition
category: logical/predicate-pushdown
databases: [postgresql, mysql, duckdb, sqlite, oracle, mssql]
standard: "sql:1992"
preconditions:
  - type: pattern
    must_match: "(filter ?pred (join inner ?cond ?left ?right))"
---
```

### Pattern Matching

Rules use s-expression patterns for matching:
- `?variable` - Pattern variable binding
- `(operator args...)` - Operator with arguments
- `...` - Variable number of arguments

### Preconditions

Rules specify applicability conditions:
- **Pattern matching**: Structural requirements
- **Predicate conditions**: Semantic requirements (e.g., deterministic expressions)
- **Cost thresholds**: Minimum benefit requirements

## Using Rules

1. **Pattern Recognition**: The optimizer matches query patterns against rule patterns
2. **Precondition Checking**: Validates rule applicability
3. **Cost Estimation**: Evaluates transformation benefit
4. **Application**: Applies beneficial transformations
5. **Iteration**: Repeats until no beneficial rules apply

## Contributing

To add new rules:
1. Create `.rra` file in appropriate category directory
2. Include complete YAML metadata
3. Document with examples and references
4. Add test cases demonstrating the transformation
5. Update category index

## Research Foundation

The rule collection draws from:
- **Classic Papers**: System R, Volcano, Cascades optimizers
- **Modern Systems**: PostgreSQL, MySQL, DuckDB, Apache Calcite
- **Research**: SIGMOD, VLDB, ICDE conference proceedings
- **Industry Practice**: Production database optimizations

See [REFERENCES.md](./REFERENCES.md) for complete bibliography.