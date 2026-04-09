# Parser System

This document explains how the RA parser system works and how to add support for new database engines.

## Overview

The RA parser system supports multiple SQL dialects through a profile-based architecture. Each database vendor and version has a profile defining supported features, operators, and syntax variants.

## Architecture

```
SQL Text
  ↓
Profile Selection (dialect detection or explicit)
  ↓
Tokenization (sqlparser lexer)
  ↓
Parsing (sqlparser with grammar extensions)
  ↓
SQL AST (sqlparser types)
  ↓
Conversion (sql_to_relexpr)
  ↓
RelExpr (ra-core types)
```

## Profile System

### Profile Structure

Profiles are TOML files defining database capabilities:

```toml
# profiles/vendors/postgresql-17.toml
[profile]
name = "postgresql-17"
vendor = "postgresql"
version = "17"
inherits_from = "postgresql-16"  # Optional inheritance

[features]
sql_92 = true
sql_2016 = true
sql_2023 = true
lateral_joins = true
window_functions = true
recursive_cte = true
materialized_cte = true
parallel_query = true

[syntax]
identifier_quote = "\""
string_quote = "'"
supports_backticks = false
dollar_quoted_strings = true

[operators]
array_ops = ["@>", "<@", "&&"]
json_ops = ["->", "->>", "#>", "#>>", "@>", "<@", "?", "?&", "?|", "@?", "@@"]
range_ops = ["@>", "<@", "&&", "<<", ">>", "&<", "&>", "-|-", "+", "*"]
network_ops = ["<<", "<<=", ">>", ">>=", "&&", "~"]
text_search_ops = ["@@", "@@@", "||"]

[functions]
aggregate = ["string_agg", "array_agg", "json_agg", "jsonb_agg"]
window = ["row_number", "rank", "dense_rank", "lead", "lag"]
json = ["json_build_object", "jsonb_set", "jsonb_insert"]
```

### Profile Directories

```
profiles/
├── standards/          # SQL standard features
│   ├── sql-92.toml
│   ├── sql-2016.toml
│   └── sql-2023.toml
├── vendors/            # Database vendor profiles
│   ├── postgresql-16.toml
│   ├── postgresql-17.toml
│   ├── mysql-8.0.toml
│   ├── mysql-8.4.toml
│   ├── oracle-21c.toml
│   └── sqlserver-2022.toml
└── extensions/         # Database extensions
    ├── pgvector.toml
    ├── postgis.toml
    ├── timescaledb.toml
    └── pg_textsearch.toml
```

### Profile Loading

**Load single profile:**

```rust
use ra_parser::ParserProfile;

let profile = ParserProfile::load("postgresql-17")?;
```

**Load profile with extensions:**

```rust
// Single extension
let profile = ParserProfile::load("postgresql-17+pgvector")?;

// Multiple extensions
let profile = ParserProfile::load("postgresql-17+postgis+timescaledb")?;
```

**Automatic dialect detection:**

```rust
let sql = "SELECT ARRAY[1,2,3] FROM users WHERE data @> '{\"key\": \"value\"}'";
let (profile, confidence) = ParserProfile::infer(sql)?;
// Returns postgresql-17 with confidence > 0.8
```

### Profile Inheritance

Profiles can inherit from parent profiles to reduce duplication:

```toml
# postgresql-17.toml
[profile]
name = "postgresql-17"
inherits_from = "postgresql-16"

[features]
sql_2023 = true  # New in PG 17
parallel_hash_join = true
```

When loaded, features from the parent profile are merged with the child.

## Parser Interface

### Basic Usage

```rust
use ra_parser::{Parser, ParserProfile};

// Create parser with explicit profile
let profile = ParserProfile::load("postgresql-17")?;
let mut parser = Parser::new(profile);

// Parse SQL to RelExpr
let sql = "SELECT name, age FROM users WHERE age > 25";
let expr = parser.parse(sql)?;

// Result is RelExpr
match expr {
    RelExpr::Project { input, columns } => {
        match *input {
            RelExpr::Filter { input, predicate } => {
                // ...
            }
        }
    }
}
```

### Error Handling

```rust
match parser.parse(sql) {
    Ok(expr) => {
        // Success
    }
    Err(ParseError::SyntaxError { message, location }) => {
        println!("Syntax error at {}: {}", location, message);
    }
    Err(ParseError::UnsupportedFeature(feature)) => {
        println!("Feature not supported: {}", feature);
    }
    Err(ParseError::AmbiguousQuery(message)) => {
        println!("Ambiguous query: {}", message);
    }
}
```

## Adding a New Database Engine

This section walks through adding support for a new database engine step by step.

### Step 1: Create Vendor Profile

Create `profiles/vendors/newdb-1.0.toml`:

```toml
[profile]
name = "newdb-1.0"
vendor = "newdb"
version = "1.0"

[features]
# Standard SQL support
sql_92 = true
sql_2016 = false
sql_2023 = false

# Query features
subqueries = true
window_functions = true
recursive_cte = false
lateral_joins = false

# Data types
arrays = false
json = true
xml = false

[syntax]
identifier_quote = "`"         # Use backticks for identifiers
string_quote = "'"
supports_backticks = true
dollar_quoted_strings = false
case_sensitive_identifiers = false

[operators]
# Custom operators specific to NewDB
custom_ops = ["<=>", "~*", "!~"]

[functions]
# Built-in functions
string = ["CONCAT", "SUBSTR", "TRIM"]
aggregate = ["COUNT", "SUM", "AVG", "MAX", "MIN"]
date = ["NOW", "DATE_ADD", "DATE_SUB"]
```

### Step 2: Add Grammar Extensions (if needed)

If the database has custom syntax, add grammar extensions:

```rust
// crates/ra-parser/src/grammar/vendors/newdb.rs

use sqlparser::ast::{Expr, Statement};
use sqlparser::parser::{Parser, ParserError};

/// Parse NewDB-specific syntax extensions
pub struct NewDbGrammar;

impl NewDbGrammar {
    /// Parse NewDB's custom MATCH operator
    pub fn parse_match_operator(parser: &mut Parser) -> Result<Expr, ParserError> {
        parser.expect_keyword("MATCH")?;
        let expr = parser.parse_expr()?;
        parser.expect_keyword("AGAINST")?;
        let pattern = parser.parse_expr()?;

        Ok(Expr::Function(Function {
            name: ObjectName(vec![Ident::new("MATCH")]),
            args: vec![
                FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)),
                FunctionArg::Unnamed(FunctionArgExpr::Expr(pattern)),
            ],
            // ... other fields
        }))
    }
}
```

Register the grammar extension:

```rust
// crates/ra-parser/src/grammar/vendors/mod.rs

pub mod mysql;
pub mod postgresql;
pub mod oracle;
pub mod sqlserver;
pub mod newdb;  // Add this line
```

### Step 3: Implement Database Adapter

Create `crates/ra-adapters/src/newdb.rs`:

```rust
use crate::{
    AdapterError, ColumnInfo, DatabaseAdapter, DatabaseCapabilities,
    ForeignKeyInfo, IndexInfo, SchemaInfo, TableInfo,
};
use anyhow::Result;
use ra_core::{DataType, FactsProvider, SqlDialect, TableInfo as CoreTableInfo};
use ra_stats::types::{ColumnStats, TableStats};
use std::collections::HashMap;

pub struct NewDbAdapter {
    connection: Option<NewDbConnection>,
    statistics: HashMap<String, TableStats>,
    schema: HashMap<String, TableInfo>,
}

impl NewDbAdapter {
    pub fn new() -> Self {
        Self {
            connection: None,
            statistics: HashMap::new(),
            schema: HashMap::new(),
        }
    }
}

impl DatabaseAdapter for NewDbAdapter {
    fn connect(&mut self, connection_string: &str) -> Result<(), AdapterError> {
        // Parse connection string
        let conn = NewDbConnection::connect(connection_string)
            .map_err(|e| AdapterError::ConnectionError(e.to_string()))?;

        self.connection = Some(conn);
        Ok(())
    }

    fn gather_statistics(&self) -> Result<HashMap<String, TableStats>, AdapterError> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".into()))?;

        // Query NewDB system tables for statistics
        let query = r#"
            SELECT
                table_name,
                row_count,
                data_length,
                index_length
            FROM information_schema.table_statistics
        "#;

        let rows = conn.query(query)
            .map_err(|e| AdapterError::QueryError(e.to_string()))?;

        let mut stats = HashMap::new();
        for row in rows {
            let table_name: String = row.get(0);
            let row_count: i64 = row.get(1);
            let data_length: i64 = row.get(2);
            let index_length: i64 = row.get(3);

            stats.insert(table_name, TableStats {
                row_count: row_count as usize,
                page_count: (data_length / 8192) as usize,
                data_size_bytes: data_length as usize,
                index_size_bytes: index_length as usize,
            });
        }

        Ok(stats)
    }

    fn gather_column_stats(
        &self,
        table: &str,
    ) -> Result<HashMap<String, ColumnStats>, AdapterError> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".into()))?;

        // Query column statistics
        let query = r#"
            SELECT
                column_name,
                distinct_count,
                null_count,
                avg_length
            FROM information_schema.column_statistics
            WHERE table_name = ?
        "#;

        let rows = conn.query(query, &[table])
            .map_err(|e| AdapterError::QueryError(e.to_string()))?;

        let mut stats = HashMap::new();
        for row in rows {
            let column_name: String = row.get(0);
            let distinct_count: i64 = row.get(1);
            let null_count: i64 = row.get(2);
            let avg_length: f64 = row.get(3);

            stats.insert(column_name, ColumnStats {
                distinct_count: distinct_count as usize,
                null_fraction: null_count as f64 / distinct_count as f64,
                avg_width: avg_length as usize,
                histogram: None,  // NewDB doesn't provide histograms
                most_common_vals: vec![],
                most_common_freqs: vec![],
            });
        }

        Ok(stats)
    }

    fn get_schema_info(&self) -> Result<SchemaInfo, AdapterError> {
        // Query table and column definitions
        // Query constraints and indexes
        // Return SchemaInfo
        todo!("Implement schema introspection")
    }

    fn get_capabilities(&self) -> Result<DatabaseCapabilities, AdapterError> {
        Ok(DatabaseCapabilities {
            database_name: "newdb".to_string(),
            dialect: SqlDialect::NewDb,  // Add to ra-core enum
            features: HashMap::from([
                ("window_functions".to_string(), true),
                ("recursive_cte".to_string(), false),
                ("lateral_joins".to_string(), false),
            ]),
            index_types: vec!["btree".to_string(), "hash".to_string()],
            max_identifier_length: 64,
        })
    }

    fn supports_feature(&self, feature: &str) -> Result<bool, AdapterError> {
        let caps = self.get_capabilities()?;
        Ok(caps.supports(feature))
    }

    fn sql_dialect(&self) -> SqlDialect {
        SqlDialect::NewDb  // Add to ra-core enum
    }

    fn database_name(&self) -> &str {
        "newdb"
    }

    fn as_facts_provider(&self) -> &dyn FactsProvider {
        self
    }
}

impl FactsProvider for NewDbAdapter {
    fn table_stats(&self, table: &str) -> Option<CoreTableStats> {
        self.statistics.get(table).map(|stats| CoreTableStats {
            row_count: stats.row_count,
            page_count: stats.page_count,
            data_size_bytes: stats.data_size_bytes,
            index_size_bytes: stats.index_size_bytes,
        })
    }

    fn column_stats(&self, table: &str, column: &str) -> Option<ColumnStats> {
        // Implement column stats lookup
        None
    }

    // ... implement other FactsProvider methods
}
```

### Step 4: Register Adapter

Add the adapter to `crates/ra-adapters/src/lib.rs`:

```rust
pub mod newdb;
pub use newdb::NewDbAdapter;
```

### Step 5: Add Tests

Create `crates/ra-adapters/tests/newdb_test.rs`:

```rust
use ra_adapters::{DatabaseAdapter, NewDbAdapter};

#[test]
fn test_newdb_connection() {
    let mut adapter = NewDbAdapter::new();
    let result = adapter.connect("newdb://localhost/testdb");
    assert!(result.is_ok(), "Failed to connect: {:?}", result.err());
}

#[test]
fn test_newdb_statistics() {
    let mut adapter = NewDbAdapter::new();
    adapter.connect("newdb://localhost/testdb").unwrap();

    let stats = adapter.gather_statistics().unwrap();
    assert!(!stats.is_empty(), "No statistics gathered");

    let users_stats = stats.get("users");
    assert!(users_stats.is_some(), "Missing users table stats");
}

#[test]
fn test_newdb_schema() {
    let mut adapter = NewDbAdapter::new();
    adapter.connect("newdb://localhost/testdb").unwrap();

    let schema = adapter.get_schema_info().unwrap();
    assert!(!schema.tables.is_empty(), "No tables found");
}
```

### Step 6: Add to Web API

Update `crates/ra-web/src/api/explain.rs` to support the new engine:

```rust
fn get_adapter_for_engine(engine: &str) -> Result<Box<dyn DatabaseAdapter>> {
    match engine {
        "postgresql" | "postgresql-16" | "postgresql-17" => {
            Ok(Box::new(PostgresAdapter::new()))
        }
        "mysql" | "mysql-8.0" | "mysql-8.4" => {
            Ok(Box::new(MySQLAdapter::new()))
        }
        "newdb" | "newdb-1.0" => {
            Ok(Box::new(NewDbAdapter::new()))  // Add this
        }
        _ => Err(anyhow!("Unsupported engine: {}", engine)),
    }
}
```

### Step 7: Add to Frontend

Update `crates/ra-web/frontend/src/constants.ts`:

```typescript
export const ENGINES = [
  { id: 'postgresql-17', name: 'PostgreSQL 17', vendor: 'postgresql' },
  { id: 'mysql-8.4', name: 'MySQL 8.4', vendor: 'mysql' },
  { id: 'newdb-1.0', name: 'NewDB 1.0', vendor: 'newdb' },  // Add this
  // ... other engines
] as const;
```

Update plan parser in `crates/ra-web/frontend/src/utils/planParser.ts`:

```typescript
export function parsePlan(text: string, engine: string): PlanNode {
  if (engine.startsWith('postgresql')) {
    return parsePostgresqlPlan(text);
  } else if (engine.startsWith('mysql')) {
    return parseMysqlPlan(text);
  } else if (engine.startsWith('newdb')) {
    return parseNewDbPlan(text);  // Add this
  } else {
    return parseGenericPlan(text);
  }
}

function parseNewDbPlan(text: string): PlanNode {
  // Parse NewDB's EXPLAIN output format
  // Return unified PlanNode structure
  // ...
}
```

## Testing Parsers

### Unit Tests

Test individual grammar rules:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_select() {
        let profile = ParserProfile::load("newdb-1.0").unwrap();
        let mut parser = Parser::new(profile);

        let sql = "SELECT id, name FROM users";
        let expr = parser.parse(sql).unwrap();

        match expr {
            RelExpr::Project { input, columns } => {
                assert_eq!(columns.len(), 2);
                // ... more assertions
            }
            _ => panic!("Expected Project node"),
        }
    }

    #[test]
    fn test_parse_custom_operator() {
        let profile = ParserProfile::load("newdb-1.0").unwrap();
        let mut parser = Parser::new(profile);

        let sql = "SELECT * FROM users WHERE name <=> 'John'";
        let expr = parser.parse(sql).unwrap();

        // Verify custom operator was parsed correctly
    }
}
```

### Integration Tests

Test against actual database:

```rust
#[test]
#[ignore]  // Requires NewDB installation
fn test_newdb_roundtrip() {
    let mut adapter = NewDbAdapter::new();
    adapter.connect("newdb://localhost/testdb").unwrap();

    let sql = "SELECT * FROM users WHERE age > 25";

    // Parse SQL
    let profile = ParserProfile::load("newdb-1.0").unwrap();
    let mut parser = Parser::new(profile);
    let expr = parser.parse(sql).unwrap();

    // Optimize
    let optimizer = Optimizer::new();
    let optimized = optimizer.optimize(&expr).unwrap();

    // Execute on NewDB
    let result = adapter.execute(&optimized).unwrap();
    assert!(!result.is_empty());
}
```

### Regression Tests

Add SQL corpus for the new engine:

```
crates/ra-regression/sql/newdb/
├── basic_select.sql
├── joins.sql
├── aggregates.sql
├── window_functions.sql
└── custom_syntax.sql
```

## Parser Specification

### RelExpr Conversion Rules

The parser converts SQL AST to RelExpr following these rules:

**SELECT clause:**
- `SELECT a, b` → `Project { columns: [Column("a"), Column("b")] }`
- `SELECT *` → `Project { columns: [Wildcard] }`
- `SELECT DISTINCT` → `Distinct { input: Project { ... } }`

**FROM clause:**
- `FROM users` → `Scan { table: "users" }`
- `FROM users u` → `Scan { table: "users", alias: Some("u") }`
- `FROM users, orders` → `Join { kind: Cross, ... }`

**WHERE clause:**
- `WHERE age > 25` → `Filter { predicate: BinaryOp(Gt, Column("age"), Literal(25)) }`

**JOIN clause:**
- `INNER JOIN` → `Join { kind: Inner, ... }`
- `LEFT JOIN` → `Join { kind: LeftOuter, ... }`
- `CROSS JOIN` → `Join { kind: Cross, ... }`

**GROUP BY clause:**
- `GROUP BY dept` → `Aggregate { group_by: [Column("dept")], ... }`

**ORDER BY clause:**
- `ORDER BY name` → `Sort { order_by: [OrderBy { expr: Column("name"), asc: true }] }`

**LIMIT clause:**
- `LIMIT 10` → `Limit { limit: 10, offset: None }`
- `LIMIT 10 OFFSET 5` → `Limit { limit: 10, offset: Some(5) }`

### Expression Types

**Literals:**
- `42` → `Literal(Value::Int(42))`
- `'hello'` → `Literal(Value::String("hello"))`
- `TRUE` → `Literal(Value::Bool(true))`

**Binary operators:**
- `a + b` → `BinaryOp(Add, Column("a"), Column("b"))`
- `a AND b` → `BinaryOp(And, a, b)`
- `a > 10` → `BinaryOp(Gt, Column("a"), Literal(10))`

**Functions:**
- `COUNT(*)` → `AggFunc { name: "COUNT", args: [Wildcard] }`
- `UPPER(name)` → `Function { name: "UPPER", args: [Column("name")] }`

## Example: Adding PostgreSQL Extension

Here's a complete example of adding pgvector extension support.

**1. Create extension profile:**

```toml
# profiles/extensions/pgvector.toml
[profile]
name = "pgvector"
extends = "postgresql"
description = "Vector similarity search extension"

[features]
vector_similarity = true
cosine_distance = true
l2_distance = true
inner_product = true

[operators]
vector_ops = ["<->", "<#>", "<=>"]

[functions]
vector_funcs = ["cosine_distance", "l2_distance", "inner_product"]

[data_types]
custom_types = ["vector"]
```

**2. Test profile loading:**

```rust
#[test]
fn test_pgvector_profile() {
    let profile = ParserProfile::load("postgresql-17+pgvector").unwrap();

    // Verify vector operators are available
    assert!(profile.operators.contains(&"<->".to_string()));
    assert!(profile.operators.contains(&"<#>".to_string()));

    // Verify vector functions are available
    assert!(profile.functions.contains(&"cosine_distance".to_string()));
}
```

**3. Parse vector queries:**

```rust
#[test]
fn test_parse_vector_query() {
    let profile = ParserProfile::load("postgresql-17+pgvector").unwrap();
    let mut parser = Parser::new(profile);

    let sql = r#"
        SELECT id, embedding <-> '[1,2,3]'::vector AS distance
        FROM documents
        ORDER BY embedding <-> '[1,2,3]'::vector
        LIMIT 10
    "#;

    let expr = parser.parse(sql).unwrap();
    // Verify vector distance operator was parsed
}
```

## Troubleshooting

### Common Issues

**Issue: Profile not found**

```
Error: Profile 'newdb-1.0' not found
```

Solution: Ensure the profile file exists in `profiles/vendors/` and has the correct name.

**Issue: Unsupported syntax**

```
Error: Unsupported syntax: MATCH AGAINST
```

Solution: Add grammar extension in `crates/ra-parser/src/grammar/vendors/newdb.rs`.

**Issue: Missing operators**

```
Error: Unknown operator '<~>'
```

Solution: Add operator to profile's `[operators]` section.

**Issue: Connection failure**

```
Error: Failed to connect to database
```

Solution: Verify connection string format and database availability.

## Best Practices

1. **Start with standard SQL:** Base profiles on SQL-92/SQL-2016 standards
2. **Test incrementally:** Add one feature at a time and test
3. **Document extensions:** Add comments explaining custom syntax
4. **Reuse adapters:** Inherit from similar databases when possible
5. **Validate inputs:** Always validate connection strings and queries
6. **Handle errors:** Provide clear, actionable error messages

## Further Reading

- [architecture.md](architecture.md) - System architecture overview
- [contributing.md](contributing.md) - Development guidelines
- [sqlparser documentation](https://docs.rs/sqlparser/) - SQL parser library
- [egg documentation](https://docs.rs/egg/) - E-graph library
