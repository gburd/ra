# Distributed Query Patterns

Analysis of distributed database patterns found in real-world SQL queries.

## 1. Multi-Tenant Sharding Patterns

### Tenant-ID Based Sharding
**Query Source**: `rails-activerecord/002_multi_tenant_saas.sql`

**Pattern**: Every table includes a `tenant_id` column used for:
- Data isolation between customers
- Query filtering (always present in WHERE clause)
- Shard key for horizontal partitioning

**Distribution Strategy**:
```sql
-- Shard key: tenant_id
-- Each tenant's data co-located on same shard
CREATE TABLE projects (
    tenant_id INTEGER NOT NULL,  -- SHARD KEY
    id BIGINT PRIMARY KEY,
    ...
) PARTITION BY HASH(tenant_id);
```

**Optimization Opportunities**:
- Single-tenant queries can be routed to specific shard
- Cross-tenant queries require scatter-gather
- Ra could optimize by recognizing tenant_id predicates

**Statistics Needed**:
- Tenant data size distribution
- Tenant query frequency
- Hot tenant identification

---

## 2. Time-Series Partitioning

### TimescaleDB Hypertables
**Query Source**: `postgres-extensions/001_timescaledb_iot_data.sql`

**Pattern**: Time-based chunking for efficient retention and query pruning
```sql
-- Partitioned by time (automatic with TimescaleDB)
CREATE TABLE sensor_readings (
    time TIMESTAMP NOT NULL,  -- PARTITION KEY
    sensor_id INTEGER NOT NULL,
    ...
);

-- Queries typically have time range predicates
WHERE time >= NOW() - INTERVAL '1 hour'
```

**Distribution Strategy**:
- Chunk data by time intervals (1 day, 1 week)
- Recent data on fast storage (SSD)
- Old data compressed or archived
- Queries typically scan recent partitions

**Optimization Opportunities**:
- Partition pruning based on time predicates
- Parallel scan of multiple time chunks
- Different compression per partition
- Ra could recognize time-based partition keys

**Statistics Needed**:
- Data volume per time chunk
- Query time range patterns (recent vs historical)
- Compression ratios per partition

---

## 3. Geographic Sharding

### Location-Based Partitioning
**Query Source**: `postgres-extensions/002_postgis_geospatial.sql`

**Pattern**: Data partitioned by geographic region
```sql
-- Common pattern: Shard by country or region
CREATE TABLE locations (
    country VARCHAR(100),  -- POTENTIAL SHARD KEY
    coordinates GEOMETRY(Point, 4326),
    ...
);

-- Queries often filter by location
WHERE ST_DWithin(coordinates, point, distance)
```

**Distribution Strategy**:
- Partition by country, state, or region
- Co-locate related geographic data
- Support for geo-bounded queries

**Optimization Opportunities**:
- Spatial index on each shard
- Route queries to specific geographic shards
- Cross-shard queries for boundary cases
- Ra could recognize spatial predicates

**Statistics Needed**:
- Data distribution across regions
- Query patterns (local vs global)
- Hotspot regions

---

## 4. User/Entity Sharding

### User-ID Based Distribution
**Query Source**: `django-migrations/002_ecommerce_orders.sql`

**Pattern**: Related entities sharded by user_id
```sql
-- Orders sharded by user_id
CREATE TABLE orders (
    user_id INTEGER NOT NULL,  -- SHARD KEY
    id BIGINT PRIMARY KEY,
    ...
) PARTITION BY HASH(user_id);

-- Order items co-located with orders
CREATE TABLE order_items (
    order_id BIGINT NOT NULL,  -- FOREIGN KEY, implies user_id shard
    ...
);

-- Queries typically filter by user
WHERE user_id = 12345
```

**Distribution Strategy**:
- Hash or range partition by user_id
- Co-locate user's orders and items
- Avoid cross-shard joins

**Optimization Opportunities**:
- Single-user queries stay on one shard
- Analytics queries need scatter-gather
- Ra could detect co-located joins

**Statistics Needed**:
- User activity distribution (power users)
- Average orders per user
- Hot user identification

---

## 5. Hybrid Time + Entity Sharding

### Composite Sharding
**Query Source**: Multiple sources combining patterns

**Pattern**: Two-level partitioning
```sql
-- First partition by time, then by entity
PARTITION BY RANGE(created_at)
    SUBPARTITION BY HASH(user_id)
```

**Example Use Cases**:
1. **Event logs**: Partition by date, sub-partition by user
2. **Transactions**: Partition by month, sub-partition by account
3. **Sensor data**: Partition by time, sub-partition by location

**Optimization Opportunities**:
- Prune on both dimensions
- Support queries with either predicate
- Balance data across shards

**Statistics Needed**:
- Data distribution across both dimensions
- Query predicate selectivity
- Correlation between dimensions

---

## 6. Reference Table Broadcasting

### Small Dimension Tables
**Query Source**: All queries with dimension joins

**Pattern**: Small lookup tables replicated to all shards
```sql
-- Small, rarely-updated reference data
CREATE TABLE categories (
    id INTEGER PRIMARY KEY,
    name VARCHAR(255),
    ...
);  -- REPLICATED TO ALL SHARDS

-- Large fact table, sharded
CREATE TABLE products (
    category_id INTEGER,  -- JOIN to replicated table
    ...
) PARTITION BY HASH(id);

-- Join doesn't require cross-shard communication
JOIN categories c ON p.category_id = c.id
```

**Candidates for Broadcasting**:
- Categories, tags
- Countries, states
- Product types
- Status codes

**Optimization Opportunities**:
- Local joins without network I/O
- Ra could identify small tables for replication
- Update propagation strategy

**Statistics Needed**:
- Table size (row count, bytes)
- Update frequency
- Join frequency

---

## 7. Aggregation Pushdown Patterns

### Two-Phase Aggregation
**Query Source**: `dbt-models/002_daily_metrics_rollup.sql`

**Pattern**: Partial aggregation on shards, final aggregation on coordinator
```sql
-- This query benefits from pushdown:
SELECT
    DATE_TRUNC('day', created_at) AS metric_date,
    COUNT(*) AS order_count,
    SUM(total_amount) AS total_revenue
FROM orders  -- SHARDED TABLE
WHERE created_at >= CURRENT_DATE - INTERVAL '90 days'
GROUP BY DATE_TRUNC('day', created_at);
```

**Distributed Execution**:
1. **Phase 1 (on each shard)**:
   ```sql
   SELECT date, partial_count, partial_sum
   FROM local_orders
   GROUP BY date
   ```

2. **Phase 2 (coordinator)**:
   ```sql
   SELECT date, SUM(partial_count), SUM(partial_sum)
   FROM shard_results
   GROUP BY date
   ```

**Optimization Opportunities**:
- Push GROUP BY to shards
- Reduce data transfer
- Parallel aggregation
- Ra could recognize distributive aggregates (COUNT, SUM)

**Statistics Needed**:
- Cardinality of GROUP BY columns
- Data volume reduction from aggregation
- Network bandwidth

---

## 8. Co-located Join Patterns

### Shard Key Join
**Query Source**: `django-migrations/002_ecommerce_orders.sql`

**Pattern**: Join on shard key avoids reshuffling
```sql
-- Both tables sharded by user_id
SELECT o.*, p.*
FROM orders o  -- PARTITION BY HASH(user_id)
JOIN order_items oi ON o.id = oi.order_id  -- FK implies same user_id
JOIN products p ON oi.product_id = p.id
WHERE o.user_id = 12345;
```

**Distribution Strategy**:
- order_items co-located with orders (via FK)
- products may be replicated or require shuffle
- Single-shard execution for user-specific queries

**Optimization Opportunities**:
- Detect co-location via foreign keys
- Avoid unnecessary data movement
- Ra could infer shard key propagation through FKs

---

## 9. Cross-Shard Join Patterns

### Broadcast Join
**Query Source**: `rails-activerecord/001_blog_posts_comments.sql`

**Pattern**: Large table join with small table
```sql
-- posts: large, sharded by author_id
-- users: small, can be broadcast
SELECT p.*, u.username
FROM posts p  -- SHARDED
JOIN users u ON p.author_id = u.id;  -- BROADCAST
```

**Strategy**: Broadcast small table to all shards

### Shuffle Join
**Pattern**: Large-to-large join on non-shard key
```sql
-- Both tables sharded differently
SELECT o.*, u.email
FROM orders o  -- PARTITION BY order_id
JOIN users u ON o.user_id = u.id;  -- PARTITION BY user_id
```

**Strategy**: Repartition one or both tables

**Optimization Opportunities**:
- Choose broadcast vs shuffle based on size
- Ra needs table statistics to decide

---

## 10. Window Function Distribution

### Partitioned Window Functions
**Query Source**: `dbt-models/002_daily_metrics_rollup.sql`

**Pattern**: Window functions aligned with sharding
```sql
-- If sharded by metric_date, can execute per-shard
SELECT
    metric_date,
    order_count,
    AVG(order_count) OVER (
        ORDER BY metric_date
        ROWS BETWEEN 6 PRECEDING AND CURRENT ROW
    ) AS moving_avg
FROM daily_metrics
PARTITION BY metric_date;  -- Aligned with shard key
```

**Optimization Opportunities**:
- Execute window functions per-shard when possible
- Coordinate window functions require data movement
- Ra could detect shard-aligned windows

---

## Summary: Ra Optimization Opportunities

### 1. Shard Key Recognition
- Identify columns used for partitioning
- Recognize tenant_id, user_id, time-based patterns
- Infer from query predicates

### 2. Partition Pruning
- Time-based pruning (very common)
- Entity-based pruning (user_id, tenant_id)
- Spatial pruning (geography)

### 3. Join Co-location
- Detect when join keys match shard keys
- Infer co-location via foreign keys
- Avoid unnecessary shuffles

### 4. Aggregation Pushdown
- Push distributive aggregates (SUM, COUNT, MIN, MAX)
- Two-phase execution
- Reduce data transfer

### 5. Small Table Replication
- Identify dimension tables for broadcasting
- Size-based heuristics
- Update frequency considerations

### 6. Query Routing
- Single-shard query detection
- Scatter-gather patterns
- Hot shard avoidance

### Statistics Required
See: `SCHEMA_PATTERNS.md` for detailed statistics modeling
