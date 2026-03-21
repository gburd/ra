# RFC 0021: Automatic Index Advisor

- Start Date: 2026-03-20
- Author: System
- Status: Implemented

## Summary

Implement an automatic index advisor that analyzes query workloads and recommends indexes to improve performance, similar to PostgreSQL's hypothetical indexes and Microsoft's Database Tuning Advisor.

## Motivation

### Problem

DBAs manually create indexes based on:
- Slow query logs
- EXPLAIN output showing sequential scans
- Intuition about query patterns

This is time-consuming and error-prone. Many beneficial indexes are never created.

### Solution

Automate index recommendations:
```bash
# Analyze workload
ra-cli advisor analyze --workload queries.sql --stats stats.toml

# Output:
# Recommended indexes:
# 1. CREATE INDEX idx_users_email ON users(email);
#    - Benefits 15 queries (avg 12x speedup)
#    - Cost: 45 MB storage, 3s build time
# 2. CREATE INDEX idx_orders_user_date ON orders(user_id, created_at);
#    - Benefits 8 queries (avg 5x speedup)
#    - Cost: 120 MB storage, 8s build time
```

## Technical design

### Architecture

```
┌─────────────────────────────────────┐
│ Workload Analysis                   │
│  - Parse queries from log           │
│  - Extract table scans + filters    │
│  - Count query frequencies          │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│ Index Candidate Generation          │
│  - Columns in WHERE clauses         │
│  - Columns in JOIN conditions       │
│  - Columns in ORDER BY/GROUP BY     │
│  - Composite index combinations     │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│ Benefit Estimation                  │
│  - Optimize with hypothetical index │
│  - Compare cost: with vs without    │
│  - Aggregate across all queries     │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│ Cost Estimation                     │
│  - Storage overhead (index size)    │
│  - Write overhead (INSERT/UPDATE)   │
│  - Build time                       │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│ Recommendation Ranking              │
│  - Benefit/cost ratio               │
│  - Remove redundant indexes         │
│  - Top N recommendations            │
└─────────────────────────────────────┘
```

### Data Structures

```rust
// crates/ra-advisor/src/lib.rs

pub struct IndexCandidate {
    pub table: String,
    pub columns: Vec<String>,
    pub index_type: IndexType,  // BTree, Hash, GIN, etc.
    pub unique: bool,
    pub partial_predicate: Option<Expr>,
}

pub struct IndexRecommendation {
    pub candidate: IndexCandidate,
    pub benefit: IndexBenefit,
    pub cost: IndexCost,
    pub score: f64,  // benefit / cost ratio
}

pub struct IndexBenefit {
    /// Queries that would use this index
    pub affected_queries: Vec<QueryId>,
    /// Average speedup per query
    pub avg_speedup: f64,
    /// Total cost saved across all queries
    pub total_cost_saved: f64,
}

pub struct IndexCost {
    /// Storage overhead in bytes
    pub storage_bytes: u64,
    /// Write overhead per INSERT (0.0 to 1.0)
    pub write_overhead: f64,
    /// Index build time in seconds
    pub build_time_secs: f64,
}
```

### Candidate Generation

```rust
impl IndexAdvisor {
    pub fn generate_candidates(&self, queries: &[Query]) -> Vec<IndexCandidate> {
        let mut candidates = Vec::new();

        for query in queries {
            // 1. WHERE clause columns
            for col in self.extract_filter_columns(query) {
                candidates.push(IndexCandidate {
                    table: col.table.clone(),
                    columns: vec![col.column.clone()],
                    index_type: IndexType::BTree,
                    unique: false,
                    partial_predicate: None,
                });
            }

            // 2. JOIN columns
            for (left, right) in self.extract_join_columns(query) {
                candidates.push(IndexCandidate {
                    table: left.table.clone(),
                    columns: vec![left.column.clone()],
                    index_type: IndexType::BTree,
                    unique: false,
                    partial_predicate: None,
                });
            }

            // 3. Composite indexes (two-column combos)
            for (col1, col2) in self.extract_composite_candidates(query) {
                candidates.push(IndexCandidate {
                    table: col1.table.clone(),
                    columns: vec![col1.column.clone(), col2.column.clone()],
                    index_type: IndexType::BTree,
                    unique: false,
                    partial_predicate: None,
                });
            }
        }

        self.deduplicate(candidates)
    }
}
```

### Hypothetical Index Optimization

```rust
impl IndexAdvisor {
    pub fn estimate_benefit(
        &self,
        candidate: &IndexCandidate,
        query: &Query,
    ) -> f64 {
        // 1. Optimize query WITHOUT index
        let plan_without = self.optimizer.optimize(query)?;
        let cost_without = self.cost_model.estimate(&plan_without)?;

        // 2. Add hypothetical index to schema
        let mut schema = self.schema.clone();
        schema.add_hypothetical_index(candidate);

        // 3. Optimize query WITH index
        let optimizer_with_index = Optimizer::new(schema);
        let plan_with = optimizer_with_index.optimize(query)?;
        let cost_with = self.cost_model.estimate(&plan_with)?;

        // 4. Benefit = cost saved
        cost_without.total() - cost_with.total()
    }
}
```

### CLI Commands

```bash
# Analyze workload from SQL file
ra-cli advisor analyze \
    --workload queries.sql \
    --stats stats.toml \
    --top 10

# Analyze from PostgreSQL slow query log
ra-cli advisor analyze \
    --postgres-log /var/log/postgresql/postgresql.log \
    --min-duration 1000ms \
    --top 10

# Generate SQL to create recommended indexes
ra-cli advisor generate-sql \
    --recommendations recommendations.json \
    --output create_indexes.sql
```

## Examples

### Input: Slow query workload
```sql
-- queries.sql
SELECT * FROM users WHERE email = 'alice@example.com';  -- runs 1000x/day
SELECT * FROM orders WHERE user_id = 123 ORDER BY created_at DESC;  -- runs 500x/day
SELECT * FROM orders o JOIN users u ON o.user_id = u.id WHERE u.country = 'US';  -- runs 200x/day
```

### Output: Recommendations
```
Index Recommendations:
======================

1. CREATE INDEX idx_users_email ON users(email);
   Benefit:
     - Affects 1 query (1000 executions/day)
     - Speedup: 15x (150ms -> 10ms)
     - Daily cost saved: 140,000 cost units
   Cost:
     - Storage: 12 MB
     - Write overhead: +5% on INSERT
     - Build time: 2 seconds
   Score: 11,666 (HIGH PRIORITY)

2. CREATE INDEX idx_orders_user_created ON orders(user_id, created_at);
   Benefit:
     - Affects 2 queries (700 executions/day)
     - Speedup: 8x (200ms -> 25ms)
     - Daily cost saved: 85,000 cost units
   Cost:
     - Storage: 45 MB
     - Write overhead: +8% on INSERT
     - Build time: 6 seconds
   Score: 1,888 (MEDIUM PRIORITY)

3. CREATE INDEX idx_users_country ON users(country);
   Benefit:
     - Affects 1 query (200 executions/day)
     - Speedup: 3x (500ms -> 166ms)
     - Daily cost saved: 20,000 cost units
   Cost:
     - Storage: 5 MB
     - Write overhead: +3% on INSERT
     - Build time: 1 second
   Score: 4,000 (MEDIUM PRIORITY)
```

## Implementation plan

- Week 1: Workload parsing and candidate generation
- Week 2: Hypothetical index optimization
- Week 3: Cost/benefit estimation
- Week 4: Recommendation ranking and CLI
- Week 5: PostgreSQL log integration
- Week 6: Testing and documentation

## Prior art

- PostgreSQL: `pg_stat_statements` + manual analysis
- Microsoft SQL Server: Database Tuning Advisor (DTA)
- Azure SQL: Automatic index recommendations
- AWS RDS: Performance Insights with index recommendations
- Dexter (standalone tool for PostgreSQL)

## Gap addressed

This addresses a feature not explicitly in postgres-planner-gaps.md but frequently requested in production: proactive performance optimization.
