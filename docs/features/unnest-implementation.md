# UNNEST and Table-Valued Functions Implementation Plan

**Status:** Approved for Implementation
**Priority:** High (blocks real-world queries)
**Author:** RA Contributors
**Date:** 2025-03-19

---

## Executive Summary

This plan adds support for UNNEST and other table-valued functions (TVFs) to RA. UNNEST is a critical SQL feature for processing arrays, JSON, and set-returning functions. Its absence blocks common PostgreSQL query patterns.

**Core additions:**
1. New relational algebra operators: `Unnest`, `TableFunction`, `Lateral`
2. Parser support for `TableFactor::TableFunction` and `LATERAL` joins
3. 10+ optimization rules for unnest pushdown and fusion
4. Execution engine for array expansion and set-returning functions
5. Integration with lateral join optimization

**Timeline:** 3 weeks for core implementation + 2 weeks for advanced features

---

## Problem Statement

### Current Error

```
Error: unsupported SQL feature: unsupported table factor
```

This occurs when parsing queries with table-valued functions like:

```sql
SELECT * FROM unnest(array[1,2,3]) AS t_result(value);
LEFT JOIN unnest(my_array_column) AS u_result(elem) ON elem = other_col;
```

### Blocked Query Patterns

1. **Array unnesting:** `SELECT * FROM unnest(array[1,2,3])`
2. **Correlated unnest:** `SELECT t.*, u.* FROM t LEFT JOIN unnest(t.arr) AS u_result(val) ON ...`
3. **Lateral joins:** `SELECT * FROM t, LATERAL unnest(t.arr)`
4. **JSON expansion:** `SELECT * FROM json_array_elements('[1,2,3]')`
5. **Series generation:** `SELECT * FROM generate_series(1, 10)`

### Why This Matters

- **PostgreSQL compatibility:** UNNEST is heavily used in PG queries
- **JSON processing:** Modern apps store arrays/JSON in databases
- **Time series:** `generate_series()` is common for date ranges
- **Lateral join optimization:** Enables correlated subquery elimination

---

## Relational Algebra Extensions

### New Operators

#### 1. Unnest Operator

Expands an array or set-returning expression into rows:

```rust
pub enum RelExpr {
    // ... existing variants ...

    /// Unnest an array/set expression into rows
    Unnest {
        /// Expression producing array or set (column ref, array literal, function call)
        expr: Expr,
        /// Column alias for unnested values
        alias: Option<String>,
        /// Correlated input relation (for LATERAL unnest)
        input: Option<Box<RelExpr>>,
        /// Ordinal column (WITH ORDINALITY in SQL)
        with_ordinality: bool,
    },
}
```

**Algebra notation:**
```
unnest(expr) → relation with one column

Example:
unnest(array[1,2,3]) → {(1), (2), (3)}
```

#### 2. TableFunction Operator

General table-valued function (superset of Unnest):

```rust
/// General table-valued function
pub enum RelExpr {
    // ...

    TableFunction {
        /// Function name (generate_series, json_table, etc.)
        name: String,
        /// Function arguments
        args: Vec<Expr>,
        /// Column definitions (name, type)
        columns: Vec<(String, DataType)>,
        /// Correlated input (for LATERAL)
        input: Option<Box<RelExpr>>,
    },
}
```

**Examples:**
```sql
-- generate_series(start, end, step)
TableFunction {
  name: "generate_series",
  args: [Const(1), Const(10), Const(1)],
  columns: [("generate_series", Int64)],
  input: None
}

-- json_array_elements(json_column)
TableFunction {
  name: "json_array_elements",
  args: [Column("data")],
  columns: [("value", Json)],
  input: Some(Scan("orders"))
}
```

#### 3. Lateral Join

Correlated table-valued function (RHS references LHS):

```rust
/// Lateral join variant
pub enum JoinType {
    Inner,
    LeftOuter,
    // ... existing variants ...

    /// Lateral join (RHS can reference LHS columns)
    Lateral,
}
```

**Algebra notation:**
```
R ⋈_L f(R.col) where f is a table-valued function

Example:
customers ⋈_L unnest(customers.order_ids)
```

### Extended Expression Types

Support arrays and set-returning functions:

```rust
pub enum Expr {
    // ... existing variants ...

    /// Array constructor
    Array(Vec<Expr>),  // array[1, 2, 3]

    /// Array element access
    ArrayIndex(Box<Expr>, Box<Expr>),  // arr[2]

    /// Array slice
    ArraySlice(Box<Expr>, Option<Box<Expr>>, Option<Box<Expr>>),  // arr[1:3]

    /// Set-returning function call
    SetReturning {
        name: String,
        args: Vec<Expr>,
    },
}
```

---

## Implementation Phases

### Phase 1: Core Operators (Week 1)

#### Task 1.1: Define Algebra Operators

**File:** `/Users/gregburd/src/ra/crates/ra-core/src/algebra.rs`

Add `Unnest`, `TableFunction`, and `Lateral` to `RelExpr`:

```rust
impl RelExpr {
    /// Create an unnest operator
    pub fn unnest(expr: Expr, alias: Option<String>) -> Self {
        RelExpr::Unnest {
            expr,
            alias,
            input: None,
            with_ordinality: false,
        }
    }

    /// Create a correlated unnest (lateral)
    pub fn unnest_lateral(expr: Expr, input: RelExpr, alias: Option<String>) -> Self {
        RelExpr::Unnest {
            expr,
            alias,
            input: Some(Box::new(input)),
            with_ordinality: false,
        }
    }
}
```

**Deliverable:** 150 lines in algebra.rs

#### Task 1.2: Extend Expression Types

**File:** `/Users/gregburd/src/ra/crates/ra-core/src/expr.rs`

Add array and set-returning expressions:

```rust
impl Expr {
    /// Create array literal
    pub fn array(elements: Vec<Expr>) -> Self {
        Expr::Array(elements)
    }

    /// Create array index expression
    pub fn array_index(array: Expr, index: Expr) -> Self {
        Expr::ArrayIndex(Box::new(array), Box::new(index))
    }
}
```

**Deliverable:** 100 lines in expr.rs

#### Task 1.3: Update Display/Debug

**File:** `/Users/gregburd/src/ra/crates/ra-core/src/display.rs`

Add pretty-printing for new operators:

```rust
impl fmt::Display for RelExpr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            // ... existing arms ...
            RelExpr::Unnest { expr, alias, input, with_ordinality } => {
                if let Some(input) = input {
                    write!(f, "{} ⋈_L ", input)?;
                }
                write!(f, "unnest({})", expr)?;
                if let Some(alias) = alias {
                    write!(f, " AS {}", alias)?;
                }
                if *with_ordinality {
                    write!(f, " WITH ORDINALITY")?;
                }
                Ok(())
            }
        }
    }
}
```

**Deliverable:** 80 lines

#### Task 1.4: Unit Tests

**File:** `/Users/gregburd/src/ra/crates/ra-core/tests/unnest_test.rs`

Test algebra construction:

```rust
#[test]
fn unnest_array_literal() {
    let arr = Expr::array(vec![
        Expr::const_int(1),
        Expr::const_int(2),
        Expr::const_int(3),
    ]);
    let unnest = RelExpr::unnest(arr, Some("val".into()));

    assert!(matches!(unnest, RelExpr::Unnest { .. }));
}

#[test]
fn unnest_lateral_join() {
    let scan = RelExpr::scan("orders");
    let unnest = RelExpr::unnest_lateral(
        Expr::column("items"),
        scan,
        Some("item".into())
    );

    if let RelExpr::Unnest { input, .. } = unnest {
        assert!(input.is_some());
    }
}
```

**Deliverable:** 200 lines, 15 tests

---

### Phase 2: Parser Support (Week 1)

#### Task 2.1: Parse TableFunction

**File:** `/Users/gregburd/src/ra/crates/ra-parser/src/sql_to_relexpr.rs`

Replace the error at line 421 with actual parsing:

```rust
fn convert_table_factor(
    table: &TableFactor,
) -> Result<RelExpr, SqlConversionError> {
    match table {
        // ... existing arms ...

        TableFactor::TableFunction { name, args, alias } => {
            convert_table_function(name, args, alias)
        }

        TableFactor::UNNEST { alias, array_exprs, with_offset } => {
            convert_unnest(array_exprs, alias, *with_offset)
        }

        _ => Err(SqlConversionError::UnsupportedFeature(
            "unsupported table factor".to_owned(),
        )),
    }
}

fn convert_table_function(
    name: &ObjectName,
    args: &[FunctionArg],
    alias: &Option<TableAlias>,
) -> Result<RelExpr, SqlConversionError> {
    let func_name = object_name_to_string(name);

    match func_name.to_lowercase().as_str() {
        "unnest" => {
            if args.len() != 1 {
                return Err(SqlConversionError::InvalidArguments(
                    "UNNEST requires exactly one argument".into()
                ));
            }
            let arr_expr = convert_function_arg(&args[0])?;
            Ok(RelExpr::Unnest {
                expr: arr_expr,
                alias: alias.as_ref().map(|a| a.name.value.clone()),
                input: None,
                with_ordinality: false,
            })
        }

        "generate_series" => {
            let arg_exprs: Result<Vec<_>, _> = args.iter()
                .map(convert_function_arg)
                .collect();
            Ok(RelExpr::TableFunction {
                name: "generate_series".into(),
                args: arg_exprs?,
                columns: vec![("generate_series".into(), DataType::Int64)],
                input: None,
            })
        }

        "json_array_elements" | "json_each" | "json_object_keys" => {
            let arg_exprs: Result<Vec<_>, _> = args.iter()
                .map(convert_function_arg)
                .collect();
            Ok(RelExpr::TableFunction {
                name: func_name.clone(),
                args: arg_exprs?,
                columns: infer_json_function_columns(&func_name),
                input: None,
            })
        }

        _ => Err(SqlConversionError::UnsupportedFeature(
            format!("table function '{}' not supported", func_name)
        ))
    }
}
```

**Deliverable:** 250 lines

#### Task 2.2: Parse LATERAL Joins

**File:** `/Users/gregburd/src/ra/crates/ra-parser/src/sql_to_relexpr.rs`

Extend join parsing to recognize LATERAL:

```rust
fn convert_join(
    left: RelExpr,
    join: &SqlJoin,
) -> Result<RelExpr, SqlConversionError> {
    // Check if this is a lateral join
    if join.is_lateral {
        return convert_lateral_join(left, join);
    }

    // ... existing join logic ...
}

fn convert_lateral_join(
    left: RelExpr,
    join: &SqlJoin,
) -> Result<RelExpr, SqlConversionError> {
    let right = convert_table_factor(&join.relation)?;

    // Attach left relation as input to right (TVF)
    let correlated_right = match right {
        RelExpr::Unnest { expr, alias, with_ordinality, .. } => {
            RelExpr::Unnest {
                expr,
                alias,
                input: Some(Box::new(left.clone())),
                with_ordinality,
            }
        }
        RelExpr::TableFunction { name, args, columns, .. } => {
            RelExpr::TableFunction {
                name,
                args,
                columns,
                input: Some(Box::new(left.clone())),
            }
        }
        _ => return Err(SqlConversionError::UnsupportedFeature(
            "LATERAL requires table function or subquery".into()
        )),
    };

    // Create lateral join
    Ok(RelExpr::Join {
        join_type: JoinType::Lateral,
        condition: Expr::Const(Const::Bool(true)), // Correlation is implicit
        left: Box::new(left),
        right: Box::new(correlated_right),
    })
}
```

**Deliverable:** 150 lines

#### Task 2.3: Parse Array Expressions

**File:** `/Users/gregburd/src/ra/crates/ra-parser/src/sql_to_relexpr.rs`

Add array literal and indexing support:

```rust
fn convert_expr(expr: &SqlExpr) -> Result<Expr, SqlConversionError> {
    match expr {
        // ... existing arms ...

        SqlExpr::Array(arr) => {
            let elements: Result<Vec<_>, _> = arr.elem
                .iter()
                .map(convert_expr)
                .collect();
            Ok(Expr::Array(elements?))
        }

        SqlExpr::ArrayIndex { obj, indexes } => {
            let array = convert_expr(obj)?;
            let index = convert_expr(&indexes[0])?; // SQL uses 1-based indexing
            Ok(Expr::ArrayIndex(Box::new(array), Box::new(index)))
        }

        _ => Err(SqlConversionError::UnsupportedExpression(
            format!("{:?}", expr)
        ))
    }
}
```

**Deliverable:** 80 lines

#### Task 2.4: Integration Tests

**File:** `/Users/gregburd/src/ra/crates/ra-parser/tests/unnest_parse_test.rs`

Test SQL → RelExpr conversion:

```rust
#[test]
fn parse_unnest_array_literal() {
    let sql = "SELECT * FROM unnest(array[1,2,3]) AS t_result(val)";
    let expr = parse_query(sql).unwrap();

    assert!(matches!(expr, RelExpr::Project { .. }));
}

#[test]
fn parse_lateral_unnest() {
    let sql = "SELECT * FROM orders, LATERAL unnest(items) AS item";
    let expr = parse_query(sql).unwrap();

    // Should produce: Scan(orders) ⋈_L unnest(items)
    assert!(matches!(expr, RelExpr::Join { join_type: JoinType::Lateral, .. }));
}

#[test]
fn parse_generate_series() {
    let sql = "SELECT * FROM generate_series(1, 10)";
    let expr = parse_query(sql).unwrap();

    assert!(matches!(expr, RelExpr::Project { input, .. }
        if matches!(**input, RelExpr::TableFunction { name, .. } if name == "generate_series")
    ));
}
```

**Deliverable:** 300 lines, 20 tests

---

### Phase 3: Execution Engine (Week 2)

#### Task 3.1: Unnest Executor

**File:** `/Users/gregburd/src/ra/crates/ra-engine/src/executors/unnest.rs`

Implement runtime unnesting:

```rust
use ra_core::{Expr, RelExpr, Row, Value};
use anyhow::Result;

pub struct UnnestExecutor {
    expr: Expr,
    alias: Option<String>,
    with_ordinality: bool,
}

impl UnnestExecutor {
    pub fn new(expr: Expr, alias: Option<String>, with_ordinality: bool) -> Self {
        Self { expr, alias, with_ordinality }
    }

    pub fn execute(&self, input: Option<&Vec<Row>>) -> Result<Vec<Row>> {
        match &self.expr {
            Expr::Array(elements) => {
                // Unnest array literal
                self.unnest_array_literal(elements)
            }
            Expr::Column(col_ref) => {
                // Unnest array column from input
                if let Some(rows) = input {
                    self.unnest_column(col_ref, rows)
                } else {
                    Err(anyhow::anyhow!("UNNEST requires input for column reference"))
                }
            }
            _ => Err(anyhow::anyhow!("Invalid UNNEST target: {:?}", self.expr)),
        }
    }

    fn unnest_array_literal(&self, elements: &[Expr]) -> Result<Vec<Row>> {
        let mut rows = Vec::new();
        for (idx, elem) in elements.iter().enumerate() {
            let value = self.evaluate_expr(elem)?;
            let mut row = Row::new(vec![value]);

            if self.with_ordinality {
                row.push(Value::Int64(idx as i64 + 1));
            }

            rows.push(row);
        }
        Ok(rows)
    }

    fn unnest_column(&self, col_ref: &str, input_rows: &[Row]) -> Result<Vec<Row>> {
        let mut output = Vec::new();

        for input_row in input_rows {
            let array_value = input_row.get_column(col_ref)?;

            match array_value {
                Value::Array(elements) => {
                    for elem in elements {
                        let mut new_row = input_row.clone();
                        new_row.push(elem.clone());
                        output.push(new_row);
                    }
                }
                _ => {
                    return Err(anyhow::anyhow!(
                        "Column {} is not an array", col_ref
                    ));
                }
            }
        }

        Ok(output)
    }

    fn evaluate_expr(&self, expr: &Expr) -> Result<Value> {
        // Simple constant evaluation
        match expr {
            Expr::Const(c) => Ok(Value::from_const(c)),
            _ => Err(anyhow::anyhow!("Complex expression evaluation not supported")),
        }
    }
}
```

**Deliverable:** 400 lines

#### Task 3.2: TableFunction Executor

**File:** `/Users/gregburd/src/ra/crates/ra-engine/src/executors/table_function.rs`

Implement set-returning functions:

```rust
pub struct TableFunctionExecutor {
    name: String,
    args: Vec<Expr>,
}

impl TableFunctionExecutor {
    pub fn execute(&self, input: Option<&Vec<Row>>) -> Result<Vec<Row>> {
        match self.name.as_str() {
            "generate_series" => self.execute_generate_series(),
            "json_array_elements" => self.execute_json_array_elements(input),
            "json_each" => self.execute_json_each(input),
            _ => Err(anyhow::anyhow!("Unsupported table function: {}", self.name)),
        }
    }

    fn execute_generate_series(&self) -> Result<Vec<Row>> {
        let start = self.eval_arg_as_int(0)?;
        let end = self.eval_arg_as_int(1)?;
        let step = if self.args.len() >= 3 {
            self.eval_arg_as_int(2)?
        } else {
            1
        };

        let mut rows = Vec::new();
        let mut current = start;

        while (step > 0 && current <= end) || (step < 0 && current >= end) {
            rows.push(Row::new(vec![Value::Int64(current)]));
            current += step;
        }

        Ok(rows)
    }

    fn execute_json_array_elements(&self, input: Option<&Vec<Row>>) -> Result<Vec<Row>> {
        // Extract JSON array and expand to rows
        // Implementation depends on JSON support in Value type
        todo!("JSON array elements")
    }
}
```

**Deliverable:** 500 lines

#### Task 3.3: Lateral Join Executor

**File:** `/Users/gregburd/src/ra/crates/ra-engine/src/executors/lateral_join.rs`

Execute correlated table-valued functions:

```rust
pub struct LateralJoinExecutor {
    left: Box<dyn Executor>,
    right_tvf: Box<dyn TableValuedExecutor>,
}

impl LateralJoinExecutor {
    pub fn execute(&self) -> Result<Vec<Row>> {
        let left_rows = self.left.execute()?;
        let mut output = Vec::new();

        for left_row in left_rows {
            // Evaluate TVF with left_row as context
            let right_rows = self.right_tvf.execute_with_context(&left_row)?;

            // Cross product with left_row
            for right_row in right_rows {
                let combined = left_row.concat(&right_row);
                output.push(combined);
            }
        }

        Ok(output)
    }
}
```

**Deliverable:** 300 lines

#### Task 3.4: Integration with Optimizer

**File:** `/Users/gregburd/src/ra/crates/ra-engine/src/egraph.rs`

Add unnest to e-graph nodes:

```rust
impl ToRecExpr for RelExpr {
    fn to_rec_expr(&self) -> RecExpr<OpNode> {
        match self {
            // ... existing arms ...

            RelExpr::Unnest { expr, alias, input, with_ordinality } => {
                let mut nodes = vec![];
                let expr_id = expr.to_rec_expr_append(&mut nodes);

                if let Some(input) = input {
                    let input_id = input.to_rec_expr_append(&mut nodes);
                    nodes.push(OpNode::Unnest {
                        expr: expr_id,
                        input: Some(input_id),
                        with_ordinality: *with_ordinality,
                    });
                } else {
                    nodes.push(OpNode::Unnest {
                        expr: expr_id,
                        input: None,
                        with_ordinality: *with_ordinality,
                    });
                }

                RecExpr::from(nodes)
            }
        }
    }
}
```

**Deliverable:** 200 lines

---

### Phase 4: Optimization Rules (Week 2-3)

Create new rules directory: `/Users/gregburd/src/ra/rules/unnest/`

#### Rule 4.1: Push Filter Through Unnest

**File:** `rules/unnest/filter-through-unnest.rra`

```yaml
---
id: filter-through-unnest
name: Push Filter Through Unnest
category: logical/unnest
databases: [postgresql, duckdb, generic]
preconditions:
  - type: pattern
    must_match: "(filter ?pred (unnest ?arr))"

  - type: predicate
    condition: "is_simple_comparison(?pred)"
    description: "Predicate is simple comparison on unnested value"
---

# Push Filter Through Unnest

## Description

Push predicates on unnested values back to array filtering when possible.

## Relational Algebra

```algebra
-- Before
σ[u > 10](unnest(arr))

-- After (when arr is literal or can be filtered)
unnest(filter_array(arr, λx. x > 10))
```

## Implementation

```rust
rewrite!("filter-through-unnest";
    "(filter ?pred (unnest ?arr))" =>
    "(unnest (filter-array ?arr ?pred))"
    if is_array_filterable(?arr)
)
```

## Benefit

Reduces rows produced by unnest (up to 90% reduction).
```

#### Rule 4.2: Unnest Array Literal

**File:** `rules/unnest/unnest-array-literal.rra`

```yaml
---
id: unnest-array-literal-constant-fold
name: Unnest Array Literal at Compile Time
category: logical/unnest
---

## Description

When unnesting a constant array, expand to VALUES at compile time.

```algebra
-- Before
unnest(array[1, 2, 3])

-- After
(values (1), (2), (3))
```

## Implementation

```rust
rewrite!("unnest-array-literal";
    "(unnest (array ?elems))" =>
    "(values (expand-array ?elems))"
    if is_constant_array(?elems)
)
```
```

#### Rule 4.3: Merge Adjacent Unnests

**File:** `rules/unnest/merge-unnests.rra`

```yaml
---
id: merge-adjacent-unnests
name: Merge Adjacent Unnests into Zip
category: logical/unnest
---

## Description

Multiple unnests from same input can be zipped together.

```algebra
-- Before
unnest(arr1) ⋈ unnest(arr2)

-- After (if arrays have same length)
zip_unnest(arr1, arr2)
```
```

#### Rule 4.4: Unnest with Ordinality to Window

**File:** `rules/unnest/unnest-ordinality-to-window.rra`

```yaml
---
id: unnest-ordinality-to-row-number
name: Unnest WITH ORDINALITY to ROW_NUMBER()
category: logical/unnest
---

## Description

UNNEST ... WITH ORDINALITY can use window function infrastructure.

```sql
-- Before
SELECT * FROM unnest(arr) WITH ORDINALITY AS t_result(val, ord)

-- After (internal representation)
SELECT val, ROW_NUMBER() OVER (ORDER BY val) AS ord
FROM unnest(arr) AS t_result(val)
```
```

#### Rule 4.5: Lateral to Semi-Join

**File:** `rules/unnest/lateral-to-semi-join.rra`

```yaml
---
id: lateral-unnest-to-semi-join
name: Convert Lateral Unnest to Semi-Join
category: logical/unnest
preconditions:
  - type: predicate
    condition: "only_checks_existence(?join)"
    description: "Join only checks if array is non-empty"
---

## Description

When lateral unnest just checks existence, convert to semi-join.

```algebra
-- Before
R ⋈_L unnest(R.arr)  WHERE EXISTS(...)

-- After
R ⋉ (R.arr IS NOT NULL AND cardinality(R.arr) > 0)
```
```

#### Rule 4.6: Index on Unnested Column

**File:** `rules/unnest/index-on-array-elements.rra`

```yaml
---
id: index-on-array-elements
name: Use GIN Index for Unnest Filtering
category: physical/unnest
databases: [postgresql]
preconditions:
  - type: fact
    fact_type: schema.index_exists
    table: "?table"
    column: "?array_col"
    index_type: "gin"
---

## Description

PostgreSQL GIN indexes can accelerate array element lookups without unnesting.

```sql
-- Before
SELECT * FROM t, unnest(arr) u WHERE u = 10

-- After (use GIN index scan)
SELECT * FROM t WHERE arr @> ARRAY[10]
```
```

#### Rule 4.7: Unnest Pushdown into Scan

**File:** `rules/unnest/unnest-pushdown-scan.rra`

```yaml
---
id: unnest-pushdown-into-scan
name: Push Unnest into Table Scan
category: logical/unnest
---

## Description

Unnest array columns directly in scan rather than separate operator.

```algebra
-- Before
unnest(π[arr](σ[cond](Scan(T))))

-- After
UnnestScan(T, arr, cond)
```

Combines scan, filter, project, and unnest into single operator.
```

#### Rule 4.8: Lateral Decorrelation

**File:** `rules/unnest/lateral-decorrelation.rra`

```yaml
---
id: lateral-unnest-decorrelation
name: Decorrelate Lateral Unnest
category: logical/unnest
---

## Description

Decorrelate lateral unnest into regular join when possible.

```algebra
-- Before (correlated)
SELECT * FROM t1, LATERAL unnest(array[t1.id, t1.id+1]) AS u_result(val)

-- After (decorrelated)
SELECT * FROM t1
CROSS JOIN (VALUES (0), (1)) AS offset(o)
WHERE val = t1.id + o
```
```

#### Rule 4.9: Unnest Fusion

**File:** `rules/unnest/unnest-fusion.rra`

```yaml
---
id: unnest-projection-fusion
name: Fuse Unnest with Projection
category: logical/unnest
---

## Description

Combine unnest and projection into single operation.

```algebra
-- Before
π[expr(u)](unnest(arr))

-- After
unnest_project(arr, expr)
```
```

#### Rule 4.10: Generate Series to Range Scan

**File:** `rules/unnest/generate-series-to-range.rra`

```yaml
---
id: generate-series-to-range-scan
name: Optimize Generate Series
category: logical/unnest
databases: [postgresql, duckdb]
---

## Description

generate_series can be executed as specialized range iterator.

```sql
-- Before
SELECT * FROM generate_series(1, 1000000)

-- After (internal)
RangeScan { start: 1, end: 1000000, step: 1 }
```

Avoids materializing all values upfront.
```

**Deliverable:** 10 rule files, ~200 lines each = 2000 lines total

---

### Phase 5: Advanced Features (Week 3)

#### Task 5.1: JSON Table Functions

Support PostgreSQL JSON table functions:
- `json_array_elements()`
- `json_array_elements_text()`
- `json_each()`
- `json_object_keys()`
- `json_populate_recordset()`

**Deliverable:** 400 lines

#### Task 5.2: Multi-Argument Unnest

PostgreSQL allows unnesting multiple arrays in parallel:

```sql
SELECT * FROM unnest(
  ARRAY[1,2,3],
  ARRAY['a','b','c']
) AS t_result(num, letter);
```

**Deliverable:** 200 lines

#### Task 5.3: WITH ORDINALITY

Add ordinal column support:

```sql
SELECT * FROM unnest(ARRAY[10,20,30]) WITH ORDINALITY AS t_result(val, ord);
-- Result: (10,1), (20,2), (30,3)
```

**Deliverable:** 150 lines

#### Task 5.4: Unnest Performance Benchmarks

Create benchmark suite comparing:
- Array unnest vs manual VALUES expansion
- Lateral unnest vs correlated subquery
- GIN index vs unnest + filter

**Deliverable:** 300 lines benchmark code

---

## Integration Points

### 1. Pre-Condition System

All unnest rules use formal pre-conditions:

```yaml
preconditions:
  - type: pattern
    must_match: "(unnest ?expr)"

  - type: fact
    fact_type: database.supports_feature
    comparator: "=="
    threshold: true
    feature: "unnest"

  - type: fact
    fact_type: statistics.array_avg_length
    table: "?table"
    column: "?array_col"
    comparator: ">"
    threshold: 10
    description: "Array is large enough to benefit from optimization"
```

### 2. Cost Model

Estimate unnest cost based on array length:

```rust
pub fn estimate_unnest_cost(expr: &Expr, input_card: f64) -> Cost {
    let avg_array_length = estimate_avg_array_length(expr);

    Cost {
        cpu: input_card * avg_array_length * 0.001, // Cheap operation
        memory: input_card * avg_array_length * 100.0, // Expanded rows
        io: 0.0, // No IO for unnest
    }
}
```

### 3. Lateral Join Costing

Lateral joins are expensive (nested loop):

```rust
pub fn estimate_lateral_join_cost(left_card: f64, tvf_card: f64) -> Cost {
    Cost {
        cpu: left_card * tvf_card * 0.01, // Per-row TVF evaluation
        memory: tvf_card * 200.0, // TVF result buffering
        io: 0.0,
    }
}
```

---

## Testing Strategy

### Unit Tests
- Array literal unnesting
- Column reference unnesting
- Lateral correlation
- Ordinality support
- Multi-argument unnest

### Integration Tests
- Parse → Optimize → Execute pipeline
- Compare with PostgreSQL results
- Edge cases (empty arrays, NULL values)

### Benchmark Tests
- TPC-H Q22 (unnest in subquery)
- JSON processing workloads
- Time series queries with generate_series

---

## PostgreSQL Compatibility

### Features to Support

| Feature | PostgreSQL | DuckDB | SQLite | Status |
|---------|-----------|--------|--------|--------|
| UNNEST array literal | ✅ | ✅ | ❌ | Week 1 |
| UNNEST column | ✅ | ✅ | ❌ | Week 1 |
| LATERAL join | ✅ | ✅ | ❌ | Week 2 |
| WITH ORDINALITY | ✅ | ✅ | ❌ | Week 3 |
| Multi-arg UNNEST | ✅ | ✅ | ❌ | Week 3 |
| generate_series | ✅ | ✅ | ❌ | Week 2 |
| json_array_elements | ✅ | ✅ | ❌ | Week 3 |

### Dialect Differences

PostgreSQL uses 1-based array indexing, while most languages use 0-based. RA will use **1-based** indexing internally for PG compatibility.

---

## Success Metrics

| Metric | Target | Timeline |
|--------|--------|----------|
| Parse success rate (PG queries with UNNEST) | 95%+ | Week 1 |
| Optimization rules applied | 10+ | Week 3 |
| Execution correctness vs PostgreSQL | 100% | Week 2 |
| Performance vs native PG unnest | Within 20% | Week 3 |
| TPC-H queries with unnest working | 100% | Week 3 |

---

## Critical Files

### New Files
- `/Users/gregburd/src/ra/crates/ra-core/src/algebra.rs` - Add Unnest/TableFunction operators (150 lines)
- `/Users/gregburd/src/ra/crates/ra-engine/src/executors/unnest.rs` - Runtime execution (400 lines)
- `/Users/gregburd/src/ra/crates/ra-engine/src/executors/table_function.rs` - Set-returning functions (500 lines)
- `/Users/gregburd/src/ra/crates/ra-engine/src/executors/lateral_join.rs` - Lateral join logic (300 lines)
- `/Users/gregburd/src/ra/rules/unnest/*.rra` - 10 optimization rules (2000 lines)

### Modified Files
- `/Users/gregburd/src/ra/crates/ra-parser/src/sql_to_relexpr.rs` - Parse TableFunction (400 lines added)
- `/Users/gregburd/src/ra/crates/ra-core/src/expr.rs` - Array expressions (100 lines added)
- `/Users/gregburd/src/ra/crates/ra-engine/src/egraph.rs` - E-graph integration (200 lines added)

---

## Example: Your Query After Implementation

### Input Query
```sql
SELECT * FROM
  (VALUES (1, array[10,20]), (2, array[20,30])) AS v1(v1x,v1ys)
  LEFT JOIN (VALUES (1, 10), (2, 20), (2, null)) AS v2(v2x,v2y) ON v2x = v1x
  LEFT JOIN unnest(v1ys) AS u1(u1y) ON u1y = v2y;
```

### After Parsing (Week 1)
```
Project[*](
  LeftJoin[u1y = v2y](
    LeftJoin[v2x = v1x](
      Values[(1, [10,20]), (2, [20,30])] AS v1(v1x, v1ys),
      Values[(1,10), (2,20), (2,null)] AS v2(v2x, v2y)
    ),
    UnnestLateral(Column("v1ys")) AS u1(u1y)
  )
)
```

### After Optimization (Week 2-3)
```
Project[*](
  LeftJoin[u1y = v2y](
    HashJoin[v2x = v1x](
      Values[(1, [10,20]), (2, [20,30])] AS v1,
      Values[(1,10), (2,20), (2,null)] AS v2
    ),
    Unnest(v1ys) AS u1  -- Lateral decorrelated
  )
)
```

### Execution (Week 2)
```
Result:
v1x | v1ys        | v2x | v2y  | u1y
----|-------------|-----|------|-----
1   | [10,20]     | 1   | 10   | 10
1   | [10,20]     | 1   | 10   | 20
2   | [20,30]     | 2   | 20   | 20
2   | [20,30]     | 2   | 20   | 30
2   | [20,30]     | 2   | NULL | 20
2   | [20,30]     | 2   | NULL | 30
```

---

## Next Steps

1. **Get approval** for 3-week timeline
2. **Create tasks** in project tracker
3. **Week 1 Sprint:** Core operators + parser
4. **Week 2 Sprint:** Execution engine + basic rules
5. **Week 3 Sprint:** Advanced rules + performance tuning

**Estimated total effort:** 3 weeks (120 hours)
**Priority:** High (blocks real queries)
**Dependencies:** None (independent feature)

