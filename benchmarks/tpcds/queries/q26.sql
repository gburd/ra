-- TPC-DS Query 26
-- Compute average quantity, price, and profit for specified promotion categories
SELECT i_item_id,
       avg(cs_quantity) AS agg1,
       avg(cs_list_price) AS agg2,
       avg(cs_coupon_amt) AS agg3,
       avg(cs_sales_price) AS agg4
FROM catalog_sales
JOIN customer_demographics ON cs_bill_cdemo_sk = cd_demo_sk
JOIN date_dim ON cs_sold_date_sk = d_date_sk
JOIN item ON cs_item_sk = i_item_sk
JOIN promotion ON cs_promo_sk = p_promo_sk
WHERE cd_gender = 'M'
  AND cd_marital_status = 'S'
  AND cd_education_status = 'College'
  AND (p_channel_email = 'N' OR p_channel_event = 'N')
  AND d_year = 2000
GROUP BY i_item_id
ORDER BY i_item_id
LIMIT 100;
