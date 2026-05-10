-- TPC-DS Query 35
-- Find customers with demographics in specific states who bought from all three channels
SELECT ca_state,
       cd_gender,
       cd_marital_status,
       cd_dep_count,
       count(*) AS cnt1,
       min(cd_dep_count) AS min1,
       max(cd_dep_count) AS max1,
       avg(cd_dep_count) AS avg1,
       cd_dep_employed_count,
       count(*) AS cnt2,
       min(cd_dep_employed_count) AS min2,
       max(cd_dep_employed_count) AS max2,
       avg(cd_dep_employed_count) AS avg2,
       cd_dep_college_count,
       count(*) AS cnt3,
       min(cd_dep_college_count) AS min3,
       max(cd_dep_college_count) AS max3,
       avg(cd_dep_college_count) AS avg3
FROM customer c
JOIN customer_address ca ON c.c_current_addr_sk = ca.ca_address_sk
JOIN customer_demographics ON cd_demo_sk = c.c_current_cdemo_sk
WHERE ca_state IN ('KS', 'IN', 'SD', 'OH', 'LA', 'MO')
  AND EXISTS (SELECT 1
              FROM store_sales
              JOIN date_dim ON ss_sold_date_sk = d_date_sk
              WHERE c.c_customer_sk = ss_customer_sk
                AND d_year = 2002
                AND d_qoy < 4)
  AND (EXISTS (SELECT 1
               FROM web_sales
               JOIN date_dim ON ws_sold_date_sk = d_date_sk
               WHERE c.c_customer_sk = ws_bill_customer_sk
                 AND d_year = 2002
                 AND d_qoy < 4)
       OR EXISTS (SELECT 1
                  FROM catalog_sales
                  JOIN date_dim ON cs_sold_date_sk = d_date_sk
                  WHERE c.c_customer_sk = cs_ship_customer_sk
                    AND d_year = 2002
                    AND d_qoy < 4))
GROUP BY ca_state, cd_gender, cd_marital_status, cd_dep_count,
         cd_dep_employed_count, cd_dep_college_count
ORDER BY ca_state, cd_gender, cd_marital_status, cd_dep_count,
         cd_dep_employed_count, cd_dep_college_count
LIMIT 100;
