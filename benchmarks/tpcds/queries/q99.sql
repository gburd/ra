-- TPC-DS Query 99: Count late shipments by shipping mode and warehouse
-- Report catalog sales shipment delays by shipping mode and warehouse
SELECT
    SUBSTR(w_warehouse_name, 1, 20) AS warehouse_name,
    sm_type AS ship_mode,
    cc_name AS call_center_name,
    SUM(CASE
        WHEN (cs_ship_date_sk - cs_sold_date_sk <= 30) THEN 1
        ELSE 0
    END) AS days_30,
    SUM(CASE
        WHEN (cs_ship_date_sk - cs_sold_date_sk > 30)
            AND (cs_ship_date_sk - cs_sold_date_sk <= 60) THEN 1
        ELSE 0
    END) AS days_31_60,
    SUM(CASE
        WHEN (cs_ship_date_sk - cs_sold_date_sk > 60)
            AND (cs_ship_date_sk - cs_sold_date_sk <= 90) THEN 1
        ELSE 0
    END) AS days_61_90,
    SUM(CASE
        WHEN (cs_ship_date_sk - cs_sold_date_sk > 90)
            AND (cs_ship_date_sk - cs_sold_date_sk <= 120) THEN 1
        ELSE 0
    END) AS days_91_120,
    SUM(CASE
        WHEN (cs_ship_date_sk - cs_sold_date_sk > 120) THEN 1
        ELSE 0
    END) AS days_over_120
FROM catalog_sales
JOIN warehouse ON cs_warehouse_sk = w_warehouse_sk
JOIN ship_mode ON cs_ship_mode_sk = sm_ship_mode_sk
JOIN call_center ON cs_call_center_sk = cc_call_center_sk
JOIN date_dim ON cs_ship_date_sk = d_date_sk
WHERE d_month_seq BETWEEN 1200 AND 1211
GROUP BY
    SUBSTR(w_warehouse_name, 1, 20),
    sm_type,
    cc_name
ORDER BY warehouse_name, ship_mode, call_center_name
LIMIT 100;
