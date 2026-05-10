-- TPC-DS Query 65
-- Find stores with revenue above average for specific dates
SELECT
  s_store_name,
  i_item_desc,
  sc.revenue,
  i_current_price,
  i_wholesale_cost,
  i_brand
FROM store
JOIN (
  SELECT
    ss_store_sk,
    ss_item_sk,
    SUM(ss_sales_price) AS revenue
  FROM store_sales
  JOIN date_dim ON ss_sold_date_sk = d_date_sk
  WHERE d_month_seq BETWEEN 1176 AND 1176 + 11
  GROUP BY ss_store_sk, ss_item_sk
) sc ON s_store_sk = sc.ss_store_sk
JOIN item ON sc.ss_item_sk = i_item_sk
JOIN (
  SELECT
    ss_store_sk,
    AVG(revenue) AS ave
  FROM (
    SELECT
      ss_store_sk,
      ss_item_sk,
      SUM(ss_sales_price) AS revenue
    FROM store_sales
    JOIN date_dim ON ss_sold_date_sk = d_date_sk
    WHERE d_month_seq BETWEEN 1176 AND 1176 + 11
    GROUP BY ss_store_sk, ss_item_sk
  ) sa
  GROUP BY ss_store_sk
) sb ON sc.ss_store_sk = sb.ss_store_sk
WHERE sc.revenue <= 0.1 * sb.ave
ORDER BY s_store_name, i_item_desc
LIMIT 100;
