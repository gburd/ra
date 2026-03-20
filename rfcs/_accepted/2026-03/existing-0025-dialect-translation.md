# RFC 0025: Multi-Database Dialect Translation

**Status:** Accepted
**Implemented:** Prior to 2026-03
**Commit:** Various

## Summary

Implemented SQL dialect translation system that converts queries between PostgreSQL, MySQL, SQLite, DuckDB, SQL Server, and Oracle dialects. The translator handles syntax differences, function mappings, operator conversions, and feature compatibility checks.

## Motivation

Organizations often need to:
- Migrate between database systems
- Support multiple database backends
- Run benchmarks across platforms
- Develop database-agnostic applications

SQL standardization is incomplete, with each database having:
- Unique syntax extensions
- Proprietary functions
- Different operator precedence
- Varying feature support

Manual translation is error-prone and maintenance-intensive.

## Technical Design

### Dialect Abstraction

```rust
pub enum Dialect {
    PostgreSql,
    MySql,
    Sqlite,
    DuckDb,
    SqlServer,
    Oracle,
}
```

### Translation Pipeline

1. **Parse** source SQL into AST
2. **Analyze** dialect-specific constructs
3. **Transform** to target dialect
4. **Generate** output SQL
5. **Validate** compatibility

### Feature Mapping

**String Concatenation:**
- PostgreSQL: `'a' || 'b'`
- MySQL: `CONCAT('a', 'b')`
- SQL Server: `'a' + 'b'`

**Limit/Offset:**
- PostgreSQL: `LIMIT 10 OFFSET 5`
- MySQL: `LIMIT 5, 10`
- SQL Server: `OFFSET 5 ROWS FETCH NEXT 10 ROWS ONLY`

**Date Functions:**
- PostgreSQL: `NOW()`
- MySQL: `NOW()`
- SQLite: `datetime('now')`
- Oracle: `SYSDATE`

### Compatibility Matrix

Track feature support across dialects:
```rust
pub struct FeatureSupport {
    pub window_functions: bool,
    pub ctes: bool,
    pub recursive_ctes: bool,
    pub lateral_joins: bool,
    pub full_outer_join: bool,
    pub arrays: bool,
    pub json: bool,
}
```

### Translation Warnings

Report incompatibilities:
```rust
pub enum TranslationWarning {
    FeatureNotSupported(SqlFeature),
    PrecisionLoss(String),
    PerformanceImpact(String),
    SemanticDifference(String),
}
```

### Function Registry

Map functions between dialects:
```rust
pub struct FunctionMapping {
    pub source: (&str, &[ParamType]),
    pub target: (&str, &[ParamType]),
    pub transform: Option<Box<dyn Fn(Vec<Expr>) -> Expr>>,
}
```

Examples:
- `SUBSTR` → `SUBSTRING`
- `IFNULL` → `COALESCE`
- `DATEADD` → `+ INTERVAL`

## Implementation

### Key Files

- `crates/ra-dialect/src/dialect.rs`
  - Dialect enum and feature flags
  - Version-specific handling

- `crates/ra-dialect/src/translator.rs`
  - Main translation engine
  - AST transformation logic
  - SQL generation

- `crates/ra-dialect/src/functions.rs`
  - Function mapping registry
  - Parameter type checking
  - Custom transformations

- `crates/ra-dialect/src/matrix.rs`
  - Compatibility matrix
  - Feature support tracking
  - Warning generation

### Translation Strategy

**Conservative by Default:**
- Preserve semantics over performance
- Emit warnings for differences
- Fail on impossible translations

**Optimization Hints:**
- Suggest indexes for target system
- Recommend native alternatives
- Identify performance risks

## Usage

### CLI Tool

```bash
# Translate single query
ra-cli translate --from postgres --to mysql \
  "SELECT * FROM users WHERE created_at > NOW() - INTERVAL '1 day'"

# Translate entire schema
ra-cli translate --from postgres --to sqlite schema.sql > sqlite_schema.sql

# Check compatibility
ra-cli compat --from postgres --to duckdb query.sql
```

### Library API

```rust
use ra_dialect::{Dialect, DialectTranslator};

let translator = DialectTranslator::new(
    Dialect::PostgreSql,
    Dialect::MySql,
);

let result = translator.translate(sql)?;
println!("Translated: {}", result.sql);
for warning in result.warnings {
    eprintln!("Warning: {}", warning);
}
```

## Testing

Comprehensive test suite:
- Round-trip translation tests
- Feature compatibility matrix
- Edge case handling
- Performance benchmarks
- Real-world query corpus

Test categories:
- DML (SELECT, INSERT, UPDATE, DELETE)
- DDL (CREATE, ALTER, DROP)
- Functions (string, date, math, aggregate)
- Joins (INNER, LEFT, RIGHT, FULL, LATERAL)
- Subqueries (correlated, scalar, EXISTS)
- CTEs (non-recursive, recursive)
- Window functions

## Compatibility

### Supported Versions

- PostgreSQL 12+
- MySQL 8.0+
- SQLite 3.35+
- DuckDB 0.8+
- SQL Server 2019+
- Oracle 19c+

### Known Limitations

- Custom types not translated
- Stored procedures excluded
- Triggers require manual review
- Permissions not migrated

## Use Cases

**Database Migration:**
- One-time schema conversion
- Query compatibility testing
- Gradual migration support

**Multi-Database Applications:**
- ORM query generation
- Cross-platform testing
- Vendor-agnostic tools

**Benchmarking:**
- Run TPC-H across systems
- Compare optimizer behavior
- Performance portability

## Performance

Translation overhead:
- Simple queries: < 1ms
- Complex queries: 10-50ms
- Large schemas: 100-500ms
- Caching reduces repeat cost by 90%

## References

- SQL:2016 Standard
- "SQL Cookbook" (Molinaro)
- Database vendor documentation
- jOOQ SQL translation

## Future Work

- Semantic-preserving rewrites
- Cost model translation
- Index recommendation translation
- Stored procedure conversion
- Schema evolution tracking