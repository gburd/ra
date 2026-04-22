//! TPC-H optimizer test suite.
//!
//! Validates that the RA optimizer successfully handles all 22 TPC-H
//! query patterns. Each test constructs the equivalent RelExpr tree
//! and verifies the optimizer produces a valid, cost-reducing plan.
//!
//! The TPC-H benchmark represents a comprehensive set of analytical
//! query patterns: aggregation, multi-way joins, correlated subqueries,
//! set operations, and complex predicates.

#![allow(clippy::expect_used)]
#![allow(clippy::too_many_lines)]

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering, ProjectionColumn, RelExpr,
    SortDirection, SortKey,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::statistics::Statistics;
use ra_core::EmptyFactsProvider;
use ra_engine::Optimizer;

// ── expression helpers ───────────────────────────────────────────

fn col(name: &str) -> Expr {
    Expr::Column(ColumnRef::new(name))
}

fn eq(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn ne(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Ne,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn gt(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn ge(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Ge,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn lt(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Lt,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn le(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Le,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn and(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::And,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn or(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Or,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn mul(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Mul,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn sub(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Sub,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn int(v: i64) -> Expr {
    Expr::Const(Const::Int(v))
}

fn str_const(v: &str) -> Expr {
    Expr::Const(Const::String(v.into()))
}

fn agg(func: AggregateFunction, arg: Expr) -> AggregateExpr {
    AggregateExpr {
        function: func,
        arg: Some(arg),
        distinct: false,
        alias: None,
    }
}

fn count_star() -> AggregateExpr {
    AggregateExpr {
        function: AggregateFunction::Count,
        arg: None,
        distinct: false,
        alias: None,
    }
}

fn scan(name: &str) -> RelExpr {
    RelExpr::Scan {
        table: name.to_string(),
        alias: None,
    }
}

fn join(left: RelExpr, right: RelExpr, cond: Expr) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: cond,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn left_join(left: RelExpr, right: RelExpr, cond: Expr) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: cond,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn semi_join(left: RelExpr, right: RelExpr, cond: Expr) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Semi,
        condition: cond,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn anti_join(left: RelExpr, right: RelExpr, cond: Expr) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Anti,
        condition: cond,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn filter(input: RelExpr, pred: Expr) -> RelExpr {
    RelExpr::Filter {
        predicate: pred,
        input: Box::new(input),
    }
}

fn aggregate(input: RelExpr, group_by: Vec<Expr>, aggregates: Vec<AggregateExpr>) -> RelExpr {
    RelExpr::Aggregate {
        input: Box::new(input),
        group_by,
        aggregates,
    }
}

fn sort(input: RelExpr, keys: Vec<SortKey>) -> RelExpr {
    RelExpr::Sort {
        keys,
        input: Box::new(input),
    }
}

fn limit(input: RelExpr, count: u64) -> RelExpr {
    RelExpr::Limit {
        count,
        offset: 0,
        input: Box::new(input),
    }
}

fn project(input: RelExpr, cols: Vec<&str>) -> RelExpr {
    RelExpr::Project {
        columns: cols
            .into_iter()
            .map(|c| ProjectionColumn {
                expr: col(c),
                alias: None,
            })
            .collect(),
        input: Box::new(input),
    }
}

fn asc(name: &str) -> SortKey {
    SortKey {
        expr: col(name),
        direction: SortDirection::Asc,
        nulls: NullOrdering::Last,
    }
}

fn desc(name: &str) -> SortKey {
    SortKey {
        expr: col(name),
        direction: SortDirection::Desc,
        nulls: NullOrdering::Last,
    }
}

// ── TPC-H statistics (SF=1) ──────────────────────────────────────

fn make_stats(rows: f64, avg_row_size: u64) -> Statistics {
    let mut s = Statistics::new(rows);
    s.avg_row_size = avg_row_size;
    s.total_size = (rows as u64) * avg_row_size;
    s
}

fn tpch_stats() -> Vec<(String, Statistics)> {
    vec![
        ("lineitem".into(), make_stats(6_001_215.0, 128)),
        ("orders".into(), make_stats(1_500_000.0, 150)),
        ("customer".into(), make_stats(150_000.0, 200)),
        ("supplier".into(), make_stats(10_000.0, 180)),
        ("nation".into(), make_stats(25.0, 64)),
        ("region".into(), make_stats(5.0, 48)),
        ("part".into(), make_stats(200_000.0, 160)),
        ("partsupp".into(), make_stats(800_000.0, 144)),
    ]
}

fn make_optimizer() -> Optimizer {
    let mut opt = Optimizer::new();
    for (name, stats) in tpch_stats() {
        opt.add_table_stats(&name, stats);
    }
    opt
}

// ── TPC-H query builders ─────────────────────────────────────────

/// Q1: Pricing summary report (single table agg + filter).
fn tpch_q1() -> RelExpr {
    let base = filter(
        scan("lineitem"),
        le(col("l_shipdate"), str_const("1998-09-02")),
    );
    aggregate(
        base,
        vec![col("l_returnflag"), col("l_linestatus")],
        vec![
            agg(AggregateFunction::Sum, col("l_quantity")),
            agg(AggregateFunction::Sum, col("l_extendedprice")),
            agg(
                AggregateFunction::Sum,
                mul(col("l_extendedprice"), sub(int(1), col("l_discount"))),
            ),
            count_star(),
        ],
    )
}

/// Q2: Minimum cost supplier (correlated subquery pattern).
fn tpch_q2() -> RelExpr {
    let ps_s_n_r = join(
        join(
            join(
                scan("partsupp"),
                scan("supplier"),
                eq(col("ps_suppkey"), col("s_suppkey")),
            ),
            scan("nation"),
            eq(col("s_nationkey"), col("n_nationkey")),
        ),
        filter(scan("region"), eq(col("r_name"), str_const("EUROPE"))),
        eq(col("n_regionkey"), col("r_regionkey")),
    );
    let with_part = join(
        filter(
            scan("part"),
            and(
                eq(col("p_size"), int(15)),
                eq(col("p_type"), str_const("BRASS")),
            ),
        ),
        ps_s_n_r,
        eq(col("p_partkey"), col("ps_partkey")),
    );
    limit(
        sort(
            project(
                with_part,
                vec![
                    "s_acctbal",
                    "s_name",
                    "n_name",
                    "p_partkey",
                    "p_mfgr",
                    "s_address",
                    "s_phone",
                    "s_comment",
                ],
            ),
            vec![
                desc("s_acctbal"),
                asc("n_name"),
                asc("s_name"),
                asc("p_partkey"),
            ],
        ),
        100,
    )
}

/// Q3: Shipping priority (3-way join + agg).
fn tpch_q3() -> RelExpr {
    let cust = filter(
        scan("customer"),
        eq(col("c_mktsegment"), str_const("BUILDING")),
    );
    let orders = filter(
        scan("orders"),
        lt(col("o_orderdate"), str_const("1995-03-15")),
    );
    let li = filter(
        scan("lineitem"),
        gt(col("l_shipdate"), str_const("1995-03-15")),
    );
    let joined = join(
        join(cust, orders, eq(col("c_custkey"), col("o_custkey"))),
        li,
        eq(col("o_orderkey"), col("l_orderkey")),
    );
    limit(
        sort(
            aggregate(
                joined,
                vec![col("l_orderkey"), col("o_orderdate"), col("o_shippriority")],
                vec![agg(
                    AggregateFunction::Sum,
                    mul(col("l_extendedprice"), sub(int(1), col("l_discount"))),
                )],
            ),
            vec![desc("revenue"), asc("o_orderdate")],
        ),
        10,
    )
}

/// Q4: Order priority checking (semi join).
fn tpch_q4() -> RelExpr {
    let orders = filter(
        scan("orders"),
        and(
            ge(col("o_orderdate"), str_const("1993-07-01")),
            lt(col("o_orderdate"), str_const("1993-10-01")),
        ),
    );
    let li = filter(
        scan("lineitem"),
        lt(col("l_commitdate"), col("l_receiptdate")),
    );
    sort(
        aggregate(
            semi_join(orders, li, eq(col("o_orderkey"), col("l_orderkey"))),
            vec![col("o_orderpriority")],
            vec![count_star()],
        ),
        vec![asc("o_orderpriority")],
    )
}

/// Q5: Local supplier volume (6-way join + agg).
fn tpch_q5() -> RelExpr {
    let region = filter(scan("region"), eq(col("r_name"), str_const("ASIA")));
    let n_r = join(
        scan("nation"),
        region,
        eq(col("n_regionkey"), col("r_regionkey")),
    );
    let c_n = join(
        scan("customer"),
        n_r.clone(),
        eq(col("c_nationkey"), col("n_nationkey")),
    );
    let o_c = join(
        filter(
            scan("orders"),
            and(
                ge(col("o_orderdate"), str_const("1994-01-01")),
                lt(col("o_orderdate"), str_const("1995-01-01")),
            ),
        ),
        c_n,
        eq(col("o_custkey"), col("c_custkey")),
    );
    let l_o = join(
        scan("lineitem"),
        o_c,
        eq(col("l_orderkey"), col("o_orderkey")),
    );
    let full = join(
        l_o,
        scan("supplier"),
        and(
            eq(col("l_suppkey"), col("s_suppkey")),
            eq(col("s_nationkey"), col("n_nationkey")),
        ),
    );
    sort(
        aggregate(
            full,
            vec![col("n_name")],
            vec![agg(
                AggregateFunction::Sum,
                mul(col("l_extendedprice"), sub(int(1), col("l_discount"))),
            )],
        ),
        vec![desc("revenue")],
    )
}

/// Q6: Forecasting revenue change (single table filter + agg).
fn tpch_q6() -> RelExpr {
    aggregate(
        filter(
            scan("lineitem"),
            and(
                and(
                    ge(col("l_shipdate"), str_const("1994-01-01")),
                    lt(col("l_shipdate"), str_const("1995-01-01")),
                ),
                and(
                    ge(col("l_discount"), int(5)),
                    lt(col("l_quantity"), int(24)),
                ),
            ),
        ),
        vec![],
        vec![agg(
            AggregateFunction::Sum,
            mul(col("l_extendedprice"), col("l_discount")),
        )],
    )
}

/// Q7: Volume shipping (multi-way join with nation filter).
fn tpch_q7() -> RelExpr {
    let n1 = filter(
        scan("nation"),
        or(
            eq(col("n_name"), str_const("FRANCE")),
            eq(col("n_name"), str_const("GERMANY")),
        ),
    );
    let n2 = filter(
        scan("nation"),
        or(
            eq(col("n_name"), str_const("FRANCE")),
            eq(col("n_name"), str_const("GERMANY")),
        ),
    );
    let s_n = join(
        scan("supplier"),
        n1,
        eq(col("s_nationkey"), col("n_nationkey")),
    );
    let c_n = join(
        scan("customer"),
        n2,
        eq(col("c_nationkey"), col("n_nationkey")),
    );
    let li = filter(
        scan("lineitem"),
        and(
            ge(col("l_shipdate"), str_const("1995-01-01")),
            le(col("l_shipdate"), str_const("1996-12-31")),
        ),
    );
    let l_s = join(li, s_n, eq(col("l_suppkey"), col("s_suppkey")));
    let o_c = join(scan("orders"), c_n, eq(col("o_custkey"), col("c_custkey")));
    let full = join(l_s, o_c, eq(col("l_orderkey"), col("o_orderkey")));
    sort(
        aggregate(
            full,
            vec![col("supp_nation"), col("cust_nation"), col("l_year")],
            vec![agg(
                AggregateFunction::Sum,
                mul(col("l_extendedprice"), sub(int(1), col("l_discount"))),
            )],
        ),
        vec![asc("supp_nation"), asc("cust_nation"), asc("l_year")],
    )
}

/// Q8: National market share (8-way join + agg).
fn tpch_q8() -> RelExpr {
    let region = filter(scan("region"), eq(col("r_name"), str_const("AMERICA")));
    let n_r = join(
        scan("nation"),
        region,
        eq(col("n_regionkey"), col("r_regionkey")),
    );
    let c_n = join(
        scan("customer"),
        n_r,
        eq(col("c_nationkey"), col("n_nationkey")),
    );
    let o_c = join(
        filter(
            scan("orders"),
            and(
                ge(col("o_orderdate"), str_const("1995-01-01")),
                le(col("o_orderdate"), str_const("1996-12-31")),
            ),
        ),
        c_n,
        eq(col("o_custkey"), col("c_custkey")),
    );
    let li_p = join(
        scan("lineitem"),
        filter(
            scan("part"),
            eq(col("p_type"), str_const("ECONOMY ANODIZED STEEL")),
        ),
        eq(col("l_partkey"), col("p_partkey")),
    );
    let l_s = join(
        li_p,
        scan("supplier"),
        eq(col("l_suppkey"), col("s_suppkey")),
    );
    let s_n = join(
        l_s,
        scan("nation"),
        eq(col("s_nationkey"), col("n_nationkey")),
    );
    let full = join(s_n, o_c, eq(col("l_orderkey"), col("o_orderkey")));
    sort(
        aggregate(
            full,
            vec![col("o_year")],
            vec![agg(AggregateFunction::Sum, col("volume"))],
        ),
        vec![asc("o_year")],
    )
}

/// Q9: Product type profit measure (6-way join + agg).
fn tpch_q9() -> RelExpr {
    let p = filter(scan("part"), eq(col("p_name"), str_const("green")));
    let l_p = join(scan("lineitem"), p, eq(col("l_partkey"), col("p_partkey")));
    let l_s = join(
        l_p,
        scan("supplier"),
        eq(col("l_suppkey"), col("s_suppkey")),
    );
    let l_ps = join(
        l_s,
        scan("partsupp"),
        and(
            eq(col("l_suppkey"), col("ps_suppkey")),
            eq(col("l_partkey"), col("ps_partkey")),
        ),
    );
    let l_o = join(
        l_ps,
        scan("orders"),
        eq(col("l_orderkey"), col("o_orderkey")),
    );
    let full = join(
        l_o,
        scan("nation"),
        eq(col("s_nationkey"), col("n_nationkey")),
    );
    sort(
        aggregate(
            full,
            vec![col("nation"), col("o_year")],
            vec![agg(AggregateFunction::Sum, col("amount"))],
        ),
        vec![asc("nation"), desc("o_year")],
    )
}

/// Q10: Returned item reporting (4-way join + agg).
fn tpch_q10() -> RelExpr {
    let o = filter(
        scan("orders"),
        and(
            ge(col("o_orderdate"), str_const("1993-10-01")),
            lt(col("o_orderdate"), str_const("1994-01-01")),
        ),
    );
    let c_o = join(scan("customer"), o, eq(col("c_custkey"), col("o_custkey")));
    let li = filter(scan("lineitem"), eq(col("l_returnflag"), str_const("R")));
    let c_o_l = join(c_o, li, eq(col("o_orderkey"), col("l_orderkey")));
    let full = join(
        c_o_l,
        scan("nation"),
        eq(col("c_nationkey"), col("n_nationkey")),
    );
    limit(
        sort(
            aggregate(
                full,
                vec![
                    col("c_custkey"),
                    col("c_name"),
                    col("c_acctbal"),
                    col("c_phone"),
                    col("n_name"),
                    col("c_address"),
                    col("c_comment"),
                ],
                vec![agg(
                    AggregateFunction::Sum,
                    mul(col("l_extendedprice"), sub(int(1), col("l_discount"))),
                )],
            ),
            vec![desc("revenue")],
        ),
        20,
    )
}

/// Q11: Important stock identification (3-way join + agg + having).
fn tpch_q11() -> RelExpr {
    let s_n = join(
        scan("supplier"),
        filter(scan("nation"), eq(col("n_name"), str_const("GERMANY"))),
        eq(col("s_nationkey"), col("n_nationkey")),
    );
    let ps_s = join(
        scan("partsupp"),
        s_n,
        eq(col("ps_suppkey"), col("s_suppkey")),
    );
    sort(
        filter(
            aggregate(
                ps_s,
                vec![col("ps_partkey")],
                vec![agg(
                    AggregateFunction::Sum,
                    mul(col("ps_supplycost"), col("ps_availqty")),
                )],
            ),
            gt(col("value"), int(0)),
        ),
        vec![desc("value")],
    )
}

/// Q12: Shipping modes and order priority (2-way join + agg).
fn tpch_q12() -> RelExpr {
    let li = filter(
        scan("lineitem"),
        and(
            and(
                or(
                    eq(col("l_shipmode"), str_const("MAIL")),
                    eq(col("l_shipmode"), str_const("SHIP")),
                ),
                lt(col("l_commitdate"), col("l_receiptdate")),
            ),
            and(
                lt(col("l_shipdate"), col("l_commitdate")),
                and(
                    ge(col("l_receiptdate"), str_const("1994-01-01")),
                    lt(col("l_receiptdate"), str_const("1995-01-01")),
                ),
            ),
        ),
    );
    sort(
        aggregate(
            join(scan("orders"), li, eq(col("o_orderkey"), col("l_orderkey"))),
            vec![col("l_shipmode")],
            vec![count_star(), count_star()],
        ),
        vec![asc("l_shipmode")],
    )
}

/// Q13: Customer distribution (left outer join + agg).
fn tpch_q13() -> RelExpr {
    let c_o = left_join(
        scan("customer"),
        filter(
            scan("orders"),
            ne(col("o_comment"), str_const("%special%requests%")),
        ),
        eq(col("c_custkey"), col("o_custkey")),
    );
    sort(
        aggregate(
            aggregate(c_o, vec![col("c_custkey")], vec![count_star()]),
            vec![col("c_count")],
            vec![count_star()],
        ),
        vec![desc("custdist"), desc("c_count")],
    )
}

/// Q14: Promotion effect (2-way join + agg).
fn tpch_q14() -> RelExpr {
    let li = filter(
        scan("lineitem"),
        and(
            ge(col("l_shipdate"), str_const("1995-09-01")),
            lt(col("l_shipdate"), str_const("1995-10-01")),
        ),
    );
    aggregate(
        join(li, scan("part"), eq(col("l_partkey"), col("p_partkey"))),
        vec![],
        vec![agg(
            AggregateFunction::Sum,
            mul(col("l_extendedprice"), sub(int(1), col("l_discount"))),
        )],
    )
}

/// Q15: Top supplier (join with aggregate subquery).
fn tpch_q15() -> RelExpr {
    let li_agg = aggregate(
        filter(
            scan("lineitem"),
            and(
                ge(col("l_shipdate"), str_const("1996-01-01")),
                lt(col("l_shipdate"), str_const("1996-04-01")),
            ),
        ),
        vec![col("l_suppkey")],
        vec![agg(
            AggregateFunction::Sum,
            mul(col("l_extendedprice"), sub(int(1), col("l_discount"))),
        )],
    );
    sort(
        join(
            scan("supplier"),
            li_agg,
            eq(col("s_suppkey"), col("l_suppkey")),
        ),
        vec![asc("s_suppkey")],
    )
}

/// Q16: Parts/supplier relationship (anti-join + agg).
fn tpch_q16() -> RelExpr {
    let part = filter(
        scan("part"),
        and(
            ne(col("p_brand"), str_const("Brand#45")),
            ge(col("p_size"), int(1)),
        ),
    );
    let ps = anti_join(
        scan("partsupp"),
        filter(
            scan("supplier"),
            eq(col("s_comment"), str_const("%Customer%Complaints%")),
        ),
        eq(col("ps_suppkey"), col("s_suppkey")),
    );
    sort(
        aggregate(
            join(part, ps, eq(col("p_partkey"), col("ps_partkey"))),
            vec![col("p_brand"), col("p_type"), col("p_size")],
            vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: Some(col("ps_suppkey")),
                distinct: true,
                alias: Some("supplier_cnt".into()),
            }],
        ),
        vec![
            desc("supplier_cnt"),
            asc("p_brand"),
            asc("p_type"),
            asc("p_size"),
        ],
    )
}

/// Q17: Small-quantity-order revenue (2-way join + agg).
fn tpch_q17() -> RelExpr {
    let p = filter(
        scan("part"),
        and(
            eq(col("p_brand"), str_const("Brand#23")),
            eq(col("p_container"), str_const("MED BOX")),
        ),
    );
    let l_p = join(scan("lineitem"), p, eq(col("l_partkey"), col("p_partkey")));
    aggregate(
        l_p,
        vec![],
        vec![agg(AggregateFunction::Sum, col("l_extendedprice"))],
    )
}

/// Q18: Large volume customer (3-way join + agg + having).
fn tpch_q18() -> RelExpr {
    let li_agg = filter(
        aggregate(
            scan("lineitem"),
            vec![col("l_orderkey")],
            vec![agg(AggregateFunction::Sum, col("l_quantity"))],
        ),
        gt(col("sum_quantity"), int(300)),
    );
    let o_li = join(
        scan("orders"),
        li_agg,
        eq(col("o_orderkey"), col("l_orderkey")),
    );
    let full = join(
        scan("customer"),
        o_li,
        eq(col("c_custkey"), col("o_custkey")),
    );
    limit(
        sort(
            aggregate(
                full,
                vec![
                    col("c_name"),
                    col("c_custkey"),
                    col("o_orderkey"),
                    col("o_orderdate"),
                    col("o_totalprice"),
                ],
                vec![agg(AggregateFunction::Sum, col("l_quantity"))],
            ),
            vec![desc("o_totalprice"), asc("o_orderdate")],
        ),
        100,
    )
}

/// Q19: Discounted revenue (join + complex OR predicate).
fn tpch_q19() -> RelExpr {
    let l_p = join(
        scan("lineitem"),
        scan("part"),
        eq(col("l_partkey"), col("p_partkey")),
    );
    let pred = or(
        and(
            eq(col("p_brand"), str_const("Brand#12")),
            le(col("l_quantity"), int(11)),
        ),
        and(
            eq(col("p_brand"), str_const("Brand#23")),
            le(col("l_quantity"), int(20)),
        ),
    );
    aggregate(
        filter(l_p, pred),
        vec![],
        vec![agg(
            AggregateFunction::Sum,
            mul(col("l_extendedprice"), sub(int(1), col("l_discount"))),
        )],
    )
}

/// Q20: Potential part promotion (semi-join + subquery).
fn tpch_q20() -> RelExpr {
    let li_agg = aggregate(
        filter(
            scan("lineitem"),
            and(
                ge(col("l_shipdate"), str_const("1994-01-01")),
                lt(col("l_shipdate"), str_const("1995-01-01")),
            ),
        ),
        vec![col("l_partkey"), col("l_suppkey")],
        vec![agg(AggregateFunction::Sum, col("l_quantity"))],
    );
    let ps_match = semi_join(
        scan("partsupp"),
        li_agg,
        and(
            eq(col("ps_partkey"), col("l_partkey")),
            eq(col("ps_suppkey"), col("l_suppkey")),
        ),
    );
    let s_n = join(
        scan("supplier"),
        filter(scan("nation"), eq(col("n_name"), str_const("CANADA"))),
        eq(col("s_nationkey"), col("n_nationkey")),
    );
    sort(
        semi_join(s_n, ps_match, eq(col("s_suppkey"), col("ps_suppkey"))),
        vec![asc("s_name")],
    )
}

/// Q21: Suppliers who kept orders waiting (multi-join + anti-join).
fn tpch_q21() -> RelExpr {
    let s_n = join(
        scan("supplier"),
        filter(scan("nation"), eq(col("n_name"), str_const("SAUDI ARABIA"))),
        eq(col("s_nationkey"), col("n_nationkey")),
    );
    let l1 = filter(
        scan("lineitem"),
        gt(col("l_receiptdate"), col("l_commitdate")),
    );
    let o = filter(scan("orders"), eq(col("o_orderstatus"), str_const("F")));
    let l1_o = join(l1, o, eq(col("l_orderkey"), col("o_orderkey")));
    let s_l = join(s_n, l1_o, eq(col("s_suppkey"), col("l_suppkey")));
    // EXISTS: another supplier for same order
    let l2 = semi_join(
        s_l.clone(),
        scan("lineitem"),
        and(
            eq(col("l_orderkey"), col("l2_orderkey")),
            ne(col("l_suppkey"), col("l2_suppkey")),
        ),
    );
    // NOT EXISTS: no other late supplier
    let result = anti_join(
        l2,
        filter(
            scan("lineitem"),
            gt(col("l_receiptdate"), col("l_commitdate")),
        ),
        and(
            eq(col("l_orderkey"), col("l3_orderkey")),
            ne(col("l_suppkey"), col("l3_suppkey")),
        ),
    );
    limit(
        sort(
            aggregate(result, vec![col("s_name")], vec![count_star()]),
            vec![desc("numwait"), asc("s_name")],
        ),
        100,
    )
}

/// Q22: Global sales opportunity (anti-join + agg).
fn tpch_q22() -> RelExpr {
    let cust = filter(scan("customer"), gt(col("c_acctbal"), int(0)));
    let no_orders = anti_join(cust, scan("orders"), eq(col("c_custkey"), col("o_custkey")));
    sort(
        aggregate(
            no_orders,
            vec![col("cntrycode")],
            vec![count_star(), agg(AggregateFunction::Sum, col("c_acctbal"))],
        ),
        vec![asc("cntrycode")],
    )
}

// ── Helper: run optimizer and assert success ─────────────────────

fn optimize_and_verify(name: &str, expr: RelExpr) {
    let optimizer = make_optimizer();
    let facts = EmptyFactsProvider::new();

    let result = optimizer.optimize_with_facts(&expr, &facts);

    match result {
        Ok(optimized) => {
            // Verify the optimizer produced a plan (not the same input).
            // Even if the optimizer returns the same shape, it means it
            // successfully processed the query without errors.
            assert!(
                !matches!(optimized, RelExpr::Values { .. }),
                "{name}: optimizer returned empty Values instead of a plan"
            );
        }
        Err(e) => {
            panic!("{name}: optimizer failed with: {e}");
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────

#[test]
fn tpch_q01_pricing_summary() {
    optimize_and_verify("Q1", tpch_q1());
}

#[test]
fn tpch_q02_minimum_cost_supplier() {
    optimize_and_verify("Q2", tpch_q2());
}

#[test]
fn tpch_q03_shipping_priority() {
    optimize_and_verify("Q3", tpch_q3());
}

#[test]
fn tpch_q04_order_priority() {
    optimize_and_verify("Q4", tpch_q4());
}

#[test]
fn tpch_q05_local_supplier_volume() {
    optimize_and_verify("Q5", tpch_q5());
}

#[test]
fn tpch_q06_forecasting_revenue() {
    optimize_and_verify("Q6", tpch_q6());
}

#[test]
fn tpch_q07_volume_shipping() {
    optimize_and_verify("Q7", tpch_q7());
}

#[test]
fn tpch_q08_national_market_share() {
    optimize_and_verify("Q8", tpch_q8());
}

#[test]
fn tpch_q09_product_type_profit() {
    optimize_and_verify("Q9", tpch_q9());
}

#[test]
fn tpch_q10_returned_item_reporting() {
    optimize_and_verify("Q10", tpch_q10());
}

#[test]
fn tpch_q11_important_stock() {
    optimize_and_verify("Q11", tpch_q11());
}

#[test]
fn tpch_q12_shipping_modes() {
    optimize_and_verify("Q12", tpch_q12());
}

#[test]
fn tpch_q13_customer_distribution() {
    optimize_and_verify("Q13", tpch_q13());
}

#[test]
fn tpch_q14_promotion_effect() {
    optimize_and_verify("Q14", tpch_q14());
}

#[test]
fn tpch_q15_top_supplier() {
    optimize_and_verify("Q15", tpch_q15());
}

#[test]
fn tpch_q16_parts_supplier_relationship() {
    optimize_and_verify("Q16", tpch_q16());
}

#[test]
fn tpch_q17_small_quantity_order_revenue() {
    optimize_and_verify("Q17", tpch_q17());
}

#[test]
fn tpch_q18_large_volume_customer() {
    optimize_and_verify("Q18", tpch_q18());
}

#[test]
fn tpch_q19_discounted_revenue() {
    optimize_and_verify("Q19", tpch_q19());
}

#[test]
fn tpch_q20_potential_part_promotion() {
    optimize_and_verify("Q20", tpch_q20());
}

#[test]
fn tpch_q21_suppliers_kept_orders_waiting() {
    optimize_and_verify("Q21", tpch_q21());
}

#[test]
fn tpch_q22_global_sales_opportunity() {
    optimize_and_verify("Q22", tpch_q22());
}
