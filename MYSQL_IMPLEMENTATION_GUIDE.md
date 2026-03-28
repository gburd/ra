# MySQL/MariaDB Feature Implementation Guide

**Purpose**: Technical guide for implementing MySQL/MariaDB-specific features in Ra optimizer
**Audience**: Ra contributors and developers
**Last Updated**: 2026-03-28

---

## Table of Contents

1. [Implementation Pattern](#implementation-pattern)
2. [Code Architecture](#code-architecture)
3. [Step-by-Step: Adding a New Feature](#step-by-step-adding-a-new-feature)
4. [Feature-Specific Guides](#feature-specific-guides)
5. [Testing Strategy](#testing-strategy)
6. [Performance Validation](#performance-validation)

---

## Implementation Pattern

### Standard Feature Implementation Flow

```
1. Research → 2. Parser → 3. Core Types → 4. Metadata → 5. Rules → 6. Cost Model → 7. Tests
```

**Estimated Time per Feature**:
- Simple (CHECK constraints): 1-2 weeks
- Medium (Spatial, Hints): 2-4 weeks
- Complex (JSON, Full-Text): 6-8 weeks

### Checklist for Each Feature

- [ ] Research MySQL/MariaDB documentation and source code
- [ ] Extend SQL parser (`ra-parser`)
- [ ] Add core types to `ra-core` (Expr, RelExpr variants)
- [ ] Update metadata layer (`ra-metadata`)
- [ ] Create optimization rules (`.rra` files)
- [ ] Implement cost model
- [ ] Write unit tests (parser, rules, cost model)
- [ ] Write integration tests (end-to-end SQL queries)
- [ ] Document in `.rra` files and this guide
- [ ] Benchmark against MySQL optimizer

---

## Code Architecture

### Directory Structure

```
ra/
├── crates/
│   ├── ra-parser/          # SQL parsing
│   │   └── src/
│   │       ├── sql_to_relexpr.rs    # Main SQL→RelExpr converter
│   │       └── mysql_extensions.rs   # (NEW) MySQL-specific parsing
│   ├── ra-core/            # Core algebra types
│   │   └── src/
│   │       ├── expr.rs     # Expression types (WHERE, SELECT clauses)
│   │       ├── algebra.rs  # Relational algebra (Scan, Join, etc.)
│   │       └── mysql/      # (NEW) MySQL-specific types
│   ├── ra-metadata/        # Database catalog access
│   │   └── src/
│   │       └── mysql.rs    # MySQL connector and metadata
│   ├── ra-dialect/         # SQL dialect features
│   │   └── src/
│   │       └── dialect.rs  # Feature support matrix
│   ├── ra-engine/          # Optimization engine
│   │   └── src/
│   │       ├── analysis.rs         # Cost analysis
│   │       └── mysql_cost_models.rs # (NEW) MySQL-specific costs
│   └── ra-compiler/        # Rule compilation
│       └── src/
│           └── rule_loader.rs
├── rules/                  # Optimization rules
│   └── database-specific/
│       └── mysql/          # MySQL-specific rules
│           ├── json-path-index.rra         # (NEW)
│           ├── fulltext-index-selection.rra # (NEW)
│           └── ...
└── tests/
    ├── mysql/              # MySQL-specific tests
    │   ├── json_tests.rs   # (NEW)
    │   └── fulltext_tests.rs # (NEW)
    └── integration/
        └── mysql_integration.rs
```

### Key Types and Traits

#### `ra-core/src/expr.rs`

```rust
/// Expression AST for WHERE clauses, SELECT lists, etc.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expr {
    // Existing variants
    Column(ColumnRef),
    Const(Const),
    BinaryOp { op: BinOp, left: Box<Expr>, right: Box<Expr> },
    UnaryOp { op: UnaryOp, expr: Box<Expr> },
    Function { name: String, args: Vec<Expr> },

    // NEW: MySQL-specific variants
    /// Full-text search: MATCH(cols) AGAINST(text [mode])
    FullTextMatch {
        columns: Vec<ColumnRef>,
        query: String,
        mode: FullTextMode,
    },

    /// JSON path extraction: col->'$.path' or JSON_EXTRACT(col, path)
    JsonPath {
        expr: Box<Expr>,
        path: String,
    },

    /// JSON functions (JSON_SET, JSON_ARRAY, etc.)
    JsonFunction {
        function: JsonFunctionType,
        args: Vec<Expr>,
    },

    /// Spatial predicate (ST_Contains, ST_Intersects, etc.)
    SpatialPredicate {
        function: SpatialFunction,
        args: Vec<Expr>,
    },
}

/// Full-text search modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FullTextMode {
    NaturalLanguage,
    Boolean,
    QueryExpansion,
}

/// JSON function types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JsonFunctionType {
    Extract,
    Set,
    Insert,
    Replace,
    Remove,
    Array,
    Object,
    Table,  // JSON_TABLE
    // ... more variants
}
```

#### `ra-metadata/src/mysql.rs`

```rust
/// Extend TableInfo with MySQL-specific metadata
pub struct TableInfo {
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    pub constraints: Vec<ConstraintInfo>,
    pub indexes: Vec<IndexInfo>,

    // NEW: MySQL-specific fields
    pub storage_engine: StorageEngine,
    pub fulltext_indexes: Vec<FullTextIndexInfo>,
    pub spatial_indexes: Vec<SpatialIndexInfo>,
    pub generated_columns: Vec<GeneratedColumnInfo>,
}

/// Storage engine type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageEngine {
    InnoDB,
    MyISAM,
    Aria,
    Memory,
    CSV,
    Archive,
}

/// Full-text index metadata
#[derive(Debug, Clone)]
pub struct FullTextIndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub parser: FullTextParser,
    pub min_word_length: u32,
}

/// Generated column metadata
#[derive(Debug, Clone)]
pub struct GeneratedColumnInfo {
    pub name: String,
    pub expression: String,
    pub is_stored: bool,  // STORED vs VIRTUAL
    pub indexed: bool,
}
```

---

## Step-by-Step: Adding a New Feature

### Example: Implementing Full-Text Search Support

This walkthrough demonstrates the complete implementation process.

#### Step 1: Research (1-2 days)

**Objectives**:
- Understand MySQL's full-text search implementation
- Identify syntax, modes, and edge cases
- Study MySQL optimizer's FTS index selection logic

**Resources**:
- MySQL Reference Manual: Full-Text Search Functions
- MySQL source: `sql/sql_select.cc`, `storage/innobase/handler/ha_innodb.cc`
- Test MySQL behavior with sample queries

**Example Research**:
```sql
-- Test MATCH...AGAINST syntax
CREATE TABLE articles (
  id INT PRIMARY KEY,
  title VARCHAR(200),
  body TEXT,
  FULLTEXT KEY ft_idx (title, body)
) ENGINE=InnoDB;

INSERT INTO articles VALUES
  (1, 'MySQL Tutorial', 'This tutorial explains MySQL full-text search'),
  (2, 'PostgreSQL Guide', 'Learn PostgreSQL advanced features');

-- Natural language mode (default)
SELECT id, MATCH(title, body) AGAINST('MySQL') AS score
FROM articles
WHERE MATCH(title, body) AGAINST('MySQL');
-- Returns: (1, 0.785)

-- Boolean mode
SELECT * FROM articles
WHERE MATCH(title, body) AGAINST('+MySQL -PostgreSQL' IN BOOLEAN MODE);
-- Returns: (1)

-- Query expansion
SELECT * FROM articles
WHERE MATCH(title, body) AGAINST('database' WITH QUERY EXPANSION);
-- Returns both rows (expands to related terms)

-- EXPLAIN to see index usage
EXPLAIN SELECT * FROM articles
WHERE MATCH(title, body) AGAINST('MySQL');
-- Shows: type=fulltext, key=ft_idx
```

**Key Findings**:
1. `MATCH` column list must exactly match a FULLTEXT index
2. Boolean mode supports operators: `+` (must have), `-` (must not have), `*` (wildcard)
3. Relevance score can be used in SELECT and ORDER BY
4. MySQL optimizer chooses fulltext index automatically when available

#### Step 2: Extend SQL Parser (3-5 days)

**File**: `crates/ra-parser/src/sql_to_relexpr.rs`

```rust
// Add to convert_expr() function
fn convert_expr(sql_expr: &SqlExpr) -> Result<Expr, SqlConversionError> {
    match sql_expr {
        // ... existing cases

        // NEW: Handle MATCH...AGAINST
        SqlExpr::Function(func) if func.name.0[0].value.eq_ignore_ascii_case("match") => {
            convert_fulltext_match(func)
        }

        _ => {
            // ... rest of cases
        }
    }
}

fn convert_fulltext_match(func: &Function) -> Result<Expr, SqlConversionError> {
    // MATCH(col1, col2, ...) AGAINST('query' [IN mode])

    // Extract column list from MATCH()
    let columns: Vec<ColumnRef> = func.args.iter()
        .map(|arg| match arg {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(SqlExpr::Identifier(ident))) => {
                Ok(ColumnRef {
                    table: None,
                    column: ident.value.clone(),
                })
            }
            _ => Err(SqlConversionError::InvalidSql(
                "MATCH() requires column names".to_string()
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Extract AGAINST clause
    // Look for AGAINST in parent expression (this is simplified; real impl needs context)
    // For now, assume AGAINST is handled by sqlparser as part of the function

    let query = extract_against_query(func)?;
    let mode = extract_fulltext_mode(func)?;

    Ok(Expr::FullTextMatch {
        columns,
        query,
        mode,
    })
}

fn extract_fulltext_mode(func: &Function) -> Result<FullTextMode, SqlConversionError> {
    // Parse IN BOOLEAN MODE, IN NATURAL LANGUAGE MODE, WITH QUERY EXPANSION
    // This requires extending sqlparser or manual parsing

    // Placeholder implementation
    Ok(FullTextMode::NaturalLanguage)
}
```

**Challenge**: `sqlparser` crate may not parse `MATCH...AGAINST` syntax. Two options:

1. **Extend sqlparser fork**: Add MySQL-specific syntax support
2. **Pre-process SQL**: Transform `MATCH...AGAINST` into a function call Ra understands

**Recommended Approach**: Extend sqlparser and contribute back upstream.

#### Step 3: Add Core Types (1-2 days)

**File**: `crates/ra-core/src/expr.rs`

Add the `FullTextMatch` variant to `Expr` enum (see Code Architecture section above).

**File**: `crates/ra-core/src/mysql/fulltext.rs` (NEW)

```rust
/// Full-text search mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FullTextMode {
    /// Default natural language search
    NaturalLanguage,
    /// Boolean search with operators (+, -, *, etc.)
    Boolean,
    /// Query expansion (find related terms)
    QueryExpansion,
}

impl fmt::Display for FullTextMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NaturalLanguage => write!(f, "NATURAL LANGUAGE"),
            Self::Boolean => write!(f, "BOOLEAN"),
            Self::QueryExpansion => write!(f, "QUERY EXPANSION"),
        }
    }
}

/// Full-text index metadata
#[derive(Debug, Clone)]
pub struct FullTextIndex {
    pub name: String,
    pub table: String,
    pub columns: Vec<String>,
    pub parser: FullTextParser,
}

/// Full-text parser type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullTextParser {
    /// Default built-in parser
    Builtin,
    /// NGRAM parser for CJK languages
    Ngram,
    /// MeCab parser for Japanese (MySQL 8.0 only)
    Mecab,
}
```

**Add to `ra-core/src/lib.rs`**:
```rust
pub mod mysql {
    pub mod fulltext;
}
```

#### Step 4: Update Metadata Layer (2-3 days)

**File**: `crates/ra-metadata/src/mysql.rs`

```rust
impl MySqlConnector {
    /// Query full-text indexes for a table
    fn query_fulltext_indexes(&mut self, table: &str) -> MetadataResult<Vec<FullTextIndexInfo>> {
        let rows: Vec<(String, String, String)> = self
            .conn
            .exec(
                "SELECT INDEX_NAME, COLUMN_NAME,
                        COALESCE(
                          (SELECT OPTION_VALUE
                           FROM information_schema.INNODB_FT_CONFIG
                           WHERE OPTION_NAME = 'parser'),
                          'builtin'
                        ) AS parser
                 FROM information_schema.STATISTICS
                 WHERE TABLE_SCHEMA = ?
                   AND TABLE_NAME = ?
                   AND INDEX_TYPE = 'FULLTEXT'
                 ORDER BY INDEX_NAME, SEQ_IN_INDEX",
                (&self.database, table),
            )
            .map_err(|e| MetadataError::Query {
                message: format!("failed to query fulltext indexes for {table}: {e}"),
            })?;

        // Group columns by index name
        let mut index_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut parser_map: HashMap<String, String> = HashMap::new();

        for (idx_name, col_name, parser) in rows {
            index_map.entry(idx_name.clone())
                .or_default()
                .push(col_name);
            parser_map.insert(idx_name.clone(), parser);
        }

        let mut indexes = Vec::new();
        for (name, columns) in index_map {
            let parser = match parser_map.get(&name).map(String::as_str) {
                Some("ngram") => FullTextParser::Ngram,
                Some("mecab") => FullTextParser::Mecab,
                _ => FullTextParser::Builtin,
            };

            indexes.push(FullTextIndexInfo {
                name,
                columns,
                parser,
                min_word_length: 4,  // Default; can query @@ft_min_word_length
            });
        }

        Ok(indexes)
    }

    /// Update gather_schema_mut to include fulltext indexes
    pub fn gather_schema_mut(&mut self) -> MetadataResult<SchemaInfo> {
        // ... existing code ...

        for name in &table_names {
            let columns = self.query_columns(name)?;
            let constraints = self.query_constraints(name)?;
            let indexes = self.query_indexes(name)?;
            let triggers = self.query_triggers(name)?;
            let fulltext_indexes = self.query_fulltext_indexes(name)?;  // NEW
            let (row_count, _) = self.query_table_row_count(name)?;

            tables.insert(
                name.clone(),
                TableInfo {
                    name: name.clone(),
                    columns,
                    constraints,
                    indexes,
                    triggers,
                    fulltext_indexes,  // NEW
                    estimated_rows: Some(row_count),
                },
            );
        }

        // ... rest of code ...
    }
}
```

#### Step 5: Create Optimization Rules (3-5 days)

**File**: `rules/database-specific/mysql/fulltext-index-selection.rra`

```markdown
---
id: mysql-fulltext-index-selection
name: MySQL Full-Text Index Selection
category: database-specific/mysql
databases: [mysql]
version: "1.0.0"
authors: ["RA Contributors"]
tags: [database-specific, mysql, full-text, index, match-against]
complexity: O(log n)
benefit_range: [0.5, 0.99]
---

# MySQL Full-Text Index Selection

## Description

Replace table scan with full-text index scan when a WHERE clause contains
`MATCH...AGAINST` and a FULLTEXT index exists on the exact column list.

**When to apply**: Query has `MATCH(col1, col2, ...) AGAINST('query')` and
a FULLTEXT index exists on columns (col1, col2, ...) in the same order.

**Why it works**: FULLTEXT indexes are inverted indexes optimized for text
search. Scanning the index is O(k) where k is the result set size, vs O(n)
for table scan where n is total rows. For text-heavy tables with selective
queries, this reduces cost by 50-99%.

**Database version**: MySQL 5.6+ (InnoDB), MySQL 3.23+ (MyISAM)

## Relational Algebra

```latex
-- Before: table scan with MATCH predicate
$$
\sigma_{MATCH(title, body) AGAINST('mysql')}(scan(articles))
$$

-- After: full-text index scan
$$
fulltext\_index\_scan(articles.ft\_idx, 'mysql')
$$
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("mysql-fulltext-index-selection";
    "(filter (match-against ?cols ?query ?mode) (scan ?table))" =>
    "(fulltext-index-scan ?table ?index ?query ?mode)"
    if is_database("mysql")
    if has_fulltext_index("?table", "?cols", "?index")
),
```

## Preconditions

```rust
fn applicable(
    table: &Table,
    columns: &[ColumnRef],
) -> bool {
    // Exact column match required
    table.fulltext_indexes.iter().any(|idx| {
        idx.columns.len() == columns.len()
        && idx.columns.iter().zip(columns).all(|(a, b)| a == b.column)
    })
}
```

**Restrictions:**
- Column list in MATCH() must exactly match a FULLTEXT index
- Cannot use multiple FULLTEXT indexes in one query (MySQL limitation)
- InnoDB full-text indexes available in MySQL 5.6+

## Cost Model

```rust
fn estimated_benefit(
    table_rows: f64,
    fulltext_selectivity: f64,  // Estimated result set size ratio
) -> f64 {
    let scan_cost = table_rows * SEQUENTIAL_SCAN_COST_PER_ROW;
    let result_rows = table_rows * fulltext_selectivity;
    let index_cost = result_rows.log2() * FULLTEXT_INDEX_LOOKUP_COST
                   + result_rows * ROW_FETCH_COST;

    scan_cost - index_cost
}
```

**Selectivity estimation**:
- Default: 0.01 (1% of rows)
- Query length-based: longer queries are more selective
- Boolean mode: estimate based on operators (+required terms are more selective)

**Typical benefit**: 50-99% cost reduction for text-heavy tables

## Test Cases

```sql
-- Positive: MATCH with FULLTEXT index
CREATE TABLE articles (
  id INT PRIMARY KEY,
  title VARCHAR(200),
  body TEXT,
  FULLTEXT KEY ft_idx (title, body)
) ENGINE=InnoDB;

SELECT * FROM articles
WHERE MATCH(title, body) AGAINST('mysql database');
-- Should use fulltext_index_scan(ft_idx)
```

```sql
-- Positive: Boolean mode
SELECT * FROM articles
WHERE MATCH(title, body) AGAINST('+mysql -postgresql' IN BOOLEAN MODE);
-- Should use fulltext_index_scan(ft_idx) with boolean mode
```

```sql
-- Negative: Column order mismatch
SELECT * FROM articles
WHERE MATCH(body, title) AGAINST('mysql');
-- FULLTEXT index is on (title, body), not (body, title)
-- Cannot use index, must scan
```

```sql
-- Negative: No FULLTEXT index
CREATE TABLE posts (
  id INT,
  content TEXT,
  INDEX idx_content (content(100))  -- Regular index, not FULLTEXT
);

SELECT * FROM posts WHERE MATCH(content) AGAINST('query');
-- ERROR: Can't find FULLTEXT index matching column list
```

## References

MySQL: "Full-Text Search Functions" in MySQL 8.0 Reference Manual
Source: `sql/sql_select.cc`, `ft_boolean_search()` in storage engines
Paper: "MySQL Full-Text Search: From Concepts to Practice"
```

**File**: `rules/database-specific/mysql/fulltext-boolean-operators.rra`

```markdown
---
id: mysql-fulltext-boolean-operators
name: MySQL Full-Text Boolean Operator Optimization
category: database-specific/mysql
databases: [mysql]
version: "1.0.0"
authors: ["RA Contributors"]
tags: [database-specific, mysql, full-text, boolean-mode]
complexity: O(k1 + k2)
benefit_range: [0.2, 0.7]
---

# MySQL Full-Text Boolean Operator Optimization

## Description

Optimize boolean full-text queries by processing required terms (+) first
and using them to filter candidates before evaluating optional terms.

**When to apply**: MATCH...AGAINST query in BOOLEAN MODE with multiple terms
including required (+) and excluded (-) operators.

**Why it works**: Required terms have smaller posting lists. Process them
first to reduce the candidate set before checking optional terms or excluded
terms.

## Relational Algebra

```latex
-- Before: Single boolean query
$$
fulltext\_scan(articles.ft\_idx, '+mysql +innodb -myisam')
$$

-- After: Intersection of required terms first
$$
\sigma_{NOT\ contains('myisam')}(
  intersect(
    fulltext\_scan(ft\_idx, 'mysql'),
    fulltext\_scan(ft\_idx, 'innodb')
  )
)
$$
```

## Implementation

```rust
rw!("mysql-fulltext-boolean-decompose";
    "(fulltext-scan ?idx (boolean-query ?required ?optional ?excluded))" =>
    "(filter (not-contains ?excluded)
       (intersect-ftscans ?idx ?required))"
    if count_terms("?required") >= 2
),
```

## Preconditions

Boolean query has multiple required (+) terms and/or excluded (-) terms.

## Cost Model

Intersection of small posting lists is faster than evaluating full boolean
expression on large candidate set.

**Benefit**: 20-70% depending on term selectivity.

## Test Cases

```sql
SELECT * FROM articles
WHERE MATCH(title, body) AGAINST('+mysql +innodb -myisam' IN BOOLEAN MODE);
-- Optimize to: intersect(mysql_results, innodb_results) EXCEPT myisam_results
```

## References

MySQL Source: `ft_boolean_search()` in storage/myisam/ft_boolean_search.c
```

#### Step 6: Implement Cost Model (2-3 days)

**File**: `crates/ra-engine/src/mysql_cost_models.rs` (NEW)

```rust
/// Cost model for MySQL full-text index scan
pub fn fulltext_index_scan_cost(
    table_stats: &TableStats,
    query: &str,
    mode: FullTextMode,
) -> f64 {
    let total_rows = table_stats.row_count;

    // Estimate selectivity based on query characteristics
    let selectivity = estimate_fulltext_selectivity(query, mode);
    let result_rows = total_rows * selectivity;

    // Cost components:
    // 1. Index lookup (logarithmic in total rows)
    // 2. Posting list traversal (linear in matching documents)
    // 3. Row fetch (linear in result set)

    const INDEX_LOOKUP_COST: f64 = 0.001;
    const POSTING_LIST_COST_PER_DOC: f64 = 0.0001;
    const ROW_FETCH_COST: f64 = 0.01;

    let index_cost = total_rows.log2() * INDEX_LOOKUP_COST;
    let posting_cost = result_rows * POSTING_LIST_COST_PER_DOC;
    let fetch_cost = result_rows * ROW_FETCH_COST;

    index_cost + posting_cost + fetch_cost
}

/// Estimate selectivity of full-text query
fn estimate_fulltext_selectivity(query: &str, mode: FullTextMode) -> f64 {
    match mode {
        FullTextMode::NaturalLanguage => {
            // Longer queries are more selective
            let term_count = query.split_whitespace().count() as f64;
            (0.1 / term_count).min(0.5).max(0.001)
        }
        FullTextMode::Boolean => {
            // Count required (+) terms
            let required_terms = query.split_whitespace()
                .filter(|t| t.starts_with('+'))
                .count() as f64;

            if required_terms > 0 {
                (0.1 / required_terms.powf(1.5)).min(0.5).max(0.001)
            } else {
                0.1  // Default for optional terms
            }
        }
        FullTextMode::QueryExpansion => {
            // Query expansion returns more results
            0.2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fulltext_selectivity() {
        // Single term: ~10%
        assert!((estimate_fulltext_selectivity("mysql", FullTextMode::NaturalLanguage) - 0.1).abs() < 0.01);

        // Multiple terms: more selective
        assert!(estimate_fulltext_selectivity("mysql innodb storage", FullTextMode::NaturalLanguage) < 0.05);

        // Boolean required terms: very selective
        assert!(estimate_fulltext_selectivity("+mysql +innodb", FullTextMode::Boolean) < 0.05);
    }

    #[test]
    fn test_fulltext_cost() {
        let stats = TableStats {
            table_name: "articles".to_string(),
            row_count: 1_000_000.0,
            total_bytes: 1_000_000_000,
            columns: HashMap::new(),
        };

        let cost = fulltext_index_scan_cost(&stats, "mysql", FullTextMode::NaturalLanguage);

        // Cost should be much less than full table scan
        let scan_cost = stats.row_count * 0.01;  // SEQUENTIAL_SCAN_COST_PER_ROW
        assert!(cost < scan_cost * 0.2);  // At least 80% savings
    }
}
```

#### Step 7: Write Tests (3-5 days)

**File**: `tests/mysql/fulltext_tests.rs` (NEW)

```rust
use ra_parser::sql_to_relexpr;
use ra_engine::optimize;
use ra_metadata::mysql::MySqlConnector;

#[test]
fn test_fulltext_parser() {
    let sql = "SELECT * FROM articles WHERE MATCH(title, body) AGAINST('mysql')";
    let relexpr = sql_to_relexpr(sql).unwrap();

    // Assert that parser creates FullTextMatch expression
    // (implementation details depend on RelExpr structure)
    assert!(contains_fulltext_match(&relexpr));
}

#[test]
fn test_fulltext_index_selection() {
    // Setup: Create in-memory table with FULLTEXT index
    let schema = create_test_schema_with_fulltext();

    let sql = "SELECT * FROM articles WHERE MATCH(title, body) AGAINST('mysql')";
    let relexpr = sql_to_relexpr(sql).unwrap();
    let optimized = optimize(relexpr, &schema).unwrap();

    // Assert that optimizer selected full-text index scan
    assert!(uses_fulltext_index(&optimized, "ft_idx"));
}

#[test]
fn test_fulltext_no_index_error() {
    let schema = create_test_schema_without_fulltext();

    let sql = "SELECT * FROM articles WHERE MATCH(title, body) AGAINST('mysql')";
    let relexpr = sql_to_relexpr(sql).unwrap();
    let result = optimize(relexpr, &schema);

    // Should fail or fall back to table scan with warning
    assert!(result.is_err() || has_warning(&result, "no fulltext index"));
}

#[test]
fn test_fulltext_boolean_mode() {
    let sql = "SELECT * FROM articles
               WHERE MATCH(title, body) AGAINST('+mysql -postgresql' IN BOOLEAN MODE)";
    let relexpr = sql_to_relexpr(sql).unwrap();

    // Assert boolean mode is parsed correctly
    assert_eq!(get_fulltext_mode(&relexpr), FullTextMode::Boolean);
}

#[test]
fn test_fulltext_cost_model() {
    let stats = TableStats {
        table_name: "articles".to_string(),
        row_count: 1_000_000.0,
        total_bytes: 1_000_000_000,
        columns: HashMap::new(),
    };

    let ft_cost = fulltext_index_scan_cost(&stats, "mysql", FullTextMode::NaturalLanguage);
    let scan_cost = stats.row_count * SEQUENTIAL_SCAN_COST;

    // Full-text should be much cheaper than full scan
    assert!(ft_cost < scan_cost * 0.3);
}

// Integration test with real MySQL database
#[test]
#[ignore]  // Requires MySQL server
fn test_fulltext_integration() {
    let mut conn = MySqlConnector::connect("mysql://localhost/test").unwrap();

    // Create table with data
    conn.conn.query_drop("
        CREATE TABLE articles (
            id INT PRIMARY KEY,
            title VARCHAR(200),
            body TEXT,
            FULLTEXT KEY ft_idx (title, body)
        ) ENGINE=InnoDB
    ").unwrap();

    conn.conn.query_drop("
        INSERT INTO articles VALUES
        (1, 'MySQL Tutorial', 'Learn MySQL full-text search'),
        (2, 'PostgreSQL Guide', 'PostgreSQL advanced features')
    ").unwrap();

    // Query with MATCH...AGAINST
    let sql = "SELECT * FROM articles WHERE MATCH(title, body) AGAINST('mysql')";
    let relexpr = sql_to_relexpr(sql).unwrap();
    let schema = conn.gather_schema_mut().unwrap();
    let optimized = optimize(relexpr, &schema).unwrap();

    // Verify full-text index is used
    assert!(uses_fulltext_index(&optimized, "ft_idx"));

    // Verify explain output from MySQL
    let explain = conn.explain_query_mut(sql).unwrap();
    assert_eq!(explain.access_type, "fulltext");
    assert_eq!(explain.key, Some("ft_idx".to_string()));
}
```

**File**: `tests/integration/mysql_fulltext_benchmark.rs` (NEW)

```rust
/// Benchmark full-text search optimization
///
/// Compares Ra optimizer decisions against MySQL optimizer.
#[test]
#[ignore]  // Requires MySQL server and large dataset
fn bench_fulltext_optimization() {
    let mut conn = MySqlConnector::connect("mysql://localhost/benchmark").unwrap();

    // Load 1M article dataset
    load_benchmark_data(&mut conn);

    let test_queries = vec![
        "SELECT * FROM articles WHERE MATCH(title, body) AGAINST('database')",
        "SELECT * FROM articles WHERE MATCH(title, body) AGAINST('+mysql +innodb' IN BOOLEAN MODE)",
        "SELECT * FROM articles WHERE MATCH(title, body) AGAINST('optimization' WITH QUERY EXPANSION)",
    ];

    for sql in test_queries {
        // Ra optimization
        let relexpr = sql_to_relexpr(sql).unwrap();
        let schema = conn.gather_schema_mut().unwrap();
        let ra_plan = optimize(relexpr, &schema).unwrap();
        let ra_cost = estimate_cost(&ra_plan, &schema);

        // MySQL EXPLAIN
        let mysql_explain = conn.explain_query_mut(sql).unwrap();
        let mysql_cost = mysql_explain.estimated_cost;

        // Ra should produce similar or better plan
        println!("Query: {}", sql);
        println!("Ra cost: {:.2}, MySQL cost: {:.2}", ra_cost, mysql_cost);
        assert!((ra_cost - mysql_cost).abs() / mysql_cost < 0.5,  // Within 50%
                "Ra cost differs significantly from MySQL");
    }
}
```

---

## Feature-Specific Guides

### JSON Functions Implementation

**Complexity**: High (6-8 weeks)

**Key Challenges**:
1. **JSON Path Parsing**: Implement JSONPath syntax (`$.field`, `$.array[*]`, `$[0].field`)
2. **Binary JSON Format**: MySQL uses custom binary JSON format for storage
3. **Multi-Valued Indexes**: MySQL 8.0 supports indexing JSON arrays
4. **JSON_TABLE**: Table-valued function requires special handling in query plan

**Implementation Steps**:

1. **Add JSON Types** (`ra-core/src/expr.rs`):
```rust
pub enum Expr {
    // ... existing variants

    /// JSON path: col->'$.path'
    JsonPath {
        expr: Box<Expr>,
        path: JsonPath,
        unquote: bool,  // true for ->> operator
    },

    /// JSON_TABLE(doc, path COLUMNS(...))
    JsonTable {
        expr: Box<Expr>,
        path: JsonPath,
        columns: Vec<JsonTableColumn>,
    },
}

/// JSONPath expression
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JsonPath {
    pub segments: Vec<JsonPathSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum JsonPathSegment {
    Root,                      // $
    Field(String),             // .field
    Index(usize),              // [0]
    Wildcard,                  // [*]
    Slice(usize, usize),       // [0:5]
}
```

2. **Parse JSON Operators**:
```rust
// In sql_to_relexpr.rs
fn convert_expr(sql_expr: &SqlExpr) -> Result<Expr, SqlConversionError> {
    match sql_expr {
        // Handle JSON operators: ->, ->>
        SqlExpr::JsonAccess { expr, path } => {
            Ok(Expr::JsonPath {
                expr: Box::new(convert_expr(expr)?),
                path: parse_json_path(path)?,
                unquote: false,
            })
        }

        // Handle JSON functions
        SqlExpr::Function(func) if is_json_function(&func.name) => {
            convert_json_function(func)
        }

        _ => { /* ... */ }
    }
}
```

3. **Optimization Rules**:
   - JSON path index selection (functional indexes)
   - JSON path pushdown (evaluate once vs multiple times)
   - JSON_TABLE unnesting (treat as lateral join)

4. **Cost Model**:
```rust
fn json_path_cost(doc_size_bytes: f64, path_depth: usize) -> f64 {
    // Binary JSON allows O(1) field access, O(n) for arrays
    const BINARY_JSON_FIELD_ACCESS: f64 = 0.0001;
    const BINARY_JSON_ARRAY_SCAN: f64 = 0.001;

    path_depth as f64 * BINARY_JSON_FIELD_ACCESS
}
```

### Spatial MySQL-Specific Optimizations

**Complexity**: Medium (2-3 weeks)

**Building on Existing**: Ra already has generic spatial rules. Add MySQL-specific optimizations:

1. **MBR Bounding Box Pre-Filter**:
```rust
// rules/database-specific/mysql/spatial-mbr-prefilter.rra
rw!("mysql-spatial-mbr-prefilter";
    "(filter (st-contains ?g1 ?g2) ?rel)" =>
    "(filter (st-contains ?g1 ?g2)
       (filter (mbr-contains ?g1 ?g2) ?rel))"
    if is_database("mysql")
),
```

2. **SPATIAL Index Selection**:
```rust
// Metadata query
SELECT INDEX_NAME, COLUMN_NAME
FROM information_schema.STATISTICS
WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
  AND INDEX_TYPE = 'SPATIAL';
```

3. **Cost Model for R-Tree Indexes**:
```rust
fn spatial_index_cost(table_rows: f64, selectivity: f64) -> f64 {
    // R-tree lookup is O(log n) for point queries
    // Range queries depend on bounding box overlap
    const RTREE_NODE_ACCESS: f64 = 0.001;

    let tree_height = (table_rows.log2() / 50.0).ceil();  // Assume fanout ~50
    let index_cost = tree_height * RTREE_NODE_ACCESS;
    let result_rows = table_rows * selectivity;

    index_cost + result_rows * ROW_FETCH_COST
}
```

### Index/Optimizer Hints

**Complexity**: Medium (2-3 weeks)

**Implementation**:

1. **Parse Hint Comments**:
```rust
// USE INDEX, FORCE INDEX, IGNORE INDEX are part of table reference
// Extend sqlparser to capture these

pub struct TableRef {
    pub name: String,
    pub alias: Option<String>,
    pub index_hints: Vec<IndexHint>,
}

pub enum IndexHint {
    Use(Vec<String>),
    Force(Vec<String>),
    Ignore(Vec<String>),
}

// Optimizer hints in comments: /*+ ... */
// Parse hint comments manually or extend sqlparser
pub struct OptimizerHint {
    pub hint_type: HintType,
    pub args: Vec<String>,
}

pub enum HintType {
    HashJoin,
    NoHashJoin,
    BKA,
    NoIndex,
    MaxExecutionTime(u32),
    // ... 50+ MySQL 8.0 hints
}
```

2. **Apply Hints as Constraints**:
```rust
// In rule application, check if hints override cost-based decision
fn apply_rule(rule: &Rule, relexpr: &RelExpr, hints: &[OptimizerHint]) -> Option<RelExpr> {
    // If hint explicitly forces a strategy, skip cost comparison
    if has_forcing_hint(hints, rule) {
        return Some(rule.apply(relexpr));
    }

    // Normal cost-based decision
    if estimated_benefit(rule, relexpr) > 0.0 {
        Some(rule.apply(relexpr))
    } else {
        None
    }
}
```

---

## Testing Strategy

### Unit Tests

**Per Component**:
- Parser: Verify SQL → RelExpr conversion
- Core Types: Serialization, equality, display
- Metadata: Mock database, verify metadata extraction
- Rules: Precondition evaluation, transformation correctness
- Cost Model: Verify cost estimates are reasonable

**Example**:
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn parse_fulltext() {
        let sql = "SELECT * FROM t WHERE MATCH(col) AGAINST('query')";
        let relexpr = sql_to_relexpr(sql).unwrap();
        // Assert structure
    }

    #[test]
    fn fulltext_rule_precondition() {
        let table = create_test_table_with_fulltext_index();
        let predicate = FullTextMatch { /* ... */ };
        assert!(fulltext_rule_applicable(&table, &predicate));
    }
}
```

### Integration Tests

**With Real MySQL**:
- Docker container with MySQL 5.7, 8.0, MariaDB 10.3, 10.6, 11.1
- Load sample data (1K rows, 100K rows, 1M rows)
- Compare Ra optimization vs MySQL EXPLAIN

**Test Matrix**:
| Feature | MySQL 5.7 | MySQL 8.0 | MariaDB 10.3 | MariaDB 11.1 |
|---------|-----------|-----------|--------------|--------------|
| Full-Text | ✅ | ✅ | ✅ | ✅ |
| JSON | Limited | ✅ | ✅ | ✅ |
| JSON_TABLE | ❌ | ✅ | ❌ (10.6+) | ✅ |
| Functional Indexes | ❌ | ✅ | ❌ | ❌ |

**Script**: `tests/integration/run_mysql_tests.sh`
```bash
#!/usr/bin/env bash
set -euo pipefail

# Start Docker containers
docker-compose up -d mysql57 mysql80 mariadb103 mariadb111

# Run tests against each version
for db in mysql57 mysql80 mariadb103 mariadb111; do
  echo "Testing against $db"
  export TEST_DATABASE_URL="mysql://root:password@localhost:$(port_for $db)/test"
  cargo test --test mysql_integration -- --ignored
done

# Compare results
./scripts/compare_mysql_versions.py
```

### Property-Based Tests

**Use `proptest` crate**:
```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn fulltext_cost_monotonic(rows in 1000u64..10_000_000u64) {
        let stats1 = TableStats { row_count: rows as f64, /* ... */ };
        let stats2 = TableStats { row_count: (rows * 10) as f64, /* ... */ };

        let cost1 = fulltext_index_scan_cost(&stats1, "query", FullTextMode::NaturalLanguage);
        let cost2 = fulltext_index_scan_cost(&stats2, "query", FullTextMode::NaturalLanguage);

        // Cost should increase with table size (but sublinearly)
        prop_assert!(cost2 > cost1);
        prop_assert!(cost2 < cost1 * 10.0);  // Less than linear
    }
}
```

---

## Performance Validation

### Benchmark Against MySQL Optimizer

**Goal**: Verify Ra produces similar or better plans than MySQL.

**Method**:
1. Collect real-world queries from production MySQL systems
2. For each query:
   - Parse and optimize with Ra
   - Execute `EXPLAIN` on MySQL
   - Compare:
     - Index selection
     - Join order
     - Estimated cost
     - Actual execution time

**Metrics**:
- **Plan Quality**: % of queries where Ra chooses same or better plan
- **Cost Accuracy**: Correlation between Ra cost estimate and actual execution time
- **Optimization Time**: Time to optimize (should be < 100ms for OLTP queries)

**Script**: `benchmarks/compare_with_mysql.py`
```python
import mysql.connector
import subprocess
import json

def compare_optimization(query, conn):
    # MySQL EXPLAIN
    cursor = conn.cursor()
    cursor.execute(f"EXPLAIN FORMAT=JSON {query}")
    mysql_plan = json.loads(cursor.fetchone()[0])

    # Ra optimization
    result = subprocess.run(
        ["cargo", "run", "--bin", "ra-cli", "explain", query],
        capture_output=True,
        text=True
    )
    ra_plan = json.loads(result.stdout)

    # Compare
    comparison = {
        "query": query,
        "mysql_cost": extract_cost(mysql_plan),
        "ra_cost": ra_plan["cost"],
        "mysql_indexes": extract_indexes(mysql_plan),
        "ra_indexes": ra_plan["indexes"],
        "plans_match": plans_equivalent(mysql_plan, ra_plan),
    }

    return comparison

# Load queries from workload
queries = load_queries("workload/mysql_queries.sql")
results = [compare_optimization(q, conn) for q in queries]

# Report
print(f"Plan Match Rate: {sum(r['plans_match'] for r in results) / len(results):.1%}")
print(f"Cost Correlation: {correlation([r['mysql_cost'] for r in results],
                                       [r['ra_cost'] for r in results]):.2f}")
```

### Regression Tests

**Prevent Performance Regressions**:
- Commit query workload and expected plans to repo
- CI runs optimization on workload after every commit
- Alert if plans change or cost estimates regress

**Example**: `tests/regression/mysql_plans.yaml`
```yaml
- query: "SELECT * FROM articles WHERE MATCH(title, body) AGAINST('mysql')"
  expected_plan:
    type: fulltext_index_scan
    index: ft_idx
    estimated_cost: 150.0
  tolerance: 0.1  # Allow 10% cost variance
```

---

## Common Pitfalls and Solutions

### Pitfall 1: Parser Ambiguity

**Problem**: `sqlparser` doesn't support MySQL-specific syntax.

**Solutions**:
1. Fork `sqlparser` and add MySQL extensions
2. Pre-process SQL to convert MySQL syntax to generic form
3. Extend `sqlparser` upstream (preferred)

### Pitfall 2: Cost Model Calibration

**Problem**: Cost estimates don't match actual MySQL performance.

**Solution**: Calibrate cost constants against real MySQL instances.

```rust
// Calibration script
fn calibrate_costs() {
    let queries = load_benchmark_queries();
    let conn = MySqlConnector::connect("mysql://localhost/benchmark").unwrap();

    for query in queries {
        let ra_cost = estimate_cost(optimize(query));
        let actual_time = execute_and_measure(query, &conn);

        // Adjust cost constants to minimize error
        println!("Query: {}, Estimated: {}, Actual: {}", query, ra_cost, actual_time);
    }

    // Use linear regression to fit cost constants
}
```

### Pitfall 3: Version Compatibility

**Problem**: Feature availability varies across MySQL versions.

**Solution**: Query MySQL version and enable features conditionally.

```rust
impl MySqlConnector {
    pub fn get_version(&mut self) -> Result<MySqlVersion, MetadataError> {
        let version_str: String = self.conn
            .query_first("SELECT VERSION()")
            .map_err(|e| MetadataError::Query {
                message: format!("failed to query version: {e}"),
            })?
            .unwrap();

        MySqlVersion::parse(&version_str)
    }
}

pub struct MySqlVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub is_mariadb: bool,
}

impl MySqlVersion {
    pub fn supports_json_table(&self) -> bool {
        (!self.is_mariadb && self.major >= 8)
        || (self.is_mariadb && (self.major > 10 || (self.major == 10 && self.minor >= 6)))
    }
}
```

### Pitfall 4: Metadata Caching

**Problem**: Frequent metadata queries slow down optimization.

**Solution**: Cache schema metadata and invalidate on DDL changes.

```rust
pub struct MetadataCache {
    schemas: HashMap<String, (SchemaInfo, Instant)>,
    ttl: Duration,
}

impl MetadataCache {
    pub fn get_schema(&mut self, db: &str, conn: &mut MySqlConnector) -> SchemaInfo {
        if let Some((schema, timestamp)) = self.schemas.get(db) {
            if timestamp.elapsed() < self.ttl {
                return schema.clone();
            }
        }

        let schema = conn.gather_schema_mut().unwrap();
        self.schemas.insert(db.to_string(), (schema.clone(), Instant::now()));
        schema
    }
}
```

---

## Appendix: Quick Reference

### File Checklist for New Feature

- [ ] Parser: `crates/ra-parser/src/sql_to_relexpr.rs`
- [ ] Core types: `crates/ra-core/src/expr.rs`, `crates/ra-core/src/mysql/`
- [ ] Metadata: `crates/ra-metadata/src/mysql.rs`
- [ ] Dialect: `crates/ra-dialect/src/dialect.rs` (feature support matrix)
- [ ] Cost model: `crates/ra-engine/src/mysql_cost_models.rs`
- [ ] Rules: `rules/database-specific/mysql/*.rra`
- [ ] Tests: `tests/mysql/*.rs`, `tests/integration/*`
- [ ] Documentation: Update this guide and `MYSQL_MARIADB_UNSUPPORTED_FEATURES.md`

### Useful Commands

```bash
# Parse SQL and show RelExpr
cargo run --bin ra-cli -- parse "SELECT * FROM t WHERE MATCH(col) AGAINST('query')"

# Optimize and show plan
cargo run --bin ra-cli -- explain "SELECT * FROM t WHERE col = 10"

# Run MySQL-specific tests
cargo test --test mysql_integration -- --ignored

# Benchmark against MySQL
./benchmarks/compare_with_mysql.sh

# Validate all rules
cargo run --bin ra-cli -- validate rules/
```

### MySQL Source Code References

**Optimizer**:
- `sql/sql_optimizer.cc` - Main optimizer entry point
- `sql/sql_select.cc` - SELECT statement optimization
- `sql/opt_range.cc` - Range optimization and index selection
- `sql/item_func.cc` - Function evaluation

**Full-Text Search**:
- `storage/innobase/fts/fts0opt.cc` - InnoDB full-text optimization
- `storage/myisam/ft_boolean_search.c` - MyISAM boolean search

**JSON**:
- `sql/json_dom.cc` - JSON document object model
- `sql/item_json_func.cc` - JSON functions

**Spatial**:
- `sql/gis/` - Geometry types and functions
- `sql/spatial.cc` - Spatial index handling

---

**End of Guide**
