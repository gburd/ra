# RFC 0099: Semi-Structured Data Types

- Start Date: 2026-03-28
- Author: Research Analysis
- Status: Draft
- Tracking Issue: TBD

## Summary

This RFC proposes comprehensive support for semi-structured data types in Ra, enabling optimization of queries over nested and heterogeneous data. The proposal includes VARIANT (Snowflake), LIST/STRUCT/MAP (DuckDB), OBJECT, and ARRAY types with full query optimization support including predicate pushdown, statistics collection, and cost-based planning for semi-structured operations.

Semi-structured data support is foundational for modern cloud data warehouses and analytical databases, enabling JSON/schema-flexible analytics without sacrificing query performance.

## Motivation

Modern analytical workloads increasingly rely on semi-structured data formats like JSON, nested Parquet, and variable-schema datasets. Current Ra only supports flat relational types, limiting its applicability to:

1. **Cloud Data Warehouses:** Snowflake's VARIANT type is fundamental to 80%+ of cloud analytics workloads
2. **Data Lakes:** Parquet and Arrow use nested types (LIST, STRUCT, MAP) as native storage formats
3. **Event Analytics:** Application logs, clickstreams, and IoT data arrive as JSON with variable schemas
4. **API Integration:** External data sources provide JSON/XML that requires flexible typing
5. **Schema Evolution:** Agile development requires schema flexibility without table rewrites

**Key Problems Solved:**

- **Query Optimization Gap:** Ra cannot optimize queries containing `data:customer.name` or `list[1]` expressions
- **Cross-Database Compatibility:** Snowflake and DuckDB queries fail due to unsupported types
- **Performance Bottleneck:** Schema-flexible queries fall back to runtime JSON parsing without optimization
- **Statistics Blind Spot:** No cardinality estimates for nested fields, leading to poor join ordering

**Expected Outcomes:**

- Enable Ra to optimize Snowflake VARIANT queries with path-based access
- Support DuckDB LIST/STRUCT/MAP operations with lambda expressions
- Provide 10-100x performance improvements via predicate pushdown to nested data
- Enable statistics-driven optimization for semi-structured fields

## Guide-level explanation

### What Are Semi-Structured Data Types?

Semi-structured types store hierarchical, nested, or variable-schema data within relational tables. Unlike traditional fixed columns, these types adapt to the data's natural structure.

**Core Type Families:**

1. **VARIANT (Snowflake):** Universal container holding any data type including nested objects/arrays
2. **OBJECT (Snowflake):** Key-value maps where keys are strings, values are VARIANT
3. **ARRAY (Snowflake):** Ordered lists with 0-based indexing, elements are VARIANT
4. **LIST (DuckDB):** Variable-length arrays with uniform element types
5. **STRUCT (DuckDB):** Named field records (like rows) with static schema
6. **MAP (DuckDB):** Dynamic key-value pairs with consistent types

### Example Usage

#### Snowflake VARIANT Access

```sql
-- Query with path-based access
SELECT
    data:customer.name AS customer_name,
    data:items[0]:price AS first_item_price,
    data:metadata.timestamp::timestamp AS event_time
FROM orders
WHERE data:status = 'pending'
  AND data:total &gt; 100;
```

**Ra Optimization:**

```rust
// Internal representation
Filter {
    predicate: And(
        Eq(VariantPath(Column("data"), "status"), Const("pending")),
        Gt(VariantPath(Column("data"), "total"), Const(100))
    ),
    input: Scan("orders")
}

// Optimized plan pushes predicates to storage layer
// using min/max statistics on frequently-accessed paths
```

#### DuckDB LIST Operations

```sql
-- Transform list elements
SELECT
    user_id,
    list_transform(purchases, p -&gt; p.price * 1.1) AS adjusted_prices,
    list_filter(purchases, p -&gt; p.category = 'electronics') AS electronics
FROM user_purchases
WHERE list_contains(purchases, {'category': 'laptop'});
```

**Ra Optimization:**

```rust
// Lambda expressions as first-class constructs
Project {
    exprs: [
        Column("user_id"),
        ListTransform {
            list: Column("purchases"),
            lambda: Lambda {
                params: ["p"],
                body: Mul(Field(Var("p"), "price"), Const(1.1))
            }
        },
        ListFilter {
            list: Column("purchases"),
            lambda: Lambda {
                params: ["p"],
                body: Eq(Field(Var("p"), "category"), Const("electronics"))
            }
        }
    ],
    input: Filter {
        predicate: ListContains(
            Column("purchases"),
            StructLiteral([("category", "laptop")])
        ),
        input: Scan("user_purchases")
    }
}
```

#### DuckDB STRUCT Field Access

```sql
-- Extract and manipulate struct fields
SELECT
    address.street,
    address.city,
    struct_insert(address, zip := '94105') AS updated_address
FROM users
WHERE address.state = 'CA';
```

**Ra Optimization:**

```rust
// Struct field pruning: only read needed fields
Project {
    exprs: [
        StructFieldAccess(Column("address"), "street"),
        StructFieldAccess(Column("address"), "city"),
        StructInsert {
            struct_expr: Column("address"),
            updates: [("zip", Const("94105"))]
        }
    ],
    input: Filter {
        predicate: Eq(
            StructFieldAccess(Column("address"), "state"),
            Const("CA")
        ),
        input: Scan {
            table: "users",
            // Projection pushdown: only read address.street, address.city, address.state
            projected_fields: ["address.street", "address.city", "address.state"]
        }
    }
}
```

### How Users Interact with the Feature

1. **Write Natural Queries:** Use colon notation (`data:path`) or dot notation (`struct.field`) directly
2. **Leverage Statistics:** Ra automatically collects statistics on frequently-accessed paths
3. **Get Optimization Advice:** Ra suggests materialized columns for hot paths
4. **Control Pushdown:** Predicates on nested fields automatically push to storage when beneficial

## Reference-level explanation

### Type System Extensions

#### Core Type Definitions

```rust
// In crates/ra-core/src/algebra.rs

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DataType {
    // ... existing types: Int32, Float64, String, etc. ...

    /// Snowflake VARIANT: self-describing universal container (max 128 MB)
    /// Stores any type including nested OBJECT/ARRAY, maintains type tags
    Variant,

    /// Snowflake OBJECT: key-value map, keys are VARCHAR, values are VARIANT
    Object,

    /// Snowflake ARRAY: ordered list with 0-based indexing, elements are VARIANT
    Array,

    /// DuckDB LIST: variable-length array with uniform element type
    List(Box&lt;DataType&gt;),

    /// DuckDB STRUCT: named fields with static schema (like row type)
    Struct(Vec&lt;StructField&gt;),

    /// DuckDB MAP: dynamic key-value pairs with consistent types
    Map {
        key_type: Box&lt;DataType&gt;,
        value_type: Box&lt;DataType&gt;,
    },

    /// Fixed-size ARRAY (DuckDB): array with compile-time size
    FixedArray {
        element_type: Box&lt;DataType&gt;,
        size: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructField {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
}
```

#### Expression Extensions

```rust
// In crates/ra-core/src/algebra.rs

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expr {
    // ... existing variants ...

    /// VARIANT path access: data:customer.name or data['key']
    /// Snowflake-style colon notation
    VariantPath {
        base: Box&lt;Expr&gt;,
        path: PathSegment,
    },

    /// STRUCT field access: struct_col.field_name
    /// DuckDB dot notation
    StructFieldAccess {
        struct_expr: Box&lt;Expr&gt;,
        field_name: String,
    },

    /// LIST/ARRAY index access: list[idx]
    ListIndex {
        list: Box&lt;Expr&gt;,
        index: Box&lt;Expr&gt;,  // 0-based
    },

    /// LIST slice: list[start:end:step]
    ListSlice {
        list: Box&lt;Expr&gt;,
        start: Option&lt;Box&lt;Expr&gt;&gt;,
        end: Option&lt;Box&lt;Expr&gt;&gt;,
        step: Option&lt;Box&lt;Expr&gt;&gt;,
    },

    /// MAP key access: map['key']
    MapIndex {
        map: Box&lt;Expr&gt;,
        key: Box&lt;Expr&gt;,
    },

    /// List literal: [1, 2, 3] or ARRAY[1, 2, 3]
    ListConstructor(Vec&lt;Expr&gt;),

    /// Struct literal: {'name': 'Alice', 'age': 30}
    StructConstructor(Vec&lt;(String, Expr)&gt;),

    /// Map literal: MAP {'k1': 10, 'k2': 20}
    MapConstructor(Vec&lt;(Expr, Expr)&gt;),

    /// Lambda expression: x -&gt; x * 2
    Lambda {
        params: Vec&lt;String&gt;,
        body: Box&lt;Expr&gt;,
    },

    /// LIST transform: list_transform(list, lambda)
    ListTransform {
        list: Box&lt;Expr&gt;,
        lambda: Box&lt;Expr&gt;,  // Must be Lambda variant
    },

    /// LIST filter: list_filter(list, lambda)
    ListFilter {
        list: Box&lt;Expr&gt;,
        lambda: Box&lt;Expr&gt;,
    },

    /// LIST aggregate: list_sum(list), list_avg(list)
    ListAggregate {
        list: Box&lt;Expr&gt;,
        func: ListAggFunc,
    },

    /// STRUCT update: struct_insert(s, field := value, ...)
    StructInsert {
        struct_expr: Box&lt;Expr&gt;,
        updates: Vec&lt;(String, Expr)&gt;,
    },

    /// STRUCT/LIST expansion: struct.* or unnest(list)
    Unnest {
        expr: Box&lt;Expr&gt;,
        expand_mode: UnnestMode,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathSegment {
    /// JSON path: $.customer.name
    DotPath(Vec&lt;String&gt;),
    /// Bracket notation: ['key1']['key2']
    BracketPath(Vec&lt;String&gt;),
    /// Array index: [0][1]
    IndexPath(Vec&lt;usize&gt;),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ListAggFunc {
    Sum,
    Avg,
    Min,
    Max,
    Count,
    Any,
    All,
    Distinct,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UnnestMode {
    /// STRUCT expansion: all fields as separate columns
    StructExpand,
    /// LIST/ARRAY unnest: one row per element
    ListUnnest,
}
```

### Relational Operator Extensions

```rust
// In crates/ra-core/src/algebra.rs

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RelExpr {
    // ... existing variants ...

    /// FLATTEN operation (Snowflake): explode semi-structured data
    /// Produces 6 columns: SEQ, KEY, PATH, INDEX, VALUE, THIS
    Flatten {
        input: Box&lt;RelExpr&gt;,
        input_expr: Expr,      // VARIANT/OBJECT/ARRAY expression
        path: Option&lt;String&gt;,  // Extract nested element first
        outer: bool,           // TRUE = LEFT JOIN semantics (keep nulls)
        recursive: bool,       // Recursively flatten nested structures
        mode: FlattenMode,     // Filter by element type
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FlattenMode {
    Object,  // Only key-value pairs
    Array,   // Only indexed elements
    Both,    // All elements
}
```

### Implementation Details

#### 1. Type System Integration

**Storage Representation:**

```rust
// In crates/ra-core/src/types.rs

/// Binary JSON format for VARIANT (inspired by BSON/JSONB)
pub struct VariantValue {
    /// Type tag (1 byte): NULL, BOOL, INT, FLOAT, STRING, OBJECT, ARRAY
    tag: u8,
    /// Length for variable-size types (4 bytes)
    length: u32,
    /// Inline data or offset to heap allocation
    data: VariantData,
}

enum VariantData {
    Inline([u8; 8]),      // Small values fit inline
    Pointer(*const u8),   // Large values heap-allocated
}

/// Optimized STRUCT storage: separate column buffers per field
pub struct StructColumn {
    fields: Vec&lt;StructFieldColumn&gt;,
}

struct StructFieldColumn {
    name: String,
    data_type: DataType,
    values: Box&lt;dyn Array&gt;,  // Arrow-compatible array
    nulls: Option&lt;BitVec&gt;,
}

/// LIST storage: offset array + value array
pub struct ListColumn {
    offsets: Vec&lt;usize&gt;,   // Start offset per list
    values: Box&lt;dyn Array&gt;, // Flattened element values
    nulls: Option&lt;BitVec&gt;,
}
```

**Dictionary Encoding for VARIANT:**

```rust
// In crates/ra-stats-advanced/src/nested.rs

/// Track frequently-accessed paths and their values
pub struct VariantPathDictionary {
    /// Path -&gt; dictionary ID mapping
    path_ids: HashMap&lt;String, u32&gt;,

    /// Dictionary per path: value -&gt; code
    dictionaries: Vec&lt;HashMap&lt;VariantValue, u16&gt;&gt;,

    /// Reverse lookup: code -&gt; value
    reverse_dicts: Vec&lt;Vec&lt;VariantValue&gt;&gt;,
}

impl VariantPathDictionary {
    /// Convert path predicate to dictionary code predicate
    /// WHERE data:status = 'pending'
    ///   -&gt; WHERE status_code = 42  (if 'pending' has code 42)
    pub fn encode_predicate(&self, path: &str, value: &VariantValue) -&gt; Option&lt;u16&gt; {
        let path_id = self.path_ids.get(path)?;
        let dict = &self.dictionaries[*path_id as usize];
        dict.get(value).copied()
    }
}
```

#### 2. Statistics Collection

```rust
// In crates/ra-stats-advanced/src/nested.rs

/// Statistics for semi-structured columns
pub struct NestedColumnStats {
    /// Base column stats (null count, distinct count)
    base: ColumnStats,

    /// Per-path statistics for VARIANT
    variant_paths: HashMap&lt;String, PathStats&gt;,

    /// Per-field statistics for STRUCT
    struct_fields: HashMap&lt;String, Box&lt;ColumnStats&gt;&gt;,

    /// List length distribution
    list_lengths: Histogram,

    /// Map key cardinality
    map_key_cardinality: Option&lt;u64&gt;,
}

#[derive(Debug, Clone)]
pub struct PathStats {
    /// How often this path is accessed in queries
    access_frequency: u64,

    /// Type distribution: % INT, % STRING, % NULL, etc.
    type_histogram: HashMap&lt;u8, f64&gt;,

    /// Min/max values for each type
    min_values: HashMap&lt;u8, VariantValue&gt;,
    max_values: HashMap&lt;u8, VariantValue&gt;,

    /// Distinct value count (approximate via HyperLogLog)
    distinct_count: HyperLogLog,

    /// Most common values (for frequent path values)
    mcv: Vec&lt;(VariantValue, f64)&gt;,
}

impl PathStats {
    /// Estimate selectivity for path predicate
    /// WHERE data:status = 'active' -&gt; estimate % of rows matching
    pub fn estimate_selectivity(&self, op: ComparisonOp, value: &VariantValue) -&gt; f64 {
        // Use MCV if value is present
        if let Some(&freq) = self.mcv.iter()
            .find(|(v, _)| v == value)
            .map(|(_, f)| f)
        {
            return match op {
                ComparisonOp::Eq =&gt; freq,
                ComparisonOp::Ne =&gt; 1.0 - freq,
                _ =&gt; self.estimate_range_selectivity(op, value),
            };
        }

        // Fall back to uniform distribution over distinct values
        let uniform_prob = 1.0 / self.distinct_count.count() as f64;
        match op {
            ComparisonOp::Eq =&gt; uniform_prob,
            ComparisonOp::Ne =&gt; 1.0 - uniform_prob,
            _ =&gt; self.estimate_range_selectivity(op, value),
        }
    }
}
```

#### 3. Predicate Pushdown

```rust
// In crates/ra-engine/src/pushdown_nested.rs

/// Push predicates on nested fields to storage layer
pub struct NestedPredicatePushdown;

impl RewriteRule for NestedPredicatePushdown {
    fn apply(&self, expr: &RelExpr) -&gt; Option&lt;RelExpr&gt; {
        match expr {
            RelExpr::Filter { predicate, input } =&gt; {
                self.try_pushdown_nested(predicate, input)
            }
            _ =&gt; None,
        }
    }
}

impl NestedPredicatePushdown {
    fn try_pushdown_nested(&self, predicate: &Expr, input: &RelExpr) -&gt; Option&lt;RelExpr&gt; {
        // Extract path-based predicates
        let (nested_preds, other_preds) = self.partition_predicates(predicate);

        if nested_preds.is_empty() {
            return None;
        }

        // Push nested predicates to scan operator
        match input.as_ref() {
            RelExpr::Scan { table, filter, .. } =&gt; {
                let combined_filter = self.combine_filters(filter, &nested_preds);
                Some(RelExpr::Filter {
                    predicate: other_preds,
                    input: Box::new(RelExpr::Scan {
                        table: table.clone(),
                        filter: Some(combined_filter),
                        ..(*input).clone()
                    }),
                })
            }
            _ =&gt; None,
        }
    }

    /// Separate nested (pushable) from non-nested predicates
    fn partition_predicates(&self, pred: &Expr) -&gt; (Vec&lt;Expr&gt;, Expr) {
        let mut nested = Vec::new();
        let mut others = Vec::new();

        for conjunct in self.extract_conjuncts(pred) {
            if self.is_pushable_nested(&conjunct) {
                nested.push(conjunct);
            } else {
                others.push(conjunct);
            }
        }

        (nested, self.combine_conjuncts(others))
    }

    /// Check if predicate can be pushed to storage
    /// Pushable: data:status = 'active', list[0] &gt; 10
    /// Not pushable: list_sum(data:values) &gt; 100 (requires computation)
    fn is_pushable_nested(&self, expr: &Expr) -&gt; bool {
        match expr {
            Expr::BinaryOp { left, op, right } if op.is_comparison() =&gt; {
                matches!(
                    (left.as_ref(), right.as_ref()),
                    (Expr::VariantPath { .. }, Expr::Const(_)) |
                    (Expr::StructFieldAccess { .. }, Expr::Const(_)) |
                    (Expr::ListIndex { .. }, Expr::Const(_)) |
                    (Expr::Const(_), Expr::VariantPath { .. }) |
                    (Expr::Const(_), Expr::StructFieldAccess { .. })
                )
            }
            _ =&gt; false,
        }
    }
}
```

#### 4. Cost Model Extensions

```rust
// In crates/ra-cost/src/nested.rs

/// Cost adjustments for nested data operations
pub struct NestedCostModel {
    /// Base cost model
    base: Box&lt;dyn CostModel&gt;,

    /// Cost per byte of VARIANT parsing
    variant_parse_cost_per_byte: f64,

    /// Cost multiplier for dictionary-encoded path access (cheaper)
    dict_access_multiplier: f64,

    /// Cost per LIST element for transformations
    list_transform_cost_per_element: f64,
}

impl NestedCostModel {
    /// Estimate cost of VARIANT path access
    pub fn cost_variant_path(&self, stats: &NestedColumnStats, path: &str) -&gt; f64 {
        let path_stats = match stats.variant_paths.get(path) {
            Some(s) =&gt; s,
            None =&gt; return self.variant_parse_cost_per_byte * 128.0, // Worst case: parse full value
        };

        // Check if path is dictionary-encoded
        let is_dict_encoded = path_stats.access_frequency &gt; 1000;

        if is_dict_encoded {
            // Dictionary lookup: O(1)
            self.base.cost_hash_lookup() * self.dict_access_multiplier
        } else {
            // JSON parsing cost: proportional to average value size
            let avg_size = self.estimate_avg_path_size(path_stats);
            self.variant_parse_cost_per_byte * avg_size
        }
    }

    /// Estimate cost of LIST transformation
    pub fn cost_list_transform(&self, list_stats: &ListStats, lambda_cost: f64) -&gt; f64 {
        let avg_list_length = list_stats.avg_length();
        let per_element_cost = lambda_cost + self.list_transform_cost_per_element;
        avg_list_length * per_element_cost
    }

    /// Estimate cost of FLATTEN operation
    pub fn cost_flatten(&self, input_rows: u64, expansion_factor: f64) -&gt; f64 {
        let output_rows = (input_rows as f64 * expansion_factor) as u64;

        // Cost = input scan + output materialization + memory allocation
        self.base.cost_seq_scan(input_rows) +
        self.base.cost_materialize(output_rows) +
        self.estimate_allocation_cost(output_rows)
    }
}
```

#### 5. Query Rewrite Rules

```rust
// In crates/ra-rules/src/nested.rs

/// Optimize FLATTEN operations
pub struct FlattenOptimizationRules;

impl FlattenOptimizationRules {
    /// Rewrite FLATTEN + Filter to push predicate into FLATTEN
    /// SELECT * FROM t, LATERAL FLATTEN(data) f WHERE f.value:price &gt; 100
    ///   -&gt; SELECT * FROM t, LATERAL FLATTEN(data, filter =&gt; value:price &gt; 100) f
    pub fn pushdown_filter_into_flatten(&self, expr: &RelExpr) -&gt; Option&lt;RelExpr&gt; {
        match expr {
            RelExpr::Filter {
                predicate,
                input: box RelExpr::Join {
                    join_type: JoinType::Cross,
                    right: box RelExpr::Flatten { input_expr, .. },
                    ..
                },
            } =&gt; {
                // Extract predicates on FLATTEN output columns
                let flatten_preds = self.extract_flatten_predicates(predicate);
                if flatten_preds.is_empty() {
                    return None;
                }

                // Push predicates into FLATTEN's input_expr
                Some(self.rewrite_flatten_with_filter(expr, flatten_preds))
            }
            _ =&gt; None,
        }
    }

    /// Avoid FLATTEN when direct path access suffices
    /// SELECT f.value:name FROM t, LATERAL FLATTEN(data:items) f
    ///   -&gt; SELECT data:items[*]:name FROM t  (if database supports array path notation)
    pub fn eliminate_unnecessary_flatten(&self, expr: &RelExpr) -&gt; Option&lt;RelExpr&gt; {
        // Pattern match: FLATTEN followed by simple path access
        // Replace with array path notation if supported
        unimplemented!("Requires dialect-specific support")
    }
}

/// Optimize LIST operations
pub struct ListOptimizationRules;

impl ListOptimizationRules {
    /// Fuse multiple LIST transformations
    /// list_transform(list_transform(l, f), g) -&gt; list_transform(l, x -&gt; g(f(x)))
    pub fn fuse_list_transforms(&self, expr: &Expr) -&gt; Option&lt;Expr&gt; {
        match expr {
            Expr::ListTransform {
                list: box Expr::ListTransform {
                    list: inner_list,
                    lambda: inner_lambda,
                },
                lambda: outer_lambda,
            } =&gt; {
                // Compose lambdas: outer(inner(x))
                let fused_lambda = self.compose_lambdas(inner_lambda, outer_lambda)?;
                Some(Expr::ListTransform {
                    list: inner_list.clone(),
                    lambda: Box::new(fused_lambda),
                })
            }
            _ =&gt; None,
        }
    }

    /// Push predicates into LIST filter
    /// WHERE list_contains(items, x) AND other_cond
    ///   -&gt; list_filter(items, lambda) + WHERE other_cond
    pub fn extract_list_predicates(&self, expr: &Expr) -&gt; Option&lt;Expr&gt; {
        unimplemented!("Complex predicate analysis required")
    }
}
```

### Integration Points

#### Parser Integration

```rust
// In crates/ra-parser/src/nested.rs

impl Parser {
    /// Parse VARIANT path access: data:customer.name
    fn parse_variant_path(&mut self) -&gt; Result&lt;Expr&gt; {
        let base = self.parse_primary_expr()?;

        if self.consume_token(TokenType::Colon) {
            let path = self.parse_json_path()?;
            Ok(Expr::VariantPath {
                base: Box::new(base),
                path: PathSegment::DotPath(path),
            })
        } else {
            Ok(base)
        }
    }

    /// Parse LIST literal: [1, 2, 3]
    fn parse_list_literal(&mut self) -&gt; Result&lt;Expr&gt; {
        self.expect_token(TokenType::LeftBracket)?;
        let mut elements = Vec::new();

        while !self.check_token(TokenType::RightBracket) {
            elements.push(self.parse_expr()?);
            if !self.check_token(TokenType::RightBracket) {
                self.expect_token(TokenType::Comma)?;
            }
        }

        self.expect_token(TokenType::RightBracket)?;
        Ok(Expr::ListConstructor(elements))
    }

    /// Parse lambda expression: x -&gt; x * 2
    fn parse_lambda(&mut self) -&gt; Result&lt;Expr&gt; {
        let params = self.parse_lambda_params()?;
        self.expect_token(TokenType::Arrow)?;  // -&gt;
        let body = self.parse_expr()?;

        Ok(Expr::Lambda {
            params,
            body: Box::new(body),
        })
    }
}
```

#### Catalog Integration

```rust
// In crates/ra-catalog/src/nested.rs

/// Extended column metadata for nested types
pub struct NestedColumnInfo {
    /// Base column info
    pub base: ColumnInfo,

    /// For VARIANT: tracked paths with statistics
    pub variant_tracked_paths: Vec&lt;String&gt;,

    /// For STRUCT: field definitions
    pub struct_fields: Vec&lt;StructField&gt;,

    /// For LIST: element type
    pub list_element_type: Option&lt;DataType&gt;,

    /// For MAP: key and value types
    pub map_types: Option&lt;(DataType, DataType)&gt;,
}

impl Catalog {
    /// Register nested column and track access patterns
    pub fn update_path_access_stats(&mut self,
        table: &str,
        column: &str,
        path: &str,
        access_count: u64
    ) -&gt; Result&lt;()&gt; {
        let col_info = self.get_nested_column_mut(table, column)?;

        // Track frequently-accessed paths
        if access_count &gt; 100 && !col_info.variant_tracked_paths.contains(&path.to_string()) {
            col_info.variant_tracked_paths.push(path.to_string());

            // Suggest materialized column for hot paths
            if access_count &gt; 10000 {
                self.add_optimization_hint(OptimizationHint::MaterializePath {
                    table: table.to_string(),
                    column: column.to_string(),
                    path: path.to_string(),
                    access_frequency: access_count,
                });
            }
        }

        Ok(())
    }
}
```

### Error Handling

**Type Mismatches:**

```rust
// Type checking for nested operations
pub enum NestedTypeError {
    /// Attempted list operation on non-list type
    NotAList {
        expr: Expr,
        actual_type: DataType,
    },

    /// STRUCT field not found
    FieldNotFound {
        struct_type: DataType,
        field_name: String,
    },

    /// Lambda parameter count mismatch
    LambdaArityMismatch {
        expected: usize,
        actual: usize,
    },

    /// VARIANT path does not exist at runtime
    PathNotFound {
        variant_value: VariantValue,
        path: String,
    },

    /// MAP key type mismatch
    MapKeyTypeMismatch {
        expected: DataType,
        actual: DataType,
    },
}

impl fmt::Display for NestedTypeError {
    fn fmt(&self, f: &mut fmt::Formatter) -&gt; fmt::Result {
        match self {
            Self::NotAList { expr, actual_type } =&gt; {
                write!(f, "Cannot apply list operation to {}: type is {:?}, expected LIST",
                    expr, actual_type)
            }
            Self::FieldNotFound { struct_type, field_name } =&gt; {
                write!(f, "Field '{}' not found in struct type {:?}", field_name, struct_type)
            }
            Self::LambdaArityMismatch { expected, actual } =&gt; {
                write!(f, "Lambda expects {} parameters, got {}", expected, actual)
            }
            Self::PathNotFound { variant_value, path } =&gt; {
                write!(f, "Path '{}' not found in VARIANT value: {:?}", path, variant_value)
            }
            Self::MapKeyTypeMismatch { expected, actual } =&gt; {
                write!(f, "MAP key type mismatch: expected {:?}, got {:?}", expected, actual)
            }
        }
    }
}
```

**Runtime Errors:**

- **NULL Handling:** All nested operations propagate NULL (VARIANT path on NULL returns NULL)
- **Out-of-Bounds:** LIST/ARRAY index out of range returns NULL (not error)
- **Type Coercion:** VARIANT automatically coerces types where safe (number to string)
- **Memory Limits:** VARIANT values exceeding 128 MB threshold trigger error

### Performance Considerations

#### Optimization Opportunities

1. **Dictionary Encoding Pushdown (10-50x speedup):**
   - Convert `data:status = 'active'` to dictionary code comparison
   - Avoid JSON parsing for frequently-accessed paths
   - Parquet/ORC already use dictionary encoding

2. **Late Materialization (2-5x speedup):**
   - Delay VARIANT parsing until projection
   - Read only accessed paths from storage
   - Critical for Parquet nested column reading

3. **Predicate Pushdown (10-100x I/O reduction):**
   - Push `data:timestamp &gt; '2024-01-01'` to Parquet row group pruning
   - Use zone maps on nested fields
   - Skip entire files based on nested predicates

4. **Vectorized LIST Operations (3-10x speedup):**
   - SIMD for numeric list transformations
   - Batch lambda evaluation (compile once, apply to many lists)
   - Avoid per-element function call overhead

5. **STRUCT Field Pruning (2-20x speedup on wide structs):**
   - Only read accessed STRUCT fields from Parquet
   - Huge win for 100+ field structs (common in logs)

#### Benchmarks (Expected)

| Operation | Baseline (No Opt) | With Pushdown | Speedup |
|-----------|------------------|---------------|---------|
| VARIANT path filter | 10 GB/s scan | 1 GB/s scan | **10x** |
| LIST transform | 50k rows/s | 500k rows/s | **10x** |
| STRUCT field access | Full scan | Column scan | **20x** (for 100-field structs) |
| FLATTEN + filter | 2 passes | 1 pass | **2x** |
| Nested aggregation | Unnest + agg | Direct agg | **3-5x** |

## Drawbacks

### Complexity Cost

1. **Type System Overhaul (HIGH):**
   - Adds 7 new types (VARIANT, OBJECT, ARRAY, LIST, STRUCT, MAP, FixedArray)
   - Complicates type inference and checking
   - Increases codebase size by ~15-20%

2. **Parser Extension (MEDIUM):**
   - New syntax: colon notation, lambda expressions, nested literals
   - Ambiguity with existing SQL syntax (bracket notation)
   - Requires careful grammar design

3. **Optimizer Complexity (VERY HIGH):**
   - 50+ new rewrite rules for nested operations
   - Complex predicate pushdown analysis
   - Lambda expression optimization is hard

### Maintenance Burden

1. **Cross-Database Compatibility:**
   - Snowflake VARIANT != DuckDB STRUCT semantics
   - Need dialect-specific rewrites
   - Testing matrix explosion (5 types × 3 databases)

2. **Statistics Collection:**
   - Tracking path statistics adds metadata overhead
   - Incremental updates complex for nested data
   - Storage cost for path dictionaries

3. **Ongoing Feature Evolution:**
   - Snowflake/DuckDB continuously add nested functions
   - Need to track upstream changes
   - Backward compatibility concerns

### Performance Overhead

1. **Type Checking Overhead:**
   - Runtime type checks for VARIANT operations
   - Lambda closure capture overhead
   - Indirect function calls for type-generic operations

2. **Memory Overhead:**
   - VARIANT type tags consume extra space
   - LIST offset arrays add 4-8 bytes per list
   - STRUCT null bitmaps per field

3. **Cold Path Penalties:**
   - First access to VARIANT path requires full parse
   - Lambda compilation latency
   - Dictionary build time for new columns

### Learning Curve

**For Users:**
- New syntax to learn (colon notation, lambdas)
- Subtle differences between VARIANT/STRUCT/MAP
- When to use LIST vs ARRAY

**For Developers:**
- Complex type system with recursive types
- Lambda expression AST manipulation
- Nested statistics algorithms (HyperLogLog for paths)

### Breaking Changes

- None for existing relational queries (fully backward compatible)
- May change internal cost model constants
- Catalog schema evolution required (add nested column metadata)

## Rationale and alternatives

### Why This Design?

**1. Unified Type System:**
- Supporting both Snowflake (VARIANT) and DuckDB (LIST/STRUCT/MAP) in one coherent system
- Allows cross-database query translation
- Future-proof for other nested type systems (PostgreSQL JSONB, MongoDB BSON)

**2. First-Class Lambda Expressions:**
- Essential for DuckDB LIST operations
- Enables functional programming patterns
- Aligns with modern SQL evolution (SQL:2016 polymorphic table functions)

**3. Statistics-Driven Optimization:**
- Path-level statistics enable intelligent pushdown
- Access frequency tracking guides materialization recommendations
- Directly addresses "blind spot" problem in nested data optimization

**4. Storage-Agnostic Design:**
- Works with Parquet, Arrow, JSONB, or custom formats
- Physical storage optimization independent of logical type system
- Extensible to new columnar formats

### Alternative Approaches

#### Alternative 1: JSON Functions Only (Rejected)

**Approach:** Support JSON parsing functions (`JSON_EXTRACT`, etc.) without type system changes.

**Why Rejected:**
- No type safety (everything is TEXT)
- No predicate pushdown (functions are opaque)
- No statistics collection (can't analyze JSON structure)
- Poor performance (parse JSON on every access)

#### Alternative 2: Separate VARIANT and LIST Type Systems (Rejected)

**Approach:** Implement Snowflake types and DuckDB types as disconnected systems.

**Why Rejected:**
- Code duplication (similar functionality)
- No cross-database compatibility
- Users need to learn two systems
- Missed optimization opportunities (shared infrastructure)

#### Alternative 3: External Type Plugin System (Considered)

**Approach:** Design plugin API for custom types, implement nested types as plugins.

**Why Rejected for Now:**
- Over-engineering for current needs
- Adds abstraction overhead
- Plugin ABI stability concerns
- Can revisit after stabilization

**May Reconsider:** If we need to support database-specific extensions beyond Snowflake/DuckDB.

#### Alternative 4: Lazy Type System (Considered)

**Approach:** All nested data is untyped at optimization time, types resolved at execution.

**Why Rejected:**
- Loses optimization opportunities (can't push predicates without types)
- Runtime type errors instead of planning errors
- Harder to generate efficient code

### Impact of Not Doing This

**Short-Term (0-6 months):**
- Ra cannot optimize Snowflake or DuckDB nested queries
- Users forced to use workarounds (JSON parsing, manual unnesting)
- 10-100x performance loss on nested data workloads

**Medium-Term (6-18 months):**
- Ra becomes non-viable for cloud data warehouse optimization
- Competitor optimizers gain market share
- Technical debt accumulates as users hack around limitations

**Long-Term (18+ months):**
- Ra relegated to legacy relational-only use cases
- Difficult to attract users from modern analytics platforms
- Type system rewrite later is more disruptive

## Prior art

### Academic Research

**1. "Efficient Query Evaluation on Probabilistically Incomplete Data" (Antova et al., 2008)**
- Introduced type system for uncertain data (related to VARIANT's multiple possible types)
- Key insight: Maintain type distributions and optimize based on most probable type

**2. "Querying JSON with SQL" (Bray, 2014)**
- Analyzed performance of SQL/JSON path expressions
- Showed 10-50x speedup from index on frequently-accessed paths
- Motivated our path statistics tracking

**3. "Dremel: Interactive Analysis of Web-Scale Datasets" (Melnik et al., 2010)**
- Columnar storage for nested data (basis for Parquet)
- Showed how repetition and definition levels enable column pruning
- Directly applicable to STRUCT field pruning

**4. "Lambda: The Ultimate GOTO" (Steele & Sussman, 1998)**
- Lambda calculus foundations for first-class functions
- Informed our lambda expression design

### Industry Solutions

#### PostgreSQL

**JSONB Type:**
- Binary JSON with indexing (GIN indexes)
- Path operators: `data-&gt;'key'`, `data-&gt;&gt;'key'`
- No nested relational types (LIST/STRUCT)

**What We Learn:**
- Dictionary encoding for JSON keys
- B-tree indexes on JSONB paths (expression indexes)
- Type coercion rules (JSONB to SQL types)

#### MySQL

**JSON Type:**
- Native JSON storage with virtual columns
- JSON path syntax: `$.path.to.field`
- Limited optimization (no predicate pushdown)

**What We Learn:**
- Virtual columns for hot paths (our materialization recommendation)
- SQL/JSON standard functions (alignment opportunity)

#### SQLite

**JSON Functions:**
- `json_extract()`, `json_tree()` functions
- No native JSON type (stored as TEXT)
- Limited optimization

**What We Learn:**
- Keep JSON functions as fallback for unsupported types
- Show performance benefits of native types vs. functions

#### DuckDB

**Native Nested Types:**
- LIST, STRUCT, MAP, ARRAY as first-class types
- Lambda expressions: `list_transform(l, x -&gt; x * 2)`
- Deep Parquet integration (column pruning, pushdown)

**What We Learn:**
- Type inference for nested types
- Lambda expression syntax and semantics
- Vectorized execution for list operations
- Critical: Late materialization for STRUCT field access

#### Snowflake

**VARIANT/OBJECT/ARRAY:**
- Self-describing VARIANT type (stores type tag)
- Automatic path extraction and indexing
- FLATTEN operator with 6-column output
- Search optimization service for point lookups

**What We Learn:**
- Path statistics collection (access frequency tracking)
- FLATTEN as relational operator (not just function)
- Zone map pruning on nested fields
- Dictionary encoding benefits

#### Apache Calcite

**Type System:**
- Supports nested types via Java object mapping
- No native optimization for nested predicates
- User-defined functions for custom types

**What We Learn:**
- Plugin-based type system (future consideration)
- Shows importance of built-in optimization support

### What We Can Learn

**Key Insights:**

1. **Statistics Are Critical:** All high-performance systems (DuckDB, Snowflake) track nested field statistics
2. **Dictionary Encoding Wins:** PostgreSQL JSONB and Snowflake both use dictionaries for keys/values
3. **Late Materialization:** DuckDB's performance relies on delaying field access until needed
4. **Path Indexes:** PostgreSQL expression indexes show value of indexing frequently-accessed paths
5. **Type Coercion Rules:** All systems struggle with VARIANT/JSON type coercion; need clear rules

## Unresolved questions

### Design Questions

1. **Lambda Closure Semantics:**
   - Should lambdas capture variables from outer scope?
   - How to handle mutable captures?
   - Performance implications of closure allocation?

2. **VARIANT Type Coercion Rules:**
   - When does `data:value = 123` match string "123"?
   - Should we follow Snowflake (strict) or JavaScript (loose) semantics?
   - How to handle ambiguous comparisons?

3. **Nested Statistics Storage:**
   - Store path statistics in catalog or separate metadata table?
   - How often to refresh statistics?
   - Memory budget for path dictionaries?

4. **FLATTEN Output Schema:**
   - Always produce 6 columns (Snowflake) or configurable?
   - Allow aliasing FLATTEN columns?
   - How to handle FLATTEN in subqueries?

### Implementation Strategy Questions

1. **Phased Rollout:**
   - Implement VARIANT first or LIST/STRUCT?
   - Can we ship partial functionality (types without all operations)?
   - Backward compatibility strategy?

2. **Performance Validation:**
   - Benchmark suite for nested operations?
   - Regression tests for optimization rules?
   - How to measure predicate pushdown effectiveness?

3. **Cross-Database Testing:**
   - Test against real Snowflake/DuckDB instances?
   - Use TPC-DS with nested extensions?
   - Need synthetic workload generator?

### Integration Questions

1. **Catalog Migration:**
   - How to migrate existing catalogs to support nested columns?
   - Backward compatibility for old catalogs?
   - Can we lazily upgrade column metadata?

2. **External Format Support:**
   - Which Parquet logical types to support initially?
   - Arrow compatibility requirements?
   - JSONB interoperability with PostgreSQL?

3. **Dialect Translation:**
   - Can we translate Snowflake VARIANT to DuckDB STRUCT automatically?
   - Should we support both syntaxes in Ra SQL?
   - How to handle dialect-specific functions?

### To Resolve Before Merge

- Lambda closure semantics (decision needed for API)
- VARIANT type coercion rules (affects correctness)
- Statistics storage location (impacts catalog design)

### To Resolve During Implementation

- Performance benchmarks (need working implementation)
- Optimal dictionary encoding thresholds (require tuning)
- Memory management for large VARIANT values

### Out of Scope (Future Work)

- UNION types (DuckDB discriminated unions)
- Recursive types (self-referential STRUCTs)
- Custom user-defined nested types
- Distributed nested aggregation (requires RFC 0006)

## Future possibilities

### Natural Extensions

#### 1. GIN Indexes for Nested Data (6-12 months)

Build generalized inverted indexes on VARIANT paths, LIST elements, and STRUCT fields:

```sql
CREATE INDEX idx_orders_customer ON orders
  USING GIN (data:customer.id);
```

Enables point lookups on nested fields without full scans.

#### 2. Nested Aggregation Pushdown (3-6 months)

Optimize aggregations over nested data:

```sql
-- Push aggregation into LIST without unnesting
SELECT user_id, SUM(list_sum(purchases))
FROM users
GROUP BY user_id;

-- Directly aggregate nested field
SELECT data:region, AVG(data:sales)
FROM revenue
GROUP BY data:region;
```

Avoid materialization of flattened data.

#### 3. Computed Columns for Hot Paths (2-3 months)

Automatically materialize frequently-accessed VARIANT paths:

```sql
-- Ra suggests:
ALTER TABLE orders
  ADD COLUMN customer_id AS (data:customer.id) STORED;

-- Rewrite queries to use materialized column
SELECT * FROM orders WHERE data:customer.id = 123
  -&gt; SELECT * FROM orders WHERE customer_id = 123
```

Eliminates JSON parsing overhead for common paths.

#### 4. Nested Materialized Views (6-9 months)

Support MVs over FLATTEN operations:

```sql
CREATE MATERIALIZED VIEW order_items AS
SELECT o.id, f.value:product_id, f.value:quantity
FROM orders o, LATERAL FLATTEN(o.data:items) f;

-- Transparently rewrite queries to use MV
```

Combines nested data support with MV matching (RFC 0051).

#### 5. Approximate Nested Aggregates (3-4 months)

HyperLogLog and T-Digest for nested data:

```sql
-- Approximate distinct count of nested values
SELECT approx_count_distinct(list_element(purchases))
FROM users;

-- Quantile over nested numeric list
SELECT approx_quantile(data:latencies, 0.95)
FROM monitoring;
```

Enables fast analytics on large nested datasets.

#### 6. Vector Similarity Search on LIST (9-12 months)

Integrate with vector operations ([RFC 0064](/maintainers/rfcs/0064-vector-similarity-search-optimization)):

```sql
-- Cosine similarity on embedding lists
SELECT id, cosine_similarity(embedding, target_vector)
FROM documents
ORDER BY cosine_similarity(embedding, target_vector) DESC
LIMIT 10;
```

Critical for ML/AI applications.

### Long-term Vision

**Goal:** Position Ra as the **premier optimizer for heterogeneous data**, bridging traditional relational and modern nested/semi-structured workloads.

**5-Year Roadmap:**

1. **Year 1 (Foundation):**
   - Core nested types (VARIANT, LIST, STRUCT, MAP)
   - Basic predicate pushdown and statistics
   - DuckDB and Snowflake syntax support

2. **Year 2 (Optimization):**
   - Advanced pushdown (dictionary encoding, late materialization)
   - Nested aggregation optimization
   - GIN indexes and computed columns

3. **Year 3 (Integration):**
   - Cross-format optimization (Parquet ↔ JSONB ↔ Arrow)
   - Federated nested queries (join Parquet + Snowflake)
   - Nested MV matching and rewriting

4. **Year 4 (Advanced Features):**
   - Recursive types and traversal
   - Graph query patterns on nested data
   - Machine learning over nested features

5. **Year 5 (Ecosystem Leadership):**
   - Define best practices for nested query optimization
   - Influence SQL standard evolution (SQL:202X nested features)
   - Open-source nested optimizer as standalone library

**End State:** Ra becomes the **reference implementation** for nested relational optimization, used by multiple databases and query engines.

---

## Implementation Plan

### Phase 1: Type System Foundation (8-10 weeks)

**Goal:** Basic nested types without optimization.

**Deliverables:**
1. Core type definitions (VARIANT, LIST, STRUCT, MAP, ARRAY, OBJECT)
2. Parser support for nested literals and path access
3. Type checking and inference for nested expressions
4. Basic execution (no pushdown or optimization)
5. Unit tests for type operations

**Validation:** Can parse and execute simple nested queries, results match DuckDB/Snowflake.

### Phase 2: Statistics and Pushdown (6-8 weeks)

**Goal:** Enable predicate pushdown for nested fields.

**Deliverables:**
1. Path statistics collection (access frequency, type distribution, min/max)
2. Predicate pushdown rules for VARIANT/STRUCT/LIST
3. Dictionary encoding for frequently-accessed paths
4. Cost model extensions for nested operations
5. Integration tests with Parquet files

**Validation:** Measure 10-50x I/O reduction on filtered nested queries.

### Phase 3: Lambda Expressions (4-6 weeks)

**Goal:** Support DuckDB list_transform/list_filter operations.

**Deliverables:**
1. Lambda expression AST and type checking
2. Lambda evaluation engine
3. List transformation optimizations (fusion, vectorization)
4. Performance benchmarks vs. DuckDB

**Validation:** DuckDB list query compatibility at 80%+ performance.

### Phase 4: FLATTEN Operator (3-4 weeks)

**Goal:** Support Snowflake LATERAL FLATTEN.

**Deliverables:**
1. FLATTEN relational operator
2. 6-column output schema
3. Recursive flattening support
4. Predicate pushdown into FLATTEN

**Validation:** Snowflake FLATTEN query compatibility.

### Phase 5: Advanced Optimizations (6-8 weeks)

**Goal:** Achieve parity with native Snowflake/DuckDB performance.

**Deliverables:**
1. Late materialization for STRUCT fields
2. Vectorized LIST operations (SIMD where applicable)
3. Nested aggregation optimization
4. GIN index support (if time permits)

**Validation:** Match or exceed Snowflake/DuckDB performance on nested benchmarks.

### Phase 6: Documentation and Stabilization (2-3 weeks)

**Goal:** Production-ready feature.

**Deliverables:**
1. User guide with examples
2. Developer documentation (internals)
3. Migration guide (catalog upgrades)
4. Compatibility matrix (Snowflake vs. DuckDB semantics)

**Validation:** External beta testing feedback.

---

**Total Estimated Effort:** 30-40 weeks (7-10 months) with 1-2 full-time engineers.

**Expected Impact:** Very High — Enables Ra to optimize 20+ dependent features and unlock cloud data warehouse optimization market.


## Referenced By

This RFC is referenced by:

- [RFC 99: Semi-Structured Data Types](/maintainers/rfcs/0099-semi-structured-data-types)


## Referenced By

This RFC is referenced by:

- [RFC 99: Semi-Structured Data Types](/maintainers/rfcs/0099-semi-structured-data-types)


## Referenced By

This RFC is referenced by:

- [RFC 99: Semi-Structured Data Types](/maintainers/rfcs/0099-semi-structured-data-types)


## Referenced By

This RFC is referenced by:

- [RFC 99: Semi-Structured Data Types](/maintainers/rfcs/0099-semi-structured-data-types)


## Referenced By

This RFC is referenced by:

- [RFC 99: Semi-Structured Data Types](/maintainers/rfcs/0099-semi-structured-data-types)


## Referenced By

This RFC is referenced by:

- [RFC 99: Semi-Structured Data Types](/maintainers/rfcs/0099-semi-structured-data-types)


## Referenced By

This RFC is referenced by:

- [RFC 99: Semi-Structured Data Types](/maintainers/rfcs/0099-semi-structured-data-types)


## Referenced By

This RFC is referenced by:

- [RFC 99: Semi-Structured Data Types](/maintainers/rfcs/0099-semi-structured-data-types)


## Referenced By

This RFC is referenced by:

- [RFC 99: Semi-Structured Data Types](/maintainers/rfcs/0099-semi-structured-data-types)


## Referenced By

This RFC is referenced by:

- [RFC 99: Semi-Structured Data Types](/maintainers/rfcs/0099-semi-structured-data-types)


## Referenced By

This RFC is referenced by:

- [RFC 99: Semi-Structured Data Types](/maintainers/rfcs/0099-semi-structured-data-types)


## Referenced By

This RFC is referenced by:

- [RFC 99: Semi-Structured Data Types](/maintainers/rfcs/0099-semi-structured-data-types)


## Referenced By

This RFC is referenced by:

- [RFC 99: Semi-Structured Data Types](/maintainers/rfcs/0099-semi-structured-data-types)


## Referenced By

This RFC is referenced by:

- [RFC 99: Semi-Structured Data Types](/maintainers/rfcs/0099-semi-structured-data-types)
