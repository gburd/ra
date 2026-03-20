# Database Metadata Integration

The `ra-metadata` crate connects the RA optimizer to live databases, enabling
schema introspection, statistics gathering, EXPLAIN plan parsing, and
differential plan validation.

## Supported Databases

| Database   | Schema | Statistics | EXPLAIN Parsing |
|------------|--------|------------|-----------------|
| PostgreSQL | Yes    | Yes        | JSON format     |
| MySQL      | Yes    | Yes        | JSON format     |
| SQLite     | Yes    | Yes        | QUERY PLAN      |

## Architecture

```
ra-metadata/
  connector.rs      DatabaseConnector trait
  schema.rs         SchemaInfo, TableInfo, ColumnInfo, IndexInfo, etc.
  explain.rs        ExplainPlan, ExplainNode, NodeType + parsers
  diff_validator.rs PlanComparison: RA plan vs DB EXPLAIN
  postgres.rs       PostgresConnector (pg_stats, pg_class, pg_attribute)
  mysql.rs          MySqlConnector (information_schema)
  sqlite.rs         SqliteConnector (PRAGMA commands)
  error.rs          MetadataError variants
```

## DatabaseConnector Trait

```rust
pub trait DatabaseConnector {
    fn kind(&self) -> DatabaseKind;
    fn gather_schema(&self) -> MetadataResult<SchemaInfo>;
    fn gather_statistics(&self, table: &str) -> MetadataResult<TableStats>;
    fn explain_query(&self, sql: &str) -> MetadataResult<ExplainPlan>;
}
```

PostgreSQL and MySQL connectors use `&mut self` methods (`gather_schema_mut`,
`gather_statistics_mut`, `explain_query_mut`) because their underlying client
types require mutable access. The immutable trait methods return
`MetadataError::Unsupported` directing callers to the `_mut` variants.

SQLite implements the trait directly since `rusqlite::Connection` supports
`&self` queries.

## Schema Types

`SchemaInfo` contains a map of `TableInfo` entries, each holding:

- `columns: Vec<ColumnInfo>` -- name, data type, nullability, ordinal, default
- `constraints: Vec<ConstraintInfo>` -- primary key, foreign key, unique, check
- `indexes: Vec<IndexInfo>` -- name, columns, uniqueness, index type
- `estimated_rows: Option<f64>` -- row count estimate

`TableStats` provides per-table statistics including `row_count`,
`total_bytes`, and per-column `ColumnStatistics` (distinct count, null
fraction, most common values, histogram bounds). Call `to_core_statistics()`
to convert into `ra_core::Statistics` for the cost model.

## EXPLAIN Plan Parsing

Three parsers convert database-specific EXPLAIN output into a unified
`ExplainPlan` tree:

- `parse_postgres_explain(json)` -- PostgreSQL `EXPLAIN (FORMAT JSON)`
- `parse_mysql_explain(json)` -- MySQL `EXPLAIN FORMAT=JSON`
- `parse_sqlite_explain(text)` -- SQLite `EXPLAIN QUERY PLAN` pipe-delimited

Each `ExplainNode` carries:

- `node_type: NodeType` -- SeqScan, IndexScan, HashJoin, Sort, etc.
- `join_type: Option<JoinType>` -- Inner, Left, Right, Full, etc.
- `relation`, `index_name`, `filter` -- scan metadata
- `startup_cost`, `total_cost`, `estimated_rows` -- cost estimates
- `children: Vec<ExplainNode>` -- child operators

## Differential Validator

`compare_plans(ra_plan, db_explain)` compares the RA optimizer's plan tree
against a database EXPLAIN plan and produces a `PlanComparison`:

```rust
pub struct PlanComparison {
    pub agreements: Vec<PlanAgreement>,
    pub disagreements: Vec<PlanDisagreement>,
    pub confidence: f64,  // 0.0 to 1.0
}
```

Comparison aspects: `AccessMethod`, `IndexSelection`, `FilterPlacement`,
`JoinStrategy`, `AggregationStrategy`, `SortStrategy`.

Each disagreement includes a severity level (`Info`, `Warning`, `High`).

## CLI Commands

### gather-metadata

Load a schema JSON file and write normalized metadata:

```sh
ra-cli gather-metadata --schema schema.json --output normalized.json
```

Use `--verbose` to list tables with column and index counts.

### compare

Compare the RA optimizer plan for a SQL query against a database EXPLAIN plan:

```sh
ra-cli compare \
  --sql "SELECT * FROM users JOIN orders ON users.id = orders.user_id" \
  --explain-json explain.json
```

Output includes both plan trees, a confidence score, and itemized
agreements/disagreements with explanations.

## Connecting to Databases

### PostgreSQL

```rust
use ra_metadata::postgres::PostgresConnector;

let mut conn = PostgresConnector::connect(
    "host=localhost dbname=mydb user=postgres"
)?;
let schema = conn.gather_schema_mut()?;
let stats = conn.gather_statistics_mut("users")?;
let plan = conn.explain_query_mut("SELECT * FROM users WHERE id = 1")?;
```

Queries `pg_class`, `pg_attribute`, `pg_constraint`, `pg_stats`,
`pg_indexes` for metadata. Statistics include most common values and
histogram bounds from `pg_stats`.

### MySQL

```rust
use ra_metadata::mysql::MySqlConnector;

let mut conn = MySqlConnector::connect(
    "mysql://root:password@localhost/mydb"
)?;
let schema = conn.gather_schema_mut()?;
let stats = conn.gather_statistics_mut("users")?;
let plan = conn.explain_query_mut("SELECT * FROM users WHERE id = 1")?;
```

Queries `information_schema.TABLES`, `COLUMNS`, `TABLE_CONSTRAINTS`,
`KEY_COLUMN_USAGE`, `STATISTICS` for metadata.

### SQLite

```rust
use ra_metadata::sqlite::SqliteConnector;

let conn = SqliteConnector::connect("/path/to/db.sqlite")?;
// or: SqliteConnector::open_in_memory()?;
let schema = conn.gather_schema()?;
let stats = conn.gather_statistics("users")?;
let plan = conn.explain_query("SELECT * FROM users WHERE id = 1")?;
```

Uses `PRAGMA table_info`, `PRAGMA index_list`, `PRAGMA index_info`,
`PRAGMA foreign_key_list`, and `sqlite_stat1` for metadata. Falls back to
`SELECT COUNT(*)` when `sqlite_stat1` is unavailable.

## Converting Statistics to Core Types

```rust
let stats = conn.gather_statistics("users")?;
let core_stats = stats.to_core_statistics();
// core_stats: ra_core::Statistics { row_count, column_stats }
```

This bridges database-gathered statistics into the RA optimizer's cost model.

## Error Handling

All operations return `MetadataResult<T>` (alias for
`Result<T, MetadataError>`). Error variants:

- `Connection` -- failed to connect
- `Query` -- SQL query failed
- `SchemaIntrospection` -- catalog query failed for a specific object
- `StatisticsGathering` -- statistics query failed for a specific table
- `ExplainParse` -- EXPLAIN output could not be parsed
- `Unsupported` -- operation not supported by this connector variant

## Testing

The crate includes 110 unit tests covering:

- Schema type construction and serialization round-trips
- EXPLAIN plan parsing for all three database formats
- Differential validator plan comparisons
- SQLite integration tests with in-memory databases
- Edge cases (empty plans, missing tables, nullable columns)

Run tests with:

```sh
cargo test -p ra-metadata
```
