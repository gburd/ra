-- TPC-DS Query 68
-- Compare customer purchase patterns in stores within certain demographics
SELECT
  c_last_name,
  c_first_name,
  ca_city,
  bought_city,
  ss_ticket_number,
  extended_price,
  extended_tax,
  list_price
FROM (
  SELECT
    ss_ticket_number,
    ss_customer_sk,
    ca_city AS bought_city,
    SUM(ss_ext_sales_price) AS extended_price,
    SUM(ss_ext_list_price) AS list_price,
    SUM(ss_ext_tax) AS extended_tax
  FROM store_sales
  JOIN date_dim ON ss_sold_date_sk = d_date_sk
  JOIN store ON ss_store_sk = s_store_sk
  JOIN household_demographics ON ss_hdemo_sk = hd_demo_sk
  JOIN customer_address ON ss_addr_sk = ca_address_sk
  WHERE store_sales.ss_sold_date_sk = date_dim.d_date_sk
    AND d_dom BETWEEN 1 AND 2
    AND (hd_dep_count = 4 OR hd_vehicle_count = 3)
    AND d_year IN (1999, 1999 + 1, 1999 + 2)
    AND s_city IN ('Fairview', 'Midway')
  GROUP BY ss_ticket_number, ss_customer_sk, ss_addr_sk, ca_city
) dn
JOIN customer ON ss_customer_sk = c_customer_sk
JOIN customer_address current_addr ON c_current_addr_sk = current_addr.ca_address_sk
WHERE current_addr.ca_city <> bought_city
ORDER BY c_last_name, ss_ticket_number
LIMIT 100;
