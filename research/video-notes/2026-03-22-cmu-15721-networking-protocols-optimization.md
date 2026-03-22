# CMU 15-721 Lecture 12: Networking Protocols and Data Transfer Optimization

**Source:** CMU 15-721 Spring 2024, Lecture 12
**Date:** 2024-03-13
**Topic:** Network protocol optimization for database query results
**Key Papers:** "Don't Hold My Data Hostage" (VLDB 2017), ConnectorX (2022), Tigger (2024)

## Key Points

This lecture covers an often-overlooked aspect of query optimization: the cost of
transferring query results from the database server to the client. For analytical
queries returning large result sets, serialization and network transfer can dominate
total query time.

### The Data Transfer Problem

1. **Serialization overhead**: Converting internal columnar representation to wire format
2. **Row-based protocols**: PostgreSQL wire protocol sends data row-by-row (text or binary)
3. **Type conversion**: Server converts internal types to protocol types
4. **Client deserialization**: Client must parse wire format back to usable types
5. **Memory copies**: Multiple copies between server buffers, OS buffers, network

**Key finding:** For TPC-H Q1 on a warm cache, data serialization and transfer can
take 90%+ of total query time when the data is already in memory.

### Protocol Optimization Techniques

**1. Columnar Result Transfer:**
- Send results in columnar format (Arrow IPC) instead of row format
- Avoids per-row overhead of traditional protocols
- Client receives data in analysis-ready format (no conversion needed)
- Used by: DuckDB (Arrow export), DataFusion, Flight SQL

**2. Zero-Copy Transfer:**
- Use shared memory (for local clients) or RDMA (for remote)
- Avoid kernel buffer copies with io_uring or kernel bypass networking
- Relevant when client and server are on same machine or same rack

**3. Result Compression:**
- Compress columnar result batches before network transfer
- LZ4 for speed, ZSTD for ratio
- Optimizer should decide based on network bandwidth vs CPU availability

**4. Partial Result Streaming:**
- Send results in batches as they're produced (don't buffer entire result)
- Allows client to begin processing before query completes
- Important for large result sets and interactive analysis

**5. Arrow Flight SQL:**
- Standard protocol for high-performance data transfer
- Built on gRPC with columnar Arrow batches
- Supports direct distributed reads (client reads from multiple endpoints)

### ConnectorX: Optimized Client-Side Loading

Approach: partition query, parallel fetch, write directly to client memory:
1. Partition source query into ranges (WHERE id BETWEEN ... AND ...)
2. Fetch partitions in parallel using multiple connections
3. Write results directly into destination format (Pandas DataFrame, Arrow, etc.)
4. Skip intermediate conversion steps

**Optimization rule:** query-partitioning-for-parallel-fetch - split large result
queries into range partitions for parallel client-side loading.

### Optimizer Implications

**1. Result Size Estimation:**
- Optimizer should estimate not just row count but total result byte size
- Byte size determines whether result fits in network buffer
- Large results benefit from streaming; small results from single-batch

**2. Projection as Transfer Optimization:**
- Column pruning reduces data transfer (less bytes over network)
- Optimizer should account for transfer cost in projection decisions
- For wide tables, removing unused columns has outsized benefit

**3. Predicate Pushdown as Transfer Optimization:**
- Filtering at the source reduces rows transferred
- Especially important for remote/federated queries
- Transfer cost should be in the cost model for federated planning

**4. Aggregate Pushdown as Transfer Optimization:**
- Aggregation at source sends summary instead of raw data
- COUNT/SUM/AVG return single row regardless of source size
- Most impactful optimization for remote data sources

## Optimization Rules for Ra

### New Rules Identified

1. **result-transfer-cost-model** - Include network transfer cost (based on estimated
   result byte size) in the overall query cost model
2. **projection-for-transfer-reduction** - Prioritize column pruning when query involves
   remote data sources or large result sets
3. **aggregate-pushdown-for-transfer** - In federated queries, always push aggregation
   to the remote source to minimize transfer
4. **result-format-selection** - Choose result wire format (text, binary, Arrow) based
   on result size and client capabilities
5. **query-partitioning-for-parallel-fetch** - Split large result queries into range
   partitions for parallel client-side consumption
6. **federated-transfer-aware-join-ordering** - In federated queries, order joins to
   minimize cross-network data transfer

### Ra Gap Analysis

Ra currently has:
- `rules/cost-models/network-cost-model.rra` - Network cost model
- `rules/distributed/` - Distributed query rules
- `rules/federated/` - Federated query rules
- No result transfer optimization
- No format-aware serialization costing

**Missing capabilities:**
- Result byte size estimation (not just row count)
- Transfer cost in the cost model for local queries
- Arrow IPC / Flight SQL format awareness
- Federated query transfer minimization
- Parallel result partitioning

## Relevance to Ra

**Priority:** Medium - Most relevant for Ra's federated query support and PostgreSQL
extension where results flow over the wire. Less relevant for embedded use cases.
However, federated-transfer-aware join ordering is critical for any distributed
query capability.

**Key insight:** For Ra's PostgreSQL extension, the cost of sending results back
through the PostgreSQL wire protocol should be factored into plan selection.
Plans that produce smaller intermediate results at network boundaries are preferred.
