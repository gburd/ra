-- Airflow ETL Pipeline Queries
-- Source: Data pipeline orchestration (Airflow, Prefect)
-- Pattern: OLAP - Batch processing and data transformation

-- Extract: Incremental load from source
SELECT
    user_id,
    event_type,
    event_timestamp,
    event_properties,
    session_id,
    device_type,
    geo_country,
    geo_city
FROM raw_events
WHERE event_timestamp >= '{{ ds }}' -- Airflow template variable
    AND event_timestamp < '{{ next_ds }}'
    AND event_type IN ('page_view', 'button_click', 'form_submit', 'purchase');

-- Transform: User session aggregation
WITH session_events AS (
    SELECT
        session_id,
        user_id,
        MIN(event_timestamp) AS session_start,
        MAX(event_timestamp) AS session_end,
        COUNT(*) AS event_count,
        COUNT(DISTINCT event_type) AS unique_event_types,
        ARRAY_AGG(event_type ORDER BY event_timestamp) AS event_sequence,
        MAX(CASE WHEN event_type = 'purchase' THEN 1 ELSE 0 END) AS has_purchase
    FROM raw_events
    WHERE event_timestamp >= '{{ ds }}'
        AND event_timestamp < '{{ next_ds }}'
    GROUP BY session_id, user_id
),

session_metrics AS (
    SELECT
        session_id,
        user_id,
        session_start,
        session_end,
        EXTRACT(EPOCH FROM (session_end - session_start)) AS session_duration_seconds,
        event_count,
        unique_event_types,
        event_sequence,
        has_purchase,
        -- Session quality score
        CASE
            WHEN has_purchase = 1 THEN 10
            WHEN event_count >= 10 AND unique_event_types >= 3 THEN 7
            WHEN event_count >= 5 THEN 5
            ELSE 3
        END AS session_quality_score
    FROM session_events
)

SELECT
    session_id,
    user_id,
    session_start,
    session_end,
    session_duration_seconds,
    event_count,
    unique_event_types,
    has_purchase,
    session_quality_score,
    CASE
        WHEN session_duration_seconds < 30 THEN 'bounce'
        WHEN session_duration_seconds < 300 THEN 'short'
        WHEN session_duration_seconds < 1800 THEN 'medium'
        ELSE 'long'
    END AS session_length_category
FROM session_metrics;

-- Load: Upsert into target table (merge pattern)
MERGE INTO user_daily_summary AS target
USING (
    SELECT
        user_id,
        DATE(event_timestamp) AS activity_date,
        COUNT(*) AS total_events,
        COUNT(DISTINCT session_id) AS session_count,
        SUM(CASE WHEN event_type = 'page_view' THEN 1 ELSE 0 END) AS page_views,
        SUM(CASE WHEN event_type = 'purchase' THEN 1 ELSE 0 END) AS purchases,
        SUM(CASE WHEN event_type = 'purchase' THEN
            CAST(event_properties->>'amount' AS DECIMAL)
            ELSE 0
        END) AS total_revenue
    FROM raw_events
    WHERE event_timestamp >= '{{ ds }}'
        AND event_timestamp < '{{ next_ds }}'
    GROUP BY user_id, DATE(event_timestamp)
) AS source
ON target.user_id = source.user_id
    AND target.activity_date = source.activity_date
WHEN MATCHED THEN
    UPDATE SET
        total_events = source.total_events,
        session_count = source.session_count,
        page_views = source.page_views,
        purchases = source.purchases,
        total_revenue = source.total_revenue,
        updated_at = CURRENT_TIMESTAMP
WHEN NOT MATCHED THEN
    INSERT (user_id, activity_date, total_events, session_count,
            page_views, purchases, total_revenue, created_at, updated_at)
    VALUES (source.user_id, source.activity_date, source.total_events,
            source.session_count, source.page_views, source.purchases,
            source.total_revenue, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP);
