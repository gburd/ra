# RFC 0053: Stored Procedure Dialect Support

- Start Date: 2026-03-24
- Author: Ra Development Team
- Status: Draft
- Tracking Issue: TBD

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

### Oracle PL/SQL Example

```sql
CREATE OR REPLACE PROCEDURE transfer_funds(
    p_from_acct IN NUMBER,
    p_to_acct   IN NUMBER,
    p_amount    IN NUMBER
) AS
    v_balance NUMBER;
    insufficient_funds EXCEPTION;
BEGIN
    SELECT balance INTO v_balance
    FROM accounts WHERE account_id = p_from_acct
    FOR UPDATE;

    IF v_balance < p_amount THEN
        RAISE insufficient_funds;
    END IF;

    UPDATE accounts SET balance = balance - p_amount
    WHERE account_id = p_from_acct;

    UPDATE accounts SET balance = balance + p_amount
    WHERE account_id = p_to_acct;

    INSERT INTO transactions (from_acct, to_acct, amount, txn_date)
    VALUES (p_from_acct, p_to_acct, p_amount, SYSDATE);

    COMMIT;
EXCEPTION
    WHEN insufficient_funds THEN
        RAISE_APPLICATION_ERROR(-20001, 'Insufficient funds');
    WHEN OTHERS THEN
        ROLLBACK;
        RAISE;
END transfer_funds;
```

Ra parses the PL/SQL exception handling block, extracts the three DML
statements, and recognizes the `SELECT...FOR UPDATE` pattern as a
locking read. It can recommend an index on `accounts(account_id)` and
flag the separate UPDATE statements as candidates for a single
`MERGE` statement.

### SQL Server T-SQL Example

```sql
CREATE PROCEDURE dbo.RecalculateInventory
    @warehouse_id INT
AS
BEGIN
    SET NOCOUNT ON;

    DECLARE @product_id INT, @qty INT;
    DECLARE product_cursor CURSOR LOCAL FAST_FORWARD FOR
        SELECT product_id, SUM(quantity) AS qty
        FROM inventory_movements
        WHERE warehouse_id = @warehouse_id
        GROUP BY product_id;

    OPEN product_cursor;
    FETCH NEXT FROM product_cursor INTO @product_id, @qty;

    WHILE @@FETCH_STATUS = 0
    BEGIN
        UPDATE inventory_levels
        SET quantity = @qty, last_updated = GETDATE()
        WHERE warehouse_id = @warehouse_id
          AND product_id = @product_id;

        FETCH NEXT FROM product_cursor INTO @product_id, @qty;
    END;

    CLOSE product_cursor;
    DEALLOCATE product_cursor;

    BEGIN TRY
        EXEC dbo.NotifyInventoryChange @warehouse_id;
    END TRY
    BEGIN CATCH
        INSERT INTO error_log (message, error_number, occurred_at)
        VALUES (ERROR_MESSAGE(), ERROR_NUMBER(), GETDATE());
    END CATCH;
END;
```

Ra identifies the explicit cursor pattern with `WHILE @@FETCH_STATUS`,
flags the row-by-row UPDATE as an N+1 anti-pattern, and suggests
replacing the entire cursor loop with a single `UPDATE...FROM` or
`MERGE` statement. It also recognizes the `TRY...CATCH` block and
nested procedure call.

### MySQL Example

```sql
DELIMITER //
CREATE PROCEDURE calculate_discounts(IN campaign_id INT)
BEGIN
    DECLARE done INT DEFAULT FALSE;
    DECLARE v_order_id INT;
    DECLARE v_total DECIMAL(10,2);
    DECLARE cur CURSOR FOR
        SELECT order_id, total_amount
        FROM orders WHERE campaign = campaign_id;
    DECLARE CONTINUE HANDLER FOR NOT FOUND SET done = TRUE;

    OPEN cur;
    read_loop: LOOP
        FETCH cur INTO v_order_id, v_total;
        IF done THEN
            LEAVE read_loop;
        END IF;

        IF v_total > 100.00 THEN
            UPDATE orders
            SET discount = v_total * 0.10
            WHERE order_id = v_order_id;
        ELSE
            UPDATE orders
            SET discount = 0
            WHERE order_id = v_order_id;
        END IF;
    END LOOP;
    CLOSE cur;
END //
DELIMITER ;
```

Ra parses the MySQL cursor declaration with its `HANDLER FOR NOT FOUND`
pattern, extracts the cursor query and the two UPDATE statements, and
suggests a single set-based UPDATE with a `CASE` expression:
`UPDATE orders SET discount = CASE WHEN total_amount > 100 THEN total_amount * 0.10 ELSE 0 END WHERE campaign = ?`.

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

Each dialect has a specialized parser implementing a common trait:

```rust
pub trait ProcedureParser {
    fn parse_procedure(&self, input: &str) -> Result<StoredProcedure>;
    fn parse_function(&self, input: &str) -> Result<StoredProcedure>;
    fn parse_block(&self, input: &str) -> Result<Vec<ProcedureStmt>>;
}
```

#### Control Flow Parsing

Each dialect has distinct control flow syntax that maps to
`ProcedureStmt` variants:

| Construct | PL/pgSQL | PL/SQL | T-SQL | MySQL |
|-----------|----------|--------|-------|-------|
| Conditional | `IF...THEN...ELSIF...ELSE...END IF` | `IF...THEN...ELSIF...ELSE...END IF` | `IF...ELSE IF...ELSE` | `IF...THEN...ELSEIF...ELSE...END IF` |
| Simple loop | `LOOP...END LOOP` | `LOOP...END LOOP` | `WHILE 1=1 BEGIN...END` | `LOOP...END LOOP` |
| While loop | `WHILE...LOOP...END LOOP` | `WHILE...LOOP...END LOOP` | `WHILE...BEGIN...END` | `WHILE...DO...END WHILE` |
| For (numeric) | `FOR i IN 1..10 LOOP` | `FOR i IN 1..10 LOOP` | N/A (use WHILE) | N/A (use WHILE) |
| For (cursor) | `FOR rec IN SELECT...LOOP` | `FOR rec IN cursor_name LOOP` | `FETCH` + `WHILE @@FETCH_STATUS` | `FETCH` + `HANDLER FOR NOT FOUND` |
| Case | `CASE...WHEN...THEN...END CASE` | `CASE...WHEN...THEN...END CASE` | `CASE` (expression only) | `CASE...WHEN...THEN...END CASE` |
| Exit/Break | `EXIT [WHEN condition]` | `EXIT [WHEN condition]` | `BREAK` | `LEAVE label` |
| Continue | `CONTINUE [WHEN condition]` | `CONTINUE [WHEN condition]` | `CONTINUE` | `ITERATE label` |

The parser normalizes these into the common `ProcedureStmt` enum. For
example, T-SQL `WHILE @@FETCH_STATUS = 0 BEGIN...FETCH NEXT...END` and
PL/pgSQL `FOR rec IN query LOOP...END LOOP` both produce the same
`ForLoop` variant.

#### DML Parsing Within Procedures

Embedded DML statements are parsed using the existing `ra-parser`
SQL-to-RelExpr pipeline. Each dialect wraps DML differently:

```rust
pub enum SqlStmt {
    Select(Box<RelExpr>),
    Insert {
        table: String,
        columns: Vec<String>,
        source: InsertSource,
    },
    Update {
        table: String,
        assignments: Vec<(String, Expr)>,
        filter: Option<Expr>,
        from: Option<Box<RelExpr>>,
    },
    Delete {
        table: String,
        filter: Option<Expr>,
        using: Option<Box<RelExpr>>,
    },
    Merge {
        target: String,
        source: Box<RelExpr>,
        condition: Expr,
        when_matched: Vec<MergeAction>,
        when_not_matched: Vec<MergeAction>,
    },
}

pub enum InsertSource {
    Values(Vec<Vec<Expr>>),
    Query(Box<RelExpr>),
    Default,
}
```

Dialect-specific DML syntax is handled during parsing:

- **PL/pgSQL**: `PERFORM` (execute query, discard result), `EXECUTE`
  (dynamic SQL), `INSERT...RETURNING INTO`
- **PL/SQL**: `SELECT...INTO`, `INSERT...RETURNING...INTO`, `FORALL`
  (bulk DML), `EXECUTE IMMEDIATE`
- **T-SQL**: `INSERT...OUTPUT`, `UPDATE...FROM`, `DELETE...FROM`,
  `EXEC`/`sp_executesql`
- **MySQL**: `SELECT...INTO`, `INSERT...ON DUPLICATE KEY UPDATE`

#### Cursor and Cursor Loop Parsing

Cursors are a major optimization target. Each dialect declares and
uses cursors differently:

```rust
pub struct CursorDecl {
    pub name: String,
    pub query: Box<RelExpr>,
    pub parameters: Vec<Parameter>,
    pub properties: CursorProperties,
}

pub struct CursorProperties {
    pub scrollable: bool,
    pub holdable: bool,
    pub sensitivity: CursorSensitivity,
    pub updatable: bool,
}

pub enum CursorSensitivity {
    Insensitive,
    Sensitive,
    Asensitive,
}

pub enum CursorOp {
    Open { cursor: String },
    Fetch { cursor: String, into: Vec<String>, direction: FetchDirection },
    Close { cursor: String },
    Deallocate { cursor: String },
}

pub enum FetchDirection {
    Next,
    Prior,
    First,
    Last,
    Absolute(i64),
    Relative(i64),
}
```

Dialect differences in cursor handling:

- **PL/pgSQL**: Implicit cursors via `FOR rec IN query`, refcursors,
  `MOVE` for repositioning without fetching
- **PL/SQL**: `%FOUND`, `%NOTFOUND`, `%ROWCOUNT`, `%ISOPEN`
  attributes, parameterized cursors, `BULK COLLECT INTO` for batch
  fetch
- **T-SQL**: `DECLARE CURSOR` with options (`LOCAL|GLOBAL`,
  `STATIC|DYNAMIC|KEYSET|FAST_FORWARD`), `@@FETCH_STATUS` for loop
  control, `DEALLOCATE` required
- **MySQL**: `DECLARE CURSOR FOR`, `DECLARE HANDLER FOR NOT FOUND`,
  forward-only, read-only

#### Exception and Error Handling

Exception handling varies across dialects:

```rust
pub enum ExceptionHandler {
    Named {
        exception_name: String,
        body: Vec<ProcedureStmt>,
    },
    SqlState {
        state: String,
        body: Vec<ProcedureStmt>,
    },
    Others {
        body: Vec<ProcedureStmt>,
    },
}

pub enum RaiseStmt {
    PlPgSQL {
        level: PlPgSQLLogLevel,
        message: String,
        detail: Option<String>,
        hint: Option<String>,
        errcode: Option<String>,
    },
    PlSQL {
        error_code: i32,
        message: String,
    },
    TSQL {
        message: String,
        severity: i32,
        state: i32,
    },
    MySQL {
        condition: String,
        message: Option<String>,
    },
}
```

Dialect mapping for exception handling:

| Feature | PL/pgSQL | PL/SQL | T-SQL | MySQL |
|---------|----------|--------|-------|-------|
| Block | `EXCEPTION WHEN...THEN` | `EXCEPTION WHEN...THEN` | `BEGIN TRY...END TRY BEGIN CATCH...END CATCH` | `DECLARE HANDLER FOR...` |
| Re-raise | `RAISE` (no args) | `RAISE` (no args) | `THROW` (no args) | `RESIGNAL` |
| Custom error | `RAISE EXCEPTION` | `RAISE_APPLICATION_ERROR` | `RAISERROR`/`THROW` | `SIGNAL SQLSTATE` |
| Built-in exceptions | `NO_DATA_FOUND`, `TOO_MANY_ROWS` | `NO_DATA_FOUND`, `TOO_MANY_ROWS`, `DUP_VAL_ON_INDEX` | `ERROR_NUMBER()`, `ERROR_MESSAGE()` | `SQLSTATE`, `SQLEXCEPTION` |
| Nested handlers | Yes (per block) | Yes (per block) | Yes (nested TRY/CATCH) | Yes (per block) |

All exception handler patterns are normalized into `ExceptionHandler`
variants during parsing. This allows Ra to analyze error handling paths
for embedded DML -- for example, identifying a `SELECT...INTO` that
could raise `NO_DATA_FOUND` and whether the handler contains
compensating DML that also needs optimization.

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

Procedures require cost modeling beyond standard relational algebra:

```rust
pub struct ProcedureCost {
    pub total_sql_cost: f64,
    pub control_flow_overhead: f64,
    pub cursor_overhead: f64,
    pub context_switch_cost: f64,
    pub estimated_iterations: Option<u64>,
}

impl ProcedureCostEstimator {
    pub fn estimate(&self, proc: &StoredProcedure) -> ProcedureCost {
        let mut cost = ProcedureCost::default();
        for stmt in &proc.body {
            self.cost_statement(stmt, &mut cost);
        }
        cost
    }

    fn cost_statement(
        &self,
        stmt: &ProcedureStmt,
        cost: &mut ProcedureCost,
    ) {
        match stmt {
            ProcedureStmt::Sql(sql) => {
                cost.total_sql_cost += self.sql_cost(sql);
                cost.context_switch_cost += CONTEXT_SWITCH_UNIT;
            }
            ProcedureStmt::ForLoop { query, body, .. } => {
                let rows = self.estimate_cardinality(query);
                cost.cursor_overhead += rows * FETCH_UNIT;
                for inner in body {
                    let inner_cost = self.cost_once(inner);
                    cost.total_sql_cost += inner_cost * rows;
                }
                cost.estimated_iterations = Some(rows as u64);
            }
            ProcedureStmt::If { then_block, else_block, .. } => {
                // Estimate both branches, weight by selectivity
                let then_cost = self.cost_block(then_block);
                let else_cost = else_block
                    .as_ref()
                    .map_or(0.0, |b| self.cost_block(b));
                cost.total_sql_cost += (then_cost + else_cost) / 2.0;
            }
            _ => {}
        }
    }
}
```

Key cost factors:
- **Context switches**: Each transition between procedural and SQL
  engine adds ~0.1ms overhead (per PostgreSQL benchmarks)
- **Cursor fetch**: Each `FETCH` is a context switch plus row
  materialization; bulk fetch (`BULK COLLECT`, `FETCH...INTO` with
  array) amortizes this
- **Loop body cost**: Multiply single-iteration cost by estimated row
  count from cursor query cardinality
- **Exception handling**: Handlers have near-zero cost until triggered;
  the setup cost is a single stack frame push

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

- PL/pgSQL is a block-structured language deliberately similar to
  Oracle PL/SQL to ease migration
- The PL/pgSQL executor pre-parses SQL statements on first execution
  and caches prepared plans via the SPI (Server Programming Interface)
- Plan invalidation occurs when catalog changes affect cached plans
- `plpgsql_check` extension provides static analysis: detects unused
  variables, unhandled exceptions, SQL injection in dynamic queries,
  and performance anti-patterns
- `auto_explain` can log execution plans for queries inside procedures
- PostgreSQL 14+ supports procedure-level `CALL` with transaction
  control (`COMMIT`/`ROLLBACK` inside procedures)
- Limitation: no automatic cursor-to-set rewriting; developers must
  manually refactor row-at-a-time code

**Oracle:**

- PL/SQL is the most feature-rich procedural SQL language, with
  packages, object types, nested table types, and autonomous
  transactions
- Oracle's PL/SQL compiler performs native compilation (NCOMP) to
  machine code since Oracle 11g
- Intra-unit inlining: the compiler can inline PL/SQL function calls
  within the same compilation unit (`PRAGMA INLINE`)
- Scalar subquery caching: Oracle caches results of PL/SQL function
  calls in SQL contexts, keyed by input parameters
- `DBMS_UTILITY.GET_DEPENDENCY` and `DBA_DEPENDENCIES` views track
  object dependencies including procedures
- `FORALL` and `BULK COLLECT` reduce context switches between PL/SQL
  and SQL engines by batching operations
- Oracle 12c introduced the `WITH FUNCTION` clause allowing inline
  PL/SQL in SQL queries, enabling the optimizer to see function bodies
- Limitation: `EXECUTE IMMEDIATE` (dynamic SQL) is opaque to the
  optimizer

**SQL Server:**

- T-SQL procedures are compiled to query plans on first execution
  (parameter sniffing)
- Plan guides and `OPTIMIZE FOR` hints address parameter sniffing
  problems
- SQL Server 2019+ supports scalar UDF inlining: the optimizer
  replaces calls to eligible scalar functions with their body
  expression, avoiding per-row function call overhead
- Natively compiled stored procedures (Hekaton) compile T-SQL to C
  code then to machine code for in-memory OLTP tables
- Query Store (SQL Server 2016+) tracks plan changes over time for
  queries inside procedures, enabling forced plan regression fixes
- `SET STATISTICS IO ON` and `SET STATISTICS TIME ON` show per-query
  I/O and timing within procedure execution
- Limitation: cursor-based T-SQL cannot be automatically rewritten by
  the engine

**MySQL:**

- MySQL stored procedures use a simple bytecode interpreter
- Each SQL statement inside a procedure is parsed, optimized, and
  executed independently -- no cross-statement optimization
- Cursors are forward-only and read-only; no bulk fetch support
- `HANDLER FOR NOT FOUND` is the only cursor exhaustion pattern
- MySQL 8.0+ stores prepared statement plans in memory but does not
  share plans across procedure invocations
- Performance Schema (`events_statements_summary_by_digest`) tracks
  query execution statistics within procedures
- Limitation: no static analysis tools, no native compilation, and no
  plan caching across calls

**Migration Tools:**

- **AWS Schema Conversion Tool (SCT)**: Converts Oracle PL/SQL to
  PostgreSQL PL/pgSQL with an assessment report scoring conversion
  complexity per procedure
- **Ora2Pg**: Open-source tool focused on Oracle-to-PostgreSQL
  migration, handles most PL/SQL constructs, reports unconverted items
- **Ispirer SQLWays**: Commercial multi-database procedure conversion
  supporting 20+ source/target combinations
- **SSMA (SQL Server Migration Assistant)**: Microsoft tool for
  migrating Oracle/MySQL/DB2 procedures to T-SQL
- All rely on AST-level pattern matching with manual review for
  edge cases -- none perform optimization of the converted code

**Apache Calcite:**

- Focuses entirely on SQL query optimization with no procedural
  language parser
- Relevant insight: Calcite's `RelNode` tree is the closest prior art
  to Ra's `RelExpr`, showing that a common algebraic representation
  can serve as the optimization target for extracted queries

### What We Can Learn

1. **Query extraction is the highest-value first step.** All four
   databases optimize embedded SQL independently. Ra can add value by
   optimizing extracted queries with its cross-database rule set.

2. **Cursor-to-set rewriting is universally desired but unsupported.**
   No production database engine automatically converts cursor loops
   to set-based operations. This is a differentiation opportunity.

3. **Plan caching semantics vary.** PostgreSQL caches per-session,
   Oracle caches globally with dependency tracking, SQL Server uses
   plan cache with parameter sniffing. Ra's analysis should be
   cache-aware when estimating procedure performance.

4. **Static analysis tools exist but are database-specific.** The
   `plpgsql_check` extension for PostgreSQL is the most mature. Ra
   can provide cross-database static analysis with a unified
   anti-pattern catalog.

5. **Migration tools prove market demand.** AWS SCT, Ora2Pg, Ispirer,
   and SSMA all exist specifically because cross-dialect procedure
   conversion is a painful manual process. Ra's common AST enables
   analysis that none of these tools perform (optimization of the
   converted code).

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

## Implementation Strategy

### Phase 1: Core AST and PL/pgSQL Parser

- [ ] Define `ProcedureStmt`, `StoredProcedure`, `SqlStmt`,
      `CursorDecl`, `ExceptionHandler` types in `ra-core`
- [ ] Implement `ProcedureParser` trait
- [ ] Build PL/pgSQL parser (most similar to PL/SQL, broadest
      open-source user base)
- [ ] Implement query extraction (`extract_queries`)
- [ ] Unit tests with real-world PL/pgSQL procedure corpus

### Phase 2: Anti-Pattern Detection

- [ ] Implement cursor loop detection (FOR...IN...LOOP with DML)
- [ ] Implement N+1 detection (DML inside loop body)
- [ ] Implement missing-index detection via index advisor integration
      (RFC 0021)
- [ ] Build anti-pattern catalog with severity levels and suggested
      rewrites
- [ ] Integration tests with known-bad procedure patterns

### Phase 3: Additional Dialect Parsers

- [ ] PL/SQL parser (Oracle): `EXCEPTION` blocks, `FORALL`,
      `BULK COLLECT`, `%TYPE`/`%ROWTYPE`, packages
- [ ] T-SQL parser (SQL Server): `DECLARE @var`, `TRY...CATCH`,
      `@@FETCH_STATUS`, `OUTPUT` clause, `EXEC`
- [ ] MySQL parser: `HANDLER FOR NOT FOUND`, `DELIMITER`, `LEAVE`/
      `ITERATE`, `SIGNAL`/`RESIGNAL`
- [ ] Cross-dialect test suite ensuring equivalent procedures produce
      equivalent `ProcedureStmt` ASTs

### Phase 4: Cost Model and Optimizer Integration

- [ ] Implement `ProcedureCostEstimator` with context-switch and
      cursor-overhead modeling
- [ ] Integrate with `ra-engine` optimizer for extracted query
      optimization
- [ ] Build `optimize_procedure` API that returns per-query
      recommendations
- [ ] Benchmark against real procedure workloads from TPC-C and
      TPC-E specifications
