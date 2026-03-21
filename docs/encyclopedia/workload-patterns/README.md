# Workload Patterns

Query workload characteristics and Ra's adaptive optimization strategies.

## Patterns

### [OLTP](oltp.md)
High-concurrency transactional workload with point lookups and small updates.

### [OLAP](olap.md)
Analytical queries with large scans, aggregations, and complex joins.

### [HTAP](htap.md)
Hybrid workload mixing OLTP and OLAP on same dataset.

### [Read-Heavy](read-heavy.md)
95%+ read queries with occasional writes.

### [Write-Heavy](write-heavy.md)
High insert/update volume requiring optimized write paths.

### [Batch Processing](batch-processing.md)
Scheduled ETL jobs and data pipeline queries.

### [Real-Time](real-time.md)
Streaming data with sub-second latency requirements.

### [Ad-Hoc](ad-hoc.md)
Unpredictable exploratory queries requiring robust optimization.

## Workload Comparison

| Workload | Query Duration | Concurrency | Index Usage | Join Strategy |
|----------|---------------|-------------|-------------|--------------|
| OLTP | < 10ms | Very high | Heavy | Nested loop |
| OLAP | seconds-minutes | Low | Selective | Hash/merge |
| HTAP | Mixed | High | Adaptive | Mixed |
| Read-heavy | < 100ms | High | Heavy | Cached |
| Write-heavy | < 10ms | High | Minimal | Rare |
| Batch | minutes-hours | Low | Minimal | Parallel |
| Real-time | < 1s | Medium | Covering | Streaming |
| Ad-hoc | Variable | Low | Adaptive | Cost-based |
