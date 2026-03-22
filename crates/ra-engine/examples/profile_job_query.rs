//! Profile a single JOB query to measure optimizer performance.
//!
//! Run with:
//!   RUST_LOG=ra_engine=info cargo run --release --example profile_job_query

use ra_core::algebra::{AggregateExpr, AggregateFunction, JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::statistics::Statistics;
use ra_engine::Optimizer;

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

fn aggregate(input: RelExpr, group_by: Vec<Expr>, aggs: Vec<AggregateExpr>) -> RelExpr {
    RelExpr::Aggregate {
        group_by,
        aggregates: aggs,
        input: Box::new(input),
    }
}

fn make_stats(rows: f64, avg_row_size: u64) -> Statistics {
    let mut s = Statistics::new(rows);
    s.avg_row_size = avg_row_size;
    s.total_size = (rows as u64) * avg_row_size;
    s
}

fn make_optimizer() -> Optimizer {
    let mut opt = Optimizer::new();

    for (name, rows, size) in [
        ("aka_name", 901_343.0, 100),
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
        opt.add_table_stats(name, make_stats(rows, size));
    }

    opt
}

/// JOB Query 13a: 7-way join with ratings
fn job_q13a() -> RelExpr {
    let cn_filtered = filter(
        scan("company_name"),
        eq(col("country_code"), str_const("[us]")),
    );

    let ct_filtered = filter(
        scan("company_type"),
        eq(col("kind"), str_const("production companies")),
    );

    let it_filtered = filter(scan("info_type"), eq(col("info"), str_const("rating")));

    let kt_filtered = filter(scan("kind_type"), eq(col("kind"), str_const("movie")));

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

fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("Profiling JOB Query 13a (7-way join)");
    println!("=====================================\n");

    let optimizer = make_optimizer();
    let query = job_q13a();

    println!("Running optimization (this will take ~1 second)...\n");

    let start = std::time::Instant::now();
    match optimizer.optimize(&query) {
        Ok(_optimized) => {
            let elapsed = start.elapsed();
            println!("\n✓ Optimization complete in {:?}", elapsed);
        }
        Err(e) => {
            eprintln!("✗ Optimization failed: {}", e);
            std::process::exit(1);
        }
    }
}
