//! Native-Rust builder layer for the Lime Rust parse path.
//!
//! GENERATED from the ffi::builders signatures. Each fn is a thin
//! wrapper over the corresponding `crate::ffi` extern-C builder, mapping the
//! native calling convention (`&mut RaParseState`, `&str`, `Value`) to the C
//! ABI (`*mut RaParseState`, NUL/len strings, `*mut RaNode`). Behavior is
//! identical to the C path because the underlying builder is the same.
#![allow(
    clippy::all, clippy::pedantic, clippy::nursery, clippy::restriction,
    clippy::not_unsafe_ptr_arg_deref, clippy::too_many_arguments,
    clippy::needless_pass_by_value, clippy::cast_possible_truncation,
    clippy::cast_sign_loss, clippy::must_use_candidate, missing_docs, unused
)]

use std::ffi::CString;
use std::os::raw::c_char;

use crate::ffi::{self, RaParseState};
use crate::rust_parser::Value;

#[must_use]
pub fn ra_scan(st: *mut RaParseState, table: &str) -> Value {
    let table_c = CString::new(table).unwrap_or_default();
    let handle = unsafe { ffi::ra_scan(st, table_c.as_ptr()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_scan_alias(st: *mut RaParseState, table: &str, alias: &str) -> Value {
    let table_c = CString::new(table).unwrap_or_default();
    let alias_c = CString::new(alias).unwrap_or_default();
    let handle = unsafe { ffi::ra_scan_alias(st, table_c.as_ptr(), alias_c.as_ptr()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_filter_agg(st: *mut RaParseState, func_name: &str, args_list: Value, filter_cond: Value) -> Value {
    let func_name_c = CString::new(func_name).unwrap_or_default();
    let handle = unsafe { ffi::ra_filter_agg(st, func_name_c.as_ptr(), args_list.handle(), filter_cond.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_filter(st: *mut RaParseState, input: Value, predicate: Value) -> Value {
    let handle = unsafe { ffi::ra_filter(st, input.handle(), predicate.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_project(st: *mut RaParseState, input: Value, columns: Value) -> Value {
    let handle = unsafe { ffi::ra_project(st, input.handle(), columns.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_alias_target(st: *mut RaParseState, expr_node: Value, alias: &str) -> Value {
    let alias_c = CString::new(alias).unwrap_or_default();
    let handle = unsafe { ffi::ra_alias_target(st, expr_node.handle(), alias_c.as_ptr()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_join(st: *mut RaParseState, join_type: i64, left: Value, right: Value, condition: Value) -> Value {
    let handle = unsafe { ffi::ra_join(st, join_type as u32, left.handle(), right.handle(), condition.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_aggregate(st: *mut RaParseState, input: Value, group_by: Value, aggs: Value) -> Value {
    let handle = unsafe { ffi::ra_aggregate(st, input.handle(), group_by.handle(), aggs.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_sort(st: *mut RaParseState, input: Value, keys: Value) -> Value {
    let handle = unsafe { ffi::ra_sort(st, input.handle(), keys.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_limit(st: *mut RaParseState, input: Value, count: u64, offset: u64) -> Value {
    let handle = unsafe { ffi::ra_limit(st, input.handle(), count, offset) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_union(st: *mut RaParseState, left: Value, right: Value, all: u32) -> Value {
    let handle = unsafe { ffi::ra_union(st, left.handle(), right.handle(), all) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_intersect(st: *mut RaParseState, left: Value, right: Value, all: u32) -> Value {
    let handle = unsafe { ffi::ra_intersect(st, left.handle(), right.handle(), all) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_except(st: *mut RaParseState, left: Value, right: Value, all: u32) -> Value {
    let handle = unsafe { ffi::ra_except(st, left.handle(), right.handle(), all) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_values(st: *mut RaParseState, rows_list: Value) -> Value {
    let handle = unsafe { ffi::ra_values(st, rows_list.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_recursive_cte(st: *mut RaParseState, name: &str, name_len: usize, base: Value, recursive: Value, body: Value) -> Value {
    let handle = unsafe { ffi::ra_recursive_cte(st, name.as_ptr().cast::<c_char>(), name_len, base.handle(), recursive.handle(), body.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_recursive_cte_auto(st: *mut RaParseState, name: &str, name_len: usize, cte_body: Value, query_body: Value) -> Value {
    let handle = unsafe { ffi::ra_recursive_cte_auto(st, name.as_ptr().cast::<c_char>(), name_len, cte_body.handle(), query_body.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_unnest(st: *mut RaParseState, array_expr: Value) -> Value {
    let handle = unsafe { ffi::ra_unnest(st, array_expr.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_unnest_ord(st: *mut RaParseState, array_expr: Value) -> Value {
    let handle = unsafe { ffi::ra_unnest_ord(st, array_expr.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_table_function(st: *mut RaParseState, name: &str, name_len: usize, args: Value) -> Value {
    let handle = unsafe { ffi::ra_table_function(st, name.as_ptr().cast::<c_char>(), name_len, args.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_window_marker(st: *mut RaParseState, name: &str, args: Value) -> Value {
    let name_c = CString::new(name).unwrap_or_default();
    let handle = unsafe { ffi::ra_window_marker(st, name_c.as_ptr(), args.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_window_marker_full(st: *mut RaParseState, name: &str, args: Value, partition_list: Value, order_list: Value, has_frame: i64) -> Value {
    let name_c = CString::new(name).unwrap_or_default();
    let handle = unsafe { ffi::ra_window_marker_full(st, name_c.as_ptr(), args.handle(), partition_list.handle(), order_list.handle(), has_frame as i32) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_cte(st: *mut RaParseState, name: &str, definition: Value, body: Value) -> Value {
    let name_c = CString::new(name).unwrap_or_default();
    let handle = unsafe { ffi::ra_cte(st, name_c.as_ptr(), definition.handle(), body.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_window(st: *mut RaParseState, input: Value, funcs: Value) -> Value {
    let handle = unsafe { ffi::ra_window(st, input.handle(), funcs.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_distinct(st: *mut RaParseState, input: Value) -> Value {
    let handle = unsafe { ffi::ra_distinct(st, input.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_insert(st: *mut RaParseState, table: &str, columns: Value, source: Value, on_conflict: Value, returning: Value) -> Value {
    let table_c = CString::new(table).unwrap_or_default();
    let handle = unsafe { ffi::ra_insert(st, table_c.as_ptr(), columns.handle(), source.handle(), on_conflict.handle(), returning.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_update(st: *mut RaParseState, table: &str, assignments: Value, filter: Value, from: Value, returning: Value) -> Value {
    let table_c = CString::new(table).unwrap_or_default();
    let handle = unsafe { ffi::ra_update(st, table_c.as_ptr(), assignments.handle(), filter.handle(), from.handle(), returning.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_delete(st: *mut RaParseState, table: &str, filter: Value, using_clause: Value, returning: Value) -> Value {
    let table_c = CString::new(table).unwrap_or_default();
    let handle = unsafe { ffi::ra_delete(st, table_c.as_ptr(), filter.handle(), using_clause.handle(), returning.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_merge(st: *mut RaParseState, target: &str, source: Value, on: Value, when_clauses: Value, returning: Value) -> Value {
    let target_c = CString::new(target).unwrap_or_default();
    let handle = unsafe { ffi::ra_merge(st, target_c.as_ptr(), source.handle(), on.handle(), when_clauses.handle(), returning.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_merge_kind_by(ident: &str) -> i32 {
    let ident_c = CString::new(ident).unwrap_or_default();
    unsafe { ffi::ra_merge_kind_by(ident_c.as_ptr()) }
}

#[must_use]
pub fn ra_merge_when_update(st: *mut RaParseState, kind: i32, cond: Value, assignments: Value) -> Value {
    let handle = unsafe { ffi::ra_merge_when_update(st, kind, cond.handle(), assignments.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_merge_when_delete(st: *mut RaParseState, kind: i32, cond: Value) -> Value {
    let handle = unsafe { ffi::ra_merge_when_delete(st, kind, cond.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_merge_when_nothing(st: *mut RaParseState, kind: i32, cond: Value) -> Value {
    let handle = unsafe { ffi::ra_merge_when_nothing(st, kind, cond.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_merge_when_insert(st: *mut RaParseState, kind: i32, cond: Value, columns: Value, values: Value) -> Value {
    let handle = unsafe { ffi::ra_merge_when_insert(st, kind, cond.handle(), columns.handle(), values.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_graph_vertex(st: *mut RaParseState, variable: Option<&str>, label: Option<&str>) -> Value {
    let variable_c = variable.map(|s| CString::new(s).unwrap_or_default());
    let label_c = label.map(|s| CString::new(s).unwrap_or_default());
    let handle = unsafe { ffi::ra_graph_vertex(st, variable_c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()), label_c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr())) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_graph_edge(st: *mut RaParseState, variable: Option<&str>, label: Option<&str>, direction: i32) -> Value {
    let variable_c = variable.map(|s| CString::new(s).unwrap_or_default());
    let label_c = label.map(|s| CString::new(s).unwrap_or_default());
    let handle = unsafe { ffi::ra_graph_edge(st, variable_c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()), label_c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()), direction) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_graph_table(st: *mut RaParseState, graph: Option<&str>, pattern: Value, columns: Value, alias: Option<&str>) -> Value {
    let graph_c = graph.map(|s| CString::new(s).unwrap_or_default());
    let alias_c = alias.map(|s| CString::new(s).unwrap_or_default());
    let handle = unsafe { ffi::ra_graph_table(st, graph_c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()), pattern.handle(), columns.handle(), alias_c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr())) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_on_conflict_nothing(st: *mut RaParseState) -> Value {
    let handle = unsafe { ffi::ra_on_conflict_nothing(st) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_on_conflict_update(st: *mut RaParseState, target_cols: Value, assignments: Value) -> Value {
    let handle = unsafe { ffi::ra_on_conflict_update(st, target_cols.handle(), assignments.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_on_conflict_select(st: *mut RaParseState, target_cols: Value) -> Value {
    let handle = unsafe { ffi::ra_on_conflict_select(st, target_cols.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_assignment(st: *mut RaParseState, column: &str, value: Value) -> Value {
    let column_c = CString::new(column).unwrap_or_default();
    let handle = unsafe { ffi::ra_assignment(st, column_c.as_ptr(), value.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_default_values(st: *mut RaParseState) -> Value {
    let handle = unsafe { ffi::ra_default_values(st) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_column(st: *mut RaParseState, name: &str) -> Value {
    let name_c = CString::new(name).unwrap_or_default();
    let handle = unsafe { ffi::ra_column(st, name_c.as_ptr()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_qualified_column(st: *mut RaParseState, table: &str, column: &str) -> Value {
    let table_c = CString::new(table).unwrap_or_default();
    let column_c = CString::new(column).unwrap_or_default();
    let handle = unsafe { ffi::ra_qualified_column(st, table_c.as_ptr(), column_c.as_ptr()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_const_int(st: *mut RaParseState, value: i64) -> Value {
    let handle = unsafe { ffi::ra_const_int(st, value) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_const_float(st: *mut RaParseState, value: f64) -> Value {
    let handle = unsafe { ffi::ra_const_float(st, value) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_const_str(st: *mut RaParseState, value: &str) -> Value {
    let value_c = CString::new(value).unwrap_or_default();
    let handle = unsafe { ffi::ra_const_str(st, value_c.as_ptr()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_const_null(st: *mut RaParseState) -> Value {
    let handle = unsafe { ffi::ra_const_null(st) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_const_bool(st: *mut RaParseState, value: u32) -> Value {
    let handle = unsafe { ffi::ra_const_bool(st, value) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_binop(st: *mut RaParseState, op: u32, left: Value, right: Value) -> Value {
    let handle = unsafe { ffi::ra_binop(st, op, left.handle(), right.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_unary_op(st: *mut RaParseState, op_code: u32, operand: Value) -> Value {
    let handle = unsafe { ffi::ra_unary_op(st, op_code, operand.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_case(st: *mut RaParseState, operand: Value, when_list: Value, else_expr: Value) -> Value {
    let handle = unsafe { ffi::ra_case(st, operand.handle(), when_list.handle(), else_expr.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_cast(st: *mut RaParseState, expr: Value, type_str: &str, type_len: usize) -> Value {
    let handle = unsafe { ffi::ra_cast(st, expr.handle(), type_str.as_ptr().cast::<c_char>(), type_len) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_subquery(st: *mut RaParseState, type_code: u32, rel_node: Value, test_expr: Value) -> Value {
    let handle = unsafe { ffi::ra_subquery(st, type_code, rel_node.handle(), test_expr.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_func(st: *mut RaParseState, name: &str, args: Value) -> Value {
    let name_c = CString::new(name).unwrap_or_default();
    let handle = unsafe { ffi::ra_func(st, name_c.as_ptr(), args.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_array(st: *mut RaParseState, elem_list: Value) -> Value {
    let handle = unsafe { ffi::ra_array(st, elem_list.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_array_index(st: *mut RaParseState, array_expr: Value, index_expr: Value) -> Value {
    let handle = unsafe { ffi::ra_array_index(st, array_expr.handle(), index_expr.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_field_access(st: *mut RaParseState, expr: Value, field_name: &str, field_len: usize) -> Value {
    let handle = unsafe { ffi::ra_field_access(st, expr.handle(), field_name.as_ptr().cast::<c_char>(), field_len) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_agg_expr(st: *mut RaParseState, func_code: u32, arg: Value, distinct: u32, alias: &str, alias_len: usize) -> Value {
    let handle = unsafe { ffi::ra_agg_expr(st, func_code, arg.handle(), distinct, alias.as_ptr().cast::<c_char>(), alias_len) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_window_expr(st: *mut RaParseState, func_code: u32, arg: Value, partition_list: Value, order_list: Value, alias: &str, alias_len: usize) -> Value {
    let handle = unsafe { ffi::ra_window_expr(st, func_code, arg.handle(), partition_list.handle(), order_list.handle(), alias.as_ptr().cast::<c_char>(), alias_len) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_list_new(st: *mut RaParseState) -> Value {
    let handle = unsafe { ffi::ra_list_new(st) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_list_push(st: *mut RaParseState, list: Value, item: Value) -> Value {
    let handle = unsafe { ffi::ra_list_push(st, list.handle(), item.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_list_prepend(st: *mut RaParseState, list: Value, item: Value) -> Value {
    let handle = unsafe { ffi::ra_list_prepend(st, list.handle(), item.handle()) };
    Value::from_node(handle)
}

#[must_use]
pub fn ra_sort_key(st: *mut RaParseState, expr: Value, ascending: i64, nulls_first: i64) -> Value {
    let handle = unsafe { ffi::ra_sort_key(st, expr.handle(), ascending as u32, nulls_first as u32) };
    Value::from_node(handle)
}
