# Rule: MongoDB Geospatial Index Optimization

**Category:** database-specific/mongodb
**File:** `rules/database-specific/mongodb/geospatial-index-optimization.rra`

## Metadata

- **ID:** `mongodb-geospatial-index`
- **Version:** "1.0.0"
- **Databases:** mongodb
- **Tags:** geospatial, 2dsphere, geonear, location
- **Authors:** "MongoDB Inc."


# MongoDB Geospatial Index Optimization

## Description

Uses 2dsphere or 2d geospatial indexes for location-based queries ($geoNear,
$geoWithin, $near) instead of scanning all documents and computing distances.
Geospatial indexes dramatically speed up proximity and containment queries.

**When to apply**: Queries with $near, $geoWithin, $geoIntersects on GeoJSON
or legacy coordinate pairs. 2dsphere index enables efficient spatial operations.

**Why it works**: Geospatial indexes use S2 geometry (spherical) or R-tree
structures to index locations, enabling logarithmic-time proximity searches
vs. linear full collection scans with distance calculations.

## Test Cases

### Positive: $near with 2dsphere index

```javascript
// Index: {location: "2dsphere"}
db.places.find({
  location: {
    $near: {
      $geometry: {type: "Point", coordinates: [-73.97, 40.77]},
      $maxDistance: 5000  // 5km
    }
  }
})
// Uses index to find nearby points, O(log n)
```

### Positive: $geoWithin polygon

```javascript
// Index: {location: "2dsphere"}
db.places.find({
  location: {
    $geoWithin: {
      $geometry: {
        type: "Polygon",
        coordinates: [[[...polygon coordinates...]]]
      }
    }
  }
})
```

## References

**Documentation:**
- MongoDB Manual: "Geospatial Queries"
- https://docs.mongodb.com/manual/geospatial-queries/
