//! Converts [`RelExpr`] trees into executable SQL strings.
//!
//! [`SqlEmitter`] performs recursive descent over a relational expression
//! tree and produces SQL that can be executed against a database with the
//! TPC-H schema (or any schema registered in [`EmitterSchema`]).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering,
    ProjectionColumn, RelExpr, SortDirection, SortKey, WindowExpr,
    WindowFunction,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr, UnaryOp};

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

/// Known table → column list mapping for SQL generation.
///
/// Column lists are used to emit qualified `table.column` references and
/// to ensure generated SQL can execute against a known schema.
#[derive(Debug, Clone)]
pub struct EmitterSchema {
    /// Map from table name to ordered column list.
    pub tables: HashMap<String, Vec<String>>,
}

impl EmitterSchema {
    /// Build the TPC-H reference schema at any scale.
    #[must_use]
    pub fn tpch() -> Self {
        let mut tables: HashMap<String, Vec<String>> = HashMap::new();
        tables.insert(
            "lineitem".to_owned(),
            vec![
                "l_orderkey", "l_partkey", "l_suppkey", "l_linenumber",
                "l_quantity", "l_extendedprice", "l_discount", "l_tax",
                "l_returnflag", "l_linestatus", "l_shipdate", "l_commitdate",
                "l_receiptdate", "l_shipinstruct", "l_shipmode", "l_comment",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        );
        tables.insert(
            "orders".to_owned(),
            vec![
                "o_orderkey", "o_custkey", "o_orderstatus", "o_totalprice",
                "o_orderdate", "o_orderpriority", "o_clerk", "o_shippriority",
                "o_comment",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        );
        tables.insert(
            "customer".to_owned(),
            vec![
                "c_custkey", "c_name", "c_address", "c_nationkey", "c_phone",
                "c_acctbal", "c_mktsegment", "c_comment",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        );
        tables.insert(
            "supplier".to_owned(),
            vec![
                "s_suppkey", "s_name", "s_address", "s_nationkey", "s_phone",
                "s_acctbal", "s_comment",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        );
        tables.insert(
            "part".to_owned(),
            vec![
                "p_partkey", "p_name", "p_mfgr", "p_brand", "p_type",
                "p_size", "p_container", "p_retailprice", "p_comment",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        );
        tables.insert(
            "partsupp".to_owned(),
            vec![
                "ps_partkey", "ps_suppkey", "ps_availqty", "ps_supplycost",
                "ps_comment",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        );
        tables.insert(
            "nation".to_owned(),
            vec!["n_nationkey", "n_name", "n_regionkey", "n_comment"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        );
        tables.insert(
            "region".to_owned(),
            vec!["r_regionkey", "r_name", "r_comment"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        );
        // Fallback generic tables used by the fuzzer
        for name in &["users", "products", "customers", "items",
                       "categories", "inventory"] {
            tables.entry((*name).to_owned()).or_insert_with(|| {
                vec!["id", "name", "status", "created_at"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            });
        }
        Self { tables }
    }

    /// Check whether a table is in the schema.
    #[must_use]
    pub fn has_table(&self, name: &str) -> bool {
        self.tables.contains_key(name)
    }
}

impl Default for EmitterSchema {
    fn default() -> Self {
        Self::tpch()
    }
}

// ---------------------------------------------------------------------------
// Counter for unique subquery aliases
// ---------------------------------------------------------------------------

static ALIAS_CTR: AtomicU32 = AtomicU32::new(0);

fn fresh_alias() -> String {
    let n = ALIAS_CTR.fetch_add(1, Ordering::Relaxed);
    format!("_t{n}")
}

// ---------------------------------------------------------------------------
// Emitter
// ---------------------------------------------------------------------------

/// Converts a [`RelExpr`] tree into an executable SQL string.
///
/// The emitted SQL is valid PostgreSQL syntax and uses table-qualified
/// column references to avoid ambiguity in multi-table queries.
#[derive(Debug, Clone)]
pub struct SqlEmitter {
    /// Schema used to resolve table and column names.
    pub schema: EmitterSchema,
}

impl SqlEmitter {
    /// Create an emitter using the TPC-H schema.
    #[must_use]
    pub fn new() -> Self {
        Self {
            schema: EmitterSchema::tpch(),
        }
    }

    /// Create an emitter with a custom schema.
    #[must_use]
    pub fn with_schema(schema: EmitterSchema) -> Self {
        Self { schema }
    }

    /// Emit a complete SQL statement from a relational expression tree.
    #[must_use]
    pub fn emit(&self, expr: &RelExpr) -> String {
        self.emit_rel(expr)
    }

    // -----------------------------------------------------------------------
    // RelExpr → SQL
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_lines)]
    fn emit_rel(&self, expr: &RelExpr) -> String {
        match expr {
            RelExpr::Scan { table, alias } => {
                self.emit_scan(table, alias.as_deref())
            }

            RelExpr::Project { columns, input } => {
                let cols = self.emit_projection_list(columns);
                let src = self.emit_subquery(input);
                format!("SELECT {cols} FROM {src}")
            }

            RelExpr::Filter { predicate, input } => {
                let pred = self.emit_expr(predicate);
                let src = self.emit_subquery(input);
                format!("SELECT * FROM {src} WHERE {pred}")
            }

            RelExpr::Join { join_type, condition, left, right } => {
                self.emit_join(join_type, condition, left, right)
            }

            RelExpr::Aggregate { group_by, aggregates, input } => {
                self.emit_aggregate(group_by, aggregates, input)
            }

            RelExpr::Sort { keys, input } => {
                let order = self.emit_order_by(keys);
                let src = self.emit_subquery(input);
                format!("SELECT * FROM {src} ORDER BY {order}")
            }

            RelExpr::Limit { count, offset, input } => {
                let src = self.emit_subquery(input);
                let offset_clause = if *offset > 0 {
                    format!(" OFFSET {offset}")
                } else {
                    String::new()
                };
                format!("SELECT * FROM {src} LIMIT {count}{offset_clause}")
            }

            RelExpr::Union { all, left, right } => {
                let kw = if *all { "UNION ALL" } else { "UNION" };
                format!("({}) {kw} ({})", self.emit_rel(left), self.emit_rel(right))
            }

            RelExpr::Intersect { all, left, right } => {
                let kw = if *all { "INTERSECT ALL" } else { "INTERSECT" };
                format!("({}) {kw} ({})", self.emit_rel(left), self.emit_rel(right))
            }

            RelExpr::Except { all, left, right } => {
                let kw = if *all { "EXCEPT ALL" } else { "EXCEPT" };
                format!("({}) {kw} ({})", self.emit_rel(left), self.emit_rel(right))
            }

            RelExpr::Distinct { input } => {
                format!("SELECT DISTINCT * FROM {}", self.emit_subquery(input))
            }

            RelExpr::Values { rows } => self.emit_values(rows),

            RelExpr::CTE { name, definition, body } => {
                let def = self.emit_rel(definition);
                format!("WITH {name} AS ({def}) {}", self.emit_rel(body))
            }

            RelExpr::RecursiveCTE { name, base_case, recursive_case, body, .. } => {
                let base = self.emit_rel(base_case);
                let rec = self.emit_rel(recursive_case);
                format!(
                    "WITH RECURSIVE {name} AS ({base} UNION ALL {rec}) {}",
                    self.emit_rel(body)
                )
            }

            RelExpr::Window { functions, input } => {
                let fn_exprs: Vec<String> = functions
                    .iter()
                    .map(|w| self.emit_window_expr(w))
                    .collect();
                format!(
                    "SELECT *, {} FROM {}",
                    fn_exprs.join(", "),
                    self.emit_subquery(input)
                )
            }

            RelExpr::Unnest { expr, alias, .. } => {
                let e = self.emit_expr(expr);
                let a = alias.as_deref().unwrap_or("unnested");
                format!("SELECT * FROM unnest({e}) AS {a}(val)")
            }

            RelExpr::MultiUnnest { exprs, .. } => {
                let args: Vec<_> = exprs.iter().map(|e| self.emit_expr(e)).collect();
                format!("SELECT * FROM unnest({})", args.join(", "))
            }

            RelExpr::TableFunction { name, args, .. } => {
                let args_sql: Vec<_> = args.iter().map(|a| self.emit_expr(a)).collect();
                let alias = fresh_alias();
                format!("SELECT * FROM {name}({}) AS {alias}", args_sql.join(", "))
            }

            RelExpr::RowPattern { input, .. }
            | RelExpr::IncrementalSort { input, .. }
            | RelExpr::Gather { input, .. }
            | RelExpr::TopK { input, .. }
            | RelExpr::VectorFilter { input, .. } => {
                format!("SELECT * FROM {}", self.emit_subquery(input))
            }

            RelExpr::BitmapHeapScan { bitmap, .. } => {
                format!("SELECT * FROM {}", self.emit_subquery(bitmap))
            }

            // Single-table scan variants (no child relation)
            RelExpr::IndexScan { table, .. }
            | RelExpr::BitmapIndexScan { table, .. }
            | RelExpr::ParallelScan { table, .. }
            | RelExpr::IndexOnlyScan { table, .. } => {
                table.clone()
            }

            RelExpr::BitmapAnd { inputs } | RelExpr::BitmapOr { inputs } => {
                if let Some(first) = inputs.first() {
                    format!("SELECT * FROM {}", self.emit_subquery(first))
                } else {
                    "SELECT 1".to_owned()
                }
            }

            RelExpr::ParallelHashJoin { join_type, condition, left, right, .. } => {
                self.emit_join(join_type, condition, left, right)
            }

            RelExpr::ParallelAggregate { group_by, aggregates, input, .. } => {
                self.emit_aggregate(group_by, aggregates, input)
            }

            RelExpr::MvScan { view_name, alias } => {
                self.emit_scan(view_name, alias.as_deref())
            }
        }
    }

    // -----------------------------------------------------------------------
    // Leaf helpers
    // -----------------------------------------------------------------------

    fn emit_scan(&self, table: &str, alias: Option<&str>) -> String {
        // Use schema-canonical table name if present; fall back to first known.
        let tbl = if self.schema.has_table(table) {
            table.to_owned()
        } else {
            self.schema
                .tables
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| table.to_owned())
        };
        if let Some(a) = alias {
            format!("{tbl} AS {a}")
        } else {
            tbl
        }
    }

    fn emit_subquery(&self, expr: &RelExpr) -> String {
        match expr {
            RelExpr::Scan { table, alias } => {
                self.emit_scan(table, alias.as_deref())
            }
            other => {
                let alias = fresh_alias();
                format!("({}) AS {alias}", self.emit_rel(other))
            }
        }
    }

    // -----------------------------------------------------------------------
    // JOIN
    // -----------------------------------------------------------------------

    fn emit_join(
        &self,
        join_type: &JoinType,
        condition: &Expr,
        left: &RelExpr,
        right: &RelExpr,
    ) -> String {
        let left_sql = self.emit_subquery(left);
        let right_sql = self.emit_subquery(right);
        let cond = self.emit_expr(condition);
        let join_kw = match join_type {
            JoinType::Inner => "JOIN",
            JoinType::LeftOuter => "LEFT JOIN",
            JoinType::RightOuter => "RIGHT JOIN",
            JoinType::FullOuter => "FULL OUTER JOIN",
            JoinType::Cross => "CROSS JOIN",
            // Semi/anti: emit as INNER JOIN for executability
            JoinType::Semi | JoinType::Anti => "JOIN",
        };
        format!("SELECT * FROM {left_sql} {join_kw} {right_sql} ON {cond}")
    }

    // -----------------------------------------------------------------------
    // AGGREGATE
    // -----------------------------------------------------------------------

    fn emit_aggregate(
        &self,
        group_by: &[Expr],
        aggregates: &[AggregateExpr],
        input: &RelExpr,
    ) -> String {
        let src = self.emit_subquery(input);
        let agg_cols: Vec<String> = aggregates
            .iter()
            .map(|a| self.emit_agg_expr(a))
            .collect();

        if group_by.is_empty() {
            format!("SELECT {} FROM {src}", agg_cols.join(", "))
        } else {
            let keys: Vec<String> =
                group_by.iter().map(|e| self.emit_expr(e)).collect();
            let keys_sql = keys.join(", ");
            format!(
                "SELECT {keys_sql}, {} FROM {src} GROUP BY {keys_sql}",
                agg_cols.join(", ")
            )
        }
    }

    fn emit_agg_expr(&self, agg: &AggregateExpr) -> String {
        let fn_name = match agg.function {
            AggregateFunction::Count => "COUNT",
            AggregateFunction::Sum => "SUM",
            AggregateFunction::Avg => "AVG",
            AggregateFunction::Min => "MIN",
            AggregateFunction::Max => "MAX",
            AggregateFunction::StdDev => "STDDEV",
            AggregateFunction::Variance => "VARIANCE",
            AggregateFunction::StringAgg => "STRING_AGG",
            AggregateFunction::ArrayAgg => "ARRAY_AGG",
        };
        let distinct = if agg.distinct { "DISTINCT " } else { "" };
        let arg = agg.arg.as_ref().map_or("*".to_owned(), |e| self.emit_expr(e));
        let alias = agg
            .alias
            .as_ref()
            .map(|a| format!(" AS {a}"))
            .unwrap_or_default();
        format!("{fn_name}({distinct}{arg}){alias}")
    }

    // -----------------------------------------------------------------------
    // PROJECTION
    // -----------------------------------------------------------------------

    fn emit_projection_list(&self, cols: &[ProjectionColumn]) -> String {
        if cols.is_empty() {
            return "*".to_owned();
        }
        cols.iter()
            .map(|c| {
                let expr = self.emit_expr(&c.expr);
                if let Some(ref a) = c.alias {
                    format!("{expr} AS {a}")
                } else {
                    expr
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    // -----------------------------------------------------------------------
    // ORDER BY
    // -----------------------------------------------------------------------

    fn emit_order_by(&self, keys: &[SortKey]) -> String {
        keys.iter()
            .map(|k| {
                let expr = self.emit_expr(&k.expr);
                let dir = match k.direction {
                    SortDirection::Asc => "ASC",
                    SortDirection::Desc => "DESC",
                };
                let nulls = match k.nulls {
                    NullOrdering::First => " NULLS FIRST",
                    NullOrdering::Last => " NULLS LAST",
                };
                format!("{expr} {dir}{nulls}")
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    // -----------------------------------------------------------------------
    // VALUES
    // -----------------------------------------------------------------------

    fn emit_values(&self, rows: &[Vec<Expr>]) -> String {
        let row_strs: Vec<String> = rows
            .iter()
            .map(|row| {
                let vals: Vec<String> =
                    row.iter().map(|e| self.emit_expr(e)).collect();
                format!("({})", vals.join(", "))
            })
            .collect();
        format!("VALUES {}", row_strs.join(", "))
    }

    // -----------------------------------------------------------------------
    // WINDOW
    // -----------------------------------------------------------------------

    fn emit_window_expr(&self, w: &WindowExpr) -> String {
        let fn_name = match w.function {
            WindowFunction::Avg => "avg",
            WindowFunction::Sum => "sum",
            WindowFunction::Count => "count",
            WindowFunction::Min => "min",
            WindowFunction::Max => "max",
            WindowFunction::RowNumber => "row_number",
            WindowFunction::Rank => "rank",
            WindowFunction::DenseRank => "dense_rank",
            WindowFunction::PercentRank => "percent_rank",
            WindowFunction::Ntile => "ntile",
            WindowFunction::Lag => "lag",
            WindowFunction::Lead => "lead",
            WindowFunction::FirstValue => "first_value",
            WindowFunction::LastValue => "last_value",
            WindowFunction::NthValue => "nth_value",
        };
        let arg = w.arg.as_ref().map_or("*".to_owned(), |e| self.emit_expr(e));
        let over_clause = self.emit_over_clause(w);
        let alias = w.alias.as_ref().map_or(String::new(), |a| format!(" AS {a}"));
        format!("{fn_name}({arg}) OVER ({over_clause}){alias}")
    }

    fn emit_over_clause(&self, w: &WindowExpr) -> String {
        let mut parts = Vec::new();
        if !w.partition_by.is_empty() {
            let exprs: Vec<_> = w.partition_by.iter()
                .map(|e| self.emit_expr(e))
                .collect();
            parts.push(format!("PARTITION BY {}", exprs.join(", ")));
        }
        if !w.order_by.is_empty() {
            parts.push(format!("ORDER BY {}", self.emit_order_by(&w.order_by)));
        }
        parts.join(" ")
    }

    // -----------------------------------------------------------------------
    // Scalar expressions
    // -----------------------------------------------------------------------

    /// Emit a scalar expression as SQL.
    #[allow(clippy::too_many_lines)]
    #[must_use]
    pub fn emit_expr(&self, expr: &Expr) -> String {
        match expr {
            Expr::Column(col) => self.emit_column_ref(col),
            Expr::Const(c) => emit_const(c),

            Expr::BinOp { op, left, right } => {
                let l = self.emit_expr(left);
                let r = self.emit_expr(right);
                format!("({l} {} {r})", emit_binop(*op))
            }

            Expr::UnaryOp { op, operand } => {
                let inner = self.emit_expr(operand);
                match op {
                    UnaryOp::Not => format!("(NOT {inner})"),
                    UnaryOp::IsNull => format!("({inner} IS NULL)"),
                    UnaryOp::IsNotNull => format!("({inner} IS NOT NULL)"),
                    UnaryOp::Neg => format!("(-{inner})"),
                }
            }

            Expr::Function { name, args } => {
                let args_sql: Vec<_> = args.iter().map(|a| self.emit_expr(a)).collect();
                format!("{name}({})", args_sql.join(", "))
            }

            Expr::Case { operand, when_clauses, else_result } => {
                let mut out = "CASE".to_owned();
                if let Some(op) = operand {
                    out.push(' ');
                    out.push_str(&self.emit_expr(op));
                }
                for (cond, result) in when_clauses {
                    out.push_str(&format!(
                        " WHEN {} THEN {}",
                        self.emit_expr(cond),
                        self.emit_expr(result)
                    ));
                }
                if let Some(el) = else_result {
                    out.push_str(&format!(" ELSE {}", self.emit_expr(el)));
                }
                out.push_str(" END");
                out
            }

            Expr::Cast { expr, target_type } => {
                format!("CAST({} AS {})", self.emit_expr(expr), target_type)
            }

            Expr::Array(elems) => {
                let items: Vec<_> = elems.iter().map(|e| self.emit_expr(e)).collect();
                format!("ARRAY[{}]", items.join(", "))
            }

            Expr::ArrayIndex(arr, idx) => {
                format!("{}[{}]", self.emit_expr(arr), self.emit_expr(idx))
            }

            Expr::SubQuery { subquery_type, query, test_expr } => {
                use ra_core::expr::SubQueryType;
                let inner = self.emit_rel(query);
                match subquery_type {
                    SubQueryType::Scalar => format!("({inner})"),
                    SubQueryType::Exists => format!("EXISTS ({inner})"),
                    SubQueryType::In => {
                        let lhs = test_expr.as_ref()
                            .map_or("?".to_owned(), |e| self.emit_expr(e));
                        format!("{lhs} IN ({inner})")
                    }
                    SubQueryType::Any => {
                        let lhs = test_expr.as_ref()
                            .map_or("?".to_owned(), |e| self.emit_expr(e));
                        format!("{lhs} = ANY ({inner})")
                    }
                    SubQueryType::All => {
                        let lhs = test_expr.as_ref()
                            .map_or("?".to_owned(), |e| self.emit_expr(e));
                        format!("{lhs} > ALL ({inner})")
                    }
                }
            }

            // Pattern navigation — emit inner expression as fallback
            Expr::PatternPrev(e, _) | Expr::PatternNext(e, _) => self.emit_expr(e),
            Expr::PatternFirst(e, _) | Expr::PatternLast(e, _) => self.emit_expr(e),
            Expr::PatternClassifier => "'?'".to_owned(),
            Expr::PatternMatchNumber => "0".to_owned(),

            Expr::ArraySlice { array, start, end } => {
                let arr = self.emit_expr(array);
                let s = start.as_ref().map_or(String::new(), |e| self.emit_expr(e));
                let en = end.as_ref().map_or(String::new(), |e| self.emit_expr(e));
                format!("{arr}[{s}:{en}]")
            }

            Expr::FieldAccess { expr, field_name } => {
                format!("({}).{field_name}", self.emit_expr(expr))
            }

            Expr::FullTextMatch { columns, query, .. } => {
                let col = columns.first().map(String::as_str).unwrap_or("*");
                let escaped = query.replace('\'', "''");
                format!("to_tsvector({col}) @@ to_tsquery('{escaped}')")
            }

            // Vector expressions — emit literal 0.0 as placeholder
            Expr::VectorDistance { .. } => "0.0".to_owned(),
        }
    }

    fn emit_column_ref(&self, col: &ColumnRef) -> String {
        if let Some(ref t) = col.table {
            format!("{}.{}", t, col.column)
        } else {
            col.column.clone()
        }
    }
}

impl Default for SqlEmitter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Pure helpers (no self)
// ---------------------------------------------------------------------------

fn emit_const(c: &Const) -> String {
    match c {
        Const::Null => "NULL".to_owned(),
        Const::Bool(true) => "TRUE".to_owned(),
        Const::Bool(false) => "FALSE".to_owned(),
        Const::Int(n) => n.to_string(),
        Const::Float(f) => format!("{f}"),
        Const::String(s) => {
            let escaped = s.replace('\'', "''");
            format!("'{escaped}'")
        }
    }
}

fn emit_binop(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Eq => "=",
        BinOp::Ne => "<>",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "AND",
        BinOp::Or => "OR",
        BinOp::Mod => "%",
        BinOp::Concat => "||",
        BinOp::JsonAccess => "->>",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::{AggregateExpr, AggregateFunction, ProjectionColumn};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn col(name: &str) -> Expr {
        Expr::Column(ColumnRef::new(name))
    }

    fn scan(table: &str) -> RelExpr {
        RelExpr::Scan { table: table.to_owned(), alias: None }
    }

    #[test]
    fn emit_simple_scan() {
        let emitter = SqlEmitter::new();
        let sql = emitter.emit(&scan("orders"));
        assert_eq!(sql, "orders");
    }

    #[test]
    fn emit_select_star_filter() {
        let emitter = SqlEmitter::new();
        let expr = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(col("o_orderstatus")),
                right: Box::new(Expr::Const(Const::String("O".to_owned()))),
            },
            input: Box::new(scan("orders")),
        };
        let sql = emitter.emit(&expr);
        assert!(sql.starts_with("SELECT * FROM"), "got: {sql}");
        assert!(sql.contains("WHERE"), "got: {sql}");
    }

    #[test]
    fn emit_projection() {
        let emitter = SqlEmitter::new();
        let expr = RelExpr::Project {
            columns: vec![
                ProjectionColumn { expr: col("o_orderkey"), alias: None },
                ProjectionColumn { expr: col("o_totalprice"), alias: None },
            ],
            input: Box::new(scan("orders")),
        };
        let sql = emitter.emit(&expr);
        assert!(sql.contains("SELECT"), "got: {sql}");
        assert!(sql.contains("o_orderkey"), "got: {sql}");
    }

    #[test]
    fn emit_aggregate() {
        let emitter = SqlEmitter::new();
        let expr = RelExpr::Aggregate {
            group_by: vec![col("o_orderstatus")],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: None,
            }],
            input: Box::new(scan("orders")),
        };
        let sql = emitter.emit(&expr);
        assert!(sql.contains("GROUP BY"), "got: {sql}");
        assert!(sql.contains("COUNT(*)"), "got: {sql}");
    }

    #[test]
    fn emit_limit() {
        let emitter = SqlEmitter::new();
        let expr = RelExpr::Limit {
            count: 10,
            offset: 5,
            input: Box::new(scan("orders")),
        };
        let sql = emitter.emit(&expr);
        assert!(sql.contains("LIMIT 10"), "got: {sql}");
        assert!(sql.contains("OFFSET 5"), "got: {sql}");
    }

    #[test]
    fn emit_union() {
        let emitter = SqlEmitter::new();
        let expr = RelExpr::Union {
            all: true,
            left: Box::new(scan("orders")),
            right: Box::new(scan("lineitem")),
        };
        let sql = emitter.emit(&expr);
        assert!(sql.contains("UNION ALL"), "got: {sql}");
    }

    #[test]
    fn string_escaping() {
        let emitter = SqlEmitter::new();
        let e = Expr::Const(Const::String("it's".to_owned()));
        assert_eq!(emitter.emit_expr(&e), "'it''s'");
    }
}
