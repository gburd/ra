# RFC 0063: Spatial Query Optimization

- Start Date: 2026-03-25
- Author: Ra Research Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Ra should provide deep optimization for PostGIS spatial queries by
understanding spatial predicates, index types (GiST, SP-GiST, BRIN),
cost characteristics of geometric operations, and spatial join strategies.
This RFC extends RFC 0061's PostGIS section with detailed spatial predicate
analysis, multi-stage cost modeling (bounding box filter + exact geometry
test), SRID-aware optimization, and spatial join rewrite rules. The goal
is to make Ra as effective as a PostGIS expert DBA at selecting spatial
indexes, estimating spatial predicate costs, and reordering spatial joins.

## Motivation

Spatial queries are fundamentally different from scalar queries. A query
like `WHERE ST_DWithin(geom, point, 1000)` involves:
1. A bounding box pre-filter via GiST index (cheap)
2. An exact distance calculation on the filtered rows (expensive)
3. Coordinate system transformations if SRIDs differ

PostgreSQL's standard planner handles step 1 (GiST index scan) but
underestimates the cost of step 2 and does not account for step 3. This
leads to suboptimal plan choices, especially in spatial joins where the
join order determines how many expensive distance calculations occur.

**Key optimization gaps:**

| Gap | Impact |
|-----|--------|
| Flat cost for spatial functions | Wrong join ordering |
| No SRID mismatch detection | Hidden ST_Transform cost |
| No KNN vs range query distinction | Wrong index type (GiST vs SP-GiST) |
| No spatial selectivity estimation | Incorrect cardinality |
| No spatial join reordering | Cartesian explosion on large datasets |

## Guide-level explanation

### Spatial predicate classification

Ra classifies PostGIS predicates into cost tiers:

**Tier 1 - Bounding box only** (cheap, index-accelerated):
- `&&` (bounding box overlap)
- `@` (bounding box contained)
- `~` (bounding box contains)

**Tier 2 - Exact geometry** (moderate, index + recheck):
- `ST_Intersects`, `ST_Contains`, `ST_Within`, `ST_Covers`, `ST_CoveredBy`
- `ST_Overlaps`, `ST_Touches`, `ST_Crosses`

**Tier 3 - Distance computation** (expensive):
- `ST_DWithin`, `ST_Distance`
- `<->` (KNN distance operator)

**Tier 4 - Geometry construction** (very expensive):
- `ST_Buffer`, `ST_Union`, `ST_Intersection`
- `ST_Transform` (coordinate reprojection)

### Spatial index selection

Ra recommends the optimal index type based on query patterns:

```
Point-only data + KNN queries    -> SP-GiST (quad-tree, 20-40% faster)
Mixed geometry types             -> GiST (R-tree, general purpose)
Spatially sorted data (sensors)  -> BRIN (block range, smallest)
High-cardinality point lookups   -> GiST with clustering
```

### Spatial join optimization

For a spatial join like:

```sql
SELECT b.name, p.type
FROM buildings b
JOIN parcels p ON ST_Within(b.geom, p.geom)
WHERE ST_DWithin(b.geom, ST_MakePoint(-73.97, 40.77)::geometry, 1000);
```

Ra optimizes by:
1. Applying the DWithin filter on buildings first (reduces cardinality)
2. Using the GiST index on parcels.geom for the spatial join
3. Estimating the join selectivity based on geometric overlap

## Reference-level explanation

### Spatial function cost model

The cost model assigns per-row costs based on function complexity and
geometry type:

| Function | Point-Point | Point-Polygon | Polygon-Polygon | Notes |
|----------|------------|---------------|-----------------|-------|
| `ST_Intersects` | 2.0 | 8.0 | 15.0 | Edge intersection test |
| `ST_Contains` | 1.5 | 6.0 | 12.0 | Point-in-polygon |
| `ST_DWithin` | 3.0 | 10.0 | 18.0 | Distance + comparison |
| `ST_Distance` | 4.0 | 12.0 | 20.0 | Full distance calc |
| `ST_Transform` | 8.0 | 15.0 | 25.0 | Proj4 reprojection |
| `ST_Buffer` | 5.0 | 20.0 | 35.0 | Geometry construction |
| `ST_Union` | N/A | 25.0 | 40.0 | Computational geometry |

For geography types (geodesic calculations), multiply costs by 3x.

### Two-phase cost model

Spatial predicates that use GiST indexes have a two-phase cost:

```
total_cost =
    cardinality * bbox_index_cost           -- Phase 1: index
  + bbox_selectivity * cardinality * exact_cost  -- Phase 2: recheck
```

Where:
- `bbox_index_cost = 0.5` (R-tree traversal per candidate)
- `bbox_selectivity` = ratio of bounding box matches to exact matches
  (typically 1.5-5x depending on geometry complexity)
- `exact_cost` = function-specific cost from the table above

### Spatial selectivity estimation

Ra estimates spatial selectivity using geometry properties:

**For ST_DWithin(geom, center, radius):**
```
selectivity = pi * radius^2 / total_extent_area
```

Where `total_extent_area` comes from `pg_statistic` or table-level
bounding box metadata.

**For ST_Contains(container, geom):**
```
selectivity = container_area / total_extent_area
```

**For ST_Intersects(geom_a, geom_b) in joins:**
```
join_selectivity =
    avg_bbox_area_a * avg_bbox_area_b / total_extent_area^2
```

### SRID mismatch detection

When spatial operations involve columns with different SRIDs, PostgreSQL
silently succeeds but produces wrong results, or PostGIS raises an error.
Ra should detect SRID mismatches and either:
1. Warn the user about potential correctness issues
2. Insert explicit `ST_Transform` calls and account for their cost

```
IF left_srid != right_srid
   AND operation is spatial predicate
THEN add ST_Transform cost to the less-indexed side
     AND warn: "SRID mismatch: {left_srid} vs {right_srid}"
```

### Spatial join rewrite rules

Rule 1: **Distance join to KNN.** When a spatial join uses ST_DWithin
with a small radius and the outer side has few rows, convert to a
KNN-based nested loop:

```
-- Before: Hash join with ST_DWithin
SELECT * FROM a JOIN b ON ST_DWithin(a.geom, b.geom, 100)

-- After: Nested loop with KNN index scan
SELECT * FROM a,
  LATERAL (SELECT * FROM b ORDER BY b.geom <-> a.geom LIMIT k)
WHERE ST_DWithin(a.geom, b.geom, 100)
```

Rule 2: **Bounding box pre-filter.** For expensive spatial predicates,
ensure the bounding box operator `&&` is applied before the exact test:

```
-- Ensure index-accelerated pre-filter
WHERE a.geom && ST_Expand(b.geom, buffer)
  AND ST_Intersects(a.geom, b.geom)
```

Rule 3: **Spatial clustering hint.** When a spatial table has an index
but data is not spatially clustered, recommend CLUSTER:

```
IF spatial_correlation < 0.3
   AND table_size > 10000
THEN recommend:
  CLUSTER table USING spatial_index;
```

### Integration with existing Ra infrastructure

- **RFC 0061 (Extension-Aware Optimization)**: This RFC provides the
  spatial-specific rules referenced in RFC 0061's PostGIS section.
- **RFC 0021 (Index Advisor)**: Spatial index recommendations feed into
  the index advisor framework.
- **RFC 0025 (Physical Property Tracking)**: SRID can be tracked as a
  physical property of spatial columns.
- **RFC 0026 (Adaptive Cost Calibration)**: Spatial function costs can
  be calibrated from pg_stat_statements execution data.

## Drawbacks

**Geometry type inference.** Determining whether a column contains points,
lines, or polygons requires querying `geometry_columns` or sampling data.
This adds planning overhead for the first query on a spatial table.

**SRID catalog dependency.** SRID information is stored in PostGIS's
`spatial_ref_sys` table, which may not be accessible in all contexts.

**Cost model accuracy.** Spatial operation costs vary by orders of
magnitude depending on geometry complexity (number of vertices). The
per-type cost multipliers are rough approximations.

## Rationale and alternatives

The two-phase cost model (bounding box + exact) is the correct model for
PostGIS queries because it matches how GiST indexes actually work. No
alternative approaches provide the same accuracy for spatial query planning.

## Prior art

- **PostGIS documentation**: Recommends `&&` pre-filters for performance.
  Ra automates this recommendation.
- **CockroachDB spatial**: Built-in spatial index support with inverted
  indexes. Uses S2 geometry library for selectivity estimation.
- **Oracle Spatial**: SDO_FILTER (bounding box) + SDO_RELATE (exact)
  two-phase approach, similar to PostGIS GiST.

## Unresolved questions

1. How to estimate vertex count for polygon complexity without scanning
   all geometries?
2. Should Ra support raster operations from PostGIS Raster, or limit
   to vector geometry?
3. How to handle PostGIS topology extension (different schema and
   operations)?

## Future possibilities

- **3D spatial optimization**: Support for 3D geometry operations
  (ST_3DDistance, ST_3DIntersects)
- **Spatial partitioning**: Recommend spatial partitioning for very
  large spatial tables (hash on geohash or grid)
- **Raster query optimization**: Cost model for raster operations
  (ST_MapAlgebra, ST_Clip)
