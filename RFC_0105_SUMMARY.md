# RFC 0105: External Tables and Cloud Storage Optimization - Summary

**Status**: Completed and committed to branch `rfc-0105-external-tables`
**Commit**: 24dd9a5d
**File**: `/home/gburd/ws/ra/.claude/worktrees/rfc-0105-external-tables/docs/rfcs/0105-external-tables-optimization.md`
**Lines**: 926 lines

## Overview

RFC 0105 provides comprehensive specification for optimizing Snowflake external tables and cloud storage integration. External tables enable querying data in S3, Azure Blob Storage, and Google Cloud Storage without loading it into the warehouse - essential for cloud data warehouses and data lake architectures.

## Key Features Covered

### 1. External Table Components
- **External Stages**: Named cloud storage locations with credentials
- **External Tables**: Virtual tables over staged files with partitioning
- **Directory Tables**: Metadata about files in stages
- **File Formats**: Parquet, Avro, ORC, CSV, JSON

### 2. Query Processing
- **File discovery**: List files matching path patterns
- **Partition pruning**: Hive-style directory-based partitioning (year=2024/month=01)
- **File-level statistics**: Min/max values per file for aggressive pruning
- **Format-specific pushdown**: Parquet column/predicate pushdown, CSV filtering
- **Parallel reading**: Distribute files across workers

### 3. Cloud Storage Integration
- **S3**: AWS SDK, IAM roles, S3 Select support
- **Azure Blob Storage**: Azure SDK, SAS tokens, service principals
- **Google Cloud Storage**: GCS SDK, service accounts
- **Credential management**: Encrypted storage, rotation handling

### 4. Optimization Opportunities
- **Partition pruning by path**: Eliminate 90-99% of files
- **File pruning by statistics**: Skip files using min/max values
- **Column pruning**: Read only required columns (Parquet/ORC)
- **Predicate pushdown**: Push filters to file format readers
- **Parallel file reading**: Distribute across workers
- **Metadata caching**: Avoid repeated cloud API calls
- **MV recommendations**: Detect frequently-accessed external tables

### 5. Cloud Cost Model
Comprehensive cost modeling for:
- **Network egress**: $/GB transferred out of cloud
- **API calls**: $/1000 requests (list, get, read)
- **Compute**: Warehouse runtime for parsing/processing
- **Storage**: $/GB/month (informational)

**Cost reduction**: 100-1000x with optimization (partition pruning + file pruning + column pruning + predicate pushdown)

### 6. Performance Analysis
- **Without optimization**: 10-50x slower than warehouse tables
- **With optimization**: <2x slowdown for selective queries
- **Baseline example**: 10,000 files (100GB), 10 minutes, $14.05
- **Optimized example**: 10 files (100MB), 5 seconds, $0.06 (234x cheaper)

### 7. Implementation Plan
**Total: 20 weeks across 6 phases**

- **Phase 1 (4 weeks)**: Core infrastructure (catalog, file formats, metadata)
- **Phase 2 (3 weeks)**: Partition pruning
- **Phase 3 (4 weeks)**: Pushdown optimization (Parquet, CSV, JSON)
- **Phase 4 (3 weeks)**: Cloud integration (S3, Azure, GCS)
- **Phase 5 (3 weeks)**: Advanced features (metadata refresh, parallel reading, MV integration)
- **Phase 6 (3 weeks)**: Production readiness (error handling, security, documentation)

### 8. Technical Design

#### Relation Expression
```rust
RelExpr::ExternalTableScan {
    table: String,
    stage: ExternalStage,
    file_format: FileFormat,
    partitions: Vec<PartitionColumn>,
    file_pattern: String,
    filter: Option<Box<Expr>>,
    projection: Vec<ColumnRef>,
}
```

#### Optimization Rules
1. **Partition pruning**: Convert filter on partition columns to file path filtering
2. **File-level pruning**: Use file statistics to skip files
3. **Column pruning**: Only read required columns (columnar formats)
4. **Predicate pushdown**: Push predicates to file format readers
5. **MV recommendation**: Suggest materializing frequently-accessed tables

#### Metadata Refresh Strategies
- **Manual**: Explicit `ALTER EXTERNAL TABLE ... REFRESH`
- **Event-driven**: Cloud storage notifications (S3 events, Azure Event Grid)
- **Scheduled**: Periodic refresh at intervals
- **On-query**: Check metadata before each query with TTL cache

## Integration with Ra Architecture

### Catalog
```rust
trait Catalog {
    fn get_external_table(&self, name: &str) -> Result<ExternalTableInfo>;
    fn refresh_external_table_metadata(&mut self, name: &str) -> Result<()>;
    fn list_external_tables(&self) -> Vec<String>;
}
```

### Statistics
Aggregate file-level statistics into table statistics:
- Row counts from file metadata
- Per-column min/max from file column statistics
- Size estimates from file sizes

### Cost Model
Extended `CostCalibration` with external table parameters:
- Network egress cost per GB
- API call cost per 1000 requests
- Format-specific parse costs (Parquet << CSV << JSON)

## Testing Strategy

### Unit Tests
- Partition pruning correctness
- File-level statistics filtering
- Cost model accuracy

### Integration Tests
- End-to-end query execution with mock S3
- Format-specific testing (Parquet, CSV, JSON)
- Multi-cloud testing (S3, Azure, GCS)

### Performance Tests
- Partition pruning effectiveness (10-100x file reduction)
- File-level pruning (90%+ for selective queries)
- Network transfer reduction (3-5x from column pruning)
- End-to-end performance (<2x slowdown vs loaded tables)

## Expected Impact

**High impact - essential for cloud data lakes**

### Benefits
1. **Cost savings**: Query data in cheap S3 storage without loading (100-1000x cost reduction)
2. **ETL elimination**: Query data in place, no loading pipelines
3. **Data freshness**: Query latest data without load lag
4. **Flexibility**: Combine warehouse tables with external data
5. **Scalability**: Query petabyte-scale data lakes

### Use Cases
- Data lake analytics (query S3 without ETL)
- Federation (join external + internal tables)
- Semi-structured exploration (query JSON before loading)
- Cost optimization (cold data in S3, hot in warehouse)
- Hybrid architectures (tiered storage)

## Alignment with Gap Analysis

Fully addresses Section 6 of SNOWFLAKE_FEATURES_GAP_ANALYSIS.md:

✅ External stages and tables
✅ Partition pruning on directory structure
✅ File-level statistics
✅ Format-specific optimizations (Parquet, Avro, ORC, CSV, JSON)
✅ Parallel file reading
✅ Cloud cost modeling (network, API calls, compute)
✅ Metadata refresh strategies
✅ MV recommendations for external tables
✅ S3 Select and server-side filtering
✅ Multi-cloud support (S3, Azure, GCS)

## Related RFCs

- **RFC 0099**: Semi-Structured Data (VARIANT/OBJECT/ARRAY) - External tables default to VARIANT column
- **RFC 0098**: LATERAL FLATTEN - Often used with external semi-structured data
- **RFC 0104**: Delta/Merge Operations - Materialized views over external tables
- **RFC 0100**: Time Travel - Could combine with external table snapshots

## Future Work

1. **S3 Select integration**: Push complex predicates to S3 Select API
2. **Delta Lake/Iceberg support**: Transactional table formats with ACID
3. **External table indexes**: Bloom filters, zone maps on frequently-queried columns
4. **Automatic format detection**: Infer format from extension/content
5. **Cost-based refresh**: Automatically schedule refresh based on query patterns
6. **Cross-cloud optimization**: Optimize queries spanning multiple providers
7. **Incremental loading**: Detect when partial loading is cheaper than repeated external queries

## Unresolved Questions

1. **Credential rotation**: How to handle automatic rotation without query interruption?
2. **Cross-region optimization**: Should Ra automatically replicate files to warehouse region?
3. **Caching strategy**: Should external table results be cached differently?
4. **Metadata consistency**: How to handle external table metadata modified during query execution?
5. **Cost attribution**: How to track and report external table costs per user/query?

## Next Steps

1. Review RFC with Ra maintainers
2. Validate cost model assumptions with benchmarks
3. Prototype partition pruning algorithm
4. Test with real S3/Azure/GCS data
5. Refine implementation timeline based on feedback

---

**RFC Location**: `/home/gburd/ws/ra/.claude/worktrees/rfc-0105-external-tables/docs/rfcs/0105-external-tables-optimization.md`
**Branch**: `rfc-0105-external-tables`
**Worktree**: `/home/gburd/ws/ra/.claude/worktrees/rfc-0105-external-tables`
