-- TimescaleDB IoT Sensor Data
-- Source: Time-series databases, IoT platforms
-- Pattern: OLAP - High-volume time-series data

-- Hypertable for sensor readings (partitioned by time)
CREATE TABLE sensor_readings (
    time TIMESTAMP NOT NULL,
    sensor_id INTEGER NOT NULL,
    location_id INTEGER NOT NULL,
    temperature DOUBLE PRECISION,
    humidity DOUBLE PRECISION,
    pressure DOUBLE PRECISION,
    battery_level DOUBLE PRECISION
);

-- TimescaleDB specific: SELECT create_hypertable('sensor_readings', 'time');
CREATE INDEX idx_sensor_readings_sensor_time ON sensor_readings(sensor_id, time DESC);
CREATE INDEX idx_sensor_readings_location_time ON sensor_readings(location_id, time DESC);

-- Query: Recent readings for specific sensor
SELECT
    time,
    sensor_id,
    temperature,
    humidity,
    pressure,
    battery_level
FROM sensor_readings
WHERE sensor_id = 42
    AND time >= NOW() - INTERVAL '1 hour'
ORDER BY time DESC
LIMIT 100;

-- Query: Hourly averages with downsampling
SELECT
    time_bucket('1 hour', time) AS hour,
    sensor_id,
    AVG(temperature) AS avg_temperature,
    MIN(temperature) AS min_temperature,
    MAX(temperature) AS max_temperature,
    STDDEV(temperature) AS stddev_temperature,
    AVG(humidity) AS avg_humidity,
    AVG(pressure) AS avg_pressure,
    COUNT(*) AS reading_count
FROM sensor_readings
WHERE time >= NOW() - INTERVAL '7 days'
    AND location_id = 5
GROUP BY hour, sensor_id
ORDER BY hour DESC, sensor_id;

-- Query: Anomaly detection (values outside 2 stddev)
WITH sensor_stats AS (
    SELECT
        sensor_id,
        AVG(temperature) AS mean_temp,
        STDDEV(temperature) AS stddev_temp
    FROM sensor_readings
    WHERE time >= NOW() - INTERVAL '30 days'
    GROUP BY sensor_id
)
SELECT
    sr.time,
    sr.sensor_id,
    sr.temperature,
    ss.mean_temp,
    ss.stddev_temp,
    ABS(sr.temperature - ss.mean_temp) / NULLIF(ss.stddev_temp, 0) AS z_score
FROM sensor_readings sr
JOIN sensor_stats ss ON sr.sensor_id = ss.sensor_id
WHERE sr.time >= NOW() - INTERVAL '24 hours'
    AND ABS(sr.temperature - ss.mean_temp) > 2 * ss.stddev_temp
ORDER BY sr.time DESC;

-- Query: Gap detection (missing sensor data)
SELECT
    sensor_id,
    time AS gap_start,
    LEAD(time) OVER (PARTITION BY sensor_id ORDER BY time) AS gap_end,
    EXTRACT(EPOCH FROM (
        LEAD(time) OVER (PARTITION BY sensor_id ORDER BY time) - time
    )) / 60 AS gap_minutes
FROM sensor_readings
WHERE time >= NOW() - INTERVAL '24 hours'
QUALIFY gap_minutes > 15  -- Gaps longer than 15 minutes
ORDER BY sensor_id, time;
