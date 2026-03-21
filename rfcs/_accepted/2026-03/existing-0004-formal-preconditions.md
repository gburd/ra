# RFC 0004: Formal Preconditions System

- **Status:** Implemented
- **Type:** Retroactive
- **Author:** RA Contributors
- **Date:** 2026-03-20

---

## Summary

A declarative precondition language that allows optimization rules to
specify when they are applicable. Preconditions reference system
facts (statistics, hardware capabilities, schema metadata, runtime
state) and are evaluated before attempting to apply a rule.

## Motivation

With 666+ rules, the optimizer needed an efficient way to filter
rules based on applicability. Previously, preconditions were expressed
informally in prose or embedded as Rust code guards, making them
opaque to analysis and impossible to evaluate generically.

## What Was Built

### Precondition Types

Three precondition types are supported:

1. **Pattern constraints** -- structural patterns that must match in
   the e-graph:
   ```yaml
   - type: pattern
     must_match: "(filter ?pred (join inner ?cond ?left ?right))"
   ```

2. **Predicates** -- boolean conditions on matched variables:
   ```yaml
   - type: predicate
     condition: "is_deterministic(?pred)"
   ```

3. **System facts** -- comparisons against runtime facts:
   ```yaml
   - type: fact
     fact_type: statistics.cardinality
     table: "?left"
     comparator: ">"
     threshold: 10000
   ```

### Fact Provider

The `FactProvider` trait abstracts access to system state:

- `StatisticsProvider` -- cardinality, selectivity, histograms
- `HardwareProvider` -- CPU, memory, GPU capabilities
- `SchemaProvider` -- indexes, constraints, column types
- `RuntimeProvider` -- current memory usage, parallelism level

### Rule Frontmatter

Preconditions are serialized as YAML frontmatter in `.rra` rule
files, making them machine-readable and self-documenting.

## Key Design Decisions

- YAML frontmatter chosen over Rust annotations for portability
  and tooling access
- Fact providers are trait-based to support testing with mock data
- Optional preconditions (marked `optional: true`) allow rules to
  fire with degraded confidence when facts are unavailable
- Preconditions are evaluated before pattern matching to short-circuit
  inapplicable rules early

## Prior Art

- Apache Calcite's `RelOptRuleOperand` for structural matching
- PostgreSQL's `pathkeys` for ordering requirements
- Oracle's rule-based optimizer with hardcoded applicability checks

## References

- `docs/PRECONDITIONS.md` -- full specification
- `docs/FACTS_PROVIDER.md` -- fact provider documentation
- `crates/ra-core/src/precondition.rs` -- implementation
- `crates/ra-engine/src/fact_provider.rs` -- fact provider traits
