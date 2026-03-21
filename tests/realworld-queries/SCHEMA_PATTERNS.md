# Schema Patterns and Statistics

Analysis of schema design patterns and required statistics for query optimization.

## 1. E-Commerce Schema

**Source**: `django-migrations/002_ecommerce_orders.sql`

### Tables

#### orders
- **Row count**: 10M - 100M (typical mid-size store)
- **Growth rate**: ~1000-10000 orders/day
- **Size**: ~500 bytes/row → 5GB - 50GB
- **Distribution**:
  - 70% completed, 20% pending, 5% processing, 5% canceled
  - Power law on user_id (top 10% users = 50% orders)
- **Hot data**: Last 90 days = 80% of queries
- **Indexes**:
  - PRIMARY KEY (id) - BTREE
  - INDEX (user_id) - BTREE - Selectivity ~0.001
  - INDEX (status) - BTREE - Selectivity ~0.25
  - INDEX (created_at) - BTREE - Highly selective for recent dates

#### order_items
- **Row count**: 25M - 250M (avg 2.5 items/order)
- **Size**: ~200 bytes/row → 5GB - 50GB
- **FK Cardinality**: order_id → orders (many-to-one, avg 2.5)
- **Distribution**:
  - Top 20% products = 80% of items (Pareto)
- **Join patterns**:
  - order_items → orders: Nearly always co-queried
  - order_items → products: Dimension join

#### products
- **Row count**: 10K - 100K (catalog size)
- **Size**: ~1KB/row → 10MB - 100MB (small enough to broadcast)
- **Update frequency**: High (price, stock changes)
- **Join patterns**: Always via product_id FK

### Query Patterns

**OLTP**: Point lookups by order_id, user_id
- Latency target: <10ms
- QPS: 100-1000

**OLAP**: Aggregations over time ranges
- Latency target: <5s
- QPS: 1-10

### Facts for Ra

```json
{
  "tables": {
    "orders": {
      "row_count": 50000000,
      "size_bytes": 25000000000,
      "columns": {
        "id": {"cardinality": 50000000, "null_frac": 0.0},
        "user_id": {"cardinality": 2000000, "null_frac": 0.0},
        "status": {"cardinality": 5, "null_frac": 0.0},
        "created_at": {"cardinality": 3650, "null_frac": 0.0}
      },
      "indexes": {
        "orders_pkey": {"keys": ["id"], "unique": true},
        "idx_orders_user_id": {"keys": ["user_id"]},
        "idx_orders_status": {"keys": ["status"]},
        "idx_orders_created_at": {"keys": ["created_at"]}
      },
      "distribution": {
        "type": "hash",
        "key": "user_id",
        "shard_count": 16
      }
    },
    "order_items": {
      "row_count": 125000000,
      "size_bytes": 25000000000,
      "columns": {
        "order_id": {"cardinality": 50000000, "null_frac": 0.0},
        "product_id": {"cardinality": 50000, "null_frac": 0.0}
      },
      "foreign_keys": [
        {"from": "order_id", "to": "orders.id", "avg_refs": 2.5}
      ],
      "distribution": {
        "type": "co-located",
        "parent": "orders",
        "key": "order_id"
      }
    },
    "products": {
      "row_count": 50000,
      "size_bytes": 50000000,
      "replicated": true
    }
  }
}
```

---

## 2. Multi-Tenant SaaS Schema

**Source**: `rails-activerecord/002_multi_tenant_saas.sql`

### Tables

#### tenants
- **Row count**: 1K - 100K
- **Size**: Small metadata table
- **Distribution**: Broadcast/replicated

#### projects
- **Row count**: 100K - 10M
- **Distribution**:
  - Highly skewed by tenant (some tenants have 1000s of projects)
  - Top 10% tenants = 90% of projects
- **Shard key**: tenant_id

#### tasks
- **Row count**: 10M - 1B
- **Distribution**:
  - Sharded by tenant_id
  - Co-located with projects
- **Indexes**:
  - Composite: (tenant_id, project_id)
  - Composite: (tenant_id, assignee_id)

### Facts for Ra

```json
{
  "tables": {
    "projects": {
      "row_count": 5000000,
      "columns": {
        "tenant_id": {"cardinality": 10000, "null_frac": 0.0}
      },
      "distribution": {
        "type": "hash",
        "key": "tenant_id",
        "shard_count": 32
      },
      "tenant_distribution": {
        "avg_rows_per_tenant": 500,
        "p50_rows_per_tenant": 100,
        "p99_rows_per_tenant": 5000,
        "max_rows_per_tenant": 50000
      }
    },
    "tasks": {
      "row_count": 50000000,
      "columns": {
        "tenant_id": {"cardinality": 10000, "null_frac": 0.0},
        "project_id": {"cardinality": 5000000, "null_frac": 0.0}
      },
      "distribution": {
        "type": "co-located",
        "parent": "projects",
        "key": "tenant_id"
      }
    }
  },
  "query_patterns": {
    "single_tenant": 0.99,
    "cross_tenant": 0.01
  }
}
```

---

## 3. Time-Series IoT Schema

**Source**: `postgres-extensions/001_timescaledb_iot_data.sql`

### Tables

#### sensor_readings
- **Row count**: 1B - 1T (very high volume)
- **Ingestion rate**: 10K - 1M rows/second
- **Retention**: 90 days hot, 2 years warm, archive older
- **Size**: ~100 bytes/row
- **Distribution**:
  - Partitioned by time (1-day chunks)
  - Each chunk: 864M - 8.6B rows
  - Recent chunks heavily queried

### Query Patterns

**Hot queries**: Last 1 hour (99% of queries)
**Warm queries**: Last 30 days
**Cold queries**: Historical analysis (rare)

### Facts for Ra

```json
{
  "tables": {
    "sensor_readings": {
      "row_count": 100000000000,
      "size_bytes": 10000000000000,
      "columns": {
        "time": {"cardinality": 7776000, "null_frac": 0.0},
        "sensor_id": {"cardinality": 10000, "null_frac": 0.0}
      },
      "distribution": {
        "type": "range",
        "key": "time",
        "partitions": [
          {"range": "2024-01-01 to 2024-01-02", "rows": 864000000},
          {"range": "2024-01-02 to 2024-01-03", "rows": 864000000}
        ]
      },
      "time_characteristics": {
        "partition_interval": "1 day",
        "hot_data_cutoff": "1 hour",
        "warm_data_cutoff": "30 days",
        "cold_data_cutoff": "90 days"
      },
      "query_time_distribution": {
        "last_1_hour": 0.80,
        "last_24_hours": 0.15,
        "last_30_days": 0.04,
        "older": 0.01
      }
    }
  }
}
```

---

## 4. Analytics/Warehouse Schema

**Source**: `dbt-models/001_customer_lifetime_value.sql`

### Tables (Fact and Dimension)

#### orders (Fact Table)
- **Row count**: 100M - 1B
- **Grain**: One row per order
- **Dimensions**: user_id, created_at
- **Measures**: total_amount

#### users (Dimension Table)
- **Row count**: 1M - 10M
- **Type**: Slowly changing dimension (SCD Type 1)
- **Size**: Small enough to broadcast

### Query Patterns

**Aggregation queries**: 90% of workload
- GROUP BY date, user segments
- Large scans with filtering
- Window functions common

**Join patterns**:
- Star schema: fact table → dimension tables
- Dimension tables replicated

### Facts for Ra

```json
{
  "tables": {
    "orders": {
      "row_count": 500000000,
      "size_bytes": 250000000000,
      "table_type": "fact",
      "distribution": {
        "type": "hash",
        "key": "user_id",
        "shard_count": 64
      }
    },
    "users": {
      "row_count": 5000000,
      "size_bytes": 1000000000,
      "table_type": "dimension",
      "distribution": {
        "type": "replicated"
      }
    }
  },
  "workload": {
    "olap_percentage": 0.95,
    "oltp_percentage": 0.05,
    "avg_scan_percentage": 0.3,
    "avg_result_percentage": 0.001
  }
}
```

---

## 5. Geospatial Schema

**Source**: `postgres-extensions/002_postgis_geospatial.sql`

### Tables

#### locations
- **Row count**: 1M - 100M (POIs)
- **Spatial distribution**: Non-uniform (cities have more POIs)
- **Indexes**:
  - GIST index on coordinates (spatial)
  - R-tree structure

#### delivery_zones
- **Row count**: 100 - 10K (polygons)
- **Size**: Small, can be replicated
- **Spatial index**: GIST on boundary

### Query Patterns

**Nearest neighbor**: Very common
- Use KNN index scan
- Typically LIMIT to small result sets

**Within/Contains**: Common
- Point-in-polygon tests
- Can benefit from spatial partitioning

### Facts for Ra

```json
{
  "tables": {
    "locations": {
      "row_count": 10000000,
      "columns": {
        "coordinates": {
          "type": "geometry",
          "spatial_index": {
            "type": "gist",
            "bounding_box": {
              "min_lat": 37.0, "max_lat": 38.0,
              "min_lon": -123.0, "max_lon": -122.0
            }
          }
        },
        "category": {"cardinality": 50}
      },
      "distribution": {
        "type": "geohash",
        "precision": 6,
        "hot_regions": ["9q8yy", "9q8yv"]
      }
    }
  },
  "query_patterns": {
    "nearest_neighbor": 0.60,
    "within_distance": 0.30,
    "within_polygon": 0.10
  }
}
```

---

## 6. Forum/Social Schema

**Source**: `codeberg/forum_activity.sql`

### Tables

#### topics
- **Row count**: 100K - 10M
- **Distribution**: Power law (few topics very popular)
- **Hot data**: Active topics (last 7 days)

#### posts
- **Row count**: 1M - 100M
- **Distribution**: Sharded by topic_id
- **Relationship**: Many posts per topic (avg 10-20)

#### likes
- **Row count**: 10M - 1B
- **Distribution**: Power law on content
- **Query patterns**: Aggregation (count likes)

### Facts for Ra

```json
{
  "tables": {
    "topics": {
      "row_count": 5000000,
      "columns": {
        "last_post_at": {
          "cardinality": 1000000,
          "recency_distribution": {
            "last_24h": 0.10,
            "last_7d": 0.30,
            "last_30d": 0.50,
            "older": 0.10
          }
        }
      }
    },
    "posts": {
      "row_count": 50000000,
      "foreign_keys": [
        {"from": "topic_id", "to": "topics.id", "avg_refs": 10}
      ],
      "distribution": {
        "type": "co-located",
        "parent": "topics",
        "key": "topic_id"
      }
    }
  }
}
```

---

## Statistics Summary

### Critical Statistics for Distributed Optimization

1. **Row Counts** (per table, per shard, per partition)
2. **Column Cardinality** (distinct values)
3. **Column Distributions** (histograms, MCVs)
4. **Null Fractions**
5. **Foreign Key Cardinality** (avg refs per parent)
6. **Index Availability** (types, keys, selectivity)
7. **Shard/Partition Key** (for routing)
8. **Co-location Information** (which tables on same shards)
9. **Replication Status** (which tables replicated)
10. **Time Characteristics** (hot/warm/cold boundaries)
11. **Query Patterns** (OLTP vs OLAP ratios)
12. **Data Skew** (hotspots, power law parameters)

### How to Collect

See: `docs/guides/modeling-production-workloads.md`
