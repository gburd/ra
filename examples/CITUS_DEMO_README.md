# Ra CLI Distributed Query Demo with CitusDB

This demo showcases Ra's distributed query optimization capabilities using a CitusDB cluster with realistic distributed data patterns.

## Overview

The demo demonstrates how Ra optimizes complex distributed queries across multiple database nodes, showing:
- **Co-located joins** (optimal distributed pattern)
- **Reference table joins** (broadcast optimization)
- **Cross-shard joins** (requiring data repartitioning)
- **Distributed aggregations** (multi-level processing)
- **Window functions** in distributed environments
- **Subquery decorrelation** across distributed tables
- **Complex CTEs** with distributed execution
- **Resource-budgeted optimization** for production workloads

## Architecture

```
┌─────────────────┐    ┌─────────────────┐
│  Ra CLI Client  │────│ Citus Coordinator│
│                 │    │   (port 5432)   │
└─────────────────┘    └─────────┬───────┘
                                 │
                   ┌─────────────┼─────────────┐
                   │             │             │
           ┌───────▼────┐ ┌──────▼────┐ ┌─────▼────┐
           │Worker Node 1│ │Worker Node│ │Worker   │
           │ (port 5433) │ │2(port 5434│ │Node 3   │
           └─────────────┘ └───────────┘ └──────────┘
```

## Quick Start

### 1. Prerequisites
```bash
# Build ra-cli
cargo build --bin ra-cli

# Ensure Docker is running
docker --version
```

### 2. Start Citus Cluster
```bash
# Start coordinator
docker run -d --name citus-coord \
  -e POSTGRES_USER=citus_demo -e POSTGRES_PASSWORD=citus_demo \
  -e POSTGRES_DB=citus_demo -p 5432:5432 \
  citusdata/citus:12.1

# Start worker nodes
docker run -d --name citus-worker1 \
  -e POSTGRES_USER=citus_demo -e POSTGRES_PASSWORD=citus_demo \
  -e POSTGRES_DB=citus_demo -p 5433:5432 \
  citusdata/citus:12.1

docker run -d --name citus-worker2 \
  -e POSTGRES_USER=citus_demo -e POSTGRES_PASSWORD=citus_demo \
  -e POSTGRES_DB=citus_demo -p 5434:5432 \
  citusdata/citus:12.1

docker run -d --name citus-worker3 \
  -e POSTGRES_USER=citus_demo -e POSTGRES_PASSWORD=citus_demo \
  -e POSTGRES_DB=citus_demo -p 5435:5432 \
  citusdata/citus:12.1
```

### 3. Configure Cluster
```bash
# Setup cluster topology
./examples/setup-citus-cluster.sh
```

### 4. Load Distributed Schema
```bash
# Create distributed tables and load sample data
docker exec -i citus-coord psql -U citus_demo -d citus_demo \
  < examples/citus-distributed-schema.sql
```

### 5. Run Demo
```bash
# Execute the full distributed query optimization demo
./examples/ra-cli-citus-demo.sh
```

## Demo Examples

### Example 1: Co-located Join (Optimal)
```sql
-- Users and events are both distributed by user_id
-- Join executes locally on each worker node
SELECT u.email, e.event_type, e.event_time
FROM events e
JOIN users u ON e.user_id = u.user_id
WHERE u.subscription_tier = 'premium'
```

### Example 2: Reference Table Join
```sql
-- Products table is replicated on all nodes
-- No data movement required
SELECT p.name, COUNT(*) as purchases
FROM events e
JOIN products p ON (e.properties->>'product_id')::int = p.product_id
WHERE e.event_type = 'purchase'
GROUP BY p.product_id, p.name
```

### Example 3: Cross-shard Join (Expensive)
```sql
-- Events distributed by user_id, sessions by session_id
-- Requires data repartitioning across workers
SELECT s.session_id, COUNT(e.event_id) as event_count
FROM user_sessions s
JOIN events e ON s.session_id = e.session_id
GROUP BY s.session_id
```

## Data Distribution

| Table | Distribution Key | Type | Shards |
|-------|-----------------|------|---------|
| `users` | `user_id` | Distributed | ~32 per worker |
| `events` | `user_id` | Distributed (co-located) | ~32 per worker |
| `products` | N/A | Reference | Replicated |
| `user_sessions` | `session_id` | Distributed | ~32 per worker |

## Sample Data

- **5,000 users** across 10 countries with different subscription tiers
- **~50,000+ events** with realistic activity patterns
- **15 products** across multiple categories
- **~9,000 user sessions** with device/browser diversity

## Expected Output

The demo shows:
1. **Query parsing** into relational algebra
2. **Distributed optimization** with rule application
3. **Plan comparisons** (before/after optimization)
4. **Cost estimates** for distributed execution
5. **Data movement** analysis and minimization
6. **Parallel execution** strategies across workers

## Performance Insights

- **Co-located joins**: ~10-100x faster than cross-shard joins
- **Reference tables**: Enable local lookups without network overhead
- **Proper sharding**: Critical for query performance
- **Aggregation pushdown**: Reduces data transfer between nodes

## Cleanup

```bash
# Stop and remove containers
docker stop citus-coord citus-worker1 citus-worker2 citus-worker3
docker rm citus-coord citus-worker1 citus-worker2 citus-worker3

# Remove demo schema file if desired
rm -f examples/citus-demo-schema.json
```

## Troubleshooting

### Connection Issues
```bash
# Verify containers are running
docker ps

# Check coordinator logs
docker logs citus-coord

# Test direct connection
PGPASSWORD=citus_demo psql -h localhost -p 5432 -U citus_demo -d citus_demo -c "SELECT * FROM citus_get_active_worker_nodes();"
```

### Performance Issues
```bash
# Check cluster status
PGPASSWORD=citus_demo psql -h localhost -p 5432 -U citus_demo -d citus_demo -c "SELECT * FROM citus_stat_activity();"

# View shard distribution
PGPASSWORD=citus_demo psql -h localhost -p 5432 -U citus_demo -d citus_demo -c "SELECT * FROM citus_shards ORDER BY table_name, shard_id;"
```

## Advanced Usage

### Custom Queries
Test your own distributed queries:
```bash
cargo run --bin ra-cli -- optimize \
  "YOUR_DISTRIBUTED_QUERY_HERE" \
  --db "postgresql://citus_demo:citus_demo@localhost:5432/citus_demo" \
  --diff colored
```

### Resource Budgets
```bash
cargo run --bin ra-cli -- optimize \
  "COMPLEX_QUERY" \
  --resource-budget large \
  --rules-all
```

This demo provides a comprehensive exploration of distributed query optimization patterns and showcases Ra's advanced capabilities in multi-node database environments.