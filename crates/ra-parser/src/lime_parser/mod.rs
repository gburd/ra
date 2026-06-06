//! Lime-based SQL parser integration.
//!
//! This module provides the entry point for parsing SQL using the
//! Lime-generated LALR(1) parser. The grammar is compiled at build time
//! by `build.rs` and linked as a C static library.
//!
//! The generated C parser calls back into the `extern "C"` functions in
//! [`crate::ffi::builders`] to construct `RelExpr` / `Expr` AST nodes.
//!
//! # Usage
//!
//! ```ignore
//! use ra_parser::lime_parser;
//! let rel = lime_parser::parse_sql("SELECT id FROM users WHERE age > 21")?;
//! ```

pub mod diagnostics;
pub mod lexer;
pub mod lime_tokenizer;

use std::os::raw::{c_char, c_int, c_void};

use ra_core::algebra::RelExpr;

use crate::ffi::node::ParseErrors;
#[cfg(not(feature = "rust-parser"))]
use crate::ffi::node::RaParseState;
#[cfg(not(feature = "rust-parser"))]
use lexer::RaToken;

// ---------------------------------------------------------------------------
// FFI declarations for the Lime-generated parser.
//
// The grammar uses `%name ra`, so the generated functions are:
//   raAlloc  -- allocate parser state
//   raFree   -- free parser state
//   ra       -- feed one token
//
// The `%extra_argument` is `RaParseState *pstate`, which becomes the
// last parameter of `ra()`.
// ---------------------------------------------------------------------------
// RaParseState is passed as an opaque pointer (`void *`) in the C API
// and is never dereferenced by the generated C code.
#[cfg(not(feature = "rust-parser"))]
#[expect(improper_ctypes)]
extern "C" {
    fn raAlloc(alloc: Option<unsafe extern "C" fn(usize) -> *mut c_void>) -> *mut c_void;

    fn raFree(parser: *mut c_void, free_fn: Option<unsafe extern "C" fn(*mut c_void)>);

    fn ra(parser: *mut c_void, token_type: c_int, token_value: RaToken, state: *mut RaParseState);
}

// Diagnostics helpers (used by `diagnostics.rs` on both paths; the C parser
// library is linked under both feature configurations).
extern "C" {
    /// Return the string name of a terminal token code, or NULL.
    fn raTokenName(token_code: c_int) -> *const c_char;

    /// Return the current parser state number, or -1 if invalid.
    fn raState(parser: *mut c_void) -> c_int;

    /// Fill `out` with token codes valid at `stateno`.
    /// Returns total count (may exceed `max`).
    fn raExpectedTokens(stateno: c_int, out: *mut c_int, max: c_int) -> c_int;
}

#[cfg(not(feature = "rust-parser"))]
extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

/// Allocator callback for `raAlloc` (wraps C `malloc`).
#[cfg(not(feature = "rust-parser"))]
unsafe extern "C" fn parser_malloc(size: usize) -> *mut c_void {
    // SAFETY: delegates to the C runtime allocator.
    unsafe { malloc(size) }
}

/// Deallocator callback for `raFree` (wraps C `free`).
#[cfg(not(feature = "rust-parser"))]
unsafe extern "C" fn parser_free(ptr: *mut c_void) {
    // SAFETY: ptr was allocated by `malloc` via `parser_malloc`.
    unsafe { free(ptr) };
}

/// Parse a SQL string into a `RelExpr` using the Lime parser.
///
/// Uses the SIMD-accelerated lime tokenizer for tokenization,
/// falling back to the pure-Rust tokenizer on failure.
///
/// # Errors
///
/// Returns a list of error messages if the SQL cannot be parsed.
pub fn parse_sql(sql: &str) -> Result<RelExpr, ParseErrors> {
    // Under the `rust-parser` feature, delegate to the native-Rust parse path
    // (Lime `--target=rust` output). This routes the entire test suite through
    // the Rust parser to prove behavioral identity with the C path.
    #[cfg(feature = "rust-parser")]
    {
        return crate::rust_parser::parse_sql(sql);
    }
    #[cfg(not(feature = "rust-parser"))]
    {
        parse_sql_c(sql)
    }
}

/// The C FFI parse path (Lime `--target=c` output via `lime-sys`).
#[cfg(not(feature = "rust-parser"))]
fn parse_sql_c(sql: &str) -> Result<RelExpr, ParseErrors> {
    // Tokenize using SIMD-accelerated lime tokenizer, falling
    // back to the pure-Rust lexer if the C tokenizer fails.
    let tokens = lime_tokenizer::tokenize_simd(sql)
        .or_else(|_| lexer::tokenize(sql))
        .map_err(|e| ParseErrors::Strings(vec![e]))?;

    // Allocate the generated parser.
    //
    // SAFETY: parser_malloc is a valid allocation function.
    let parser = unsafe { raAlloc(Some(parser_malloc)) };
    if parser.is_null() {
        return Err(ParseErrors::Strings(vec![
            "failed to allocate parser".to_owned()
        ]));
    }

    let mut state = RaParseState::new();

    // Feed each token to the parser.
    for tok in &tokens {
        // SAFETY: parser is a valid raAlloc handle, state is valid.
        unsafe {
            ra(parser, tok.code, tok.value.clone(), &raw mut state);
        }
    }

    // Feed EOF (token code 0) to finalize parsing.
    // Set location to end of input so any error at EOF points to
    // the right position.
    let eof_token = RaToken {
        location: i32::try_from(sql.len()).unwrap_or(0),
        length: 0,
        ..RaToken::default()
    };
    // SAFETY: same as above.
    unsafe {
        ra(parser, 0, eof_token, &raw mut state);
    }

    // Free the parser.
    // SAFETY: parser was allocated by raAlloc.
    unsafe {
        raFree(parser, Some(parser_free));
    }

    // Extract the result from the parse state.
    state.take_result()
}

#[cfg(test)]
#[expect(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;

    /// Helper: parse SQL and assert it produces the expected
    /// top-level `RelExpr` variant.
    fn assert_parses_as(sql: &str, check: fn(&RelExpr) -> bool, label: &str) {
        let rel = parse_sql(sql).unwrap_or_else(|errs| panic!("{label}: {errs:?}"));
        assert!(check(&rel), "expected {label}, got {rel:?}");
    }

    #[test]
    fn parse_select_star_from() {
        assert_parses_as(
            "SELECT * FROM users",
            |r| matches!(r, RelExpr::Project { .. }),
            "Project",
        );
    }

    #[test]
    fn parse_select_with_where() {
        assert_parses_as(
            "SELECT id, name FROM users WHERE age > 21",
            |r| matches!(r, RelExpr::Project { .. }),
            "Project",
        );
    }

    #[test]
    fn parse_join() {
        assert_parses_as(
            "SELECT * FROM a JOIN b ON a.id = b.id",
            |r| matches!(r, RelExpr::Project { .. }),
            "Project",
        );
    }

    #[test]
    fn parse_group_by() {
        assert_parses_as(
            "SELECT dept, COUNT(*) FROM employees GROUP BY dept",
            |r| matches!(r, RelExpr::Project { .. }),
            "Project",
        );
    }

    #[test]
    fn parse_order_by_limit() {
        // Grammar currently has LIMIT as a placeholder;
        // just verify it doesn't error.
        parse_sql("SELECT * FROM t ORDER BY id LIMIT 10").expect("should parse");
    }

    #[test]
    fn parse_empty_input() {
        assert!(parse_sql("").is_err());
    }

    #[test]
    fn parse_with_semicolon() {
        parse_sql("SELECT * FROM t;").expect("should parse");
    }

    #[test]
    fn parse_union() {
        assert_parses_as(
            "SELECT id FROM a UNION ALL SELECT id FROM b",
            |r| matches!(r, RelExpr::Union { all: true, .. }),
            "Union",
        );
    }

    #[test]
    fn parse_distinct() {
        assert_parses_as(
            "SELECT DISTINCT name FROM users",
            |r| matches!(r, RelExpr::Distinct { .. }),
            "Distinct",
        );
    }

    // ---------------------------------------------------------------
    // New grammar constructs
    // ---------------------------------------------------------------

    #[test]
    fn parse_intersect() {
        assert_parses_as(
            "SELECT id FROM a INTERSECT SELECT id FROM b",
            |r| matches!(r, RelExpr::Intersect { all: false, .. }),
            "Intersect",
        );
    }

    #[test]
    fn parse_intersect_all() {
        assert_parses_as(
            "SELECT id FROM a INTERSECT ALL SELECT id FROM b",
            |r| matches!(r, RelExpr::Intersect { all: true, .. }),
            "Intersect All",
        );
    }

    #[test]
    fn parse_except() {
        assert_parses_as(
            "SELECT id FROM a EXCEPT SELECT id FROM b",
            |r| matches!(r, RelExpr::Except { all: false, .. }),
            "Except",
        );
    }

    #[test]
    fn parse_except_all() {
        assert_parses_as(
            "SELECT id FROM a EXCEPT ALL SELECT id FROM b",
            |r| matches!(r, RelExpr::Except { all: true, .. }),
            "Except All",
        );
    }

    #[test]
    fn parse_case_searched() {
        parse_sql("SELECT CASE WHEN x > 0 THEN 1 ELSE 0 END FROM t")
            .expect("searched CASE should parse");
    }

    #[test]
    fn parse_case_simple() {
        parse_sql(
            "SELECT CASE status \
             WHEN 1 THEN 'active' \
             WHEN 2 THEN 'inactive' \
             ELSE 'unknown' END FROM t",
        )
        .expect("simple CASE should parse");
    }

    #[test]
    fn parse_case_no_else() {
        parse_sql("SELECT CASE WHEN x = 1 THEN 'one' END FROM t")
            .expect("CASE without ELSE should parse");
    }

    #[test]
    fn parse_cast() {
        parse_sql("SELECT CAST(x AS integer) FROM t").expect("CAST should parse");
    }

    #[test]
    fn parse_cast_with_precision() {
        parse_sql("SELECT CAST(x AS varchar(255)) FROM t")
            .expect("CAST with precision should parse");
    }

    #[test]
    fn parse_not_expr() {
        parse_sql("SELECT * FROM t WHERE NOT active").expect("NOT expression should parse");
    }

    #[test]
    fn parse_between() {
        parse_sql("SELECT * FROM t WHERE x BETWEEN 1 AND 10").expect("BETWEEN should parse");
    }

    #[test]
    fn parse_not_between() {
        parse_sql("SELECT * FROM t WHERE x NOT BETWEEN 1 AND 10")
            .expect("NOT BETWEEN should parse");
    }

    #[test]
    fn parse_like() {
        parse_sql("SELECT * FROM t WHERE name LIKE '%foo%'").expect("LIKE should parse");
    }

    #[test]
    fn parse_not_like() {
        parse_sql("SELECT * FROM t WHERE name NOT LIKE '%bar%'").expect("NOT LIKE should parse");
    }

    #[test]
    fn parse_ilike() {
        parse_sql("SELECT * FROM t WHERE name ILIKE '%foo%'").expect("ILIKE should parse");
    }

    #[test]
    fn parse_in_list() {
        parse_sql("SELECT * FROM t WHERE id IN (1, 2, 3)").expect("IN (list) should parse");
    }

    #[test]
    fn parse_not_in_list() {
        parse_sql("SELECT * FROM t WHERE id NOT IN (1, 2, 3)").expect("NOT IN (list) should parse");
    }

    #[test]
    fn parse_in_subquery() {
        parse_sql(
            "SELECT * FROM t WHERE id IN \
             (SELECT id FROM other)",
        )
        .expect("IN (subquery) should parse");
    }

    #[test]
    fn parse_not_in_subquery() {
        parse_sql(
            "SELECT * FROM t WHERE id NOT IN \
             (SELECT id FROM other)",
        )
        .expect("NOT IN (subquery) should parse");
    }

    #[test]
    fn parse_exists() {
        parse_sql(
            "SELECT * FROM t WHERE EXISTS \
             (SELECT 1 FROM other WHERE other.id = t.id)",
        )
        .expect("EXISTS should parse");
    }

    #[test]
    fn parse_scalar_subquery() {
        parse_sql("SELECT (SELECT COUNT(*) FROM other) FROM t")
            .expect("scalar subquery should parse");
    }

    #[test]
    fn parse_derived_table() {
        parse_sql(
            "SELECT * FROM \
             (SELECT id, name FROM users) AS sub",
        )
        .expect("derived table should parse");
    }

    #[test]
    fn parse_derived_table_no_as() {
        parse_sql(
            "SELECT * FROM \
             (SELECT id FROM users) sub",
        )
        .expect("derived table without AS should parse");
    }

    #[test]
    fn parse_having() {
        parse_sql(
            "SELECT dept, COUNT(*) FROM employees \
             GROUP BY dept HAVING COUNT(*) > 5",
        )
        .expect("HAVING should parse");
    }

    #[test]
    fn parse_values() {
        assert_parses_as(
            "VALUES (1, 'a'), (2, 'b')",
            |r| matches!(r, RelExpr::Values { .. }),
            "Values",
        );
    }

    #[test]
    fn parse_cte() {
        assert_parses_as(
            "WITH active AS (SELECT * FROM users WHERE active = 1) \
             SELECT * FROM active",
            |r| matches!(r, RelExpr::CTE { .. }),
            "CTE",
        );
    }

    #[test]
    fn parse_window_function() {
        parse_sql(
            "SELECT id, row_number() OVER \
             (PARTITION BY dept ORDER BY id) FROM t",
        )
        .expect("window function should parse");
    }

    #[test]
    fn parse_window_no_partition() {
        parse_sql(
            "SELECT id, SUM(amount) OVER \
             (ORDER BY id) FROM t",
        )
        .expect("window without PARTITION BY should parse");
    }

    #[test]
    fn parse_window_empty_over() {
        parse_sql("SELECT id, COUNT(*) OVER () FROM t")
            .expect("window with empty OVER should parse");
    }

    #[test]
    fn parse_union_distinct() {
        assert_parses_as(
            "SELECT id FROM a UNION SELECT id FROM b",
            |r| matches!(r, RelExpr::Union { all: false, .. }),
            "Union distinct",
        );
    }

    #[test]
    fn parse_aggregate_distinct() {
        parse_sql("SELECT COUNT(DISTINCT name) FROM users")
            .expect("aggregate DISTINCT should parse");
    }

    #[test]
    fn tokenize_new_keywords() {
        let tokens = lexer::tokenize(
            "CASE WHEN THEN ELSE END CAST BETWEEN \
             LIKE ILIKE IN EXISTS WITH RECURSIVE \
             PARTITION OVER VALUES INTERSECT EXCEPT",
        )
        .expect("new keywords should tokenize");
        assert_eq!(tokens.len(), 18);
        assert_eq!(tokens[0].code, lexer::token::CASE);
        assert_eq!(tokens[1].code, lexer::token::WHEN);
        assert_eq!(tokens[2].code, lexer::token::THEN);
        assert_eq!(tokens[3].code, lexer::token::ELSE);
        assert_eq!(tokens[4].code, lexer::token::END);
        assert_eq!(tokens[5].code, lexer::token::CAST);
        assert_eq!(tokens[6].code, lexer::token::BETWEEN);
        assert_eq!(tokens[7].code, lexer::token::LIKE);
        assert_eq!(tokens[8].code, lexer::token::ILIKE);
        assert_eq!(tokens[9].code, lexer::token::IN);
        assert_eq!(tokens[10].code, lexer::token::EXISTS);
        assert_eq!(tokens[11].code, lexer::token::WITH);
        assert_eq!(tokens[12].code, lexer::token::RECURSIVE);
        assert_eq!(tokens[13].code, lexer::token::PARTITION);
        assert_eq!(tokens[14].code, lexer::token::OVER);
        assert_eq!(tokens[15].code, lexer::token::VALUES);
        assert_eq!(tokens[16].code, lexer::token::INTERSECT);
        assert_eq!(tokens[17].code, lexer::token::EXCEPT);
    }

    // ---------------------------------------------------------------
    // DML statements
    // ---------------------------------------------------------------

    #[test]
    fn parse_insert_values() {
        assert_parses_as(
            "INSERT INTO t(a) VALUES (1)",
            |r| {
                matches!(r, RelExpr::Insert { table, columns, .. }
                         if table == "t" && columns == &["a"])
            },
            "Insert",
        );
    }

    #[test]
    fn parse_insert_select() {
        assert_parses_as(
            "INSERT INTO t SELECT * FROM s",
            |r| matches!(r, RelExpr::Insert { table, .. } if table == "t"),
            "Insert with SELECT",
        );
    }

    #[test]
    fn parse_insert_default_values() {
        assert_parses_as(
            "INSERT INTO t DEFAULT VALUES",
            |r| {
                matches!(r, RelExpr::Insert { table, source, .. }
                         if table == "t"
                         && matches!(source.as_ref(), RelExpr::Values { rows } if rows.is_empty()))
            },
            "Insert DEFAULT VALUES",
        );
    }

    #[test]
    fn parse_insert_on_conflict_nothing() {
        assert_parses_as(
            "INSERT INTO t(a) VALUES (1) ON CONFLICT DO NOTHING",
            |r| {
                matches!(
                    r,
                    RelExpr::Insert {
                        on_conflict: Some(ra_core::algebra::OnConflict::DoNothing),
                        ..
                    }
                )
            },
            "Insert ON CONFLICT DO NOTHING",
        );
    }

    #[test]
    fn parse_insert_on_conflict_update() {
        assert_parses_as(
            "INSERT INTO t(a) VALUES (1) ON CONFLICT (a) DO UPDATE SET a = 2",
            |r| {
                matches!(
                    r,
                    RelExpr::Insert {
                        on_conflict: Some(ra_core::algebra::OnConflict::DoUpdate { .. }),
                        ..
                    }
                )
            },
            "Insert ON CONFLICT DO UPDATE",
        );
    }

    #[test]
    fn parse_insert_on_conflict_select_with_target() {
        assert_parses_as(
            "INSERT INTO t(a) VALUES (1) ON CONFLICT (a) DO SELECT",
            |r| {
                matches!(
                    r,
                    RelExpr::Insert {
                        on_conflict: Some(ra_core::algebra::OnConflict::DoSelect { target }),
                        ..
                    } if target == &["a".to_owned()]
                )
            },
            "Insert ON CONFLICT (a) DO SELECT (PG19)",
        );
    }

    #[test]
    fn parse_insert_on_conflict_select_bare() {
        assert_parses_as(
            "INSERT INTO t(a) VALUES (1) ON CONFLICT DO SELECT",
            |r| {
                matches!(
                    r,
                    RelExpr::Insert {
                        on_conflict: Some(ra_core::algebra::OnConflict::DoSelect { target }),
                        ..
                    } if target.is_empty()
                )
            },
            "Insert ON CONFLICT DO SELECT (bare, PG19)",
        );
    }

    #[test]
    fn parse_merge_upsert() {
        assert_parses_as(
            "MERGE INTO target t USING source s ON t.id = s.id \
             WHEN MATCHED THEN UPDATE SET v = s.v \
             WHEN NOT MATCHED THEN INSERT (id, v) VALUES (s.id, s.v)",
            |r| {
                let RelExpr::Merge {
                    target,
                    when_clauses,
                    ..
                } = r
                else {
                    return false;
                };
                target == "target"
                    && when_clauses.len() == 2
                    && matches!(
                        when_clauses[0].action,
                        ra_core::algebra::MergeAction::Update { .. }
                    )
                    && matches!(when_clauses[0].kind, ra_core::algebra::MergeMatchKind::Matched)
                    && matches!(
                        when_clauses[1].action,
                        ra_core::algebra::MergeAction::Insert { .. }
                    )
                    && matches!(
                        when_clauses[1].kind,
                        ra_core::algebra::MergeMatchKind::NotMatched
                    )
            },
            "MERGE upsert (matched update + not matched insert)",
        );
    }

    #[test]
    fn parse_merge_delete_and_by_source() {
        assert_parses_as(
            "MERGE INTO t USING s ON t.id = s.id \
             WHEN MATCHED AND s.del = 1 THEN DELETE \
             WHEN NOT MATCHED BY SOURCE THEN DELETE \
             WHEN NOT MATCHED THEN DO NOTHING",
            |r| {
                let RelExpr::Merge { when_clauses, .. } = r else {
                    return false;
                };
                when_clauses.len() == 3
                    && when_clauses[0].condition.is_some()
                    && matches!(
                        when_clauses[0].action,
                        ra_core::algebra::MergeAction::Delete
                    )
                    && matches!(
                        when_clauses[1].kind,
                        ra_core::algebra::MergeMatchKind::NotMatchedBySource
                    )
                    && matches!(
                        when_clauses[2].action,
                        ra_core::algebra::MergeAction::DoNothing
                    )
            },
            "MERGE with AND condition, BY SOURCE, and DO NOTHING",
        );
    }

    #[test]
    fn parse_graph_table() {
        assert_parses_as(
            "SELECT * FROM GRAPH_TABLE (g MATCH (a IS person)-[e IS knows]->(b IS person) \
             COLUMNS (a.name AS who, b.name AS friend))",
            |r| {
                // GraphTable is nested under the SELECT's Project.
                fn find_graph_table(e: &RelExpr) -> Option<&RelExpr> {
                    if matches!(e, RelExpr::GraphTable { .. }) {
                        return Some(e);
                    }
                    e.children().into_iter().find_map(find_graph_table)
                }
                let Some(RelExpr::GraphTable {
                    graph,
                    pattern,
                    columns,
                    ..
                }) = find_graph_table(r)
                else {
                    return false;
                };
                graph == "g"
                    && pattern.len() == 3
                    && columns.len() == 2
                    && matches!(
                        pattern[0],
                        ra_core::algebra::GraphPatternElement::Vertex { .. }
                    )
                    && matches!(
                        pattern[1],
                        ra_core::algebra::GraphPatternElement::Edge {
                            direction: ra_core::algebra::EdgeDirection::Right,
                            ..
                        }
                    )
            },
            "GRAPH_TABLE with vertex/edge/vertex pattern and COLUMNS",
        );
    }

    #[test]
    fn parse_graph_table_left_and_undirected_edges() {
        assert_parses_as(
            "SELECT * FROM GRAPH_TABLE (g MATCH (a)<-[e]-(b)-[f]-(c) COLUMNS (a.id AS x)) AS gt",
            |r| {
                fn find_graph_table(e: &RelExpr) -> Option<&RelExpr> {
                    if matches!(e, RelExpr::GraphTable { .. }) {
                        return Some(e);
                    }
                    e.children().into_iter().find_map(find_graph_table)
                }
                let Some(RelExpr::GraphTable {
                    pattern, alias, ..
                }) = find_graph_table(r)
                else {
                    return false;
                };
                pattern.len() == 5
                    && alias.as_deref() == Some("gt")
                    && matches!(
                        pattern[1],
                        ra_core::algebra::GraphPatternElement::Edge {
                            direction: ra_core::algebra::EdgeDirection::Left,
                            ..
                        }
                    )
                    && matches!(
                        pattern[3],
                        ra_core::algebra::GraphPatternElement::Edge {
                            direction: ra_core::algebra::EdgeDirection::Undirected,
                            ..
                        }
                    )
            },
            "GRAPH_TABLE with left and undirected edges and alias",
        );
    }

    #[test]
    fn parse_insert_returning() {
        assert_parses_as(
            "INSERT INTO t(a) VALUES (1) RETURNING a",            |r| matches!(r, RelExpr::Insert { returning: Some(cols), .. } if !cols.is_empty()),
            "Insert RETURNING",
        );
    }

    #[test]
    fn parse_update_simple() {
        assert_parses_as(
            "UPDATE t SET a = 1",
            |r| {
                matches!(r, RelExpr::Update { table, assignments, filter: None, .. }
                         if table == "t" && assignments.len() == 1)
            },
            "Update",
        );
    }

    #[test]
    fn parse_update_with_where() {
        assert_parses_as(
            "UPDATE t SET a = 1, b = 2 WHERE id = 5",
            |r| {
                matches!(r, RelExpr::Update { table, assignments, filter: Some(_), .. }
                         if table == "t" && assignments.len() == 2)
            },
            "Update with WHERE",
        );
    }

    #[test]
    fn parse_update_with_from() {
        assert_parses_as(
            "UPDATE t SET a = s.x FROM s WHERE t.id = s.id",
            |r| {
                matches!(
                    r,
                    RelExpr::Update {
                        from: Some(_),
                        filter: Some(_),
                        ..
                    }
                )
            },
            "Update with FROM",
        );
    }

    #[test]
    fn parse_update_returning() {
        assert_parses_as(
            "UPDATE t SET a = 1 RETURNING a",
            |r| matches!(r, RelExpr::Update { returning: Some(cols), .. } if !cols.is_empty()),
            "Update RETURNING",
        );
    }

    #[test]
    fn parse_delete_simple() {
        assert_parses_as(
            "DELETE FROM t",
            |r| {
                matches!(r, RelExpr::Delete { table, filter: None, using: None, .. }
                         if table == "t")
            },
            "Delete",
        );
    }

    #[test]
    fn parse_delete_with_where() {
        assert_parses_as(
            "DELETE FROM t WHERE id = 5",
            |r| {
                matches!(r, RelExpr::Delete { table, filter: Some(_), .. }
                         if table == "t")
            },
            "Delete with WHERE",
        );
    }

    #[test]
    fn parse_delete_with_using() {
        assert_parses_as(
            "DELETE FROM t USING s WHERE t.id = s.id",
            |r| {
                matches!(
                    r,
                    RelExpr::Delete {
                        using: Some(_),
                        filter: Some(_),
                        ..
                    }
                )
            },
            "Delete with USING",
        );
    }

    #[test]
    fn parse_delete_returning() {
        assert_parses_as(
            "DELETE FROM t WHERE id = 5 RETURNING id",
            |r| matches!(r, RelExpr::Delete { returning: Some(cols), .. } if !cols.is_empty()),
            "Delete RETURNING",
        );
    }

    #[test]
    fn tokenize_dml_keywords() {
        let tokens =
            lexer::tokenize("INSERT INTO UPDATE SET DELETE RETURNING CONFLICT DO NOTHING DEFAULT")
                .expect("DML keywords should tokenize");
        assert_eq!(tokens.len(), 10);
        assert_eq!(tokens[0].code, lexer::token::INSERT);
        assert_eq!(tokens[1].code, lexer::token::INTO);
        assert_eq!(tokens[2].code, lexer::token::UPDATE);
        assert_eq!(tokens[3].code, lexer::token::SET);
        assert_eq!(tokens[4].code, lexer::token::DELETE);
        assert_eq!(tokens[5].code, lexer::token::RETURNING);
        assert_eq!(tokens[6].code, lexer::token::CONFLICT);
        assert_eq!(tokens[7].code, lexer::token::DO);
        assert_eq!(tokens[8].code, lexer::token::NOTHING);
        assert_eq!(tokens[9].code, lexer::token::DEFAULT);
    }
}
