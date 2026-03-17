//! Query generation from parsed intent to relational algebra.
//!
//! Converts [`QueryIntent`] into [`ra_core::RelExpr`] trees by
//! resolving column references against the schema and building
//! the appropriate relational algebra operators.

use ra_core::{
    AggregateExpr, AggregateFunction, BinOp, ColumnRef, Const, Expr,
    JoinType, NullOrdering, ProjectionColumn, RelExpr, SortDirection,
    SortKey,
};

use crate::error::SynthesisError;
use crate::intent::{
    AggregateIntent, FilterIntent, FilterOp, OrderIntent, QueryIntent,
};
use crate::schema::SchemaInfo;

/// Generates [`RelExpr`] trees from [`QueryIntent`].
pub struct QueryGenerator<'a> {
    schema: &'a SchemaInfo,
}

impl<'a> QueryGenerator<'a> {
    /// Create a new generator for the given schema.
    #[must_use]
    pub fn new(schema: &'a SchemaInfo) -> Self {
        Self { schema }
    }

    /// Generate a relational expression from parsed intent.
    ///
    /// # Errors
    ///
    /// Returns `SynthesisError` if tables or columns referenced
    /// in the intent cannot be resolved against the schema.
    pub fn generate(
        &self,
        intent: &QueryIntent,
    ) -> Result<RelExpr, SynthesisError> {
        let base = self.build_base_scan(intent)?;
        let filtered = apply_filters(base, &intent.filters);
        let aggregated = apply_aggregates(
            filtered,
            &intent.aggregates,
            &intent.group_by,
        );
        let projected = apply_projection(aggregated, intent);
        let sorted = apply_order_by(projected, &intent.order_by);
        let limited = apply_limit(sorted, intent.limit);
        Ok(limited)
    }

    fn build_base_scan(
        &self,
        intent: &QueryIntent,
    ) -> Result<RelExpr, SynthesisError> {
        let first_table = intent
            .tables
            .first()
            .ok_or(SynthesisError::NoTablesIdentified)?;

        self.schema
            .find_table(first_table)
            .ok_or_else(|| {
                SynthesisError::UnknownTable(first_table.clone())
            })?;

        let mut expr = RelExpr::scan(first_table.clone());

        for join in &intent.joins {
            let right_table = if join.right_table == *first_table {
                &join.left_table
            } else {
                &join.right_table
            };

            let condition = self.build_join_condition(
                &join.left_table,
                right_table,
            );

            expr = RelExpr::Join {
                join_type: JoinType::Inner,
                condition,
                left: Box::new(expr),
                right: Box::new(RelExpr::scan(right_table.clone())),
            };
        }

        Ok(expr)
    }

    fn build_join_condition(&self, left: &str, right: &str) -> Expr {
        if let Some(expr) = self.find_fk_condition(left, right) {
            return expr;
        }
        if let Some(expr) = self.find_fk_condition(right, left) {
            return expr;
        }
        Expr::Const(Const::Bool(true))
    }

    fn find_fk_condition(
        &self,
        from: &str,
        to: &str,
    ) -> Option<Expr> {
        let table = self.schema.find_table(from)?;
        for fk in &table.foreign_keys {
            if fk.referenced_table.eq_ignore_ascii_case(to) {
                if let (Some(lcol), Some(rcol)) =
                    (fk.columns.first(), fk.referenced_columns.first())
                {
                    return Some(Expr::BinOp {
                        op: BinOp::Eq,
                        left: Box::new(Expr::Column(
                            ColumnRef::qualified(from, lcol.as_str()),
                        )),
                        right: Box::new(Expr::Column(
                            ColumnRef::qualified(to, rcol.as_str()),
                        )),
                    });
                }
            }
        }
        None
    }
}

fn apply_filters(input: RelExpr, filters: &[FilterIntent]) -> RelExpr {
    let mut expr = input;
    for filter in filters {
        let predicate = build_filter_expr(filter);
        expr = expr.filter(predicate);
    }
    expr
}

fn apply_aggregates(
    input: RelExpr,
    aggregates: &[AggregateIntent],
    group_by_cols: &[String],
) -> RelExpr {
    if aggregates.is_empty() {
        return input;
    }

    let agg_exprs: Vec<AggregateExpr> = aggregates
        .iter()
        .map(build_aggregate_expr)
        .collect();

    let group_by: Vec<Expr> = group_by_cols
        .iter()
        .map(|col| Expr::Column(ColumnRef::new(col.as_str())))
        .collect();

    RelExpr::Aggregate {
        group_by,
        aggregates: agg_exprs,
        input: Box::new(input),
    }
}

fn apply_projection(input: RelExpr, intent: &QueryIntent) -> RelExpr {
    if intent.select_columns.is_empty()
        || !intent.aggregates.is_empty()
    {
        return input;
    }

    let columns: Vec<ProjectionColumn> = intent
        .select_columns
        .iter()
        .map(|ci| {
            let col_ref = match &ci.table {
                Some(t) => ColumnRef::qualified(
                    t.as_str(),
                    ci.column.as_str(),
                ),
                None => ColumnRef::new(ci.column.as_str()),
            };
            ProjectionColumn {
                expr: Expr::Column(col_ref),
                alias: None,
            }
        })
        .collect();

    if columns.is_empty() {
        return input;
    }

    input.project(columns)
}

fn apply_order_by(
    input: RelExpr,
    orders: &[OrderIntent],
) -> RelExpr {
    if orders.is_empty() {
        return input;
    }

    let keys: Vec<SortKey> = orders
        .iter()
        .map(|o| SortKey {
            expr: Expr::Column(ColumnRef::new(o.column.as_str())),
            direction: if o.descending {
                SortDirection::Desc
            } else {
                SortDirection::Asc
            },
            nulls: NullOrdering::Last,
        })
        .collect();

    RelExpr::Sort {
        keys,
        input: Box::new(input),
    }
}

fn build_filter_expr(filter: &FilterIntent) -> Expr {
    let col = Expr::Column(ColumnRef::new(filter.column.as_str()));
    let value = parse_literal(&filter.value);

    match filter.op {
        FilterOp::Like => Expr::Function {
            name: "LIKE".to_string(),
            args: vec![col, value],
        },
        _ => Expr::BinOp {
            op: filter_op_to_binop(filter.op),
            left: Box::new(col),
            right: Box::new(value),
        },
    }
}

fn filter_op_to_binop(op: FilterOp) -> BinOp {
    match op {
        FilterOp::Eq | FilterOp::Like => BinOp::Eq,
        FilterOp::Ne => BinOp::Ne,
        FilterOp::Gt => BinOp::Gt,
        FilterOp::Ge => BinOp::Ge,
        FilterOp::Lt => BinOp::Lt,
        FilterOp::Le => BinOp::Le,
    }
}

fn parse_literal(value: &str) -> Expr {
    let trimmed = value.trim().trim_matches('"').trim_matches('\'');

    if let Ok(i) = trimmed.parse::<i64>() {
        return Expr::Const(Const::Int(i));
    }
    if let Ok(f) = trimmed.parse::<f64>() {
        return Expr::Const(Const::Float(f));
    }
    if trimmed.eq_ignore_ascii_case("true") {
        return Expr::Const(Const::Bool(true));
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Expr::Const(Const::Bool(false));
    }
    if trimmed.eq_ignore_ascii_case("null") {
        return Expr::Const(Const::Null);
    }
    Expr::Const(Const::String(trimmed.to_string()))
}

fn build_aggregate_expr(agg: &AggregateIntent) -> AggregateExpr {
    let function = match agg.function.as_str() {
        "sum" => AggregateFunction::Sum,
        "avg" => AggregateFunction::Avg,
        "min" => AggregateFunction::Min,
        "max" => AggregateFunction::Max,
        _ => AggregateFunction::Count,
    };

    let arg = agg
        .column
        .as_ref()
        .map(|c| Expr::Column(ColumnRef::new(c.as_str())));

    AggregateExpr {
        function,
        arg,
        distinct: false,
        alias: Some(format!(
            "{}_{}",
            agg.function,
            agg.column.as_deref().unwrap_or("all")
        )),
    }
}

fn apply_limit(input: RelExpr, limit: Option<u64>) -> RelExpr {
    match limit {
        Some(count) => input.limit(count, 0),
        None => input,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::intent::{ColumnIntent, IntentParser};
    use crate::schema::{ColumnInfo, ForeignKey, TableInfo};

    fn test_schema() -> SchemaInfo {
        let mut schema = SchemaInfo::new();
        schema.add_table(TableInfo::new(
            "users",
            vec![
                ColumnInfo::new("id", "INTEGER").primary_key(),
                ColumnInfo::new("name", "TEXT").not_null(),
                ColumnInfo::new("age", "INTEGER"),
            ],
        ));
        let mut orders = TableInfo::new(
            "orders",
            vec![
                ColumnInfo::new("id", "INTEGER").primary_key(),
                ColumnInfo::new("user_id", "INTEGER").not_null(),
                ColumnInfo::new("amount", "REAL").not_null(),
            ],
        );
        orders.add_foreign_key(ForeignKey {
            columns: vec!["user_id".into()],
            referenced_table: "users".into(),
            referenced_columns: vec!["id".into()],
        });
        schema.add_table(orders);
        schema
    }

    #[test]
    fn generate_simple_scan() {
        let schema = test_schema();
        let parser = IntentParser::new(&schema);
        let gen = QueryGenerator::new(&schema);
        let intent = parser.parse("show all users").expect("test");
        let expr = gen.generate(&intent).expect("test");
        assert!(matches!(expr, RelExpr::Scan { .. }));
    }

    #[test]
    fn generate_with_filter() {
        let schema = test_schema();
        let parser = IntentParser::new(&schema);
        let gen = QueryGenerator::new(&schema);
        let intent = parser
            .parse("users with age greater than 25")
            .expect("test");
        let expr = gen.generate(&intent).expect("test");
        assert!(
            find_filter(&expr),
            "expected a filter in the plan"
        );
    }

    fn find_filter(expr: &RelExpr) -> bool {
        match expr {
            RelExpr::Filter { .. } => true,
            _ => expr.children().iter().any(|c| find_filter(c)),
        }
    }

    #[test]
    fn generate_with_limit() {
        let schema = test_schema();
        let intent = QueryIntent {
            tables: vec!["users".into()],
            select_columns: vec![],
            filters: vec![],
            aggregates: vec![],
            group_by: vec![],
            order_by: vec![],
            limit: Some(10),
            joins: vec![],
        };
        let gen = QueryGenerator::new(&schema);
        let expr = gen.generate(&intent).expect("test");
        assert!(matches!(expr, RelExpr::Limit { count: 10, .. }));
    }

    #[test]
    fn generate_with_projection() {
        let schema = test_schema();
        let intent = QueryIntent {
            tables: vec!["users".into()],
            select_columns: vec![
                ColumnIntent {
                    column: "name".into(),
                    table: Some("users".into()),
                },
                ColumnIntent {
                    column: "age".into(),
                    table: Some("users".into()),
                },
            ],
            filters: vec![],
            aggregates: vec![],
            group_by: vec![],
            order_by: vec![],
            limit: None,
            joins: vec![],
        };
        let gen = QueryGenerator::new(&schema);
        let expr = gen.generate(&intent).expect("test");
        assert!(matches!(expr, RelExpr::Project { .. }));
    }

    #[test]
    fn generate_with_aggregate() {
        let schema = test_schema();
        let intent = QueryIntent {
            tables: vec!["orders".into()],
            select_columns: vec![],
            filters: vec![],
            aggregates: vec![AggregateIntent {
                function: "sum".into(),
                column: Some("amount".into()),
            }],
            group_by: vec![],
            order_by: vec![],
            limit: None,
            joins: vec![],
        };
        let gen = QueryGenerator::new(&schema);
        let expr = gen.generate(&intent).expect("test");
        assert!(matches!(expr, RelExpr::Aggregate { .. }));
    }

    #[test]
    fn generate_with_join() {
        let schema = test_schema();
        let parser = IntentParser::new(&schema);
        let gen = QueryGenerator::new(&schema);
        let intent = parser
            .parse("show users and their orders")
            .expect("test");
        let expr = gen.generate(&intent).expect("test");
        assert!(find_join(&expr), "expected a join in the plan");
    }

    fn find_join(expr: &RelExpr) -> bool {
        match expr {
            RelExpr::Join { .. } => true,
            _ => expr.children().iter().any(|c| find_join(c)),
        }
    }

    #[test]
    fn parse_literal_values() {
        assert_eq!(
            parse_literal("42"),
            Expr::Const(Const::Int(42))
        );
        assert_eq!(
            parse_literal("9.99"),
            Expr::Const(Const::Float(9.99_f64))
        );
        assert_eq!(
            parse_literal("true"),
            Expr::Const(Const::Bool(true))
        );
        assert_eq!(
            parse_literal("hello"),
            Expr::Const(Const::String("hello".into()))
        );
    }

    #[test]
    fn unknown_table_error() {
        let schema = test_schema();
        let intent = QueryIntent {
            tables: vec!["nonexistent".into()],
            select_columns: vec![],
            filters: vec![],
            aggregates: vec![],
            group_by: vec![],
            order_by: vec![],
            limit: None,
            joins: vec![],
        };
        let gen = QueryGenerator::new(&schema);
        let result = gen.generate(&intent);
        assert!(result.is_err());
    }
}
