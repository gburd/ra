# RFC 0053: Stored Procedure Dialect Support

- Start Date: 2026-03-22
- Author: Ra Development Team
- Status: Proposed
- Tracking Issue: N/A

## Summary

Enable Ra optimizer to parse, analyze, and optimize stored procedures across multiple RDBMS dialects (PL/pgSQL, PL/SQL, T-SQL, MySQL procedures). This extends Ra's optimization capabilities beyond single SQL queries to multi-statement procedural code, enabling cross-database stored procedure analysis and optimization recommendations.

## Motivation

Modern database applications extensively use stored procedures for business logic, complex data transformations, and performance-critical operations. However, stored procedures are:

1. **Dialect-specific**: Each database has different syntax and semantics
2. **Hard to optimize**: Control flow makes static analysis challenging
3. **Difficult to migrate**: Porting between databases requires manual rewriting
4. **Performance-critical**: Often contain hotspots that need optimization

**Problems this solves:**

- **Cross-database migration**: Analyze stored procedures written in one dialect, identify optimization opportunities, assist with migration to another database
- **Performance analysis**: Identify inefficient patterns (cursor loops, unnecessary round-trips, missing indexes)
- **Code quality**: Detect anti-patterns and suggest improvements
- **Documentation**: Extract query patterns and dependencies from procedures

**Use cases:**

- Migrating Oracle PL/SQL procedures to PostgreSQL PL/pgSQL
- Optimizing MySQL stored procedures with slow query patterns
- Analyzing SQL Server T-SQL procedures for index recommendations
- Extracting embedded SQL queries for standalone optimization

## Guide-level explanation

When you have a stored procedure like this PostgreSQL PL/pgSQL example:

```sql
CREATE OR REPLACE FUNCTION get_customer_orders(customer_id INTEGER)
RETURNS TABLE(order_id INTEGER, total NUMERIC) AS $$
BEGIN
    RETURN QUERY
    SELECT o.id, o.total
    FROM orders o
    WHERE o.customer_id = get_customer_orders.customer_id
      AND o.status = 'completed'
    ORDER BY o.created_at DESC;
END;
$$ LANGUAGE plpgsql;
```

Ra can parse this procedure, extract the embedded SELECT query, and optimize it just like a standalone query. Ra will:

1. Parse the PL/pgSQL syntax and identify the `RETURN QUERY` statement
2. Extract the SELECT query
3. Optimize the query using Ra's optimizer (predicate pushdown, join reordering, index selection)
4. Provide recommendations like "Add index on orders(customer_id, status, created_at)"

For more complex procedures with control flow:

```sql
CREATE PROCEDURE process_orders() AS $$
DECLARE
    order_rec RECORD;
    total NUMERIC := 0;
BEGIN
    FOR order_rec IN
        SELECT id, amount FROM orders WHERE status = 'pending'
    LOOP
        UPDATE orders SET status = 'processing' WHERE id = order_rec.id;
        total := total + order_rec.amount;

        IF total > 10000 THEN
            RAISE NOTICE 'High volume batch: %', total;
        END IF;
    END LOOP;

    INSERT INTO batch_summary (total_amount, processed_at)
    VALUES (total, NOW());
END;
$$ LANGUAGE plpgsql;
```

Ra will:

1. Identify the cursor loop pattern (`FOR...IN SELECT`)
2. Analyze the UPDATE within the loop (potential N+1 problem)
3. Suggest set-based alternative: `UPDATE orders SET status = 'processing' WHERE status = 'pending'`
4. Extract all SQL statements for independent optimization

### Example Usage

```rust
use ra_parser::{parse_stored_procedure, ProcedureDialect};
use ra_core::algebra::RelExpr;

// Parse PL/pgSQL procedure
let procedure = parse_stored_procedure(plpgsql_code, ProcedureDialect::PlPgSQL)?;

// Extract SQL queries
let queries: Vec<RelExpr> = procedure.extract_queries();

// Optimize each query
for query in queries {
    let optimized = optimizer.optimize(&query)?;
    println!("Original cost: {}", query.cost());
    println!("Optimized cost: {}", optimized.cost());
}

// Detect anti-patterns
let issues = procedure.detect_antipatterns();
for issue in issues {
    println!("WARNING: {}: {}", issue.severity, issue.message);
}
```

## Reference-level explanation

### Implementation Details

**Parser Architecture:**

```rust
// Core procedural AST
pub enum ProcedureStmt {
    Declare { vars: Vec<VarDecl> },
    Assign { target: Ident, expr: Expr },
    If { condition: Expr, then_block: Vec<ProcedureStmt>, else_block: Option<Vec<ProcedureStmt>> },
    Loop { body: Vec<ProcedureStmt> },
    While { condition: Expr, body: Vec<ProcedureStmt> },
    ForLoop { var: Ident, query: Box<RelExpr>, body: Vec<ProcedureStmt> },
    Return { expr: Option<Expr> },
    ReturnQuery { query: Box<RelExpr> },
    Sql { stmt: SqlStmt },  // Embedded SQL: SELECT, INSERT, UPDATE, DELETE
    Raise { level: LogLevel, message: String },
    Exit { label: Option<String> },
}

pub struct StoredProcedure {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<Type>,
    pub variables: Vec<VarDecl>,
    pub body: Vec<ProcedureStmt>,
    pub dialect: ProcedureDialect,
}

pub enum ProcedureDialect {
    PlPgSQL,   // PostgreSQL PL/pgSQL
    PlSQL,     // Oracle PL/SQL
    TSQL,      // SQL Server T-SQL
    MySQL,     // MySQL stored procedures
}
```

**Dialect-Specific Parsers:**

Each dialect has a specialized parser that extends the base SQL parser:

```rust
pub trait ProcedureParser {
    fn parse_procedure(&self, input: &str) -> Result<StoredProcedure>;
    fn parse_function(&self, input: &str) -> Result<StoredProcedure>;
    fn parse_block(&self, input: &str) -> Result<Vec<ProcedureStmt>>;
}

impl ProcedureParser for PlPgSQLParser {
    // PL/pgSQL-specific syntax:
    // - DECLARE ... BEGIN ... END
    // - FOR...IN...LOOP
    // - RAISE NOTICE/EXCEPTION
    // - RETURN NEXT, RETURN QUERY
}

impl ProcedureParser for PlSQLParser {
    // PL/SQL-specific syntax:
    // - DECLARE ... BEGIN ... EXCEPTION ... END
    // - FOR...IN...LOOP with BULK COLLECT
    // - RAISE_APPLICATION_ERROR
    // - FORALL statements
    // - %TYPE and %ROWTYPE
}

impl ProcedureParser for TSQLParser {
    // T-SQL-specific syntax:
    // - DECLARE @var
    // - BEGIN...END blocks
    // - RAISERROR
    // - WHILE loops
    // - TRY...CATCH blocks
}
```

**Query Extraction:**

```rust
impl StoredProcedure {
    /// Extract all SQL queries from the procedure
    pub fn extract_queries(&self) -> Vec<ExtractedQuery> {
        let mut queries = Vec::new();
        self.visit_statements(&self.body, &mut |stmt| {
            match stmt {
                ProcedureStmt::Sql(SqlStmt::Select(query)) => {
                    queries.push(ExtractedQuery {
                        query: query.clone(),
                        context: QueryContext::Statement,
                        line_number: stmt.span().start_line(),
                    });
                }
                ProcedureStmt::ForLoop { query, .. } => {
                    queries.push(ExtractedQuery {
                        query: query.clone(),
                        context: QueryContext::CursorLoop,
                        line_number: stmt.span().start_line(),
                    });
                }
                ProcedureStmt::ReturnQuery { query } => {
                    queries.push(ExtractedQuery {
                        query: query.clone(),
                        context: QueryContext::Return,
                        line_number: stmt.span().start_line(),
                    });
                }
                _ => {}
            }
        });
        queries
    }
}
```

**Anti-pattern Detection:**

```rust
pub struct AntiPattern {
    pub severity: Severity,
    pub message: String,
    pub suggestion: String,
    pub line_number: usize,
}

impl StoredProcedure {
    pub fn detect_antipatterns(&self) -> Vec<AntiPattern> {
        let mut issues = Vec::new();

        // Detect cursor loops with UPDATE/DELETE inside
        for stmt in &self.body {
            if let ProcedureStmt::ForLoop { body, .. } = stmt {
                for inner_stmt in body {
                    if matches!(inner_stmt, ProcedureStmt::Sql(SqlStmt::Update(_) | SqlStmt::Delete(_))) {
                        issues.push(AntiPattern {
                            severity: Severity::High,
                            message: "N+1 problem: UPDATE/DELETE inside cursor loop".to_string(),
                            suggestion: "Consider set-based UPDATE/DELETE instead of row-by-row".to_string(),
                            line_number: stmt.span().start_line(),
                        });
                    }
                }
            }
        }

        // Detect SELECT without WHERE in loop
        // Detect missing indexes on joined tables
        // Detect unnecessary DISTINCT
        // Detect inefficient EXISTS checks
        // ... more patterns

        issues
    }
}
```

### Integration Points

**1. Parser Integration:**

Extend `ra-parser` with new modules:
- `ra-parser::procedure` - Base procedural AST and traits
- `ra-parser::plpgsql` - PostgreSQL PL/pgSQL parser
- `ra-parser::plsql` - Oracle PL/SQL parser
- `ra-parser::tsql` - SQL Server T-SQL parser
- `ra-parser::mysql_proc` - MySQL procedure parser

**2. Optimizer Integration:**

```rust
impl Optimizer {
    pub fn optimize_procedure(&self, proc: &StoredProcedure) -> OptimizedProcedure {
        let mut optimized_queries = Vec::new();

        for query in proc.extract_queries() {
            let opt_query = self.optimize(&query.query)?;
            optimized_queries.push((query.context, opt_query));
        }

        OptimizedProcedure {
            original: proc.clone(),
            optimized_queries,
            recommendations: proc.detect_antipatterns(),
        }
    }
}
```

**3. Cost Model:**

Procedures require different cost modeling:
- **Control flow overhead**: IF statements, loops
- **Variable assignments**: Memory operations
- **Cursor operations**: Fetching rows iteratively
- **Context switches**: Switching between procedural and SQL execution

**4. Index Advisor Integration:**

Extract queries from procedures and feed them to the index advisor (RFC 0021):

```rust
let procedure = parse_stored_procedure(code)?;
let queries = procedure.extract_queries();

for query in queries {
    let recommendations = index_advisor.recommend_indexes(&query);
    println!("Query at line {}: {}", query.line_number, recommendations);
}
```

### Error Handling

**Parse Errors:**

```rust
pub enum ProcedureParseError {
    SyntaxError { line: usize, column: usize, message: String },
    UnsupportedConstruct { construct: String, dialect: ProcedureDialect },
    UnknownDialect { input: String },
    NestedProcedureNotSupported,
}
```

**Optimization Errors:**

- **Unsupported SQL features**: Some embedded SQL may use dialect-specific extensions not yet supported by Ra
- **Dynamic SQL**: Cannot optimize SQL built as strings at runtime
- **External dependencies**: Procedures calling other procedures or external functions

**Fallback Strategy:**

- If a construct is unsupported, skip optimization for that section but continue analyzing the rest
- Emit warnings for unsupported features
- Provide partial optimization results

### Performance Considerations

**Parsing Performance:**

- Stored procedures can be large (1000+ lines)
- Use incremental parsing: parse only changed sections when re-analyzing
- Cache parsed AST keyed by procedure hash

**Memory:**

- Full AST for large procedures can consume significant memory
- Use arena allocation for AST nodes
- Support streaming analysis (don't hold entire AST in memory)

**Optimization Time:**

- Each embedded query needs optimization
- Parallelize query optimization within a procedure
- Set timeout limits for complex procedures (e.g., 10 seconds)

## Drawbacks

**Complexity:**

- Each dialect has unique syntax and semantics
- Maintaining 4+ dialect parsers is significant work
- Edge cases and dialect-specific features are numerous

**Incomplete Coverage:**

- Cannot support every dialect feature immediately
- Dynamic SQL (strings built at runtime) is fundamentally hard to optimize
- Procedures calling external code (UDFs, external procedures) are opaque

**Maintenance Burden:**

- Each database vendor updates their procedural language
- Must track dialect evolution (e.g., PostgreSQL 17 PL/pgSQL changes)
- Testing requires comprehensive procedure corpus for each dialect

**Performance Overhead:**

- Parsing large procedures is slower than single queries
- Control flow analysis adds complexity
- May not be suitable for real-time optimization

## Rationale and alternatives

### Why This Design?

**Modular Dialect Support:**

- Each dialect has its own parser implementing a common trait
- Easy to add new dialects incrementally
- Shared core AST with dialect-specific extensions

**Query-Centric Optimization:**

- Focus on optimizing embedded SQL queries first
- Control flow optimization is a future enhancement
- Delivers value immediately (extract and optimize queries)

**Anti-pattern Detection:**

- Complements query optimization with structural analysis
- Helps developers understand their code
- Provides actionable recommendations

### Alternative Approaches

**1. Single Universal Procedural Language:**

- Define Ra's own procedural language
- Transpile from other dialects to Ra procedural language
- **Rejected**: Too ambitious, doesn't help existing codebases

**2. SQL-Only (No Procedures):**

- Only optimize SQL queries, ignore procedural code
- **Rejected**: Misses major use case (stored procedures are widely used)

**3. Black-Box Procedure Analysis:**

- Treat procedures as opaque, only analyze execution plans
- **Rejected**: Cannot provide static analysis or migration help

**4. Full Control Flow Optimization:**

- Optimize loops, conditionals, variable usage
- **Rejected**: Too complex for initial version, limited benefit vs. query optimization

### Impact of Not Doing This

**Without stored procedure support:**

- Ra cannot analyze a large portion of database workloads
- Migration tooling is incomplete (can't handle procedures)
- Developers must manually extract queries from procedures
- Anti-pattern detection is not available for procedural code
- Cross-database procedure comparison is impossible

**Workaround:**

- Users manually extract SQL queries from procedures
- Use database-specific tools for procedure analysis
- Limited cross-database portability

## Prior art

### Academic Research

**Static Analysis of Stored Procedures:**

- [Checking Database Schema Update Safety](https://dl.acm.org/doi/10.1145/3133956) - Analyzing schema evolution with procedures
- [Automated Testing of Database Schema Migrations](https://ieeexplore.ieee.org/document/8330239) - Includes procedure migration testing

**Cross-Database Query Translation:**

- [QTrans: Database Schema and Query Translation](https://arxiv.org/abs/2109.09204) - Translating queries between SQL dialects (could extend to procedures)

### Industry Solutions

**PostgreSQL:**

- PL/pgSQL is a block-structured language similar to Oracle PL/SQL
- Built-in validation and static analysis via `plpgsql_check` extension
- Can extract query plans for embedded queries via `EXPLAIN`

**Oracle:**

- PL/SQL is feature-rich with advanced control flow
- `DBMS_UTILITY.GET_DEPENDENCY` extracts procedure dependencies
- Oracle SQL Developer provides static analysis and tuning recommendations
- Bulk operations (`FORALL`, `BULK COLLECT`) for performance

**SQL Server:**

- T-SQL procedures with `BEGIN...END` blocks
- `SET STATISTICS IO ON` shows query I/O inside procedures
- SQL Server Management Studio provides execution plan analysis
- Query Store tracks procedure performance over time

**MySQL:**

- Stored procedures with limited control flow
- `SHOW PROCEDURE STATUS` and `SHOW CREATE PROCEDURE`
- No built-in static analysis tools
- Performance schema tracks procedure execution

**Migration Tools:**

- **AWS Schema Conversion Tool (SCT)**: Converts Oracle PL/SQL to PostgreSQL PL/pgSQL
- **Ispirer SQLWays**: Multi-database procedure conversion
- **SwisSQL**: Converts stored procedures between dialects
- All rely on pattern matching and manual review

**Apache Calcite:**

- Focuses on SQL query optimization, minimal stored procedure support
- No procedural language parser

**What We Can Learn:**

- PL/pgSQL and PL/SQL have extensive prior art for parsing
- Anti-pattern detection is valuable (cursor loops, N+1 queries)
- Static analysis tools exist but are database-specific
- Migration tools show demand for cross-dialect procedure analysis
- Query extraction is the most valuable initial feature

## Unresolved questions

**Design Questions:**

1. Should Ra define a canonical procedural IR (intermediate representation), or keep dialect-specific ASTs?
2. How to handle dialect-specific features with no cross-database equivalent (e.g., Oracle packages, SQL Server table variables)?
3. Should Ra support procedure-to-procedure call graph analysis?

**Implementation Questions:**

1. Which dialect should be implemented first? (Recommendation: PL/pgSQL, as it's similar to PL/SQL and widely used)
2. How to handle incremental parsing for large procedures?
3. Should Ra cache parsed procedures, and how to invalidate cache?

**Integration Questions:**

1. How to integrate with Ra's existing query parser? (Shared lexer, separate grammar?)
2. Should procedure optimization be part of the core `ra-core` or a separate `ra-procedure` crate?
3. How to expose procedure analysis in the PostgreSQL extension (`ra-pg-extension`)?

**Out of Scope:**

- **Procedure execution**: Ra is an optimizer, not a database engine
- **Debugging**: Step-through debugging of procedures
- **Full semantic analysis**: Type checking, scope resolution (only basic validation)
- **Automatic procedure refactoring**: Suggest improvements but don't auto-rewrite code (yet)

## Future possibilities

### Natural Extensions

**1. Control Flow Optimization:**

- Hoist invariant computations out of loops
- Constant folding for procedure variables
- Dead code elimination (unreachable IF branches)

**2. Procedure Inlining:**

- Inline small procedures into caller
- Reduces context switches and enables cross-procedure optimization

**3. Set-Based Transformation:**

- Automatically convert cursor loops to set-based operations
- Detect row-by-row patterns and suggest bulk operations

**4. Cross-Database Translation:**

- Given Oracle PL/SQL, generate equivalent PostgreSQL PL/pgSQL
- Handle dialect differences (syntax, built-in functions, semantics)
- Provide migration reports highlighting manual review areas

**5. Procedure Performance Modeling:**

- Estimate execution time for procedures
- Model control flow (loops, conditionals) in cost model
- Predict procedure scalability based on data size

**6. Dependency Analysis:**

- Extract procedure dependencies (which tables, views, other procedures)
- Impact analysis: "If I change this table, which procedures are affected?"
- Build call graph for multi-procedure analysis

### Long-term Vision

Ra becomes a **universal stored procedure analyzer** that can:

- Parse procedures from any major database dialect
- Provide cross-database migration assistance
- Detect anti-patterns and suggest improvements
- Extract and optimize all embedded SQL queries
- Estimate procedure performance across databases
- Generate test cases for procedure validation

This positions Ra as a key tool for:

- **Database migration projects** (Oracle -> PostgreSQL, SQL Server -> PostgreSQL)
- **Performance tuning** (identify slow procedures, suggest optimizations)
- **Code quality** (enforce best practices, detect anti-patterns)
- **Documentation** (extract dependencies, generate call graphs)

Integration with existing RFCs:

- **RFC 0021 (Index Advisor)**: Recommend indexes for queries in procedures
- **RFC 0051 (Materialized Views)**: Detect queries in procedures that could use materialized views
- **RFC 0052 (Progressive Re-Optimization)**: Re-optimize procedures when statistics change
- **RFC 0054 (Streaming Plans)**: Adapt procedure execution plans dynamically

This RFC lays the foundation for Ra to handle the full spectrum of database workloads, not just ad-hoc queries.
