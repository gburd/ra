-- TPC-DS Query 73
-- Count customers who visited stores more than average
SELECT
  c_last_name,
  c_first_name,
  c_salutation,
  c_preferred_cust_flag,
  ss_ticket_number,
  cnt
FROM (
  SELECT
    ss_ticket_number,
    ss_customer_sk,
    COUNT(*) AS cnt
  FROM store_sales
  JOIN date_dim ON ss_sold_date_sk = d_date_sk
  JOIN store ON ss_store_sk = s_store_sk
  JOIN household_demographics ON ss_hdemo_sk = hd_demo_sk
  WHERE (d_dom BETWEEN 1 AND 2)
    AND (hd_buy_potential = '>10000' OR hd_buy_potential = 'Unknown')
    AND hd_vehicle_count > 0
    AND CASE WHEN hd_vehicle_count > 0
             THEN hd_dep_count / hd_vehicle_count
             ELSE NULL END > 1
    AND d_year IN (1999, 1999 + 1, 1999 + 2)
    AND s_county IN ('Williamson County', 'Franklin Parish', 'Bronx County', 'Orange County')
  GROUP BY ss_ticket_number, ss_customer_sk
) dj
JOIN customer ON ss_customer_sk = c_customer_sk
WHERE cnt BETWEEN 1 AND 5
ORDER BY cnt DESC, c_last_name ASC
LIMIT 100;
