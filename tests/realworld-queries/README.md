# Real-World SQL Query Test Suite

Curated collection of production SQL patterns for testing Ra's query optimizer.

## Overview

This directory contains real-world SQL queries extracted from common application patterns and frameworks. The queries represent actual production workloads and serve as a comprehensive test suite for Ra's parsing, optimization, and distributed execution capabilities.

## Directory Structure

```
tests/realworld-queries/
|---- README.md                           # This file
|---- DISTRIBUTED_PATTERNS.md             # Analysis of distributed query patterns
|---- SCHEMA_PATTERNS.md                  # Schema statistics and facts
|---- WORKLOAD_CHARACTERISTICS.md         # Workload analysis
|---- RESULTS.md                          # Test results and gap analysis
|---- github/
|   |---- django-migrations/              # Django ORM patterns
|   |   |---- 001_initial_user_model.sql
|   |   `---- 002_ecommerce_orders.sql
|   |---- rails-activerecord/             # Rails ActiveRecord patterns
|   |   |---- 001_blog_posts_comments.sql
|   |   `---- 002_multi_tenant_saas.sql
|   |---- dbt-models/                     # Data warehouse transformations
|   |   |---- 001_customer_lifetime_value.sql
|   |   |---- 002_daily_metrics_rollup.sql
|   |   `---- 003_funnel_analysis.sql
|   |---- postgres-extensions/            # PostgreSQL extensions
|   |   |---- 001_timescaledb_iot_data.sql
|   |   `---- 002_postgis_geospatial.sql
|   `---- airflow-dags/                   # ETL pipelines
|       |---- 001_etl_pipeline.sql
|       `---- 002_data_quality_checks.sql
|---- codeberg/
|   |---- gitea_repository_analytics.sql  # Git forge analytics
|   `---- forum_activity.sql              # Forum/discussion patterns
`---- gitea/                              # (Reserved for future)
```

## Query Categories

### 1. OLTP Queries (Transactional)
**Sources**: Django, Rails
**Characteristics**:
- Point lookups by primary key
- User-scoped queries
- Simple 2-3 table joins
- Pagination with LIMIT/OFFSET
- Counter caches

**Example Applications**:
- E-commerce order processing
- User authentication
- Content management systems
- Multi-tenant SaaS

### 2. OLAP Queries (Analytical)
**Sources**: dbt, Airflow
**Characteristics**:
- Large aggregations (GROUP BY)
- Complex CTEs
- Window functions
- Multi-table star schema joins
- Statistical functions

**Example Applications**:
- Customer lifetime value
- Sales dashboards
- Funnel analysis
- Daily metrics rollups

### 3. Time-Series Queries
**Sources**: TimescaleDB patterns
**Characteristics**:
- Time range filtering
- Time bucketing (hourly, daily)
- Downsampling
- Gap detection
- Anomaly detection

**Example Applications**:
- IoT sensor data
- System monitoring
- Financial tick data
- Application metrics

### 4. Geospatial Queries
**Sources**: PostGIS patterns
**Characteristics**:
- Distance calculations
- Point-in-polygon tests
- Nearest neighbor searches
- Spatial clustering

**Example Applications**:
- Location-based services
- Delivery routing
- Real estate search
- Geofencing

### 5. ETL Queries
**Sources**: Airflow, data pipelines
**Characteristics**:
- Batch transformations
- MERGE/UPSERT operations
- Data quality checks
- Incremental processing

**Example Applications**:
- Data warehouse loading
- Data validation
- Schema migrations
- Metric aggregation

## SQL Features Covered

### Standard SQL
- [x] SELECT, FROM, WHERE, JOIN
- [x] GROUP BY, HAVING
- [x] ORDER BY, LIMIT, OFFSET
- [x] UNION, UNION ALL
- [x] Subqueries (scalar, correlated)
- [x] CTEs (WITH clause)
- [x] Window functions (OVER, PARTITION BY)
- [x] CASE expressions
- [x] Aggregate functions (COUNT, SUM, AVG, MIN, MAX)

### PostgreSQL Extensions
- FILTER clause
- LATERAL joins
- ARRAY operations
- PERCENTILE functions
- DATE_TRUNC, EXTRACT
- INTERVAL arithmetic
- time_bucket() (TimescaleDB)
- PostGIS spatial functions
- MERGE statement

## Distributed Query Patterns

### 1. Single-Shard Routing (80% of queries)
```sql
WHERE tenant_id = 123  -- Route to specific shard
WHERE user_id = 456    -- Hash to shard
```

### 2. Partition Pruning (70% of queries)
```sql
WHERE created_at >= NOW() - INTERVAL '30 days'
```

### 3. Co-located Joins (40% of queries)
```sql
FROM orders o
JOIN order_items oi ON o.id = oi.order_id
WHERE o.user_id = 123  -- Single-shard execution
```

### 4. Broadcast Joins (50% of queries)
```sql
FROM orders o  -- Large, sharded
JOIN products p ON o.product_id = p.id  -- Small, replicated
```

### 5. Aggregation Pushdown (60% of queries)
```sql
SELECT
    DATE_TRUNC('day', created_at),
    COUNT(*), SUM(amount)
FROM orders
GROUP BY DATE_TRUNC('day', created_at)
```

## Using the Test Suite

### Running Tests

```bash
# Parse all queries
for file in tests/realworld-queries/**/*.sql; do
    echo "Testing $file"
    cargo run --bin ra-cli -- parse "$file"
done

# Optimize specific query
cargo run --bin ra-cli -- optimize "$(cat tests/realworld-queries/github/dbt-models/001_customer_lifetime_value.sql)"

# With statistics
cargo run --bin ra-cli -- optimize \
    --stats tests/realworld-queries/github/dbt-models/stats.json \
    "$(cat tests/realworld-queries/github/dbt-models/001_customer_lifetime_value.sql)"
```

### Adding New Queries

1. Identify source (GitHub, Codeberg, production logs)
2. Extract representative queries
3. Document schema and statistics
4. Add to appropriate directory
5. Update documentation

**Template**:
```sql
-- Query Title/Description
-- Source: Application name, framework
-- Pattern: OLTP/OLAP/Time-series/Geospatial/ETL
-- Distributed: Single-shard/Multi-shard/Broadcast

-- Schema (if not already defined)
CREATE TABLE ...

-- Query
SELECT ...
```

## Schema and Statistics

Each query file documents:
- Table schemas (CREATE TABLE)
- Index definitions (CREATE INDEX)
- Typical row counts
- Data distributions
- Query patterns

For detailed statistics modeling, see:
- `SCHEMA_PATTERNS.md`: Example statistics
- `docs/guides/modeling-production-workloads.md`: Collection guide

## Documentation Files

### DISTRIBUTED_PATTERNS.md
Analysis of distributed query patterns:
- Shard key filtering
- Partition pruning
- Co-located joins
- Broadcast joins
- Aggregation pushdown
- Geographic sharding
- Multi-tenant isolation

### SCHEMA_PATTERNS.md
Schema statistics and facts:
- Table row counts and sizes
- Column cardinality and distributions
- Index definitions and selectivity
- Foreign key relationships
- Shard/partition information
- Workload characteristics

### WORKLOAD_CHARACTERISTICS.md
Workload profiles:
- OLTP vs OLAP mix
- Query latency requirements
- Concurrency levels
- Read/write ratios
- Data access patterns
- Hot/warm/cold data

### RESULTS.md
Test results:
- Parse success rates
- Query coverage analysis
- Gap identification
- Optimization impact
- Recommendations

## Statistics Collection

See `docs/guides/modeling-production-workloads.md` for:
- How to extract statistics from production databases
- Mapping schemas to Ra facts
- Providing shard/partition information
- Modeling workload characteristics

## Contributing

To add queries from a new source:

1. **Identify representative queries**:
   - Common application patterns
   - Diverse SQL features
   - Challenging optimization cases

2. **Document thoroughly**:
   - Source application/framework
   - Query pattern type
   - Schema context
   - Expected statistics

3. **Test with Ra**:
   - Verify parsing
   - Check logical plan
   - Test optimization rules

4. **Update documentation**:
   - Add to RESULTS.md
   - Document any gaps
   - Update coverage metrics

## Related Documentation

- `docs/testing/realworld-coverage.md`: Detailed coverage analysis
- `docs/guides/modeling-production-workloads.md`: Statistics guide
- `docs/optimization/`: Optimization techniques
- `rfcs/`: Query optimizer RFCs

## Query Statistics

- **Total files**: 13
- **Total queries**: 70+
- **Schemas documented**: 30+ tables
- **Patterns covered**: 10 major categories
- **SQL features**: 50+ constructs

## Next Steps

1. Fix ra-cli compilation issues
2. Run parser tests on all queries
3. Document parse failures
4. Add statistics files
5. Test distributed optimization
6. Create integration tests
7. Benchmark performance
