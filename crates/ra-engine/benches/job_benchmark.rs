//! Join Order Benchmark (JOB) optimizer benchmarks.
//!
//! All 113 JOB queries from the IMDB dataset, translated to Ra
//! relational algebra expressions. Measures optimizer latency for
//! join ordering across varying query complexity (2-17 tables).
//!
//! Reference: "How Good Are Query Optimizers, Really?" (Leis et al.)
//!
//! Run with:
//!   cargo bench --package ra-engine --bench job_benchmark

#![allow(clippy::expect_used)]
#![allow(clippy::too_many_lines)]

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId, Criterion,
};
use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, RelExpr,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::statistics::Statistics;
use ra_core::EmptyFactsProvider;
use ra_engine::Optimizer;

// ── expression helpers ──────────────────────────────────────────

fn col(name: &str) -> Expr {
    Expr::Column(ColumnRef::new(name))
}

fn eq(l: Expr, r: Expr) -> Expr {
    Expr::BinOp { op: BinOp::Eq, left: Box::new(l), right: Box::new(r) }
}

fn ne(l: Expr, r: Expr) -> Expr {
    Expr::BinOp { op: BinOp::Ne, left: Box::new(l), right: Box::new(r) }
}

fn gt(l: Expr, r: Expr) -> Expr {
    Expr::BinOp { op: BinOp::Gt, left: Box::new(l), right: Box::new(r) }
}

fn ge(l: Expr, r: Expr) -> Expr {
    Expr::BinOp { op: BinOp::Ge, left: Box::new(l), right: Box::new(r) }
}

fn le(l: Expr, r: Expr) -> Expr {
    Expr::BinOp { op: BinOp::Le, left: Box::new(l), right: Box::new(r) }
}

fn and(l: Expr, r: Expr) -> Expr {
    Expr::BinOp { op: BinOp::And, left: Box::new(l), right: Box::new(r) }
}

fn int(v: i64) -> Expr { Expr::Const(Const::Int(v)) }

fn str_c(v: &str) -> Expr { Expr::Const(Const::String(v.into())) }

fn min_agg(e: Expr) -> AggregateExpr {
    AggregateExpr {
        function: AggregateFunction::Min,
        arg: Some(e),
        distinct: false,
        alias: None,
    }
}

fn scan(name: &str) -> RelExpr {
    RelExpr::Scan { table: name.to_string(), alias: None }
}

fn join(l: RelExpr, r: RelExpr, c: Expr) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: c,
        left: Box::new(l),
        right: Box::new(r),
    }
}

fn filt(input: RelExpr, pred: Expr) -> RelExpr {
    RelExpr::Filter { predicate: pred, input: Box::new(input) }
}

fn agg(input: RelExpr, aggs: Vec<AggregateExpr>) -> RelExpr {
    RelExpr::Aggregate {
        group_by: vec![],
        aggregates: aggs,
        input: Box::new(input),
    }
}

// ── IMDB statistics (May 2013 snapshot) ─────────────────────────

fn mk_stats(rows: f64, avg: u64) -> Statistics {
    let mut s = Statistics::new(rows);
    s.avg_row_size = avg;
    s.total_size = (rows as u64) * avg;
    s
}

fn make_optimizer() -> Optimizer {
    let mut opt = Optimizer::new();
    for (name, rows, sz) in [
        ("aka_name", 901_343.0, 100_u64),
        ("aka_title", 361_472.0, 150),
        ("cast_info", 36_244_344.0, 80),
        ("char_name", 3_140_339.0, 90),
        ("comp_cast_type", 4.0, 50),
        ("company_name", 234_997.0, 120),
        ("company_type", 4.0, 50),
        ("complete_cast", 135_086.0, 60),
        ("info_type", 113.0, 50),
        ("keyword", 134_170.0, 100),
        ("kind_type", 7.0, 50),
        ("link_type", 18.0, 50),
        ("movie_companies", 2_609_129.0, 100),
        ("movie_info", 14_835_720.0, 150),
        ("movie_info_idx", 1_380_035.0, 100),
        ("movie_keyword", 4_523_930.0, 60),
        ("movie_link", 29_997.0, 80),
        ("name", 4_167_491.0, 110),
        ("person_info", 2_963_664.0, 120),
        ("role_type", 12.0, 50),
        ("title", 2_528_312.0, 180),
    ] {
        opt.add_table_stats(name, mk_stats(rows, sz));
    }
    opt
}

// ── JOB Queries 1a-1d (5 tables) ───────────────────────────────
// Tables: company_type, info_type, movie_companies, movie_info_idx, title

fn q1a() -> RelExpr {
    let ct = filt(scan("company_type"), eq(col("kind"), str_c("production companies")));
    let it = filt(scan("info_type"), eq(col("info"), str_c("top 250 rank")));
    let mc_ct = join(scan("movie_companies"), ct, eq(col("company_type_id"), col("ct.id")));
    let mc_t = join(mc_ct, scan("title"), eq(col("movie_id"), col("t.id")));
    let mc_t_mi = join(mc_t, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let full = join(mc_t_mi, it, eq(col("mi_idx.info_type_id"), col("it.id")));
    agg(full, vec![min_agg(col("mc.note")), min_agg(col("t.title")), min_agg(col("t.production_year"))])
}

fn q1b() -> RelExpr {
    let ct = filt(scan("company_type"), eq(col("kind"), str_c("production companies")));
    let it = filt(scan("info_type"), eq(col("info"), str_c("bottom 10 rank")));
    let t = filt(scan("title"), and(ge(col("production_year"), int(2005)), le(col("production_year"), int(2010))));
    let mc_ct = join(scan("movie_companies"), ct, eq(col("company_type_id"), col("ct.id")));
    let mc_t = join(mc_ct, t, eq(col("movie_id"), col("t.id")));
    let mc_t_mi = join(mc_t, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let full = join(mc_t_mi, it, eq(col("mi_idx.info_type_id"), col("it.id")));
    agg(full, vec![min_agg(col("mc.note")), min_agg(col("t.title")), min_agg(col("t.production_year"))])
}

fn q1c() -> RelExpr {
    let ct = filt(scan("company_type"), eq(col("kind"), str_c("production companies")));
    let it = filt(scan("info_type"), eq(col("info"), str_c("top 250 rank")));
    let t = filt(scan("title"), gt(col("production_year"), int(2010)));
    let mc_ct = join(scan("movie_companies"), ct, eq(col("company_type_id"), col("ct.id")));
    let mc_t = join(mc_ct, t, eq(col("movie_id"), col("t.id")));
    let mc_t_mi = join(mc_t, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let full = join(mc_t_mi, it, eq(col("mi_idx.info_type_id"), col("it.id")));
    agg(full, vec![min_agg(col("mc.note")), min_agg(col("t.title")), min_agg(col("t.production_year"))])
}

fn q1d() -> RelExpr {
    let ct = filt(scan("company_type"), eq(col("kind"), str_c("production companies")));
    let it = filt(scan("info_type"), eq(col("info"), str_c("bottom 10 rank")));
    let t = filt(scan("title"), gt(col("production_year"), int(2000)));
    let mc_ct = join(scan("movie_companies"), ct, eq(col("company_type_id"), col("ct.id")));
    let mc_t = join(mc_ct, t, eq(col("movie_id"), col("t.id")));
    let mc_t_mi = join(mc_t, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let full = join(mc_t_mi, it, eq(col("mi_idx.info_type_id"), col("it.id")));
    agg(full, vec![min_agg(col("mc.note")), min_agg(col("t.title")), min_agg(col("t.production_year"))])
}

// ── JOB Queries 2a-2d (5 tables) ───────────────────────────────
// Tables: company_name, keyword, movie_companies, movie_keyword, title

fn q2a() -> RelExpr {
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c("[de]")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("character-name-in-title")));
    let cn_mc = join(cn, scan("movie_companies"), eq(col("cn.id"), col("mc.company_id")));
    let cn_mc_t = join(cn_mc, scan("title"), eq(col("mc.movie_id"), col("t.id")));
    let cn_mc_t_mk = join(cn_mc_t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let full = join(cn_mc_t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    agg(full, vec![min_agg(col("t.title"))])
}

fn q2b() -> RelExpr {
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c("[nl]")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("character-name-in-title")));
    let cn_mc = join(cn, scan("movie_companies"), eq(col("cn.id"), col("mc.company_id")));
    let cn_mc_t = join(cn_mc, scan("title"), eq(col("mc.movie_id"), col("t.id")));
    let cn_mc_t_mk = join(cn_mc_t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let full = join(cn_mc_t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    agg(full, vec![min_agg(col("t.title"))])
}

fn q2c() -> RelExpr {
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c("[sm]")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("character-name-in-title")));
    let cn_mc = join(cn, scan("movie_companies"), eq(col("cn.id"), col("mc.company_id")));
    let cn_mc_t = join(cn_mc, scan("title"), eq(col("mc.movie_id"), col("t.id")));
    let cn_mc_t_mk = join(cn_mc_t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let full = join(cn_mc_t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    agg(full, vec![min_agg(col("t.title"))])
}

fn q2d() -> RelExpr {
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c("[us]")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("character-name-in-title")));
    let cn_mc = join(cn, scan("movie_companies"), eq(col("cn.id"), col("mc.company_id")));
    let cn_mc_t = join(cn_mc, scan("title"), eq(col("mc.movie_id"), col("t.id")));
    let cn_mc_t_mk = join(cn_mc_t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let full = join(cn_mc_t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    agg(full, vec![min_agg(col("t.title"))])
}

// ── JOB Queries 3a-3c (4 tables) ───────────────────────────────
// Tables: keyword, movie_info, movie_keyword, title

fn q3a() -> RelExpr {
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("sequel")));
    let t = filt(scan("title"), gt(col("production_year"), int(2005)));
    let t_mi = join(t, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_mk = join(t_mi, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let full = join(t_mi_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    agg(full, vec![min_agg(col("t.title"))])
}

fn q3b() -> RelExpr {
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("sequel")));
    let t = filt(scan("title"), gt(col("production_year"), int(2010)));
    let t_mi = join(t, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_mk = join(t_mi, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let full = join(t_mi_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    agg(full, vec![min_agg(col("t.title"))])
}

fn q3c() -> RelExpr {
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("sequel")));
    let t = filt(scan("title"), gt(col("production_year"), int(1990)));
    let t_mi = join(t, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_mk = join(t_mi, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let full = join(t_mi_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    agg(full, vec![min_agg(col("t.title"))])
}

// ── JOB Queries 4a-4c (5 tables) ───────────────────────────────
// Tables: info_type, keyword, movie_info_idx, movie_keyword, title

fn q4a() -> RelExpr {
    let it = filt(scan("info_type"), eq(col("info"), str_c("rating")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("sequel")));
    let t = filt(scan("title"), gt(col("production_year"), int(2005)));
    let t_mk = join(t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mk_mi = join(t_mk_k, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let full = join(t_mk_mi, it, eq(col("mi_idx.info_type_id"), col("it.id")));
    agg(full, vec![min_agg(col("mi_idx.info")), min_agg(col("t.title"))])
}

fn q4b() -> RelExpr {
    let it = filt(scan("info_type"), eq(col("info"), str_c("rating")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("sequel")));
    let t = filt(scan("title"), gt(col("production_year"), int(2010)));
    let t_mk = join(t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mk_mi = join(t_mk_k, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let full = join(t_mk_mi, it, eq(col("mi_idx.info_type_id"), col("it.id")));
    agg(full, vec![min_agg(col("mi_idx.info")), min_agg(col("t.title"))])
}

fn q4c() -> RelExpr {
    let it = filt(scan("info_type"), eq(col("info"), str_c("rating")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("sequel")));
    let t = filt(scan("title"), gt(col("production_year"), int(1990)));
    let t_mk = join(t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mk_mi = join(t_mk_k, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let full = join(t_mk_mi, it, eq(col("mi_idx.info_type_id"), col("it.id")));
    agg(full, vec![min_agg(col("mi_idx.info")), min_agg(col("t.title"))])
}

// ── JOB Queries 5a-5c (5 tables) ───────────────────────────────
// Tables: company_type, info_type, movie_companies, movie_info, title

fn q5a() -> RelExpr {
    let ct = filt(scan("company_type"), eq(col("kind"), str_c("production companies")));
    let t = filt(scan("title"), gt(col("production_year"), int(2005)));
    let t_mi = join(t, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_mc = join(t_mi, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let t_mi_mc_ct = join(t_mi_mc, ct, eq(col("mc.company_type_id"), col("ct.id")));
    let full = join(t_mi_mc_ct, scan("info_type"), eq(col("mi.info_type_id"), col("it.id")));
    agg(full, vec![min_agg(col("t.title"))])
}

fn q5b() -> RelExpr {
    let ct = filt(scan("company_type"), eq(col("kind"), str_c("production companies")));
    let t = filt(scan("title"), gt(col("production_year"), int(2010)));
    let t_mi = join(t, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_mc = join(t_mi, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let t_mi_mc_ct = join(t_mi_mc, ct, eq(col("mc.company_type_id"), col("ct.id")));
    let full = join(t_mi_mc_ct, scan("info_type"), eq(col("mi.info_type_id"), col("it.id")));
    agg(full, vec![min_agg(col("t.title"))])
}

fn q5c() -> RelExpr {
    let ct = filt(scan("company_type"), eq(col("kind"), str_c("production companies")));
    let t = filt(scan("title"), gt(col("production_year"), int(1990)));
    let t_mi = join(t, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_mc = join(t_mi, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let t_mi_mc_ct = join(t_mi_mc, ct, eq(col("mc.company_type_id"), col("ct.id")));
    let full = join(t_mi_mc_ct, scan("info_type"), eq(col("mi.info_type_id"), col("it.id")));
    agg(full, vec![min_agg(col("t.title"))])
}

// ── JOB Queries 6a-6f (5 tables) ───────────────────────────────
// Tables: cast_info, keyword, movie_keyword, name, title

fn q6a() -> RelExpr {
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("marvel-cinematic-universe")));
    let t = filt(scan("title"), gt(col("production_year"), int(2010)));
    let t_mk = join(t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mk_ci = join(t_mk_k, scan("cast_info"), eq(col("t.id"), col("ci.movie_id")));
    let full = join(t_mk_ci, scan("name"), eq(col("ci.person_id"), col("n.id")));
    agg(full, vec![min_agg(col("k.keyword")), min_agg(col("n.name")), min_agg(col("t.title"))])
}

fn q6b() -> RelExpr {
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("superhero")));
    let t = filt(scan("title"), gt(col("production_year"), int(2014)));
    let t_mk = join(t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mk_ci = join(t_mk_k, scan("cast_info"), eq(col("t.id"), col("ci.movie_id")));
    let full = join(t_mk_ci, scan("name"), eq(col("ci.person_id"), col("n.id")));
    agg(full, vec![min_agg(col("k.keyword")), min_agg(col("n.name")), min_agg(col("t.title"))])
}

fn q6c() -> RelExpr {
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("marvel-cinematic-universe")));
    let t = filt(scan("title"), gt(col("production_year"), int(2014)));
    let t_mk = join(t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mk_ci = join(t_mk_k, scan("cast_info"), eq(col("t.id"), col("ci.movie_id")));
    let full = join(t_mk_ci, scan("name"), eq(col("ci.person_id"), col("n.id")));
    agg(full, vec![min_agg(col("k.keyword")), min_agg(col("n.name")), min_agg(col("t.title"))])
}

fn q6d() -> RelExpr {
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("superhero")));
    let t = filt(scan("title"), gt(col("production_year"), int(2000)));
    let t_mk = join(t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mk_ci = join(t_mk_k, scan("cast_info"), eq(col("t.id"), col("ci.movie_id")));
    let full = join(t_mk_ci, scan("name"), eq(col("ci.person_id"), col("n.id")));
    agg(full, vec![min_agg(col("k.keyword")), min_agg(col("n.name")), min_agg(col("t.title"))])
}

fn q6e() -> RelExpr {
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("marvel-cinematic-universe")));
    let t = filt(scan("title"), gt(col("production_year"), int(2000)));
    let t_mk = join(t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mk_ci = join(t_mk_k, scan("cast_info"), eq(col("t.id"), col("ci.movie_id")));
    let full = join(t_mk_ci, scan("name"), eq(col("ci.person_id"), col("n.id")));
    agg(full, vec![min_agg(col("k.keyword")), min_agg(col("n.name")), min_agg(col("t.title"))])
}

fn q6f() -> RelExpr {
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("superhero")));
    let t = filt(scan("title"), gt(col("production_year"), int(2000)));
    let t_mk = join(t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mk_ci = join(t_mk_k, scan("cast_info"), eq(col("t.id"), col("ci.movie_id")));
    let full = join(t_mk_ci, scan("name"), eq(col("ci.person_id"), col("n.id")));
    agg(full, vec![min_agg(col("k.keyword")), min_agg(col("n.name")), min_agg(col("t.title"))])
}

// ── JOB Queries 7a-7c (8 tables) ───────────────────────────────
// Tables: aka_name, cast_info, info_type, link_type, movie_link, name, person_info, title

fn q7_base(t_lo: i64, t_hi: i64) -> RelExpr {
    let it = filt(scan("info_type"), eq(col("info"), str_c("mini biography")));
    let lt = filt(scan("link_type"), eq(col("link"), str_c("features")));
    let t = filt(scan("title"), and(ge(col("production_year"), int(t_lo)), le(col("production_year"), int(t_hi))));
    let n_an = join(scan("name"), scan("aka_name"), eq(col("n.id"), col("an.person_id")));
    let n_pi = join(n_an, scan("person_info"), eq(col("n.id"), col("pi.person_id")));
    let n_pi_it = join(n_pi, it, eq(col("pi.info_type_id"), col("it.id")));
    let n_ci = join(n_pi_it, scan("cast_info"), eq(col("n.id"), col("ci.person_id")));
    let n_ci_t = join(n_ci, t, eq(col("ci.movie_id"), col("t.id")));
    let n_ci_ml = join(n_ci_t, scan("movie_link"), eq(col("t.id"), col("ml.linked_movie_id")));
    let full = join(n_ci_ml, lt, eq(col("ml.link_type_id"), col("lt.id")));
    agg(full, vec![min_agg(col("n.name")), min_agg(col("t.title"))])
}

fn q7a() -> RelExpr { q7_base(1980, 1995) }
fn q7b() -> RelExpr { q7_base(1980, 1984) }
fn q7c() -> RelExpr { q7_base(1980, 2010) }

// ── JOB Queries 8a-8d (7 tables) ───────────────────────────────
// Tables: aka_name, cast_info, company_name, movie_companies, name, role_type, title

fn q8_base(rt_role: &str, cn_code: &str) -> RelExpr {
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c(cn_code)));
    let rt = filt(scan("role_type"), eq(col("role"), str_c(rt_role)));
    let n_an = join(scan("name"), scan("aka_name"), eq(col("n.id"), col("an.person_id")));
    let n_ci = join(n_an, scan("cast_info"), eq(col("n.id"), col("ci.person_id")));
    let n_ci_rt = join(n_ci, rt, eq(col("ci.role_id"), col("rt.id")));
    let n_ci_t = join(n_ci_rt, scan("title"), eq(col("ci.movie_id"), col("t.id")));
    let n_ci_mc = join(n_ci_t, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let full = join(n_ci_mc, cn, eq(col("mc.company_id"), col("cn.id")));
    agg(full, vec![min_agg(col("an.name")), min_agg(col("t.title"))])
}

fn q8a() -> RelExpr { q8_base("actress", "[jp]") }
fn q8b() -> RelExpr { q8_base("actress", "[jp]") }
fn q8c() -> RelExpr { q8_base("writer", "[us]") }
fn q8d() -> RelExpr { q8_base("costume designer", "[us]") }

// ── JOB Queries 9a-9d (8 tables) ───────────────────────────────
// Tables: aka_name, char_name, cast_info, company_name, movie_companies, name, role_type, title

fn q9_base(cn_code: &str, rt_role: &str) -> RelExpr {
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c(cn_code)));
    let rt = filt(scan("role_type"), eq(col("role"), str_c(rt_role)));
    let n_an = join(scan("name"), scan("aka_name"), eq(col("n.id"), col("an.person_id")));
    let n_ci = join(n_an, scan("cast_info"), eq(col("n.id"), col("ci.person_id")));
    let n_ci_rt = join(n_ci, rt, eq(col("ci.role_id"), col("rt.id")));
    let n_ci_chn = join(n_ci_rt, scan("char_name"), eq(col("ci.person_role_id"), col("chn.id")));
    let n_ci_t = join(n_ci_chn, scan("title"), eq(col("ci.movie_id"), col("t.id")));
    let n_ci_mc = join(n_ci_t, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let full = join(n_ci_mc, cn, eq(col("mc.company_id"), col("cn.id")));
    agg(full, vec![min_agg(col("an.name")), min_agg(col("chn.name")), min_agg(col("t.title"))])
}

fn q9a() -> RelExpr { q9_base("[us]", "actress") }
fn q9b() -> RelExpr { q9_base("[us]", "actress") }
fn q9c() -> RelExpr { q9_base("[us]", "actress") }
fn q9d() -> RelExpr { q9_base("[us]", "actress") }

// ── JOB Queries 10a-10c (7 tables) ─────────────────────────────
// Tables: char_name, cast_info, company_name, company_type, movie_companies, role_type, title

fn q10_base(cn_code: &str, t_year: i64) -> RelExpr {
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c(cn_code)));
    let rt = filt(scan("role_type"), eq(col("role"), str_c("actor")));
    let t = filt(scan("title"), gt(col("production_year"), int(t_year)));
    let ci_rt = join(scan("cast_info"), rt, eq(col("ci.role_id"), col("rt.id")));
    let ci_chn = join(ci_rt, scan("char_name"), eq(col("ci.person_role_id"), col("chn.id")));
    let ci_t = join(ci_chn, t, eq(col("ci.movie_id"), col("t.id")));
    let ci_mc = join(ci_t, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let ci_mc_ct = join(ci_mc, scan("company_type"), eq(col("mc.company_type_id"), col("ct.id")));
    let full = join(ci_mc_ct, cn, eq(col("mc.company_id"), col("cn.id")));
    agg(full, vec![min_agg(col("chn.name")), min_agg(col("t.title"))])
}

fn q10a() -> RelExpr { q10_base("[ru]", 2005) }
fn q10b() -> RelExpr { q10_base("[ru]", 2010) }
fn q10c() -> RelExpr { q10_base("[us]", 1990) }

// ── JOB Queries 11a-11d (8 tables) ─────────────────────────────
// Tables: company_name, company_type, keyword, link_type, movie_companies, movie_keyword, movie_link, title

fn q11_base(t_lo: i64, t_hi: i64) -> RelExpr {
    let cn = filt(scan("company_name"), ne(col("country_code"), str_c("[pl]")));
    let ct = filt(scan("company_type"), eq(col("kind"), str_c("production companies")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("sequel")));
    let lt = filt(scan("link_type"), eq(col("link"), str_c("follows")));
    let t = filt(scan("title"), and(ge(col("production_year"), int(t_lo)), le(col("production_year"), int(t_hi))));
    let t_mk = join(t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mc = join(t_mk_k, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let t_mc_ct = join(t_mc, ct, eq(col("mc.company_type_id"), col("ct.id")));
    let t_mc_cn = join(t_mc_ct, cn, eq(col("mc.company_id"), col("cn.id")));
    let t_ml = join(t_mc_cn, scan("movie_link"), eq(col("t.id"), col("ml.movie_id")));
    let full = join(t_ml, lt, eq(col("ml.link_type_id"), col("lt.id")));
    agg(full, vec![min_agg(col("cn.name")), min_agg(col("lt.link")), min_agg(col("t.title"))])
}

fn q11a() -> RelExpr { q11_base(1950, 2000) }
fn q11b() -> RelExpr { q11_base(1998, 1998) }
fn q11c() -> RelExpr { q11_base(1950, 2000) }
fn q11d() -> RelExpr { q11_base(1950, 2000) }

// ── JOB Queries 12a-12c (8 tables) ─────────────────────────────
// Tables: company_name, company_type, info_type(x2), movie_companies, movie_info, movie_info_idx, title

fn q12_base(t_lo: i64, t_hi: i64) -> RelExpr {
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c("[us]")));
    let ct = filt(scan("company_type"), eq(col("kind"), str_c("production companies")));
    let it1 = filt(scan("info_type"), eq(col("info"), str_c("genres")));
    let it2 = filt(scan("info_type"), eq(col("info"), str_c("rating")));
    let t = filt(scan("title"), and(ge(col("production_year"), int(t_lo)), le(col("production_year"), int(t_hi))));
    let t_mi = join(t, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_it1 = join(t_mi, it1, eq(col("mi.info_type_id"), col("it1.id")));
    let t_mi_idx = join(t_mi_it1, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let t_mi_idx_it2 = join(t_mi_idx, it2, eq(col("mi_idx.info_type_id"), col("it2.id")));
    let t_mc = join(t_mi_idx_it2, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let t_mc_ct = join(t_mc, ct, eq(col("mc.company_type_id"), col("ct.id")));
    let full = join(t_mc_ct, cn, eq(col("mc.company_id"), col("cn.id")));
    agg(full, vec![min_agg(col("cn.name")), min_agg(col("mi_idx.info")), min_agg(col("t.title"))])
}

fn q12a() -> RelExpr { q12_base(2005, 2008) }
fn q12b() -> RelExpr { q12_base(2000, 2013) }
fn q12c() -> RelExpr { q12_base(2000, 2010) }

// ── JOB Queries 13a-13d (9 tables) ─────────────────────────────
// Tables: company_name, company_type, info_type(x2), kind_type, movie_companies, movie_info, movie_info_idx, title

fn q13_base(cn_code: &str) -> RelExpr {
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c(cn_code)));
    let ct = filt(scan("company_type"), eq(col("kind"), str_c("production companies")));
    let it = filt(scan("info_type"), eq(col("info"), str_c("rating")));
    let it2 = filt(scan("info_type"), eq(col("info"), str_c("release dates")));
    let kt = filt(scan("kind_type"), eq(col("kind"), str_c("movie")));
    let t_kt = join(scan("title"), kt, eq(col("t.kind_id"), col("kt.id")));
    let t_mi = join(t_kt, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_it2 = join(t_mi, it2, eq(col("mi.info_type_id"), col("it2.id")));
    let t_mi_idx = join(t_mi_it2, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let t_mi_idx_it = join(t_mi_idx, it, eq(col("mi_idx.info_type_id"), col("it.id")));
    let t_mc = join(t_mi_idx_it, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let t_mc_ct = join(t_mc, ct, eq(col("mc.company_type_id"), col("ct.id")));
    let full = join(t_mc_ct, cn, eq(col("mc.company_id"), col("cn.id")));
    agg(full, vec![min_agg(col("cn.name")), min_agg(col("mi_idx.info")), min_agg(col("t.title"))])
}

fn q13a() -> RelExpr { q13_base("[de]") }
fn q13b() -> RelExpr { q13_base("[us]") }
fn q13c() -> RelExpr { q13_base("[us]") }
fn q13d() -> RelExpr { q13_base("[us]") }

// ── JOB Queries 14a-14c (8 tables) ─────────────────────────────
// Tables: info_type(x2), keyword, kind_type, movie_info, movie_info_idx, movie_keyword, title

fn q14_base(t_year: i64) -> RelExpr {
    let it1 = filt(scan("info_type"), eq(col("info"), str_c("countries")));
    let it2 = filt(scan("info_type"), eq(col("info"), str_c("rating")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("murder")));
    let kt = filt(scan("kind_type"), eq(col("kind"), str_c("movie")));
    let t = filt(scan("title"), gt(col("production_year"), int(t_year)));
    let t_kt = join(t, kt, eq(col("t.kind_id"), col("kt.id")));
    let t_mi = join(t_kt, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_it1 = join(t_mi, it1, eq(col("mi.info_type_id"), col("it1.id")));
    let t_mi_idx = join(t_mi_it1, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let t_mi_idx_it2 = join(t_mi_idx, it2, eq(col("mi_idx.info_type_id"), col("it2.id")));
    let t_mk = join(t_mi_idx_it2, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let full = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    agg(full, vec![min_agg(col("mi_idx.info")), min_agg(col("t.title"))])
}

fn q14a() -> RelExpr { q14_base(2010) }
fn q14b() -> RelExpr { q14_base(2010) }
fn q14c() -> RelExpr { q14_base(2005) }

// ── JOB Queries 15a-15d (9 tables) ─────────────────────────────
// Tables: aka_title, company_name, company_type, info_type, keyword, movie_companies, movie_info, movie_keyword, title

fn q15_base(t_year: i64) -> RelExpr {
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c("[us]")));
    let it = filt(scan("info_type"), eq(col("info"), str_c("release dates")));
    let t = filt(scan("title"), gt(col("production_year"), int(t_year)));
    let t_at = join(t, scan("aka_title"), eq(col("t.id"), col("at.movie_id")));
    let t_mi = join(t_at, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_it = join(t_mi, it, eq(col("mi.info_type_id"), col("it.id")));
    let t_mk = join(t_mi_it, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, scan("keyword"), eq(col("mk.keyword_id"), col("k.id")));
    let t_mc = join(t_mk_k, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let t_mc_cn = join(t_mc, cn, eq(col("mc.company_id"), col("cn.id")));
    let full = join(t_mc_cn, scan("company_type"), eq(col("mc.company_type_id"), col("ct.id")));
    agg(full, vec![min_agg(col("mi.info")), min_agg(col("t.title"))])
}

fn q15a() -> RelExpr { q15_base(2000) }
fn q15b() -> RelExpr { q15_base(2005) }
fn q15c() -> RelExpr { q15_base(1990) }
fn q15d() -> RelExpr { q15_base(1990) }

// ── JOB Queries 16a-16d (8 tables) ─────────────────────────────
// Tables: aka_name, cast_info, company_name, keyword, movie_companies, movie_keyword, name, title

fn q16_base(cn_code: &str) -> RelExpr {
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c(cn_code)));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("character-name-in-title")));
    let n_an = join(scan("name"), scan("aka_name"), eq(col("n.id"), col("an.person_id")));
    let n_ci = join(n_an, scan("cast_info"), eq(col("n.id"), col("ci.person_id")));
    let n_ci_t = join(n_ci, scan("title"), eq(col("ci.movie_id"), col("t.id")));
    let n_ci_mk = join(n_ci_t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let n_ci_mk_k = join(n_ci_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let n_ci_mc = join(n_ci_mk_k, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let full = join(n_ci_mc, cn, eq(col("mc.company_id"), col("cn.id")));
    agg(full, vec![min_agg(col("an.name")), min_agg(col("t.title"))])
}

fn q16a() -> RelExpr { q16_base("[us]") }
fn q16b() -> RelExpr { q16_base("[us]") }
fn q16c() -> RelExpr { q16_base("[us]") }
fn q16d() -> RelExpr { q16_base("[us]") }

// ── JOB Queries 17a-17f (7 tables) ─────────────────────────────
// Tables: cast_info, company_name, keyword, movie_companies, movie_keyword, name, title

fn q17_base(cn_code: &str) -> RelExpr {
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("character-name-in-title")));
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c(cn_code)));
    let n_ci = join(scan("name"), scan("cast_info"), eq(col("n.id"), col("ci.person_id")));
    let n_ci_t = join(n_ci, scan("title"), eq(col("ci.movie_id"), col("t.id")));
    let n_ci_mk = join(n_ci_t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let n_ci_mk_k = join(n_ci_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let n_ci_mc = join(n_ci_mk_k, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let full = join(n_ci_mc, cn, eq(col("mc.company_id"), col("cn.id")));
    agg(full, vec![min_agg(col("n.name"))])
}

fn q17a() -> RelExpr { q17_base("[us]") }
fn q17b() -> RelExpr { q17_base("[us]") }
fn q17c() -> RelExpr { q17_base("[us]") }
fn q17d() -> RelExpr { q17_base("[us]") }
fn q17e() -> RelExpr { q17_base("[us]") }
fn q17f() -> RelExpr { q17_base("[us]") }

// ── JOB Queries 18a-18c (7 tables) ─────────────────────────────
// Tables: cast_info, info_type(x2), movie_info, movie_info_idx, name, title

fn q18_base(gender: &str) -> RelExpr {
    let it1 = filt(scan("info_type"), eq(col("info"), str_c("budget")));
    let it2 = filt(scan("info_type"), eq(col("info"), str_c("votes")));
    let n = filt(scan("name"), eq(col("gender"), str_c(gender)));
    let n_ci = join(n, scan("cast_info"), eq(col("n.id"), col("ci.person_id")));
    let n_ci_t = join(n_ci, scan("title"), eq(col("ci.movie_id"), col("t.id")));
    let t_mi = join(n_ci_t, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_it1 = join(t_mi, it1, eq(col("mi.info_type_id"), col("it1.id")));
    let t_mi_idx = join(t_mi_it1, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let full = join(t_mi_idx, it2, eq(col("mi_idx.info_type_id"), col("it2.id")));
    agg(full, vec![min_agg(col("mi.info")), min_agg(col("mi_idx.info")), min_agg(col("t.title"))])
}

fn q18a() -> RelExpr { q18_base("m") }
fn q18b() -> RelExpr { q18_base("f") }
fn q18c() -> RelExpr { q18_base("m") }

// ── JOB Queries 19a-19d (10 tables) ────────────────────────────
// Tables: aka_name, char_name, cast_info, company_name, info_type, movie_companies, movie_info, name, role_type, title

fn q19_base(t_year: i64) -> RelExpr {
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c("[us]")));
    let it = filt(scan("info_type"), eq(col("info"), str_c("release dates")));
    let rt = filt(scan("role_type"), eq(col("role"), str_c("actress")));
    let n = filt(scan("name"), eq(col("gender"), str_c("f")));
    let t = filt(scan("title"), gt(col("production_year"), int(t_year)));
    let n_an = join(n, scan("aka_name"), eq(col("n.id"), col("an.person_id")));
    let n_ci = join(n_an, scan("cast_info"), eq(col("n.id"), col("ci.person_id")));
    let n_ci_rt = join(n_ci, rt, eq(col("ci.role_id"), col("rt.id")));
    let n_ci_chn = join(n_ci_rt, scan("char_name"), eq(col("ci.person_role_id"), col("chn.id")));
    let n_ci_t = join(n_ci_chn, t, eq(col("ci.movie_id"), col("t.id")));
    let t_mi = join(n_ci_t, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_it = join(t_mi, it, eq(col("mi.info_type_id"), col("it.id")));
    let t_mc = join(t_mi_it, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let full = join(t_mc, cn, eq(col("mc.company_id"), col("cn.id")));
    agg(full, vec![min_agg(col("n.name")), min_agg(col("t.title"))])
}

fn q19a() -> RelExpr { q19_base(2005) }
fn q19b() -> RelExpr { q19_base(2007) }
fn q19c() -> RelExpr { q19_base(2000) }
fn q19d() -> RelExpr { q19_base(2000) }

// ── JOB Queries 20a-20c (10 tables) ────────────────────────────
// Tables: complete_cast, comp_cast_type(x2), char_name, cast_info, keyword, kind_type, movie_keyword, name, title

fn q20_base(t_year: i64) -> RelExpr {
    let cct1 = filt(scan("comp_cast_type"), eq(col("kind"), str_c("cast")));
    let cct2 = filt(scan("comp_cast_type"), eq(col("kind"), str_c("complete")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("superhero")));
    let kt = filt(scan("kind_type"), eq(col("kind"), str_c("movie")));
    let t = filt(scan("title"), gt(col("production_year"), int(t_year)));
    let t_kt = join(t, kt, eq(col("t.kind_id"), col("kt.id")));
    let t_mk = join(t_kt, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_ci = join(t_mk_k, scan("cast_info"), eq(col("t.id"), col("ci.movie_id")));
    let t_ci_n = join(t_ci, scan("name"), eq(col("ci.person_id"), col("n.id")));
    let t_ci_chn = join(t_ci_n, scan("char_name"), eq(col("ci.person_role_id"), col("chn.id")));
    let t_cc = join(t_ci_chn, scan("complete_cast"), eq(col("t.id"), col("cc.movie_id")));
    let t_cc_cct1 = join(t_cc, cct1, eq(col("cc.subject_id"), col("cct1.id")));
    let full = join(t_cc_cct1, cct2, eq(col("cc.status_id"), col("cct2.id")));
    agg(full, vec![min_agg(col("t.title"))])
}

fn q20a() -> RelExpr { q20_base(1950) }
fn q20b() -> RelExpr { q20_base(2000) }
fn q20c() -> RelExpr { q20_base(2000) }

// ── JOB Queries 21a-21c (9 tables) ─────────────────────────────
// Tables: company_name, company_type, keyword, link_type, movie_companies, movie_info, movie_keyword, movie_link, title

fn q21_base(t_lo: i64, t_hi: i64) -> RelExpr {
    let cn = filt(scan("company_name"), ne(col("country_code"), str_c("[pl]")));
    let ct = filt(scan("company_type"), eq(col("kind"), str_c("production companies")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("sequel")));
    let lt = filt(scan("link_type"), eq(col("link"), str_c("follows")));
    let t = filt(scan("title"), and(ge(col("production_year"), int(t_lo)), le(col("production_year"), int(t_hi))));
    let t_mk = join(t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mc = join(t_mk_k, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let t_mc_ct = join(t_mc, ct, eq(col("mc.company_type_id"), col("ct.id")));
    let t_mc_cn = join(t_mc_ct, cn, eq(col("mc.company_id"), col("cn.id")));
    let t_mi = join(t_mc_cn, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_ml = join(t_mi, scan("movie_link"), eq(col("t.id"), col("ml.movie_id")));
    let full = join(t_ml, lt, eq(col("ml.link_type_id"), col("lt.id")));
    agg(full, vec![min_agg(col("cn.name")), min_agg(col("lt.link")), min_agg(col("t.title"))])
}

fn q21a() -> RelExpr { q21_base(1950, 2000) }
fn q21b() -> RelExpr { q21_base(2000, 2010) }
fn q21c() -> RelExpr { q21_base(1950, 2010) }

// ── JOB Queries 22a-22d (11 tables) ────────────────────────────
// Tables: company_name, company_type, info_type(x2), keyword, kind_type, movie_companies, movie_info, movie_info_idx, movie_keyword, title

fn q22_base(t_year: i64) -> RelExpr {
    let cn = filt(scan("company_name"), ne(col("country_code"), str_c("[us]")));
    let it1 = filt(scan("info_type"), eq(col("info"), str_c("countries")));
    let it2 = filt(scan("info_type"), eq(col("info"), str_c("rating")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("murder")));
    let kt = filt(scan("kind_type"), eq(col("kind"), str_c("movie")));
    let t = filt(scan("title"), gt(col("production_year"), int(t_year)));
    let t_kt = join(t, kt, eq(col("t.kind_id"), col("kt.id")));
    let t_mi = join(t_kt, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_it1 = join(t_mi, it1, eq(col("mi.info_type_id"), col("it1.id")));
    let t_mk = join(t_mi_it1, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mi_idx = join(t_mk_k, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let t_mi_idx_it2 = join(t_mi_idx, it2, eq(col("mi_idx.info_type_id"), col("it2.id")));
    let t_mc = join(t_mi_idx_it2, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let t_mc_ct = join(t_mc, scan("company_type"), eq(col("mc.company_type_id"), col("ct.id")));
    let full = join(t_mc_ct, cn, eq(col("mc.company_id"), col("cn.id")));
    agg(full, vec![min_agg(col("cn.name")), min_agg(col("mi_idx.info")), min_agg(col("t.title"))])
}

fn q22a() -> RelExpr { q22_base(2008) }
fn q22b() -> RelExpr { q22_base(2009) }
fn q22c() -> RelExpr { q22_base(2005) }
fn q22d() -> RelExpr { q22_base(2005) }

// ── JOB Queries 23a-23c (11 tables) ────────────────────────────
// Tables: complete_cast, comp_cast_type, company_name, company_type, info_type, keyword, kind_type, movie_companies, movie_info, movie_keyword, title

fn q23_base(t_year: i64) -> RelExpr {
    let cct1 = filt(scan("comp_cast_type"), eq(col("kind"), str_c("complete+verified")));
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c("[us]")));
    let it = filt(scan("info_type"), eq(col("info"), str_c("release dates")));
    let kt = filt(scan("kind_type"), eq(col("kind"), str_c("movie")));
    let t = filt(scan("title"), gt(col("production_year"), int(t_year)));
    let t_kt = join(t, kt, eq(col("t.kind_id"), col("kt.id")));
    let t_mi = join(t_kt, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_it = join(t_mi, it, eq(col("mi.info_type_id"), col("it.id")));
    let t_mk = join(t_mi_it, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, scan("keyword"), eq(col("mk.keyword_id"), col("k.id")));
    let t_mc = join(t_mk_k, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let t_mc_cn = join(t_mc, cn, eq(col("mc.company_id"), col("cn.id")));
    let t_mc_ct = join(t_mc_cn, scan("company_type"), eq(col("mc.company_type_id"), col("ct.id")));
    let t_cc = join(t_mc_ct, scan("complete_cast"), eq(col("t.id"), col("cc.movie_id")));
    let full = join(t_cc, cct1, eq(col("cc.status_id"), col("cct1.id")));
    agg(full, vec![min_agg(col("kt.kind")), min_agg(col("t.title"))])
}

fn q23a() -> RelExpr { q23_base(2000) }
fn q23b() -> RelExpr { q23_base(2000) }
fn q23c() -> RelExpr { q23_base(1990) }

// ── JOB Queries 24a-24b (12 tables) ────────────────────────────
// Tables: aka_name, char_name, cast_info, company_name, info_type, keyword, movie_companies, movie_info, movie_keyword, name, role_type, title

fn q24_base(t_year: i64) -> RelExpr {
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c("[us]")));
    let it = filt(scan("info_type"), eq(col("info"), str_c("release dates")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("hero")));
    let rt = filt(scan("role_type"), eq(col("role"), str_c("actress")));
    let n = filt(scan("name"), eq(col("gender"), str_c("f")));
    let t = filt(scan("title"), gt(col("production_year"), int(t_year)));
    let n_an = join(n, scan("aka_name"), eq(col("n.id"), col("an.person_id")));
    let n_ci = join(n_an, scan("cast_info"), eq(col("n.id"), col("ci.person_id")));
    let n_ci_rt = join(n_ci, rt, eq(col("ci.role_id"), col("rt.id")));
    let n_ci_chn = join(n_ci_rt, scan("char_name"), eq(col("ci.person_role_id"), col("chn.id")));
    let n_ci_t = join(n_ci_chn, t, eq(col("ci.movie_id"), col("t.id")));
    let t_mk = join(n_ci_t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mi = join(t_mk_k, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_it = join(t_mi, it, eq(col("mi.info_type_id"), col("it.id")));
    let t_mc = join(t_mi_it, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let full = join(t_mc, cn, eq(col("mc.company_id"), col("cn.id")));
    agg(full, vec![min_agg(col("chn.name")), min_agg(col("n.name")), min_agg(col("t.title"))])
}

fn q24a() -> RelExpr { q24_base(2010) }
fn q24b() -> RelExpr { q24_base(2010) }

// ── JOB Queries 25a-25c (9 tables) ─────────────────────────────
// Tables: cast_info, info_type(x2), keyword, movie_info, movie_info_idx, movie_keyword, name, title

fn q25_base(gender: &str) -> RelExpr {
    let it1 = filt(scan("info_type"), eq(col("info"), str_c("genres")));
    let it2 = filt(scan("info_type"), eq(col("info"), str_c("votes")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("murder")));
    let n = filt(scan("name"), eq(col("gender"), str_c(gender)));
    let n_ci = join(n, scan("cast_info"), eq(col("n.id"), col("ci.person_id")));
    let n_ci_t = join(n_ci, scan("title"), eq(col("ci.movie_id"), col("t.id")));
    let t_mi = join(n_ci_t, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_it1 = join(t_mi, it1, eq(col("mi.info_type_id"), col("it1.id")));
    let t_mi_idx = join(t_mi_it1, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let t_mi_idx_it2 = join(t_mi_idx, it2, eq(col("mi_idx.info_type_id"), col("it2.id")));
    let t_mk = join(t_mi_idx_it2, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let full = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    agg(full, vec![min_agg(col("mi.info")), min_agg(col("mi_idx.info")), min_agg(col("n.name")), min_agg(col("t.title"))])
}

fn q25a() -> RelExpr { q25_base("m") }
fn q25b() -> RelExpr { q25_base("m") }
fn q25c() -> RelExpr { q25_base("m") }

// ── JOB Queries 26a-26c (12 tables) ────────────────────────────
// Tables: complete_cast, comp_cast_type(x2), char_name, cast_info, info_type, keyword, kind_type, movie_info_idx, movie_keyword, name, title

fn q26_base(t_year: i64) -> RelExpr {
    let cct1 = filt(scan("comp_cast_type"), eq(col("kind"), str_c("cast")));
    let cct2 = filt(scan("comp_cast_type"), eq(col("kind"), str_c("complete")));
    let it = filt(scan("info_type"), eq(col("info"), str_c("rating")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("superhero")));
    let kt = filt(scan("kind_type"), eq(col("kind"), str_c("movie")));
    let t = filt(scan("title"), gt(col("production_year"), int(t_year)));
    let t_kt = join(t, kt, eq(col("t.kind_id"), col("kt.id")));
    let t_mk = join(t_kt, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_ci = join(t_mk_k, scan("cast_info"), eq(col("t.id"), col("ci.movie_id")));
    let t_ci_n = join(t_ci, scan("name"), eq(col("ci.person_id"), col("n.id")));
    let t_ci_chn = join(t_ci_n, scan("char_name"), eq(col("ci.person_role_id"), col("chn.id")));
    let t_mi_idx = join(t_ci_chn, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let t_mi_idx_it = join(t_mi_idx, it, eq(col("mi_idx.info_type_id"), col("it.id")));
    let t_cc = join(t_mi_idx_it, scan("complete_cast"), eq(col("t.id"), col("cc.movie_id")));
    let t_cc_cct1 = join(t_cc, cct1, eq(col("cc.subject_id"), col("cct1.id")));
    let full = join(t_cc_cct1, cct2, eq(col("cc.status_id"), col("cct2.id")));
    agg(full, vec![min_agg(col("chn.name")), min_agg(col("mi_idx.info")), min_agg(col("n.name")), min_agg(col("t.title"))])
}

fn q26a() -> RelExpr { q26_base(2000) }
fn q26b() -> RelExpr { q26_base(2005) }
fn q26c() -> RelExpr { q26_base(2000) }

// ── JOB Queries 27a-27c (12 tables) ────────────────────────────
// Tables: complete_cast, comp_cast_type(x2), company_name, company_type, keyword, link_type, movie_companies, movie_info, movie_keyword, movie_link, title

fn q27_base(t_lo: i64, t_hi: i64) -> RelExpr {
    let cct1 = filt(scan("comp_cast_type"), eq(col("kind"), str_c("cast")));
    let cct2 = filt(scan("comp_cast_type"), eq(col("kind"), str_c("complete")));
    let cn = filt(scan("company_name"), ne(col("country_code"), str_c("[pl]")));
    let ct = filt(scan("company_type"), eq(col("kind"), str_c("production companies")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("sequel")));
    let lt = filt(scan("link_type"), eq(col("link"), str_c("follows")));
    let t = filt(scan("title"), and(ge(col("production_year"), int(t_lo)), le(col("production_year"), int(t_hi))));
    let t_mk = join(t, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mc = join(t_mk_k, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let t_mc_ct = join(t_mc, ct, eq(col("mc.company_type_id"), col("ct.id")));
    let t_mc_cn = join(t_mc_ct, cn, eq(col("mc.company_id"), col("cn.id")));
    let t_mi = join(t_mc_cn, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_ml = join(t_mi, scan("movie_link"), eq(col("t.id"), col("ml.movie_id")));
    let t_ml_lt = join(t_ml, lt, eq(col("ml.link_type_id"), col("lt.id")));
    let t_cc = join(t_ml_lt, scan("complete_cast"), eq(col("t.id"), col("cc.movie_id")));
    let t_cc_cct1 = join(t_cc, cct1, eq(col("cc.subject_id"), col("cct1.id")));
    let full = join(t_cc_cct1, cct2, eq(col("cc.status_id"), col("cct2.id")));
    agg(full, vec![min_agg(col("cn.name")), min_agg(col("lt.link")), min_agg(col("t.title"))])
}

fn q27a() -> RelExpr { q27_base(1950, 2000) }
fn q27b() -> RelExpr { q27_base(1998, 1998) }
fn q27c() -> RelExpr { q27_base(1950, 2010) }

// ── JOB Queries 28a-28c (14 tables) ────────────────────────────
// Tables: complete_cast, comp_cast_type(x2), company_name, company_type, info_type(x2), keyword, kind_type, movie_companies, movie_info, movie_info_idx, movie_keyword, title

fn q28_base(t_year: i64) -> RelExpr {
    let cct1 = filt(scan("comp_cast_type"), eq(col("kind"), str_c("crew")));
    let cn = filt(scan("company_name"), ne(col("country_code"), str_c("[us]")));
    let it1 = filt(scan("info_type"), eq(col("info"), str_c("countries")));
    let it2 = filt(scan("info_type"), eq(col("info"), str_c("rating")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("murder")));
    let kt = filt(scan("kind_type"), eq(col("kind"), str_c("movie")));
    let t = filt(scan("title"), gt(col("production_year"), int(t_year)));
    let t_kt = join(t, kt, eq(col("t.kind_id"), col("kt.id")));
    let t_mi = join(t_kt, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_it1 = join(t_mi, it1, eq(col("mi.info_type_id"), col("it1.id")));
    let t_mk = join(t_mi_it1, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mi_idx = join(t_mk_k, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let t_mi_idx_it2 = join(t_mi_idx, it2, eq(col("mi_idx.info_type_id"), col("it2.id")));
    let t_mc = join(t_mi_idx_it2, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let t_mc_ct = join(t_mc, scan("company_type"), eq(col("mc.company_type_id"), col("ct.id")));
    let t_mc_cn = join(t_mc_ct, cn, eq(col("mc.company_id"), col("cn.id")));
    let t_cc = join(t_mc_cn, scan("complete_cast"), eq(col("t.id"), col("cc.movie_id")));
    let full = join(t_cc, cct1, eq(col("cc.subject_id"), col("cct1.id")));
    agg(full, vec![min_agg(col("cn.name")), min_agg(col("mi_idx.info")), min_agg(col("t.title"))])
}

fn q28a() -> RelExpr { q28_base(2000) }
fn q28b() -> RelExpr { q28_base(2005) }
fn q28c() -> RelExpr { q28_base(2005) }

// ── JOB Queries 29a-29c (17 tables) ────────────────────────────
// Tables: aka_name, complete_cast, comp_cast_type(x2), char_name, cast_info, company_name, info_type(x2), keyword, movie_companies, movie_info, movie_keyword, name, person_info, role_type, title

fn q29_base(t_lo: i64, t_hi: i64) -> RelExpr {
    let cct1 = filt(scan("comp_cast_type"), eq(col("kind"), str_c("cast")));
    let cct2 = filt(scan("comp_cast_type"), eq(col("kind"), str_c("complete+verified")));
    let cn = filt(scan("company_name"), eq(col("country_code"), str_c("[us]")));
    let it = filt(scan("info_type"), eq(col("info"), str_c("release dates")));
    let it3 = filt(scan("info_type"), eq(col("info"), str_c("trivia")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("computer-animation")));
    let rt = filt(scan("role_type"), eq(col("role"), str_c("actress")));
    let n = filt(scan("name"), eq(col("gender"), str_c("f")));
    let t = filt(scan("title"), and(ge(col("production_year"), int(t_lo)), le(col("production_year"), int(t_hi))));
    let n_an = join(n, scan("aka_name"), eq(col("n.id"), col("an.person_id")));
    let n_ci = join(n_an, scan("cast_info"), eq(col("n.id"), col("ci.person_id")));
    let n_ci_rt = join(n_ci, rt, eq(col("ci.role_id"), col("rt.id")));
    let n_ci_chn = join(n_ci_rt, scan("char_name"), eq(col("ci.person_role_id"), col("chn.id")));
    let n_pi = join(n_ci_chn, scan("person_info"), eq(col("n.id"), col("pi.person_id")));
    let n_pi_it3 = join(n_pi, it3, eq(col("pi.info_type_id"), col("it3.id")));
    let n_ci_t = join(n_pi_it3, t, eq(col("ci.movie_id"), col("t.id")));
    let t_mi = join(n_ci_t, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_it = join(t_mi, it, eq(col("mi.info_type_id"), col("it.id")));
    let t_mk = join(t_mi_it, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mc = join(t_mk_k, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let t_mc_cn = join(t_mc, cn, eq(col("mc.company_id"), col("cn.id")));
    let t_cc = join(t_mc_cn, scan("complete_cast"), eq(col("t.id"), col("cc.movie_id")));
    let t_cc_cct1 = join(t_cc, cct1, eq(col("cc.subject_id"), col("cct1.id")));
    let full = join(t_cc_cct1, cct2, eq(col("cc.status_id"), col("cct2.id")));
    agg(full, vec![min_agg(col("chn.name")), min_agg(col("n.name")), min_agg(col("t.title"))])
}

fn q29a() -> RelExpr { q29_base(2000, 2010) }
fn q29b() -> RelExpr { q29_base(2000, 2005) }
fn q29c() -> RelExpr { q29_base(2000, 2010) }

// ── JOB Queries 30a-30c (12 tables) ────────────────────────────
// Tables: complete_cast, comp_cast_type(x2), cast_info, info_type(x2), keyword, movie_info, movie_info_idx, movie_keyword, name, title

fn q30_base(gender: &str) -> RelExpr {
    let cct1 = filt(scan("comp_cast_type"), eq(col("kind"), str_c("cast")));
    let cct2 = filt(scan("comp_cast_type"), eq(col("kind"), str_c("complete+verified")));
    let it1 = filt(scan("info_type"), eq(col("info"), str_c("genres")));
    let it2 = filt(scan("info_type"), eq(col("info"), str_c("votes")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("murder")));
    let n = filt(scan("name"), eq(col("gender"), str_c(gender)));
    let n_ci = join(n, scan("cast_info"), eq(col("n.id"), col("ci.person_id")));
    let n_ci_t = join(n_ci, scan("title"), eq(col("ci.movie_id"), col("t.id")));
    let t_mi = join(n_ci_t, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_it1 = join(t_mi, it1, eq(col("mi.info_type_id"), col("it1.id")));
    let t_mi_idx = join(t_mi_it1, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let t_mi_idx_it2 = join(t_mi_idx, it2, eq(col("mi_idx.info_type_id"), col("it2.id")));
    let t_mk = join(t_mi_idx_it2, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_cc = join(t_mk_k, scan("complete_cast"), eq(col("t.id"), col("cc.movie_id")));
    let t_cc_cct1 = join(t_cc, cct1, eq(col("cc.subject_id"), col("cct1.id")));
    let full = join(t_cc_cct1, cct2, eq(col("cc.status_id"), col("cct2.id")));
    agg(full, vec![min_agg(col("mi.info")), min_agg(col("mi_idx.info")), min_agg(col("n.name")), min_agg(col("t.title"))])
}

fn q30a() -> RelExpr { q30_base("m") }
fn q30b() -> RelExpr { q30_base("m") }
fn q30c() -> RelExpr { q30_base("m") }

// ── JOB Queries 31a-31c (11 tables) ────────────────────────────
// Tables: cast_info, company_name, info_type(x2), keyword, movie_companies, movie_info, movie_info_idx, movie_keyword, name, title

fn q31_base(cn_name: &str) -> RelExpr {
    let cn = filt(scan("company_name"), eq(col("name"), str_c(cn_name)));
    let it1 = filt(scan("info_type"), eq(col("info"), str_c("genres")));
    let it2 = filt(scan("info_type"), eq(col("info"), str_c("votes")));
    let k = filt(scan("keyword"), eq(col("keyword"), str_c("murder")));
    let n = filt(scan("name"), eq(col("gender"), str_c("m")));
    let n_ci = join(n, scan("cast_info"), eq(col("n.id"), col("ci.person_id")));
    let n_ci_t = join(n_ci, scan("title"), eq(col("ci.movie_id"), col("t.id")));
    let t_mi = join(n_ci_t, scan("movie_info"), eq(col("t.id"), col("mi.movie_id")));
    let t_mi_it1 = join(t_mi, it1, eq(col("mi.info_type_id"), col("it1.id")));
    let t_mi_idx = join(t_mi_it1, scan("movie_info_idx"), eq(col("t.id"), col("mi_idx.movie_id")));
    let t_mi_idx_it2 = join(t_mi_idx, it2, eq(col("mi_idx.info_type_id"), col("it2.id")));
    let t_mk = join(t_mi_idx_it2, scan("movie_keyword"), eq(col("t.id"), col("mk.movie_id")));
    let t_mk_k = join(t_mk, k, eq(col("mk.keyword_id"), col("k.id")));
    let t_mc = join(t_mk_k, scan("movie_companies"), eq(col("t.id"), col("mc.movie_id")));
    let full = join(t_mc, cn, eq(col("mc.company_id"), col("cn.id")));
    agg(full, vec![min_agg(col("mi.info")), min_agg(col("mi_idx.info")), min_agg(col("n.name")), min_agg(col("t.title"))])
}

fn q31a() -> RelExpr { q31_base("Lionsgate") }
fn q31b() -> RelExpr { q31_base("Lionsgate") }
fn q31c() -> RelExpr { q31_base("Lionsgate") }

// ── JOB Queries 32a-32b (6 tables) ─────────────────────────────
// Tables: keyword, link_type, movie_keyword, movie_link, title(x2)

fn q32_base(kw: &str) -> RelExpr {
    let k = filt(scan("keyword"), eq(col("keyword"), str_c(kw)));
    let k_mk = join(k, scan("movie_keyword"), eq(col("k.id"), col("mk.keyword_id")));
    let mk_t1 = join(k_mk, scan("title"), eq(col("mk.movie_id"), col("t1.id")));
    let t1_ml = join(mk_t1, scan("movie_link"), eq(col("t1.id"), col("ml.movie_id")));
    let ml_t2 = join(t1_ml, scan("title"), eq(col("ml.linked_movie_id"), col("t2.id")));
    let full = join(ml_t2, scan("link_type"), eq(col("ml.link_type_id"), col("lt.id")));
    agg(full, vec![min_agg(col("lt.link")), min_agg(col("t1.title")), min_agg(col("t2.title"))])
}

fn q32a() -> RelExpr { q32_base("10,000-mile-club") }
fn q32b() -> RelExpr { q32_base("character-name-in-title") }

// ── JOB Queries 33a-33c (14 tables) ────────────────────────────
// Tables: company_name(x2), info_type(x2), kind_type(x2), link_type, movie_companies(x2), movie_info_idx(x2), movie_link, title(x2)

fn q33_base(cn_code: &str, t2_year: i64) -> RelExpr {
    let cn1 = filt(scan("company_name"), eq(col("country_code"), str_c(cn_code)));
    let it1 = filt(scan("info_type"), eq(col("info"), str_c("rating")));
    let it2 = filt(scan("info_type"), eq(col("info"), str_c("rating")));
    let kt1 = filt(scan("kind_type"), eq(col("kind"), str_c("tv series")));
    let kt2 = filt(scan("kind_type"), eq(col("kind"), str_c("tv series")));
    let lt = filt(scan("link_type"), eq(col("link"), str_c("sequel")));
    let t1_kt1 = join(scan("title"), kt1, eq(col("t1.kind_id"), col("kt1.id")));
    let t2 = filt(scan("title"), gt(col("production_year"), int(t2_year)));
    let t2_kt2 = join(t2, kt2, eq(col("t2.kind_id"), col("kt2.id")));
    let ml_lt = join(scan("movie_link"), lt, eq(col("ml.link_type_id"), col("lt.id")));
    let ml_t1 = join(ml_lt, t1_kt1, eq(col("ml.movie_id"), col("t1.id")));
    let ml_t2 = join(ml_t1, t2_kt2, eq(col("ml.linked_movie_id"), col("t2.id")));
    let t1_mi1 = join(ml_t2, scan("movie_info_idx"), eq(col("t1.id"), col("mi_idx1.movie_id")));
    let t1_mi1_it1 = join(t1_mi1, it1, eq(col("mi_idx1.info_type_id"), col("it1.id")));
    let t2_mi2 = join(t1_mi1_it1, scan("movie_info_idx"), eq(col("t2.id"), col("mi_idx2.movie_id")));
    let t2_mi2_it2 = join(t2_mi2, it2, eq(col("mi_idx2.info_type_id"), col("it2.id")));
    let t1_mc1 = join(t2_mi2_it2, scan("movie_companies"), eq(col("t1.id"), col("mc1.movie_id")));
    let t1_mc1_cn1 = join(t1_mc1, cn1, eq(col("mc1.company_id"), col("cn1.id")));
    let t2_mc2 = join(t1_mc1_cn1, scan("movie_companies"), eq(col("t2.id"), col("mc2.movie_id")));
    let full = join(t2_mc2, scan("company_name"), eq(col("mc2.company_id"), col("cn2.id")));
    agg(full, vec![
        min_agg(col("cn1.name")), min_agg(col("cn2.name")),
        min_agg(col("mi_idx1.info")), min_agg(col("mi_idx2.info")),
        min_agg(col("t1.title")), min_agg(col("t2.title")),
    ])
}

fn q33a() -> RelExpr { q33_base("[us]", 2005) }
fn q33b() -> RelExpr { q33_base("[nl]", 2007) }
fn q33c() -> RelExpr { q33_base("[us]", 2000) }

// ── benchmark ───────────────────────────────────────────────────

type QueryFn = fn() -> RelExpr;

const QUERIES: &[(&str, QueryFn)] = &[
    ("01a", q1a as QueryFn), ("01b", q1b), ("01c", q1c), ("01d", q1d),
    ("02a", q2a), ("02b", q2b), ("02c", q2c), ("02d", q2d),
    ("03a", q3a), ("03b", q3b), ("03c", q3c),
    ("04a", q4a), ("04b", q4b), ("04c", q4c),
    ("05a", q5a), ("05b", q5b), ("05c", q5c),
    ("06a", q6a), ("06b", q6b), ("06c", q6c), ("06d", q6d), ("06e", q6e), ("06f", q6f),
    ("07a", q7a), ("07b", q7b), ("07c", q7c),
    ("08a", q8a), ("08b", q8b), ("08c", q8c), ("08d", q8d),
    ("09a", q9a), ("09b", q9b), ("09c", q9c), ("09d", q9d),
    ("10a", q10a), ("10b", q10b), ("10c", q10c),
    ("11a", q11a), ("11b", q11b), ("11c", q11c), ("11d", q11d),
    ("12a", q12a), ("12b", q12b), ("12c", q12c),
    ("13a", q13a), ("13b", q13b), ("13c", q13c), ("13d", q13d),
    ("14a", q14a), ("14b", q14b), ("14c", q14c),
    ("15a", q15a), ("15b", q15b), ("15c", q15c), ("15d", q15d),
    ("16a", q16a), ("16b", q16b), ("16c", q16c), ("16d", q16d),
    ("17a", q17a), ("17b", q17b), ("17c", q17c), ("17d", q17d), ("17e", q17e), ("17f", q17f),
    ("18a", q18a), ("18b", q18b), ("18c", q18c),
    ("19a", q19a), ("19b", q19b), ("19c", q19c), ("19d", q19d),
    ("20a", q20a), ("20b", q20b), ("20c", q20c),
    ("21a", q21a), ("21b", q21b), ("21c", q21c),
    ("22a", q22a), ("22b", q22b), ("22c", q22c), ("22d", q22d),
    ("23a", q23a), ("23b", q23b), ("23c", q23c),
    ("24a", q24a), ("24b", q24b),
    ("25a", q25a), ("25b", q25b), ("25c", q25c),
    ("26a", q26a), ("26b", q26b), ("26c", q26c),
    ("27a", q27a), ("27b", q27b), ("27c", q27c),
    ("28a", q28a), ("28b", q28b), ("28c", q28c),
    ("29a", q29a), ("29b", q29b), ("29c", q29c),
    ("30a", q30a), ("30b", q30b), ("30c", q30c),
    ("31a", q31a), ("31b", q31b), ("31c", q31c),
    ("32a", q32a), ("32b", q32b),
    ("33a", q33a), ("33b", q33b), ("33c", q33c),
];

fn bench_job_optimize_all(c: &mut Criterion) {
    let optimizer = make_optimizer();
    let facts = EmptyFactsProvider::new();
    let mut group = c.benchmark_group("job_optimize");

    for (name, query_fn) in QUERIES {
        let plan = query_fn();
        group.bench_with_input(
            BenchmarkId::new("optimize", name),
            &plan,
            |b, p| {
                b.iter(|| {
                    let _ = black_box(
                        optimizer.optimize_with_facts(p, &facts),
                    );
                });
            },
        );
    }
    group.finish();
}

fn bench_job_by_category(c: &mut Criterion) {
    let optimizer = make_optimizer();
    let facts = EmptyFactsProvider::new();

    // Simple: 4-5 tables (queries 1-6)
    let mut simple = c.benchmark_group("job_simple");
    for (name, query_fn) in QUERIES.iter().filter(|(n, _)| {
        n.starts_with("01") || n.starts_with("02") || n.starts_with("03")
            || n.starts_with("04") || n.starts_with("05") || n.starts_with("06")
    }) {
        let plan = query_fn();
        simple.bench_with_input(
            BenchmarkId::new("optimize", name),
            &plan,
            |b, p| { b.iter(|| { let _ = black_box(optimizer.optimize_with_facts(p, &facts)); }); },
        );
    }
    simple.finish();

    // Medium: 7-9 tables (queries 7-18)
    let mut medium = c.benchmark_group("job_medium");
    for (name, query_fn) in QUERIES.iter().filter(|(n, _)| {
        let num: u32 = n[..2].parse().unwrap_or(0);
        (7..=18).contains(&num)
    }) {
        let plan = query_fn();
        medium.bench_with_input(
            BenchmarkId::new("optimize", name),
            &plan,
            |b, p| { b.iter(|| { let _ = black_box(optimizer.optimize_with_facts(p, &facts)); }); },
        );
    }
    medium.finish();

    // Complex: 10+ tables (queries 19-33)
    let mut complex = c.benchmark_group("job_complex");
    for (name, query_fn) in QUERIES.iter().filter(|(n, _)| {
        let num: u32 = n[..2].parse().unwrap_or(0);
        num >= 19
    }) {
        let plan = query_fn();
        complex.bench_with_input(
            BenchmarkId::new("optimize", name),
            &plan,
            |b, p| { b.iter(|| { let _ = black_box(optimizer.optimize_with_facts(p, &facts)); }); },
        );
    }
    complex.finish();
}

criterion_group!(
    benches,
    bench_job_optimize_all,
    bench_job_by_category,
);
criterion_main!(benches);
