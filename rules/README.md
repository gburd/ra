# Relational Algebra Rules

This directory contains transformation rules in `.rra` (Relational Rule Algebra) format.

## Directory Structure

- `logical/` - Logical transformations (predicate pushdown, join reordering, etc.)
- `physical/` - Physical optimizations (join algorithms, index selection, etc.)
- `database-specific/` - Database-specific optimizations
- `execution-models/` - Execution model specific rules
- `cost-models/` - Cost estimation rules
- `experimental/` - Research and experimental rules

## Rule Format

Each rule is a literate markdown file (`.rra`) with:

1. **YAML frontmatter** - Metadata (id, name, category, databases, etc.)
2. **Description** - Human-readable explanation
3. **Relational Algebra** - Formal notation of the transformation
4. **Implementation** - Rust code (egg rewrite rules)
5. **Preconditions** - When the rule applies
6. **Cost Model** - Estimated benefit
7. **Test Cases** - SQL examples
8. **References** - Source code and papers

## Example Rule

```markdown
---
id: filter-through-join
name: Filter Pushdown Through Join
category: logical/predicate-pushdown
databases: [postgresql, mysql, duckdb, sqlite]
standard: sql:1992
---

# Filter Pushdown Through Join

## Description
Pushes selection predicates through join operators...

## Relational Algebra
σ[p](R ⋈[c] S) → (σ[p](R)) ⋈[c] S  where attrs(p) ⊆ attrs(R)

## Implementation
[Rust code]

## Test Cases
[SQL examples]
```

## Writing Rules

See [docs/guides/rule-authoring.md](../docs/guides/rule-authoring.md) for the complete guide.

## Rule Index

The `index.toml` file contains metadata for all rules and is automatically
generated. Do not edit it manually.
