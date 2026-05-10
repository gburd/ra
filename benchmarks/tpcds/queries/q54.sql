-- TPC-DS Query 54
-- Count unique customers who purchased items in specific categories
-- from catalog but purchased from store
WITH my_customers AS (
  SELECT DISTINCT c_customer_sk
  FROM catalog_sales
  JOIN date_dim ON cs_sold_date_sk = d_date_sk
  JOIN item ON cs_item_sk = i_item_sk
  JOIN customer ON cs_bill_customer_sk = c_customer_sk
  WHERE i_category IN ('Women', 'Music', 'Men')
    AND d_moy = 1
    AND d_year = 1998
),
my_revenue AS (
  SELECT
    c_customer_sk,
    SUM(ss_ext_sales_price) AS revenue
  FROM my_customers
  JOIN store_sales ON c_customer_sk = ss_customer_sk
  JOIN customer_address ON ss_addr_sk = ca_address_sk
  JOIN store ON ss_store_sk = s_store_sk
  JOIN date_dim ON ss_sold_date_sk = d_date_sk
  WHERE ca_county = s_county
    AND ca_state = s_state
    AND d_month_seq BETWEEN 1200 AND 1200 + 11
  GROUP BY c_customer_sk
)
SELECT
  COUNT(*) AS customer_count,
  segment
FROM (
  SELECT
    c_customer_sk,
    CAST(revenue / 50 AS INTEGER) AS segment
  FROM my_revenue
) segments
GROUP BY segment
ORDER BY segment, customer_count
LIMIT 100;
