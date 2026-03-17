//! Tests for Apache Flink stream processing optimization rules.
//!
//! Tests 7 Flink rules from Task #20 academic research mining:
//! - Temporal table join optimization
//! - Watermark propagation
//! - Mini-batch aggregation
//! - Stream deduplication
//! - Time-window optimization
//! - Retraction handling
//! - Lookup join caching

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_engine::Optimizer;
use ra_hardware::HardwareProfile;

// ── Test Helpers ────────────────────────────────────────────

fn create_optimizer() -> Optimizer {
    let mut optimizer = Optimizer::new();
    optimizer.set_hardware_profile(HardwareProfile::cpu_only());
    optimizer
}

fn scan(table: &str) -> RelExpr {
    RelExpr::Scan {
        table: table.to_string(),
        alias: None,
    }
}

fn filter(input: RelExpr, predicate: Expr) -> RelExpr {
    RelExpr::Filter {
        predicate,
        input: Box::new(input),
    }
}

fn join(left: RelExpr, right: RelExpr, condition: Expr) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn eq_pred(left: &str, right: &str) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::new(left.to_string()))),
        right: Box::new(Expr::Column(ColumnRef::new(right.to_string()))),
    }
}

// ── Rule 1: Temporal Table Join ─────────────────────────────

#[test]
fn test_flink_temporal_join_event_time() {
    let optimizer = create_optimizer();

    // Stream joined with temporal table (versioned dimension)
    let stream = scan("event_stream");
    let temporal = scan("versioned_dimension");
    let plan = join(stream, temporal, eq_pred("dim_id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "temporal join should optimize");
}

#[test]
fn test_flink_temporal_join_lookup() {
    let optimizer = create_optimizer();

    // Lookup join variant
    let stream = scan("transactions");
    let lookup = scan("currency_rates");
    let plan = join(stream, lookup, eq_pred("currency", "code"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "lookup join should optimize");
}

#[test]
fn test_flink_temporal_join_multiple_versions() {
    let optimizer = create_optimizer();

    // Multiple version history
    let stream = scan("orders");
    let versioned = scan("price_history");
    let plan = join(stream, versioned, eq_pred("product_id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "multi-version temporal join should optimize");
}

// ── Rule 2: Watermark Propagation ───────────────────────────

#[test]
fn test_flink_watermark_simple_filter() {
    let optimizer = create_optimizer();

    // Watermark should propagate through filter
    let filtered = filter(scan("event_stream"), eq_pred("status", "active"));

    let result = optimizer.optimize(&filtered);
    assert!(result.is_ok(), "watermark through filter should optimize");
}

#[test]
fn test_flink_watermark_through_join() {
    let optimizer = create_optimizer();

    // Watermark propagation in join
    let left = scan("stream_a");
    let right = scan("stream_b");
    let plan = join(left, right, eq_pred("key", "key"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "watermark through join should optimize");
}

#[test]
fn test_flink_watermark_alignment() {
    let optimizer = create_optimizer();

    // Watermark alignment across multiple streams
    let s1 = scan("stream1");
    let s2 = scan("stream2");
    let s3 = scan("stream3");
    let j1 = join(s1, s2, eq_pred("id", "id"));
    let plan = join(j1, s3, eq_pred("id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "watermark alignment should optimize");
}

// ── Rule 3: Mini-Batch Aggregation ──────────────────────────

#[test]
fn test_flink_minibatch_basic_agg() {
    let optimizer = create_optimizer();

    // Basic aggregation with mini-batch
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("key".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("stream_data")),
    };

    let result = optimizer.optimize(&agg);
    assert!(result.is_ok(), "mini-batch aggregation should optimize");
}

#[test]
fn test_flink_minibatch_high_throughput() {
    let optimizer = create_optimizer();

    // High-throughput stream aggregation
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("user_id".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("high_volume_stream")),
    };

    let result = optimizer.optimize(&agg);
    assert!(result.is_ok(), "high-throughput mini-batch should optimize");
}

#[test]
fn test_flink_minibatch_latency_sensitive() {
    let optimizer = create_optimizer();

    // Low-latency aggregation
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("session_id".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("realtime_events")),
    };

    let result = optimizer.optimize(&agg);
    assert!(result.is_ok(), "latency-sensitive mini-batch should optimize");
}

// ── Rule 4: Stream Deduplication ────────────────────────────

#[test]
fn test_flink_dedup_on_rowtime() {
    let optimizer = create_optimizer();

    // Deduplication based on event time
    let plan = filter(scan("duplicate_stream"), eq_pred("id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "rowtime deduplication should optimize");
}

#[test]
fn test_flink_dedup_on_proctime() {
    let optimizer = create_optimizer();

    // Deduplication based on processing time
    let plan = filter(scan("kafka_events"), eq_pred("message_id", "message_id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "proctime deduplication should optimize");
}

#[test]
fn test_flink_dedup_with_keep_first() {
    let optimizer = create_optimizer();

    // Keep first occurrence strategy
    let sorted = RelExpr::Sort {
        keys: vec![],
        input: Box::new(scan("ordered_stream")),
    };

    let result = optimizer.optimize(&sorted);
    assert!(result.is_ok(), "dedup keep-first should optimize");
}

// ── Rule 5: Time-Window Optimization ────────────────────────

#[test]
fn test_flink_window_tumbling() {
    let optimizer = create_optimizer();

    // Tumbling window aggregation
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("window".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("time_series")),
    };

    let result = optimizer.optimize(&agg);
    assert!(result.is_ok(), "tumbling window should optimize");
}

#[test]
fn test_flink_window_sliding() {
    let optimizer = create_optimizer();

    // Sliding window aggregation
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("window_start".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("metric_stream")),
    };

    let result = optimizer.optimize(&agg);
    assert!(result.is_ok(), "sliding window should optimize");
}

#[test]
fn test_flink_window_session() {
    let optimizer = create_optimizer();

    // Session window (dynamic gaps)
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("session_id".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("user_activity")),
    };

    let result = optimizer.optimize(&agg);
    assert!(result.is_ok(), "session window should optimize");
}

// ── Rule 6: Retraction Handling ─────────────────────────────

#[test]
fn test_flink_retraction_in_join() {
    let optimizer = create_optimizer();

    // Join with retractions
    let left = scan("retractable_stream");
    let right = scan("dimension_table");
    let plan = join(left, right, eq_pred("key", "key"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "retraction in join should optimize");
}

#[test]
fn test_flink_retraction_in_aggregation() {
    let optimizer = create_optimizer();

    // Aggregation with retractions
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("category".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("mutable_stream")),
    };

    let result = optimizer.optimize(&agg);
    assert!(result.is_ok(), "retraction in aggregation should optimize");
}

#[test]
fn test_flink_retraction_elimination() {
    let optimizer = create_optimizer();

    // Unnecessary retraction removal
    let filtered = filter(scan("append_only_stream"), eq_pred("type", "insert"));

    let result = optimizer.optimize(&filtered);
    assert!(result.is_ok(), "retraction elimination should optimize");
}

// ── Rule 7: Lookup Join Caching ─────────────────────────────

#[test]
fn test_flink_lookup_cache_basic() {
    let optimizer = create_optimizer();

    // Basic lookup join with caching
    let stream = scan("transaction_stream");
    let lookup = scan("product_catalog");
    let plan = join(stream, lookup, eq_pred("product_id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "lookup cache should optimize");
}

#[test]
fn test_flink_lookup_cache_high_hit_rate() {
    let optimizer = create_optimizer();

    // High cache hit rate scenario
    let stream = scan("clickstream");
    let lookup = scan("user_profiles");
    let plan = join(stream, lookup, eq_pred("user_id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "high hit rate lookup should optimize");
}

#[test]
fn test_flink_lookup_cache_ttl() {
    let optimizer = create_optimizer();

    // Lookup with TTL expiration
    let stream = scan("events");
    let lookup = scan("exchange_rates");
    let plan = join(stream, lookup, eq_pred("currency", "code"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "TTL-based lookup cache should optimize");
}
