#!/usr/bin/env python3
"""Run PostgreSQL planning time benchmark for ra_vs_pg queries.

Requires PostgreSQL running with TPC-H data loaded.
Adjust PG, PORT, and DB as needed for your environment.
"""
import subprocess, json, statistics, re

PG = "psql"  # Path to psql binary
PORT = "5432"
DB = "tpch"
WARMUP = 5
ITERATIONS = 30

queries = [
    ("scan_01", "1_simple",
     "SELECT COUNT(*) FROM lineitem WHERE l_shipdate >= '1994-01-01'"),
    ("scan_02", "1_simple",
     "SELECT l_returnflag, l_linestatus, COUNT(*) FROM lineitem "
     "GROUP BY l_returnflag, l_linestatus"),
    ("scan_03", "1_simple",
     "SELECT COUNT(*) FROM orders "
     "WHERE o_orderdate BETWEEN '1995-01-01' AND '1995-03-31'"),
    ("join2_01", "2_join",
     "SELECT c_name, COUNT(o_orderkey) FROM customer "
     "JOIN orders ON c_custkey = o_custkey "
     "WHERE o_orderdate >= '1995-01-01' GROUP BY c_name "
     "ORDER BY COUNT(o_orderkey) DESC LIMIT 10"),
    ("join2_02", "2_join",
     "SELECT o_orderpriority, SUM(l_extendedprice * l_discount) as revenue "
     "FROM orders JOIN lineitem ON o_orderkey = l_orderkey "
     "WHERE l_discount BETWEEN 0.05 AND 0.07 GROUP BY o_orderpriority"),
    ("join2_03", "2_join",
     "SELECT n_name, COUNT(*) as supplier_count FROM nation "
     "JOIN supplier ON n_nationkey = s_nationkey "
     "GROUP BY n_name ORDER BY supplier_count DESC"),
    ("join3_01", "3_multi_join",
     "SELECT c_mktsegment, SUM(l_extendedprice) as revenue FROM customer "
     "JOIN orders ON c_custkey = o_custkey "
     "JOIN lineitem ON o_orderkey = l_orderkey "
     "WHERE l_shipdate > '1995-03-15' GROUP BY c_mktsegment"),
    ("join3_02", "3_multi_join",
     "SELECT n_name, SUM(l_extendedprice * (1 - l_discount)) as revenue "
     "FROM nation JOIN supplier ON n_nationkey = s_nationkey "
     "JOIN lineitem ON s_suppkey = l_suppkey "
     "GROUP BY n_name ORDER BY revenue DESC"),
    ("join3_03", "3_multi_join",
     "SELECT r_name, n_name, COUNT(DISTINCT c_custkey) as customers "
     "FROM region JOIN nation ON r_regionkey = n_regionkey "
     "JOIN customer ON n_nationkey = c_nationkey "
     "JOIN orders ON c_custkey = o_custkey "
     "GROUP BY r_name, n_name"),
    ("star_01", "4_star_join",
     "SELECT n_name, SUM(l_extendedprice * (1 - l_discount)) as revenue "
     "FROM customer JOIN orders ON c_custkey = o_custkey "
     "JOIN lineitem ON o_orderkey = l_orderkey "
     "JOIN supplier ON l_suppkey = s_suppkey "
     "JOIN nation ON s_nationkey = n_nationkey "
     "WHERE o_orderdate >= '1994-01-01' AND o_orderdate < '1995-01-01' "
     "GROUP BY n_name ORDER BY revenue DESC"),
    ("star_02", "4_star_join",
     "SELECT n_name, p_type, "
     "SUM(l_extendedprice * (1 - l_discount)) as volume "
     "FROM part JOIN lineitem ON p_partkey = l_partkey "
     "JOIN supplier ON l_suppkey = s_suppkey "
     "JOIN orders ON l_orderkey = o_orderkey "
     "JOIN customer ON o_custkey = c_custkey "
     "JOIN nation ON c_nationkey = n_nationkey "
     "WHERE o_orderdate BETWEEN '1995-01-01' AND '1996-12-31' "
     "GROUP BY n_name, p_type"),
    ("agg_01", "5_aggregation",
     "SELECT c_name, SUM(o_totalprice) as total_spent FROM customer "
     "JOIN orders ON c_custkey = o_custkey "
     "GROUP BY c_name HAVING SUM(o_totalprice) > 100000 "
     "ORDER BY total_spent DESC LIMIT 20"),
    ("agg_02", "5_aggregation",
     "SELECT o_orderpriority, COUNT(*) as order_count FROM orders "
     "WHERE o_orderdate >= '1993-07-01' AND o_orderdate < '1993-10-01' "
     "AND EXISTS (SELECT 1 FROM lineitem l "
     "WHERE l.l_orderkey = orders.o_orderkey "
     "AND l.l_commitdate < l.l_receiptdate) "
     "GROUP BY o_orderpriority ORDER BY o_orderpriority"),
    ("corr_01", "6_correlated",
     "SELECT c_name, c_acctbal FROM customer "
     "WHERE c_nationkey IN "
     "(SELECT n_nationkey FROM nation WHERE n_regionkey = 1) "
     "ORDER BY c_acctbal DESC LIMIT 10"),
    ("corr_02", "6_correlated",
     "SELECT s_name, s_address FROM supplier "
     "WHERE s_suppkey IN "
     "(SELECT ps_suppkey FROM partsupp WHERE ps_partkey IN "
     "(SELECT p_partkey FROM part WHERE p_name LIKE 'forest%')) "
     "AND s_nationkey = "
     "(SELECT n_nationkey FROM nation WHERE n_name = 'CANADA')"),
    ("tpch_q1", "7_tpch",
     "SELECT l_returnflag, l_linestatus, SUM(l_quantity), "
     "SUM(l_extendedprice), "
     "SUM(l_extendedprice * (1 - l_discount)), "
     "SUM(l_extendedprice * (1 - l_discount) * (1 + l_tax)), "
     "AVG(l_quantity), AVG(l_extendedprice), AVG(l_discount), COUNT(*) "
     "FROM lineitem WHERE l_shipdate <= '1998-12-01' "
     "GROUP BY l_returnflag, l_linestatus "
     "ORDER BY l_returnflag, l_linestatus"),
    ("tpch_q3", "7_tpch",
     "SELECT l_orderkey, "
     "SUM(l_extendedprice * (1 - l_discount)) as revenue, "
     "o_orderdate, o_shippriority FROM customer "
     "JOIN orders ON c_custkey = o_custkey "
     "JOIN lineitem ON l_orderkey = o_orderkey "
     "WHERE c_mktsegment = 'BUILDING' AND o_orderdate < '1995-03-15' "
     "AND l_shipdate > '1995-03-15' "
     "GROUP BY l_orderkey, o_orderdate, o_shippriority "
     "ORDER BY revenue DESC, o_orderdate LIMIT 10"),
    ("tpch_q5", "7_tpch",
     "SELECT n_name, SUM(l_extendedprice * (1 - l_discount)) as revenue "
     "FROM customer JOIN orders ON c_custkey = o_custkey "
     "JOIN lineitem ON l_orderkey = o_orderkey "
     "JOIN supplier ON l_suppkey = s_suppkey "
     "AND c_nationkey = s_nationkey "
     "JOIN nation ON s_nationkey = n_nationkey "
     "JOIN region ON n_regionkey = r_regionkey "
     "WHERE r_name = 'ASIA' "
     "AND o_orderdate >= '1994-01-01' AND o_orderdate < '1995-01-01' "
     "GROUP BY n_name ORDER BY revenue DESC"),
    ("tpch_q10", "7_tpch",
     "SELECT c_custkey, c_name, "
     "SUM(l_extendedprice * (1 - l_discount)) as revenue, "
     "c_acctbal, n_name, c_address, c_phone, c_comment FROM customer "
     "JOIN orders ON c_custkey = o_custkey "
     "JOIN lineitem ON l_orderkey = o_orderkey "
     "JOIN nation ON c_nationkey = n_nationkey "
     "WHERE o_orderdate >= '1993-10-01' AND o_orderdate < '1994-01-01' "
     "AND l_returnflag = 'R' "
     "GROUP BY c_custkey, c_name, c_acctbal, c_phone, n_name, "
     "c_address, c_comment ORDER BY revenue DESC LIMIT 20"),
    ("win_01", "8_window",
     "SELECT o_orderkey, o_totalprice, "
     "RANK() OVER (ORDER BY o_totalprice DESC), "
     "AVG(o_totalprice) OVER (PARTITION BY o_orderstatus) "
     "FROM orders WHERE o_orderdate >= '1995-01-01' LIMIT 100"),
    ("win_02", "8_window",
     "SELECT l_orderkey, l_extendedprice, "
     "SUM(l_extendedprice) OVER (PARTITION BY l_orderkey "
     "ORDER BY l_linenumber "
     "ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) "
     "FROM lineitem "
     "WHERE l_shipdate >= '1995-01-01' AND l_shipdate < '1995-02-01'"),
]

results = []
for i, (qid, cat, sql) in enumerate(queries):
    print(f"  [{i+1}/{len(queries)}] {qid}", end="", flush=True)
    times = []
    for iteration in range(WARMUP + ITERATIONS):
        explain_sql = f"EXPLAIN (ANALYZE, FORMAT JSON) {sql}"
        proc = subprocess.run(
            [PG, "-p", PORT, "-d", DB, "-t", "-A", "-c", explain_sql],
            capture_output=True, text=True, timeout=30
        )
        if proc.returncode != 0:
            print(f" ERROR: {proc.stderr[:100]}")
            break
        try:
            output = proc.stdout.strip()
            lines = output.split('\n')
            json_lines = [l for l in lines if not l.startswith('Time:')]
            json_str = '\n'.join(json_lines)
            plan_json = json.loads(json_str)
            planning_time = plan_json[0]["Planning Time"]
            if iteration >= WARMUP:
                times.append(planning_time)
        except (json.JSONDecodeError, KeyError, IndexError) as e:
            m = re.search(r'"Planning Time":\s*([\d.]+)', proc.stdout)
            if m:
                planning_time = float(m.group(1))
                if iteration >= WARMUP:
                    times.append(planning_time)
            else:
                print(f" PARSE ERROR: {e}")
                break

    if times:
        med = statistics.median(times)
        print(f" {med:.3f}ms")
    else:
        print(" FAILED")

    results.append({
        "id": qid,
        "category": cat,
        "plan_ms": times,
        "success": len(times) == ITERATIONS,
        "error": None if len(times) == ITERATIONS else "incomplete"
    })

json.dump(results, open("/dev/stdout", "w"), indent=2)
