# Rules by Database System

This index organizes rules by the database systems they support.

## PostgreSQL

Rules optimized for or compatible with PostgreSQL:

### Core Optimizations
- Predicate pushdown through joins and aggregates
- Join elimination and reordering
- Subquery unnesting (LATERAL, EXISTS, IN)
- CTE optimization and inlining
- Window function optimization
- Partial index usage
- Index-only scans (covering indexes)
- Bitmap index scans
- Parallel query execution

### PostgreSQL-Specific
- Multi-version concurrency control (MVCC) aware optimizations
- Toast table handling for large objects
- Tablespace-aware data placement
- Extension-specific optimizations (PostGIS, pg_trgm, etc.)
- Foreign data wrapper (FDW) pushdown

## MySQL / MariaDB

Rules optimized for MySQL and MariaDB:

### Core Optimizations
- Index merge optimization
- Join buffer optimization
- Subquery materialization
- Derived table merging
- Partition pruning
- Semi-join optimization

### MySQL-Specific
- Storage engine specific (InnoDB, MyISAM, Memory)
- Multi-range read optimization
- Index condition pushdown (ICP)
- Batched key access (BKA)
- Hash join optimization (MySQL 8.0+)

## DuckDB

Rules for the analytical database DuckDB:

### Core Optimizations
- Vectorized execution
- Columnar storage optimization
- Parallel aggregation
- Join order optimization
- Filter pushdown to Parquet files
- Adaptive query execution

### DuckDB-Specific
- Out-of-core processing
- Compression-aware query processing
- Arrow integration
- CSV/JSON direct querying
- Checkpoint optimization

## Oracle Database

Rules for Oracle Database:

### Core Optimizations
- Cost-based optimization (CBO)
- Adaptive query optimization
- Star transformation
- Partition-wise joins
- Parallel DML and DDL
- Result cache usage

### Oracle-Specific
- Exadata smart scan
- In-memory column store
- Automatic indexing
- SQL plan management
- Real Application Clusters (RAC) optimization

## Microsoft SQL Server

Rules for SQL Server:

### Core Optimizations
- Batch mode processing
- Adaptive joins
- Memory grant feedback
- Interleaved execution
- Intelligent query processing

### SQL Server-Specific
- Columnstore indexes
- In-memory OLTP (Hekaton)
- PolyBase external tables
- Query Store integration
- Resource Governor aware

## Apache Calcite

Rules from the Apache Calcite optimizer framework:

### Core Rules
- RelOptRule implementations
- Planner rules (Volcano, HepPlanner)
- Trait propagation
- Cost model abstractions
- Physical property enforcement

## ClickHouse

Rules for the columnar OLAP database:

### Core Optimizations
- MergeTree optimizations
- Distributed query execution
- Sampling and approximation
- Materialized view selection
- Dictionary encoding

## CockroachDB

Rules for the distributed SQL database:

### Core Optimizations
- Distributed join strategies
- Range-based sharding
- Locality-aware optimization
- Vectorized execution
- Raft consensus optimization

## MongoDB

Rules for document database optimization:

### Core Optimizations
- Index intersection
- Covered queries
- Pipeline optimization
- Sharding-aware queries
- Aggregation pushdown

## Cross-Database Rules

Rules that apply across multiple systems:

### Universal Optimizations
- Common subexpression elimination
- Constant folding
- Dead code elimination
- Predicate simplification
- Join associativity and commutativity

### SQL Standard Compliance
- SQL:1992 compliant transformations
- SQL:1999 window functions
- SQL:2003 features
- SQL:2011 temporal queries
- SQL:2016 JSON support

## Database Feature Matrix

| Feature | PostgreSQL | MySQL | DuckDB | Oracle | SQL Server | ClickHouse |
|---------|------------|-------|--------|--------|------------|------------|
| Parallel Query | ✓ | ✓ (8.0+) | ✓ | ✓ | ✓ | ✓ |
| Vectorized Execution | Partial | - | ✓ | - | ✓ | ✓ |
| Columnar Storage | Extension | - | ✓ | ✓ | ✓ | ✓ |
| Adaptive Optimization | ✓ | Limited | ✓ | ✓ | ✓ | - |
| Cost-Based Optimizer | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Join Algorithms | 3 types | 3 types | 4 types | 5 types | 4 types | 3 types |
| Index Types | 7+ | 4 | 2 | 8+ | 6+ | 5+ |

## Usage Guide

To find rules for your database:
1. Check the database-specific section above
2. Browse `/database-specific/[your-db]/` directory
3. Search rule metadata for your database name
4. Review cross-database rules for general optimizations