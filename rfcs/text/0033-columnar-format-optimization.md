# RFC 0033: Columnar Format Optimization System

- **Start Date**: 2026-03-20
- **Author**: RA Core Team
- **Status**: Draft
- **Tracking Issue**: TBD

---

## Summary

Add first-class support for columnar file formats (Parquet, ORC, Arrow) to enable 10-1000x query speedups through metadata-driven optimizations: column pruning, predicate pushdown to row groups, and statistics propagation.

---

## Motivation

### Current Limitations

RA treats all tables as opaque, requiring `ANALYZE` to gather statistics. This has several problems:

1. **ANALYZE is slow**: Scanning 1TB table takes hours
2. **Statistics become stale**: Need periodic re-analysis
3. **No file format awareness**: Can't leverage Parquet metadata
4. **Poor data lake performance**: 100-1000x slower than DuckDB on Parquet

### Why This Matters

Modern data lakes use Parquet/ORC for storage. These formats embed:
- **Column statistics** (min/max/null_count) per row group
- **Bloom filters** for fast set membership
- **Schema metadata** for projection pushdown
- **Encoding information** (dictionary, RLE)

**DuckDB demonstrates**: Parquet metadata enables 100-1000x speedups over naive scans.

### Use Cases

1. **Data Lake Querying**:
   ```sql
   -- 1TB Parquet table, 1000 row groups
   SELECT AVG(amount) FROM sales WHERE date = '2023-01-15';
   -- Without optimization: Scan 1TB (300s)
   -- With Parquet pushdown: Scan 1GB (0.3s)
   -- Speedup: 1000x
   ```

2. **Zero-Cost Statistics**:
   ```sql
   -- Traditional: ANALYZE sales (scan 1TB, 1 hour)
   -- Parquet: Read footer metadata (4KB, 0.001s)
   ```

3. **Partition Pruning**:
   ```
   Files: sales_2023-01-01.parquet, sales_2023-01-02.parquet, ...
   Query: WHERE date = '2023-01-15'
   → Skip 364/365 files (99.7% less I/O)
   ```

---

## Guide-Level Explanation

### How It Works

When you query a Parquet file, RA automatically optimizes the plan:

```sql
-- Query
SELECT name, email FROM 'users.parquet' WHERE age > 50;

-- Logical Plan
Project[name, email]
  Filter[age > 50]
    Scan[users.parquet]

-- Optimized Physical Plan (automatic)
ParquetScan[
  file="users.parquet",
  columns=[name, email, age],        # Column pruning (only needed cols)
  predicate=(age > 50),                # Predicate pushdown (skip row groups)
  row_groups=[2, 5, 7, 9, 15, ...]    # Filtered by statistics
]
```

### What Changes

1. **No ANALYZE needed**: Statistics come from file metadata
2. **Automatic optimizations**: Column pruning, predicate pushdown
3. **Transparent**: Existing queries get faster, no code changes
4. **Cost model aware**: Planner knows Parquet scans are cheap

### User Experience

```rust
// Before (CSV, slow)
let plan = optimizer.optimize("SELECT * FROM 'data.csv' WHERE x > 100")?;
// Seq Scan on data.csv (cost=10000..50000 rows=1000000)

// After (Parquet, fast)
let plan = optimizer.optimize("SELECT * FROM 'data.parquet' WHERE x > 100")?;
// Parquet Scan on data.parquet (cost=10..50 rows=1000)
//   Row Groups: 5/100 (95% skipped via statistics)
//   Columns: [x] (90% less I/O)
```

---

## Reference-Level Explanation

### Architecture

```
┌─────────────────────────────────────────────────────┐
│  ra-core/formats/                                   │
│    ├─ mod.rs        (FileFormat trait)              │
│    ├─ parquet.rs    (ParquetFormat impl)            │
│    ├─ orc.rs        (ORCFormat impl)                │
│    └─ arrow.rs      (ArrowIPCFormat impl)           │
└─────────────────────────────────────────────────────┘
           ↓
┌─────────────────────────────────────────────────────┐
│  ra-stats/                                          │
│    ├─ file_stats.rs (Populate from file metadata)   │
│    └─ staleness.rs  (File mtime = last_analyzed)    │
└─────────────────────────────────────────────────────┘
           ↓
┌─────────────────────────────────────────────────────┐
│  ra-engine/rewrite/                                 │
│    ├─ column_pruning.rs                             │
│    └─ predicate_pushdown.rs                         │
└─────────────────────────────────────────────────────┘
```

### FileFormat Trait

```rust
/// Abstraction over columnar file formats.
pub trait FileFormat: Send + Sync + Debug {
    /// Format name (e.g., "parquet", "orc")
    fn name(&self) -> &str;

    /// Read schema without scanning data.
    ///
    /// For Parquet: Read footer (last 4KB of file)
    /// Cost: O(1), ~1ms
    fn read_schema(&self, path: &Path) -> Result<Schema>;

    /// Read file metadata (statistics, row groups).
    ///
    /// For Parquet: Parse footer metadata
    /// Returns: Min/max/null_count per column per row group
    fn read_metadata(&self, path: &Path) -> Result<FileMetadata>;

    /// Check if format supports optimization.
    fn capabilities(&self) -> FormatCapabilities;

    /// Scan file with optimizations.
    ///
    /// # Arguments
    /// - `projection`: Only read these columns
    /// - `predicate`: Push down to row group filtering
    /// - `row_group_filter`: Pre-filtered row groups (from planner)
    fn scan(&self, path: &Path, options: ScanOptions) -> Result<RecordBatchStream>;
}

pub struct FormatCapabilities {
    pub column_pruning: bool,
    pub predicate_pushdown: bool,
    pub column_statistics: bool,
    pub bloom_filters: bool,
    pub nested_columns: bool,
}

pub struct ScanOptions {
    pub projection: Vec<String>,
    pub predicate: Option<Expr>,
    pub row_group_filter: Option<Vec<usize>>,
    pub bloom_filter_columns: Vec<String>,
}
```

### FileMetadata Structure

```rust
pub struct FileMetadata {
    /// Schema
    pub schema: Schema,

    /// Total rows across all row groups
    pub num_rows: u64,

    /// Row groups (Parquet) / Stripes (ORC)
    pub row_groups: Vec<RowGroupMeta>,

    /// File-level aggregated statistics
    pub file_stats: HashMap<String, ColumnStats>,

    /// Bloom filters (optional, per column)
    pub bloom_filters: HashMap<String, BloomFilter>,

    /// File modification time (for staleness tracking)
    pub mtime: SystemTime,
}

pub struct RowGroupMeta {
    /// Row group index (0-based)
    pub index: usize,

    /// Byte offset in file
    pub offset: u64,

    /// Number of rows in this group
    pub num_rows: u64,

    /// Per-column statistics
    pub column_stats: HashMap<String, ColumnStats>,

    /// Compressed size in bytes
    pub compressed_size: u64,

    /// Uncompressed size in bytes
    pub uncompressed_size: u64,
}

pub struct ColumnStats {
    /// Minimum value (None if all NULL)
    pub min: Option<ScalarValue>,

    /// Maximum value (None if all NULL)
    pub max: Option<ScalarValue>,

    /// Number of NULL values
    pub null_count: u64,

    /// Distinct value count (optional, not all formats)
    pub distinct_count: Option<u64>,

    /// Min string length (for VARCHAR)
    pub min_len: Option<u32>,

    /// Max string length
    pub max_len: Option<u32>,
}
```

### Parquet Implementation

```rust
pub struct ParquetFormat {
    // Configuration options
}

impl FileFormat for ParquetFormat {
    fn name(&self) -> &str {
        "parquet"
    }

    fn read_metadata(&self, path: &Path) -> Result<FileMetadata> {
        // 1. Read Parquet footer (last 4KB)
        let file = File::open(path)?;
        let reader = SerializedFileReader::new(file)?;
        let parquet_meta = reader.metadata();

        // 2. Extract row group statistics
        let mut row_groups = Vec::new();
        for (i, rg) in parquet_meta.row_groups().iter().enumerate() {
            let mut column_stats = HashMap::new();
            for col_chunk in rg.columns() {
                if let Some(stats) = col_chunk.statistics() {
                    column_stats.insert(
                        col_chunk.column_path().string(),
                        ColumnStats {
                            min: stats.min_bytes().map(|b| decode_value(b, col_chunk.column_type())),
                            max: stats.max_bytes().map(|b| decode_value(b, col_chunk.column_type())),
                            null_count: stats.null_count(),
                            distinct_count: stats.distinct_count(),
                            min_len: None, // Parquet doesn't track
                            max_len: None,
                        },
                    );
                }
            }
            row_groups.push(RowGroupMeta {
                index: i,
                offset: rg.file_offset(),
                num_rows: rg.num_rows() as u64,
                column_stats,
                compressed_size: rg.compressed_size() as u64,
                uncompressed_size: rg.total_byte_size() as u64,
            });
        }

        Ok(FileMetadata {
            schema: convert_parquet_schema(parquet_meta.file_metadata().schema()),
            num_rows: parquet_meta.file_metadata().num_rows() as u64,
            row_groups,
            file_stats: aggregate_stats(&row_groups),
            bloom_filters: read_bloom_filters(&reader)?,
            mtime: file.metadata()?.modified()?,
        })
    }

    fn scan(&self, path: &Path, options: ScanOptions) -> Result<RecordBatchStream> {
        // 1. Open Parquet file
        let file = File::open(path)?;
        let reader = ParquetRecordBatchReaderBuilder::try_new(file)?;

        // 2. Apply column projection
        if !options.projection.is_empty() {
            reader = reader.with_projection(mask_from_columns(&options.projection)?);
        }

        // 3. Apply row group filtering
        if let Some(row_groups) = options.row_group_filter {
            reader = reader.with_row_groups(row_groups);
        }

        // 4. Build arrow reader (predicate applied as post-filter)
        let arrow_reader = reader.build()?;

        // 5. Wrap with predicate filter if needed
        Ok(if let Some(pred) = options.predicate {
            Box::pin(FilterRecordBatchStream::new(arrow_reader, pred))
        } else {
            Box::pin(arrow_reader)
        })
    }

    fn capabilities(&self) -> FormatCapabilities {
        FormatCapabilities {
            column_pruning: true,
            predicate_pushdown: true,
            column_statistics: true,
            bloom_filters: true, // If available
            nested_columns: true,
        }
    }
}
```

### Integration with ra-stats

Extend `StatisticsProvider` to read from file metadata:

```rust
pub struct FileStatisticsProvider {
    file_formats: HashMap<String, Box<dyn FileFormat>>,
    file_cache: RwLock<HashMap<PathBuf, (FileMetadata, SystemTime)>>,
}

impl StatisticsProvider for FileStatisticsProvider {
    fn get_statistics(&self, table: &str) -> Option<Statistics> {
        // 1. Determine if table is a file path
        let path = Path::new(table);
        if !path.exists() {
            return None;
        }

        // 2. Detect file format from extension
        let format = self.detect_format(path)?;

        // 3. Read metadata (cached)
        let meta = self.read_metadata_cached(path, format)?;

        // 4. Convert to Statistics
        Some(Statistics {
            row_count: meta.num_rows,
            column_stats: meta.file_stats.into_iter().map(|(col, stats)| {
                (col, ColumnStatistics {
                    null_fraction: stats.null_count as f64 / meta.num_rows as f64,
                    distinct_count: stats.distinct_count,
                    min_value: stats.min,
                    max_value: stats.max,
                    histogram: None, // File formats don't have histograms
                    avg_width: estimate_avg_width(&stats),
                })
            }).collect(),
            last_analyzed: meta.mtime, // File modification time
        })
    }
}
```

### Rewrite Rules

#### Rule 1: Column Pruning

```yaml
---
id: parquet-column-pruning
name: Parquet Column Pruning
category: physical/file-format
databases: [all]
preconditions:
  - type: pattern
    must_match: "(project ?cols (scan ?table))"
  - type: predicate
    condition: "is_file_table(?table) AND supports_column_pruning(format(?table))"
---
```

```rust
rw!("parquet-column-pruning";
    "(project ?cols (scan ?table))" =>
    "(scan[format=parquet, columns=?cols] ?table)"
    if is_parquet_file("?table")
),
```

#### Rule 2: Predicate Pushdown

```yaml
---
id: parquet-predicate-pushdown
name: Parquet Row Group Filtering
category: physical/file-format
databases: [all]
preconditions:
  - type: pattern
    must_match: "(filter ?pred (scan ?table))"
  - type: predicate
    condition: "is_file_table(?table) AND supports_predicate_pushdown(format(?table))"
  - type: predicate
    condition: "is_pushdown_safe(?pred)"
---
```

```rust
rw!("parquet-predicate-pushdown";
    "(filter ?pred (scan ?table))" =>
    "(scan[format=parquet, predicate=?pred] ?table)"
    if is_parquet_file("?table") && is_pushdown_safe("?pred")
),
```

**Safety check**: `is_pushdown_safe` ensures predicate uses columns with statistics and is sargable (>, <, =, BETWEEN, IN, IS NULL).

#### Rule 3: Late Materialization

```yaml
---
id: parquet-late-materialization
name: Parquet Late Materialization
category: physical/file-format
preconditions:
  - type: pattern
    must_match: "(project ?out_cols (filter ?pred (scan ?table)))"
  - type: predicate
    condition: "predicate_cols(?pred) ∩ ?out_cols = ∅"
---
```

**When to apply**: Predicate columns disjoint from output columns.

**Example**:
```sql
SELECT name, email FROM users WHERE age > 50;
-- Predicate cols: {age}
-- Output cols: {name, email}
-- Disjoint? Yes → Apply late materialization
-- Read {age} → filter → read {name, email} for survivors
```

### Cost Model Adjustments

```rust
impl CostModel {
    fn cost_parquet_scan(
        &self,
        meta: &FileMetadata,
        projection: &[String],
        predicate: Option<&Expr>,
    ) -> Cost {
        // 1. Row group filtering
        let surviving_row_groups = if let Some(pred) = predicate {
            self.filter_row_groups(&meta.row_groups, pred)
        } else {
            meta.row_groups.len()
        };

        // 2. I/O cost (columnar, only projected columns)
        let column_factor = projection.len() as f64 / meta.schema.fields().len() as f64;
        let total_compressed_size: u64 = meta.row_groups[..surviving_row_groups]
            .iter()
            .map(|rg| rg.compressed_size)
            .sum();
        let io_cost = (total_compressed_size as f64 * column_factor) / 1_000_000.0; // MB

        // 3. CPU cost (decompression + deserialization)
        let surviving_rows = meta.row_groups[..surviving_row_groups]
            .iter()
            .map(|rg| rg.num_rows)
            .sum::<u64>();
        let cpu_cost = surviving_rows as f64 * projection.len() as f64 * 0.001;

        // 4. Memory cost (row batch size)
        let memory_cost = 1024.0 * projection.len() as f64 * 8.0; // 1024 rows × cols × 8 bytes

        Cost { io: io_cost, cpu: cpu_cost, memory: memory_cost }
    }

    fn filter_row_groups(&self, row_groups: &[RowGroupMeta], pred: &Expr) -> usize {
        row_groups.iter().filter(|rg| {
            self.row_group_matches_predicate(rg, pred)
        }).count()
    }

    fn row_group_matches_predicate(&self, rg: &RowGroupMeta, pred: &Expr) -> bool {
        match pred {
            Expr::BinaryOp { op: BinOp::Gt, left, right } => {
                // col > value: Check if max(col) > value
                if let (Expr::Column(col), Expr::Const(val)) = (&**left, &**right) {
                    if let Some(stats) = rg.column_stats.get(col.name) {
                        return stats.max.as_ref().map_or(true, |max| max > val);
                    }
                }
                true // Can't filter, must scan
            }
            Expr::BinaryOp { op: BinOp::Lt, left, right } => {
                // col < value: Check if min(col) < value
                if let (Expr::Column(col), Expr::Const(val)) = (&**left, &**right) {
                    if let Some(stats) = rg.column_stats.get(col.name) {
                        return stats.min.as_ref().map_or(true, |min| min < val);
                    }
                }
                true
            }
            Expr::BinaryOp { op: BinOp::Eq, left, right } => {
                // col = value: Check bloom filter if available
                // Otherwise, check if value ∈ [min, max]
                if let (Expr::Column(col), Expr::Const(val)) = (&**left, &**right) {
                    if let Some(stats) = rg.column_stats.get(col.name) {
                        let in_range = stats.min.as_ref().map_or(true, |min| val >= min)
                            && stats.max.as_ref().map_or(true, |max| val <= max);
                        return in_range;
                    }
                }
                true
            }
            // AND: Both sides must match
            Expr::BinaryOp { op: BinOp::And, left, right } => {
                self.row_group_matches_predicate(rg, left)
                    && self.row_group_matches_predicate(rg, right)
            }
            // OR: At least one side must match
            Expr::BinaryOp { op: BinOp::Or, left, right } => {
                self.row_group_matches_predicate(rg, left)
                    || self.row_group_matches_predicate(rg, right)
            }
            _ => true, // Unknown predicate, must scan
        }
    }
}
```

---

## Drawbacks

1. **Complexity**: Adds file format abstraction layer
2. **Dependencies**: Requires `parquet`, `orc-rust`, `arrow` crates
3. **Metadata overhead**: Reading footer adds ~1ms per file (negligible for large files, noticeable for 1000s of small files)
4. **Partial pushdown**: Not all predicates can be pushed down (e.g., `LIKE '%pattern%'`, UDFs)

---

## Rationale and Alternatives

### Why This Design?

1. **Trait-based**: Extensible to new formats (Iceberg, Delta Lake)
2. **Statistics integration**: Reuses existing `ra-stats` infrastructure
3. **Transparent**: Users don't need to change queries
4. **Cost-based**: Planner chooses between full scan and pushdown

### Alternatives Considered

#### Alternative 1: External Catalog (Hive Metastore)
- **Pros**: Centralized metadata, multi-table statistics
- **Cons**: Requires external service, slower, adds operational complexity
- **Decision**: Use file metadata as primary source, optionally integrate with catalogs later

#### Alternative 2: Pre-Analyze Phase
- **Pros**: Simpler, reuses existing ANALYZE code
- **Cons**: Slow (must scan all files), doesn't leverage Parquet metadata
- **Decision**: Read metadata directly from files

#### Alternative 3: No Optimization (User-Driven)
- **Pros**: Simplest, no code changes
- **Cons**: 100-1000x slower than DuckDB, poor user experience
- **Decision**: Unacceptable for data lake use case

---

## Prior Art

### DuckDB
- **Approach**: Built-in Parquet/CSV/JSON readers with automatic optimization
- **Features**: Column pruning, predicate pushdown, parallel reads
- **Performance**: 100-1000x faster than naive scans
- **Lesson**: File format awareness is critical for data lake performance

### Apache Spark
- **Approach**: Catalyst optimizer with data source API
- **Features**: Partition pruning, column pruning, predicate pushdown
- **Lesson**: Abstract data sources behind common API

### Presto/Trino
- **Approach**: Connector SPI with pushdown capabilities
- **Features**: Dynamic filtering, runtime filters, bloom filters
- **Lesson**: Cost-based decision on pushdown vs scan

### Polars
- **Approach**: LazyFrame with query optimization
- **Features**: Predicate pushdown, column projection, parallel Parquet reading
- **Lesson**: Lazy evaluation enables better optimization

---

## Unresolved Questions

1. **Bloom filter integration**: When to build, when to use, cache policy?
2. **Nested column pushdown**: How to handle `address.city` in Parquet?
3. **Multi-file queries**: Union 1000 Parquet files - aggregate statistics?
4. **Partition discovery**: Auto-detect `date=2023-01-01` folder structure?
5. **Remote files (S3)**: How to minimize S3 API calls for metadata?

---

## Future Possibilities

### Phase 2: Partition Awareness
```
Files: s3://bucket/sales/date=2023-01-01/part-0001.parquet
       s3://bucket/sales/date=2023-01-02/part-0002.parquet

Query: SELECT * FROM sales WHERE date = '2023-01-15'
→ Skip directories where date != '2023-01-15' (0 S3 API calls)
```

### Phase 3: Iceberg/Delta Lake Integration
```rust
pub trait TableFormat {
    fn list_files(&self, table: &str) -> Vec<FileWithMetadata>;
    fn read_transaction_log(&self) -> Vec<Transaction>;
}
```

### Phase 4: Adaptive File Format
```
If query has predicate pushdown: Use Parquet
Else if schema projection only: Use Arrow IPC (zero-copy)
Else if full scan: Use CSV (simpler, less overhead)
```

### Phase 5: Query Result Caching (Iceberg)
```
Cache query results as Parquet files
On subsequent query: Check cache, return if fresh
```

---

## References

- **Parquet Format**: https://parquet.apache.org/docs/file-format/
- **ORC Specification**: https://orc.apache.org/specification/
- **DuckDB Parquet Docs**: https://duckdb.org/docs/guides/file_formats/parquet_files
- **Liu et al. "File Format Benchmarks"**: CMU 15-721 Spring 2024
- **Arrow Columnar Format**: https://arrow.apache.org/docs/format/Columnar.html
- **Spark Data Source API**: https://spark.apache.org/docs/latest/sql-data-sources.html

---

## Implementation Checklist

### Phase 1: Foundation (2-3 weeks)
- [ ] Create `ra-core/src/formats/mod.rs` with `FileFormat` trait
- [ ] Implement `ParquetFormat` in `ra-core/src/formats/parquet.rs`
- [ ] Add `FileMetadata` and `ColumnStats` types
- [ ] Unit tests for metadata reading
- [ ] Benchmark: TPC-H Q1 on Parquet vs CSV

### Phase 2: Query Integration (2-3 weeks)
- [ ] Add `FileStatisticsProvider` to `ra-stats`
- [ ] Column pruning rewrite rule
- [ ] Predicate pushdown rewrite rule
- [ ] Cost model updates
- [ ] Integration tests: Verify pushdown applied

### Phase 3: Advanced Optimizations (3-4 weeks)
- [ ] Bloom filter support
- [ ] Late materialization
- [ ] Nested column projection
- [ ] Partition pruning (directory-based)
- [ ] Benchmark: TPC-H all queries

### Phase 4: Additional Formats (2-3 weeks)
- [ ] ORC support (`ORCFormat` impl)
- [ ] Arrow IPC support (`ArrowIPCFormat` impl)
- [ ] CSV with schema hints
- [ ] Format auto-detection

### Phase 5: Production Readiness (2-3 weeks)
- [ ] S3 remote file support
- [ ] Metadata caching layer
- [ ] Parallel file reading
- [ ] Error handling and retry logic
- [ ] Documentation and examples
