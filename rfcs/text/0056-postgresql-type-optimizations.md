# RFC 0056: PostgreSQL Type-Specific Optimizations

- Start Date: 2026-03-22
- Author: Ra Development Team
- Status: Proposed
- Tracking Issue: N/A

## Summary

Deep optimizations for PostgreSQL's advanced type system: JSONB, XML, arrays,
and TOAST (The Oversized-Attribute Storage Technique). This RFC builds on
RFC 0055 (general type support) with PostgreSQL-specific rewrite rules, cost
model adjustments, index recommendations, and late materialization strategies.
The goal is to close the gap between how Ra handles these types today (opaque
columns) and how the PostgreSQL executor actually processes them.

## Motivation

PostgreSQL has the richest type system among production relational databases.
Five categories demand optimizer-level awareness:

| Type feature   | Why it matters                                         |
|----------------|--------------------------------------------------------|
| JSONB          | Binary JSON with GIN indexing; `@>` is O(1) vs O(n) extraction |
| XML            | Native XML with XPath; parsing cost 10-100x text ops   |
| Arrays         | First-class arrays with GIN overlap/containment         |
| TOAST          | Automatic out-of-line storage for values >2 KB          |
| Full-text      | tsvector/tsquery with GIN; separate from LIKE optimization |

**Problems with generic handling:**

1. **JSONB**: `data->>'status' = 'active'` cannot use a GIN index.
   Rewriting to `data @> '{"status":"active"}'` enables index scan.
   Measured difference on 10M rows: 1.2 s vs 12 ms (100x).

2. **TOAST**: A `SELECT *` that fetches a 50 KB text column stored in TOAST
   pays 2-3x I/O per row. Projection pushdown that eliminates the column
   avoids all TOAST I/O.

3. **XML**: XPath evaluation on a 10 KB document costs ~10 ms. Without this
   in the cost model, the optimizer may choose a plan that evaluates XPath on
   every row instead of filtering first.

4. **Arrays**: `WHERE tags @> ARRAY['pg']` benefits from GIN, but Ra treats
   the `@>` operator generically and does not recommend index creation.

**Expected performance impact:**

| Optimization                      | Workload type        | Estimated gain |
|-----------------------------------|----------------------|----------------|
| JSONB containment rewrite         | JSONB filter queries | 50-100x        |
| GIN index recommendation          | JSONB/array queries  | 10-100x        |
| TOAST-aware projection pushdown   | Wide tables          | 2-5x           |
| TOAST late materialization        | LIMIT + large cols   | 5-20x          |
| XML cost model adjustment         | XML-heavy queries    | 2-10x (plan quality) |

## Guide-level explanation

### JSONB: Containment Rewrite

Users write natural extraction-based predicates. Ra rewrites them into
indexable containment form.

**Before optimization:**

```sql
-- Three separate JSON extractions; no GIN index can help
SELECT id, data->>'name', data->>'email'
FROM users
WHERE data->>'status' = 'active'
  AND data->>'verified' = 'true'
  AND data->>'country' = 'US';
```

Execution plan: Seq Scan on users, Filter on three `->>`  calls.

**After optimization:**

```sql
-- Single containment check; GIN index eligible
SELECT id, data->>'name', data->>'email'
FROM users
WHERE data @> '{"status":"active","verified":"true","country":"US"}';
```

Execution plan: Bitmap Index Scan using GIN index on `data`, then Bitmap
Heap Scan. The three extraction predicates collapse into a single `@>`
operator.

Ra also recommends the GIN index if one does not exist:

```sql
CREATE INDEX idx_users_data_gin ON users USING GIN (data);
```

For workloads that filter on a small set of keys, Ra can recommend a partial
GIN index or a B-tree expression index on the extracted key:

```sql
-- Partial GIN for common filter patterns
CREATE INDEX idx_users_active ON users USING GIN (data)
  WHERE data->>'status' = 'active';

-- Expression B-tree for equality on a single key
CREATE INDEX idx_users_country ON users ((data->>'country'));
```

### TOAST: Late Materialization

Consider a table with a TOAST-eligible column:

```sql
CREATE TABLE articles (
    id        SERIAL PRIMARY KEY,
    title     TEXT,
    status    TEXT,
    body      TEXT,           -- average 50 KB, stored in TOAST
    created   TIMESTAMPTZ
);
```

**Query that triggers TOAST reads unnecessarily:**

```sql
SELECT * FROM articles
WHERE status = 'published'
ORDER BY created DESC
LIMIT 10;
```

Without TOAST awareness, PostgreSQL fetches `body` for every candidate row
before applying ORDER BY and LIMIT. For 100K published articles, that means
~5 GB of TOAST reads.

**With TOAST-aware late materialization:**

Ra rewrites the plan so that TOAST columns are fetched only for the final
10 rows:

```
Nested Loop (10 rows)
  -> Limit 10
       -> Sort (created DESC)
            -> Seq Scan on articles (filter: status = 'published')
               Output: id, title, status, created  -- no body
  -> Index Scan on articles_pkey (fetch body for each of 10 rows)
```

This reduces TOAST I/O from 100K detoast operations to 10.

### XML: Cost Model Integration

```sql
SELECT doc_id, xpath('//author/text()', xmldoc) AS author
FROM documents
WHERE xpath_exists('//published[@year="2025"]', xmldoc);
```

Ra assigns an XPath evaluation cost proportional to average document size.
This causes the optimizer to prefer plans that filter on cheaper predicates
first, then evaluate XPath only on surviving rows.

## Reference-level explanation

### JSONB Optimization Rules

#### Rule 1: Extraction-to-Containment Rewrite

**Pattern:**

```
data->>'key' = 'value'  -->  data @> '{"key": "value"}'
```

**Applicability conditions:**

- Left side is `Expr::JsonExtractText` (the `->>` operator)
- Right side is a string constant
- The JSONB column has (or could benefit from) a GIN index

**Combining multiple predicates:**

When multiple extraction equality predicates target the same JSONB column
and are ANDed together, combine them into a single containment object:

```
data->>'k1' = 'v1' AND data->>'k2' = 'v2'
-->
data @> '{"k1":"v1","k2":"v2"}'
```

This is semantically valid because `@>` checks that the left operand
contains the right operand as a subset.

**Implementation sketch:**

```rust
pub fn rewrite_jsonb_extraction_to_containment(
    expr: &Expr,
    column: &str,
) -> Option<Expr> {
    // Collect all (path, value) pairs from ANDed conditions
    // on the same JSONB column
    let pairs = collect_jsonb_equality_pairs(expr, column);
    if pairs.is_empty() {
        return None;
    }

    // Build the containment object
    let mut obj = serde_json::Map::new();
    for (path, value) in &pairs {
        obj.insert(path.clone(), json!(value));
    }

    Some(Expr::BinOp {
        op: Op::JsonContains,
        left: Box::new(Expr::Column(column.into())),
        right: Box::new(Expr::Const(
            Const::Jsonb(serde_json::Value::Object(obj)),
        )),
    })
}
```

**Restrictions:**

- Does NOT apply to `!=`, `<`, `>`, `LIKE`, or `IS NULL` predicates
  on extracted values (these have no containment equivalent)
- Does NOT apply when the right side is a non-constant expression
- Does NOT apply to nested path extraction (`data->'a'->>'b'`)
  without first normalizing the path

**Cost of the rewrite itself:** O(p) where p = number of ANDed predicates.
Negligible relative to query execution.

#### Rule 2: GIN Index Recommendation

Trigger conditions (any of):

- Query uses `@>` (containment) on a JSONB or array column
- Query uses `?` (key existence) on a JSONB column
- Query uses `@?` or `@@` (jsonpath) on a JSONB column
- Rule 1 rewrote extraction predicates to containment

Recommendation logic:

```rust
fn recommend_jsonb_index(
    table: &str,
    column: &str,
    predicates: &[Expr],
    existing_indexes: &[IndexMetadata],
) -> Vec<IndexRecommendation> {
    let mut recs = Vec::new();

    // Skip if GIN index already exists on this column
    let has_gin = existing_indexes.iter().any(|idx| {
        matches!(&idx.index_type,
            IndexType::GIN { column: c, .. } if c == column)
    });
    if has_gin {
        return recs;
    }

    // Full GIN index for general containment
    recs.push(IndexRecommendation {
        index_type: IndexType::GIN {
            column: column.into(),
            opclass: "jsonb_ops".into(),
        },
        sql: format!(
            "CREATE INDEX idx_{}_{}_gin ON {} USING GIN ({});",
            table, column, table, column,
        ),
        estimated_speedup: 100.0,
    });

    // For queries on few specific keys, also suggest
    // jsonb_path_ops (smaller, faster for containment only)
    let keys = extract_queried_keys(predicates);
    if keys.len() <= 3 {
        recs.push(IndexRecommendation {
            index_type: IndexType::GIN {
                column: column.into(),
                opclass: "jsonb_path_ops".into(),
            },
            sql: format!(
                "CREATE INDEX idx_{}_{}_pathops ON {} \
                 USING GIN ({} jsonb_path_ops);",
                table, column, table, column,
            ),
            estimated_speedup: 120.0,
        });
    }

    recs
}
```

**`jsonb_ops` vs `jsonb_path_ops`:**

| Operator class   | Supports `?` (exists) | Supports `@>` | Index size |
|------------------|-----------------------|---------------|------------|
| `jsonb_ops`      | Yes                   | Yes           | Larger     |
| `jsonb_path_ops` | No                    | Yes           | 2-3x smaller |

Ra recommends `jsonb_path_ops` when the workload uses only `@>` and never
`?`, `?|`, or `?&`.

#### Rule 3: Partial Index Suggestion

When the optimizer detects a frequently filtered JSONB value (via
statistics or workload analysis), it can suggest a partial index:

```sql
-- Workload frequently filters status = 'active'
CREATE INDEX idx_users_active_gin ON users USING GIN (data)
  WHERE data @> '{"status":"active"}';
```

Partial indexes are smaller and faster to maintain. Ra recommends them when:

- A predicate appears in >30% of queries referencing the table
- The predicate selectivity is <20% (filters out most rows)
- The table has >100K rows

### TOAST Awareness

#### Background: How TOAST Works

PostgreSQL stores row data in 8 KB pages. When a single row exceeds ~2 KB
of variable-length data, PostgreSQL uses TOAST to store oversized attributes
out-of-line:

1. **Compression** (`attstorage = 'm'` or `'x'`): Compress the value
   in-place using LZ compression. If the result fits in-line, stop.
2. **Out-of-line storage** (`attstorage = 'x'` or `'e'`): Move the value
   to a separate TOAST table. The main row stores a ~20-byte TOAST pointer.
3. **Chunk storage**: The TOAST table stores values in ~2 KB chunks, each
   identified by `(chunk_id, chunk_seq)`.

**I/O cost of detoasting:**

| Operation             | Cost                                         |
|-----------------------|----------------------------------------------|
| Read TOAST pointer    | Included in main table scan (no extra I/O)   |
| Follow pointer        | 1 random read to TOAST table index           |
| Read chunks           | ceil(value_size / 2000) sequential reads     |
| Decompress            | CPU-bound; ~0.5 us per KB                    |

For a 50 KB column, detoasting costs: 1 random read + 25 sequential reads.

#### Rule 1: TOAST Column Detection

Query `pg_attribute` to identify TOAST-eligible columns:

```sql
SELECT a.attname,
       a.attstorage,
       s.avg_width
FROM pg_attribute a
JOIN pg_stats s
  ON s.tablename = a.attrelid::regclass::text
 AND s.attname = a.attname
WHERE a.attrelid = 'articles'::regclass
  AND a.attstorage IN ('x', 'e', 'm')
  AND a.atttypid IN (
      'text'::regtype, 'bytea'::regtype,
      'jsonb'::regtype, 'json'::regtype, 'xml'::regtype
  )
  AND s.avg_width > 2048;
```

`attstorage` values:

| Value | Name     | Behavior                                    |
|-------|----------|---------------------------------------------|
| `p`   | Plain    | Never compressed, never out-of-line          |
| `m`   | Main     | Compressed in-place, never out-of-line       |
| `x`   | Extended | Compressed first, then moved out-of-line     |
| `e`   | External | Moved out-of-line without compression         |

Columns with `attstorage IN ('x', 'e')` are candidates for out-of-line
storage. The `avg_width` from `pg_stats` tells us whether values are
*actually* being TOASTed (average > 2 KB).

**Caching:** TOAST column metadata is cached per table and invalidated on
DDL changes. The cache TTL matches the statistics refresh interval.

```rust
pub struct ToastColumnInfo {
    pub column_name: String,
    pub storage_type: char,       // 'x', 'e', 'm', 'p'
    pub avg_width: usize,         // from pg_stats
    pub is_out_of_line: bool,     // storage in ('x','e') AND avg > 2048
    pub estimated_chunks: usize,  // ceil(avg_width / 2000)
}
```

#### Rule 2: TOAST-Aware Projection Pushdown

When a query does not reference a TOAST-eligible column in its output or
predicates, ensure the scan operator excludes it from its projection list.

This is standard projection pushdown, but TOAST awareness changes the
priority: eliminating a TOASTed column from a scan saves 2-3x more I/O
per row than eliminating an inline column.

**Cost formula:**

```
scan_cost(table, columns) =
    base_row_cost * cardinality
  + SUM over toasted_columns_in(columns):
        cardinality * (
            random_io_cost                     -- TOAST index lookup
          + estimated_chunks * seq_io_cost     -- chunk reads
          + avg_width * decompress_cost_per_byte
        )
```

Where:

| Parameter                   | Default value | Source                 |
|-----------------------------|---------------|------------------------|
| `base_row_cost`             | 0.01          | Hardware profile       |
| `random_io_cost`            | 4.0           | Hardware profile (SSD) |
| `seq_io_cost`               | 1.0           | Hardware profile       |
| `decompress_cost_per_byte`  | 0.0000005     | Calibrated             |
| `estimated_chunks`          | ceil(avg_width / 2000) | pg_stats      |

**Example calculation:**

Table `articles` with 100K rows, `body` column with avg_width = 50,000:

- Without TOAST column: 100K * 0.01 = 1,000
- With TOAST column: 1,000 + 100K * (4.0 + 25 * 1.0 + 50000 * 0.0000005)
  = 1,000 + 100K * 29.025 = 2,903,500
- **TOAST overhead: 2,903x the base scan cost**

This makes TOAST-aware projection pushdown one of the highest-value
optimizations for wide tables.

#### Rule 3: Late Materialization for TOASTed Columns

When a query includes TOASTed columns in its output but applies filtering,
sorting, or LIMIT before the columns are needed, defer TOAST column reads.

**Triggering conditions:**

1. Query projects TOASTed columns
2. Query has a filter or LIMIT that reduces cardinality by >50%
3. The table has a primary key or unique index for the re-join

**Rewrite pattern:**

```
Project [id, title, body]              Project [id, title, body]
  |                                      |
  Sort [created DESC]          -->     NestedLoop (pk join)
    |                                    |            |
    Filter [status='pub']             Limit 10     IndexScan(pk)
      |                                  |           [fetch body]
      SeqScan [articles]              Sort [created]
                                         |
                                       Filter [status]
                                         |
                                       SeqScan [articles]
                                         [id, title, status, created]
```

The left branch fetches only non-TOASTed columns through filter + sort +
limit. The right branch fetches TOASTed columns only for the surviving
rows via primary key lookup.

**Net benefit formula:**

```
benefit = rows_before_filter * toast_cost_per_row
        - rows_after_filter * (toast_cost_per_row + pk_lookup_cost)
```

Apply the rewrite only when `benefit > 0`. For the articles example with
100K rows filtered to 10:

```
benefit = 100,000 * 29.025 - 10 * (29.025 + 4.0)
        = 2,902,500 - 330.25
        = 2,902,170  (apply the rewrite)
```

### XML Optimization

#### Cost Model for XPath Operations

XML operations are CPU-intensive because they require DOM or SAX parsing.
The cost is proportional to document size and XPath complexity.

**Cost formula:**

```
xpath_cost(doc_size, xpath_depth) =
    doc_size * parse_cost_per_byte
  + xpath_depth * node_traversal_cost
```

| Parameter                | Default value | Notes                           |
|--------------------------|---------------|---------------------------------|
| `parse_cost_per_byte`    | 0.001         | ~1 ms per KB of XML             |
| `node_traversal_cost`    | 0.1           | Per level of XPath nesting      |

For a 10 KB document with XPath depth 3:

```
cost = 10000 * 0.001 + 3 * 0.1 = 10.3
```

This is 1,000x more expensive than a text comparison (cost ~0.01). The
optimizer uses this to prefer plans that apply cheap predicates before
XPath evaluation.

#### Predicate Ordering with XML

When a query has both XML and non-XML predicates, ensure non-XML predicates
evaluate first:

```sql
-- Original
SELECT doc_id FROM docs
WHERE xpath_exists('//author[@name="Smith"]', doc)
  AND category = 'research';

-- Optimizer reorders: cheap predicate first
-- Plan: SeqScan with filter (category = 'research')
--        then filter (xpath_exists(...))
```

This is standard predicate reordering, but the XML cost model provides
accurate selectivity-adjusted cost that drives the correct ordering.

### Array Optimization

#### GIN Index for Containment

PostgreSQL arrays support `@>` (contains), `<@` (contained by), and
`&&` (overlap) operators. All three benefit from GIN indexes.

```sql
-- Query
SELECT * FROM posts WHERE tags @> ARRAY['postgresql', 'optimization'];

-- Recommended index
CREATE INDEX idx_posts_tags_gin ON posts USING GIN (tags);
```

#### Array Unnest Optimization

For queries that unnest arrays and then aggregate:

```sql
SELECT tag, COUNT(*)
FROM posts, unnest(tags) AS tag
GROUP BY tag;
```

If the table has a GIN index on `tags`, Ra can consider a GIN-scan-based
plan that iterates index entries directly rather than scanning all rows and
unnesting.

### Integration Points

#### 1. Query Parser

Register PostgreSQL-specific operators in the parser:

```rust
// JSONB operators
parser.register_infix("@>",  Prec::Comparison, Op::JsonContains);
parser.register_infix("<@",  Prec::Comparison, Op::JsonContainedBy);
parser.register_infix("->>", Prec::Primary,    Op::JsonExtractText);
parser.register_infix("->",  Prec::Primary,    Op::JsonExtract);
parser.register_infix("@?",  Prec::Comparison, Op::JsonPathQuery);
parser.register_infix("@@",  Prec::Comparison, Op::JsonPathMatch);

// Array operators
parser.register_infix("&&",  Prec::Comparison, Op::ArrayOverlap);
```

#### 2. Statistics Collection

Extend the statistics bridge (`ra-pg-extension/src/stats_bridge.rs`) to
collect:

- TOAST column metadata from `pg_attribute`
- Average JSONB key frequency from `pg_stats.most_common_elems`
- Array length distribution from `pg_stats.avg_width` and element type

#### 3. Cost Model (`ra-engine`)

The existing `estimate_plan_cost` function in `crates/ra-engine/src/egraph.rs`
must incorporate TOAST overhead. The `HardwareCostModel` already supports
per-operation costs; TOAST adds a new category of I/O cost.

#### 4. Index Advisor (RFC 0021)

The existing `IndexType::GIN` variant in `crates/ra-stats/src/index_types.rs`
already supports `column` and `opclass` fields. The JSONB optimization rules
produce `IndexRecommendation` values that reference this type.

#### 5. Planner Hook (`ra-pg-extension`)

The planner hook applies JSONB rewrites and TOAST-aware plan modifications
before passing the optimized plan to PostgreSQL's executor. The existing
detoasting code in `stats_bridge.rs` (line 30-33) already handles the
mechanics of following TOAST pointers.

### Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum PgTypeOptError {
    #[error(
        "JSONB containment rewrite failed for column {column}: {reason}"
    )]
    JsonbRewriteFailed {
        column: String,
        reason: String,
    },

    #[error(
        "TOAST detection failed for table {table}: {source}"
    )]
    ToastDetection {
        table: String,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "PostgreSQL version {version} does not support {feature}"
    )]
    UnsupportedVersion {
        version: String,
        feature: String,
    },
}
```

All optimization errors are non-fatal. If a rewrite fails, the original
plan is preserved unchanged. Errors are logged at `warn!` level.

## Drawbacks

**PostgreSQL lock-in.** Every rule in this RFC is PostgreSQL-specific. Other
databases have analogous features (MySQL JSON, Oracle XMLTYPE) but different
syntax, index types, and storage internals. The code cannot be reused
directly.

**Cost model calibration.** The default cost parameters (random I/O = 4.0,
TOAST chunk read = 1.0) assume modern SSDs. HDDs, cloud storage, and
NVMe have different characteristics. Incorrect calibration can lead to
wrong plan choices. Mitigation: integrate with RFC 0026 (Adaptive Cost
Calibration) to learn parameters from runtime feedback.

**Rewrite correctness edge cases.** The JSONB containment rewrite changes
semantics in subtle ways:

- `data->>'key' = 'value'` returns `NULL` when `key` is missing.
  `data @> '{"key":"value"}'` returns `FALSE`. The rewrite is only
  safe when the column is known non-null for the key, or when the
  query already filters out nulls.
- Numeric coercion: `data->>'count' = '5'` is string comparison.
  `data @> '{"count": 5}'` uses JSONB numeric comparison. These
  differ for values like `'05'`.

**Late materialization overhead.** The re-join adds a nested loop with
primary key lookups. For queries that do not reduce cardinality (no filter
or LIMIT), late materialization is a net loss. The benefit formula must be
evaluated carefully.

## Rationale and alternatives

### Why This Design?

**Rewrite-based approach.** Users write natural SQL using `->>`  extraction.
Ra transparently rewrites to `@>` containment. This is invisible to users
and requires no query changes. The alternative (expecting users to write
`@>` directly) requires PostgreSQL expertise and is error-prone.

**Type-aware cost model.** Without TOAST awareness, the cost model treats
a 50 KB text column the same as a 4-byte integer. This leads to plans
that fetch unnecessary TOAST data. By integrating TOAST I/O cost into
the scan cost formula, the optimizer naturally prefers projection pushdown
and late materialization.

**Layered on RFC 0055.** RFC 0055 provides the type system (`PostgreSQLType::Jsonb`,
etc.) and operator definitions. This RFC adds optimization rules that use
those types. The separation keeps the type system clean and the optimization
rules self-contained.

### Alternative Approaches

**1. User-provided hints.** Users annotate queries with optimization hints
(`/*+ USE_GIN_INDEX(data) */`). This requires expertise and breaks when
schemas change. Rejected in favor of automatic optimization.

**2. Ignore TOAST entirely.** Treat all columns as inline. This is what Ra
does today. The cost model underestimates scan cost by 3,000x for tables
with large TOAST columns. Rejected because plan quality degrades.

**3. Full TOAST detoasting in Ra.** Ra could detoast values itself rather
than letting PostgreSQL do it. This would require duplicating PostgreSQL's
TOAST access code. Rejected because it adds complexity without benefit;
PostgreSQL already detoasts efficiently.

### Impact of Not Doing This

- JSONB queries miss GIN indexes: 100x slower on indexed workloads
- TOAST overhead unmodeled: plan quality degrades on wide tables
- Ra provides less value for PostgreSQL-specific workloads than
  PostgreSQL's native optimizer (which is already TOAST-aware)

## Prior art

### PostgreSQL Native Optimizer

- **TOAST-aware cost model**: `cost_qual_eval` in PostgreSQL source
  (`src/backend/optimizer/path/costsize.c`) adds per-column width
  to scan cost estimates. It does not perform late materialization.
- **GIN index selection**: The planner uses GIN indexes for `@>`, `?`,
  `?|`, `?&` operators automatically, but does NOT rewrite `->>`
  extraction to containment form.
- **No automatic late materialization**: PostgreSQL does not defer TOAST
  reads past filter/sort/limit boundaries.

### CitusDB

- Distributes JSONB queries across shards
- Uses PostgreSQL's native JSONB optimization within each shard
- Does not add JSONB-specific rewrite rules

### TimescaleDB

- Compression-aware cost model for time-series data
- Similar in spirit to TOAST awareness: compressed chunks have different
  I/O costs than uncompressed
- Does not handle JSONB specifically

### Apache Calcite

- Extensible type system via `RelDataType`
- JSON support through `JSON_VALUE` / `JSON_QUERY` functions
- No TOAST equivalent (Calcite is not a storage engine)
- Does not rewrite JSON extraction to containment

### DuckDB

- Native JSON extension with automatic type detection
- No TOAST (DuckDB uses columnar storage; large values are always
  compressed in-band)
- JSON extraction is pushed down into the scan operator

### Key Insight from Prior Art

PostgreSQL's native optimizer is TOAST-aware in its cost model but does
not perform late materialization or JSONB containment rewrites. These are
genuine optimization opportunities where Ra adds value beyond what
PostgreSQL provides natively.

## Unresolved questions

**Design questions:**

1. **JSONB rewrite safety**: Should the rewrite be applied only when
   `pg_stats` confirms the key exists in >95% of rows? Or always, with
   a warning about NULL semantics?

2. **Late materialization threshold**: What cardinality reduction ratio
   justifies the re-join overhead? Initial proposal: >50% reduction.
   Needs benchmarking.

3. **TOAST detection frequency**: Should Ra re-check `pg_attribute` on
   every optimization, or cache and refresh periodically?

**Implementation questions:**

1. How to test JSONB optimizations without a running PostgreSQL instance?
   Options: mock statistics, embedded pg_regress, or snapshot-based tests.

2. Should TOAST cost parameters be part of the hardware profile
   (`HardwareCostModel`) or a separate PostgreSQL-specific profile?

3. How to handle PostgreSQL version differences? `jsonb_path_ops` was
   added in 9.4; `jsonpath` operators (`@?`, `@@`) in 12.

**Out of scope:**

- Full-text search optimization (tsvector/tsquery): separate RFC
- Range type optimization (int4range, tsrange): separate RFC
- PostGIS spatial optimization: separate RFC
- User-defined types (`CREATE TYPE`): separate RFC

## Future possibilities

### Natural Extensions

**1. Full-text search optimization.** Optimize `tsvector @@ tsquery` with
GIN indexes. Suggest `to_tsvector()` on text columns that are frequently
used with `LIKE '%pattern%'`.

**2. JSONB schema inference.** Analyze JSONB statistics to detect consistent
key patterns. Recommend normalizing JSONB into relational columns when keys
are present in >99% of rows and the table exceeds 1M rows.

**3. TOAST-aware join ordering.** When joining two tables where one has
TOAST columns, prefer the join order that filters first and projects TOAST
columns last.

**4. Adaptive TOAST cost calibration.** Use RFC 0026 (Adaptive Cost
Calibration) to learn actual TOAST I/O costs from runtime feedback.
Different hardware (SSD vs HDD vs cloud block storage) has different
random I/O profiles that affect TOAST read costs.

### Long-term Vision

Ra becomes the best query optimizer for PostgreSQL workloads by:

- Rewriting JSONB predicates to indexable form (not done by native optimizer)
- Recommending GIN indexes with the right operator class
- Modeling TOAST I/O cost accurately
- Performing late materialization to avoid unnecessary TOAST reads

Integration with other RFCs:

- **RFC 0021 (Index Advisor)**: JSONB/array GIN index recommendations
- **RFC 0026 (Adaptive Cost Calibration)**: Learn TOAST cost parameters
- **RFC 0052 (Progressive Reoptimization)**: TOAST-aware plans in early
  optimization phases
- **RFC 0055 (Type Support)**: Foundation type system and operators
- **RFC 0057 (Cross-DB Type Adaptation)**: Map PostgreSQL JSONB to other
  databases' JSON support
