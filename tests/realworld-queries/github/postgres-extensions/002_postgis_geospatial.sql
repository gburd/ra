-- PostGIS Geospatial Queries
-- Source: Location-based services, mapping applications
-- Pattern: OLTP/OLAP hybrid - Spatial queries

CREATE TABLE locations (
    id INTEGER PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    category VARCHAR(100),
    coordinates GEOMETRY(Point, 4326),
    address TEXT,
    city VARCHAR(100),
    country VARCHAR(100)
);

CREATE TABLE delivery_zones (
    id INTEGER PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    boundary GEOMETRY(Polygon, 4326),
    active BOOLEAN NOT NULL DEFAULT TRUE
);

-- Spatial indexes
CREATE INDEX idx_locations_coordinates ON locations USING GIST(coordinates);
CREATE INDEX idx_delivery_zones_boundary ON delivery_zones USING GIST(boundary);

-- Query: Find nearby restaurants (within 5km radius)
SELECT
    id,
    name,
    category,
    ST_Distance(
        coordinates,
        ST_SetSRID(ST_MakePoint(-122.4194, 37.7749), 4326)::geography
    ) / 1000 AS distance_km
FROM locations
WHERE category = 'restaurant'
    AND ST_DWithin(
        coordinates::geography,
        ST_SetSRID(ST_MakePoint(-122.4194, 37.7749), 4326)::geography,
        5000  -- 5km in meters
    )
ORDER BY distance_km
LIMIT 20;

-- Query: Points within delivery zone
SELECT
    l.id,
    l.name,
    l.coordinates,
    dz.name AS delivery_zone
FROM locations l
JOIN delivery_zones dz ON ST_Within(l.coordinates, dz.boundary)
WHERE dz.active = TRUE
    AND l.category = 'customer_address';

-- Query: Nearest neighbor search
SELECT
    id,
    name,
    category,
    ST_Distance(
        coordinates,
        ST_SetSRID(ST_MakePoint(-122.4194, 37.7749), 4326)::geography
    ) AS distance_meters
FROM locations
WHERE category IN ('hospital', 'pharmacy')
ORDER BY coordinates <-> ST_SetSRID(ST_MakePoint(-122.4194, 37.7749), 4326)
LIMIT 5;

-- Query: Spatial clustering (find dense areas)
WITH location_clusters AS (
    SELECT
        ST_ClusterKMeans(coordinates, 10) OVER() AS cluster_id,
        id,
        name,
        coordinates
    FROM locations
    WHERE category = 'store'
)
SELECT
    cluster_id,
    COUNT(*) AS location_count,
    ST_Centroid(ST_Collect(coordinates)) AS cluster_center,
    ST_ConvexHull(ST_Collect(coordinates)) AS cluster_area
FROM location_clusters
GROUP BY cluster_id
HAVING COUNT(*) >= 3
ORDER BY location_count DESC;
