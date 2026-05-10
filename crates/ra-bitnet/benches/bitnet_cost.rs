use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ra_bitnet::BitNetCostModel;

fn sample_features() -> [f32; 12] {
    [
        4.0,      // table_count
        3.0,      // join_count
        5.0,      // filter_count
        1.0,      // aggregate_count
        0.0,      // subquery_count
        0.0,      // cte_count
        0.0,      // window_function_count
        2.0,      // order_by_count
        1.0,      // group_by_count
        0.0,      // distinct_flag
        0.0,      // limit_present
        10_000.0, // max_join_cardinality
    ]
}

fn create_test_model() -> BitNetCostModel {
    let mut w1 = [[0.0f32; 32]; 12];
    let mut w2 = [[0.0f32; 16]; 32];

    for j in 0..12 {
        for i in 0..32 {
            w1[j][i] = if (j + i) % 3 == 0 {
                0.5
            } else if (j + i) % 3 == 1 {
                -0.3
            } else {
                0.01
            };
        }
    }
    for i in 0..32 {
        for j in 0..16 {
            w2[i][j] = if (i + j) % 2 == 0 { 0.4 } else { -0.2 };
        }
    }

    BitNetCostModel::from_f32_weights(
        &w1,
        &[0.1; 32],
        &w2,
        &[0.05; 16],
        [0.0; 12],
        [1.0; 12],
        5000,
    )
}

fn bench_predict_cpu_ms(c: &mut Criterion) {
    let model = create_test_model();
    let features = sample_features();

    c.bench_function("bitnet_predict_cpu_ms", |b| {
        b.iter(|| model.predict_cpu_ms(black_box(&features)))
    });
}

fn bench_predict_all(c: &mut Criterion) {
    let model = create_test_model();
    let features = sample_features();

    c.bench_function("bitnet_predict_all", |b| {
        b.iter(|| model.predict_all(black_box(&features)))
    });
}

fn bench_predict_scalar(c: &mut Criterion) {
    let model = create_test_model();
    let features = sample_features();

    c.bench_function("bitnet_predict_scalar", |b| {
        b.iter(|| model.predict_scalar(black_box(&features)))
    });
}

criterion_group!(benches, bench_predict_cpu_ms, bench_predict_all, bench_predict_scalar);
criterion_main!(benches);
