-- TPC-DS Query 57
-- Monthly catalog sales summary with moving average
WITH v1 AS (
  SELECT
    i_category,
    i_brand,
    cc_name,
    d_year,
    d_moy,
    SUM(cs_sales_price) AS sum_sales,
    AVG(SUM(cs_sales_price)) OVER (
      PARTITION BY i_category, i_brand, cc_name, d_year
      ORDER BY d_moy
      ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING
    ) AS avg_monthly_sales,
    RANK() OVER (
      PARTITION BY i_category, i_brand, cc_name
      ORDER BY d_year, d_moy
    ) AS rn
  FROM item
  JOIN catalog_sales ON cs_item_sk = i_item_sk
  JOIN date_dim ON cs_sold_date_sk = d_date_sk
  JOIN call_center ON cs_call_center_sk = cc_call_center_sk
  WHERE (d_year = 1999 OR (d_year = 1998 AND d_moy = 12) OR (d_year = 2000 AND d_moy = 1))
  GROUP BY i_category, i_brand, cc_name, d_year, d_moy
)
SELECT
  v1.i_category,
  v1.i_brand,
  v1.cc_name,
  v1.d_year,
  v1.d_moy,
  v1.avg_monthly_sales,
  v1.sum_sales,
  v1_lag.sum_sales AS psum,
  v1_lead.sum_sales AS nsum
FROM v1
JOIN v1 v1_lag ON v1.i_category = v1_lag.i_category
  AND v1.i_brand = v1_lag.i_brand
  AND v1.cc_name = v1_lag.cc_name
  AND v1.rn = v1_lag.rn + 1
JOIN v1 v1_lead ON v1.i_category = v1_lead.i_category
  AND v1.i_brand = v1_lead.i_brand
  AND v1.cc_name = v1_lead.cc_name
  AND v1.rn = v1_lead.rn - 1
WHERE v1.d_year = 1999
  AND v1.avg_monthly_sales > 0
  AND CASE WHEN v1.avg_monthly_sales > 0
           THEN ABS(v1.sum_sales - v1.avg_monthly_sales) / v1.avg_monthly_sales
           ELSE NULL END > 0.1
ORDER BY v1.sum_sales - v1.avg_monthly_sales, v1.d_moy
LIMIT 100;
