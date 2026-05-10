-- TPC-DS Query 64
-- Find items with comparable store and catalog sales revenue
-- (detailed cross-channel analysis)
WITH cs_ui AS (
  SELECT
    cs_item_sk,
    SUM(cs_ext_list_price) AS sale,
    SUM(cr_refunded_cash + cr_reversed_charge + cr_store_credit) AS refund
  FROM catalog_sales
  JOIN catalog_returns ON cs_item_sk = cr_item_sk
    AND cs_order_number = cr_order_number
  GROUP BY cs_item_sk
  HAVING SUM(cs_ext_list_price) > 2 * (SUM(cr_refunded_cash + cr_reversed_charge + cr_store_credit))
),
cross_sales AS (
  SELECT
    i_product_name AS product_name,
    i_item_sk AS item_sk,
    s_store_name AS store_name,
    s_zip AS store_zip,
    ad1.ca_street_number AS b_street_number,
    ad1.ca_street_name AS b_street_name,
    ad1.ca_city AS b_city,
    ad1.ca_zip AS b_zip,
    ad2.ca_street_number AS c_street_number,
    ad2.ca_street_name AS c_street_name,
    ad2.ca_city AS c_city,
    ad2.ca_zip AS c_zip,
    d1.d_year AS syear,
    d1.d_year AS fsyear,
    d2.d_year AS s2year,
    COUNT(*) AS cnt,
    SUM(ss_wholesale_cost) AS s1,
    SUM(ss_list_price) AS s2,
    SUM(ss_coupon_amt) AS s3
  FROM store_sales
  JOIN store_returns ON ss_item_sk = sr_item_sk AND ss_ticket_number = sr_ticket_number
  JOIN cs_ui ON ss_item_sk = cs_item_sk
  JOIN customer ON ss_customer_sk = c_customer_sk
  JOIN date_dim d1 ON ss_sold_date_sk = d1.d_date_sk
  JOIN date_dim d2 ON sr_returned_date_sk = d2.d_date_sk
  JOIN store ON s_store_sk = ss_store_sk
  JOIN item ON i_item_sk = ss_item_sk
  JOIN customer_address ad1 ON c_current_addr_sk = ad1.ca_address_sk
  JOIN customer_address ad2 ON ss_addr_sk = ad2.ca_address_sk
  WHERE d1.d_year = 2000
    AND i_color IN ('maroon', 'burnished', 'dim', 'steel', 'navajo', 'chocolate')
    AND i_current_price BETWEEN 22 AND 22 + 30
    AND i_current_price BETWEEN 22 + 1 AND 22 + 15
  GROUP BY
    i_product_name, i_item_sk, s_store_name, s_zip,
    ad1.ca_street_number, ad1.ca_street_name, ad1.ca_city, ad1.ca_zip,
    ad2.ca_street_number, ad2.ca_street_name, ad2.ca_city, ad2.ca_zip,
    d1.d_year, d2.d_year
)
SELECT
  cs1.product_name,
  cs1.store_name,
  cs1.store_zip,
  cs1.b_street_number,
  cs1.b_street_name,
  cs1.b_city,
  cs1.b_zip,
  cs1.c_street_number,
  cs1.c_street_name,
  cs1.c_city,
  cs1.c_zip,
  cs1.syear,
  cs1.cnt,
  cs1.s1 AS s11,
  cs1.s2 AS s21,
  cs1.s3 AS s31,
  cs2.s1 AS s12,
  cs2.s2 AS s22,
  cs2.s3 AS s32
FROM cross_sales cs1
JOIN cross_sales cs2 ON cs1.item_sk = cs2.item_sk
  AND cs1.store_name = cs2.store_name
  AND cs1.store_zip = cs2.store_zip
WHERE cs1.syear = 2000
  AND cs2.syear = 2000 + 1
  AND cs2.cnt <= cs1.cnt
  AND cs1.s1 * 1.0 / cs1.s2 * 1.0 / cs1.s3 IS NOT NULL
ORDER BY
  cs1.product_name, cs1.store_name, cs2.cnt, cs1.s1, cs2.s1
LIMIT 100;
