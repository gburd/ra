# Federated Query Optimization

Federated query optimization enables `ra` to execute queries spanning
multiple database backends. The optimizer evaluates network costs,
remote capabilities, and data volumes to select the cheapest execution
strategy for each query segment.

## Architecture

```
SQL query
    |
    v
ra-parser  (parse + convert to RelExpr)
    |
    v
ra-engine/FederatedOptimizer
    |--- enumerate strategies (ShipQuery, ShipData, Hybrid, Local)
    |--- cost each via FederatedCostModel
    |--- pick cheapest
    v
FederatedPlan (chosen strategy + cost breakdown + steps)
```

### Core types (`ra-core::federated`)

| Type | Purpose |
|------|---------|
| `FederatedQuery` | Query + data sources + optimization hints |
| `DataSource` | `Local` or `Remote(RemoteConnection)` |
| `RemoteConnection` | Endpoint, DB type, capabilities, network params |
| `DatabaseType` | PostgreSQL, MySQL, SQLite, Snowflake, BigQuery, etc. |
| `QueryCapabilities` | What the remote can execute (filter, join, agg, ...) |
| `ExecutionLocation` | ShipQuery / ShipData / Hybrid / Local |
| `FederatedCostBreakdown` | network_transfer_ms, remote_exec_ms, local_exec_ms, total_ms |
| `FederatedPlan` | Chosen location, cost, human-readable steps |

### Optimizer (`ra-engine::federated_optimizer`)

`FederatedOptimizer` accepts a `FederatedQuery` and returns the
cheapest `FederatedPlan`. It works in three phases:

1. **Enumerate** -- generate candidate strategies based on source
   types, remote capabilities, and data volumes.
2. **Cost** -- use `FederatedCostModel` to estimate wall-clock time
   for each candidate (network transfer + remote execution + local
   execution).
3. **Select** -- sort by `total_ms`, pick the cheapest.

### Cost model (`ra-engine::federated_cost`)

`FederatedCostModel` estimates time for each strategy:

- **ShipQuery**: send the full query to the remote; fetch only the
  result. Best when the remote supports the entire query and the
  result set is small relative to the input.
- **ShipData**: fetch raw data locally, execute everything here.
  Best when the query is too complex for the remote or the dataset
  is small enough that transfer is cheap.
- **Hybrid**: push filters/projections to the remote (reducing
  transfer volume), then finish locally. Best when the remote
  supports partial pushdown and the filter is selective.
- **Local**: no remote involved, standard local execution.

Key parameters: `latency_ms`, `bandwidth_mbps`, `estimated_rows`,
`avg_row_bytes`, `default_filter_selectivity`.

## Execution strategies

### ShipQuery

Send the full query to the remote database for execution and
retrieve only the result rows.

**When chosen:**
- Remote supports all operators in the query
- Expected result set is small (high selectivity or LIMIT)
- Network is slow relative to data volume

### ShipData

Fetch entire tables from the remote and execute the query locally.

**When chosen:**
- Remote lacks required capabilities (e.g., window functions)
- Tables are small enough that full transfer is acceptable
- Query involves complex local-only operations

### Hybrid

Push supported operations (filters, projections, partial
aggregates) to the remote, then complete execution locally.

**When chosen:**
- Remote supports some but not all operators
- Pushable filters are selective (reduce transfer significantly)
- Neither pure ShipQuery nor ShipData is viable

## Pushdown rules

24 optimization rules in `rules/federated/` define rewrite patterns:

| Category | Count | Examples |
|----------|-------|---------|
| Filter pushdown | 1 | Push WHERE to remote scans |
| Projection pushdown | 5 | Column pruning, computed columns, star elimination |
| Aggregate pushdown | 8 | COUNT, SUM, AVG decomposition, GROUP BY, HAVING |
| Join pushdown | 4 | Colocated, semi-join, bind-join, hash-partition |
| Prefer query shipping | 3 | Small result, high selectivity, LIMIT |
| Prefer data shipping | 3 | Complex query, small table, cross-site join |

Each rule is a `.rra` file with YAML frontmatter (metadata) and
markdown body (description, algebra, before/after examples, tests).

## CLI usage

```
ra-cli federated analyze \
  --query "SELECT * FROM orders WHERE total > 100" \
  --remote-db "postgresql://warehouse:5432/sales" \
  --remote-table orders \
  --latency 25 \
  --bandwidth 100 \
  --remote-rows 10000000 \
  --avg-row-size 256
```

Output:

```
Federated Query Analysis
========================
Strategy: ShipQuery
  Remote executes full query, returns results only

Steps:
  1. Push full query to remote
  2. Execute filter (total > 100) at remote
  3. Transfer result set (estimated 50000 rows, 12.2 MB)

Cost Breakdown:
  Network transfer : 1.0 ms
  Remote execution : 120.0 ms
  Local processing : 0.5 ms
  Total            : 121.5 ms

Alternatives considered:
  ShipData : 20580.0 ms
  Hybrid   : 145.0 ms

Estimated savings vs next best: 16.2%
```

### Parameters

| Flag | Default | Description |
|------|---------|-------------|
| `--query` | required | SQL query to analyze |
| `--remote-db` | required | Remote connection string |
| `--remote-table` | required | Table name at the remote |
| `--latency` | `10` | Network round-trip latency in ms |
| `--bandwidth` | `100` | Network bandwidth in Mbps |
| `--remote-rows` | `1000000` | Estimated row count at remote |
| `--avg-row-size` | `128` | Average row size in bytes |

## Supported databases

Each database type has default capabilities reflecting what it can
execute remotely:

| Database | Filter | Project | Join | Aggregate | Window | Subquery |
|----------|--------|---------|------|-----------|--------|----------|
| `PostgreSQL` | yes | yes | yes | yes | yes | yes |
| `MySQL` | yes | yes | yes | yes | yes | yes |
| `SQLite` | yes | yes | yes | yes | no | yes |
| `Snowflake` | yes | yes | yes | yes | yes | yes |
| `BigQuery` | yes | yes | yes | yes | yes | yes |
| `DuckDB` | yes | yes | yes | yes | yes | yes |
| `SparkSQL` | yes | yes | yes | yes | yes | yes |

## Testing

```
# Unit tests (ra-core federated types)
cargo test -p ra-core federated

# Unit tests (cost model + optimizer)
cargo test -p ra-engine federated

# Integration tests (50 tests covering all strategies and DB types)
cargo test -p ra-engine --test federated_integration_test

# Validate rule files
ra-cli validate rules/federated/
```
