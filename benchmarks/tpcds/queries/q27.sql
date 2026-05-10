-- TPC-DS Query 27
-- Compute average quantity, average list price, and total count grouped by store and item
SELECT i_item_id,
       s_state,
       grouping(s_state) AS g_state,
       avg(ss_quantity) AS agg1,
       avg(ss_list_price) AS agg2,
       avg(ss_coupon_amt) AS agg3,
       avg(ss_sales_price) AS agg4
FROM store_sales
JOIN customer_demographics ON ss_cdemo_sk = cd_demo_sk
JOIN date_dim ON ss_sold_date_sk = d_date_sk
JOIN store ON ss_store_sk = s_store_sk
JOIN item ON ss_item_sk = i_item_sk
WHERE cd_gender = 'M'
  AND cd_marital_status = 'S'
  AND cd_education_status = 'College'
  AND d_year = 2002
  AND s_state IN ('TN', 'SD', 'OH', 'NM', 'WI', 'AL')
GROUP BY ROLLUP(i_item_id, s_state)
ORDER BY i_item_id, s_state
LIMIT 100;
