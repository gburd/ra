# ra-dialect

SQL dialect translation for cross-database compatibility.

## Features

- **Native Backend (Default)**: Hand-written translation logic for 6 core dialects
- **Polyglot Backend (Optional)**: Integration with polyglot-sql transpiler for 32+ dialects

## Supported Dialects

### Core Dialects (Native Backend)
- PostgreSQL (9.6+)
- MySQL (5.7+ / 8.0+)
- SQLite (3.x)
- DuckDB
- Microsoft SQL Server (2016+)
- Oracle Database (12c+)

### Extended Dialects (Polyglot Backend)
When the `polyglot-backend` feature is enabled, the following additional dialects are supported:

- Google BigQuery
- Snowflake
- Databricks
- Amazon Redshift
- ClickHouse
- Trino (formerly PrestoSQL)
- Presto
- Amazon Athena
- Apache Hive
- Apache Spark SQL
- Teradata
- Exasol
- Microsoft Fabric
- Dremio
- Apache Drill
- Apache Druid
- CockroachDB
- Materialize
- RisingWave
- SingleStore (formerly MemSQL)
- StarRocks
- Apache Doris
- TiDB
- Tableau
- Apache Solr
- Dune Analytics

## Usage

### Basic Usage (Native Backend)

```rust
use ra_dialect::{Dialect, DialectTranslator};

let translator = DialectTranslator::new(
    Dialect::PostgreSql,
    Dialect::MySql,
);

let result = translator
    .translate("SELECT first_name || ' ' || last_name FROM users")
    .unwrap();

// MySQL uses CONCAT() instead of ||
assert!(result.sql.contains("CONCAT"));
```

### Using Polyglot Backend

First, enable the feature in your `Cargo.toml`:

```toml
[dependencies]
ra-dialect = { version = "...", features = ["polyglot-backend"] }
```

Then use it in your code:

```rust
use ra_dialect::{Dialect, DialectTranslator, TranslationBackend};

// Translate from PostgreSQL to BigQuery
let translator = DialectTranslator::with_backend(
    Dialect::PostgreSql,
    Dialect::BigQuery,
    TranslationBackend::Polyglot,
);

let result = translator
    .translate("SELECT * FROM users WHERE age > 18")
    .unwrap();
```

## Choosing a Backend

### When to Use Native Backend

- You only need the 6 core dialects
- Binary size is critical (native backend is lightweight)
- You need custom translation logic or fine-grained control
- You want predictable, hand-crafted translations

### When to Use Polyglot Backend

- You need support for modern cloud databases (BigQuery, Snowflake, Databricks, etc.)
- You require high-fidelity dialect translation with extensive test coverage
- You're working with multiple diverse SQL dialects
- Binary size increase is acceptable (adds ~2-3 MB)

## Performance

The native backend is generally faster for simple translations due to its focused scope. The polyglot backend provides more comprehensive translation at the cost of slightly higher latency.

Benchmark results (example):
- Native backend: ~50$\mu$s for simple SELECT
- Polyglot backend: ~200$\mu$s for simple SELECT
- Both backends handle complex queries in 1-5ms

## Migration Guide

### From Native to Polyglot

1. Add the feature to your `Cargo.toml`:
   ```toml
   ra-dialect = { version = "...", features = ["polyglot-backend"] }
   ```

2. Update your code to specify the backend:
   ```rust
   // Before (uses native by default)
   let translator = DialectTranslator::new(source, target);

   // After (explicit backend selection)
   let translator = DialectTranslator::with_backend(
       source,
       target,
       TranslationBackend::Polyglot,
   );
   ```

3. Test thoroughly as translation results may differ between backends

## Compatibility Matrix

Use the `CompatibilityMatrix` to understand feature support across dialects:

```rust
use ra_dialect::CompatibilityMatrix;

let matrix = CompatibilityMatrix::build();
let table = matrix.to_table();
println!("{}", table);
```

## Contributing

When adding new dialect support:
1. Add the dialect to the `Dialect` enum (feature-gated if polyglot-only)
2. Update the Display implementation
3. Add mapping in the polyglot backend
4. Add comprehensive tests
5. Update documentation

## License

Licensed under the same terms as the parent RA project.