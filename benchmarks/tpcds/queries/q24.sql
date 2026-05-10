-- TPC-DS Query 24
-- Find products that are above average in sales for a given year and state
-- (2 parts combined - part 1).
WITH ssales AS (
    SELECT
        c_last_name,
        c_first_name,
        s_store_name,
        ca_state,
        s_state,
        i_color,
        i_current_price,
        i_manager_id,
        i_units,
        i_size,
        SUM(ss_net_paid) AS netpaid
    FROM store_sales
    JOIN store_returns ON ss_ticket_number = sr_ticket_number
        AND ss_item_sk = sr_item_sk
    JOIN store ON ss_store_sk = s_store_sk
    JOIN item ON ss_item_sk = i_item_sk
    JOIN customer ON ss_customer_sk = c_customer_sk
    JOIN customer_address ON c_current_addr_sk = ca_address_sk
    WHERE c_birth_country = UPPER(ca_country)
      AND s_zip = ca_zip
      AND s_market_id = 8
    GROUP BY c_last_name, c_first_name, s_store_name, ca_state,
             s_state, i_color, i_current_price, i_manager_id,
             i_units, i_size
)
SELECT
    c_last_name,
    c_first_name,
    s_store_name,
    netpaid AS paid
FROM ssales
WHERE i_color = 'peach'
GROUP BY c_last_name, c_first_name, s_store_name, netpaid
HAVING SUM(netpaid) > (
    SELECT 0.05 * AVG(netpaid)
    FROM ssales
)
ORDER BY c_last_name, c_first_name, s_store_name
LIMIT 100;
