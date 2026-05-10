-- TPC-DS Query 84: Find specific customers with specific income in specific cities
-- Report customer demographic details for a specific income band and city
SELECT
    c_customer_id AS customer_id,
    COALESCE(c_last_name, '') || ', ' || COALESCE(c_first_name, '') AS customername
FROM customer
JOIN customer_address ON c_current_addr_sk = ca_address_sk
JOIN customer_demographics ON cd_demo_sk = c_current_cdemo_sk
JOIN household_demographics ON hd_demo_sk = c_current_hdemo_sk
JOIN income_band ON ib_income_band_sk = hd_income_band_sk
WHERE ca_city = 'Edgewood'
    AND ib_lower_bound >= 38128
    AND ib_upper_bound <= 88128
ORDER BY customer_id
LIMIT 100;
