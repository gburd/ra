# RFC 0012: Monitoring and Advisory System

- **Status:** Accepted
- **Type:** Prospective
- **Author:** RA Contributors
- **Date:** 2026-03-20
- **Tracking:** Phase 6 of deployment plan

---

## Summary

A monitoring system that observes PostgreSQL query performance in
real time, detects plan regressions, identifies optimization
opportunities, and proactively advises DBAs through alerts,
dashboards, and automated recommendations.

## Motivation

The RA optimizer produces better plans, but knowing *when* to
intervene requires continuous monitoring. DBAs need:

- Alerts when query performance degrades (plan regressions)
- Identification of queries that would benefit from RA optimization
- Tracking of optimization impact over time
- Automated recommendations for index creation, statistics refresh,
  and configuration changes

The monitoring system bridges the gap between RA's optimization
capabilities and production database operations.

## Guide-Level Explanation

### Setup

```bash
# Start the monitoring daemon
ra-monitor --database postgres://localhost/mydb

# Or configure via file
ra-monitor --config monitor.toml
```

### Configuration

```toml
[database]
url = "postgres://localhost/mydb"
poll_interval_seconds = 60

[alerts]
regression_threshold_pct = 20.0
slow_query_threshold_ms = 1000
notification = "slack"  # or "email", "webhook", "log"

[advisor]
auto_analyze = true
index_recommendations = true
```

### Dashboard

The monitoring system exposes a web dashboard showing:

- Top N slowest queries with optimization potential
- Plan regression timeline
- RA optimization impact (before/after comparison)
- Statistics freshness across tables
- Index usage and recommendation status

### Advisory Output

```
[ADVISORY] Query 0x7a3f... regressed 3.2x after table growth
  Table: orders (1.2M -> 4.8M rows)
  Current plan: Sequential Scan + Hash Join
  Recommended: Index Scan on orders_date_idx + Hash Join
  Action: ANALYZE orders; -- refresh statistics

[ADVISORY] Missing index detected
  Query: SELECT * FROM orders WHERE customer_id = $1
  Frequency: 1200/hour
  Recommendation: CREATE INDEX idx_orders_customer ON orders(customer_id);
  Estimated improvement: 45x
```

## Reference-Level Explanation

### Architecture

```
PostgreSQL
  |
  +-- pg_stat_statements (query performance)
  +-- pg_stat_user_tables (table statistics)
  +-- pg_stat_user_indexes (index usage)
  |
  v
ra-monitor daemon
  |
  +-- QueryCollector: polls pg_stat_statements
  +-- RegressionDetector: compares query performance over time
  +-- OpportunityAnalyzer: runs RA optimizer on slow queries
  +-- AdvisoryEngine: generates recommendations
  +-- AlertDispatcher: sends notifications
  |
  v
Dashboard (web UI) / Alerts (Slack, email, webhook)
```

### Regression Detection

The detector maintains a rolling baseline of query performance
(mean execution time, row estimates, plan hash). A regression is
flagged when:

- Execution time increases by more than the configured threshold
- The plan hash changes (indicating a plan flip)
- Estimated vs actual row count divergence exceeds 10x

### Opportunity Analysis

For each slow query, the analyzer:

1. Extracts the query from `pg_stat_statements`
2. Gathers current statistics from the database
3. Runs the RA optimizer with the full rule set
4. Compares the RA plan cost against the current plan
5. If improvement exceeds a threshold, generates an advisory

### Recommendation Categories

| Category | Examples |
|----------|---------|
| Statistics | ANALYZE table, adjust autovacuum |
| Indexes | CREATE INDEX, DROP unused index |
| Configuration | work_mem, effective_cache_size |
| Query rewrite | Suggest query reformulation |
| Plan advice | pg_plan_advice hints (RFC 0003) |

## Drawbacks

- Polling `pg_stat_statements` adds load to the monitored database
- False positive regressions from workload changes (not actual
  regressions)
- Automated recommendations without human review risk creating
  unnecessary indexes
- Requires `pg_stat_statements` extension to be enabled

## Rationale and Alternatives

**Alternative: Integration with existing monitoring tools.**
Grafana/Prometheus exporters instead of a custom dashboard. This
could work for metrics but lacks the RA-specific optimization
analysis.

**Alternative: pganalyze or similar SaaS.** Existing tools provide
query analysis but lack RA optimization integration. The monitoring
system is specifically designed to leverage RA's rule set.

A hybrid approach is possible: export metrics to Prometheus while
keeping the advisory engine in ra-monitor.

## Prior Art

- pganalyze -- PostgreSQL performance monitoring SaaS
- pg_stat_monitor -- enhanced statistics collector
- Oracle's Automatic Database Diagnostic Monitor (ADDM)
- SQL Server's Database Engine Tuning Advisor (DTA)

## Unresolved Questions

- Should the monitor run as a PostgreSQL background worker or
  external daemon?
- How to handle multi-tenant databases with different SLAs?
- What is the minimum polling interval that doesn't impact
  production performance?
- Should recommendations be auto-applied or always require
  human approval?

## Future Possibilities

- Machine learning for regression prediction (detect before impact)
- Workload classification (OLTP vs OLAP vs mixed)
- Multi-database monitoring across a fleet
- Integration with Kubernetes operators for autoscaling decisions
- Automated A/B testing of optimization recommendations
