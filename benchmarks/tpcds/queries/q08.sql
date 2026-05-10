-- TPC-DS Query 8
-- Compute the net profit of stores located in geographic regions with
-- customer demographics matching specific criteria.
SELECT s_store_name, SUM(ss_net_profit) AS net_profit
FROM store_sales
JOIN date_dim ON ss_sold_date_sk = d_date_sk
JOIN store ON ss_store_sk = s_store_sk
JOIN customer_address ON ss_addr_sk = ca_address_sk
WHERE d_year = 2001
  AND (
    (ca_street_number BETWEEN '100' AND '500'
     AND ca_address_sk BETWEEN 10000 AND 50000)
    OR
    ca_zip IN (
        SELECT ca_zip
        FROM (
            SELECT SUBSTRING(ca_zip, 1, 5) AS ca_zip
            FROM customer_address
            WHERE ca_address_sk BETWEEN 10000 AND 50000
            GROUP BY ca_zip
            HAVING COUNT(*) > 10
        ) a1
    )
  )
GROUP BY s_store_name
ORDER BY s_store_name
LIMIT 100;
