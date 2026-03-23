# SQL Query Encyclopedia

Comprehensive reference guide documenting all SQL query patterns, schema designs, dataset characteristics, and workload patterns that Ra optimizes. This encyclopedia bridges theory and practice, showing how Ra's optimizer handles real-world database scenarios.

## Purpose

This encyclopedia serves three audiences:

1. **Database Engineers** - Understand how Ra optimizes your specific query patterns
2. **Optimizer Developers** - Learn which patterns Ra supports and how to extend coverage
3. **Researchers** - See practical applications of relational algebra transformations

## Structure

###  [Query Patterns](query-patterns/)

50+ SQL query patterns organized by category:

- **[OLTP Queries](query-patterns/oltp/)** - Point lookups, simple updates, transactional patterns
- **[OLAP Queries](query-patterns/olap/)** - Aggregations, GROUP BY, ROLLUP, CUBE, materialized views
- **[Analytical](query-patterns/analytical/)** - Window functions, ranking, percentiles, moving averages
- **[Recursive](query-patterns/recursive/)** - CTEs with UNION ALL, transitive closure
- **[Hierarchical](query-patterns/hierarchical/)** - Tree traversal, bill of materials, org charts
- **[Temporal](query-patterns/temporal/)** - Date ranges, time series, temporal joins
- **[Set Operations](query-patterns/set-operations/)** - UNION, INTERSECT, EXCEPT
- **[Subqueries](query-patterns/subqueries/)** - Correlated, uncorrelated, EXISTS, IN, scalar
- **[Joins](query-patterns/joins/)** - Inner, outer, cross, self, lateral, semi, anti

Each pattern includes:
- Plain English description and use cases
- Relational algebra notation (LaTeX)
- Ra optimization rules that apply
- API usage for providing statistics
- Code examples with expected plans

###  [Schema Patterns](schema-patterns/)

Database schema designs and how Ra handles them:

- **[Normalized](schema-patterns/normalized.md)** - 3NF, BCNF with many joins
- **[Denormalized](schema-patterns/denormalized.md)** - Wide tables, redundant data
- **[Star Schema](schema-patterns/star-schema.md)** - Fact and dimension tables
- **[Snowflake Schema](schema-patterns/snowflake-schema.md)** - Normalized dimensions
- **[Temporal Tables](schema-patterns/temporal-tables.md)** - History tracking, Type 2 SCD
- **[Partitioned Tables](schema-patterns/partitioned-tables.md)** - Range, hash, list partitioning
- **[Sharded Tables](schema-patterns/sharded-tables.md)** - Multi-node distribution

###  [Dataset Characteristics](dataset-characteristics/)

How data properties affect optimization:

- **[Cardinality](dataset-characteristics/cardinality.md)** - Low vs high cardinality columns
- **[Distribution](dataset-characteristics/distribution.md)** - Uniform, Zipfian, normal
- **[Skew](dataset-characteristics/skew.md)** - Hotspots, imbalanced partitions
- **[Correlation](dataset-characteristics/correlation.md)** - Column dependencies
- **[Null Handling](dataset-characteristics/null-handling.md)** - Sparse columns
- **[String Patterns](dataset-characteristics/string-patterns.md)** - Short vs long strings
- **[Numeric Ranges](dataset-characteristics/numeric-ranges.md)** - Bounded vs unbounded

###  [Workload Patterns](workload-patterns/)

Query workload characteristics:

- **[OLTP](workload-patterns/oltp.md)** - Short transactions, high concurrency
- **[OLAP](workload-patterns/olap.md)** - Long analytical queries, batch processing
- **[HTAP](workload-patterns/htap.md)** - Mixed OLTP/OLAP workloads
- **[Read-Heavy](workload-patterns/read-heavy.md)** - 95%+ read queries
- **[Write-Heavy](workload-patterns/write-heavy.md)** - High insert/update volume
- **[Batch Processing](workload-patterns/batch-processing.md)** - ETL, nightly jobs
- **[Real-Time](workload-patterns/real-time.md)** - Streaming, sub-second latency
- **[Ad-Hoc](workload-patterns/ad-hoc.md)** - Unpredictable query patterns

###  [Distributed Patterns](distributed-patterns/)

Multi-node query execution:

- **[Partition Pruning](distributed-patterns/partition-pruning.md)** - Eliminating partitions
- **[Co-located Joins](distributed-patterns/co-located-joins.md)** - Joining on partition keys
- **[Broadcast Joins](distributed-patterns/broadcast-joins.md)** - Small table replication
- **[Shuffle Joins](distributed-patterns/shuffle-joins.md)** - Hash redistribution
- **[Push-down Aggregation](distributed-patterns/pushdown-aggregation.md)** - Pre-aggregation
- **[Union Over Partitions](distributed-patterns/union-over-partitions.md)** - Parallel scan

###  [Index Structures](index-structures/)

Index types and selection:

- **[B-tree Indexes](index-structures/btree.md)** - Range queries, sorted access
- **[Hash Indexes](index-structures/hash.md)** - Exact match lookups
- **[Bitmap Indexes](index-structures/bitmap.md)** - Low cardinality, set operations
- **[GiST Indexes](index-structures/gist.md)** - Geometric, full-text search
- **[GIN Indexes](index-structures/gin.md)** - Array, JSONB queries
- **[Covering Indexes](index-structures/covering.md)** - Index-only scans
- **[Partial Indexes](index-structures/partial.md)** - Filtered indexes

## Navigation Guide

### By Use Case

**"I need to optimize a specific query"**
-> Start with [Query Patterns](query-patterns/) matching your query structure

**"My queries are slow on this schema"**
-> Check [Schema Patterns](schema-patterns/) for your design

**"My data has unusual characteristics"**
-> Read [Dataset Characteristics](dataset-characteristics/)

**"I'm tuning for a specific workload"**
-> See [Workload Patterns](workload-patterns/)

**"I'm deploying distributed queries"**
-> Study [Distributed Patterns](distributed-patterns/)

**"Ra isn't using my indexes"**
-> Review [Index Structures](index-structures/)

### By Role

**Database Administrator**
- [Index Structures](index-structures/) - Understand index selection
- [Schema Patterns](schema-patterns/) - Design for optimal performance
- [Workload Patterns](workload-patterns/) - Tune for your workload

**Application Developer**
- [Query Patterns](query-patterns/) - Write optimizable queries
- [OLTP Queries](query-patterns/oltp/) - Transaction patterns
- [Subqueries](query-patterns/subqueries/) - Avoid anti-patterns

**Data Analyst**
- [OLAP Queries](query-patterns/olap/) - Aggregation patterns
- [Analytical Queries](query-patterns/analytical/) - Window functions
- [Temporal Queries](query-patterns/temporal/) - Time series analysis

**Data Engineer**
- [Distributed Patterns](distributed-patterns/) - Multi-node execution
- [Batch Processing](workload-patterns/batch-processing.md) - ETL optimization
- [Partitioned Tables](schema-patterns/partitioned-tables.md) - Data layout

## Mathematical Notation

This encyclopedia uses standard database notation:

### Relations and Tuples
- $R, S, T$ - Relations (tables)
- $r, s, t$ - Tuples (rows)
- $|R|$ - Cardinality (row count) of relation $R$
- $\text{dom}(A)$ - Domain (distinct values) of attribute $A$

### Relational Algebra Operators
- $\sigma_{\theta}(R)$ - Selection (WHERE clause) with predicate $\theta$
- $\pi_{A_1, \ldots, A_n}(R)$ - Projection (SELECT clause)
- $R \bowtie_{\theta} S$ - Join with predicate $\theta$
- $R \times S$ - Cross product (Cartesian product)
- $R \cup S$ - Union
- $R \cap S$ - Intersection
- $R - S$ - Difference (EXCEPT)
- $\rho_{A/B}(R)$ - Rename attribute $B$ to $A$
- $\gamma_{G, F}(R)$ - Grouping by $G$ with aggregates $F$
- $\tau_{A}(R)$ - Sort by attribute $A$

### Cost Model Notation
- $C_{\text{io}}$ - I/O cost per page
- $C_{\text{cpu}}$ - CPU cost per tuple
- $C_{\text{network}}$ - Network cost per byte
- $P$ - Number of partitions/nodes
- $B(R)$ - Number of blocks/pages for relation $R$
- $\text{sel}(\theta)$ - Selectivity (fraction selected) of predicate $\theta$

### Cardinality Estimation
- $\hat{|R|}$ - Estimated cardinality
- $|R \bowtie_{\theta} S| \approx \frac{|R| \times |S|}{\max(\text{dom}(A), \text{dom}(B))}$ - Join estimation

## Cross-References

Links throughout this encyclopedia:

- **[Rule Name]** -> Links to `/docs/rules/` for specific transformation rules
- **[Feature Name]** -> Links to `/docs/features/` for capability deep-dives
- **[Example]** -> Links to `/docs/examples/` for working code
- **[API]** -> Links to `/docs/api-reference.md` for programmatic usage

## Contributing

To add new patterns:

1. Create markdown file in appropriate directory
2. Follow the template structure (see any existing pattern)
3. Include relational algebra, optimization rules, and examples
4. Cross-link to related patterns and rules
5. Update this README with new entry

## References

- [Ra Architecture](../architecture.md)
- [Relational Algebra Concepts](../concepts/relational-algebra.md)
- [Rule Categories](../concepts/rule-categories.md)
- [Optimization Guide](../guides/optimization.md)
