# Interactive Demonstrations

This document describes the 10+ interactive demonstrations available through the RA Web API.

## Overview

The demonstrations showcase how statistics staleness, hardware profiles, and optimizer decisions interact in query planning. Each demo is accessible via a REST endpoint and returns interactive results that can be visualized in a web UI.

## Demonstrations

### 1. Statistics Staleness Impact
**Endpoint**: `POST /api/demos/staleness-impact`

Visualizes how stale statistics affect query plan quality and cardinality estimation.

**Request**:
```json
{
  "initial_rows": 1000000,
  "modifications": 200000,
  "source": "exact"
}
```

**Sources**: `exact`, `sampled_10`, `sampled_50`, `histogram`, `ml_model`, `default`

**Response**:
- Staleness level (Fresh, SlightlyStale, ModeratelyStale, VeryStale)
- Confidence score
- Plan quality impact
- Cardinality error percentage
- Recommendation

### 2. Hardware-Specific Plans
**Endpoint**: `POST /api/demos/hardware-plan`

Shows how GPU/FPGA availability changes operator placement decisions.

**Request**:
```json
{
  "workload": "scan",
  "data_size_bytes": 100000000,
  "hardware_profile": "gpu_server"
}
```

**Workloads**: `scan`, `join`, `aggregation`, `filter`
**Profiles**: `gpu_server`, `fpga_appliance`, `cpu_only`

**Response**:
- Selected device
- Speedup vs CPU
- Estimated execution time
- Memory required
- Operator placement with reasoning

### 3. Join Algorithm Selection
**Endpoint**: `POST /api/demos/join-algorithm`

Compares hash join vs nested loop vs sort-merge based on data size and memory.

**Request**:
```json
{
  "left_size": 1000000,
  "right_size": 100000,
  "selectivity": 0.01,
  "memory_bytes": 1073741824
}
```

**Response**:
- Selected algorithm
- Estimated cost
- Output rows
- Memory usage
- Reasoning and alternatives

### 4. Aggregation Strategy Selection
**Endpoint**: `POST /api/demos/aggregation-strategy`

Chooses between hash, streaming, and sort-based aggregation.

**Request**:
```json
{
  "input_rows": 10000000,
  "num_groups": 100000,
  "memory_bytes": 1073741824,
  "workers": 8
}
```

**Response**:
- Selected strategy
- Estimated time
- Memory usage
- Parallelism used
- Reasoning

### 5. Index Selection
**Endpoint**: `POST /api/demos/index-selection`

Compares index scan vs full table scan based on selectivity and clustering.

**Request**:
```json
{
  "selectivity": 0.05,
  "table_rows": 10000000,
  "available_indexes": ["idx_col1"],
  "clustering_factor": 2.5
}
```

**Response**:
- Access method (Index Scan or Full Table Scan)
- Index used (if any)
- Estimated cost
- Rows accessed
- Reasoning

### 6. Subquery Unnesting
**Endpoint**: `POST /api/demos/subquery-unnesting`

Transforms correlated subqueries into joins for better performance.

**Request**:
```json
{
  "subquery_type": "exists",
  "outer_rows": 100000,
  "inner_rows": 50000,
  "multi_row": false
}
```

**Subquery Types**: `exists`, `in`, `scalar`, `not_exists`

**Response**:
- Original plan
- Unnested plan
- Can be unnested
- Estimated speedup
- Explanation

### 7. Parallel Query Execution
**Endpoint**: `POST /api/demos/parallel-query`

Determines optimal parallelism level based on data size and complexity.

**Request**:
```json
{
  "data_size_bytes": 1000000000,
  "available_cores": 16,
  "complexity": 5
}
```

**Response**:
- Recommended parallel workers
- Estimated speedup
- Coordination overhead percentage
- Explanation

### 8. GPU Offloading Decision
**Endpoint**: `POST /api/demos/gpu-offloading`

Evaluates GPU vs CPU execution considering transfer overhead.

**Request**:
```json
{
  "operator": "join",
  "data_size_bytes": 100000000,
  "gpu_memory_bytes": 85899345920,
  "pcie_bandwidth_gbps": 25.0
}
```

**Operators**: `scan`, `join`, `aggregation`, `sort`

**Response**:
- Use GPU decision
- Transfer time
- GPU execution time
- CPU execution time
- Recommendation

### 9. Distributed Query Planning
**Endpoint**: `POST /api/demos/distributed-query`

Chooses between co-located, broadcast, and shuffle joins in distributed systems.

**Request**:
```json
{
  "num_nodes": 10,
  "distribution": "hash",
  "join_type": "hash_join",
  "network_bandwidth_gbps": 10.0
}
```

**Distributions**: `hash`, `random`, `range`
**Join Types**: `hash_join`, `broadcast_join`

**Response**:
- Chosen strategy
- Data movement in bytes
- Estimated time
- Explanation

### 10. Cost Model Calibration
**Endpoint**: `POST /api/demos/cost-calibration`

Adjusts statistics profiles based on workload patterns and accuracy.

**Request**:
```json
{
  "stats_profile": "standard",
  "workload": "oltp",
  "historical_accuracy": 0.7
}
```

**Profiles**: `realtime`, `standard`, `lazy`, `stale`, `analytical`, `streaming`
**Workloads**: `oltp`, `olap`, `mixed`

**Response**:
- Recommended profile
- Confidence in estimates
- Calibration suggestions

## List All Demos
**Endpoint**: `GET /api/demos`

Returns metadata for all available demonstrations.

**Response**:
```json
{
  "count": 10,
  "demos": [
    {
      "id": "staleness-impact",
      "title": "Statistics Staleness Impact",
      "description": "...",
      "endpoint": "/api/demos/staleness-impact",
      "category": "Statistics"
    },
    ...
  ]
}
```

## Example Usage

### Using curl

```bash
# Statistics Staleness Impact
curl -X POST http://localhost:8000/api/demos/staleness-impact \
  -H "Content-Type: application/json" \
  -d '{
    "initial_rows": 1000000,
    "modifications": 200000,
    "source": "exact"
  }'

# Hardware-Specific Plans
curl -X POST http://localhost:8000/api/demos/hardware-plan \
  -H "Content-Type: application/json" \
  -d '{
    "workload": "join",
    "data_size_bytes": 100000000,
    "hardware_profile": "gpu_server"
  }'

# List all demos
curl http://localhost:8000/api/demos
```

### Using JavaScript (fetch)

```javascript
// Statistics Staleness Impact
const response = await fetch('http://localhost:8000/api/demos/staleness-impact', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    initial_rows: 1000000,
    modifications: 200000,
    source: 'exact'
  })
});
const result = await response.json();
console.log(result);
```

## Categories

Demos are organized into the following categories:

- **Statistics**: Staleness impact, cost calibration
- **Hardware**: Hardware-specific plans, GPU offloading
- **Algorithms**: Join algorithm, aggregation strategy
- **Access Methods**: Index selection
- **Optimization**: Subquery unnesting
- **Execution**: Parallel query execution
- **Distributed**: Distributed query planning

## Implementation Notes

- All endpoints accept JSON requests and return JSON responses
- Endpoints are subject to rate limiting (100 requests per 60 seconds by default)
- CORS is enabled for all origins
- No authentication required (demonstration purposes)

## Future Enhancements

Potential additions:
- Save/load demo configurations
- Export configurations as JSON
- Side-by-side comparison mode
- Video tutorial integration
- Interactive sliders in web UI
- WASM database integration for live queries
