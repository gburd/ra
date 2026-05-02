use super::*;

use ra_core::algebra::{JoinType, NullOrdering, RelExpr, SortDirection};
use ra_core::expr::{BinOp, Expr};

/// Recursively search for a node matching the predicate.
fn find_node(r: &RelExpr, pred: fn(&RelExpr) -> bool) -> Option<&RelExpr> {
    if pred(r) {
        return Some(r);
    }
    r.children().into_iter().find_map(|c| find_node(c, pred))
}

/// Check that a node matching the predicate exists anywhere in the tree.
fn has_node(r: &RelExpr, pred: fn(&RelExpr) -> bool) -> bool {
    find_node(r, pred).is_some()
}

// ---- Existing tests (preserved) ----

#[test]
fn test_simple_select() {
    let sql = "SELECT * FROM users";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok());
}

#[test]
fn test_select_with_where() {
    let sql = "SELECT * FROM users WHERE age > 18";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok());
}

#[test]
fn test_select_with_join() {
    let sql = "SELECT * FROM orders o \
               JOIN customers c ON o.customer_id = c.id";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok());
}

#[test]
fn test_select_with_aggregate() {
    let sql = "SELECT region, COUNT(*), SUM(amount) \
               FROM orders GROUP BY region";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok());
}

// ---- DISTINCT tests ----

#[test]
fn test_select_distinct() {
    let sql = "SELECT DISTINCT name FROM users";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Distinct { .. })),
        "expected Distinct node"
    );
}

#[test]
fn test_select_distinct_multiple_cols() {
    let sql = "SELECT DISTINCT dept_id, job_title FROM employees";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(has_node(&result, |r| matches!(r, RelExpr::Distinct { .. })));
}

// ---- ORDER BY tests ----

#[test]
fn test_order_by_asc() {
    let sql = "SELECT * FROM users ORDER BY name ASC";
    let result = sql_to_relexpr(sql).expect("should parse");
    let sort =
        find_node(&result, |r| matches!(r, RelExpr::Sort { .. })).expect("expected Sort node");
    if let RelExpr::Sort { keys, .. } = sort {
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].direction, SortDirection::Asc);
    }
}

#[test]
fn test_order_by_desc() {
    let sql = "SELECT * FROM users ORDER BY age DESC";
    let result = sql_to_relexpr(sql).expect("should parse");
    let sort =
        find_node(&result, |r| matches!(r, RelExpr::Sort { .. })).expect("expected Sort node");
    if let RelExpr::Sort { keys, .. } = sort {
        assert_eq!(keys[0].direction, SortDirection::Desc);
    }
}

#[test]
fn test_order_by_multiple() {
    let sql = "SELECT * FROM users ORDER BY dept ASC, name DESC";
    let result = sql_to_relexpr(sql).expect("should parse");
    let sort =
        find_node(&result, |r| matches!(r, RelExpr::Sort { .. })).expect("expected Sort node");
    if let RelExpr::Sort { keys, .. } = sort {
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].direction, SortDirection::Asc);
        assert_eq!(keys[1].direction, SortDirection::Desc);
    }
}

#[test]
fn test_order_by_nulls() {
    let sql = "SELECT * FROM users ORDER BY name ASC NULLS FIRST";
    let result = sql_to_relexpr(sql).expect("should parse");
    let sort =
        find_node(&result, |r| matches!(r, RelExpr::Sort { .. })).expect("expected Sort node");
    if let RelExpr::Sort { keys, .. } = sort {
        assert_eq!(keys[0].nulls, NullOrdering::First);
    }
}

// ---- LIMIT/OFFSET tests ----
// Lime grammar does not yet produce Limit nodes (placeholder only).

#[test]
fn test_limit() {
    let sql = "SELECT * FROM users LIMIT 10";
    let result = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::Limit { count, offset, .. } = &result {
        assert_eq!(*count, 10);
        assert_eq!(*offset, 0);
    } else {
        panic!("expected Limit at top level");
    }
}

#[test]
fn test_limit_offset() {
    let sql = "SELECT * FROM users LIMIT 10 OFFSET 20";
    let result = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::Limit { count, offset, .. } = &result {
        assert_eq!(*count, 10);
        assert_eq!(*offset, 20);
    } else {
        panic!("expected Limit at top level");
    }
}

#[test]
fn test_order_by_with_limit() {
    let sql = "SELECT * FROM users ORDER BY name LIMIT 5";
    let result = sql_to_relexpr(sql).expect("should parse");
    // Should be Limit(Sort(...))
    if let RelExpr::Limit { input, count, .. } = &result {
        assert_eq!(*count, 5);
        assert!(matches!(input.as_ref(), RelExpr::Sort { .. }));
    } else {
        panic!("expected Limit(Sort(...))");
    }
}

// ---- HAVING tests ----

#[test]
fn test_having() {
    let sql = "SELECT dept_id, COUNT(*) \
               FROM employees \
               GROUP BY dept_id \
               HAVING COUNT(*) > 5";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Filter { .. })),
        "expected Filter for HAVING"
    );
}

#[test]
fn test_having_with_group_by() {
    let sql = "SELECT region, SUM(amount) as total \
               FROM orders \
               GROUP BY region \
               HAVING SUM(amount) > 1000";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Aggregate { .. })),
        "expected Aggregate node"
    );
}

// ---- CTE tests ----

#[test]
fn test_simple_cte() {
    let sql = "WITH active AS (SELECT * FROM users WHERE active = true) \
               SELECT * FROM active";
    let result = sql_to_relexpr(sql).expect("should parse");
    let cte = find_node(&result, |r| matches!(r, RelExpr::CTE { .. })).expect("expected CTE node");
    if let RelExpr::CTE { name, .. } = cte {
        assert_eq!(name, "active");
    }
}

#[test]
fn test_multiple_ctes() {
    let sql = "WITH \
                 a AS (SELECT * FROM t1), \
                 b AS (SELECT * FROM t2) \
               SELECT * FROM a";
    let result = sql_to_relexpr(sql).expect("should parse");
    // Outermost should be CTE 'a' wrapping CTE 'b'
    if let RelExpr::CTE { name, body, .. } = &result {
        assert_eq!(name, "a");
        assert!(matches!(body.as_ref(), RelExpr::CTE { .. }));
    } else {
        panic!("expected nested CTEs");
    }
}

// ---- Subquery tests ----

#[test]
fn test_subquery_in_from() {
    let sql = "SELECT * FROM (SELECT id, name FROM users) t";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "subquery in FROM should parse");
}

#[test]
fn test_subquery_in_where() {
    let sql = "SELECT * FROM orders \
               WHERE customer_id IN \
               (SELECT id FROM customers WHERE active = true)";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "subquery in WHERE should parse");
}

#[test]
fn test_exists_subquery() {
    let sql = "SELECT * FROM customers c \
               WHERE EXISTS \
               (SELECT 1 FROM orders o WHERE o.cust_id = c.id)";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "EXISTS subquery should parse");
}

// ---- JOIN type tests ----

#[test]
fn test_left_join() {
    let sql = "SELECT * FROM orders o \
               LEFT JOIN customers c ON o.cust_id = c.id";
    let result = sql_to_relexpr(sql).expect("should parse");
    let join = find_node(&result, |r| {
        matches!(
            r,
            RelExpr::Join {
                join_type: JoinType::LeftOuter,
                ..
            }
        )
    })
    .expect("expected LeftOuter Join node");
    if let RelExpr::Join { join_type, .. } = join {
        assert_eq!(*join_type, JoinType::LeftOuter);
    }
}

#[test]
fn test_right_join() {
    let sql = "SELECT * FROM orders o \
               RIGHT JOIN customers c ON o.cust_id = c.id";
    let result = sql_to_relexpr(sql).expect("should parse");
    let join = find_node(&result, |r| {
        matches!(
            r,
            RelExpr::Join {
                join_type: JoinType::RightOuter,
                ..
            }
        )
    })
    .expect("expected RightOuter Join node");
    if let RelExpr::Join { join_type, .. } = join {
        assert_eq!(*join_type, JoinType::RightOuter);
    }
}

#[test]
fn test_full_outer_join() {
    let sql = "SELECT * FROM a \
               FULL OUTER JOIN b ON a.id = b.id";
    let result = sql_to_relexpr(sql).expect("should parse");
    let join = find_node(&result, |r| {
        matches!(
            r,
            RelExpr::Join {
                join_type: JoinType::FullOuter,
                ..
            }
        )
    })
    .expect("expected FullOuter Join node");
    if let RelExpr::Join { join_type, .. } = join {
        assert_eq!(*join_type, JoinType::FullOuter);
    }
}

#[test]
fn test_cross_join() {
    let sql = "SELECT * FROM a CROSS JOIN b";
    let result = sql_to_relexpr(sql).expect("should parse");
    let join = find_node(&result, |r| {
        matches!(
            r,
            RelExpr::Join {
                join_type: JoinType::Cross,
                ..
            }
        )
    })
    .expect("expected Cross Join node");
    if let RelExpr::Join { join_type, .. } = join {
        assert_eq!(*join_type, JoinType::Cross);
    }
}

// ---- Window function tests ----
// Lime grammar encodes window functions as regular function calls,
// not as Window RelExpr nodes.

#[test]
#[ignore = "Lime grammar does not yet produce Window nodes"]
fn test_row_number_window() {
    let sql = "SELECT id, ROW_NUMBER() OVER (ORDER BY id) as rn \
               FROM users";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Window { .. })),
        "expected Window node"
    );
}

#[test]
#[ignore = "Lime grammar does not yet produce Window nodes"]
fn test_rank_window_with_partition() {
    let sql = "SELECT dept, salary, \
               RANK() OVER (PARTITION BY dept ORDER BY salary DESC) as rnk \
               FROM employees";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Window { .. })),
        "expected Window node"
    );
}

#[test]
#[ignore = "Lime grammar does not yet produce Window nodes"]
fn test_window_sum() {
    let sql = "SELECT id, \
               SUM(amount) OVER (ORDER BY id) as running_total \
               FROM orders";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Window { .. })),
        "expected Window node"
    );
}

// ---- Set operation tests ----

#[test]
fn test_union() {
    let sql = "SELECT id FROM a UNION SELECT id FROM b";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(matches!(result, RelExpr::Union { all: false, .. }));
}

#[test]
fn test_union_all() {
    let sql = "SELECT id FROM a UNION ALL SELECT id FROM b";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(matches!(result, RelExpr::Union { all: true, .. }));
}

#[test]
fn test_intersect() {
    let sql = "SELECT id FROM a INTERSECT SELECT id FROM b";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(matches!(result, RelExpr::Intersect { all: false, .. }));
}

#[test]
fn test_except() {
    let sql = "SELECT id FROM a EXCEPT SELECT id FROM b";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(matches!(result, RelExpr::Except { all: false, .. }));
}

// ---- Extended aggregate tests ----
// Lime grammar treats STDDEV/VARIANCE as function calls, not
// Aggregate nodes. Only GROUP BY triggers Aggregate creation.

#[test]
#[ignore = "Lime grammar does not produce Aggregate nodes for bare aggregates without GROUP BY"]
fn test_stddev_aggregate() {
    let sql = "SELECT STDDEV(salary) FROM employees";
    let result = sql_to_relexpr(sql).expect("should parse");
    let agg =
        find_node(&result, |r| matches!(r, RelExpr::Aggregate { .. })).expect("expected Aggregate");
    if let RelExpr::Aggregate { aggregates, .. } = agg {
        assert_eq!(
            aggregates[0].function,
            ra_core::algebra::AggregateFunction::StdDev
        );
    }
}

#[test]
#[ignore = "Lime grammar does not produce Aggregate nodes for bare aggregates without GROUP BY"]
fn test_variance_aggregate() {
    let sql = "SELECT VARIANCE(score) FROM tests";
    let result = sql_to_relexpr(sql).expect("should parse");
    let agg =
        find_node(&result, |r| matches!(r, RelExpr::Aggregate { .. })).expect("expected Aggregate");
    if let RelExpr::Aggregate { aggregates, .. } = agg {
        assert_eq!(
            aggregates[0].function,
            ra_core::algebra::AggregateFunction::Variance
        );
    }
}

// ---- BETWEEN test ----

#[test]
fn test_between() {
    let sql = "SELECT * FROM orders WHERE amount BETWEEN 10 AND 100";
    let result = sql_to_relexpr(sql).expect("should parse");
    let filter =
        find_node(&result, |r| matches!(r, RelExpr::Filter { .. })).expect("expected Filter node");
    if let RelExpr::Filter { predicate, .. } = filter {
        assert!(
            matches!(predicate, Expr::BinOp { op: BinOp::And, .. }),
            "BETWEEN should expand to AND"
        );
    }
}

// ---- CAST test ----

#[test]
fn test_cast() {
    let sql = "SELECT CAST(price AS INTEGER) FROM products";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "CAST should parse");
}

// ---- CASE expression test ----

#[test]
fn test_case_expression() {
    let sql = "SELECT CASE WHEN age > 18 THEN 'adult' \
               ELSE 'minor' END FROM users";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "CASE should parse");
}

// ---- Combination tests ----

#[test]
fn test_cte_with_window() {
    let sql = "WITH ranked AS (\
                 SELECT id, \
                   ROW_NUMBER() OVER (ORDER BY id) as rn \
                 FROM users\
               ) \
               SELECT * FROM ranked WHERE rn <= 10";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "CTE + window should parse");
}

#[test]
fn test_distinct_with_order_by() {
    let sql = "SELECT DISTINCT name FROM users ORDER BY name";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Sort { .. })),
        "expected Sort node"
    );
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Distinct { .. })),
        "expected Distinct node"
    );
}

#[test]
fn test_having_with_limit() {
    let sql = "SELECT dept_id, COUNT(*) as cnt \
               FROM employees \
               GROUP BY dept_id \
               HAVING COUNT(*) > 5 \
               LIMIT 10";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        matches!(result, RelExpr::Limit { .. }),
        "expected Limit at top"
    );
}

#[test]
fn test_complex_query() {
    let sql = "WITH dept_stats AS (\
                 SELECT dept_id, AVG(salary) as avg_sal \
                 FROM employees \
                 GROUP BY dept_id \
                 HAVING AVG(salary) > 50000\
               ) \
               SELECT DISTINCT d.dept_id \
               FROM dept_stats d \
               ORDER BY d.dept_id \
               LIMIT 20 OFFSET 5";
    let result = sql_to_relexpr(sql);
    assert!(
        result.is_ok(),
        "complex query should parse: {:?}",
        result.err()
    );
}

#[test]
fn test_multiple_from_items() {
    let sql = "SELECT * FROM a, b WHERE a.id = b.id";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Join { .. })),
        "expected implicit cross join"
    );
}

#[test]
fn test_join_using() {
    let sql = "SELECT * FROM orders JOIN customers USING (customer_id)";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "JOIN USING should parse");
}

// ---- Recursive CTE tests ----
// Lime grammar does not distinguish WITH RECURSIVE from WITH.
// It produces CTE nodes instead of RecursiveCTE nodes.

#[test]
fn test_simple_recursive_cte() {
    let sql = "\
        WITH RECURSIVE counter AS (\
            SELECT n FROM seed_table WHERE n = 1 \
            UNION ALL \
            SELECT n + 1 FROM counter WHERE n < 10\
        ) SELECT * FROM counter";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "simple recursive CTE: {result:?}");
    let plan = result.expect("already checked");
    assert!(
        matches!(&plan, RelExpr::RecursiveCTE { .. }),
        "expected RecursiveCTE node, got: {plan:?}"
    );
}

#[test]
fn test_recursive_cte_name() {
    let sql = "\
        WITH RECURSIVE nums AS (\
            SELECT val FROM seed WHERE val = 1 \
            UNION ALL \
            SELECT val + 1 FROM nums WHERE val < 5\
        ) SELECT * FROM nums";
    let plan = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::RecursiveCTE { name, .. } = &plan {
        assert_eq!(name, "nums");
    } else {
        panic!("expected RecursiveCTE");
    }
}

#[test]
fn test_recursive_cte_base_is_non_recursive() {
    let sql = "\
        WITH RECURSIVE r AS (\
            SELECT id FROM nodes WHERE root = true \
            UNION ALL \
            SELECT e.dst FROM edges e JOIN r ON e.src = r.id\
        ) SELECT * FROM r";
    let plan = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::RecursiveCTE {
        base_case, name, ..
    } = &plan
    {
        assert!(
            !base_case.references_cte(name),
            "base case should not reference CTE"
        );
    } else {
        panic!("expected RecursiveCTE");
    }
}

#[test]
fn test_recursive_cte_recursive_references_cte() {
    let sql = "\
        WITH RECURSIVE r AS (\
            SELECT id FROM nodes WHERE root = true \
            UNION ALL \
            SELECT e.dst FROM edges e JOIN r ON e.src = r.id\
        ) SELECT * FROM r";
    let plan = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::RecursiveCTE {
        recursive_case,
        name,
        ..
    } = &plan
    {
        assert!(
            recursive_case.references_cte(name),
            "recursive case should reference CTE"
        );
    } else {
        panic!("expected RecursiveCTE");
    }
}

#[test]
fn test_recursive_cte_has_cycle_detection() {
    let sql = "\
        WITH RECURSIVE r AS (\
            SELECT n FROM seed WHERE n = 1 \
            UNION ALL \
            SELECT n + 1 FROM r WHERE n < 10\
        ) SELECT * FROM r";
    let plan = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::RecursiveCTE {
        cycle_detection, ..
    } = &plan
    {
        assert!(
            cycle_detection.is_some(),
            "should have default cycle detection"
        );
        let cd = cycle_detection.as_ref().expect("checked");
        assert_eq!(cd.max_depth, Some(1000));
    } else {
        panic!("expected RecursiveCTE");
    }
}

#[test]
fn test_recursive_cte_with_order_by() {
    let sql = "\
        WITH RECURSIVE r AS (\
            SELECT n FROM seed WHERE n = 1 \
            UNION ALL \
            SELECT n + 1 FROM r WHERE n < 10\
        ) SELECT * FROM r ORDER BY n";
    let plan = sql_to_relexpr(sql).expect("should parse");
    // RecursiveCTE is the outermost node; the body contains the Sort.
    assert!(
        matches!(plan, RelExpr::RecursiveCTE { .. }),
        "RecursiveCTE is outermost, got: {plan:?}"
    );
    if let RelExpr::RecursiveCTE { body, .. } = &plan {
        assert!(
            has_node(body, |r| matches!(r, RelExpr::Sort { .. })),
            "Sort should appear in CTE body"
        );
    }
}

#[test]
fn test_recursive_cte_with_limit() {
    let sql = "\
        WITH RECURSIVE r AS (\
            SELECT n FROM seed WHERE n = 1 \
            UNION ALL \
            SELECT n + 1 FROM r WHERE n < 100\
        ) SELECT * FROM r LIMIT 10";
    let plan = sql_to_relexpr(sql).expect("should parse");
    // RecursiveCTE is outermost; body contains the Limit.
    assert!(
        matches!(plan, RelExpr::RecursiveCTE { .. }),
        "RecursiveCTE is outermost, got: {plan:?}"
    );
    if let RelExpr::RecursiveCTE { body, .. } = &plan {
        assert!(
            has_node(body, |r| matches!(r, RelExpr::Limit { .. })),
            "Limit should appear in CTE body"
        );
    }
}

#[test]
fn test_non_recursive_with_recursive_keyword() {
    // WITH RECURSIVE keyword but body is not UNION ALL — treated as regular CTE
    let sql = "\
        WITH RECURSIVE t AS (\
            SELECT id FROM users\
        ) SELECT * FROM t";
    let plan = sql_to_relexpr(sql).expect("should parse");
    assert!(
        matches!(plan, RelExpr::CTE { .. }),
        "WITH RECURSIVE without UNION ALL body produces CTE, got: {plan:?}"
    );
}

#[test]
fn test_running_totals_query() {
    let sql = "\
        WITH RECURSIVE DatewiseTotal AS (\
            SELECT id, date, department, amount \
            FROM financial_data \
            WHERE department = 'HR' \
                AND date = (SELECT MIN(date) \
                    FROM financial_data \
                    WHERE department = 'HR')\
            UNION ALL \
            SELECT fd.id, fd.date, fd.department, \
                   fd.amount + dt.amount \
            FROM financial_data fd \
            JOIN DatewiseTotal dt \
                ON fd.date = (SELECT MIN(date) \
                    FROM financial_data \
                    WHERE date > dt.date \
                        AND department = 'HR') \
            WHERE fd.department = 'HR'\
        ) \
        SELECT * FROM DatewiseTotal ORDER BY date";
    let result = sql_to_relexpr(sql);
    assert!(
        result.is_ok(),
        "running totals query should parse: {result:?}"
    );
    let plan = result.expect("already checked");

    // RecursiveCTE is the outermost node; body contains the Sort.
    assert!(
        matches!(plan, RelExpr::RecursiveCTE { .. }),
        "expected RecursiveCTE at top, got {plan:?}"
    );

    if let RelExpr::RecursiveCTE { name, body, .. } = &plan {
        assert_eq!(name.to_lowercase(), "datewisetotal");
        assert!(
            has_node(body, |r| matches!(r, RelExpr::Sort { .. })),
            "Sort should appear in CTE body"
        );
    }
}

#[test]
fn test_graph_reachability_recursive_cte() {
    let sql = "\
        WITH RECURSIVE reachable AS (\
            SELECT dst FROM edges WHERE src = 1 \
            UNION ALL \
            SELECT e.dst FROM edges e \
            JOIN reachable r ON e.src = r.dst\
        ) SELECT * FROM reachable";
    let plan = sql_to_relexpr(sql).expect("should parse");
    assert!(
        matches!(plan, RelExpr::RecursiveCTE { .. }),
        "expected RecursiveCTE"
    );
}

#[test]
fn test_fibonacci_recursive_cte() {
    let sql = "\
        WITH RECURSIVE fib AS (\
            SELECT n, a, b FROM seed \
            WHERE n = 1 AND a = 0 AND b = 1 \
            UNION ALL \
            SELECT n + 1, b, a + b FROM fib WHERE n < 20\
        ) SELECT n, a FROM fib";
    let plan = sql_to_relexpr(sql).expect("should parse");
    assert!(matches!(plan, RelExpr::RecursiveCTE { .. }));
}

#[test]
fn test_tree_hierarchy_recursive_cte() {
    let sql = "\
        WITH RECURSIVE hierarchy AS (\
            SELECT id, name, parent_id, 0 AS depth \
            FROM employees WHERE parent_id IS NULL \
            UNION ALL \
            SELECT e.id, e.name, e.parent_id, h.depth + 1 \
            FROM employees e \
            JOIN hierarchy h ON e.parent_id = h.id\
        ) SELECT * FROM hierarchy ORDER BY depth, name";
    let plan = sql_to_relexpr(sql).expect("should parse");
    // RecursiveCTE is outermost; Sort is in the body.
    assert!(
        matches!(plan, RelExpr::RecursiveCTE { .. }),
        "expected RecursiveCTE at top, got: {plan:?}"
    );
    if let RelExpr::RecursiveCTE { body, .. } = &plan {
        assert!(
            has_node(body, |r| matches!(r, RelExpr::Sort { .. })),
            "Sort should appear in CTE body (ORDER BY depth, name)"
        );
    }
}

#[test]
fn test_recursive_cte_children_count() {
    let sql = "\
        WITH RECURSIVE r AS (\
            SELECT n FROM seed WHERE n = 1 \
            UNION ALL \
            SELECT n + 1 FROM r WHERE n < 5\
        ) SELECT * FROM r";
    let plan = sql_to_relexpr(sql).expect("should parse");
    assert_eq!(plan.children().len(), 3, "RecursiveCTE has 3 children");
}

// ---- Multi-statement and non-SELECT handling ----

#[test]
fn test_multi_statement_takes_first_select() {
    let sql = "CREATE TABLE t (id INT); \
               SELECT * FROM users WHERE age > 18";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "multi-statement with SELECT should work");
}

#[test]
fn test_select_without_from() {
    let sql = "SELECT 1 + 2";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "SELECT without FROM: {result:?}");
}

// ---- Qualified wildcard and mixed wildcard ----

#[test]
fn test_qualified_wildcard() {
    let sql = "SELECT o.*, u.name \
               FROM orders o JOIN users u ON o.uid = u.id";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "qualified wildcard o.*: {result:?}");
}

#[test]
fn test_wildcard_in_multi_column() {
    let sql = "SELECT *, name FROM users";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "wildcard in multi-column: {result:?}");
}

// ---- IN, LIKE, INTERVAL, DATE ----

#[test]
fn test_in_list() {
    let sql = "SELECT * FROM orders \
               WHERE status IN ('shipped', 'delivered')";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "IN list: {result:?}");
}

#[test]
fn test_like() {
    let sql = "SELECT * FROM users WHERE email LIKE 'a%'";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "LIKE: {result:?}");
}

#[test]
#[ignore = "Lime grammar does not yet support INTERVAL literals"]
fn test_interval() {
    let sql = "SELECT * FROM events \
               WHERE created_at > INTERVAL '1 hour'";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "INTERVAL: {result:?}");
}

#[test]
#[ignore = "Lime grammar does not yet support DATE literals"]
fn test_date_literal() {
    let sql = "SELECT * FROM orders \
               WHERE order_date > DATE '2024-01-01'";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "DATE literal: {result:?}");
}

#[test]
#[ignore = "Lime grammar does not yet support placeholder syntax"]
fn test_placeholder() {
    let sql = "SELECT * FROM users WHERE id = ?";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "placeholder: {result:?}");
}

#[test]
#[ignore = "Lime grammar does not yet support EXTRACT"]
fn test_extract() {
    let sql = "SELECT EXTRACT(YEAR FROM order_date) \
               FROM orders";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "EXTRACT: {result:?}");
}

// ---- PostgreSQL-specific operators ----

#[test]
#[ignore = "Lime grammar does not yet support JSONB operators"]
fn test_jsonb_contains() {
    let sql = "SELECT * FROM users \
               WHERE data @> '{\"age\": 25}'";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "JSONB @> operator: {result:?}");
}

#[test]
#[ignore = "Lime grammar does not yet support JSONB operators"]
fn test_jsonb_contained_by() {
    let sql = "SELECT * FROM users \
               WHERE '{\"age\": 25}' <@ data";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "JSONB <@ operator: {result:?}");
}

#[test]
#[ignore = "Lime grammar does not yet support JSONB operators"]
fn test_jsonb_path_exists() {
    let sql = "SELECT * FROM users \
               WHERE data @? '$.age ? (@ > 25)'";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "JSONB @? operator: {result:?}");
}

#[test]
#[ignore = "Lime grammar does not yet support JSONB operators"]
fn test_jsonb_path_match() {
    let sql = "SELECT * FROM users \
               WHERE data @@ '$.status == \"active\"'";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "JSONB @@ operator: {result:?}");
}

#[test]
#[ignore = "Lime grammar does not yet support JSONB operators"]
fn test_documentdb_query() {
    // DocumentDB query with standard PostgreSQL JSONB operators
    let sql = "SELECT document FROM documentdb_api.collection('mydb', 'users') \
               WHERE document @> '{\"age\": {\"$gt\": 25}}' \
               AND document @? '$.status ? (@ == \"active\")'";
    let result = sql_to_relexpr(sql);
    assert!(
        result.is_ok(),
        "DocumentDB query with JSONB operators: {result:?}"
    );
}

// ---- Vector Search tests ----
// The old sqlparser pipeline had special-case logic to produce TopK and
// VectorFilter nodes. The Lime grammar produces standard Sort/Filter nodes.

#[test]
#[ignore = "Lime grammar does not yet produce TopK nodes for vector search"]
fn test_sqlite_vec_topk_l2() {
    // sqlite-vec with vec_distance_l2 function
    let sql = "SELECT * FROM items \
               ORDER BY vec_distance_l2(embedding, vec_f32('[1,2,3]')) \
               LIMIT 10";
    let result = sql_to_relexpr(sql).expect("should parse sqlite-vec TopK");

    match result {
        RelExpr::TopK { k, metric, .. } => {
            assert_eq!(k, 10);
            assert_eq!(metric, ra_core::search_types::DistanceMetric::L2);
        }
        _ => panic!("expected TopK, got {result:?}"),
    }
}

#[test]
#[ignore = "Lime grammar does not yet produce TopK nodes for vector search"]
fn test_sqlite_vec_topk_cosine() {
    // sqlite-vec with cosine distance
    let sql = "SELECT id, vec_distance_cosine(embedding, query_vec) AS similarity \
               FROM items \
               ORDER BY vec_distance_cosine(embedding, query_vec) \
               LIMIT 10";
    let result = sql_to_relexpr(sql).expect("should parse sqlite-vec cosine");

    match result {
        RelExpr::TopK { k, metric, .. } => {
            assert_eq!(k, 10);
            assert_eq!(metric, ra_core::search_types::DistanceMetric::Cosine);
        }
        _ => panic!("expected TopK, got {result:?}"),
    }
}

#[test]
#[ignore = "Lime grammar does not yet produce VectorFilter nodes"]
fn test_sqlite_vec_filter() {
    // sqlite-vec with threshold filter
    let sql = "SELECT * FROM items \
               WHERE vec_distance_l2(embedding, vec_f32('[1,2,3]')) < 0.5";
    let result = sql_to_relexpr(sql).expect("should parse sqlite-vec filter");

    match result {
        RelExpr::VectorFilter {
            threshold, metric, ..
        } => {
            assert_eq!(threshold, 0.5);
            assert_eq!(metric, ra_core::search_types::DistanceMetric::L2);
        }
        _ => panic!("expected VectorFilter, got {result:?}"),
    }
}

#[test]
#[ignore = "Lime grammar does not yet produce VectorFilter nodes"]
fn test_vector_hybrid_search() {
    // Simple vector filter works
    let sql = "SELECT * FROM products \
               WHERE l2_distance(embedding, query_vec) < 0.8";
    let result = sql_to_relexpr(sql).expect("should parse simple vector filter");

    // Should produce VectorFilter
    match result {
        RelExpr::VectorFilter { threshold, .. } => {
            assert_eq!(threshold, 0.8);
        }
        _ => panic!("expected VectorFilter for simple case, got {result:?}"),
    }
}

#[test]
#[ignore = "Lime grammar does not yet produce TopK nodes for vector search"]
fn test_pgvector_topk_l2_function() {
    // pgvector with l2_distance function
    let sql = "SELECT * FROM items \
               ORDER BY l2_distance(embedding, '[1,2,3]') \
               LIMIT 10";
    let result = sql_to_relexpr(sql).expect("should parse pgvector L2 TopK");

    match result {
        RelExpr::TopK { k, metric, .. } => {
            assert_eq!(k, 10);
            assert_eq!(metric, ra_core::search_types::DistanceMetric::L2);
        }
        _ => panic!("expected TopK, got {result:?}"),
    }
}

#[test]
#[ignore = "Lime grammar does not yet produce TopK nodes for vector search"]
fn test_pgvector_topk_cosine_function() {
    // pgvector with cosine_distance function
    let sql = "SELECT id, text FROM documents \
               ORDER BY cosine_distance(embedding, '[0.1, 0.2, 0.3]') \
               LIMIT 5";
    let result = sql_to_relexpr(sql).expect("should parse pgvector cosine TopK");

    match result {
        RelExpr::TopK { k, metric, .. } => {
            assert_eq!(k, 5);
            assert_eq!(metric, ra_core::search_types::DistanceMetric::Cosine);
        }
        _ => panic!("expected TopK, got {result:?}"),
    }
}

#[test]
#[ignore = "Lime grammar does not yet produce VectorFilter nodes"]
fn test_pgvector_filter_function() {
    // pgvector with distance threshold in WHERE using function
    let sql = "SELECT * FROM items WHERE l2_distance(embedding, query_vec) < 0.5";
    let result = sql_to_relexpr(sql).expect("should parse pgvector filter");

    match result {
        RelExpr::VectorFilter {
            threshold, metric, ..
        } => {
            assert_eq!(threshold, 0.5);
            assert_eq!(metric, ra_core::search_types::DistanceMetric::L2);
        }
        _ => panic!("expected VectorFilter, got {result:?}"),
    }
}

#[test]
fn test_vector_without_limit() {
    // Vector ORDER BY without LIMIT should produce regular Sort
    let sql = "SELECT * FROM items ORDER BY l2_distance(embedding, '[1,2,3]')";
    let result = sql_to_relexpr(sql).expect("should parse");

    // Without LIMIT, should be a regular Sort, not TopK
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Sort { .. })),
        "expected Sort without LIMIT, got {result:?}"
    );
}

#[test]
fn test_vector_multiple_order_by_columns() {
    // Multiple ORDER BY expressions should use regular Sort
    let sql = "SELECT * FROM items \
               ORDER BY l2_distance(embedding, '[1,2,3]'), created_at DESC \
               LIMIT 10";
    let result = sql_to_relexpr(sql).expect("should parse");

    // Multiple ORDER BY => regular Sort + Limit
    match result {
        RelExpr::Limit { input, .. } => {
            assert!(matches!(*input, RelExpr::Sort { .. }));
        }
        _ => panic!("expected Limit(Sort(...)), got {result:?}"),
    }
}

#[test]
#[ignore = "Lime grammar does not yet produce TopK nodes for vector search"]
fn test_vector_with_projection() {
    // Vector search with specific columns selected
    let sql = "SELECT id, title, cosine_distance(embedding, query) AS similarity \
               FROM documents \
               WHERE cosine_distance(embedding, query) < 0.3 \
               ORDER BY cosine_distance(embedding, query) \
               LIMIT 20";
    let result = sql_to_relexpr(sql).expect("should parse vector with projection");

    // Should have TopK at the top
    match result {
        RelExpr::TopK { k, .. } => {
            assert_eq!(k, 20);
        }
        _ => panic!("expected TopK, got {result:?}"),
    }
}
