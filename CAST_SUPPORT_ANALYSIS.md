# CAST Support Analysis & Implementation Plan

## Current State

**Parser:** ✅ sqlparser-rs already parses CAST expressions
**Ra Core:** ✅ `Expr::Cast { expr, target_type }` already defined
**E-graph:** ❌ Explicitly rejected with error message

```rust
// crates/ra-engine/src/egraph.rs line 2130
Expr::Cast { .. } => Err(EGraphError::ConversionError(
    "CAST expressions are not yet supported in the e-graph representation".into(),
))
```

## Database Support Matrix

### PostgreSQL ✅ Full Support
```sql
-- SQL standard syntax
CAST(column AS INTEGER)
CAST(column AS VARCHAR(50))
CAST(column AS TIMESTAMP)

-- PostgreSQL-specific shorthand (:: operator)
column::INTEGER
column::VARCHAR(50)
'[1,2,3]'::vector
'2024-01-01'::date
```

**Types:** All standard SQL types + extensions (jsonb, uuid, vector, etc.)

### MySQL ✅ Standard CAST
```sql
-- SQL standard syntax
CAST(column AS SIGNED)
CAST(column AS CHAR(50))
CAST(column AS DATETIME)

-- MySQL-specific CONVERT
CONVERT(column, SIGNED)
CONVERT(column USING utf8mb4)  -- Character set conversion
```

**Types:** Standard types (SIGNED, UNSIGNED, CHAR, DATE, TIME, DATETIME, DECIMAL, JSON)

### SQL Server ✅ CAST + CONVERT
```sql
-- SQL standard syntax
CAST(column AS INT)
CAST(column AS VARCHAR(50))
CAST(column AS DATETIME2)

-- SQL Server-specific CONVERT with style codes
CONVERT(VARCHAR(50), column)
CONVERT(VARCHAR(10), date_col, 120)  -- ISO format YYYY-MM-DD
```

**Types:** Full T-SQL type system

### Oracle ✅ CAST + TO_* Functions
```sql
-- SQL standard syntax
CAST(column AS NUMBER)
CAST(column AS VARCHAR2(50))

-- Oracle-specific conversion functions
TO_NUMBER(column)
TO_CHAR(column, 'format')
TO_DATE(column, 'format')
```

**Types:** Oracle type system (NUMBER, VARCHAR2, DATE, TIMESTAMP, etc.)

### SQLite ✅ Weak Typing
```sql
-- Accepts CAST but largely ignores it
CAST(column AS INTEGER)
CAST(column AS TEXT)

-- SQLite uses type affinity, not strict types
```

**Note:** SQLite's type system is permissive - CAST is mostly documentation

### DuckDB ✅ PostgreSQL-Compatible
```sql
-- SQL standard
CAST(column AS INTEGER)

-- PostgreSQL-style shorthand
column::INTEGER
column::DATE
```

**Types:** Rich type system including ARRAY, STRUCT, MAP

---

## Implementation Plan

### Step 1: Add Cast Operator to RelLang

**File:** `crates/ra-engine/src/egraph.rs` (add to define_language! macro)

```rust
define_language! {
    pub enum RelLang {
        // ... existing operators ...

        // Type casting operator
        // Children: [expr, target_type]
        "cast" = Cast([Id; 2]),
    }
}
```

### Step 2: Add E-graph Conversion

**File:** `crates/ra-engine/src/egraph.rs` (replace error with conversion)

```rust
Expr::Cast { expr, target_type } => {
    let expr_id = add_scalar_expr(rec, expr)?;
    let type_id = add_symbol(rec, target_type);
    Ok(rec.add(RelLang::Cast([expr_id, type_id])))
}
```

### Step 3: Add Extraction (Path 1: egraph.rs)

**File:** `crates/ra-engine/src/egraph.rs` in `scalar_from_node()`

```rust
RelLang::Cast([expr_id, type_id]) => {
    let expr = extract_scalar_expr(egraph, *expr_id)?;
    let target_type = extract_symbol(egraph, *type_id)?;
    Ok(Expr::Cast {
        expr: Box::new(expr),
        target_type,
    })
}
```

### Step 4: Add Extraction (Path 2: extract.rs)

**File:** `crates/ra-engine/src/extract.rs` in `convert_scalar_operator()`

```rust
if let RelLang::Cast([expr_id, type_id]) = node {
    let expr = convert_scalar(nodes, id(*expr_id))?;
    let target_type = get_symbol(nodes, id(*type_id))?;
    return Ok(Expr::Cast {
        expr: Box::new(expr),
        target_type,
    });
}
```

### Step 5: Add Optimization Rules

**File:** `crates/ra-engine/src/rewrite.rs` (add new function)

```rust
/// Cast optimization rules.
pub(crate) fn cast_optimization_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Remove redundant casts (cast to same type)
        rewrite!("remove-identity-cast";
            "(cast ?expr ?type)" => "?expr"
            if expr_has_type("?expr", "?type")
        ),

        // Fold constants through casts
        rewrite!("fold-cast-constant";
            "(cast (const-int ?val) int)" => "(const-int ?val)"
        ),
        rewrite!("fold-cast-string-to-int";
            "(cast (const-str ?val) int)" => "(const-int (parse-int ?val))"
            if can_parse_int("?val")
        ),

        // Push casts down through expressions (when safe)
        rewrite!("push-cast-through-add";
            "(cast (+ ?a ?b) ?type)" => "(+ (cast ?a ?type) (cast ?b ?type))"
            if is_numeric_type("?type")
        ),

        // Eliminate double casts
        rewrite!("eliminate-double-cast";
            "(cast (cast ?expr ?type1) ?type2)" => "(cast ?expr ?type2)"
        ),
    ]
}
```

Add to `all_rules_unsorted()`:
```rust
rules.extend(cast_optimization_rules());
```

### Step 6: Add Cost Model Entry

**File:** `crates/ra-engine/src/extract.rs` in `RelCostFn::cost()`

```rust
RelLang::Cast(_) => 0.01,  // Casts are cheap (usually free)
```

---

## Database-Specific Considerations

### PostgreSQL :: Operator

The parser already handles `::` as a type cast. No special handling needed in optimizer.

**Example:**
```sql
-- Parser converts this:
embedding::vector
-- To:
CAST(embedding AS vector)
-- Which becomes:
Expr::Cast { expr: embedding, target_type: "vector" }
```

### MySQL CONVERT()

Currently not supported - would need parser enhancement.

**Future work:** Add `Expr::Convert { expr, target_type, charset }` variant.

### SQL Server Style Codes

Currently not supported - style codes are lost.

**Future work:** Add optional `style` parameter to `Expr::Cast`.

### Oracle TO_* Functions

Parser treats these as regular functions. Could add optimization rules:

```rust
rewrite!("to-number-to-cast";
    "(func TO_NUMBER ?expr)" => "(cast ?expr number)"
);
```

---

## Testing Strategy

### Unit Tests

**File:** `crates/ra-engine/tests/cast_optimization_test.rs` (new file)

```rust
#[test]
fn test_cast_to_egraph_and_back() {
    let expr = Expr::Cast {
        expr: Box::new(Expr::Column(ColumnRef::new("age"))),
        target_type: "INTEGER".to_string(),
    };

    let rec = to_rec_expr(&expr).unwrap();
    let extracted = rec_expr_to_rel_expr(&rec).unwrap();

    // Should round-trip
    assert_eq!(extracted, expr);
}

#[test]
fn test_remove_redundant_cast() {
    let query = "SELECT CAST(id AS INTEGER) FROM items WHERE id = 1";
    let optimized = optimize_query(query);

    // Should eliminate CAST since id is already INTEGER
    assert!(!optimized.contains_cast());
}

#[test]
fn test_vector_cast_preserved() {
    let query = "SELECT * FROM items ORDER BY embedding::vector <-> '[1,2,3]' LIMIT 10";
    let optimized = optimize_query(query);

    // Should work now!
    assert!(optimized.is_ok());
}
```

### Integration Tests

Test with each database dialect:

```rust
#[test]
fn test_postgresql_double_colon() {
    let query = "SELECT col::integer FROM t";
    // Should parse and optimize
}

#[test]
fn test_mysql_cast() {
    let query = "SELECT CAST(col AS SIGNED) FROM t";
    // Should parse and optimize
}

#[test]
fn test_sqlserver_convert() {
    // Future: when CONVERT is supported
}
```

---

## Implementation Checklist

- [ ] Add `Cast([Id; 2])` to RelLang enum
- [ ] Add e-graph conversion (egraph.rs add_scalar_expr)
- [ ] Add extraction path 1 (egraph.rs scalar_from_node)
- [ ] Add extraction path 2 (extract.rs convert_scalar_operator)
- [ ] Add cost model entry (extract.rs RelCostFn)
- [ ] Add optimization rules (rewrite.rs cast_optimization_rules)
- [ ] Load rules in all_rules_unsorted()
- [ ] Add unit tests
- [ ] Add integration tests
- [ ] Test with PostgreSQL :: operator
- [ ] Test with MySQL CAST
- [ ] Update documentation

---

## Expected Impact

### Before
```
$ cargo run --bin ra-cli -- optimize "SELECT * FROM items ORDER BY embedding::vector <-> '[1,2,3]' LIMIT 10"
Error: CAST expressions are not yet supported in the e-graph representation
```

### After
```
$ cargo run --bin ra-cli -- optimize "SELECT * FROM items ORDER BY embedding::vector <-> '[1,2,3]' LIMIT 10"

Original Plan:
└─ Limit(count=10, offset=0)
   └─ Sort
      keys: VectorDistance {
        metric: "l2",
        column: Cast(embedding AS vector),  ← Cast preserved
        target: "[1,2,3]"
      } ASC
      └─ Scan(items)

Optimized Plan:
└─ Limit(count=10, offset=0)
   └─ Scan(items AS vector_knn_scan)  ← Rule matched despite Cast!
```

The optimizer can now:
1. Handle CAST expressions without failing
2. Optimize through CASTs when appropriate
3. Remove redundant CASTs
4. Support PostgreSQL's `::` syntax fully

---

## Related Issues

- Vector optimization blocked by CAST (#current)
- Project node blocking vector rules (#documented)
- CONVERT() not supported (#future)
- Oracle TO_* functions not recognized as casts (#future)

---

**Estimated Implementation Time:** 2-3 hours
**Priority:** High (blocks common PostgreSQL usage)
**Complexity:** Medium (requires 5-file change + tests)
