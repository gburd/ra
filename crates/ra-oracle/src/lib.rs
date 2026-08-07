//! PostgreSQL parse oracle (RA-STEERING §5.2).
#![expect(
    clippy::doc_markdown,
    clippy::struct_excessive_bools,
    clippy::match_same_arms,
    clippy::too_many_lines,
    reason = "prose mentions of PG/SQL identifiers; ParseFacts is a flat fact \
              record where separate bools read clearest; the RelExpr match arms \
              are kept split for one-node-per-arm readability; collect_tables is \
              a single wide match over every RelExpr variant"
)]
//!
//! Parses SQL with PostgreSQL's own parser (via the `pg_query` crate, which
//! vendors libpg_query — no live server needed), extracts comparable
//! *semantic parse facts*, extracts the same facts from Ra's `RelExpr` (via
//! `ra_parser::sql_to_relexpr`), and structurally diffs the two fact sets.
//!
//! We deliberately do **not** node-by-node diff the two parse trees: PG's tree
//! is syntactic and Ra's is a relational-algebra tree. Instead we extract a
//! small set of facts that are reliably comparable across both representations
//! (`ParseFacts`) and diff those. Any divergence is a candidate Ra
//! parser/analysis bug.
//!
//! ## PostgreSQL version caveat
//!
//! The `pg_query` crate vendors PostgreSQL **17**'s parser (libpg_query).
//! SQL syntax that only exists in PG 18/19 but that Ra already supports
//! (e.g. SQL/PGQ `GRAPH_TABLE`) will therefore show up as "Ra parsed but
//! PG failed" divergences. That is an accepted limitation, not an Ra bug,
//! until `pg_query` bumps its vendored PostgreSQL version.

use std::collections::BTreeSet;

use ra_core::algebra::RelExpr;
use serde::Serialize;

/// Semantic parse facts extractable from both PG's syntactic parse tree and
/// Ra's `RelExpr`. Only facts that can be pulled reliably from *both* sides
/// live here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ParseFacts {
    /// Base relation names referenced (sorted). Excludes CTE names.
    pub tables: BTreeSet<String>,
    /// Number of output columns in the top projection. `None` for `SELECT *`
    /// (star), which both sides represent specially rather than as a fixed
    /// arity.
    pub output_arity: Option<usize>,
    /// Number of join operations.
    pub join_count: usize,
    /// Whether the query has a WHERE clause.
    pub has_where: bool,
    /// Whether the query has a GROUP BY clause.
    pub has_group_by: bool,
    /// Whether the query has a HAVING clause.
    pub has_having: bool,
    /// Whether the query has an ORDER BY clause.
    pub has_order_by: bool,
    /// Whether the query has a LIMIT (or FETCH) clause.
    pub has_limit: bool,
    /// Whether the query is SELECT DISTINCT.
    pub has_distinct: bool,
}

/// The result of comparing PG parse facts against Ra parse facts.
#[derive(Debug, Clone, Serialize)]
pub struct ParseComparison {
    /// The SQL that was compared.
    pub sql: String,
    /// Facts extracted from PostgreSQL's parser.
    pub pg: ParseFacts,
    /// Facts extracted from Ra's parser.
    pub ra: ParseFacts,
    /// Human-readable per-field divergences. Empty == parse-equivalent.
    pub divergences: Vec<String>,
    /// Set when BOTH parsers rejected the SQL. This is *agreement*
    /// (both sides say "not valid SQL"), so it does NOT count as a
    /// divergence and does NOT flip `is_equivalent()` to false.
    pub both_rejected: Option<String>,
}

impl ParseComparison {
    /// True when PG and Ra agree on every extracted fact.
    #[must_use]
    pub fn is_equivalent(&self) -> bool {
        self.divergences.is_empty()
    }
}

impl ParseFacts {
    /// Extract parse facts from PostgreSQL's parser via `pg_query`.
    ///
    /// # Errors
    ///
    /// Returns an error if `pg_query` cannot parse the SQL.
    pub fn from_pg(sql: &str) -> anyhow::Result<Self> {
        use pg_query::protobuf::node::Node as PgNode;

        let parsed = pg_query::parse(sql)?;

        let tables: BTreeSet<String> = parsed.tables().into_iter().collect();

        // Reach the root SELECT statement (if this is a SELECT).
        let root_select = parsed
            .protobuf
            .stmts
            .first()
            .and_then(|s| s.stmt.as_ref())
            .and_then(|n| n.node.as_ref())
            .and_then(|node| match node {
                PgNode::SelectStmt(s) => Some(s.as_ref()),
                _ => None,
            });

        let mut facts = Self {
            tables,
            ..Self::default()
        };

        if let Some(sel) = root_select {
            // For UNION/INTERSECT/EXCEPT the top node has op != NONE and empty
            // clauses; descend to the left arm for clause facts / arity.
            let effective = leftmost_select_arm(sel);

            facts.output_arity = pg_target_arity(&effective.target_list);
            facts.has_where = effective.where_clause.is_some();
            facts.has_group_by = !effective.group_clause.is_empty();
            facts.has_having = effective.having_clause.is_some();
            // ORDER BY / LIMIT / DISTINCT belong to the top set-op node.
            facts.has_order_by = !sel.sort_clause.is_empty();
            facts.has_limit = pg_has_row_limit(sel.limit_count.as_deref());
            facts.has_distinct = !sel.distinct_clause.is_empty();
            facts.join_count = pg_count_joins(&effective.from_clause);
        }

        Ok(facts)
    }

    /// Extract parse facts from a Ra `RelExpr`.
    #[must_use]
    pub fn from_relexpr(expr: &RelExpr) -> Self {
        let mut tables = BTreeSet::new();
        let mut cte_names = BTreeSet::new();
        collect_tables(expr, &mut tables, &mut cte_names);
        // PG's `tables()` excludes CTE names; match that.
        for name in &cte_names {
            tables.remove(name);
        }

        Self {
            tables,
            output_arity: ra_output_arity(expr),
            join_count: ra_count_joins(expr),
            has_where: ra_has(expr, Clause::Where),
            has_group_by: ra_has(expr, Clause::GroupBy),
            has_having: ra_has(expr, Clause::Having),
            has_order_by: ra_has(expr, Clause::OrderBy),
            has_limit: ra_has(expr, Clause::Limit),
            has_distinct: ra_has(expr, Clause::Distinct),
        }
    }
}

/// Split a multi-statement SQL string into individual statements using
/// PostgreSQL's own scanner (via `pg_query`), which correctly ignores `;`
/// inside single-quoted, dollar-quoted, and comment text.
///
/// Returns owned, trimmed, non-empty statements. Note: this does *not* strip
/// psql `\` meta-commands — the caller should do that first, since they are
/// not SQL and the scanner will fold them into the following statement.
///
/// # Errors
///
/// Returns an error if PG's scanner rejects the input (e.g. an unterminated
/// quote). Callers running over messy corpora should fall back to a tolerant
/// splitter on error rather than aborting.
pub fn split_statements(sql: &str) -> anyhow::Result<Vec<String>> {
    let parts = pg_query::split_with_scanner(sql)?;
    Ok(parts
        .into_iter()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Compare PG and Ra parse facts for one SQL string.
///
/// A divergence is a human string like `tables: PG {t1,t2} vs Ra {t1}` for
/// each field that differs. `divergences.is_empty()` == parse-equivalent.
///
/// Parse failures on either side are reported as a single divergence rather
/// than an error, so a corpus checker can keep going.
///
/// # Errors
///
/// Never returns `Err` for parse failures (those become divergences). It only
/// propagates truly unexpected `pg_query` internal errors that are not parse
/// errors is not distinguished here — all `pg_query` failures become the
/// "PG failed to parse" divergence.
pub fn compare(sql: &str) -> anyhow::Result<ParseComparison> {
    let pg_res = ParseFacts::from_pg(sql);
    let ra_res = ra_parser::sql_to_relexpr(sql).map(|e| ParseFacts::from_relexpr(&e));

    let mut divergences = Vec::new();
    let mut both_rejected = None;

    let (pg, ra) = match (pg_res, ra_res) {
        (Ok(pg), Ok(ra)) => {
            diff_facts(&pg, &ra, &mut divergences);
            (pg, ra)
        }
        (Ok(pg), Err(e)) => {
            divergences.push(format!("PG parsed but Ra failed: {e}"));
            (pg, ParseFacts::default())
        }
        (Err(e), Ok(ra)) => {
            divergences.push(format!("Ra parsed but PG failed: {e}"));
            (ParseFacts::default(), ra)
        }
        (Err(pe), Err(re)) => {
            // Both reject — this is *agreement* (both sides call the SQL
            // invalid), so it is NOT a divergence: record it separately and
            // leave `divergences` empty so `is_equivalent()` stays true and
            // the CLI exits 0.
            both_rejected = Some(format!("both parsers rejected (PG: {pe}; Ra: {re})"));
            (ParseFacts::default(), ParseFacts::default())
        }
    };

    Ok(ParseComparison {
        sql: sql.to_owned(),
        pg,
        ra,
        divergences,
        both_rejected,
    })
}

fn diff_facts(pg: &ParseFacts, ra: &ParseFacts, out: &mut Vec<String>) {
    if pg.tables != ra.tables {
        out.push(format!(
            "tables: PG {} vs Ra {}",
            fmt_set(&pg.tables),
            fmt_set(&ra.tables)
        ));
    }
    if pg.output_arity != ra.output_arity {
        out.push(format!(
            "output_arity: PG {:?} vs Ra {:?}",
            pg.output_arity, ra.output_arity
        ));
    }
    if pg.join_count != ra.join_count {
        out.push(format!(
            "join_count: PG {} vs Ra {}",
            pg.join_count, ra.join_count
        ));
    }
    diff_bool("has_where", pg.has_where, ra.has_where, out);
    diff_bool("has_group_by", pg.has_group_by, ra.has_group_by, out);
    diff_bool("has_having", pg.has_having, ra.has_having, out);
    diff_bool("has_order_by", pg.has_order_by, ra.has_order_by, out);
    diff_bool("has_limit", pg.has_limit, ra.has_limit, out);
    diff_bool("has_distinct", pg.has_distinct, ra.has_distinct, out);
}

fn diff_bool(name: &str, pg: bool, ra: bool, out: &mut Vec<String>) {
    if pg != ra {
        out.push(format!("{name}: PG {pg} vs Ra {ra}"));
    }
}

fn fmt_set(s: &BTreeSet<String>) -> String {
    let mut out = String::from("{");
    out.push_str(&s.iter().cloned().collect::<Vec<_>>().join(","));
    out.push('}');
    out
}

/// True only when a PG `LIMIT` clause imposes a real row cap.
///
/// PostgreSQL parses `LIMIT ALL` as a `limitCount` node that is a NULL
/// constant (`AConst { isnull: true }`); it is documented as equivalent to
/// omitting `LIMIT`. Treat that (and an absent clause) as no row limit so
/// this fact matches Ra's `u64::MAX` "no limit" sentinel.
fn pg_has_row_limit(limit_count: Option<&pg_query::protobuf::Node>) -> bool {
    use pg_query::protobuf::node::Node as PgNode;
    match limit_count.and_then(|n| n.node.as_ref()) {
        None => false,
        Some(PgNode::AConst(c)) => !c.isnull,
        Some(_) => true,
    }
}

// ── PG-side extraction helpers ─────────────────────────────

/// Descend into the leftmost arm of a set-op (UNION/INTERSECT/EXCEPT) tree to
/// find the SELECT that carries the projection / WHERE / GROUP BY facts. For a
/// plain SELECT this returns the node itself.
fn leftmost_select_arm(sel: &pg_query::protobuf::SelectStmt) -> &pg_query::protobuf::SelectStmt {
    let mut cur = sel;
    while let Some(larg) = cur.larg.as_ref() {
        cur = larg.as_ref();
    }
    cur
}

/// Output arity of a PG target list. `None` if the only target is a bare star
/// (`SELECT *`), matching how Ra represents star specially.
fn pg_target_arity(target_list: &[pg_query::protobuf::Node]) -> Option<usize> {
    use pg_query::protobuf::node::Node as PgNode;

    if target_list.is_empty() {
        return None;
    }

    let is_bare_star = target_list.len() == 1
        && target_list.iter().all(|n| match n.node.as_ref() {
            Some(PgNode::ResTarget(rt)) => rt.val.as_ref().is_some_and(|v| match v.node.as_ref() {
                Some(PgNode::ColumnRef(cr)) => {
                    cr.fields.len() == 1
                        && matches!(
                            cr.fields.first().and_then(|f| f.node.as_ref()),
                            Some(PgNode::AStar(_))
                        )
                }
                _ => false,
            }),
            _ => false,
        });

    if is_bare_star {
        None
    } else {
        Some(target_list.len())
    }
}

/// Count JoinExpr nodes reachable in a FROM clause, plus implicit comma-joins
/// (`FROM a, b` is a join even though PG models it as two range-table entries
/// rather than a `JoinExpr`). `join_count = explicit_joins + (from_items - 1)`
/// when there is more than one top-level FROM item, matching how Ra lowers
/// comma-separated FROM into a join tree.
fn pg_count_joins(from_clause: &[pg_query::protobuf::Node]) -> usize {
    let explicit: usize = from_clause.iter().map(pg_count_joins_node).sum();
    let implicit = from_clause.len().saturating_sub(1);
    explicit + implicit
}

fn pg_count_joins_node(node: &pg_query::protobuf::Node) -> usize {
    use pg_query::protobuf::node::Node as PgNode;
    match node.node.as_ref() {
        Some(PgNode::JoinExpr(j)) => {
            let l = j.larg.as_ref().map_or(0, |n| pg_count_joins_node(n));
            let r = j.rarg.as_ref().map_or(0, |n| pg_count_joins_node(n));
            1 + l + r
        }
        _ => 0,
    }
}

// ── Ra-side extraction helpers ─────────────────────────────

/// True when a projection is the `SELECT *` star sentinel Ra uses: a single
/// unqualified column named `"*"`. PG represents star as `A_Star` and yields
/// no fixed arity, so we must too.
fn is_star_projection(columns: &[ra_core::algebra::ProjectionColumn]) -> bool {
    use ra_core::expr::{ColumnRef, Expr};
    columns.len() == 1
        && matches!(
            &columns[0].expr,
            Expr::Column(ColumnRef { table: None, column }) if column == "*"
        )
}

/// Collect base table names referenced by scans, and (separately) the names
/// introduced by CTE definitions. CTE names are removed from the table set by
/// the caller so the result matches PG's `tables()` (which excludes CTEs).
/// Descends into scalar subqueries (IN/EXISTS/scalar) so their base tables are
/// counted, matching PG walking the whole tree.
fn collect_tables(expr: &RelExpr, out: &mut BTreeSet<String>, ctes: &mut BTreeSet<String>) {
    match expr {
        RelExpr::Scan { table, .. }
        | RelExpr::IndexScan { table, .. }
        | RelExpr::IndexOnlyScan { table, .. }
        | RelExpr::BitmapIndexScan { table, .. }
        | RelExpr::ParallelScan { table, .. } => {
            // `__dual` is Ra's synthetic no-FROM relation (Oracle-DUAL-style);
            // it is not a base table PG's `tables()` would report, so skip it.
            if table != "__dual" {
                out.insert(table.clone());
            }
        }
        RelExpr::MvScan { view_name, .. } => {
            out.insert(view_name.clone());
        }
        RelExpr::Filter { predicate, input } => {
            collect_expr_tables(predicate, out, ctes);
            collect_tables(input, out, ctes);
        }
        RelExpr::Project { columns, input } => {
            for c in columns {
                collect_expr_tables(&c.expr, out, ctes);
            }
            collect_tables(input, out, ctes);
        }
        RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::DistinctOn { input, .. }
        | RelExpr::IncrementalSort { input, .. }
        | RelExpr::RowPattern { input, .. }
        | RelExpr::ParallelAggregate { input, .. }
        | RelExpr::Gather { input, .. }
        | RelExpr::TopK { input, .. }
        | RelExpr::SubqueryAlias { input, .. }
        | RelExpr::VectorFilter { input, .. } => collect_tables(input, out, ctes),
        RelExpr::Join {
            condition,
            left,
            right,
            ..
        } => {
            collect_expr_tables(condition, out, ctes);
            collect_tables(left, out, ctes);
            collect_tables(right, out, ctes);
        }
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. }
        | RelExpr::ParallelHashJoin { left, right, .. } => {
            collect_tables(left, out, ctes);
            collect_tables(right, out, ctes);
        }
        RelExpr::CTE {
            name,
            definition,
            body,
        } => {
            ctes.insert(name.clone());
            collect_tables(definition, out, ctes);
            collect_tables(body, out, ctes);
        }
        RelExpr::RecursiveCTE {
            name,
            base_case,
            recursive_case,
            body,
            ..
        } => {
            ctes.insert(name.clone());
            collect_tables(base_case, out, ctes);
            collect_tables(recursive_case, out, ctes);
            collect_tables(body, out, ctes);
        }
        RelExpr::Unnest {
            input: Some(input), ..
        }
        | RelExpr::TableFunction {
            input: Some(input), ..
        } => collect_tables(input, out, ctes),
        RelExpr::Insert { table, source, .. } => {
            out.insert(table.clone());
            collect_tables(source, out, ctes);
        }
        RelExpr::Update { table, from, .. } => {
            out.insert(table.clone());
            if let Some(from) = from {
                collect_tables(from, out, ctes);
            }
        }
        RelExpr::Delete { table, using, .. } => {
            out.insert(table.clone());
            if let Some(using) = using {
                collect_tables(using, out, ctes);
            }
        }
        RelExpr::Merge { target, source, .. } => {
            out.insert(target.clone());
            collect_tables(source, out, ctes);
        }
        _ => {}
    }
}

/// Descend into a scalar expression collecting base tables from any nested
/// subqueries (IN/EXISTS/scalar/ANY/ALL). PG's `tables()` walks these, so we
/// must too for the table sets to agree.
fn collect_expr_tables(
    expr: &ra_core::expr::Expr,
    out: &mut BTreeSet<String>,
    ctes: &mut BTreeSet<String>,
) {
    use ra_core::expr::Expr;
    match expr {
        Expr::SubQuery {
            query, test_expr, ..
        } => {
            collect_tables(query, out, ctes);
            if let Some(t) = test_expr {
                collect_expr_tables(t, out, ctes);
            }
        }
        Expr::BinOp { left, right, .. } => {
            collect_expr_tables(left, out, ctes);
            collect_expr_tables(right, out, ctes);
        }
        Expr::UnaryOp { operand, .. } => collect_expr_tables(operand, out, ctes),
        Expr::Function { args, .. } | Expr::Array(args) => {
            for a in args {
                collect_expr_tables(a, out, ctes);
            }
        }
        Expr::Case {
            operand,
            when_clauses,
            else_result,
        } => {
            if let Some(o) = operand {
                collect_expr_tables(o, out, ctes);
            }
            for (w, t) in when_clauses {
                collect_expr_tables(w, out, ctes);
                collect_expr_tables(t, out, ctes);
            }
            if let Some(e) = else_result {
                collect_expr_tables(e, out, ctes);
            }
        }
        Expr::Cast { expr, .. } | Expr::FieldAccess { expr, .. } => {
            collect_expr_tables(expr, out, ctes);
        }
        Expr::ArrayIndex(a, b) => {
            collect_expr_tables(a, out, ctes);
            collect_expr_tables(b, out, ctes);
        }
        _ => {}
    }
}

/// The output arity of Ra's top projection. `None` if the plan does not have a
/// top `Project` that fixes an arity (i.e. `SELECT *`, which Ra leaves as a
/// bare scan / no projection). Peels off ordering/limiting/distinct wrappers
/// to find the projection that establishes the output shape.
fn ra_output_arity(expr: &RelExpr) -> Option<usize> {
    match expr {
        RelExpr::Project { columns, .. } => {
            if is_star_projection(columns) {
                None
            } else {
                Some(columns.len())
            }
        }
        // Set operations: arity is that of an arm (both arms have equal arity).
        RelExpr::Union { left, .. }
        | RelExpr::Intersect { left, .. }
        | RelExpr::Except { left, .. } => ra_output_arity(left),
        // CTE / recursive CTE: arity is the body's arity.
        RelExpr::CTE { body, .. } | RelExpr::RecursiveCTE { body, .. } => ra_output_arity(body),
        // Wrappers that sit above the projection — peel them.
        RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::DistinctOn { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::TopK { input, .. } => ra_output_arity(input),
        // Aggregate establishes its own output arity (group keys + aggregates).
        RelExpr::Aggregate {
            group_by,
            aggregates,
            ..
        } => Some(group_by.len() + aggregates.len()),
        _ => None,
    }
}

/// Count Join nodes belonging to the query's OWN scope (does not count set
/// operations — PG models UNION/etc. as SetOperationStmt, not JoinExpr, so
/// neither side counts them). Joins inside a FROM-subquery (derived table) are
/// NOT counted: PG reports clause facts only from the root SELECT, so we stop
/// descending at the derived-table boundary (see `own_scope` docs) to stay
/// symmetric. `seen_project` becomes true once we pass the top projection; any
/// `Project` reached after that begins a derived table and terminates descent.
fn ra_count_joins(expr: &RelExpr) -> usize {
    ra_count_joins_scoped(expr, false)
}

fn ra_count_joins_scoped(expr: &RelExpr, seen_project: bool) -> usize {
    match expr {
        // A `Project` after we've already passed the top projection is a
        // derived-table boundary — do not descend.
        RelExpr::Project { .. } if seen_project => 0,
        RelExpr::Project { input, .. } => ra_count_joins_scoped(input, true),
        RelExpr::Join { left, right, .. } | RelExpr::ParallelHashJoin { left, right, .. } => {
            1 + ra_count_joins_scoped(left, seen_project)
                + ra_count_joins_scoped(right, seen_project)
        }
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            ra_count_joins_scoped(left, seen_project) + ra_count_joins_scoped(right, seen_project)
        }
        // Own-query clause nodes: descend, keeping scope.
        RelExpr::Filter { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::ParallelAggregate { input, .. }
        | RelExpr::Gather { input, .. }
        | RelExpr::VectorFilter { input, .. } => ra_count_joins_scoped(input, seen_project),
        // Ordering/limit/distinct/window wrappers: own-query only when they sit
        // ABOVE the top projection. Below it (`seen_project`) they belong to a
        // derived table — stop.
        RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::DistinctOn { input, .. }
        | RelExpr::IncrementalSort { input, .. }
        | RelExpr::RowPattern { input, .. }
        | RelExpr::TopK { input, .. } => {
            if seen_project {
                0
            } else {
                ra_count_joins_scoped(input, seen_project)
            }
        }
        RelExpr::CTE {
            definition, body, ..
        } => ra_count_joins_scoped(definition, false) + ra_count_joins_scoped(body, seen_project),
        _ => 0,
    }
}

#[derive(Clone, Copy)]
enum Clause {
    Where,
    GroupBy,
    Having,
    OrderBy,
    Limit,
    Distinct,
}

/// Whether a Ra plan contains a node implying the given SQL clause, scoped to
/// the query's OWN operators. Clauses that live inside a FROM-subquery (derived
/// table) are NOT counted: PG reads clause facts only from the root SELECT, so
/// we stop descending at the derived-table boundary to stay symmetric.
///
/// A derived table lowers to a nested `Project` with no explicit boundary node
/// (`SELECT x FROM (SELECT ...) s` -> `Project { input: Project { .. } }`). The
/// FIRST `Project` reached is the query's top projection; ANY `Project` after
/// that begins a derived table and terminates the own-scope walk. WHERE/HAVING
/// are distinguished by whether the `Filter`'s input is an Aggregate.
fn ra_has(expr: &RelExpr, clause: Clause) -> bool {
    match clause {
        Clause::Having => ra_has_having(expr, false),
        Clause::Where => ra_has_where(expr, false),
        _ => ra_has_simple(expr, clause, false),
    }
}

/// Detect Sort/Limit/Distinct/GroupBy in the query's own scope. Ordering/limit/
/// distinct/window wrappers belong to the query only when they sit ABOVE the
/// top projection; below it (`seen_project`) they belong to a derived table, so
/// we neither fire nor descend through them.
fn ra_has_simple(expr: &RelExpr, clause: Clause, seen_project: bool) -> bool {
    // Any wrapper below the top projection starts a derived table — stop.
    if seen_project && is_derived_table_wrapper(expr) {
        return false;
    }
    let hit = match clause {
        Clause::OrderBy => matches!(expr, RelExpr::Sort { .. } | RelExpr::TopK { .. }),
        Clause::Limit => {
            matches!(expr, RelExpr::Limit { count, .. } if *count != u64::MAX)
                || matches!(expr, RelExpr::TopK { .. })
        }
        Clause::Distinct => matches!(expr, RelExpr::Distinct { .. } | RelExpr::DistinctOn { .. }),
        Clause::GroupBy => {
            matches!(expr, RelExpr::Aggregate { group_by, .. } if !group_by.is_empty())
        }
        Clause::Where | Clause::Having => false,
    };
    hit || descend_simple(expr, clause, seen_project)
}

/// A node that, when found BELOW the top projection, marks a derived-table
/// boundary: a nested `Project` (the subquery's own projection) or any
/// ordering/limit/distinct/window wrapper (the outer query keeps those ABOVE
/// its top projection, so below it they must be a subquery's).
fn is_derived_table_wrapper(expr: &RelExpr) -> bool {
    matches!(
        expr,
        RelExpr::Project { .. }
            | RelExpr::Sort { .. }
            | RelExpr::Limit { .. }
            | RelExpr::Window { .. }
            | RelExpr::Distinct { .. }
            | RelExpr::DistinctOn { .. }
            | RelExpr::IncrementalSort { .. }
            | RelExpr::RowPattern { .. }
            | RelExpr::TopK { .. }
    )
}

/// Descend through the query's own wrappers, tracking whether we have passed
/// the top projection so a deeper derived-table wrapper stops the walk.
fn descend_simple(expr: &RelExpr, clause: Clause, seen_project: bool) -> bool {
    match expr {
        RelExpr::Project { input, .. } => ra_has_simple(input, clause, true),
        RelExpr::Filter { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::DistinctOn { input, .. }
        | RelExpr::IncrementalSort { input, .. }
        | RelExpr::ParallelAggregate { input, .. }
        | RelExpr::Gather { input, .. }
        | RelExpr::TopK { input, .. }
        | RelExpr::VectorFilter { input, .. } => ra_has_simple(input, clause, seen_project),
        RelExpr::Join { left, right, .. } | RelExpr::ParallelHashJoin { left, right, .. } => {
            ra_has_simple(left, clause, seen_project) || ra_has_simple(right, clause, seen_project)
        }
        _ => false,
    }
}

/// HAVING = a Filter whose input (through projections) is an Aggregate, in the
/// query's own scope. Stops at the derived-table `Project` boundary.
fn ra_has_having(expr: &RelExpr, seen_project: bool) -> bool {
    if seen_project && is_derived_table_wrapper(expr) {
        return false;
    }
    match expr {
        RelExpr::Filter { input, .. } => {
            filter_input_is_aggregate(input) || ra_has_having(input, seen_project)
        }
        RelExpr::Project { input, .. } => ra_has_having(input, true),
        RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::DistinctOn { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::TopK { input, .. } => ra_has_having(input, seen_project),
        _ => false,
    }
}

fn filter_input_is_aggregate(input: &RelExpr) -> bool {
    match input {
        RelExpr::Aggregate { .. } | RelExpr::ParallelAggregate { .. } => true,
        RelExpr::Project { input, .. } => filter_input_is_aggregate(input),
        _ => false,
    }
}

/// WHERE = a Filter whose input is NOT an Aggregate (that would be HAVING), in
/// the query's own scope. Stops at the derived-table `Project` boundary so a
/// WHERE inside a FROM-subquery is not attributed to the outer query.
fn ra_has_where(expr: &RelExpr, seen_project: bool) -> bool {
    if seen_project && is_derived_table_wrapper(expr) {
        return false;
    }
    match expr {
        RelExpr::Filter { input, .. } => {
            !filter_input_is_aggregate(input) || ra_has_where(input, seen_project)
        }
        RelExpr::Project { input, .. } => ra_has_where(input, true),
        RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::DistinctOn { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::IncrementalSort { input, .. }
        | RelExpr::TopK { input, .. }
        | RelExpr::VectorFilter { input, .. } => ra_has_where(input, seen_project),
        RelExpr::Join { left, right, .. } | RelExpr::ParallelHashJoin { left, right, .. } => {
            ra_has_where(left, seen_project) || ra_has_where(right, seen_project)
        }
        _ => false,
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn simple_select_agrees() {
        let cmp = compare("SELECT a FROM t WHERE a > 1").expect("compare");
        assert!(
            cmp.is_equivalent(),
            "expected agreement, got: {:?}",
            cmp.divergences
        );
        assert_eq!(cmp.pg.tables, cmp.ra.tables);
        assert!(cmp.pg.tables.contains("t"));
        assert!(cmp.pg.has_where);
        assert_eq!(cmp.pg.output_arity, Some(1));
    }

    #[test]
    fn join_and_where_agree() {
        let cmp =
            compare("SELECT a.x FROM t1 a JOIN t2 b ON a.id=b.id WHERE a.x>5").expect("compare");
        assert!(
            cmp.is_equivalent(),
            "expected agreement, got: {:?}",
            cmp.divergences
        );
        assert_eq!(cmp.pg.join_count, 1);
        assert_eq!(cmp.pg.tables, ["t1".to_owned(), "t2".to_owned()].into());
    }

    #[test]
    fn star_select_has_no_arity() {
        let cmp = compare("SELECT * FROM users").expect("compare");
        assert_eq!(cmp.pg.output_arity, None);
        assert_eq!(cmp.ra.output_arity, None);
    }

    #[test]
    fn group_by_having() {
        let cmp = compare("SELECT status FROM orders GROUP BY status HAVING count(*)>5")
            .expect("compare");
        assert!(cmp.pg.has_group_by);
        assert!(cmp.pg.has_having);
        // Ra facts extracted independently.
        assert!(cmp.ra.has_group_by, "Ra missed GROUP BY: {:?}", cmp.ra);
    }

    #[test]
    fn distinct_order_limit() {
        let cmp = compare("SELECT DISTINCT x FROM t ORDER BY x LIMIT 3").expect("compare");
        assert!(cmp.pg.has_distinct);
        assert!(cmp.pg.has_order_by);
        assert!(cmp.pg.has_limit);
    }

    #[test]
    fn limit_all_and_offset_before_limit_agree() {
        // LIMIT ALL is documented as "no row limit": both sides must report
        // has_limit == false and agree overall. OFFSET-before-LIMIT is just a
        // clause-order variant PG accepts.
        for sql in [
            "SELECT x FROM t LIMIT ALL",
            "SELECT x FROM t LIMIT ALL OFFSET 5",
            "SELECT x FROM t OFFSET 5 LIMIT ALL",
        ] {
            let cmp = compare(sql).expect("compare");
            assert!(!cmp.pg.has_limit, "LIMIT ALL is not a row cap: {sql:?}");
            assert!(
                cmp.is_equivalent(),
                "{sql:?} diverged: {:?}",
                cmp.divergences
            );
        }
        let cmp = compare("SELECT x FROM t OFFSET 5 LIMIT 10").expect("compare");
        assert!(cmp.pg.has_limit);
        assert!(cmp.is_equivalent(), "diverged: {:?}", cmp.divergences);
    }

    #[test]
    fn both_parsers_rejecting_is_agreement_not_divergence() {
        // Garbage SQL both reject -> AGREEMENT (both say "invalid"), so it is
        // equivalent with zero divergences, but recorded in `both_rejected`.
        let cmp = compare("SELECT FROM WHERE )(").expect("compare returns Ok even on parse error");
        assert!(cmp.is_equivalent(), "both-rejected must not diverge");
        assert!(cmp.divergences.is_empty());
        assert!(cmp.both_rejected.is_some());
    }

    #[test]
    fn derived_table_clauses_do_not_cry_wolf() {
        // Clauses inside a FROM-subquery belong to the subquery, not the outer
        // query: PG reports them nested, so Ra must not attribute them to root.
        for sql in [
            "SELECT x FROM (SELECT x FROM t WHERE x > 1) s",
            "SELECT x FROM (SELECT x FROM t GROUP BY x) s",
            "SELECT x FROM (SELECT x FROM t ORDER BY x LIMIT 5) s",
            "SELECT x FROM (SELECT DISTINCT x FROM t) s",
            "SELECT x FROM (SELECT a.x FROM t a JOIN u b ON a.id = b.id) s",
        ] {
            let cmp = compare(sql).expect("compare");
            assert!(
                cmp.is_equivalent(),
                "derived-table false positive for {sql:?}: {:?}",
                cmp.divergences
            );
        }
    }

    #[test]
    fn top_level_clause_over_subquery_stays_visible() {
        // A WHERE that is genuinely at the outer level must still register on
        // both sides even though the FROM item is a derived table.
        let cmp = compare("SELECT x FROM (SELECT x FROM t) s WHERE x > 1").expect("compare");
        assert!(cmp.pg.has_where, "PG should see top-level WHERE");
        assert!(cmp.ra.has_where, "Ra should see top-level WHERE");
        assert!(
            cmp.is_equivalent(),
            "unexpected divergence: {:?}",
            cmp.divergences
        );
    }

    #[test]
    fn dml_target_tables_collected() {
        for (sql, want) in [
            ("INSERT INTO t (x) VALUES (1)", vec!["t"]),
            ("INSERT INTO t SELECT x FROM u", vec!["t", "u"]),
            ("UPDATE t SET x = 1 WHERE id = 2", vec!["t"]),
            ("DELETE FROM t WHERE id = 1", vec!["t"]),
        ] {
            let cmp = compare(sql).expect("compare");
            let want: BTreeSet<String> = want.into_iter().map(str::to_owned).collect();
            assert_eq!(cmp.ra.tables, want, "Ra tables for {sql:?}");
            assert_eq!(cmp.pg.tables, want, "PG tables for {sql:?}");
            assert!(
                cmp.is_equivalent(),
                "DML divergence {sql:?}: {:?}",
                cmp.divergences
            );
        }
    }

    #[test]
    fn window_top_node_has_arity() {
        // A window function as the top node must not report output_arity=None.
        let cmp = compare("SELECT x, row_number() OVER (ORDER BY x) FROM t").expect("compare");
        assert_eq!(cmp.ra.output_arity, Some(2));
        assert_eq!(cmp.pg.output_arity, Some(2));
        assert!(
            cmp.is_equivalent(),
            "window divergence: {:?}",
            cmp.divergences
        );
    }

    #[test]
    fn diverge_when_ra_fails_but_pg_ok() {
        // If we can find SQL PG accepts and Ra rejects, this fires. We do not
        // hard-assert a specific query (Ra's grammar evolves); instead verify
        // the machinery reports the right shape for a synthetic Ra failure by
        // comparing the two sides directly.
        let pg = ParseFacts::from_pg("SELECT 1").expect("pg parse");
        let ra = ParseFacts::default();
        let mut d = Vec::new();
        diff_facts(&pg, &ra, &mut d);
        assert!(
            d.iter().any(|s| s.starts_with("output_arity")),
            "expected an output_arity divergence, got {d:?}"
        );
    }
}
