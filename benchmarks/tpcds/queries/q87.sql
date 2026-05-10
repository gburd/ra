-- TPC-DS Query 87: Count customers from all three channels
-- Count distinct customers who bought from store, catalog, and web
SELECT COUNT(*) AS customer_count
FROM (
    SELECT DISTINCT c_last_name, c_first_name, d_date
    FROM store_sales
    JOIN date_dim ON ss_sold_date_sk = d_date_sk
    JOIN customer ON ss_customer_sk = c_customer_sk
    WHERE d_month_seq BETWEEN 1200 AND 1211

    EXCEPT

    SELECT DISTINCT c_last_name, c_first_name, d_date
    FROM catalog_sales
    JOIN date_dim ON cs_sold_date_sk = d_date_sk
    JOIN customer ON cs_bill_customer_sk = c_customer_sk
    WHERE d_month_seq BETWEEN 1200 AND 1211

    EXCEPT

    SELECT DISTINCT c_last_name, c_first_name, d_date
    FROM web_sales
    JOIN date_dim ON ws_sold_date_sk = d_date_sk
    JOIN customer ON ws_bill_customer_sk = c_customer_sk
    WHERE d_month_seq BETWEEN 1200 AND 1211
) cool_cust;
