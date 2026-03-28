# Snowflake Features: Quick Reference Summary

**Related Document:** [SNOWFLAKE_FEATURES_GAP_ANALYSIS.md](./SNOWFLAKE_FEATURES_GAP_ANALYSIS.md)

---

## Features by Category

### 1. Semi-Structured Data
- **VARIANT**: Universal container (128 MB max)
- **OBJECT**: Key-value maps
- **ARRAY**: Ordered collections with 0-based indexing
- **Path Access**: `column:path.to.field` or `column['key']`
- **FLATTEN**: Table function for array/object expansion

**Ra Gap:** Need dynamic type system, path-based references, semi-structured statistics

---

### 2. Temporal Features
- **Time Travel**: Query historical data with AT/BEFORE clauses (1-90 day retention)
- **Zero-Copy Cloning**: Metadata-only table/database copies
- **AT(TIMESTAMP)**: Point-in-time queries
- **BEFORE(STATEMENT)**: Query state before specific operation

**Ra Gap:** Temporal dimension in scan operators, versioned statistics

---

### 3. Cloud Storage Integration
- **External Tables**: Query S3/Azure/GCS without loading
- **External Stages**: Named cloud storage locations
- **Directory Tables**: Metadata about staged files
- **Metadata Refresh**: Event-driven or manual

**Ra Gap:** Cloud cost model, file format awareness, partition pruning for cloud data

---

### 4. Data Ingestion
- **Snowpipe**: Event-driven continuous loading
- **COPY INTO with Transformations**: ETL during load
- **File Format Support**: CSV, JSON, Avro, Parquet, ORC, XML
- **Stage Querying**: Query files without loading

**Ra Gap:** Not optimizer-relevant (DDL/DML operations)

---

### 5. Change Data Capture
- **Streams**: Track INSERT/UPDATE/DELETE changes
  - Standard: All DML operations
  - Append-only: INSERT only (more efficient)
  - Insert-only: For external tables
- **Tasks**: Schedule SQL statements (CRON or stream-triggered)
- **Task DAGs**: Complex workflows

**Ra Gap:** CDC-aware cost model, stream scan operators

---

### 6. Extensibility
- **UDFs**: SQL, JavaScript, Java, Python, Scala
  - Scalar, UDAF, UDTF variants
  - Vectorized Python UDFs (batch processing)
- **Stored Procedures**: Multi-statement workflows with control flow
- **Caller's vs. Owner's Rights**: Privilege delegation

**Ra Gap:** UDF registry, execution time profiling, batching optimizations

---

### 7. Physical Optimization
- **Micro-Partitions**: 50-150 MB immutable columnar units
- **Clustering Keys**: Co-locate related data (auto-maintained)
- **Search Optimization Service**: Accelerate point lookups
- **Query Acceleration Service**: Serverless compute offloading

**Ra Gap:** Clustering recommendations, workload profiling, pruning benefits

---

### 8. Materialized Views
- **Auto-Refresh**: Background maintenance on base table changes
- **Transparent Rewriting**: Optimizer uses MVs without explicit reference
- **Single-Table Only**: No joins allowed
- **Limitations**: No window functions, UDFs, HAVING, non-deterministic functions

**Ra Gap:** Auto-refresh cost modeling, transparent rewriting (Ra has basic MV matching)

---

### 9. Caching Architecture
- **Result Cache**: 24-hour query result persistence
- **Metadata Cache**: Table/column statistics
- **Warehouse Cache**: Local SSD micro-partition cache
- **Invalidation**: Any data change invalidates cache

**Ra Gap:** Cache hit probability modeling, syntax-normalized fingerprinting

---

### 10. Security and Governance
- **Dynamic Data Masking**: Column-level masking policies
- **Row Access Policies**: Row-level security
- **Secure Views**: Optimization restrictions for security
- **Tag-Based Governance**: Metadata tags for compliance
- **Data Sharing**: Cross-account read-only access

**Ra Gap:** Policy-aware optimization, role-based context

---

### 11. Transactions
- **Isolation Level**: READ COMMITTED only
- **Explicit Transactions**: BEGIN/COMMIT/ROLLBACK
- **Locking**: Partition-level for tables, row-level for hybrid tables
- **MVCC**: Lock-free reads during writes

**Ra Gap:** Partially addressed by RFC 0058 (isolation-aware planning)

---

### 12. Metadata and Monitoring
- **Information Schema Extensions**: Snowflake-specific metadata views
- **Account Usage Views**: 1-year historical data (45 min - 3 hr latency)
- **Query Profiling**: Detailed execution statistics
- **No Query Hints**: Fully automated optimization

**Ra Gap:** Catalog integration, historical query analysis for calibration

---

## Priority Implementation Order

### P0: Core Functionality (Must-Have)
1. **VARIANT/OBJECT/ARRAY** - Fundamental to Snowflake workloads
2. **LATERAL FLATTEN** - Essential for semi-structured data processing

### P1: High-Value Optimizations
3. **Time Travel** - Unique Snowflake feature with optimization implications
4. **External Tables** - Critical for data lake integration
5. **Clustering Keys** - Major performance impact
6. **Materialized Views** - Already partially supported, needs enhancement
7. **Row Access Policies** - Affects query semantics and cost

### P2: Enhanced Optimization
8. **Streams (CDC)** - Important for incremental ETL
9. **Search Optimization** - Point lookup acceleration
10. **Result Caching** - Already supported, needs enhancement
11. **Dynamic Data Masking** - Security feature with minor optimization impact
12. **UDFs** - Common in Snowflake workloads
13. **Transactions** - Already in progress (RFC 0058)
14. **Secure Views** - Security constraint on optimization

### P3: Completeness Features
15. **Query Acceleration Service** - Snowflake-internal optimization
16. **Tag-Based Governance** - Metadata management
17. **Data Sharing** - Cross-account optimization
18. **Information Schema** - Catalog integration
19. **Account Usage** - Historical analysis
20. **Stored Procedures** - Limited optimizer impact

### P4: Non-Optimizer Features
21. **Zero-Copy Cloning** - DDL operation, no query impact
22. **Snowpipe** - Data loading, not query optimization
23. **Tasks** - Orchestration, not query optimization

---

## Key Snowflake Optimization Principles

1. **Micro-Partition Pruning** - Leverage 50-150 MB immutable units with min/max metadata
2. **Cloud Cost Awareness** - Network I/O and serverless compute differ from traditional RDBMS
3. **Semi-Structured First** - VARIANT/JSON are first-class citizens
4. **Metadata-Driven** - Heavy use of metadata for pruning and caching
5. **Separation of Concerns** - Storage and compute are independent
6. **Automatic Maintenance** - Background services handle clustering, MV refresh
7. **Result Caching** - 24-hour cache fundamentally changes repeated query costs
8. **Security Boundaries** - Policies impose hard constraints on optimization

---

## Effort Estimates

| Phase | Duration | Features |
|-------|----------|----------|
| **Phase 1: Semi-Structured Data** | 8-10 weeks | VARIANT/OBJECT/ARRAY, FLATTEN |
| **Phase 2: Cloud Storage** | 6-8 weeks | External Tables, Time Travel |
| **Phase 3: Advanced Optimization** | 8-10 weeks | Clustering, Row Policies, MV enhancements |
| **Phase 4: Additional Features** | 6-8 weeks | Streams, Search Optimization, UDFs |
| **Phase 5: Governance** | 4-6 weeks | Masking, Tags, Account Usage |
| **Total** | **32-42 weeks** | Complete Snowflake support |

---

## Quick Feature Lookup

**Need to handle JSON data?** → VARIANT/OBJECT/ARRAY + FLATTEN
**Need historical queries?** → Time Travel (AT/BEFORE clauses)
**Querying S3 data?** → External Tables
**Slow point lookups?** → Search Optimization Service
**Large table scans?** → Clustering Keys
**Repeated expensive queries?** → Materialized Views
**Need row-level security?** → Row Access Policies
**Column-level security?** → Dynamic Data Masking
**Incremental ETL?** → Streams + Tasks
**Custom logic?** → UDFs or Stored Procedures
**Cross-account sharing?** → Data Sharing + Secure Views
**Cost attribution?** → Tags on warehouses
**Performance debugging?** → Query Profile + Account Usage

---

## Next Steps

1. Review full analysis: [SNOWFLAKE_FEATURES_GAP_ANALYSIS.md](./SNOWFLAKE_FEATURES_GAP_ANALYSIS.md)
2. Prioritize features based on user workload patterns
3. Begin Phase 1 implementation: Semi-Structured Data Foundation
4. Integrate with Snowflake metadata catalog (Information Schema)
5. Implement cloud-aware cost model
6. Build workload profiling for clustering recommendations
7. Extend security-aware optimization for policies

---

## References

- [Snowflake Documentation](https://docs.snowflake.com/)
- Ra RFC 0058: Isolation-Aware Planning (`/home/gburd/ws/ra/rfcs/0058-isolation-aware-planning.md`)
- Ra MV Matching: `/home/gburd/ws/ra/crates/ra-engine/src/mv_matching.rs`
- Ra Caching: `/home/gburd/ws/ra/crates/ra-cache/`
- Ra Isolation Support: `/home/gburd/ws/ra/crates/ra-core/src/isolation.rs`
