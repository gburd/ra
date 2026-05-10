-- TPC-DS Query 94: Count web orders fulfilled from another warehouse (no returns)
-- Find web orders where the ship warehouse differs from the bill warehouse
-- and the order was never returned
SELECT
    COUNT(DISTINCT ws_order_number) AS order_count,
    SUM(ws_ext_ship_cost) AS total_shipping_cost,
    SUM(ws_net_profit) AS total_net_profit
FROM web_sales ws1
JOIN date_dim ON ws1.ws_ship_date_sk = d_date_sk
JOIN customer_address ON ws1.ws_ship_addr_sk = ca_address_sk
JOIN web_site ON ws1.ws_web_site_sk = web_site_sk
WHERE d_date BETWEEN '1999-02-01' AND (DATE '1999-02-01' + INTERVAL '60 days')
    AND ca_state = 'IL'
    AND web_company_name = 'pri'
    AND EXISTS (
        SELECT 1
        FROM web_sales ws2
        WHERE ws2.ws_order_number = ws1.ws_order_number
            AND ws2.ws_warehouse_sk <> ws1.ws_warehouse_sk
    )
    AND NOT EXISTS (
        SELECT 1
        FROM web_returns wr1
        WHERE wr1.wr_order_number = ws1.ws_order_number
    )
ORDER BY order_count
LIMIT 100;
