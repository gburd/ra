/*
 * ra_planner_hook.h
 *
 * Compile-time integration header for the Ra query optimizer.
 *
 * When PostgreSQL is compiled with -DUSE_RA=1 (see
 * scripts/build-postgres-with-ra.sh), the standard_planner() function in
 * src/backend/optimizer/plan/planner.c calls ra_try_optimize() at the top
 * of its body.  The ra_planner extension (loaded via shared_preload_libraries)
 * provides the implementation through a function pointer registered at
 * library init time.
 *
 * The compile-time approach is stronger than pgrx's runtime planner_hook:
 *  - It is guaranteed to be called before any other planner hook can run.
 *  - It allows Ra to bypass cost manipulation entirely and return a fully
 *    formed PlannedStmt when it has high confidence in its plan.
 *  - The fallback to the standard planner is a simple NULL return, with
 *    no overhead on the standard path.
 *
 * USAGE IN planner.c:
 *
 *   #ifdef USE_RA
 *   #include "ra_planner_hook.h"
 *   #endif
 *
 *   PlannedStmt *
 *   standard_planner(Query *parse, const char *query_string,
 *                    int cursorOptions, ParamListInfo boundParams)
 *   {
 *   #ifdef USE_RA
 *       PlannedStmt *ra_result =
 *           ra_try_optimize(parse, query_string, cursorOptions, boundParams);
 *       if (ra_result != NULL)
 *           return ra_result;
 *   #endif
 *       // ... standard planning continues
 *   }
 *
 * IMPLEMENTATION (in the extension, registered in _PG_init):
 *
 *   ra_planner_hook_fn = my_ra_planner_impl;
 *
 * The function pointer is a global variable so the extension can hot-reload
 * the implementation without restarting PostgreSQL.
 */

#ifndef RA_PLANNER_HOOK_H
#define RA_PLANNER_HOOK_H

#include "nodes/plannodes.h"
#include "nodes/parsenodes.h"
#include "utils/palloc.h"

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Function pointer type for the Ra optimizer hook.
 *
 * Returns a fully planned PlannedStmt when Ra can optimize the query,
 * or NULL to fall through to the standard PostgreSQL planner.
 *
 * The returned PlannedStmt must be allocated in the current memory context
 * (CurrentMemoryContext) so it survives the planner call frame.
 */
typedef PlannedStmt *(*ra_planner_hook_fn_t)(
    Query          *parse,
    const char     *query_string,
    int             cursor_options,
    ParamListInfo   bound_params
);

/*
 * The global hook function pointer.
 *
 * Set to NULL initially.  The ra_planner extension sets this in its
 * _PG_init() function and clears it in _PG_fini().
 *
 * Access from planner.c:
 *
 *   PlannedStmt *
 *   ra_try_optimize(Query *parse, const char *query_string,
 *                   int cursor_options, ParamListInfo bound_params)
 *   {
 *       if (ra_planner_hook_fn == NULL)
 *           return NULL;
 *       return (*ra_planner_hook_fn)(parse, query_string,
 *                                   cursor_options, bound_params);
 *   }
 */
extern PGDLLIMPORT ra_planner_hook_fn_t ra_planner_hook_fn;

/*
 * Thin dispatcher called from standard_planner().
 *
 * Defined in src/backend/optimizer/plan/planner.c (with USE_RA):
 *
 *   PlannedStmt *
 *   ra_try_optimize(Query *parse, const char *query_string,
 *                   int cursor_options, ParamListInfo bound_params)
 *   {
 *       if (ra_planner_hook_fn == NULL)
 *           return NULL;
 *       return (*ra_planner_hook_fn)(parse, query_string,
 *                                   cursor_options, bound_params);
 *   }
 */
extern PlannedStmt *ra_try_optimize(
    Query          *parse,
    const char     *query_string,
    int             cursor_options,
    ParamListInfo   bound_params
);

#ifdef __cplusplus
}
#endif

#endif /* RA_PLANNER_HOOK_H */
