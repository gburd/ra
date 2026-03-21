# Modeling Production Workloads in Ra

Guide for extracting statistics from production databases and modeling them as Ra facts.

## Overview

Ra's query optimizer relies on accurate statistics to make good decisions about:
- Join ordering
- Index selection
- Partition pruning
- Shard routing
- Aggregation pushdown
- Join strategy (broadcast vs shuffle)

This guide explains how to collect these statistics from production databases and provide them to Ra.

## 1. Table Statistics

### Basic Metadata

Collect from your database:

```sql
-- PostgreSQL
SELECT
    schemaname,
    tablename,
    n_live_tup AS row_count,
    pg_total_relation_size(schemaname || '.' || tablename) AS size_bytes
FROM pg_stat_user_tables
ORDER BY n_live_tup DESC;
```

**Provide to Ra**:
```json
{
  "tables": {
    "orders": {
      "row_count": 50000000,
      "size_bytes": 25000000000
    }
  }
}
```

### Column Statistics

```sql
-- PostgreSQL
SELECT
    schemaname,
    tablename,
    attname AS column_name,
    n_distinct,
    null_frac,
    avg_width
FROM pg_stats
WHERE schemaname = 'public'
    AND tablename = 'orders';
```

**Provide to Ra**:
```json
{
  "tables": {
    "orders": {
      "columns": {
        "id": {
          "cardinality": 50000000,
          "null_frac": 0.0,
          "avg_width": 8
        },
        "user_id": {
          "cardinality": 2000000,
          "null_frac": 0.0,
          "avg_width": 4
        },
        "status": {
          "cardinality": 5,
          "null_frac": 0.0,
          "avg_width": 10,
          "most_common_values": [
            {"value": "completed", "frequency": 0.70},
            {"value": "pending", "frequency": 0.20},
            {"value": "processing", "frequency": 0.05},
            {"value": "shipped", "frequency": 0.03},
            {"value": "canceled", "frequency": 0.02}
          ]
        }
      }
    }
  }
}
```

### Histograms

For columns with non-uniform distributions:

```sql
-- PostgreSQL extended stats
SELECT
    schemaname,
    tablename,
    attname,
    histogram_bounds
FROM pg_stats
WHERE schemaname = 'public'
    AND tablename = 'orders'
    AND attname = 'created_at';
```

**Provide to Ra**:
```json
{
  "tables": {
    "orders": {
      "columns": {
        "created_at": {
          "histogram": [
            {"bucket": "2023-01-01", "frequency": 0.05},
            {"bucket": "2023-04-01", "frequency": 0.10},
            {"bucket": "2023-07-01", "frequency": 0.15},
            {"bucket": "2023-10-01", "frequency": 0.20},
            {"bucket": "2024-01-01", "frequency": 0.50}
          ]
        }
      }
    }
  }
}
```

## 2. Index Statistics

### Available Indexes

```sql
-- PostgreSQL
SELECT
    schemaname,
    tablename,
    indexname,
    indexdef
FROM pg_indexes
WHERE schemaname = 'public'
ORDER BY tablename, indexname;
```

**Provide to Ra**:
```json
{
  "tables": {
    "orders": {
      "indexes": {
        "orders_pkey": {
          "keys": ["id"],
          "type": "btree",
          "unique": true,
          "size_bytes": 1000000000
        },
        "idx_orders_user_id": {
          "keys": ["user_id"],
          "type": "btree",
          "unique": false,
          "size_bytes": 500000000
        },
        "idx_orders_created_at": {
          "keys": ["created_at"],
          "type": "btree",
          "unique": false,
          "size_bytes": 500000000
        },
        "idx_orders_user_created": {
          "keys": ["user_id", "created_at"],
          "type": "btree",
          "unique": false,
          "covering": ["status", "total_amount"]
        }
      }
    }
  }
}
```

### Index Selectivity

Estimate how many rows an index lookup returns:

```sql
-- For equality predicates
SELECT
    COUNT(*) AS total_rows,
    COUNT(DISTINCT user_id) AS distinct_users,
    COUNT(*) / COUNT(DISTINCT user_id) AS avg_rows_per_user
FROM orders;

-- For range predicates
SELECT
    COUNT(*) FILTER (WHERE created_at >= CURRENT_DATE - INTERVAL '7 days')
        AS last_7_days,
    COUNT(*) FILTER (WHERE created_at >= CURRENT_DATE - INTERVAL '30 days')
        AS last_30_days,
    COUNT(*) AS total
FROM orders;
```

**Provide to Ra**:
```json
{
  "tables": {
    "orders": {
      "predicates": {
        "user_id = ?": {
          "avg_selectivity": 0.000025,
          "avg_rows": 25
        },
        "created_at >= NOW() - INTERVAL '7 days'": {
          "avg_selectivity": 0.05,
          "avg_rows": 2500000
        },
        "status = 'completed'": {
          "avg_selectivity": 0.70,
          "avg_rows": 35000000
        }
      }
    }
  }
}
```

## 3. Join Statistics

### Foreign Key Relationships

```sql
-- PostgreSQL
SELECT
    tc.table_schema,
    tc.table_name,
    kcu.column_name,
    ccu.table_name AS foreign_table_name,
    ccu.column_name AS foreign_column_name
FROM information_schema.table_constraints AS tc
JOIN information_schema.key_column_usage AS kcu
    ON tc.constraint_name = kcu.constraint_name
JOIN information_schema.constraint_column_usage AS ccu
    ON ccu.constraint_name = tc.constraint_name
WHERE tc.constraint_type = 'FOREIGN KEY';
```

**Average cardinality per FK**:
```sql
-- How many order_items per order?
SELECT
    AVG(item_count) AS avg_items_per_order,
    PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY item_count) AS median,
    PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY item_count) AS p95,
    MAX(item_count) AS max_items
FROM (
    SELECT order_id, COUNT(*) AS item_count
    FROM order_items
    GROUP BY order_id
) counts;
```

**Provide to Ra**:
```json
{
  "tables": {
    "order_items": {
      "foreign_keys": [
        {
          "from": "order_id",
          "to": "orders.id",
          "avg_refs": 2.5,
          "median_refs": 2,
          "p95_refs": 5,
          "max_refs": 50
        }
      ]
    }
  }
}
```

## 4. Distributed System Statistics

### Sharding/Partitioning Information

**Shard key identification**:
```json
{
  "tables": {
    "orders": {
      "distribution": {
        "type": "hash",
        "key": "user_id",
        "shard_count": 16
      }
    }
  }
}
```

**Shard size distribution**:
```sql
-- If using hash-based sharding (e.g., Citus)
SELECT
    get_shard_id_for_distribution_column('orders', user_id) AS shard_id,
    COUNT(*) AS row_count,
    pg_size_pretty(pg_total_relation_size(shard)) AS size
FROM orders
GROUP BY shard_id
ORDER BY row_count DESC;
```

**Provide to Ra**:
```json
{
  "tables": {
    "orders": {
      "shards": [
        {"id": 0, "rows": 3200000, "size_bytes": 1600000000},
        {"id": 1, "rows": 3150000, "size_bytes": 1575000000},
        ...
        {"id": 15, "rows": 3100000, "size_bytes": 1550000000}
      ],
      "shard_skew": {
        "max_to_avg_ratio": 1.15,
        "hot_shards": [0, 5, 12]
      }
    }
  }
}
```

### Co-location Information

For distributed databases, specify which tables are co-located:

```json
{
  "co_location": [
    {
      "tables": ["orders", "order_items"],
      "key": "user_id",
      "reason": "FK: order_items.order_id → orders.id implies same user_id"
    }
  ]
}
```

### Replication Information

```json
{
  "tables": {
    "products": {
      "distribution": {
        "type": "replicated",
        "reason": "Small dimension table (50K rows, 50MB)"
      }
    },
    "categories": {
      "distribution": {
        "type": "replicated",
        "reason": "Reference data (100 rows, 10KB)"
      }
    }
  }
}
```

## 5. Time-Series Statistics

### Partition Boundaries

```sql
-- TimescaleDB
SELECT
    hypertable_name,
    chunk_name,
    range_start,
    range_end,
    chunk_table_size,
    row_count
FROM chunks_detailed_size('sensor_readings')
ORDER BY range_start DESC;
```

**Provide to Ra**:
```json
{
  "tables": {
    "sensor_readings": {
      "distribution": {
        "type": "range",
        "key": "time",
        "partitions": [
          {
            "range_start": "2024-03-21 00:00:00",
            "range_end": "2024-03-22 00:00:00",
            "rows": 864000000,
            "size_bytes": 50000000000,
            "compression_ratio": 8.5
          },
          {
            "range_start": "2024-03-20 00:00:00",
            "range_end": "2024-03-21 00:00:00",
            "rows": 862000000,
            "size_bytes": 48000000000,
            "compression_ratio": 8.7
          }
        ]
      },
      "time_characteristics": {
        "partition_interval": "1 day",
        "hot_data_cutoff": "1 hour",
        "warm_data_cutoff": "7 days",
        "cold_data_cutoff": "90 days"
      }
    }
  }
}
```

### Query Time Distribution

```sql
-- Analyze query logs to find time range patterns
SELECT
    CASE
        WHEN time_range < INTERVAL '1 hour' THEN 'last_1_hour'
        WHEN time_range < INTERVAL '24 hours' THEN 'last_24_hours'
        WHEN time_range < INTERVAL '7 days' THEN 'last_7_days'
        WHEN time_range < INTERVAL '30 days' THEN 'last_30_days'
        ELSE 'older'
    END AS range_bucket,
    COUNT(*) AS query_count,
    AVG(duration_ms) AS avg_duration
FROM query_log
WHERE query_text LIKE '%sensor_readings%'
    AND query_text LIKE '%WHERE time%'
GROUP BY range_bucket
ORDER BY query_count DESC;
```

**Provide to Ra**:
```json
{
  "tables": {
    "sensor_readings": {
      "query_time_distribution": {
        "last_1_hour": 0.80,
        "last_24_hours": 0.15,
        "last_7_days": 0.04,
        "last_30_days": 0.01
      }
    }
  }
}
```

## 6. Workload Characteristics

### OLTP vs OLAP Mix

```sql
-- Analyze query logs
SELECT
    CASE
        WHEN query_text LIKE '%LIMIT%' AND query_text LIKE '%WHERE%id = %'
            THEN 'OLTP'
        WHEN query_text LIKE '%GROUP BY%' OR query_text LIKE '%aggregate%'
            THEN 'OLAP'
        ELSE 'Other'
    END AS workload_type,
    COUNT(*) AS query_count,
    AVG(duration_ms) AS avg_duration,
    PERCENTILE_CONT(0.99) WITHIN GROUP (ORDER BY duration_ms) AS p99_duration
FROM query_log
WHERE timestamp >= NOW() - INTERVAL '24 hours'
GROUP BY workload_type;
```

**Provide to Ra**:
```json
{
  "workload": {
    "oltp_percentage": 0.80,
    "olap_percentage": 0.20,
    "oltp_latency_target_ms": 50,
    "olap_latency_target_ms": 5000,
    "peak_qps": 1000,
    "concurrent_queries": 100
  }
}
```

### Table Access Patterns

```sql
-- PostgreSQL: Table scan vs index scan ratio
SELECT
    schemaname,
    tablename,
    seq_scan AS table_scans,
    idx_scan AS index_scans,
    CASE
        WHEN seq_scan + idx_scan > 0 THEN
            idx_scan::DECIMAL / (seq_scan + idx_scan)
        ELSE 0
    END AS index_scan_ratio
FROM pg_stat_user_tables
WHERE schemaname = 'public'
ORDER BY seq_scan DESC;
```

**Provide to Ra**:
```json
{
  "tables": {
    "orders": {
      "access_patterns": {
        "seq_scan_percentage": 0.05,
        "index_scan_percentage": 0.95,
        "avg_scan_percentage": 0.001,
        "avg_result_percentage": 0.00001
      }
    }
  }
}
```

## 7. Data Skew and Hotspots

### Identify Hot Partitions

```sql
-- Tenant activity distribution
SELECT
    tenant_id,
    COUNT(*) AS query_count,
    SUM(data_size_mb) AS total_data_mb
FROM (
    SELECT
        tenant_id,
        COUNT(*) AS row_count,
        pg_total_relation_size(tablename) / 1024 / 1024 AS data_size_mb
    FROM projects
    GROUP BY tenant_id
) tenant_stats
GROUP BY tenant_id
ORDER BY query_count DESC
LIMIT 20;
```

**Provide to Ra**:
```json
{
  "tables": {
    "projects": {
      "tenant_distribution": {
        "avg_rows_per_tenant": 500,
        "median_rows_per_tenant": 100,
        "p95_rows_per_tenant": 5000,
        "p99_rows_per_tenant": 25000,
        "max_rows_per_tenant": 100000,
        "hot_tenants": [123, 456, 789],
        "skew_factor": 10.0
      }
    }
  }
}
```

## 8. Complete Example

Here's a complete statistics file for an e-commerce database:

```json
{
  "database": "ecommerce_production",
  "collected_at": "2024-03-21T00:00:00Z",
  "tables": {
    "orders": {
      "row_count": 50000000,
      "size_bytes": 25000000000,
      "columns": {
        "id": {
          "cardinality": 50000000,
          "null_frac": 0.0,
          "avg_width": 8
        },
        "user_id": {
          "cardinality": 2000000,
          "null_frac": 0.0,
          "avg_width": 4
        },
        "status": {
          "cardinality": 5,
          "null_frac": 0.0,
          "most_common_values": [
            {"value": "completed", "frequency": 0.70},
            {"value": "pending", "frequency": 0.20}
          ]
        },
        "created_at": {
          "cardinality": 3650,
          "null_frac": 0.0,
          "histogram": [
            {"bucket": "2023-01-01", "frequency": 0.10},
            {"bucket": "2024-01-01", "frequency": 0.40},
            {"bucket": "2024-03-21", "frequency": 0.50}
          ]
        }
      },
      "indexes": {
        "orders_pkey": {
          "keys": ["id"],
          "type": "btree",
          "unique": true
        },
        "idx_orders_user_id": {
          "keys": ["user_id"],
          "type": "btree"
        }
      },
      "distribution": {
        "type": "hash",
        "key": "user_id",
        "shard_count": 16
      },
      "predicates": {
        "user_id = ?": {
          "avg_selectivity": 0.000025,
          "avg_rows": 25
        }
      },
      "access_patterns": {
        "seq_scan_percentage": 0.05,
        "index_scan_percentage": 0.95
      }
    },
    "order_items": {
      "row_count": 125000000,
      "size_bytes": 25000000000,
      "foreign_keys": [
        {
          "from": "order_id",
          "to": "orders.id",
          "avg_refs": 2.5
        }
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
      "distribution": {
        "type": "replicated",
        "reason": "Small dimension table"
      }
    }
  },
  "workload": {
    "oltp_percentage": 0.80,
    "olap_percentage": 0.20,
    "peak_qps": 1000
  }
}
```

## 9. Using Statistics in Ra

Once collected, provide statistics to Ra via:

1. **Configuration file** (JSON/TOML)
2. **Programmatic API**
3. **System catalog integration**

Example API usage:

```rust
use ra::optimizer::stats::StatisticsProvider;

let stats = StatisticsProvider::from_file("stats.json")?;
let optimizer = Optimizer::new(stats);

let query = "SELECT * FROM orders WHERE user_id = 123";
let plan = optimizer.optimize(query)?;
```

## 10. Automation

### Periodic Collection Script

```bash
#!/bin/bash
# collect_stats.sh

PGDATABASE="production"
OUTPUT_DIR="/var/lib/ra/stats"
DATE=$(date +%Y%m%d)

# Table statistics
psql $PGDATABASE -f collect_table_stats.sql > "$OUTPUT_DIR/table_stats_$DATE.json"

# Column statistics
psql $PGDATABASE -f collect_column_stats.sql > "$OUTPUT_DIR/column_stats_$DATE.json"

# Index statistics
psql $PGDATABASE -f collect_index_stats.sql > "$OUTPUT_DIR/index_stats_$DATE.json"

# Merge into single file
python3 merge_stats.py "$OUTPUT_DIR" > "$OUTPUT_DIR/stats_$DATE.json"

# Update Ra configuration
cp "$OUTPUT_DIR/stats_$DATE.json" /etc/ra/stats.json
systemctl reload ra-optimizer
```

### Schedule with Cron

```cron
# Run every 6 hours
0 */6 * * * /usr/local/bin/collect_stats.sh
```

## Summary

Key statistics for Ra optimization:

1. **Table**: Row counts, sizes
2. **Column**: Cardinality, null fractions, histograms
3. **Index**: Keys, types, selectivity
4. **Join**: FK relationships, cardinality
5. **Distribution**: Shard keys, partition boundaries
6. **Workload**: OLTP/OLAP mix, access patterns
7. **Skew**: Hot partitions, tenant distribution

Collect regularly (every 6-24 hours) and provide to Ra for optimal query planning.
