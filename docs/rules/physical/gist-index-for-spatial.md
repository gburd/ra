# Rule: GiST Index for Spatial and Range Types

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/gist-index-for-spatial.rra`

## Metadata

- **ID:** `gist-index-for-spatial`
- **Version:** "1.0.0"
- **Databases:** postgresql
- **Tags:** index, gist, spatial, range, postgresql, nearest-neighbor
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter (spatial_op ?col ?geom) (scan ?table))"
    description: "Spatial predicate with GiST index"
  - type: "predicate"
    condition: "has_gist_index(?table, ?col)"
    description: "GiST index must exist on the spatial column"
  - type: "capability"
    database: "current"
    requires: "gist_index"
    description: "Database supports GiST indexes"
```


# GiST Index for Spatial and Range Types

## Description

Uses a PostgreSQL GiST (Generalized Search Tree) index for spatial
queries, range type containment, and nearest-neighbor searches. GiST
is a balanced tree that supports arbitrary decomposition strategies,
making it suitable for geometric, range, and network address data.

**When to apply**: A query uses spatial operators (&&, @>, <@),
range overlap, or ORDER BY distance with a GiST-indexed column.

**Why it works**: GiST indexes partition the search space into
hierarchical bounding structures. Spatial containment checks prune
entire subtrees, and KNN searches use priority-queue-based traversal.

## Relational Algebra

```algebra
filter[geom && bbox](scan[T])
  -> gist_index_scan[I](geom, &&, bbox)
  where I is a GiST index on column geom

sort[ST_Distance(geom, point)](scan[T]) LIMIT k
  -> gist_knn_scan[I](geom, point, k)
```

## Implementation

```rust
rw!("gist-index-for-overlap";
    "(filter (overlaps ?col ?val) (scan ?table))" =>
    "(gist-index-scan ?index ?col overlaps ?val)"
    if has_gist_index_on("?table", "?col")
),

rw!("gist-index-for-knn";
    "(limit ?k (sort (st-distance ?col ?point)
        (scan ?table)))" =>
    "(gist-knn-scan ?index ?col ?point ?k)"
    if has_gist_index_on("?table", "?col")
),
```

## Cost Model

```rust
fn cost(
    tree_height: u32,
    matching_nodes: u64,
    matching_rows: u64,
) -> f64 {
    let traversal = tree_height as f64 * 2.0;
    let node_checks = matching_nodes as f64;
    let recheck = matching_rows as f64 * 5.0;
    traversal + node_checks + recheck
}
```

**Typical benefit**: 50-95% for spatial and range containment queries.

## Test Cases

### Positive: PostGIS bounding box query

```sql
CREATE INDEX idx_parcels_geom ON parcels USING GIST(geom);

SELECT * FROM parcels
WHERE geom && ST_MakeEnvelope(-74, 40, -73, 41, 4326);

-- GiST prunes non-overlapping bounding boxes
```

### Positive: KNN nearest-neighbor

```sql
CREATE INDEX idx_restaurants_loc ON restaurants USING GIST(location);

SELECT name, ST_Distance(location, ST_Point(-73.99, 40.73)) AS dist
FROM restaurants
ORDER BY location <-> ST_Point(-73.99, 40.73)
LIMIT 10;

-- GiST KNN: traverses tree by distance priority
```

### Positive: Range type containment

```sql
CREATE INDEX idx_bookings_period ON bookings USING GIST(period);

SELECT * FROM bookings
WHERE period && tsrange('2025-01-01', '2025-02-01');

-- GiST for temporal range overlap
```

## References

- PostgreSQL: GiST indexes
- PostGIS: Spatial indexing with GiST
- IndexType::GiST in ra-stats/src/index_types.rs
