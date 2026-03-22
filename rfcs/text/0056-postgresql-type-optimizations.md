# RFC 0056: PostgreSQL Type-Specific Optimizations

- Start Date: 2026-03-22
- Author: Ra Development Team
- Status: Proposed
- Tracking Issue: N/A

## Summary

Deep optimizations for PostgreSQL's advanced type system, focusing on JSONB, XML, arrays, and TOAST (The Oversized-Attribute Storage Technique). This RFC builds on RFC 0055 (general type support) with PostgreSQL-specific rules, cost model adjustments, and index recommendations.

## Motivation

PostgreSQL has the most advanced type system among open-source databases, featuring:

- **JSONB**: Binary JSON format with GIN indexes, faster than MySQL/Oracle JSON
- **XML**: Native XML type with XPath support
- **Arrays**: First-class native arrays (not simulated via strings)
- **TOAST**: Automatic compression and out-of-line storage for large values
- **Range types**: Discrete and continuous ranges with GiST indexes
- **Full-text search**: tsvector/tsquery with GIN indexes

**These types have unique optimization opportunities:**

1. **JSONB containment rewrites**: `data->>'key' = 'value'` → `data @> '{"key": "value"}'` (indexable)
2. **GIN index selection**: Automatically suggest GIN for JSONB/array containment queries
3. **TOAST awareness**: Avoid fetching large columns unless needed, adjust cost model
4. **Partial indexes**: Create indexes on JSONB subsets for common filters
5. **Array unnesting**: Transform array operations to set operations

This RFC provides PostgreSQL-specific optimization rules that leverage these features.

## Guide-level explanation

### JSONB Optimization Example

**Query:**

```sql
SELECT id, data->>'name' AS name, data->>'email' AS email
FROM users
WHERE data->>'status' = 'active'
  AND data->>'verified' = 'true'
  AND data->>'country' = 'US';
```

**Without PostgreSQL-specific optimization:**

- Ra treats `data` as generic column
- Sequential scan (no index used)
- Three separate JSON extractions in WHERE clause

**With PostgreSQL-specific optimization:**

```rust
// Rewrite to containment (indexable)
let rewritten_predicate = Expr::BinOp {
    op: Op::JsonContains,
    left: col("data"),
    right: jsonb_literal(r#"{"status": "active", "verified": "true", "country": "US"}"#),
};

// Suggest GIN index
let recommendation = IndexRecommendation {
    table: "users",
    columns: vec!["data"],
    index_type: IndexType::Gin,
    rationale: "JSONB containment query benefits from GIN index",
    sql: "CREATE INDEX idx_users_data ON users USING GIN (data);",
    estimated_speedup: 100.0,  // 100x faster
};

// Or suggest partial GIN index for specific keys
let partial_index = IndexRecommendation {
    table: "users",
    columns: vec!["data"],
    index_type: IndexType::GinPartial,
    rationale: "Partial GIN index for frequent status/country filters",
    sql: "CREATE INDEX idx_users_active_us ON users USING GIN (data) WHERE (data->>'status' = 'active' AND data->>'country' = 'US');",
    estimated_speedup: 150.0,  // Even faster due to smaller index
};
```

### TOAST Optimization Example

**Query:**

```sql
SELECT id, title  -- Don't select large_description column
FROM articles
WHERE status = 'published'
ORDER BY created_at DESC
LIMIT 10;
```

**TOAST-aware optimization:**

```rust
// Detect that large_description is TOASTed (>2KB average size)
let large_cols = stats.toasted_columns("articles");
// large_cols = ["large_description", "full_content"]

// Ensure projection excludes TOASTed columns
// Cost model: avoid fetching TOASTed data unless needed
let scan_cost = if projection_uses_toasted_columns(&projection, &large_cols) {
    base_cost * 3.0  // 3x I/O cost (main table + TOAST table reads)
} else {
    base_cost  // Only main table
};
```

**Optimization: Late materialization for TOASTed columns**

```sql
-- Original (inefficient)
SELECT *
FROM articles
WHERE status = 'published'
ORDER BY created_at DESC
LIMIT 10;

-- Optimized (avoid TOAST reads until after LIMIT)
SELECT a.*, t.large_description
FROM (
    SELECT id, title, status, created_at
    FROM articles
    WHERE status = 'published'
    ORDER BY created_at DESC
    LIMIT 10
) a
LEFT JOIN articles t ON a.id = t.id;  -- Fetch TOASTed columns only for 10 rows
```

## Reference-level explanation

### Implementation Details

**JSONB Optimization Rules:**

**Rule 1: JSON Extraction to Containment**

```rust
pub fn jsonb_extraction_to_containment(expr: &Expr) -> Option<Expr> {
    match expr {
        // Single key-value: data->>'key' = 'value'
        Expr::BinOp {
            op: Op::Eq,
            left: box Expr::JsonExtractText { object, path },
            right: box Expr::Const(value),
        } => {
            Some(Expr::BinOp {
                op: Op::JsonContains,
                left: object.clone(),
                right: Box::new(Expr::Const(Const::Jsonb(
                    json!({ path: value })
                ))),
            })
        }

        // Multiple ANDed conditions → single containment
        Expr::BinOp {
            op: Op::And,
            left: box json_eq1,
            right: box json_eq2,
        } => {
            // Combine into single @> operator
            // data->>'k1' = 'v1' AND data->>'k2' = 'v2'
            // → data @> '{"k1": "v1", "k2": "v2"}'
            combine_json_conditions(json_eq1, json_eq2)
        }

        _ => None,
    }
}
```

**Rule 2: GIN Index Recommendations**

```rust
impl IndexAdvisor {
    pub fn recommend_jsonb_indexes(&self, column: &Column, predicates: &[Expr]) -> Vec<IndexRecommendation> {
        let mut recommendations = Vec::new();

        // Check for containment queries
        let has_containment = predicates.iter().any(|p| {
            matches!(p, Expr::BinOp { op: Op::JsonContains, .. })
        });

        if has_containment {
            recommendations.push(IndexRecommendation {
                index_type: IndexType::Gin,
                sql: format!("CREATE INDEX idx_{}_gin ON {} USING GIN ({});",
                    column.name, column.table, column.name),
                rationale: "JSONB containment (@>) benefits from GIN index".to_string(),
                estimated_speedup: 100.0,
            });
        }

        // Check for path-specific queries
        let frequent_paths = self.extract_frequent_json_paths(predicates);
        if frequent_paths.len() > 0 && frequent_paths.len() <= 3 {
            // Suggest expression index on specific paths
            let paths_expr = frequent_paths.iter()
                .map(|p| format!("({}->>'{}'))", column.name, p))
                .collect::<Vec<_>>()
                .join(", ");

            recommendations.push(IndexRecommendation {
                index_type: IndexType::BTree,
                sql: format!("CREATE INDEX idx_{}_{} ON {} ({});",
                    column.name, frequent_paths.join("_"), column.table, paths_expr),
                rationale: format!("Expression index for frequently queried JSON paths: {:?}", frequent_paths),
                estimated_speedup: 50.0,
            });
        }

        recommendations
    }
}
```

**TOAST Optimization Rules:**

**Rule 1: TOAST Detection**

```rust
impl Statistics {
    pub fn detect_toasted_columns(&self, table: &str) -> Vec<String> {
        // Query PostgreSQL system catalogs
        // SELECT attname FROM pg_attribute
        // WHERE attrelid = 'table'::regclass
        //   AND attstorage IN ('x', 'e', 'm')  -- Extended, External, Main
        //   AND typlen = -1;  -- Variable-length type

        let mut toasted = Vec::new();

        for col in self.columns(table) {
            let avg_size = self.avg_column_size(&col.name);
            if avg_size > 2048 {  // TOAST threshold (2KB)
                toasted.push(col.name.clone());
            }
        }

        toasted
    }
}
```

**Rule 2: Late Materialization for TOAST**

```rust
pub fn apply_toast_late_materialization(plan: &RelExpr) -> RelExpr {
    match plan {
        RelExpr::Project {
            columns,
            input: box RelExpr::Sort {
                keys,
                input: box RelExpr::Filter { predicate, input },
            },
        } => {
            let toasted_cols = detect_toasted_columns(input);
            let (non_toasted, toasted): (Vec<_>, Vec<_>) = columns.iter()
                .partition(|c| !toasted_cols.contains(&c.name));

            if toasted.is_empty() {
                return plan.clone();  // No optimization needed
            }

            // Rewrite: Fetch non-TOASTed columns first, apply filters/sort, then fetch TOASTed
            RelExpr::Project {
                columns: columns.clone(),
                input: Box::new(RelExpr::Join {
                    join_type: JoinType::Inner,
                    condition: /* join on primary key */,
                    left: Box::new(RelExpr::Project {
                        columns: non_toasted,
                        input: Box::new(RelExpr::Sort {
                            keys: keys.clone(),
                            input: Box::new(RelExpr::Filter {
                                predicate: predicate.clone(),
                                input: input.clone(),
                            }),
                        }),
                    }),
                    right: Box::new(RelExpr::Project {
                        columns: toasted,
                        input: input.clone(),
                    }),
                }),
            }
        }
        _ => plan.clone(),
    }
}
```

**Rule 3: TOAST-Aware Cost Model**

```rust
impl CostModel {
    pub fn estimate_scan_cost(&self, table: &str, columns: &[Column], stats: &Statistics) -> Cost {
        let base_rows = stats.table_cardinality(table);
        let base_cost = self.seq_scan_cost * (base_rows as f64);

        // Check if query accesses TOASTed columns
        let toasted_cols = stats.detect_toasted_columns(table);
        let accesses_toast = columns.iter().any(|c| toasted_cols.contains(&c.name));

        if accesses_toast {
            // TOAST access requires:
            // 1. Read main table row (base_cost)
            // 2. Follow TOAST pointer (random I/O)
            // 3. Read TOAST chunks (sequential I/O)
            // Approximate: 2-3x base cost
            let toast_overhead = 2.5;
            base_cost * toast_overhead
        } else {
            base_cost
        }
    }
}
```

**Array Optimization Rules:**

**Rule 1: Array Containment Optimization**

```rust
// Query: WHERE tags @> ARRAY['postgresql', 'optimization']
// Optimization: Use GIN index on tags array

impl IndexAdvisor {
    pub fn recommend_array_gin_index(&self, column: &Column, predicates: &[Expr]) -> Option<IndexRecommendation> {
        let uses_containment = predicates.iter().any(|p| {
            matches!(p, Expr::BinOp {
                op: Op::ArrayContains | Op::ArrayContainedBy | Op::ArrayOverlap,
                ..
            })
        });

        if uses_containment {
            Some(IndexRecommendation {
                index_type: IndexType::Gin,
                sql: format!("CREATE INDEX idx_{}_gin ON {} USING GIN ({});",
                    column.name, column.table, column.name),
                rationale: "Array containment operations benefit from GIN index".to_string(),
                estimated_speedup: 80.0,
            })
        } else {
            None
        }
    }
}
```

**Rule 2: Array Unnesting Optimization**

```rust
// Query: SELECT unnest(tags) FROM posts WHERE id = 123
// Optimization: If cardinality is low, evaluate eagerly

pub fn optimize_array_unnest(expr: &RelExpr) -> RelExpr {
    match expr {
        RelExpr::Project {
            columns: vec![Expr::Function { name: "unnest", args }],
            input,
        } => {
            // If input has low cardinality, unnesting is cheap
            let input_card = estimate_cardinality(input);
            if input_card < 100 {
                // Keep as-is, unnesting is fast
                expr.clone()
            } else {
                // Consider lateral join instead
                rewrite_to_lateral_join(expr)
            }
        }
        _ => expr.clone(),
    }
}
```

**XML Optimization Rules:**

**Rule 1: XPath Extraction Cost**

```rust
impl CostModel {
    pub fn estimate_xml_extraction_cost(&self, xpath: &str, doc_size: usize) -> Cost {
        // XML parsing cost is proportional to document size
        let parse_cost = (doc_size as f64) * 0.001;  // 0.001 per byte

        // XPath complexity (nested paths are more expensive)
        let xpath_depth = xpath.split('/').count();
        let xpath_cost = (xpath_depth as f64) * 0.1;

        parse_cost + xpath_cost
    }
}
```

### Integration Points

**1. Query Parser:**

Parse PostgreSQL-specific syntax:

```rust
// JSONB operators
parser.register_infix_op("@>", OpPrecedence::Comparison, Op::JsonContains);
parser.register_infix_op("<@", OpPrecedence::Comparison, Op::JsonContainedBy);
parser.register_infix_op("->", OpPrecedence::Primary, Op::JsonExtract);
parser.register_infix_op("->>", OpPrecedence::Primary, Op::JsonExtractText);
parser.register_infix_op("@?", OpPrecedence::Comparison, Op::JsonPathQuery);
parser.register_infix_op("@@", OpPrecedence::Comparison, Op::JsonPathMatch);

// Array operators
parser.register_infix_op("@>", OpPrecedence::Comparison, Op::ArrayContains);
parser.register_infix_op("<@", OpPrecedence::Comparison, Op::ArrayContainedBy);
parser.register_infix_op("&&", OpPrecedence::Comparison, Op::ArrayOverlap);
```

**2. Statistics Collection (PostgreSQL Extension):**

```rust
impl StatisticsCollector {
    pub fn collect_postgresql_type_stats(&self, table: &str) -> PostgreSQLTypeStats {
        PostgreSQLTypeStats {
            jsonb_columns: self.collect_jsonb_stats(table),
            array_columns: self.collect_array_stats(table),
            toast_columns: self.detect_toasted_columns(table),
        }
    }

    fn collect_jsonb_stats(&self, table: &str) -> HashMap<String, JsonbColumnStats> {
        // Query: SELECT jsonb_object_keys(data) AS key, COUNT(*) FROM users GROUP BY key
        // Get most common keys, average depth, etc.
        todo!()
    }
}
```

**3. Index Advisor (RFC 0021):**

Integrate PostgreSQL-specific index recommendations.

**4. Planner Hook (ra-pg-extension):**

Apply optimizations in PostgreSQL planner hook.

### Error Handling

```rust
#[derive(Debug, Error)]
pub enum PostgreSQLOptimizationError {
    #[error("JSONB containment rewrite failed: {0}")]
    JsonbRewriteError(String),

    #[error("TOAST column detection failed: {0}")]
    ToastDetectionError(String),

    #[error("Unsupported PostgreSQL version: {version}")]
    UnsupportedVersion { version: String },
}
```

### Performance Considerations

**JSONB Rewrite Performance:**

- Rewriting `data->>'key' = 'value'` to `data @> '{...}'` is fast (AST transformation)
- Cost: O(number of AND conditions)
- Benefit: 100x query speedup with GIN index

**TOAST Detection:**

- Requires querying `pg_attribute` (one-time cost)
- Cache results per table
- Cost: ~10ms per table

**Late Materialization:**

- Adds join overhead
- Benefit: Avoid reading large TOAST values
- Net win if: (rows before filter × TOAST cost) > (rows after filter × join cost)

## Drawbacks

**PostgreSQL-Specific:**

- Optimizations only work for PostgreSQL
- Must maintain separate code paths for other databases

**Complexity:**

- JSONB rewriting has edge cases (nested ANDs, ORs)
- TOAST detection requires PostgreSQL-specific queries
- Late materialization changes plan structure significantly

**Risk of Over-Optimization:**

- Aggressively rewriting queries may break user expectations
- JSONB containment may be slower than extraction for small tables
- Late materialization adds joins (overhead)

## Rationale and alternatives

### Why This Design?

**Rewrite-Based:**

- Transform non-indexable queries to indexable form
- Users write natural SQL, Ra makes it fast

**Type-Aware Cost Model:**

- Accurate costs for TOAST, XML parsing
- Better join/scan decisions

**Comprehensive:**

- Covers major PostgreSQL types (JSONB, arrays, XML, TOAST)
- Provides both rules and cost adjustments

### Alternative Approaches

**1. User Hints:**

- Users manually specify `data @>` instead of `data->>`
- **Rejected**: Requires PostgreSQL expertise, error-prone

**2. Ignore TOAST:**

- Treat all columns uniformly
- **Rejected**: Cost model would be inaccurate for large columns

**3. PostgreSQL-Only Ra:**

- Fork Ra specifically for PostgreSQL
- **Rejected**: Want multi-database support

### Impact of Not Doing This

**Without PostgreSQL-specific optimizations:**

- JSONB queries miss GIN indexes (100x slower)
- TOAST overhead not modeled (inaccurate costs)
- Users must manually optimize queries
- Ra provides less value for PostgreSQL workloads

## Prior art

### PostgreSQL Native Optimizer

- Automatically uses GIN indexes for `@>` operator
- Does not rewrite `data->>'key' = 'value'` to containment
- TOAST-aware cost model (built-in)
- No automatic late materialization for TOAST

### pg_hint_plan Extension

- Allows users to specify index hints
- No automatic query rewriting

### HypoPG Extension

- Creates hypothetical indexes for testing
- Used by index advisors

### What We Can Learn

- PostgreSQL's native optimizer is TOAST-aware (we should be too)
- Query rewriting for JSONB is not standard (we provide value here)
- GIN index selection is critical for performance

## Unresolved questions

**Design Questions:**

1. Should JSONB rewriting be opt-in or automatic?
2. How aggressive should late materialization be? (Always? Only for LIMIT queries?)
3. Should Ra suggest converting JSON to JSONB?

**Implementation Questions:**

1. How to test JSONB optimizations without running PostgreSQL?
2. Should TOAST detection be cached? For how long?
3. How to handle PostgreSQL version differences? (Some features are version-specific)

**Out of Scope:**

- **Full-text search optimization**: tsvector/tsquery (future work)
- **Range type optimization**: Complex, defer to RFC
- **Custom types**: User-defined types (out of scope)

## Future possibilities

### Natural Extensions

**1. Full-Text Search Optimization:**

- Optimize `tsvector @@ tsquery` with GIN indexes
- Suggest `to_tsvector()` on text columns

**2. Range Type Optimization:**

- Optimize range containment with GiST indexes
- Rewrite overlaps to indexed operations

**3. Partial JSONB Indexes:**

- Detect common filter patterns
- Suggest partial indexes: `WHERE data->>'status' = 'active'`

**4. JSONB Schema Extraction:**

- Analyze JSONB structure, suggest schema normalization
- "Your JSONB has consistent keys, consider separate columns"

### Long-term Vision

Ra becomes the **best PostgreSQL query optimizer**, surpassing the native optimizer by:

- Aggressive query rewriting (JSONB containment)
- Smart index recommendations (GIN, GiST, partial)
- TOAST-aware optimization (late materialization)
- Cross-query optimization (shared JSONB paths)

Integration with other RFCs:

- **RFC 0053 (Stored Procedures)**: Optimize JSONB in PL/pgSQL procedures
- **RFC 0054 (Streaming Plans)**: Adjust when GIN indexes are added
- **RFC 0055 (Type Support)**: Foundation for type-specific rules

This RFC makes Ra a powerful tool for PostgreSQL optimization.
