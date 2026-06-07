#!/usr/bin/env python3
"""Quantify the live-conditions cost effect on Ra plan choice + execution.

For each query, run EXPLAIN (ANALYZE, FORMAT JSON) with ra_planner on under three
FORCED fingerprints (via the debug GUCs): neutral, fully-cached, and
cold/contended. Compare the plan node-type structure (did plan CHOICE change?),
Ra's estimated total cost (magnitude), and actual execution time.
"""
import json
import subprocess

PSQL = ["psql", "-h", "/tmp", "-p", "5433", "-U", "postgres", "-d", "tpch", "-tAq"]

CONDITIONS = {
    "neutral":   (0.0, 0.0, 0.0),
    "cached":    (0.99, 0.0, 0.0),
    "contended": (0.0, 0.9, 0.9),
}

QUERIES = {
    "join2": "SELECT count(*) FROM lineitem l JOIN orders o ON l.l_orderkey=o.o_orderkey WHERE o.o_orderdate >= DATE '1994-01-01'",
    "join3": "SELECT c.c_mktsegment, count(*) FROM lineitem l JOIN orders o ON l.l_orderkey=o.o_orderkey JOIN customer c ON o.o_custkey=c.c_custkey GROUP BY c.c_mktsegment",
    "join4": "SELECT n.n_name, sum(l.l_extendedprice*(1-l.l_discount)) FROM lineitem l JOIN orders o ON l.l_orderkey=o.o_orderkey JOIN customer c ON o.o_custkey=c.c_custkey JOIN nation n ON c.c_nationkey=n.n_nationkey GROUP BY n.n_name",
    "agg":   "SELECT o_orderstatus, count(*), avg(o_totalprice) FROM orders GROUP BY o_orderstatus",
    "scan_filter": "SELECT count(*) FROM lineitem WHERE l_shipdate >= DATE '1994-01-01' AND l_discount < 0.05",
    "part_join": "SELECT p.p_brand, count(*) FROM lineitem l JOIN part p ON l.l_partkey=p.p_partkey JOIN supplier s ON l.l_suppkey=s.s_suppkey GROUP BY p.p_brand",
}


def node_shape(plan):
    """Recursively collect (NodeType, JoinType?) tags as a structural signature."""
    tag = plan.get("Node Type", "?")
    jt = plan.get("Join Type")
    if jt:
        tag += f"[{jt}]"
    kids = plan.get("Plans", [])
    return tag + "(" + ",".join(node_shape(k) for k in kids) + ")" if kids else tag


def run(query, hr, io, cpu):
    sql = (
        "SET ra_planner.enabled=on;"
        f"SET ra_planner.debug_hit_rate={hr};"
        f"SET ra_planner.debug_io_saturation={io};"
        f"SET ra_planner.debug_cpu_load={cpu};"
        f"EXPLAIN (ANALYZE, FORMAT JSON) {query}"
    )
    out = subprocess.run(PSQL + ["-c", sql], capture_output=True, text=True)
    txt = "".join(l for l in out.stdout.splitlines() if not l.startswith("SET") and "Time:" not in l)
    try:
        j = json.loads(txt)
        plan = j[0]["Plan"]
        return node_shape(plan), plan.get("Total Cost"), j[0].get("Execution Time")
    except Exception as e:
        return f"<parse-fail: {e}>", None, None


print(f"{'query':<12} {'condition':<10} {'totalcost':>12} {'exec_ms':>9}  plan-shape")
changed = 0
for qname, q in QUERIES.items():
    shapes = {}
    for cname, (hr, io, cpu) in CONDITIONS.items():
        shape, cost, exec_ms = run(q, hr, io, cpu)
        shapes[cname] = shape
        cs = f"{cost:.1f}" if cost is not None else "n/a"
        es = f"{exec_ms:.2f}" if exec_ms is not None else "n/a"
        print(f"{qname:<12} {cname:<10} {cs:>12} {es:>9}  {shape}")
    distinct = len(set(shapes.values()))
    flip = "  <== PLAN CHOICE CHANGED" if distinct > 1 else ""
    if distinct > 1:
        changed += 1
    print(f"{'':12} -> {distinct} distinct plan shape(s){flip}")
print(f"\nqueries whose plan CHOICE changed across conditions: {changed}/{len(QUERIES)}")
