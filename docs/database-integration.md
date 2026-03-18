# Database Metadata Integration

The `ra-metadata` crate provides tools to connect to live databases, gather schema and statistics from system catalogs, parse EXPLAIN plans, and compare RA optimizer recommendations against actual database query plans.

## Supported Databases

| Database   | Schema Gathering | Statistics | EXPLAIN Parsing |
|-----------|-----------------|------------|-----------------|
| PostgreSQL | pg_class, pg_attribute, pg_constraint, pg_stats | pg_stats, pg_stat_user_indexes | EXPLAIN (FORMAT JSON) |
| MySQL      | information_schema | information_schema.STATISTICS | EXPLAIN FORMAT=JSON |
| SQLite     | PRAGMA table_info, index_list | sqlite_stat1 | EXPLAIN QUERY PLAN |

## Architecture

The crate is organized as **query definitions and result parsers** that are independent of any specific database client library. This allows callers to use their preferred driver (sync/async, tokio-postgres, sqlx, rusqlite, etc.) while reusing the catalog query logic and output parsers.

### Modules

- `connector` - Core types (`SchemaInfo`, `TableInfo`, `ColumnInfo`, etc.) and the `DatabaseConnector` trait
- `explain` - EXPLAIN plan parsing for PostgreSQL JSON, MySQL JSON, and SQLite text formats
- `postgres` - PostgreSQL-specific catalog queries (`PostgresQueries`) and result parsers
- `mysql` - MySQL-specific catalog queries (`MySqlQueries`) and result parsers
- `sqlite` - SQLite PRAGMA commands (`SqliteQueries`) and result parsers
- `diff` - Differential validator for comparing RA plans with database EXPLAIN plans
- `error` - Error types for metadata operations

## Connection Strings

```
postgresql://user:password@host:5432/database
postgres://user:password@host/database
mysql://user:password@host:3306/database
sqlite:///path/to/database.db
sqlite://relative/path.db
/path/to/file.sqlite3
```

## CLI Commands

### gather-metadata

Gather schema metadata from a database and write to JSON.

```bash
ra-cli gather-metadata --db postgresql://localhost/mydb --output schema.json
```

### compare

Compare an RA optimizer plan with a database EXPLAIN plan.

```bash
ra-cli compare --sql "SELECT * FROM orders o JOIN users u ON o.user_id = u.id" --schema schema.json
```

## Usage with Database Drivers

### PostgreSQL Example

```rust
use ra_metadata::postgres::{PostgresQueries, PgTableRow, build_pg_schema};

// 1. Get the SQL query
let sql = PostgresQueries::list_tables_sql();

// 2. Execute with your driver (e.g., tokio-postgres)
// let rows = client.query(sql, &[]).await?;

// 3. Map rows to intermediate types
// let table_rows: Vec<PgTableRow> = rows.iter().map(|r| PgTableRow { ... }).collect();

// 4. Build the schema
// let schema = build_pg_schema("mydb", &table_rows, &columns, ...);
```

### SQLite Example

```rust
use ra_metadata::sqlite::{SqliteQueries, SqliteColumnRow, build_sqlite_schema};

// 1. Get the PRAGMA command
let pragma = SqliteQueries::table_info_pragma("users");

// 2. Execute with rusqlite
// let stmt = conn.prepare(&pragma)?;

// 3. Map to intermediate types and build schema
// let schema = build_sqlite_schema("mydb", &tables, &columns, ...);
```

### Parsing EXPLAIN Output

```rust
use ra_metadata::explain::{parse_postgres_explain, parse_mysql_explain, parse_sqlite_explain};

// PostgreSQL: EXPLAIN (FORMAT JSON) SELECT ...
let plan = parse_postgres_explain(json_text, "SELECT * FROM users")?;

// MySQL: EXPLAIN FORMAT=JSON SELECT ...
let plan = parse_mysql_explain(json_text, "SELECT * FROM users")?;

// SQLite: EXPLAIN QUERY PLAN SELECT ...
let plan = parse_sqlite_explain(text_output, "SELECT * FROM users")?;
```

### Differential Validation

```rust
use ra_metadata::diff::compare_plans;
use ra_core::algebra::RelExpr;

let ra_plan = RelExpr::scan("users");
let explain = /* parse EXPLAIN output */;
let report = compare_plans(&ra_plan, &explain);

println!("Confidence: {:.0}%", report.confidence * 100.0);
for agreement in &report.agreements {
    println!("  AGREE: {} - {}", agreement.aspect, agreement.explanation);
}
for disagreement in &report.disagreements {
    println!("  DIFFER: {} - {}", disagreement.aspect, disagreement.explanation);
}
```

## Testing with Docker

Start database containers:

```bash
docker compose --profile test-db up -d
```

Connection strings for test containers:

```
postgresql://ra_test:ra_test_pass@localhost:5432/ra_test
mysql://ra_test:ra_test_pass@localhost:3306/ra_test
```

Run integration tests:

```bash
cargo test --test metadata_test
```

## Comparison Aspects

The differential validator compares plans on these aspects:

| Aspect | Description | Confidence |
|--------|-------------|------------|
| Table Access | Which tables are accessed | 0.9 |
| Join Order | Order of join operations | 0.7 |
| Join Algorithm | Hash/nested-loop/merge selection | 0.7 |
| Index Usage | Whether indexes are used | 0.5 |
| Filter Placement | Where predicates are applied | 0.8 |
| Aggregation Strategy | Presence of GROUP BY operations | 0.8 |
| Sort Operation | Presence of ORDER BY operations | 0.8 |
