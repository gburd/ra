# Index Types

This document describes the index type model in `ra-stats-advanced::index_types`,
which the RA optimizer uses to select access paths and estimate I/O cost
for each candidate plan.

## Overview

The `IndexType` enum represents every index access method the optimizer
knows about. Each variant carries the structural parameters the cost model
needs to estimate lookup, range-scan, and tuple-fetch costs.

The companion `IndexMetadata` struct bundles an `IndexType` with its
table, physical `IndexStats` (from the catalog), and per-operation
`IndexCostFactors`.

Source: `crates/ra-stats-advanced/src/index_types.rs`

## Index Types

### Clustered

The table's rows are physically stored in index-key order. Range scans
are sequential I/O; no separate heap fetch is needed.

```sql
-- MySQL InnoDB: primary key is always clustered
-- SQL Server: CREATE CLUSTERED INDEX ix ON orders(order_id)
```

Best for: wide range scans, ORDER BY on the clustering key.

### NonClustered (Covering)

A secondary B-tree with optional `INCLUDE` columns. When all columns
the query needs are in the index, the optimizer can do an index-only
scan and skip the heap entirely.

```sql
CREATE INDEX ix_cust ON orders(customer_id) INCLUDE (order_date, total);
```

Best for: point lookups and narrow queries that touch only indexed columns.

### Composite

A multi-column B-tree with explicit column ordering. The optimizer checks
whether the query's predicate matches a prefix of the key columns.

```sql
CREATE INDEX ix_name_date ON events(user_id, event_date);
```

Best for: queries filtering on the leading columns of the key.

### FullText

An inverted index for natural-language search. Supports `@@` (PostgreSQL),
`MATCH AGAINST` (MySQL), and `CONTAINS` (SQL Server).

```sql
CREATE INDEX ix_body ON articles USING gin(to_tsvector('english', body));
```

Best for: text search, relevance ranking, phrase matching.

### Unique

A B-tree with a uniqueness constraint. The optimizer knows an equality
lookup returns at most one row, enabling cardinality-1 propagation.

```sql
CREATE UNIQUE INDEX ix_email ON users(email);
```

Best for: primary key and candidate key lookups.

### Filtered (Partial)

An index that covers only rows matching a WHERE clause. Smaller than a
full index, yielding faster lookups and less storage.

```sql
-- PostgreSQL
CREATE INDEX ix_active ON users(email) WHERE active = true;
-- SQL Server
CREATE INDEX ix_active ON users(email) WHERE active = 1;
```

Best for: queries whose predicates imply the index filter, targeting a
common subset of the table.

### Spatial

An R-tree or GiST index for geometric/geographic data. Supports bounding-
box queries and nearest-neighbor lookups.

```sql
CREATE INDEX ix_geom ON parcels USING gist(geom);
```

Best for: ST_Contains, ST_Intersects, ST_DWithin, KNN queries.

### Columnstore

A column-oriented storage index optimized for analytical aggregation.
Stores data in compressed column segments; scans only the columns needed.

```sql
-- SQL Server
CREATE COLUMNSTORE INDEX ix_cs ON sales(amount, quantity, region);
```

Best for: full-table aggregations, OLAP workloads, wide tables with
selective column access.

### Hash

Equality-only index with O(1) lookups. Does not support range scans or
ordering.

```sql
-- PostgreSQL
CREATE INDEX ix_token ON sessions USING hash(token);
```

Best for: exact-match lookups on high-cardinality columns.

### GIN (Generalized Inverted Index)

PostgreSQL index for composite values: arrays, JSONB, full-text vectors.
Supports containment (`@>`), overlap (`&&`), and text-search operators.

```sql
CREATE INDEX ix_tags ON posts USING gin(tags);
CREATE INDEX ix_data ON events USING gin(data jsonb_ops);
```

Best for: array containment, JSONB queries, trigram text search.

### GiST (Generalized Search Tree)

PostgreSQL extensible index supporting spatial, range, and nearest-
neighbor queries. Used by PostGIS for geometry columns.

```sql
CREATE INDEX ix_geo ON stores USING gist(location);
```

Best for: spatial predicates, range types, exclusion constraints.

### BRIN (Block Range Index)

PostgreSQL index that stores min/max summaries per block range. Tiny
(often <1% of a B-tree) but effective only when the column is well-
correlated with physical row order.

```sql
CREATE INDEX ix_ts ON logs USING brin(created_at);
```

Best for: append-only time-series tables, naturally ordered data.

### Bitmap

Stores a bitmap per distinct value. Efficient for low-cardinality columns
and for combining multiple predicates via bitwise AND/OR.

Best for: data-warehouse fact tables with low-cardinality dimension keys.

### Expression

An index on a computed expression rather than a raw column. The optimizer
matches predicates that apply the same function.

```sql
CREATE INDEX ix_lower_email ON users (LOWER(email));
-- Matches: WHERE LOWER(email) = 'alice@example.com'
```

Best for: case-insensitive lookups, date truncation, computed values.

## Cost Factors

Each index carries `IndexCostFactors` with four fields:

| Field             | Meaning                                       |
|-------------------|-----------------------------------------------|
| `lookup_cost`     | Cost of a single-key B-tree traversal         |
| `range_scan_cost` | Cost per leaf page during a range scan         |
| `tuple_fetch_cost`| Cost per row when a heap fetch is required     |
| `covering`        | If true, heap fetches are never needed         |

The optimizer calls `point_lookup_cost(rows)` or `range_cost(pages, rows)`
to get total estimated I/O for each access path.

Default cost factories are provided:
`btree_default()`, `hash_default()`, `brin_default()`, `gin_default()`,
`columnstore_default()`.

## Access Path Selection

The optimizer evaluates candidate indexes by:

1. Checking `matches_predicate()` -- do the key columns match the query?
2. Checking `leading_column_matches()` -- is the leading key column bound?
3. Computing the cost via `IndexCostFactors` methods.
4. Comparing against a sequential scan and other candidate indexes.
5. Selecting the cheapest access path.

See also: [cost-models.md](cost-models.md), optimization rules in
`rules/physical/index-selection/`.
