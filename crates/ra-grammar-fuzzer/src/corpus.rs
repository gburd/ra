//! Hand-crafted SQL corpus covering all major Postgres usage patterns.
//!
//! The corpus is organized into named categories.  Each entry is a
//! `(category, sql)` pair.  Call [`all_queries()`] to get the full set.
//!
//! ## Benchmark Naming
//!
//! We use `HammerDB`'s TPROC-? naming convention for benchmarks:
//! - **TPROC-H**: OLAP queries (based on TPC-H specification)
//! - **TPROC-C**: OLTP queries (based on TPC-C specification)
//!
//! TPC-H, TPC-C, and TPC-DS are trademarks of the Transaction Processing
//! Performance Council. `HammerDB` provides open-source implementations under
//! the TPROC-? names to avoid trademark issues.

/// One corpus entry: a category tag and a SQL string.
#[derive(Debug, Clone)]
pub struct CorpusEntry {
    /// Short category label (e.g. `"simple_crud"`, `"tpch"`).
    pub category: &'static str,
    /// The SQL query string.
    pub sql: &'static str,
}

/// Return the full query corpus as a slice of [`CorpusEntry`] values.
#[must_use]
pub fn all_queries() -> Vec<CorpusEntry> {
    let mut out = Vec::with_capacity(260);
    out.extend_from_slice(SIMPLE_CRUD);
    out.extend_from_slice(ANALYTICS);
    out.extend_from_slice(MULTI_TABLE_JOINS);
    out.extend_from_slice(CTES);
    out.extend_from_slice(SUBQUERIES);
    out.extend_from_slice(JSONB);
    out.extend_from_slice(TPCH);
    out.extend_from_slice(TPCDS);
    out.extend_from_slice(EDGE_CASES);
    out
}

// ---------------------------------------------------------------------------
// Simple CRUD (20)
// ---------------------------------------------------------------------------

static SIMPLE_CRUD: &[CorpusEntry] = &[
    CorpusEntry { category: "simple_crud",
        sql: "SELECT * FROM orders" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT o_orderkey, o_totalprice FROM orders" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT * FROM orders WHERE o_orderstatus = 'O'" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT * FROM orders WHERE o_totalprice > 10000" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT * FROM orders WHERE o_orderdate >= '1995-01-01'" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT * FROM customer WHERE c_mktsegment = 'BUILDING'" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT c_custkey, c_name FROM customer ORDER BY c_name" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT * FROM lineitem WHERE l_quantity > 20" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT * FROM part WHERE p_size BETWEEN 1 AND 10" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT * FROM supplier WHERE s_acctbal < 0" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT * FROM nation ORDER BY n_name" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT * FROM region" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT DISTINCT o_orderstatus FROM orders" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT o_orderkey FROM orders WHERE o_shippriority = 1" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT * FROM lineitem WHERE l_returnflag = 'R' AND l_linestatus = 'F'" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT * FROM orders LIMIT 100" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT * FROM orders LIMIT 50 OFFSET 100" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT COUNT(*) FROM orders" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT MIN(o_totalprice), MAX(o_totalprice) FROM orders" },
    CorpusEntry { category: "simple_crud",
        sql: "SELECT AVG(l_quantity) FROM lineitem" },
];

// ---------------------------------------------------------------------------
// Analytics (25)
// ---------------------------------------------------------------------------

static ANALYTICS: &[CorpusEntry] = &[
    CorpusEntry { category: "analytics",
        sql: "SELECT o_orderstatus, COUNT(*) FROM orders GROUP BY o_orderstatus" },
    CorpusEntry { category: "analytics",
        sql: "SELECT o_orderstatus, SUM(o_totalprice) FROM orders GROUP BY o_orderstatus" },
    CorpusEntry { category: "analytics",
        sql: "SELECT o_orderstatus, AVG(o_totalprice) FROM orders GROUP BY o_orderstatus" },
    CorpusEntry { category: "analytics",
        sql: "SELECT l_returnflag, l_linestatus, SUM(l_quantity), \
              SUM(l_extendedprice), COUNT(*) \
              FROM lineitem \
              GROUP BY l_returnflag, l_linestatus \
              ORDER BY l_returnflag, l_linestatus" },
    CorpusEntry { category: "analytics",
        sql: "SELECT o_orderstatus, COUNT(*) FROM orders \
              GROUP BY o_orderstatus HAVING COUNT(*) > 1000" },
    CorpusEntry { category: "analytics",
        sql: "SELECT p_type, COUNT(*), AVG(p_retailprice) \
              FROM part GROUP BY p_type ORDER BY COUNT(*) DESC" },
    CorpusEntry { category: "analytics",
        sql: "SELECT n_name, COUNT(*) FROM supplier \
              JOIN nation ON s_nationkey = n_nationkey \
              GROUP BY n_name ORDER BY COUNT(*) DESC" },
    CorpusEntry { category: "analytics",
        sql: "SELECT \
                row_number() OVER (ORDER BY o_totalprice DESC) AS rn, \
                o_orderkey, o_totalprice \
              FROM orders" },
    CorpusEntry { category: "analytics",
        sql: "SELECT o_custkey, \
                SUM(o_totalprice) OVER (PARTITION BY o_custkey) AS cust_total \
              FROM orders" },
    CorpusEntry { category: "analytics",
        sql: "SELECT o_orderdate, \
                AVG(o_totalprice) OVER (ORDER BY o_orderdate \
                    ROWS BETWEEN 6 PRECEDING AND CURRENT ROW) AS moving_avg \
              FROM orders" },
    CorpusEntry { category: "analytics",
        sql: "SELECT rank() OVER (PARTITION BY o_orderstatus ORDER BY o_totalprice DESC), \
                o_orderkey \
              FROM orders" },
    CorpusEntry { category: "analytics",
        sql: "SELECT dense_rank() OVER (ORDER BY o_totalprice DESC) AS dr, \
                o_orderkey FROM orders LIMIT 10" },
    CorpusEntry { category: "analytics",
        sql: "SELECT \
                l_shipdate, \
                SUM(l_extendedprice * (1 - l_discount)) OVER \
                    (ORDER BY l_shipdate) AS running_revenue \
              FROM lineitem" },
    CorpusEntry { category: "analytics",
        sql: "SELECT p_mfgr, p_brand, SUM(p_retailprice) \
              FROM part GROUP BY ROLLUP(p_mfgr, p_brand)" },
    CorpusEntry { category: "analytics",
        sql: "SELECT p_mfgr, p_type, COUNT(*) \
              FROM part GROUP BY CUBE(p_mfgr, p_type)" },
    CorpusEntry { category: "analytics",
        sql: "SELECT l_shipmode, SUM(l_extendedprice) \
              FROM lineitem GROUP BY l_shipmode \
              ORDER BY SUM(l_extendedprice) DESC" },
    CorpusEntry { category: "analytics",
        sql: "SELECT o_orderstatus, o_orderpriority, COUNT(*), SUM(o_totalprice) \
              FROM orders GROUP BY GROUPING SETS \
              ((o_orderstatus, o_orderpriority), (o_orderstatus), ())" },
    CorpusEntry { category: "analytics",
        sql: "SELECT l_returnflag, \
                SUM(l_extendedprice * (1 - l_discount)) AS revenue, \
                AVG(l_discount) AS avg_disc \
              FROM lineitem GROUP BY l_returnflag" },
    CorpusEntry { category: "analytics",
        sql: "SELECT ntile(4) OVER (ORDER BY o_totalprice) AS quartile, \
                o_orderkey, o_totalprice \
              FROM orders" },
    CorpusEntry { category: "analytics",
        sql: "SELECT percent_rank() OVER (ORDER BY o_totalprice) AS pct, \
                o_orderkey FROM orders" },
    CorpusEntry { category: "analytics",
        sql: "SELECT lag(o_totalprice, 1) OVER (ORDER BY o_orderdate) AS prev_price, \
                o_orderkey FROM orders" },
    CorpusEntry { category: "analytics",
        sql: "SELECT lead(o_totalprice) OVER (ORDER BY o_orderdate) AS next_price, \
                o_orderkey FROM orders" },
    CorpusEntry { category: "analytics",
        sql: "SELECT first_value(o_totalprice) OVER \
                (PARTITION BY o_custkey ORDER BY o_orderdate) AS first_order, \
                o_orderkey FROM orders" },
    CorpusEntry { category: "analytics",
        sql: "SELECT l_partkey, SUM(l_quantity) FROM lineitem \
              GROUP BY l_partkey HAVING SUM(l_quantity) > 300" },
    CorpusEntry { category: "analytics",
        sql: "SELECT o_orderpriority, COUNT(DISTINCT o_custkey) \
              FROM orders GROUP BY o_orderpriority" },
];

// ---------------------------------------------------------------------------
// Multi-table joins (20)
// ---------------------------------------------------------------------------

static MULTI_TABLE_JOINS: &[CorpusEntry] = &[
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT o_orderkey, c_name \
              FROM orders JOIN customer ON o_custkey = c_custkey" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT l_orderkey, o_orderdate, l_extendedprice \
              FROM lineitem JOIN orders ON l_orderkey = o_orderkey" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT c_name, o_orderkey, l_partkey \
              FROM customer \
              JOIN orders ON c_custkey = o_custkey \
              JOIN lineitem ON o_orderkey = l_orderkey" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT p_name, l_quantity \
              FROM part JOIN lineitem ON p_partkey = l_partkey" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT s_name, n_name \
              FROM supplier JOIN nation ON s_nationkey = n_nationkey" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT n_name, r_name \
              FROM nation JOIN region ON n_regionkey = r_regionkey" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT ps_partkey, ps_suppkey, p_name, s_name \
              FROM partsupp \
              JOIN part ON ps_partkey = p_partkey \
              JOIN supplier ON ps_suppkey = s_suppkey" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT c_name, n_name, r_name \
              FROM customer \
              JOIN nation ON c_nationkey = n_nationkey \
              JOIN region ON n_regionkey = r_regionkey" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT * FROM orders LEFT JOIN customer ON o_custkey = c_custkey \
              WHERE c_custkey IS NULL" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT * FROM customer LEFT JOIN orders ON c_custkey = o_custkey \
              WHERE o_orderkey IS NULL" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT c_name, COUNT(o_orderkey) \
              FROM customer LEFT JOIN orders ON c_custkey = o_custkey \
              GROUP BY c_name" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT l_orderkey, SUM(l_extendedprice * (1 - l_discount)) AS revenue \
              FROM lineitem \
              JOIN orders ON l_orderkey = o_orderkey \
              WHERE o_orderdate >= '1994-01-01' \
              GROUP BY l_orderkey" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT n1.n_name AS supplier_nation, n2.n_name AS customer_nation, \
                SUM(l_extendedprice * (1 - l_discount)) AS revenue \
              FROM supplier \
              JOIN lineitem ON s_suppkey = l_suppkey \
              JOIN orders ON l_orderkey = o_orderkey \
              JOIN customer ON o_custkey = c_custkey \
              JOIN nation n1 ON s_nationkey = n1.n_nationkey \
              JOIN nation n2 ON c_nationkey = n2.n_nationkey \
              GROUP BY n1.n_name, n2.n_name" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT * FROM orders CROSS JOIN region" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT o.o_orderkey, c.c_name \
              FROM orders o JOIN customer c ON o.o_custkey = c.c_custkey \
              WHERE c.c_mktsegment = 'AUTOMOBILE'" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT DISTINCT l_partkey \
              FROM lineitem \
              JOIN orders ON l_orderkey = o_orderkey \
              WHERE o_orderstatus = 'F'" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT p_partkey, AVG(ps_supplycost) \
              FROM partsupp JOIN part ON ps_partkey = p_partkey \
              GROUP BY p_partkey" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT r_name, COUNT(DISTINCT n_nationkey) \
              FROM region JOIN nation ON r_regionkey = n_regionkey \
              GROUP BY r_name" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT s_name, SUM(ps_availqty) \
              FROM supplier JOIN partsupp ON s_suppkey = ps_suppkey \
              GROUP BY s_name HAVING SUM(ps_availqty) > 1000" },
    CorpusEntry { category: "multi_table_joins",
        sql: "SELECT s1.s_name, s2.s_name \
              FROM supplier s1 JOIN supplier s2 \
              ON s1.s_nationkey = s2.s_nationkey AND s1.s_suppkey < s2.s_suppkey" },
];

// ---------------------------------------------------------------------------
// CTEs (15)
// ---------------------------------------------------------------------------

static CTES: &[CorpusEntry] = &[
    CorpusEntry { category: "ctes",
        sql: "WITH big_orders AS (\
                SELECT * FROM orders WHERE o_totalprice > 100000\
              ) SELECT COUNT(*) FROM big_orders" },
    CorpusEntry { category: "ctes",
        sql: "WITH top_customers AS (\
                SELECT c_custkey, SUM(o_totalprice) AS total \
                FROM customer JOIN orders ON c_custkey = o_custkey \
                GROUP BY c_custkey\
              ) SELECT c_custkey FROM top_customers WHERE total > 500000" },
    CorpusEntry { category: "ctes",
        sql: "WITH ranked AS (\
                SELECT o_orderkey, \
                  rank() OVER (ORDER BY o_totalprice DESC) AS rk \
                FROM orders\
              ) SELECT o_orderkey FROM ranked WHERE rk <= 10" },
    CorpusEntry { category: "ctes",
        sql: "WITH a AS (SELECT * FROM orders WHERE o_orderstatus = 'O'), \
                   b AS (SELECT * FROM orders WHERE o_orderstatus = 'F') \
              SELECT COUNT(*) FROM a UNION ALL SELECT COUNT(*) FROM b" },
    CorpusEntry { category: "ctes",
        sql: "WITH nation_stats AS (\
                SELECT n_regionkey, COUNT(*) AS nation_count \
                FROM nation GROUP BY n_regionkey\
              ), region_stats AS (\
                SELECT r_regionkey, r_name FROM region\
              ) SELECT r_name, nation_count \
                FROM region_stats JOIN nation_stats \
                ON r_regionkey = n_regionkey" },
    CorpusEntry { category: "ctes",
        sql: "WITH RECURSIVE subordinates(id, level) AS (\
                SELECT 1, 0 \
                UNION ALL \
                SELECT id + 1, level + 1 FROM subordinates WHERE level < 5\
              ) SELECT * FROM subordinates" },
    CorpusEntry { category: "ctes",
        sql: "WITH revenue AS (\
                SELECT l_suppkey, SUM(l_extendedprice * (1 - l_discount)) AS total \
                FROM lineitem GROUP BY l_suppkey\
              ) SELECT * FROM revenue WHERE total = (SELECT MAX(total) FROM revenue)" },
    CorpusEntry { category: "ctes",
        sql: "WITH filtered_parts AS (\
                SELECT * FROM part WHERE p_size >= 4 AND p_size <= 8\
              ) SELECT COUNT(*) FROM filtered_parts" },
    CorpusEntry { category: "ctes",
        sql: "WITH cust_orders AS (\
                SELECT c_custkey, COUNT(o_orderkey) AS order_count \
                FROM customer LEFT JOIN orders ON c_custkey = o_custkey \
                GROUP BY c_custkey\
              ) SELECT AVG(order_count) FROM cust_orders" },
    CorpusEntry { category: "ctes",
        sql: "WITH active AS (\
                SELECT * FROM supplier WHERE s_acctbal >= 0\
              ) SELECT n_name, COUNT(*) \
                FROM active JOIN nation ON s_nationkey = n_nationkey \
                GROUP BY n_name" },
    CorpusEntry { category: "ctes",
        sql: "WITH monthly AS (\
                SELECT DATE_TRUNC('month', o_orderdate) AS month, \
                  SUM(o_totalprice) AS revenue \
                FROM orders GROUP BY DATE_TRUNC('month', o_orderdate)\
              ) SELECT month, revenue FROM monthly ORDER BY month" },
    CorpusEntry { category: "ctes",
        sql: "WITH expensive AS (\
                SELECT p_partkey, p_retailprice FROM part \
                WHERE p_retailprice > 1000\
              ) SELECT ps_suppkey, COUNT(*) \
                FROM partsupp JOIN expensive ON ps_partkey = expensive.p_partkey \
                GROUP BY ps_suppkey" },
    CorpusEntry { category: "ctes",
        sql: "WITH t AS (SELECT 1 AS x UNION ALL SELECT 2 UNION ALL SELECT 3) \
              SELECT SUM(x) FROM t" },
    CorpusEntry { category: "ctes",
        sql: "WITH RECURSIVE series(n) AS (\
                SELECT 1 \
                UNION ALL \
                SELECT n + 1 FROM series WHERE n < 10\
              ) SELECT SUM(n) FROM series" },
    CorpusEntry { category: "ctes",
        sql: "WITH big AS (\
                SELECT * FROM lineitem WHERE l_extendedprice > 50000\
              ) SELECT l_returnflag, COUNT(*) FROM big GROUP BY l_returnflag" },
];

// ---------------------------------------------------------------------------
// Subqueries (15)
// ---------------------------------------------------------------------------

static SUBQUERIES: &[CorpusEntry] = &[
    CorpusEntry { category: "subqueries",
        sql: "SELECT * FROM orders \
              WHERE o_custkey IN (SELECT c_custkey FROM customer \
              WHERE c_mktsegment = 'BUILDING')" },
    CorpusEntry { category: "subqueries",
        sql: "SELECT * FROM orders \
              WHERE o_custkey NOT IN (SELECT c_custkey FROM customer \
              WHERE c_acctbal < 0)" },
    CorpusEntry { category: "subqueries",
        sql: "SELECT * FROM orders WHERE EXISTS (\
                SELECT 1 FROM lineitem WHERE l_orderkey = o_orderkey\
              )" },
    CorpusEntry { category: "subqueries",
        sql: "SELECT * FROM supplier WHERE NOT EXISTS (\
                SELECT 1 FROM lineitem WHERE l_suppkey = s_suppkey\
              )" },
    CorpusEntry { category: "subqueries",
        sql: "SELECT * FROM orders \
              WHERE o_totalprice > (SELECT AVG(o_totalprice) FROM orders)" },
    CorpusEntry { category: "subqueries",
        sql: "SELECT * FROM part \
              WHERE p_retailprice = (SELECT MAX(p_retailprice) FROM part)" },
    CorpusEntry { category: "subqueries",
        sql: "SELECT o_orderkey, \
                (SELECT COUNT(*) FROM lineitem WHERE l_orderkey = o_orderkey) AS line_count \
              FROM orders" },
    CorpusEntry { category: "subqueries",
        sql: "SELECT * FROM supplier s \
              WHERE s_acctbal > (SELECT AVG(s_acctbal) FROM supplier \
              WHERE s_nationkey = s.s_nationkey)" },
    CorpusEntry { category: "subqueries",
        sql: "SELECT * FROM lineitem \
              WHERE l_orderkey IN (\
                SELECT o_orderkey FROM orders WHERE o_orderstatus = 'F'\
              ) AND l_returnflag = 'R'" },
    CorpusEntry { category: "subqueries",
        sql: "SELECT c_name, \
                (SELECT SUM(o_totalprice) FROM orders WHERE o_custkey = c_custkey) AS total \
              FROM customer" },
    CorpusEntry { category: "subqueries",
        sql: "SELECT * FROM orders o1 WHERE o_totalprice > ALL (\
                SELECT o_totalprice FROM orders o2 WHERE o2.o_custkey = o1.o_custkey \
                AND o2.o_orderkey <> o1.o_orderkey\
              )" },
    CorpusEntry { category: "subqueries",
        sql: "SELECT * FROM part WHERE p_partkey IN (\
                SELECT l_partkey FROM lineitem WHERE l_quantity > 30\
              )" },
    CorpusEntry { category: "subqueries",
        sql: "SELECT n_name, (\
                SELECT COUNT(*) FROM supplier WHERE s_nationkey = n_nationkey\
              ) AS supplier_count FROM nation" },
    CorpusEntry { category: "subqueries",
        sql: "SELECT * FROM customer WHERE c_custkey IN (\
                SELECT o_custkey FROM orders \
                WHERE o_orderdate BETWEEN '1993-10-01' AND '1994-01-01'\
              )" },
    CorpusEntry { category: "subqueries",
        sql: "SELECT * FROM lineitem \
              WHERE (l_partkey, l_suppkey) IN (\
                SELECT ps_partkey, ps_suppkey FROM partsupp \
                WHERE ps_availqty > 500\
              )" },
];

// ---------------------------------------------------------------------------
// JSONB (10)
// ---------------------------------------------------------------------------

static JSONB: &[CorpusEntry] = &[
    CorpusEntry { category: "jsonb",
        sql: "SELECT data->>'name' FROM orders WHERE data IS NOT NULL" },
    CorpusEntry { category: "jsonb",
        sql: "SELECT data->'address'->>'city' FROM customer WHERE data IS NOT NULL" },
    CorpusEntry { category: "jsonb",
        sql: "SELECT * FROM orders WHERE data @> '{\"status\": \"shipped\"}'" },
    CorpusEntry { category: "jsonb",
        sql: "SELECT * FROM orders WHERE data ? 'tracking_number'" },
    CorpusEntry { category: "jsonb",
        sql: "SELECT data#>>'{address,city}' FROM customer WHERE data IS NOT NULL" },
    CorpusEntry { category: "jsonb",
        sql: "SELECT * FROM orders WHERE data @? '$.items[*].qty > 5'" },
    CorpusEntry { category: "jsonb",
        sql: "SELECT * FROM orders WHERE data @@ '$.status == \"pending\"'" },
    CorpusEntry { category: "jsonb",
        sql: "SELECT data->>'total' AS total FROM orders \
              WHERE (data->>'total')::numeric > 1000" },
    CorpusEntry { category: "jsonb",
        sql: "SELECT * FROM orders WHERE data ?| ARRAY['status', 'tracking']" },
    CorpusEntry { category: "jsonb",
        sql: "SELECT * FROM orders WHERE data ?& ARRAY['id', 'total']" },
];

// ---------------------------------------------------------------------------
// TPROC-H (HammerDB OLAP benchmark, 22 queries)
// Based on TPC-H spec but using HammerDB naming
// ---------------------------------------------------------------------------

static TPCH: &[CorpusEntry] = &[
    // Q1
    CorpusEntry { category: "tpch",
        sql: "SELECT l_returnflag, l_linestatus, \
                SUM(l_quantity) AS sum_qty, \
                SUM(l_extendedprice) AS sum_base_price, \
                SUM(l_extendedprice * (1 - l_discount)) AS sum_disc_price, \
                SUM(l_extendedprice * (1 - l_discount) * (1 + l_tax)) AS sum_charge, \
                AVG(l_quantity) AS avg_qty, \
                AVG(l_extendedprice) AS avg_price, \
                AVG(l_discount) AS avg_disc, \
                COUNT(*) AS count_order \
              FROM lineitem \
              WHERE l_shipdate <= '1998-09-02' \
              GROUP BY l_returnflag, l_linestatus \
              ORDER BY l_returnflag, l_linestatus" },
    // Q2
    CorpusEntry { category: "tpch",
        sql: "SELECT s_acctbal, s_name, n_name, p_partkey, p_mfgr, \
                s_address, s_phone, s_comment \
              FROM part, supplier, partsupp, nation, region \
              WHERE p_partkey = ps_partkey \
                AND s_suppkey = ps_suppkey \
                AND p_size = 15 \
                AND p_type LIKE '%BRASS' \
                AND s_nationkey = n_nationkey \
                AND n_regionkey = r_regionkey \
                AND r_name = 'EUROPE' \
                AND ps_supplycost = (\
                  SELECT MIN(ps_supplycost) \
                  FROM partsupp, supplier, nation, region \
                  WHERE p_partkey = ps_partkey \
                    AND s_suppkey = ps_suppkey \
                    AND s_nationkey = n_nationkey \
                    AND n_regionkey = r_regionkey \
                    AND r_name = 'EUROPE'\
                ) \
              ORDER BY s_acctbal DESC, n_name, s_name, p_partkey \
              LIMIT 100" },
    // Q3
    CorpusEntry { category: "tpch",
        sql: "SELECT l_orderkey, \
                SUM(l_extendedprice * (1 - l_discount)) AS revenue, \
                o_orderdate, o_shippriority \
              FROM customer, orders, lineitem \
              WHERE c_mktsegment = 'BUILDING' \
                AND c_custkey = o_custkey \
                AND l_orderkey = o_orderkey \
                AND o_orderdate < '1995-03-15' \
                AND l_shipdate > '1995-03-15' \
              GROUP BY l_orderkey, o_orderdate, o_shippriority \
              ORDER BY revenue DESC, o_orderdate \
              LIMIT 10" },
    // Q4
    CorpusEntry { category: "tpch",
        sql: "SELECT o_orderpriority, COUNT(*) AS order_count \
              FROM orders \
              WHERE o_orderdate >= '1993-07-01' \
                AND o_orderdate < '1993-10-01' \
                AND EXISTS (\
                  SELECT * FROM lineitem \
                  WHERE l_orderkey = o_orderkey \
                    AND l_commitdate < l_receiptdate\
                ) \
              GROUP BY o_orderpriority ORDER BY o_orderpriority" },
    // Q5
    CorpusEntry { category: "tpch",
        sql: "SELECT n_name, SUM(l_extendedprice * (1 - l_discount)) AS revenue \
              FROM customer, orders, lineitem, supplier, nation, region \
              WHERE c_custkey = o_custkey \
                AND l_orderkey = o_orderkey \
                AND l_suppkey = s_suppkey \
                AND c_nationkey = s_nationkey \
                AND s_nationkey = n_nationkey \
                AND n_regionkey = r_regionkey \
                AND r_name = 'ASIA' \
                AND o_orderdate >= '1994-01-01' \
                AND o_orderdate < '1995-01-01' \
              GROUP BY n_name ORDER BY revenue DESC" },
    // Q6
    CorpusEntry { category: "tpch",
        sql: "SELECT SUM(l_extendedprice * l_discount) AS revenue \
              FROM lineitem \
              WHERE l_shipdate >= '1994-01-01' \
                AND l_shipdate < '1995-01-01' \
                AND l_discount BETWEEN 0.05 AND 0.07 \
                AND l_quantity < 24" },
    // Q7
    CorpusEntry { category: "tpch",
        sql: "SELECT supp_nation, cust_nation, l_year, \
                SUM(volume) AS revenue \
              FROM (\
                SELECT n1.n_name AS supp_nation, n2.n_name AS cust_nation, \
                  EXTRACT(YEAR FROM l_shipdate) AS l_year, \
                  l_extendedprice * (1 - l_discount) AS volume \
                FROM supplier, lineitem, orders, customer, \
                     nation n1, nation n2 \
                WHERE s_suppkey = l_suppkey \
                  AND o_orderkey = l_orderkey \
                  AND c_custkey = o_custkey \
                  AND s_nationkey = n1.n_nationkey \
                  AND c_nationkey = n2.n_nationkey \
                  AND ((n1.n_name = 'FRANCE' AND n2.n_name = 'GERMANY') \
                    OR (n1.n_name = 'GERMANY' AND n2.n_name = 'FRANCE')) \
                  AND l_shipdate BETWEEN '1995-01-01' AND '1996-12-31'\
              ) AS shipping \
              GROUP BY supp_nation, cust_nation, l_year \
              ORDER BY supp_nation, cust_nation, l_year" },
    // Q8
    CorpusEntry { category: "tpch",
        sql: "SELECT o_year, \
                SUM(CASE WHEN nation = 'BRAZIL' THEN volume ELSE 0 END) / \
                SUM(volume) AS mkt_share \
              FROM (\
                SELECT EXTRACT(YEAR FROM o_orderdate) AS o_year, \
                  l_extendedprice * (1 - l_discount) AS volume, \
                  n2.n_name AS nation \
                FROM part, supplier, lineitem, orders, customer, \
                     nation n1, nation n2, region \
                WHERE p_partkey = l_partkey \
                  AND s_suppkey = l_suppkey \
                  AND l_orderkey = o_orderkey \
                  AND o_custkey = c_custkey \
                  AND c_nationkey = n1.n_nationkey \
                  AND n1.n_regionkey = r_regionkey \
                  AND r_name = 'AMERICA' \
                  AND s_nationkey = n2.n_nationkey \
                  AND o_orderdate BETWEEN '1995-01-01' AND '1996-12-31' \
                  AND p_type = 'ECONOMY ANODIZED STEEL'\
              ) AS all_nations \
              GROUP BY o_year ORDER BY o_year" },
    // Q9
    CorpusEntry { category: "tpch",
        sql: "SELECT nation, o_year, SUM(amount) AS sum_profit \
              FROM (\
                SELECT n_name AS nation, \
                  EXTRACT(YEAR FROM o_orderdate) AS o_year, \
                  l_extendedprice * (1 - l_discount) - ps_supplycost * l_quantity AS amount \
                FROM part, supplier, lineitem, partsupp, orders, nation \
                WHERE s_suppkey = l_suppkey \
                  AND ps_suppkey = l_suppkey \
                  AND ps_partkey = l_partkey \
                  AND p_partkey = l_partkey \
                  AND o_orderkey = l_orderkey \
                  AND s_nationkey = n_nationkey \
                  AND p_name LIKE '%green%'\
              ) AS profit \
              GROUP BY nation, o_year ORDER BY nation, o_year DESC" },
    // Q10
    CorpusEntry { category: "tpch",
        sql: "SELECT c_custkey, c_name, \
                SUM(l_extendedprice * (1 - l_discount)) AS revenue, \
                c_acctbal, n_name, c_address, c_phone, c_comment \
              FROM customer, orders, lineitem, nation \
              WHERE c_custkey = o_custkey \
                AND l_orderkey = o_orderkey \
                AND o_orderdate >= '1993-10-01' \
                AND o_orderdate < '1994-01-01' \
                AND l_returnflag = 'R' \
                AND c_nationkey = n_nationkey \
              GROUP BY c_custkey, c_name, c_acctbal, c_phone, n_name, \
                       c_address, c_comment \
              ORDER BY revenue DESC LIMIT 20" },
    // Q11
    CorpusEntry { category: "tpch",
        sql: "SELECT ps_partkey, SUM(ps_supplycost * ps_availqty) AS val \
              FROM partsupp, supplier, nation \
              WHERE ps_suppkey = s_suppkey \
                AND s_nationkey = n_nationkey \
                AND n_name = 'GERMANY' \
              GROUP BY ps_partkey \
              HAVING SUM(ps_supplycost * ps_availqty) > (\
                SELECT SUM(ps_supplycost * ps_availqty) * 0.0001 \
                FROM partsupp, supplier, nation \
                WHERE ps_suppkey = s_suppkey \
                  AND s_nationkey = n_nationkey \
                  AND n_name = 'GERMANY'\
              ) \
              ORDER BY val DESC" },
    // Q12
    CorpusEntry { category: "tpch",
        sql: "SELECT l_shipmode, \
                SUM(CASE WHEN o_orderpriority = '1-URGENT' OR o_orderpriority = '2-HIGH' \
                         THEN 1 ELSE 0 END) AS high_line_count, \
                SUM(CASE WHEN o_orderpriority <> '1-URGENT' AND o_orderpriority <> '2-HIGH' \
                         THEN 1 ELSE 0 END) AS low_line_count \
              FROM orders, lineitem \
              WHERE o_orderkey = l_orderkey \
                AND l_shipmode IN ('MAIL', 'SHIP') \
                AND l_commitdate < l_receiptdate \
                AND l_shipdate < l_commitdate \
                AND l_receiptdate >= '1994-01-01' \
                AND l_receiptdate < '1995-01-01' \
              GROUP BY l_shipmode ORDER BY l_shipmode" },
    // Q13
    CorpusEntry { category: "tpch",
        sql: "SELECT c_count, COUNT(*) AS custdist \
              FROM (\
                SELECT c_custkey, COUNT(o_orderkey) AS c_count \
                FROM customer LEFT OUTER JOIN orders \
                ON c_custkey = o_custkey \
                  AND o_comment NOT LIKE '%special%requests%' \
                GROUP BY c_custkey\
              ) AS c_orders \
              GROUP BY c_count ORDER BY custdist DESC, c_count DESC" },
    // Q14
    CorpusEntry { category: "tpch",
        sql: "SELECT 100.00 * SUM(CASE WHEN p_type LIKE 'PROMO%' \
                THEN l_extendedprice * (1 - l_discount) \
                ELSE 0 END) / \
                SUM(l_extendedprice * (1 - l_discount)) AS promo_revenue \
              FROM lineitem, part \
              WHERE l_partkey = p_partkey \
                AND l_shipdate >= '1995-09-01' \
                AND l_shipdate < '1995-10-01'" },
    // Q15
    CorpusEntry { category: "tpch",
        sql: "WITH revenue AS (\
                SELECT l_suppkey, SUM(l_extendedprice * (1 - l_discount)) AS total_revenue \
                FROM lineitem \
                WHERE l_shipdate >= '1996-01-01' AND l_shipdate < '1996-04-01' \
                GROUP BY l_suppkey\
              ) \
              SELECT s_suppkey, s_name, s_address, s_phone, total_revenue \
              FROM supplier, revenue \
              WHERE s_suppkey = l_suppkey \
                AND total_revenue = (SELECT MAX(total_revenue) FROM revenue) \
              ORDER BY s_suppkey" },
    // Q16
    CorpusEntry { category: "tpch",
        sql: "SELECT p_brand, p_type, p_size, COUNT(DISTINCT ps_suppkey) AS supplier_cnt \
              FROM partsupp, part \
              WHERE p_partkey = ps_partkey \
                AND p_brand <> 'Brand#45' \
                AND p_type NOT LIKE 'MEDIUM POLISHED%' \
                AND p_size IN (49, 14, 23, 45, 19, 3, 36, 9) \
                AND ps_suppkey NOT IN (\
                  SELECT s_suppkey FROM supplier \
                  WHERE s_comment LIKE '%Customer%Complaints%'\
                ) \
              GROUP BY p_brand, p_type, p_size \
              ORDER BY supplier_cnt DESC, p_brand, p_type, p_size" },
    // Q17
    CorpusEntry { category: "tpch",
        sql: "SELECT SUM(l_extendedprice) / 7.0 AS avg_yearly \
              FROM lineitem, part \
              WHERE p_partkey = l_partkey \
                AND p_brand = 'Brand#23' \
                AND p_container = 'MED BOX' \
                AND l_quantity < (\
                  SELECT 0.2 * AVG(l_quantity) \
                  FROM lineitem WHERE l_partkey = p_partkey\
                )" },
    // Q18
    CorpusEntry { category: "tpch",
        sql: "SELECT c_name, c_custkey, o_orderkey, o_orderdate, \
                o_totalprice, SUM(l_quantity) \
              FROM customer, orders, lineitem \
              WHERE o_orderkey IN (\
                SELECT l_orderkey FROM lineitem \
                GROUP BY l_orderkey HAVING SUM(l_quantity) > 300\
              ) \
                AND c_custkey = o_custkey \
                AND o_orderkey = l_orderkey \
              GROUP BY c_name, c_custkey, o_orderkey, o_orderdate, o_totalprice \
              ORDER BY o_totalprice DESC, o_orderdate \
              LIMIT 100" },
    // Q19
    CorpusEntry { category: "tpch",
        sql: "SELECT SUM(l_extendedprice * (1 - l_discount)) AS revenue \
              FROM lineitem, part \
              WHERE p_partkey = l_partkey \
                AND ((\
                  p_brand = 'Brand#12' \
                  AND p_container IN ('SM CASE', 'SM BOX', 'SM PACK', 'SM PKG') \
                  AND l_quantity >= 1 AND l_quantity <= 11 \
                  AND p_size BETWEEN 1 AND 5 \
                  AND l_shipmode IN ('AIR', 'AIR REG')\
                ) OR (\
                  p_brand = 'Brand#23' \
                  AND p_container IN ('MED BAG', 'MED BOX', 'MED PKG', 'MED PACK') \
                  AND l_quantity >= 10 AND l_quantity <= 20 \
                  AND p_size BETWEEN 1 AND 10 \
                  AND l_shipmode IN ('AIR', 'AIR REG')\
                ) OR (\
                  p_brand = 'Brand#34' \
                  AND p_container IN ('LG CASE', 'LG BOX', 'LG PACK', 'LG PKG') \
                  AND l_quantity >= 20 AND l_quantity <= 30 \
                  AND p_size BETWEEN 1 AND 15 \
                  AND l_shipmode IN ('AIR', 'AIR REG')\
                ))" },
    // Q20
    CorpusEntry { category: "tpch",
        sql: "SELECT s_name, s_address FROM supplier, nation \
              WHERE s_suppkey IN (\
                SELECT ps_suppkey FROM partsupp \
                WHERE ps_partkey IN (\
                  SELECT p_partkey FROM part WHERE p_name LIKE 'forest%'\
                ) \
                AND ps_availqty > (\
                  SELECT 0.5 * SUM(l_quantity) \
                  FROM lineitem \
                  WHERE l_partkey = ps_partkey \
                    AND l_suppkey = ps_suppkey \
                    AND l_shipdate >= '1994-01-01' \
                    AND l_shipdate < '1995-01-01'\
                )\
              ) \
                AND s_nationkey = n_nationkey \
                AND n_name = 'CANADA' \
              ORDER BY s_name" },
    // Q21
    CorpusEntry { category: "tpch",
        sql: "SELECT s_name, COUNT(*) AS numwait \
              FROM supplier, lineitem l1, orders, nation \
              WHERE s_suppkey = l1.l_suppkey \
                AND o_orderkey = l1.l_orderkey \
                AND o_orderstatus = 'F' \
                AND l1.l_receiptdate > l1.l_commitdate \
                AND EXISTS (\
                  SELECT * FROM lineitem l2 \
                  WHERE l2.l_orderkey = l1.l_orderkey \
                    AND l2.l_suppkey <> l1.l_suppkey\
                ) \
                AND NOT EXISTS (\
                  SELECT * FROM lineitem l3 \
                  WHERE l3.l_orderkey = l1.l_orderkey \
                    AND l3.l_suppkey <> l1.l_suppkey \
                    AND l3.l_receiptdate > l3.l_commitdate\
                ) \
                AND s_nationkey = n_nationkey \
                AND n_name = 'SAUDI ARABIA' \
              GROUP BY s_name ORDER BY numwait DESC, s_name \
              LIMIT 100" },
    // Q22
    CorpusEntry { category: "tpch",
        sql: "SELECT cntrycode, COUNT(*) AS numcust, SUM(c_acctbal) AS totacctbal \
              FROM (\
                SELECT SUBSTRING(c_phone FROM 1 FOR 2) AS cntrycode, c_acctbal \
                FROM customer \
                WHERE SUBSTRING(c_phone FROM 1 FOR 2) IN \
                  ('13', '31', '23', '29', '30', '18', '17') \
                  AND c_acctbal > (\
                    SELECT AVG(c_acctbal) FROM customer \
                    WHERE c_acctbal > 0.00 \
                      AND SUBSTRING(c_phone FROM 1 FOR 2) IN \
                        ('13', '31', '23', '29', '30', '18', '17')\
                  ) \
                  AND NOT EXISTS (\
                    SELECT * FROM orders WHERE o_custkey = c_custkey\
                  )\
              ) AS custsale \
              GROUP BY cntrycode ORDER BY cntrycode" },
];

// ---------------------------------------------------------------------------
// Edge cases (15)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// TPC-DS (representative subset — 20 queries from the 99-query benchmark)
// ---------------------------------------------------------------------------

static TPCDS: &[CorpusEntry] = &[
    // Q3: Total extended sales price per item brand
    CorpusEntry { category: "tpcds",
        sql: "SELECT dt.d_year, item.i_brand_id, item.i_brand, SUM(ss_ext_sales_price) as sum_agg \
              FROM date_dim dt, store_sales, item \
              WHERE dt.d_date_sk = store_sales.ss_sold_date_sk \
              AND store_sales.ss_item_sk = item.i_item_sk \
              AND item.i_manufact_id = 128 \
              AND dt.d_moy = 11 \
              GROUP BY dt.d_year, item.i_brand_id, item.i_brand \
              ORDER BY dt.d_year, sum_agg DESC, item.i_brand_id \
              LIMIT 100" },
    // Q6: Customers buying items with prices higher than average for their state
    CorpusEntry { category: "tpcds",
        sql: "SELECT a.ca_state, COUNT(*) as cnt \
              FROM customer_address a, customer c, store_sales s, date_dim d, item i \
              WHERE a.ca_address_sk = c.c_current_addr_sk \
              AND c.c_customer_sk = s.ss_customer_sk \
              AND s.ss_sold_date_sk = d.d_date_sk \
              AND s.ss_item_sk = i.i_item_sk \
              AND d.d_month_seq BETWEEN 1200 AND 1211 \
              AND i.i_current_price > 1.2 * (SELECT AVG(j.i_current_price) FROM item j \
                  WHERE j.i_category = i.i_category) \
              GROUP BY a.ca_state \
              HAVING COUNT(*) >= 10 \
              ORDER BY cnt LIMIT 100" },
    // Q7: Average quantity/price/discount for promotional items
    CorpusEntry { category: "tpcds",
        sql: "SELECT i_item_id, AVG(ss_quantity) as agg1, AVG(ss_list_price) as agg2, \
              AVG(ss_coupon_amt) as agg3, AVG(ss_sales_price) as agg4 \
              FROM store_sales, customer_demographics, date_dim, item, promotion \
              WHERE ss_sold_date_sk = d_date_sk AND ss_item_sk = i_item_sk \
              AND ss_cdemo_sk = cd_demo_sk AND ss_promo_sk = p_promo_sk \
              AND cd_gender = 'M' AND cd_marital_status = 'S' \
              AND cd_education_status = 'College' AND (p_channel_email = 'N' OR p_channel_event = 'N') \
              AND d_year = 2000 \
              GROUP BY i_item_id ORDER BY i_item_id LIMIT 100" },
    // Q13: Store sales analysis with demographics
    CorpusEntry { category: "tpcds",
        sql: "SELECT AVG(ss_quantity) as avg_qty, AVG(ss_ext_sales_price) as avg_sp, \
              AVG(ss_ext_wholesale_cost) as avg_wc, SUM(ss_ext_wholesale_cost) as sum_wc \
              FROM store_sales, store, customer_demographics, household_demographics, \
              customer_address, date_dim \
              WHERE s_store_sk = ss_store_sk AND ss_sold_date_sk = d_date_sk \
              AND ss_cdemo_sk = cd_demo_sk AND ss_hdemo_sk = hd_demo_sk \
              AND ss_addr_sk = ca_address_sk AND d_year = 2001 \
              AND cd_marital_status = 'M' AND cd_education_status = 'Advanced Degree' \
              AND hd_dep_count = 3" },
    // Q19: Revenue by item manufacturer and brand
    CorpusEntry { category: "tpcds",
        sql: "SELECT i_brand_id, i_brand, i_manufact_id, i_manufact, SUM(ss_ext_sales_price) as ext_price \
              FROM date_dim, store_sales, item, customer, customer_address, store \
              WHERE d_date_sk = ss_sold_date_sk AND ss_item_sk = i_item_sk \
              AND ss_customer_sk = c_customer_sk AND c_current_addr_sk = ca_address_sk \
              AND ss_store_sk = s_store_sk AND d_moy = 11 AND d_year = 1998 \
              AND ca_gmt_offset = -5 AND s_gmt_offset = -5 \
              GROUP BY i_brand_id, i_brand, i_manufact_id, i_manufact \
              ORDER BY ext_price DESC, i_brand, i_brand_id, i_manufact_id, i_manufact \
              LIMIT 100" },
    // Q25: Quantity sold from store and catalog for specific items
    CorpusEntry { category: "tpcds",
        sql: "SELECT i_item_id, i_item_desc, s_store_id, s_store_name, \
              SUM(ss_net_profit) as store_sales_profit, \
              SUM(sr_net_loss) as store_returns_loss, \
              SUM(cs_net_profit) as catalog_sales_profit \
              FROM store_sales, store_returns, catalog_sales, date_dim d1, date_dim d2, \
              date_dim d3, store, item \
              WHERE d1.d_moy = 4 AND d1.d_year = 2001 \
              AND d1.d_date_sk = ss_sold_date_sk \
              AND i_item_sk = ss_item_sk \
              AND s_store_sk = ss_store_sk \
              AND ss_customer_sk = sr_customer_sk AND ss_item_sk = sr_item_sk \
              AND ss_ticket_number = sr_ticket_number \
              AND sr_returned_date_sk = d2.d_date_sk \
              AND d2.d_moy BETWEEN 4 AND 10 AND d2.d_year = 2001 \
              AND sr_customer_sk = cs_bill_customer_sk AND sr_item_sk = cs_item_sk \
              AND cs_sold_date_sk = d3.d_date_sk \
              AND d3.d_moy BETWEEN 4 AND 10 AND d3.d_year = 2001 \
              GROUP BY i_item_id, i_item_desc, s_store_id, s_store_name \
              ORDER BY i_item_id, i_item_desc, s_store_id, s_store_name \
              LIMIT 100" },
    // Q34: Customers buying specific items more than 4 times
    CorpusEntry { category: "tpcds",
        sql: "SELECT c_last_name, c_first_name, c_salutation, c_preferred_cust_flag, \
              ss_ticket_number, cnt \
              FROM (SELECT ss_ticket_number, ss_customer_sk, COUNT(*) as cnt \
                    FROM store_sales, date_dim, store, household_demographics \
                    WHERE store_sales.ss_sold_date_sk = date_dim.d_date_sk \
                    AND store_sales.ss_store_sk = store.s_store_sk \
                    AND store_sales.ss_hdemo_sk = household_demographics.hd_demo_sk \
                    AND (date_dim.d_dom BETWEEN 1 AND 3 OR date_dim.d_dom BETWEEN 25 AND 28) \
                    AND (household_demographics.hd_buy_potential = '>10000' \
                         OR household_demographics.hd_buy_potential = '5001-10000') \
                    AND household_demographics.hd_vehicle_count > 0 \
                    AND (CASE WHEN household_demographics.hd_vehicle_count > 0 \
                         THEN household_demographics.hd_dep_count / household_demographics.hd_vehicle_count \
                         ELSE NULL END) > 1.2 \
                    AND date_dim.d_year IN (1999, 2000, 2001) \
                    AND store.s_county IN ('Williamson County', 'Walker County') \
                    GROUP BY ss_ticket_number, ss_customer_sk) dn, customer \
              WHERE ss_customer_sk = c_customer_sk AND cnt BETWEEN 15 AND 20 \
              ORDER BY c_last_name, c_first_name, c_salutation, c_preferred_cust_flag DESC" },
    // Q42: Year-to-year comparison of quarterly sales
    CorpusEntry { category: "tpcds",
        sql: "SELECT dt.d_year, item.i_category_id, item.i_category, SUM(ss_ext_sales_price) as s \
              FROM date_dim dt, store_sales, item \
              WHERE dt.d_date_sk = store_sales.ss_sold_date_sk \
              AND store_sales.ss_item_sk = item.i_item_sk \
              AND item.i_manager_id = 1 AND dt.d_moy = 11 AND dt.d_year = 2000 \
              GROUP BY dt.d_year, item.i_category_id, item.i_category \
              ORDER BY s DESC, dt.d_year, item.i_category_id, item.i_category \
              LIMIT 100" },
    // Q46: Per-customer store sales for specific counties
    CorpusEntry { category: "tpcds",
        sql: "SELECT c_last_name, c_first_name, ca_city, bought_city, ss_ticket_number, \
              amt, profit \
              FROM (SELECT ss_ticket_number, ss_customer_sk, ca_city as bought_city, \
                    SUM(ss_coupon_amt) as amt, SUM(ss_net_profit) as profit \
                    FROM store_sales, date_dim, store, household_demographics, customer_address \
                    WHERE store_sales.ss_sold_date_sk = date_dim.d_date_sk \
                    AND store_sales.ss_store_sk = store.s_store_sk \
                    AND store_sales.ss_hdemo_sk = household_demographics.hd_demo_sk \
                    AND store_sales.ss_addr_sk = customer_address.ca_address_sk \
                    AND (household_demographics.hd_dep_count = 4 \
                         OR household_demographics.hd_vehicle_count = 3) \
                    AND date_dim.d_dow IN (6, 0) \
                    AND date_dim.d_year IN (1999, 2000, 2001) \
                    AND store.s_city IN ('Midway', 'Fairview', 'Fairview', 'Midway', 'Fairview') \
                    GROUP BY ss_ticket_number, ss_customer_sk, ss_addr_sk, ca_city) dn, \
              customer, customer_address current_addr \
              WHERE ss_customer_sk = c_customer_sk \
              AND customer.c_current_addr_sk = current_addr.ca_address_sk \
              AND current_addr.ca_city <> bought_city \
              ORDER BY c_last_name, c_first_name, ca_city, bought_city, ss_ticket_number \
              LIMIT 100" },
    // Q52: Revenue breakdown for specific departments
    CorpusEntry { category: "tpcds",
        sql: "SELECT dt.d_year, item.i_brand_id, item.i_brand, SUM(ss_ext_sales_price) as ext_price \
              FROM date_dim dt, store_sales, item \
              WHERE dt.d_date_sk = store_sales.ss_sold_date_sk \
              AND store_sales.ss_item_sk = item.i_item_sk \
              AND item.i_manager_id = 1 AND dt.d_moy = 11 AND dt.d_year = 2000 \
              GROUP BY dt.d_year, item.i_brand_id, item.i_brand \
              ORDER BY dt.d_year, ext_price DESC, item.i_brand_id \
              LIMIT 100" },
    // Q55: Revenue by brand for specific months
    CorpusEntry { category: "tpcds",
        sql: "SELECT i_brand_id, i_brand, SUM(ss_ext_sales_price) as ext_price \
              FROM date_dim, store_sales, item \
              WHERE d_date_sk = ss_sold_date_sk AND ss_item_sk = i_item_sk \
              AND i_manager_id = 28 AND d_moy = 11 AND d_year = 1999 \
              GROUP BY i_brand_id, i_brand \
              ORDER BY ext_price DESC, i_brand_id LIMIT 100" },
    // Q65: Stores with revenue above average
    CorpusEntry { category: "tpcds",
        sql: "SELECT s_store_name, i_item_desc, sc.revenue, i_current_price, i_wholesale_cost, \
              i_brand \
              FROM store, item, \
              (SELECT ss_store_sk, ss_item_sk, SUM(ss_sales_price) as revenue \
               FROM store_sales, date_dim \
               WHERE ss_sold_date_sk = d_date_sk AND d_month_seq BETWEEN 1176 AND 1187 \
               GROUP BY ss_store_sk, ss_item_sk) sc \
              WHERE ss_store_sk = s_store_sk AND ss_item_sk = i_item_sk \
              AND sc.revenue <= 0.1 * \
                  (SELECT AVG(revenue) FROM \
                   (SELECT ss_store_sk, AVG(ss_sales_price) as revenue \
                    FROM store_sales, date_dim \
                    WHERE ss_sold_date_sk = d_date_sk AND d_month_seq BETWEEN 1176 AND 1187 \
                    GROUP BY ss_store_sk) sa) \
              ORDER BY s_store_name, i_item_desc LIMIT 100" },
    // Q73: Customers who visited stores more than average
    CorpusEntry { category: "tpcds",
        sql: "SELECT c_last_name, c_first_name, c_salutation, c_preferred_cust_flag, \
              ss_ticket_number, cnt \
              FROM (SELECT ss_ticket_number, ss_customer_sk, COUNT(*) as cnt \
                    FROM store_sales, date_dim, store, household_demographics \
                    WHERE store_sales.ss_sold_date_sk = date_dim.d_date_sk \
                    AND store_sales.ss_store_sk = store.s_store_sk \
                    AND store_sales.ss_hdemo_sk = household_demographics.hd_demo_sk \
                    AND (date_dim.d_dom BETWEEN 1 AND 3 OR date_dim.d_dom BETWEEN 25 AND 28) \
                    AND (household_demographics.hd_buy_potential = '>10000' \
                         OR household_demographics.hd_buy_potential = '5001-10000') \
                    AND household_demographics.hd_vehicle_count > 0 \
                    AND date_dim.d_year IN (1999, 2000, 2001) \
                    AND store.s_county IN ('Williamson County') \
                    GROUP BY ss_ticket_number, ss_customer_sk) dn, customer \
              WHERE ss_customer_sk = c_customer_sk AND cnt BETWEEN 1 AND 5 \
              ORDER BY cnt DESC" },
    // Q79: Annual income of store customers
    CorpusEntry { category: "tpcds",
        sql: "SELECT c_last_name, c_first_name, SUBSTRING(s_city FROM 1 FOR 30) as city, \
              ss_ticket_number, amt, profit \
              FROM (SELECT ss_ticket_number, ss_customer_sk, store.s_city, \
                    SUM(ss_coupon_amt) as amt, SUM(ss_net_profit) as profit \
                    FROM store_sales, date_dim, store, household_demographics \
                    WHERE store_sales.ss_sold_date_sk = date_dim.d_date_sk \
                    AND store_sales.ss_store_sk = store.s_store_sk \
                    AND store_sales.ss_hdemo_sk = household_demographics.hd_demo_sk \
                    AND (household_demographics.hd_dep_count = 6 \
                         OR household_demographics.hd_vehicle_count > 2) \
                    AND date_dim.d_dow = 1 \
                    AND date_dim.d_year IN (1999, 2000, 2001) \
                    AND store.s_number_employees BETWEEN 200 AND 295 \
                    GROUP BY ss_ticket_number, ss_customer_sk, ss_addr_sk, store.s_city) ms, \
              customer \
              WHERE ss_customer_sk = c_customer_sk \
              ORDER BY c_last_name, c_first_name, city, profit LIMIT 100" },
    // Q88: Store sales by hourly ranges
    CorpusEntry { category: "tpcds",
        sql: "SELECT * FROM \
              (SELECT COUNT(*) as h8_30_to_9 FROM store_sales, household_demographics, time_dim, store \
               WHERE ss_sold_time_sk = time_dim.t_time_sk AND ss_hdemo_sk = household_demographics.hd_demo_sk \
               AND ss_store_sk = s_store_sk AND time_dim.t_hour = 8 AND time_dim.t_minute >= 30 \
               AND household_demographics.hd_dep_count = 4 AND store.s_store_name = 'ese') s1, \
              (SELECT COUNT(*) as h9_to_9_30 FROM store_sales, household_demographics, time_dim, store \
               WHERE ss_sold_time_sk = time_dim.t_time_sk AND ss_hdemo_sk = household_demographics.hd_demo_sk \
               AND ss_store_sk = s_store_sk AND time_dim.t_hour = 9 AND time_dim.t_minute < 30 \
               AND household_demographics.hd_dep_count = 4 AND store.s_store_name = 'ese') s2" },
    // Q89: Revenue by items and categories
    CorpusEntry { category: "tpcds",
        sql: "SELECT * FROM \
              (SELECT i_category, i_class, i_brand, s_store_name, s_company_name, \
               d_moy, SUM(ss_sales_price) as sum_sales, \
               AVG(SUM(ss_sales_price)) OVER (PARTITION BY i_category, i_brand, s_store_name, s_company_name) as avg_monthly_sales \
               FROM item, store_sales, date_dim, store \
               WHERE ss_item_sk = i_item_sk AND ss_sold_date_sk = d_date_sk \
               AND ss_store_sk = s_store_sk \
               AND d_year IN (1999) AND i_category IN ('Books', 'Electronics', 'Sports') \
               GROUP BY i_category, i_class, i_brand, s_store_name, s_company_name, d_moy) tmp1 \
              WHERE CASE WHEN avg_monthly_sales <> 0 \
                    THEN ABS(sum_sales - avg_monthly_sales) / avg_monthly_sales \
                    ELSE NULL END > 0.1 \
              ORDER BY sum_sales - avg_monthly_sales, s_store_name \
              LIMIT 100" },
    // Q96: Morning store sales count
    CorpusEntry { category: "tpcds",
        sql: "SELECT COUNT(*) \
              FROM store_sales, household_demographics, time_dim, store \
              WHERE ss_sold_time_sk = time_dim.t_time_sk \
              AND ss_hdemo_sk = household_demographics.hd_demo_sk \
              AND ss_store_sk = s_store_sk \
              AND time_dim.t_hour = 20 AND time_dim.t_minute >= 30 \
              AND household_demographics.hd_dep_count = 7 \
              AND store.s_store_name = 'ese' \
              ORDER BY COUNT(*) LIMIT 100" },
    // Q97: Distinct item-customer combinations
    CorpusEntry { category: "tpcds",
        sql: "WITH ssci AS (SELECT ss_customer_sk as customer_sk, ss_item_sk as item_sk \
                            FROM store_sales, date_dim \
                            WHERE ss_sold_date_sk = d_date_sk AND d_month_seq BETWEEN 1200 AND 1211 \
                            GROUP BY ss_customer_sk, ss_item_sk), \
              csci AS (SELECT cs_bill_customer_sk as customer_sk, cs_item_sk as item_sk \
                       FROM catalog_sales, date_dim \
                       WHERE cs_sold_date_sk = d_date_sk AND d_month_seq BETWEEN 1200 AND 1211 \
                       GROUP BY cs_bill_customer_sk, cs_item_sk) \
              SELECT SUM(CASE WHEN ssci.customer_sk IS NOT NULL AND csci.customer_sk IS NULL THEN 1 ELSE 0 END) as store_only, \
                     SUM(CASE WHEN ssci.customer_sk IS NULL AND csci.customer_sk IS NOT NULL THEN 1 ELSE 0 END) as catalog_only, \
                     SUM(CASE WHEN ssci.customer_sk IS NOT NULL AND csci.customer_sk IS NOT NULL THEN 1 ELSE 0 END) as store_and_catalog \
              FROM ssci FULL OUTER JOIN csci ON ssci.customer_sk = csci.customer_sk AND ssci.item_sk = csci.item_sk \
              LIMIT 100" },
    // Q98: Revenue ranking by department
    CorpusEntry { category: "tpcds",
        sql: "SELECT i_item_id, i_item_desc, i_category, i_class, i_current_price, \
              SUM(ss_ext_sales_price) as itemrevenue, \
              SUM(ss_ext_sales_price) * 100 / SUM(SUM(ss_ext_sales_price)) OVER (PARTITION BY i_class) as revenueratio \
              FROM store_sales, item, date_dim \
              WHERE ss_item_sk = i_item_sk AND i_category IN ('Sports', 'Books', 'Home') \
              AND ss_sold_date_sk = d_date_sk AND d_date BETWEEN '1999-02-22' AND '1999-03-24' \
              GROUP BY i_item_id, i_item_desc, i_category, i_class, i_current_price \
              ORDER BY i_category, i_class, i_item_id, i_item_desc, revenueratio" },
    // Q99: Late shipments by mode and warehouse
    CorpusEntry { category: "tpcds",
        sql: "SELECT SUBSTRING(w_warehouse_name FROM 1 FOR 20) as wname, sm_type, \
              cc_name, \
              SUM(CASE WHEN (cs_ship_date_sk - cs_sold_date_sk <= 30) THEN 1 ELSE 0 END) as d30, \
              SUM(CASE WHEN (cs_ship_date_sk - cs_sold_date_sk > 30) AND (cs_ship_date_sk - cs_sold_date_sk <= 60) THEN 1 ELSE 0 END) as d31_60, \
              SUM(CASE WHEN (cs_ship_date_sk - cs_sold_date_sk > 60) AND (cs_ship_date_sk - cs_sold_date_sk <= 90) THEN 1 ELSE 0 END) as d61_90, \
              SUM(CASE WHEN (cs_ship_date_sk - cs_sold_date_sk > 90) AND (cs_ship_date_sk - cs_sold_date_sk <= 120) THEN 1 ELSE 0 END) as d91_120, \
              SUM(CASE WHEN (cs_ship_date_sk - cs_sold_date_sk > 120) THEN 1 ELSE 0 END) as d120plus \
              FROM catalog_sales, warehouse, ship_mode, call_center, date_dim \
              WHERE d_month_seq BETWEEN 1200 AND 1211 AND cs_ship_date_sk = d_date_sk \
              AND cs_warehouse_sk = w_warehouse_sk AND cs_ship_mode_sk = sm_ship_mode_sk \
              AND cs_call_center_sk = cc_call_center_sk \
              GROUP BY SUBSTRING(w_warehouse_name FROM 1 FOR 20), sm_type, cc_name \
              ORDER BY SUBSTRING(w_warehouse_name FROM 1 FOR 20), sm_type, cc_name \
              LIMIT 100" },
];

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

static EDGE_CASES: &[CorpusEntry] = &[
    CorpusEntry { category: "edge_cases",
        sql: "SELECT * FROM orders LIMIT 0" },
    CorpusEntry { category: "edge_cases",
        sql: "SELECT * FROM orders WHERE FALSE" },
    CorpusEntry { category: "edge_cases",
        sql: "SELECT * FROM orders WHERE TRUE" },
    CorpusEntry { category: "edge_cases",
        sql: "SELECT 1" },
    CorpusEntry { category: "edge_cases",
        sql: "SELECT NULL" },
    CorpusEntry { category: "edge_cases",
        sql: "SELECT DISTINCT o_custkey FROM orders ORDER BY o_custkey" },
    CorpusEntry { category: "edge_cases",
        sql: "SELECT * FROM orders EXCEPT SELECT * FROM orders WHERE o_totalprice > 100000" },
    CorpusEntry { category: "edge_cases",
        sql: "SELECT * FROM orders INTERSECT SELECT * FROM orders WHERE o_orderstatus = 'O'" },
    CorpusEntry { category: "edge_cases",
        sql: "SELECT * FROM orders UNION ALL SELECT * FROM orders" },
    CorpusEntry { category: "edge_cases",
        sql: "SELECT o_orderkey, o_totalprice FROM orders \
              ORDER BY o_totalprice DESC NULLS LAST LIMIT 10 OFFSET 5" },
    CorpusEntry { category: "edge_cases",
        sql: "SELECT * FROM orders WHERE o_totalprice IS NULL" },
    CorpusEntry { category: "edge_cases",
        sql: "SELECT * FROM orders WHERE o_totalprice IS NOT NULL" },
    CorpusEntry { category: "edge_cases",
        sql: "SELECT COALESCE(o_comment, 'none') FROM orders" },
    CorpusEntry { category: "edge_cases",
        sql: "SELECT CASE WHEN o_orderstatus = 'O' THEN 'open' \
                         WHEN o_orderstatus = 'F' THEN 'closed' \
                         ELSE 'other' END AS status_label \
              FROM orders" },
    CorpusEntry { category: "edge_cases",
        sql: "SELECT o_totalprice::integer FROM orders" },
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_is_non_empty() {
        let corpus = all_queries();
        assert!(!corpus.is_empty());
    }

    #[test]
    fn corpus_covers_all_categories() {
        let corpus = all_queries();
        let categories: std::collections::HashSet<&str> =
            corpus.iter().map(|e| e.category).collect();
        for expected in &[
            "simple_crud", "analytics", "multi_table_joins",
            "ctes", "subqueries", "jsonb", "tpch", "tpcds", "edge_cases",
        ] {
            assert!(
                categories.contains(expected),
                "missing category: {expected}"
            );
        }
    }

    #[test]
    fn tpch_has_22_queries() {
        let count = all_queries()
            .iter()
            .filter(|e| e.category == "tpch")
            .count();
        assert_eq!(count, 22, "TPC-H should have exactly 22 queries");
    }

    #[test]
    fn all_queries_have_non_empty_sql() {
        for entry in all_queries() {
            assert!(
                !entry.sql.trim().is_empty(),
                "empty SQL in category {}", entry.category
            );
        }
    }
}
