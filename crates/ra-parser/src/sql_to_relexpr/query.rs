use ra_core::algebra::RelExpr;
use sqlparser::ast::{Query, SetExpr, SetOperator, SetQuantifier};

use super::error::SqlConversionError;
use super::expr::convert_expr;
use super::helpers::extract_u64_from_expr;
use super::select::{convert_order_by_exprs, convert_select, try_convert_topk};

pub fn convert_query(query: &Query) -> Result<RelExpr, SqlConversionError> {
    let mut plan = convert_query_body(query)?;

    if let Some(with) = &query.with {
        for cte in with.cte_tables.iter().rev() {
            let cte_name = cte.alias.name.value.clone();
            let cte_def = convert_query(&cte.query)?;

            if with.recursive && cte_def_is_recursive(&cte_def, &cte_name) {
                plan = convert_recursive_cte(&cte_name, &cte_def, plan)?;
            } else {
                plan = RelExpr::CTE {
                    name: cte_name,
                    definition: Box::new(cte_def),
                    body: Box::new(plan),
                };
            }
        }
    }

    if let Some(order_by) = &query.order_by {
        if !order_by.exprs.is_empty() {
            if let Some(topk_plan) = try_convert_topk(&order_by.exprs, query.limit.as_ref(), &plan)?
            {
                plan = topk_plan;
            } else {
                let keys = convert_order_by_exprs(&order_by.exprs)?;
                plan = RelExpr::Sort {
                    keys,
                    input: Box::new(plan),
                };

                if query.limit.is_some() || query.offset.is_some() {
                    let count = match &query.limit {
                        Some(expr) => extract_u64_from_expr(expr)?,
                        None => u64::MAX,
                    };
                    let offset = match &query.offset {
                        Some(off) => extract_u64_from_expr(&off.value)?,
                        None => 0,
                    };
                    plan = RelExpr::Limit {
                        count,
                        offset,
                        input: Box::new(plan),
                    };
                }
            }
        } else {
            plan = apply_limit_offset(plan, query)?;
        }
    } else {
        plan = apply_limit_offset(plan, query)?;
    }

    Ok(plan)
}

fn apply_limit_offset(plan: RelExpr, query: &Query) -> Result<RelExpr, SqlConversionError> {
    if query.limit.is_some() || query.offset.is_some() {
        let count = match &query.limit {
            Some(expr) => extract_u64_from_expr(expr)?,
            None => u64::MAX,
        };
        let offset = match &query.offset {
            Some(off) => extract_u64_from_expr(&off.value)?,
            None => 0,
        };
        Ok(RelExpr::Limit {
            count,
            offset,
            input: Box::new(plan),
        })
    } else {
        Ok(plan)
    }
}

fn cte_def_is_recursive(def: &RelExpr, name: &str) -> bool {
    def.references_cte(name)
}

fn convert_recursive_cte(
    name: &str,
    definition: &RelExpr,
    body: RelExpr,
) -> Result<RelExpr, SqlConversionError> {
    let RelExpr::Union {
        all: true,
        left,
        right,
    } = definition
    else {
        return Err(SqlConversionError::InvalidRecursiveCTE(
            "recursive CTE must use UNION ALL".to_owned(),
        ));
    };

    let (base_case, recursive_case) = if right.references_cte(name) && !left.references_cte(name) {
        (left.as_ref().clone(), right.as_ref().clone())
    } else if left.references_cte(name) && !right.references_cte(name) {
        (right.as_ref().clone(), left.as_ref().clone())
    } else if left.references_cte(name) && right.references_cte(name) {
        return Err(SqlConversionError::InvalidRecursiveCTE(
            "both sides of UNION ALL reference the CTE".to_owned(),
        ));
    } else {
        return Err(SqlConversionError::InvalidRecursiveCTE(
            "neither side of UNION ALL references the CTE".to_owned(),
        ));
    };

    Ok(RelExpr::RecursiveCTE {
        name: name.to_owned(),
        base_case: Box::new(base_case),
        recursive_case: Box::new(recursive_case),
        body: Box::new(body),
        cycle_detection: Some(ra_core::algebra::CycleDetection {
            track_columns: vec![],
            max_depth: Some(1000),
            cycle_mark_column: None,
            path_column: None,
        }),
    })
}

fn convert_query_body(query: &Query) -> Result<RelExpr, SqlConversionError> {
    match &*query.body {
        SetExpr::Select(select) => convert_select(select),
        SetExpr::SetOperation {
            op,
            set_quantifier,
            left,
            right,
        } => convert_set_operation(op, set_quantifier, left, right),
        SetExpr::Query(inner) => convert_query(inner),
        SetExpr::Values(values) => {
            let mut rows = Vec::new();
            for row in &values.rows {
                let mut exprs = Vec::new();
                for val in row {
                    exprs.push(convert_expr(val)?);
                }
                rows.push(exprs);
            }
            Ok(RelExpr::Values { rows })
        }
        _ => Err(SqlConversionError::UnsupportedFeature(
            "unsupported query body type".to_owned(),
        )),
    }
}

fn convert_set_operation(
    op: &SetOperator,
    quantifier: &SetQuantifier,
    left: &SetExpr,
    right: &SetExpr,
) -> Result<RelExpr, SqlConversionError> {
    let left_plan = convert_set_expr(left)?;
    let right_plan = convert_set_expr(right)?;
    let all = matches!(quantifier, SetQuantifier::All | SetQuantifier::AllByName);

    match op {
        SetOperator::Union => Ok(RelExpr::Union {
            all,
            left: Box::new(left_plan),
            right: Box::new(right_plan),
        }),
        SetOperator::Intersect => Ok(RelExpr::Intersect {
            all,
            left: Box::new(left_plan),
            right: Box::new(right_plan),
        }),
        SetOperator::Except => Ok(RelExpr::Except {
            all,
            left: Box::new(left_plan),
            right: Box::new(right_plan),
        }),
    }
}

fn convert_set_expr(set_expr: &SetExpr) -> Result<RelExpr, SqlConversionError> {
    match set_expr {
        SetExpr::Select(select) => convert_select(select),
        SetExpr::SetOperation {
            op,
            set_quantifier,
            left,
            right,
        } => convert_set_operation(op, set_quantifier, left, right),
        SetExpr::Query(inner) => convert_query(inner),
        _ => Err(SqlConversionError::UnsupportedFeature(
            "unsupported set expression type".to_owned(),
        )),
    }
}
