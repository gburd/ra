# RFC 0008: Multi-Database Dialect Translation

- **Status:** Implemented
- **Type:** Retroactive
- **Author:** RA Contributors
- **Date:** 2026-03-20

---

## Summary

A SQL dialect translation system that converts SQL statements between
PostgreSQL, MySQL, SQLite, DuckDB, MSSQL, and Oracle, handling
syntax differences, function name mappings, operator variations,
and feature availability.

## Motivation

Database systems implement SQL with varying syntax, function names,
operators, and feature support. Users migrating between databases,
running cross-database tests, or building database-agnostic tools
need automatic translation. The RA optimizer also needs dialect
awareness to generate valid SQL for each target database when
outputting optimized queries.

## What Was Built

### Supported Dialects

| Dialect    | Version | Notes |
|------------|---------|-------|
| PostgreSQL | 9.6+    | Primary dialect |
| MySQL      | 5.7+    | Including 8.0 features |
| SQLite     | 3.x     | With extension functions |
| DuckDB     | 0.8+    | Analytical functions |
| MSSQL      | 2016+   | T-SQL translation |
| Oracle     | 12c+    | PL/SQL basics |

### Translation Engine

The `DialectTranslator` performs:

- **String concatenation**: `||` (PG) vs `CONCAT()` (MySQL)
- **Type casting**: `::type` (PG) vs `CAST(x AS type)` (standard)
- **Date functions**: `NOW()` vs `GETDATE()` vs `datetime('now')`
- **Limit syntax**: `LIMIT/OFFSET` vs `TOP` vs `FETCH FIRST`
- **Boolean literals**: `TRUE/FALSE` vs `1/0`
- **Identifier quoting**: `"double"` vs `` `backtick` ``

### Compatibility Matrix

`CompatibilityMatrix` generates a feature support table across all
dialects, helping users understand what translations are lossless
vs lossy.

### Crate

The `ra-dialect` crate provides `Dialect`, `DialectTranslator`,
`CompatibilityMatrix`, and per-dialect formatting modules.

## Key Design Decisions

- Translation is SQL-to-SQL (text level) using the parsed AST,
  not plan-level conversion
- Lossy translations (where the target dialect lacks a feature)
  emit warnings rather than failing silently
- The compatibility matrix is generated from dialect metadata,
  not hardcoded
- Each dialect has a formatter module that handles quoting,
  keywords, and syntax conventions

## Prior Art

- jOOQ's SQL dialect abstraction (Java)
- SQLAlchemy's dialect system (Python)
- Apache Calcite's SQL parser with dialect support

## References

- `docs/dialect-translation.md` -- full documentation
- `crates/ra-dialect/` -- translation engine and dialect modules
