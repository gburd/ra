# Workload Characteristics

Analysis of real-world workload patterns for query optimization.

## 1. E-Commerce Workload

**Source**: `django-migrations/002_ecommerce_orders.sql`

### Query Mix
- **OLTP (80%)**:
  - Point queries by order_id: 40%
  - User order history: 25%
  - Product lookup: 15%
- **OLAP (20%)**:
  - Daily sales reports: 10%
  - Top products: 5%
  - Inventory analysis: 5%

### Latency Requirements
- **OLTP**: p99 < 50ms
- **OLAP**: p99 < 5s

### Concurrency
- **Peak QPS**: 1000 (orders), 500 (analytics)
- **Connection pool**: 100-200 connections

### Data Access Patterns
- **Recent bias**: 80% queries touch last 30 days
- **User locality**: 90% queries filtered by user_id
- **Read/Write ratio**: 90/10

---

## 2. Multi-Tenant SaaS Workload

**Source**: `rails-activerecord/002_multi_tenant_saas.sql`

### Query Mix
- **Single-tenant queries**: 99%
- **Cross-tenant admin queries**: 1%

### Tenant Characteristics
- **Active tenants**: 30% of total (daily)
- **Hot tenants**: 10% account for 80% of traffic
- **Tenant size distribution**: Power law
  - Small tenants (<100 projects): 70%
  - Medium tenants (100-1000 projects): 25%
  - Large tenants (>1000 projects): 5%

### Isolation Requirements
- **Strict tenant isolation**: Critical for security
- **Query must always have tenant_id predicate**
- **Cross-tenant joins**: Not allowed

### Resource Allocation
- **Per-tenant quotas**: CPU, storage, QPS
- **Hot tenant throttling**: Required
- **Shard rebalancing**: Periodic (for tenant growth)

---

## 3. Time-Series IoT Workload

**Source**: `postgres-extensions/001_timescaledb_iot_data.sql`

### Query Mix
- **Recent data queries (last 1 hour)**: 80%
- **Historical queries (last 30 days)**: 15%
- **Archive queries (>30 days)**: 5%

### Write Characteristics
- **Ingestion rate**: 10K-100K rows/sec
- **Batch vs streaming**:
  - Streaming: 60%
  - Batch (hourly): 40%
- **Write amplification**: Minimal (append-only)

### Read Characteristics
- **Time range queries**: 95%
- **Point queries**: 5%
- **Aggregation level**:
  - Raw data: 20%
  - Downsampled (hourly): 50%
  - Downsampled (daily): 30%

### Retention Policy
- **Hot (last 7 days)**: SSD, no compression
- **Warm (7-90 days)**: SSD, compressed
- **Cold (>90 days)**: S3/archive, highly compressed

### Query Optimization Opportunities
- **Partition pruning**: Critical (time-based)
- **Compression**: High ratio (10:1 typical)
- **Downsampling**: Pre-aggregate common queries
- **Late materialization**: Defer column reads

---

## 4. Analytics Warehouse Workload

**Source**: `dbt-models/001_customer_lifetime_value.sql`

### Query Mix
- **Ad-hoc queries**: 30%
- **Dashboard queries**: 50%
- **Batch ETL**: 20%

### Query Characteristics
- **Average query duration**: 5-30 seconds
- **Scan percentage**: 10-50% of table
- **Join complexity**: 3-8 tables typical
- **Aggregation heavy**: 90% of queries

### Batch Processing
- **Daily ETL windows**: 2-4 hours
- **Dependencies**: DAG of transformations
- **Incremental updates**: 80% of jobs
- **Full refreshes**: 20% of jobs

### Concurrency
- **Peak concurrent queries**: 50-100
- **Resource contention**: Common during ETL
- **Query queuing**: Necessary for large scans

### Optimization Opportunities
- **Materialized views**: For dashboards
- **Pre-aggregation**: Common group-by patterns
- **Partitioning**: By date (most queries)
- **Columnar storage**: High compression, fast scans
- **Result caching**: Frequently-run queries

---

## 5. Geospatial Workload

**Source**: `postgres-extensions/002_postgis_geospatial.sql`

### Query Mix
- **Nearest neighbor**: 60%
- **Within radius**: 30%
- **Within polygon**: 10%

### Spatial Characteristics
- **Query radius distribution**:
  - <1km: 50%
  - 1-5km: 30%
  - 5-10km: 15%
  - >10km: 5%

### Geographic Distribution
- **Urban hotspots**: 70% of queries
- **Rural areas**: 20% of queries
- **Remote areas**: 10% of queries

### Index Usage
- **Spatial index hit rate**: >95%
- **Index type**: GiST R-tree
- **Index selectivity**: Highly variable by density

### Optimization Opportunities
- **Spatial partitioning**: By geohash or quadtree
- **Hot region caching**: In-memory for cities
- **Approximate neighbors**: For large radius queries
- **Spatial pre-filtering**: Bounding box then precise

---

## 6. Forum/Social Workload

**Source**: `codeberg/forum_activity.sql`

### Query Mix
- **Homepage (hot topics)**: 40%
- **Topic detail (with posts)**: 30%
- **User profile**: 15%
- **Search**: 10%
- **Moderation**: 5%

### Content Distribution
- **Active topics (last 7 days)**: 5% of total, 80% of traffic
- **Popular topics**: Power law (1% topics = 50% views)
- **Zombie topics**: 50% of topics have no activity in 90 days

### Read/Write Ratio
- **Read**: 95%
- **Write**: 5%

### Cache Hit Rates
- **Hot topics**: 90% cache hit
- **User sessions**: 80% cache hit
- **Full-text search**: 50% cache hit

### Optimization Opportunities
- **Denormalization**: Post counts, last post time
- **Counter caches**: Views, likes
- **Hot content caching**: Recent posts in memory
- **Search index**: Separate from primary DB

---

## 7. ETL Pipeline Workload

**Source**: `airflow-dags/001_etl_pipeline.sql`

### Job Characteristics
- **Batch size**: 1M - 100M rows per job
- **Frequency**:
  - Hourly: 40%
  - Daily: 50%
  - Weekly: 10%

### Data Flow
1. **Extract**: Read from source (can be slow)
2. **Transform**: CPU-intensive (joins, aggregations)
3. **Load**: Write to target (can cause contention)

### Dependencies
- **DAG depth**: 3-10 stages typical
- **Parallel branches**: 5-20 jobs
- **Critical path**: Determines total runtime

### Resource Usage
- **CPU**: High during transformation
- **Memory**: Large for joins
- **Disk I/O**: High for staging
- **Network**: High for data movement

### Optimization Opportunities
- **Incremental processing**: Load only new data
- **Predicate pushdown**: Filter at source
- **Columnar formats**: Fast scanning (Parquet)
- **Compression**: Reduce I/O
- **Partition alignment**: Avoid reshuffling

---

## 8. Data Quality Workload

**Source**: `airflow-dags/002_data_quality_checks.sql`

### Check Types
- **Null checks**: 20%
- **Referential integrity**: 20%
- **Range validation**: 20%
- **Duplicate detection**: 15%
- **Freshness checks**: 10%
- **Distribution consistency**: 15%

### Execution Frequency
- **Per batch**: 60%
- **Hourly**: 30%
- **Daily**: 10%

### Failure Modes
- **Hard failures**: Stop pipeline
- **Soft failures**: Log and alert
- **Warnings**: Record for analysis

### Query Patterns
- **Full table scans**: Common for null checks
- **Aggregations**: For statistical validation
- **Joins**: For referential integrity
- **Window functions**: For anomaly detection

---

## Workload Summary by Pattern

| Workload Type | OLTP % | OLAP % | Latency (p99) | QPS | Hot Data |
|---------------|--------|--------|---------------|-----|----------|
| E-commerce | 80 | 20 | 50ms / 5s | 1000 | 30 days |
| Multi-tenant | 95 | 5 | 100ms / 10s | 500 | Current |
| Time-series | 20 | 80 | N/A / 5s | 100 | 1 hour |
| Analytics | 5 | 95 | N/A / 30s | 50 | 90 days |
| Geospatial | 70 | 30 | 100ms / 2s | 200 | Urban |
| Forum/Social | 90 | 10 | 50ms / 2s | 500 | 7 days |
| ETL Pipeline | 0 | 100 | N/A / 1h | 10 | Incremental |
| Data Quality | 0 | 100 | N/A / 5m | 20 | Current |

---

## Key Insights for Ra Optimization

### 1. Recency Bias
Most workloads heavily favor recent data:
- E-commerce: 80% queries on last 30 days
- Time-series: 80% queries on last 1 hour
- Forum: 80% queries on last 7 days

**Implication**: Partition pruning on time columns is critical

### 2. OLTP vs OLAP Split
Many workloads have mixed OLTP/OLAP:
- Different latency requirements
- Different query patterns
- Different optimization strategies

**Implication**: Ra needs separate optimization paths

### 3. Data Skew
Nearly all workloads have power law distributions:
- Users (10% = 50% of activity)
- Tenants (10% = 80% of traffic)
- Content (1% = 50% of views)

**Implication**: Shard balancing and hot spot detection needed

### 4. Co-location Patterns
Most schemas have parent-child relationships:
- orders -> order_items
- topics -> posts
- projects -> tasks

**Implication**: Co-located joins should be optimized

### 5. Reference Data
Small dimension tables in every workload:
- products, categories, users
- Often joined to large fact tables

**Implication**: Broadcasting small tables is beneficial

### 6. Aggregation Heavy
Analytics queries dominated by aggregations:
- GROUP BY on date, user, category
- SUM, COUNT, AVG, PERCENTILE
- Window functions common

**Implication**: Pushdown and two-phase aggregation critical
