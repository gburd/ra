# RFC 0105: External Tables and Cloud Storage Optimization

- Start Date: 2026-03-28
- Author: Ra Research Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Ra should provide comprehensive optimization for Snowflake external tables and cloud storage integration, enabling efficient querying of data in S3, Azure Blob Storage, and Google Cloud Storage without loading it into the warehouse. This includes partition pruning on cloud storage paths, file-level statistics utilization, format-specific pushdown (Parquet column pruning, CSV predicate filtering), parallel file reading strategies, and cloud-aware cost modeling that accounts for network egress, API calls, and storage access patterns. External tables are essential for cloud data warehouses and data lake architectures, offering 5-50x I/O reduction via pushdown and partition elimination.

## Motivation

Cloud data warehouses like Snowflake separate compute from storage, making external tables a critical feature for querying data lakes without ETL overhead. Organizations store petabytes of data in S3/Azure/GCS, and the ability to query this data in place eliminates costly and time-consuming data loading pipelines.

**Key optimization challenges:**

| Challenge | Impact |
|-----------|--------|
| Network I/O cost | 2-5x slower than local storage without optimization |
| Cloud API call pricing | Excessive file listings drive up costs |
| File format efficiency | CSV row-based vs Parquet columnar (10-100x difference) |
| Partition pruning | Directory-based partitioning can eliminate 90-99% of files |
| Metadata staleness | External table metadata may be out of sync with cloud storage |
| File-level statistics | Min/max values per file enable aggressive pruning |

**Use cases:**

1. **Data lake querying**: Query S3 data lakes without ETL (e.g., query 1TB of Parquet logs, scan only 10GB after pushdown)
2. **Federation**: Join external data with warehouse tables (e.g., join S3 user events with customer dimension table)
3. **Semi-structured ingestion**: Query JSON logs before deciding what to load
4. **Cost optimization**: Keep cold data in cheap S3 storage, query on-demand
5. **Hybrid architectures**: Some data in warehouse (hot), most in cloud storage (warm/cold)

**Performance impact:** Without optimization, external table scans can be 10-50x slower than warehouse tables due to network overhead. With proper pushdown and partition pruning, external tables can approach warehouse table performance for selective queries.

## Guide-level explanation

### External table components

**External Stage**: Named cloud storage location with credentials and configuration.

```sql
CREATE STAGE s3_logs
  URL = 's3://my-bucket/logs/'
  CREDENTIALS = (AWS_KEY_ID = '...' AWS_SECRET_KEY = '...');
```

**External Table**: Virtual table over staged files with optional partitioning.

```sql
CREATE EXTERNAL TABLE web_logs (
  timestamp TIMESTAMP,
  user_id VARCHAR,
  url VARCHAR,
  status_code INT
)
LOCATION = @s3_logs/year={YYYY}/month={MM}/
FILE_FORMAT = (TYPE = PARQUET)
PARTITION BY (year, month);
```

**Directory Table**: Metadata about files in a stage.

```sql
-- Query file metadata without reading contents
SELECT file_name, file_size, last_modified
FROM DIRECTORY(@s3_logs);
```

### Query processing flow

1. **File discovery**: List files in S3 matching path pattern
2. **Partition pruning**: Filter files by partition expressions in WHERE clause
3. **File pruning**: Use file-level statistics (min/max values) to skip files
4. **Format-specific pushdown**: Push predicates to Parquet/ORC readers
5. **Parallel reading**: Distribute files across workers
6. **Result assembly**: Combine results from all files

### Example optimization

```sql
-- Query: Find errors in recent logs
SELECT timestamp, user_id, url
FROM web_logs
WHERE year = 2026
  AND month = 3
  AND status_code &gt;= 500;
```

**Optimization steps:**

1. **Partition pruning**: Only scan `s3://my-bucket/logs/year=2026/month=03/` (eliminates 99% of files)
2. **File-level pruning**: Use Parquet row group statistics on `status_code` to skip files where `max(status_code) &lt; 500` (eliminates 95% of remaining files)
3. **Column pruning**: Only read `timestamp`, `user_id`, `url`, `status_code` columns from Parquet (25% of columns)
4. **Predicate pushdown**: Push `status_code &gt;= 500` to Parquet reader (scans only matching row groups)
5. **Parallel reading**: Distribute remaining ~50 files across 10 workers

**Result**: Scan 500MB instead of 10TB, complete in 30 seconds instead of 2 hours.

## Reference-level explanation

### External table representation

Add new relation expression for external table scans:

```rust
pub enum RelExpr {
    // ... existing variants ...

    /// Scan of external table (cloud storage)
    ExternalTableScan {
        /// External table name
        table: String,

        /// External stage location
        stage: ExternalStage,

        /// File format configuration
        file_format: FileFormat,

        /// Partition columns and expressions
        partitions: Vec&lt;PartitionColumn&gt;,

        /// File pattern (with {variable} placeholders)
        file_pattern: String,

        /// Predicate to push to file readers
        filter: Option&lt;Box&lt;Expr&gt;&gt;,

        /// Columns to project (for columnar formats)
        projection: Vec&lt;ColumnRef&gt;,
    },
}
```

### External stage definition

```rust
pub struct ExternalStage {
    pub name: String,
    pub url: CloudStorageUrl,
    pub credentials: CloudCredentials,
    pub encryption: Option&lt;EncryptionConfig&gt;,
}

pub enum CloudStorageUrl {
    S3 { bucket: String, prefix: String, region: String },
    Azure { account: String, container: String, path: String },
    GCS { bucket: String, prefix: String },
}

pub enum CloudCredentials {
    AwsIam { role_arn: String },
    AwsKeys { access_key_id: String, secret_access_key: String },
    AzureSas { sas_token: String },
    AzureServicePrincipal { tenant_id: String, client_id: String, client_secret: String },
    GcsServiceAccount { key_file: String },
}
```

### File format support

```rust
pub enum FileFormat {
    Parquet {
        /// Enable column pruning
        column_projection: bool,
        /// Enable predicate pushdown
        predicate_pushdown: bool,
    },
    Avro {
        /// Schema evolution handling
        schema_evolution: SchemaEvolutionMode,
    },
    Orc {
        /// Enable predicate pushdown
        predicate_pushdown: bool,
    },
    Csv {
        delimiter: char,
        header: bool,
        compression: Option&lt;CompressionType&gt;,
    },
    Json {
        /// Parse mode (strict, permissive)
        parse_mode: JsonParseMode,
    },
}

pub enum CompressionType {
    Gzip,
    Bzip2,
    Snappy,
    Zstd,
}
```

### Partition pruning

Hive-style partitioning uses directory structure:

```
s3://bucket/data/
  year=2024/
    month=01/
      day=01/
        file1.parquet
        file2.parquet
    month=02/
      day=01/
        file3.parquet
```

Partition pruning algorithm:

```rust
fn prune_partitions(
    file_pattern: &str,
    partitions: &[PartitionColumn],
    filter: &Expr,
) -&gt; Vec&lt;String&gt; {
    // 1. Extract partition predicates from filter
    let partition_predicates = extract_partition_predicates(filter, partitions);

    // 2. Evaluate partition expressions
    let mut file_paths = Vec::new();
    for partition_values in generate_partition_combinations(partitions) {
        if evaluate_predicates(&partition_predicates, &partition_values) {
            let path = substitute_variables(file_pattern, &partition_values);
            file_paths.push(path);
        }
    }

    file_paths
}
```

Example:

```sql
WHERE year = 2024 AND month BETWEEN 1 AND 3
```

Generates paths:
- `s3://bucket/data/year=2024/month=01/`
- `s3://bucket/data/year=2024/month=02/`
- `s3://bucket/data/year=2024/month=03/`

### File-level statistics

External tables maintain metadata per file:

```rust
pub struct FileMetadata {
    pub path: String,
    pub size_bytes: u64,
    pub row_count: u64,
    pub last_modified: DateTime&lt;Utc&gt;,
    pub column_statistics: HashMap&lt;ColumnRef, ColumnStats&gt;,
}

pub struct ColumnStats {
    pub min_value: Option&lt;Scalar&gt;,
    pub max_value: Option&lt;Scalar&gt;,
    pub null_count: u64,
    pub distinct_count: Option&lt;u64&gt;,
}
```

File pruning uses these statistics:

```rust
fn prune_files(
    files: &[FileMetadata],
    filter: &Expr,
) -&gt; Vec&lt;&FileMetadata&gt; {
    files.iter()
        .filter(|file| {
            // Check if filter could possibly match any row in this file
            evaluate_with_statistics(filter, &file.column_statistics)
        })
        .collect()
}
```

For `WHERE status_code &gt;= 500`, skip files where `max(status_code) &lt; 500`.

### Predicate pushdown to file formats

**Parquet pushdown**: Parquet stores min/max values per row group (8KB-1MB of data).

```rust
impl ParquetReader {
    fn scan_with_predicate(&self, filter: &Expr) -&gt; impl Iterator&lt;Item = Row&gt; {
        self.row_groups()
            .filter(|rg| {
                // Skip row group if predicate cannot match
                evaluate_with_statistics(filter, rg.statistics())
            })
            .flat_map(|rg| rg.scan_rows())
            .filter(|row| filter.evaluate(row))
    }
}
```

**CSV pushdown**: Limited to row-level filtering after parsing (no statistics).

**JSON pushdown**: Can skip entire files if schema is known, but limited intra-file pushdown.

### Parallel file reading

Distribute files across workers based on:

1. **File count**: Each worker gets subset of files
2. **File size**: Balance total bytes per worker
3. **Locality**: Prefer workers in same cloud region

```rust
fn distribute_files(
    files: Vec&lt;FileMetadata&gt;,
    worker_count: usize,
) -&gt; Vec&lt;Vec&lt;FileMetadata&gt;&gt; {
    // Sort by size (descending) for better load balancing
    let mut files = files;
    files.sort_by_key(|f| std::cmp::Reverse(f.size_bytes));

    // Round-robin assignment
    let mut assignments = vec![Vec::new(); worker_count];
    for (i, file) in files.into_iter().enumerate() {
        assignments[i % worker_count].push(file);
    }

    assignments
}
```

### Cloud storage cost model

External table scans have different cost structure than local scans:

```rust
pub struct ExternalTableCost {
    /// Network egress cost ($/GB transferred)
    pub network_egress_cost: f64,

    /// API call cost ($/1000 requests)
    pub api_call_cost: f64,

    /// Compute cost (warehouse runtime)
    pub compute_cost: f64,

    /// External storage cost ($/GB/month, informational)
    pub storage_cost: f64,
}

impl CostModel {
    fn estimate_external_scan_cost(
        &self,
        files: &[FileMetadata],
        file_format: &FileFormat,
        selectivity: f64,
    ) -&gt; Cost {
        let total_bytes: u64 = files.iter().map(|f| f.size_bytes).sum();
        let file_count = files.len();

        // Cloud API calls (list, get metadata, read)
        let api_calls = file_count as f64 * 2.0; // List + read
        let api_cost = (api_calls / 1000.0) * self.external.api_call_cost;

        // Network transfer (only bytes actually read after pushdown)
        let bytes_transferred = match file_format {
            FileFormat::Parquet { column_projection: true, .. } =&gt; {
                // Column pruning reduces transfer
                total_bytes as f64 * selectivity * 0.3 // ~30% of columns
            }
            FileFormat::Csv { .. } =&gt; {
                // Must read entire file
                total_bytes as f64
            }
            _ =&gt; total_bytes as f64 * selectivity,
        };
        let network_cost = (bytes_transferred / 1e9) * self.external.network_egress_cost;

        // Compute cost (warehouse runtime)
        let parse_cost = match file_format {
            FileFormat::Parquet { .. } =&gt; bytes_transferred * 0.001, // Fast binary format
            FileFormat::Csv { .. } =&gt; bytes_transferred * 0.01,      // Slow text parsing
            FileFormat::Json { .. } =&gt; bytes_transferred * 0.02,     // Very slow JSON parsing
            _ =&gt; bytes_transferred * 0.005,
        };

        Cost {
            io: network_cost + api_cost,
            cpu: parse_cost,
            memory: bytes_transferred * 0.1, // Decompression buffer
        }
    }
}
```

**Cost reduction from optimization:**

| Optimization | Cost Reduction |
|--------------|----------------|
| Partition pruning (90% of files) | 10x reduction in API calls and network |
| File-level statistics (95% of remaining) | 20x additional reduction |
| Column pruning (70% of columns) | 3x reduction in network transfer |
| Predicate pushdown to Parquet | 5-50x reduction in compute |
| **Combined** | **100-1000x total cost reduction** |

### Metadata refresh strategies

External table metadata can become stale when cloud storage changes:

```rust
pub enum RefreshStrategy {
    /// Manual refresh via ALTER EXTERNAL TABLE ... REFRESH
    Manual,

    /// Automatic refresh on cloud storage notifications
    EventDriven {
        /// S3 event notifications, Azure Event Grid, GCS Pub/Sub
        notification_channel: String,
    },

    /// Periodic refresh
    Scheduled {
        interval: Duration,
    },

    /// On-query refresh (check metadata before each query)
    OnQuery {
        /// Cache metadata for this duration
        cache_ttl: Duration,
    },
}
```

**Metadata cache invalidation:**

```rust
fn should_refresh_metadata(
    external_table: &ExternalTableInfo,
    cache_entry: &MetadataCache,
) -&gt; bool {
    match &external_table.refresh_strategy {
        RefreshStrategy::Manual =&gt; false,
        RefreshStrategy::EventDriven { .. } =&gt; {
            // Check if notification received since last cache
            has_notification(external_table, cache_entry.last_refresh)
        }
        RefreshStrategy::Scheduled { interval } =&gt; {
            cache_entry.last_refresh + *interval &lt; Utc::now()
        }
        RefreshStrategy::OnQuery { cache_ttl } =&gt; {
            cache_entry.last_refresh + *cache_ttl &lt; Utc::now()
        }
    }
}
```

### Optimization rules

**Rule 1: Partition pruning**

```rust
// Convert filter on partition columns to file path filtering
fn partition_pruning_rule(
    scan: &ExternalTableScan,
    filter: &Expr,
) -&gt; ExternalTableScan {
    let partition_filter = extract_partition_predicates(filter, &scan.partitions);
    let pruned_paths = prune_partitions(&scan.file_pattern, &scan.partitions, &partition_filter);

    ExternalTableScan {
        file_pattern: pruned_paths.join(","),
        filter: remove_partition_predicates(filter, &scan.partitions),
        ..scan.clone()
    }
}
```

**Rule 2: File-level pruning**

```rust
// Use file statistics to skip files
fn file_pruning_rule(
    scan: &ExternalTableScan,
    metadata: &[FileMetadata],
) -&gt; Vec&lt;FileMetadata&gt; {
    metadata.iter()
        .filter(|file| {
            scan.filter.as_ref()
                .map(|f| evaluate_with_statistics(f, &file.column_statistics))
                .unwrap_or(true)
        })
        .cloned()
        .collect()
}
```

**Rule 3: Column pruning**

```rust
// For columnar formats, only read required columns
fn column_pruning_rule(
    scan: &ExternalTableScan,
    required_columns: &[ColumnRef],
) -&gt; ExternalTableScan {
    match &scan.file_format {
        FileFormat::Parquet { .. } | FileFormat::Orc { .. } =&gt; {
            ExternalTableScan {
                projection: required_columns.to_vec(),
                ..scan.clone()
            }
        }
        _ =&gt; scan.clone(), // No column pruning for row-based formats
    }
}
```

**Rule 4: Predicate pushdown**

```rust
// Push predicates to file format readers
fn predicate_pushdown_rule(
    scan: &ExternalTableScan,
    filter: &Expr,
) -&gt; ExternalTableScan {
    match &scan.file_format {
        FileFormat::Parquet { predicate_pushdown: true, .. } =&gt; {
            // Push simple predicates to Parquet reader
            let pushable = extract_pushable_predicates(filter);
            ExternalTableScan {
                filter: Some(Box::new(pushable)),
                ..scan.clone()
            }
        }
        _ =&gt; scan.clone(),
    }
}
```

**Rule 5: Materialized view recommendation**

```rust
// Recommend materializing frequently-accessed external tables
fn external_table_mv_recommendation(
    query_log: &[Query],
    external_table: &str,
) -&gt; Option&lt;MaterializedViewRecommendation&gt; {
    let queries = query_log.iter()
        .filter(|q| q.references_external_table(external_table))
        .count();

    if queries &gt; 10 {
        Some(MaterializedViewRecommendation {
            reason: format!(
                "External table '{}' accessed {} times. \
                Materializing would save network I/O costs.",
                external_table, queries
            ),
            estimated_savings: calculate_mv_savings(query_log, external_table),
        })
    } else {
        None
    }
}
```

### Integration with Ra architecture

**Catalog integration:**

```rust
pub struct ExternalTableInfo {
    pub name: String,
    pub stage: ExternalStage,
    pub file_format: FileFormat,
    pub partitions: Vec&lt;PartitionColumn&gt;,
    pub file_pattern: String,
    pub refresh_strategy: RefreshStrategy,
    pub metadata_cache: MetadataCache,
}

pub struct MetadataCache {
    pub last_refresh: DateTime&lt;Utc&gt;,
    pub files: Vec&lt;FileMetadata&gt;,
    pub total_bytes: u64,
    pub total_rows: u64,
}
```

Add to `ra-catalog`:

```rust
pub trait Catalog {
    // ... existing methods ...

    fn get_external_table(&self, name: &str) -&gt; Result&lt;ExternalTableInfo&gt;;
    fn refresh_external_table_metadata(&mut self, name: &str) -&gt; Result&lt;()&gt;;
    fn list_external_tables(&self) -&gt; Vec&lt;String&gt;;
}
```

**Statistics integration:**

```rust
impl StatisticsCollector {
    fn collect_external_table_stats(
        &mut self,
        external_table: &ExternalTableInfo,
    ) -&gt; Result&lt;TableStatistics&gt; {
        // Aggregate file-level statistics
        let mut stats = TableStatistics::default();
        stats.row_count = external_table.metadata_cache.total_rows;
        stats.size_bytes = external_table.metadata_cache.total_bytes;

        // Per-column statistics from file metadata
        for file in &external_table.metadata_cache.files {
            for (col, col_stats) in &file.column_statistics {
                stats.column_stats.entry(col.clone())
                    .or_insert_with(ColumnStats::default)
                    .merge(col_stats);
            }
        }

        Ok(stats)
    }
}
```

**Cost model integration:**

Extend `CostCalibration` with external table parameters:

```rust
pub struct CostCalibration {
    // ... existing fields ...

    pub external_table_costs: ExternalTableCostParams,
}

pub struct ExternalTableCostParams {
    /// Network egress cost ($/GB)
    pub network_egress_cost_per_gb: f64,

    /// API call cost ($/1000 requests)
    pub api_call_cost_per_1000: f64,

    /// Parquet parsing cost (CPU cycles per byte)
    pub parquet_parse_cost: f64,

    /// CSV parsing cost (higher due to text processing)
    pub csv_parse_cost: f64,

    /// JSON parsing cost (highest due to complex parsing)
    pub json_parse_cost: f64,
}
```

### Performance analysis

**Baseline: External table scan without optimization**

```sql
SELECT COUNT(*)
FROM s3_web_logs
WHERE status_code &gt;= 500;
```

- Files in S3: 10,000 Parquet files (100GB total)
- Network transfer: 100GB
- API calls: 10,000 GET requests
- Time: ~10 minutes
- Cost: Network ($9) + API ($0.05) + Compute ($5) = **$14.05**

**With optimization:**

1. **Partition pruning** (year=2026, month=3): 100 files remain
2. **File-level statistics**: 10 files have `max(status_code) &gt;= 500`
3. **Column pruning**: Only read `status_code` column (10% of data)
4. **Predicate pushdown**: Parquet row groups skip 90% of rows

Result:
- Network transfer: 100MB (1000x reduction)
- API calls: 20 requests (500x reduction)
- Time: ~5 seconds (120x faster)
- Cost: Network ($0.009) + API ($0.0001) + Compute ($0.05) = **$0.06** (234x cheaper)

**Break-even analysis:**

External tables are cost-effective when:
- Query selectivity &lt; 10% (partition pruning eliminates most files)
- Query frequency &lt; 10x per day (avoid materialization overhead)
- Data size &gt; 100GB (loading costs exceed query-in-place costs)

## Implementation plan

### Phase 1: Core infrastructure (4 weeks)

1. **Week 1-2: Catalog integration**
   - Add `ExternalTableInfo` to catalog
   - Implement cloud storage URL parsing
   - Add credential management (encrypted storage)

2. **Week 3: File format readers**
   - Integrate Parquet reader (Arrow Parquet library)
   - Add CSV reader with compression support
   - Basic JSON reader

3. **Week 4: Metadata collection**
   - Implement file listing for S3/Azure/GCS
   - Extract file-level statistics from Parquet
   - Build metadata cache

### Phase 2: Partition pruning (3 weeks)

1. **Week 5: Partition expression evaluation**
   - Parse Hive-style partition paths
   - Extract partition predicates from filters
   - Generate pruned file lists

2. **Week 6: File-level statistics**
   - Implement statistics-based file pruning
   - Add min/max value checks
   - Handle NULL propagation

3. **Week 7: Testing and calibration**
   - Benchmark partition pruning effectiveness
   - Test with various partition schemes
   - Add regression tests

### Phase 3: Pushdown optimization (4 weeks)

1. **Week 8-9: Parquet pushdown**
   - Implement column pruning
   - Add predicate pushdown to row groups
   - Optimize for Arrow in-memory format

2. **Week 10: CSV/JSON optimization**
   - Add streaming parsing for large files
   - Implement compression-aware reading
   - Add format-specific heuristics

3. **Week 11: Integration testing**
   - End-to-end query testing
   - Performance benchmarking
   - Cost model validation

### Phase 4: Cloud integration (3 weeks)

1. **Week 12: AWS S3**
   - Implement IAM role authentication
   - Add S3 Select support (server-side filtering)
   - Handle multi-region access

2. **Week 13: Azure/GCS**
   - Add Azure Blob Storage support
   - Add Google Cloud Storage support
   - Unified credential management

3. **Week 14: Monitoring and observability**
   - Add cloud API call tracking
   - Monitor network transfer metrics
   - Cost attribution reporting

### Phase 5: Advanced features (3 weeks)

1. **Week 15: Metadata refresh**
   - Implement event-driven refresh (S3 notifications)
   - Add scheduled refresh
   - Handle metadata staleness

2. **Week 16: Parallel reading**
   - Distribute files across workers
   - Load balancing by file size
   - Handle worker failures

3. **Week 17: Materialized view integration**
   - Detect frequently-accessed external tables
   - Recommend materialization with cost-benefit analysis
   - Auto-refresh MVs on external table changes

### Phase 6: Production readiness (3 weeks)

1. **Week 18: Error handling**
   - Retry logic for transient failures
   - Credential expiration handling
   - Network timeout management

2. **Week 19: Security**
   - Encrypt credentials at rest
   - Add column-level access control
   - Audit logging for external access

3. **Week 20: Documentation and examples**
   - User guide for external tables
   - Best practices for partition design
   - Troubleshooting guide

**Total estimated effort: 20 weeks**

## Testing strategy

### Unit tests

1. **Partition pruning correctness**
   - Test Hive-style path parsing
   - Verify predicate extraction
   - Check edge cases (missing partitions, NULL values)

2. **File-level statistics**
   - Test min/max filtering
   - Verify NULL handling
   - Check multi-column predicates

3. **Cost model accuracy**
   - Validate network cost calculations
   - Check API call counting
   - Verify format-specific costs

### Integration tests

1. **End-to-end query execution**
   - Create mock S3 bucket with sample data
   - Run queries with various predicates
   - Verify result correctness

2. **Format-specific testing**
   - Parquet: column pruning, predicate pushdown
   - CSV: compression handling, delimiter variations
   - JSON: nested structure parsing

3. **Multi-cloud testing**
   - Test S3, Azure Blob Storage, GCS
   - Verify credential handling
   - Check cross-region access

### Performance tests

1. **Partition pruning effectiveness**
   - Measure files scanned vs total files
   - Verify 10-100x reduction in typical queries

2. **File-level pruning**
   - Measure files skipped using statistics
   - Target 90%+ pruning for selective queries

3. **Network transfer reduction**
   - Measure bytes transferred vs total size
   - Verify column pruning reduces transfer 3-5x

4. **End-to-end performance**
   - Compare external table vs loaded table
   - Target <2x slowdown for selective queries
   - Measure cost savings (100-1000x)

### Regression tests

1. **Metadata refresh**
   - Add files to cloud storage, verify detection
   - Remove files, verify staleness handling
   - Test event-driven refresh

2. **Error scenarios**
   - Network failures, verify retry logic
   - Invalid credentials, verify error messages
   - Missing files, verify graceful handling

## Drawbacks and alternatives

### Drawbacks

1. **Performance overhead**: External tables are 2-5x slower than loaded tables for full scans due to network I/O
2. **Cost complexity**: Need to track network egress and API calls, not just compute
3. **Metadata staleness**: External table metadata may be out of sync with cloud storage
4. **Limited optimization**: Cannot create indexes on external tables
5. **Security complexity**: Managing cloud credentials adds operational burden

### Alternatives

**Alternative 1: Always load data into warehouse**

- Pros: Predictable performance, full optimization support (indexes, clustering)
- Cons: ETL overhead, storage costs, data freshness lag
- Decision: External tables complement loading, not replace it. Use external tables for infrequent queries and cold data.

**Alternative 2: Use cloud-native query engines (Athena, BigQuery)**

- Pros: Purpose-built for external data, no metadata management
- Cons: Requires separate tool, no integration with warehouse
- Decision: Ra optimizes Snowflake external tables to compete with these tools.

**Alternative 3: Serverless compute for external tables**

- Pros: No warehouse needed, pay only for actual usage
- Cons: Cold start latency, limited optimization
- Decision: Could add in future (Phase 7), but start with warehouse-based execution.

## Future work

1. **S3 Select integration**: Push more complex predicates to S3 Select API (server-side filtering)
2. **Delta Lake/Iceberg support**: Support transactional table formats with ACID guarantees
3. **External table indexes**: Build lightweight indexes on frequently-queried columns (bloom filters, zone maps)
4. **Automatic format detection**: Infer file format from extension and content
5. **Cost-based refresh**: Automatically schedule metadata refresh based on query patterns
6. **Cross-cloud optimization**: Optimize queries spanning multiple cloud providers
7. **Incremental loading recommendations**: Detect patterns where partial loading is cheaper than repeated external queries

## References

1. Snowflake External Tables documentation: https://docs.snowflake.com/en/user-guide/tables-external-intro.html
2. AWS S3 Select: https://docs.aws.amazon.com/AmazonS3/latest/userguide/selecting-content-from-objects.html
3. Apache Parquet format: https://parquet.apache.org/docs/file-format/
4. Delta Lake: https://delta.io/
5. Apache Iceberg: https://iceberg.apache.org/

## Unresolved questions

1. **Credential rotation**: How to handle automatic credential rotation without query interruption?
2. **Cross-region optimization**: Should Ra automatically replicate frequently-accessed files to warehouse region?
3. **Caching strategy**: Should external table results be cached differently than warehouse table results?
4. **Metadata consistency**: How to handle external table metadata being modified during query execution?
5. **Cost attribution**: How to track and report external table costs per user/query?


## Referenced By

This RFC is referenced by:

- [RFC 105: External Tables and Cloud Storage Optimization](/maintainers/rfcs/0105-external-tables-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 105: External Tables and Cloud Storage Optimization](/maintainers/rfcs/0105-external-tables-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 105: External Tables and Cloud Storage Optimization](/maintainers/rfcs/0105-external-tables-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 105: External Tables and Cloud Storage Optimization](/maintainers/rfcs/0105-external-tables-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 105: External Tables and Cloud Storage Optimization](/maintainers/rfcs/0105-external-tables-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 105: External Tables and Cloud Storage Optimization](/maintainers/rfcs/0105-external-tables-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 105: External Tables and Cloud Storage Optimization](/maintainers/rfcs/0105-external-tables-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 105: External Tables and Cloud Storage Optimization](/maintainers/rfcs/0105-external-tables-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 105: Enhanced Timeline Format with SQL DDL and Parametric Definitions](/maintainers/rfcs/0105-timeline-enhanced-format)


## Referenced By

This RFC is referenced by:

- [RFC 105: Enhanced Timeline Format with SQL DDL and Parametric Definitions](/maintainers/rfcs/0105-timeline-enhanced-format)


## Referenced By

This RFC is referenced by:

- [RFC 105: Enhanced Timeline Format with SQL DDL and Parametric Definitions](/maintainers/rfcs/0105-timeline-enhanced-format)
