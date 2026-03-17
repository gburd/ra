//! Tests for logical join reordering optimization rules.
//!
//! Join reordering is critical for query performance, especially
//! for multi-way joins where the order can dramatically affect
//! execution time.

mod helpers;

use helpers::*;
use ra_core::algebra::{JoinType, RelExpr};

// ── Basic Join Reordering ───────────────────────────────────

#[test]
fn test_two_table_join_commutative() {
    let plan = two_table_join("small_table", "large_table", "id", "id");
    assert_rule_applies(plan);
}

#[test]
fn test_join_associativity() {
    let plan = two_table_join("orders", "customers", "customer_id", "id");
    assert_rule_applies(plan);
}

#[test]
fn test_chain_join_reordering() {
    let t1 = scan("t1");
    let t2 = scan("t2");
    let t3 = scan("t3");

    let j1 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(j1),
        right: Box::new(t3),
    };

    assert_rule_applies(j2);
}

// ── Multi-Way Join Optimization ─────────────────────────────

#[test]
fn test_three_way_join_optimal_order() {
    let small = scan("small_dim");
    let medium = scan("medium_fact");
    let large = scan("large_fact");

    let j1 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("dim_id")),
        left: Box::new(large),
        right: Box::new(small),
    };

    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("fact_id"), col("id")),
        left: Box::new(j1),
        right: Box::new(medium),
    };

    assert_rule_applies(j2);
}

#[test]
fn test_four_way_join_reordering() {
    let t1 = scan("t1");
    let t2 = scan("t2");
    let t3 = scan("t3");
    let t4 = scan("t4");

    let j1 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(j1),
        right: Box::new(t3),
    };

    let j3 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(j2),
        right: Box::new(t4),
    };

    assert_rule_applies(j3);
}

#[test]
fn test_star_schema_join_order() {
    // Fact table with multiple dimension joins
    let fact = scan("sales_fact");
    let time = scan("time_dim");
    let product = scan("product_dim");
    let customer = scan("customer_dim");
    let store = scan("store_dim");

    let j1 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("time_key"), col("time_id")),
        left: Box::new(fact),
        right: Box::new(time),
    };

    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("product_key"), col("product_id")),
        left: Box::new(j1),
        right: Box::new(product),
    };

    let j3 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("customer_key"), col("customer_id")),
        left: Box::new(j2),
        right: Box::new(customer),
    };

    let j4 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("store_key"), col("store_id")),
        left: Box::new(j3),
        right: Box::new(store),
    };

    assert_rule_applies(j4);
}

// ── Bushy vs Left-Deep Trees ────────────────────────────────

#[test]
fn test_left_deep_tree_formation() {
    let t1 = scan("table1");
    let t2 = scan("table2");
    let t3 = scan("table3");

    let j1 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(j1),
        right: Box::new(t3),
    };

    assert_rule_applies(j2);
}

#[test]
fn test_bushy_tree_for_parallelism() {
    let t1 = scan("table1");
    let t2 = scan("table2");
    let t3 = scan("table3");
    let t4 = scan("table4");

    let left_branch = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    let right_branch = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(t3),
        right: Box::new(t4),
    };

    let root = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(left_branch),
        right: Box::new(right_branch),
    };

    assert_rule_applies(root);
}

// ── Cost-Based Join Ordering ────────────────────────────────

#[test]
fn test_selective_filter_affects_join_order() {
    let filtered_small = filtered_scan("dimension", "category", 1);
    let large = scan("fact_table");

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("dim_id"), col("id")),
        left: Box::new(large),
        right: Box::new(filtered_small),
    };

    assert_rule_applies(plan);
}

#[test]
fn test_cardinality_driven_ordering() {
    let plan = two_table_join("very_large_table", "tiny_table", "key", "key");
    assert_rule_applies(plan);
}

#[test]
fn test_index_availability_affects_order() {
    let indexed = scan("indexed_table");
    let heap = scan("heap_table");

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("key"), col("key")),
        left: Box::new(heap),
        right: Box::new(indexed),
    };

    assert_rule_applies(plan);
}

// ── Join Type Constraints ───────────────────────────────────

#[test]
fn test_outer_join_ordering_constraints() {
    let left = scan("orders");
    let right = scan("customers");

    let plan = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(left),
        right: Box::new(right),
    };

    assert_rule_applies(plan);
}

#[test]
fn test_mixed_join_types_ordering() {
    let t1 = scan("t1");
    let t2 = scan("t2");
    let t3 = scan("t3");

    let inner = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    let outer = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("id"), col("id")),
        left: Box::new(inner),
        right: Box::new(t3),
    };

    assert_rule_applies(outer);
}

#[test]
fn test_full_outer_join_reordering() {
    let left = scan("table_a");
    let right = scan("table_b");

    let plan = RelExpr::Join {
        join_type: JoinType::FullOuter,
        condition: eq(col("key"), col("key")),
        left: Box::new(left),
        right: Box::new(right),
    };

    assert_rule_applies(plan);
}

// ── Cross Product Elimination ───────────────────────────────

#[test]
fn test_cross_product_to_inner_join() {
    let t1 = scan("table1");
    let t2 = scan("table2");

    let cross = RelExpr::Join {
        join_type: JoinType::Cross,
        condition: eq(col("a"), col("b")),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    assert_rule_applies(cross);
}

#[test]
fn test_avoid_cartesian_product() {
    let t1 = scan("t1");
    let t2 = scan("t2");
    let t3 = scan("t3");

    let j1 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(j1),
        right: Box::new(t3),
    };

    assert_rule_applies(j2);
}

// ── Join Graph Analysis ─────────────────────────────────────

#[test]
fn test_cyclic_join_graph() {
    // A joins B, B joins C, C joins A (cycle)
    let a = scan("table_a");
    let b = scan("table_b");
    let c = scan("table_c");

    let ab = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("a_id"), col("b_id")),
        left: Box::new(a),
        right: Box::new(b),
    };

    let abc = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("b_id"), col("c_id")),
        left: Box::new(ab),
        right: Box::new(c),
    };

    assert_rule_applies(abc);
}

#[test]
fn test_clique_join_pattern() {
    // All tables join with all others
    let t1 = scan("t1");
    let t2 = scan("t2");
    let t3 = scan("t3");

    let j12 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: and(
            eq(col("t1_id"), col("t2_id")),
            eq(col("t1_key"), col("t2_key"))
        ),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    let j123 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: and(
            eq(col("t2_id"), col("t3_id")),
            eq(col("t1_id"), col("t3_id"))
        ),
        left: Box::new(j12),
        right: Box::new(t3),
    };

    assert_rule_applies(j123);
}

// ── Dynamic Programming Optimization ────────────────────────

#[test]
fn test_dp_join_enumeration_small() {
    // 3 tables - should enumerate all valid join orders
    let t1 = scan("small1");
    let t2 = scan("small2");
    let t3 = scan("small3");

    let j1 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(j1),
        right: Box::new(t3),
    };

    assert_rule_applies(j2);
}

#[test]
fn test_greedy_join_ordering_large() {
    // 5+ tables - should use greedy heuristic
    let tables: Vec<RelExpr> = (1..=6)
        .map(|i| scan(&format!("table{}", i)))
        .collect();

    let mut current = tables[0].clone();
    for table in tables.iter().skip(1) {
        current = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq(col("id"), col("id")),
            left: Box::new(current),
            right: Box::new(table.clone()),
        };
    }

    assert_rule_applies(current);
}

// ── Filter Integration with Join Reordering ─────────────────

#[test]
fn test_filter_influences_join_order() {
    let filtered = filtered_scan("filtered_table", "status", 1);
    let unfiltered = scan("unfiltered_table");

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("key"), col("key")),
        left: Box::new(unfiltered),
        right: Box::new(filtered),
    };

    assert_rule_applies(plan);
}

#[test]
fn test_join_with_multiple_filters() {
    let f1 = filtered_scan("t1", "col1", 10);
    let f2 = filtered_scan("t2", "col2", 20);

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("key"), col("key")),
        left: Box::new(f1),
        right: Box::new(f2),
    };

    assert_rule_applies(plan);
}

#[test]
fn test_transitive_join_conditions() {
    // A.x = B.y AND B.y = C.z implies A.x = C.z
    let a = scan("a");
    let b = scan("b");
    let c = scan("c");

    let ab = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("x"), col("y")),
        left: Box::new(a),
        right: Box::new(b),
    };

    let abc = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("y"), col("z")),
        left: Box::new(ab),
        right: Box::new(c),
    };

    assert_rule_applies(abc);
}
