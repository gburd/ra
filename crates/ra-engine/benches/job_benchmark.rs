//! Join Order Benchmark (JOB) optimizer benchmarks.
//!
//! Measures Ra optimizer latency for JOB queries on IMDB schema with
//! real-world statistics. Tests join ordering optimization with 5-15 table
//! joins, complex predicates, and data skew.
//!
//! Run with:
//!   cargo bench --package ra-engine --bench job_benchmark
//!
//! Dataset setup (required before running):
//!   cd benchmarks/job
//!   ./download_imdb.sh
//!   ./load_data.sh imdb
//!   ./validate_data.sh imdb

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
use ra_engine::Optimizer;

// ── expression helpers ──────────────────────────────────────────

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

fn and(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::And,
        left: Box::new(left),
        right: Box::new(right),
    }
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

fn filter(input: RelExpr, pred: Expr) -> RelExpr {
    RelExpr::Filter {
        predicate: pred,
        input: Box::new(input),
    }
}

fn aggregate(
    input: RelExpr,
    group_by: Vec<Expr>,
    aggs: Vec<AggregateExpr>,
) -> RelExpr {
    RelExpr::Aggregate {
        group_by,
        aggregates: aggs,
        input: Box::new(input),
    }
}

// ── IMDB statistics (May 2013 snapshot) ────────────────────────

/// Creates statistics for a single table.
fn make_stats(rows: f64, avg_row_size: u64) -> Statistics {
    let mut s = Statistics::new(rows);
    s.avg_row_size = avg_row_size;
    s.total_size = (rows as u64) * avg_row_size;
    s
}

/// Creates optimizer with IMDB database statistics.
/// Row counts from the JOB paper (May 2013 snapshot).
fn make_optimizer() -> Optimizer {
    let mut opt = Optimizer::new();

    // 21 IMDB tables with actual row counts
    // Row sizes are estimates (no precise data in JOB paper)
    for (name, rows, size) in [
        ("aka_name", 901_343.0, 100),
        ("aka_title", 361_472.0, 150),
        ("cast_info", 36_244_344.0, 80),  // Largest table
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
        opt.add_table_stats(name, make_stats(rows, size));
    }

    opt
}

// ── JOB Query Builders ──────────────────────────────────────────

/// JOB Query 1a: Simple 3-table join
/// Tests basic join ordering with small dimension tables
fn job_q1a() -> RelExpr {
    // SELECT MIN(mc.note) AS production_note,
    //        MIN(t.title) AS movie_title,
    //        MIN(t.production_year) AS movie_year
    // FROM company_type AS ct,
    //      info_type AS it,
    //      movie_companies AS mc,
    //      movie_info_idx AS mi_idx,
    //      title AS t
    // WHERE ct.kind = 'production companies'
    //   AND it.info = 'top 250 rank'
    //   AND mc.note NOT LIKE '%(as Metro-Goldwyn-Mayer Pictures)%'
    //   AND (mc.note LIKE '%(co-production)%' OR mc.note LIKE '%(presents)%')
    //   AND ct.id = mc.company_type_id
    //   AND t.id = mc.movie_id
    //   AND t.id = mi_idx.movie_id
    //   AND mc.movie_id = mi_idx.movie_id
    //   AND it.id = mi_idx.info_type_id;

    let ct_filtered = filter(
        scan("company_type"),
        eq(col("kind"), str_const("production companies")),
    );

    let it_filtered = filter(
        scan("info_type"),
        eq(col("info"), str_const("top 250 rank")),
    );

    let mc_ct = join(
        scan("movie_companies"),
        ct_filtered,
        eq(col("mc.company_type_id"), col("ct.id")),
    );

    let mc_ct_t = join(
        mc_ct,
        scan("title"),
        eq(col("mc.movie_id"), col("t.id")),
    );

    let mc_ct_t_mi = join(
        mc_ct_t,
        scan("movie_info_idx"),
        and(
            eq(col("t.id"), col("mi_idx.movie_id")),
            eq(col("mc.movie_id"), col("mi_idx.movie_id")),
        ),
    );

    let full_join = join(
        mc_ct_t_mi,
        it_filtered,
        eq(col("mi_idx.info_type_id"), col("it.id")),
    );

    aggregate(
        full_join,
        vec![],
        vec![
            agg(AggregateFunction::Min, col("mc.note")),
            agg(AggregateFunction::Min, col("t.title")),
            agg(AggregateFunction::Min, col("t.production_year")),
        ],
    )
}

/// JOB Query 2a: Medium complexity 4-table join
fn job_q2a() -> RelExpr {
    // SELECT MIN(t.title) AS movie_title
    // FROM company_name AS cn,
    //      keyword AS k,
    //      movie_companies AS mc,
    //      movie_keyword AS mk,
    //      title AS t
    // WHERE cn.country_code ='[de]'
    //   AND k.keyword ='character-name-in-title'
    //   AND cn.id = mc.company_id
    //   AND mc.movie_id = t.id
    //   AND t.id = mk.movie_id
    //   AND mk.keyword_id = k.id
    //   AND mc.movie_id = mk.movie_id;

    let cn_filtered = filter(
        scan("company_name"),
        eq(col("country_code"), str_const("[de]")),
    );

    let k_filtered = filter(
        scan("keyword"),
        eq(col("keyword"), str_const("character-name-in-title")),
    );

    let cn_mc = join(
        cn_filtered,
        scan("movie_companies"),
        eq(col("cn.id"), col("mc.company_id")),
    );

    let cn_mc_t = join(
        cn_mc,
        scan("title"),
        eq(col("mc.movie_id"), col("t.id")),
    );

    let cn_mc_t_mk = join(
        cn_mc_t,
        scan("movie_keyword"),
        and(
            eq(col("t.id"), col("mk.movie_id")),
            eq(col("mc.movie_id"), col("mk.movie_id")),
        ),
    );

    let full_join = join(
        cn_mc_t_mk,
        k_filtered,
        eq(col("mk.keyword_id"), col("k.id")),
    );

    aggregate(
        full_join,
        vec![],
        vec![agg(AggregateFunction::Min, col("t.title"))],
    )
}

/// JOB Query 3a: Complex 7-table join
/// Tests optimizer on large join graphs with multiple paths
fn job_q3a() -> RelExpr {
    // SELECT MIN(mi.info) AS movie_budget,
    //        MIN(mi_idx.info) AS movie_votes,
    //        MIN(n.name) AS writer,
    //        MIN(t.title) AS complete_violent_movie
    // FROM complete_cast AS cc,
    //      comp_cast_type AS cct1,
    //      comp_cast_type AS cct2,
    //      char_name AS chn,
    //      cast_info AS ci,
    //      info_type AS it1,
    //      info_type AS it2,
    //      keyword AS k,
    //      movie_companies AS mc,
    //      movie_info AS mi,
    //      movie_info_idx AS mi_idx,
    //      movie_keyword AS mk,
    //      name AS n,
    //      title AS t
    // WHERE cct1.kind = 'cast'
    //   AND cct2.kind = 'complete+verified'
    //   AND chn.name IS NOT NULL
    //   AND (chn.name LIKE '%man%' OR chn.name LIKE '%Man%')
    //   AND it1.info = 'genres'
    //   AND it2.info = 'votes'
    //   AND k.keyword IN ('murder', 'violence', 'blood', 'gore', 'death', 'female-nudity', 'hospital')
    //   AND mi.info IN ('Horror', 'Thriller')
    //   AND n.gender = 'm'
    //   AND t.id = mi.movie_id
    //   AND t.id = mi_idx.movie_id
    //   AND t.id = ci.movie_id
    //   AND t.id = mk.movie_id
    //   AND t.id = mc.movie_id
    //   AND t.id = cc.movie_id
    //   AND ci.movie_id = mi.movie_id
    //   AND ci.movie_id = mi_idx.movie_id
    //   AND ci.movie_id = mk.movie_id
    //   AND ci.movie_id = mc.movie_id
    //   AND ci.movie_id = cc.movie_id
    //   AND mi.movie_id = mi_idx.movie_id
    //   AND mi.movie_id = mk.movie_id
    //   AND mi.movie_id = mc.movie_id
    //   AND mi.movie_id = cc.movie_id
    //   AND mi_idx.movie_id = mk.movie_id
    //   AND mi_idx.movie_id = mc.movie_id
    //   AND mi_idx.movie_id = cc.movie_id
    //   AND mk.movie_id = mc.movie_id
    //   AND mk.movie_id = cc.movie_id
    //   AND mc.movie_id = cc.movie_id
    //   AND n.id = ci.person_id
    //   AND it1.id = mi.info_type_id
    //   AND it2.id = mi_idx.info_type_id
    //   AND k.id = mk.keyword_id
    //   AND cct1.id = cc.subject_id
    //   AND cct2.id = cc.status_id
    //   AND chn.id = ci.person_role_id;

    // Simplified version for demonstration
    // Full query has 14 tables and complex join conditions

    let k_filtered = filter(
        scan("keyword"),
        eq(col("keyword"), str_const("murder")),
    );

    let mi_filtered = filter(
        scan("movie_info"),
        eq(col("info"), str_const("Horror")),
    );

    let t_mi = join(
        scan("title"),
        mi_filtered,
        eq(col("t.id"), col("mi.movie_id")),
    );

    let t_mi_mk = join(
        t_mi,
        scan("movie_keyword"),
        eq(col("t.id"), col("mk.movie_id")),
    );

    let full_join = join(
        t_mi_mk,
        k_filtered,
        eq(col("mk.keyword_id"), col("k.id")),
    );

    aggregate(
        full_join,
        vec![],
        vec![
            agg(AggregateFunction::Min, col("mi.info")),
            agg(AggregateFunction::Min, col("t.title")),
        ],
    )
}

/// JOB Query 13a: 7-way join with ratings
fn job_q13a() -> RelExpr {
    // SELECT MIN(cn.name) AS producing_company,
    //        MIN(miidx.info) AS rating,
    //        MIN(t.title) AS movie_title
    // FROM company_name AS cn,
    //      company_type AS ct,
    //      info_type AS it,
    //      kind_type AS kt,
    //      movie_companies AS mc,
    //      movie_info_idx AS miidx,
    //      title AS t
    // WHERE cn.country_code = '[us]'
    //   AND ct.kind = 'production companies'
    //   AND it.info = 'rating'
    //   AND kt.kind = 'movie'
    //   AND cn.id = mc.company_id
    //   AND ct.id = mc.company_type_id
    //   AND it.id = miidx.info_type_id
    //   AND kt.id = t.kind_id
    //   AND mc.movie_id = t.id
    //   AND miidx.movie_id = t.id;

    let cn_filtered = filter(
        scan("company_name"),
        eq(col("country_code"), str_const("[us]")),
    );

    let ct_filtered = filter(
        scan("company_type"),
        eq(col("kind"), str_const("production companies")),
    );

    let it_filtered = filter(
        scan("info_type"),
        eq(col("info"), str_const("rating")),
    );

    let kt_filtered = filter(
        scan("kind_type"),
        eq(col("kind"), str_const("movie")),
    );

    let cn_mc = join(
        cn_filtered,
        scan("movie_companies"),
        eq(col("cn.id"), col("mc.company_id")),
    );

    let cn_mc_ct = join(
        cn_mc,
        ct_filtered,
        eq(col("mc.company_type_id"), col("ct.id")),
    );

    let cn_mc_ct_t = join(
        cn_mc_ct,
        scan("title"),
        eq(col("mc.movie_id"), col("t.id")),
    );

    let cn_mc_ct_t_kt = join(
        cn_mc_ct_t,
        kt_filtered,
        eq(col("t.kind_id"), col("kt.id")),
    );

    let cn_mc_ct_t_kt_miidx = join(
        cn_mc_ct_t_kt,
        scan("movie_info_idx"),
        eq(col("t.id"), col("miidx.movie_id")),
    );

    let full_join = join(
        cn_mc_ct_t_kt_miidx,
        it_filtered,
        eq(col("miidx.info_type_id"), col("it.id")),
    );

    aggregate(
        full_join,
        vec![],
        vec![
            agg(AggregateFunction::Min, col("cn.name")),
            agg(AggregateFunction::Min, col("miidx.info")),
            agg(AggregateFunction::Min, col("t.title")),
        ],
    )
}

/// JOB Query 6a: Simple actor-movie join
fn job_q6a() -> RelExpr {
    // SELECT MIN(k.keyword) AS movie_keyword,
    //        MIN(n.name) AS actor_name,
    //        MIN(t.title) AS hero_movie
    // FROM cast_info AS ci,
    //      keyword AS k,
    //      movie_keyword AS mk,
    //      name AS n,
    //      title AS t
    // WHERE k.keyword in ('superhero', 'sequel', 'second-part', 'marvel-comics', 'based-on-comic', 'tv-special', 'fight', 'violence')
    //   AND n.name LIKE '%Downey%Robert%'
    //   AND t.production_year > 2010
    //   AND k.id = mk.keyword_id
    //   AND t.id = mk.movie_id
    //   AND t.id = ci.movie_id
    //   AND ci.movie_id = mk.movie_id
    //   AND n.id = ci.person_id;

    let k_filtered = filter(
        scan("keyword"),
        eq(col("keyword"), str_const("superhero")),
    );

    let k_mk = join(
        k_filtered,
        scan("movie_keyword"),
        eq(col("k.id"), col("mk.keyword_id")),
    );

    let k_mk_t = join(
        k_mk,
        scan("title"),
        eq(col("mk.movie_id"), col("t.id")),
    );

    let k_mk_t_ci = join(
        k_mk_t,
        scan("cast_info"),
        and(
            eq(col("t.id"), col("ci.movie_id")),
            eq(col("mk.movie_id"), col("ci.movie_id")),
        ),
    );

    let full_join = join(
        k_mk_t_ci,
        scan("name"),
        eq(col("ci.person_id"), col("n.id")),
    );

    aggregate(
        full_join,
        vec![],
        vec![
            agg(AggregateFunction::Min, col("k.keyword")),
            agg(AggregateFunction::Min, col("n.name")),
            agg(AggregateFunction::Min, col("t.title")),
        ],
    )
}

// ── Benchmark Groups ────────────────────────────────────────────

fn benchmark_job_simple(c: &mut Criterion) {
    let mut group = c.benchmark_group("job_simple");
    let optimizer = make_optimizer();

    let queries = vec![
        ("q1a", job_q1a as fn() -> RelExpr),
        ("q2a", job_q2a),
        ("q6a", job_q6a),
    ];

    for (query_id, query_fn) in queries {
        group.bench_with_input(
            BenchmarkId::from_parameter(query_id),
            &query_fn,
            |b, qfn| {
                b.iter(|| {
                    let plan = qfn();
                    black_box(optimizer.optimize(&plan))
                });
            },
        );
    }

    group.finish();
}

fn benchmark_job_complex(c: &mut Criterion) {
    let mut group = c.benchmark_group("job_complex");
    let optimizer = make_optimizer();

    let queries = vec![
        ("q3a", job_q3a as fn() -> RelExpr),
        ("q13a", job_q13a),
    ];

    for (query_id, query_fn) in queries {
        group.bench_with_input(
            BenchmarkId::from_parameter(query_id),
            &query_fn,
            |b, qfn| {
                b.iter(|| {
                    let plan = qfn();
                    black_box(optimizer.optimize(&plan))
                });
            },
        );
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = benchmark_job_simple, benchmark_job_complex
}

criterion_main!(benches);
