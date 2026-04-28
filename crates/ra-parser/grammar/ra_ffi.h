/*
 * ra_ffi.h -- C declarations for the Rust FFI builder functions.
 *
 * The Lime-generated parser calls these functions in its reduction
 * actions to construct RelExpr / Expr AST nodes.  The actual
 * implementations live in crates/ra-parser/src/ffi/builders.rs and
 * are linked as #[no_mangle] extern "C" symbols.
 */
#ifndef RA_FFI_H
#define RA_FFI_H

#include <stdint.h>

/* Opaque handles -- never dereferenced by C code. */
typedef struct RaParseState RaParseState;
typedef struct RaNode       RaNode;

/* ------------------------------------------------------------------
 * Relational builders
 * ------------------------------------------------------------------ */

RaNode *ra_scan(RaParseState *st, const char *table);
RaNode *ra_filter(RaParseState *st, RaNode *input, RaNode *predicate);
RaNode *ra_project(RaParseState *st, RaNode *input, RaNode *columns);
RaNode *ra_join(RaParseState *st, uint32_t join_type,
                RaNode *left, RaNode *right, RaNode *condition);
RaNode *ra_aggregate(RaParseState *st, RaNode *input,
                     RaNode *group_by, RaNode *aggs);
RaNode *ra_sort(RaParseState *st, RaNode *input, RaNode *keys);
RaNode *ra_limit(RaParseState *st, RaNode *input,
                 uint64_t count, uint64_t offset);
RaNode *ra_union(RaParseState *st, RaNode *left, RaNode *right,
                 uint32_t all);
RaNode *ra_cte(RaParseState *st, const char *name,
               RaNode *definition, RaNode *body);
RaNode *ra_window(RaParseState *st, RaNode *input, RaNode *funcs);
RaNode *ra_distinct(RaParseState *st, RaNode *input);

/* ------------------------------------------------------------------
 * Expression builders
 * ------------------------------------------------------------------ */

RaNode *ra_column(RaParseState *st, const char *name);
RaNode *ra_qualified_column(RaParseState *st,
                            const char *table, const char *column);
RaNode *ra_const_int(RaParseState *st, int64_t value);
RaNode *ra_const_float(RaParseState *st, double value);
RaNode *ra_const_str(RaParseState *st, const char *value);
RaNode *ra_const_null(RaParseState *st);
RaNode *ra_binop(RaParseState *st, uint32_t op,
                 RaNode *left, RaNode *right);
RaNode *ra_func(RaParseState *st, const char *name, RaNode *args);

/* ------------------------------------------------------------------
 * List and sort-key builders
 * ------------------------------------------------------------------ */

RaNode *ra_list_new(RaParseState *st);
RaNode *ra_list_push(RaParseState *st, RaNode *list, RaNode *item);
RaNode *ra_sort_key(RaParseState *st, RaNode *expr,
                    uint32_t ascending, uint32_t nulls_first);

#endif /* RA_FFI_H */
