# Rule: Spatial Index for Geometry Queries

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/spatial-index-for-geometry.rra`

## Metadata

- **ID:** `spatial-index-for-geometry`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, mssql, oracle
- **Tags:** index, spatial, r-tree, gist, geometry, geographic
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter (spatial_op ?col ?geom) (scan ?table))"
    description: "Spatial operation filter with spatial index"
  - type: "predicate"
    condition: "has_spatial_index(?table, ?col)"
    description: "Spatial index (R-tree/GiST) must exist on the column"
  - type: "capability"
    database: "current"
    requires: "spatial_index"
    description: "Database supports spatial indexes"
```


# Spatial Index for Geometry Queries

## Description

Uses an R-tree or GiST spatial index for geometry predicates such as
ST_Contains, ST_Intersects, ST_DWithin, and bounding box operations.
Spatial indexes partition 2D/3D space into hierarchical bounding
boxes, enabling logarithmic search for spatial relationships.

**When to apply**: A spatial predicate (containment, intersection,
distance) operates on a geometry column with a spatial index.

**Why it works**: Without a spatial index, every row's geometry must
be tested against the predicate (O(n)). R-tree indexes prune entire
subtrees of non-overlapping bounding boxes, reducing to O(log n + k).

## Relational Algebra

```algebra
filter[ST_Contains(geom, ?point)](scan[T])
  -> spatial_index_scan[I](geom, ST_Contains, ?point)
  where I is a spatial index on column geom
```

## Implementation

```rust
rw!("spatial-index-for-contains";
    "(filter (st-contains ?col ?geom) (scan ?table))" =>
    "(spatial-index-scan ?index ?col st-contains ?geom)"
    if has_spatial_index_on("?table", "?col")
),

rw!("spatial-index-for-dwithin";
    "(filter (st-dwithin ?col ?point ?dist) (scan ?table))" =>
    "(spatial-index-scan ?index ?col st-dwithin ?point ?dist)"
    if has_spatial_index_on("?table", "?col")
),
```

## Cost Model

```rust
fn cost(
    bounding_box_matches: u64,
    exact_matches: u64,
    index_height: u32,
) -> f64 {
    let tree_search = index_height as f64 * 2.0;
    let leaf_scan = bounding_box_matches as f64;
    let recheck = exact_matches as f64 * 5.0; // Exact geometry test
    tree_search + leaf_scan + recheck
}
```

**Typical benefit**: 50-99% for selective spatial queries.

## Test Cases

### Positive: Point-in-polygon with spatial index

```sql
CREATE INDEX idx_zones_geom ON zones USING GIST(geom);

SELECT * FROM zones
WHERE ST_Contains(geom, ST_Point(-73.9857, 40.7484));

-- R-tree prunes non-overlapping zones
```

### Positive: Nearest-neighbor search

```sql
CREATE INDEX idx_stores_location ON stores USING GIST(location);

SELECT * FROM stores
WHERE ST_DWithin(location, ST_Point(-122.4, 37.8), 1000);

-- Spatial index finds stores within 1km
```

### Negative: No spatial index

```sql
SELECT * FROM parcels
WHERE ST_Intersects(boundary, ST_MakeEnvelope(0, 0, 10, 10));

-- Without spatial index: full sequential scan
```

## References

- PostgreSQL: GiST indexes for PostGIS
- MySQL: Spatial indexes on GEOMETRY columns
- IndexType::Spatial and IndexType::GiST in ra-stats/src/index_types.rs
