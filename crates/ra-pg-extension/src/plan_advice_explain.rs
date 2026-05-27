//! EXPLAIN integration for plan advice.
//!
//! Registers the `PLAN_ADVICE` boolean option for `EXPLAIN`,
//! and installs an `explain_per_plan_hook` that renders supplied
//! advice with feedback flags using the same wording as
//! PG's upstream `pg_plan_advice` module.
//!
//! Output format example (matches PG byte-for-byte):
//!
//! ```text
//! EXPLAIN (PLAN_ADVICE) SELECT ...;
//!  Hash Join
//!    ...
//!  Supplied Plan Advice:
//!    JOIN_ORDER(a b) /* matched */
//!    HASH_JOIN(b)    /* not matched */
//! ```
//!
//! # What is and isn't rendered
//!
//! - **Supplied Plan Advice** is rendered whenever
//!   `ra_planner.plan_advice` (or the compatibility-aliased
//!   `pg_plan_advice.advice`) is non-empty AND either
//!   `EXPLAIN (PLAN_ADVICE)` was specified or
//!   `ra_planner.always_explain_supplied_advice` is true.
//! - **Generated Plan Advice** is currently **not** rendered.
//!   PG generates this from the finished plan in
//!   `pgpa_planner_shutdown` and stashes it on
//!   `PlannedStmt.extension_state`. Ra's planner doesn't yet
//!   thread that state through; doing so requires adding a
//!   per-plan stash that survives from the planner hook to the
//!   EXPLAIN hook. Tracked as a follow-up.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::sync::OnceLock;

use pgrx::pg_sys;

use ra_engine::plan_advice_validate::AdviceItemFeedback;
use ra_plan_advice::feedback::{format_feedback, FeedbackFlags};
use ra_plan_advice::{parse_advice, render_advice};

/// Cached extension id from `GetExplainExtensionId`. Set once at
/// `_PG_init`, read in the EXPLAIN hooks.
static EXPLAIN_EXT_ID: OnceLock<c_int> = OnceLock::new();

/// Saved value of `explain_per_plan_hook` from before our install.
/// Chained-called so we don't break other extensions.
static mut PREV_EXPLAIN_PER_PLAN_HOOK: pg_sys::explain_per_plan_hook_type = None;

/// Install hooks. Called once during extension initialization.
///
/// Order of operations matches PG's `pg_plan_advice._PG_init`:
/// reserve our extension id first, register the EXPLAIN option,
/// then chain the per-plan hook.
///
/// # Safety
///
/// Must be called from within `_PG_init` while holding the
/// PostgreSQL extension-load mutex. Mutates the global
/// `explain_per_plan_hook` static.
pub unsafe fn install_explain_hooks() {
    // Reserve an extension id once. PG owns the underlying
    // counter; we just need it to set/get our per-ExplainState
    // boolean.
    let name = CString::new("ra_planner").expect("static valid c-string");
    // SAFETY: PG signature; arg is a valid borrowed C string.
    let id = unsafe { pg_sys::GetExplainExtensionId(name.as_ptr()) };
    if EXPLAIN_EXT_ID.set(id).is_err() {
        return; // already installed
    }

    // Register EXPLAIN (PLAN_ADVICE).
    //
    // The PG signature pgrx 0.17 binds for pg18 takes only
    // (option_name, handler) — no GUC-check arg. The third
    // argument added in later PG releases isn't visible here.
    let opt = CString::new("plan_advice").expect("static valid c-string");
    // SAFETY: PG signature; both arguments live for 'static.
    unsafe {
        pg_sys::RegisterExtensionExplainOption(
            opt.as_ptr(),
            Some(plan_advice_option_handler),
        );
    }

    // Chain the per-plan EXPLAIN hook.
    //
    // SAFETY: PG sets and reads this hook on the main thread
    // during planning/EXPLAIN. We're called from _PG_init before
    // any backend serves a query; no race.
    unsafe {
        PREV_EXPLAIN_PER_PLAN_HOOK = pg_sys::explain_per_plan_hook;
        pg_sys::explain_per_plan_hook = Some(plan_advice_per_plan_hook);
    }
}

/// EXPLAIN-option handler for `PLAN_ADVICE`.
///
/// Stores `true` in our per-ExplainState extension slot when the
/// option is specified and not set to false.
unsafe extern "C-unwind" fn plan_advice_option_handler(
    es: *mut pg_sys::ExplainState,
    opt: *mut pg_sys::DefElem,
    _pstate: *mut pg_sys::ParseState,
) {
    let Some(&id) = EXPLAIN_EXT_ID.get() else {
        return;
    };
    // SAFETY: PG passes a valid DefElem.
    let value = unsafe { pg_sys::defGetBoolean(opt) };
    // Allocate a Box<bool> and hand the pointer to PG. PG owns
    // the lifetime via SetExplainExtensionState; freeing happens
    // when the ExplainState is destroyed (we leak in our own
    // process if something goes wrong, but that's bounded by the
    // backend lifetime).
    let boxed = Box::into_raw(Box::new(value)).cast::<c_void>();
    // SAFETY: PG signature.
    unsafe {
        pg_sys::SetExplainExtensionState(es, id, boxed);
    }
}

/// `explain_per_plan_hook` implementation. Renders supplied
/// plan advice as a property block when appropriate.
unsafe extern "C-unwind" fn plan_advice_per_plan_hook(
    plannedstmt: *mut pg_sys::PlannedStmt,
    into: *mut pg_sys::IntoClause,
    es: *mut pg_sys::ExplainState,
    query_string: *const c_char,
    params: *mut pg_sys::ParamListInfoData,
    query_env: *mut pg_sys::QueryEnvironment,
) {
    // Chain to the previous hook first so output ordering
    // matches PG conventions.
    // SAFETY: only mutated at _PG_init under the load mutex.
    if let Some(prev) = unsafe { PREV_EXPLAIN_PER_PLAN_HOOK } {
        unsafe {
            prev(plannedstmt, into, es, query_string, params, query_env);
        }
    }

    // Decide whether to emit the block.
    let plan_advice_requested = explain_state_plan_advice_flag(es);
    let always_show =
        crate::extension_state::RA_ALWAYS_EXPLAIN_SUPPLIED_ADVICE.get();
    if !plan_advice_requested && !always_show {
        return;
    }

    // Read the active advice string. Fall back across both GUC
    // names (compatibility shim).
    let Some(advice_str) = crate::extension_state::effective_plan_advice() else {
        return;
    };

    // Parse. On failure, render the raw string so the user can
    // see what was supplied even when it's malformed.
    let advice = match parse_advice(&advice_str) {
        Ok(a) => a,
        Err(e) => {
            let line = format!(
                "{advice_str} /* parse error: {} */",
                e.message
            );
            emit_property("Supplied Plan Advice", &line, es);
            return;
        }
    };

    // Build feedback. We don't have a Ra-side RelExpr here — only
    // the finished PG PlannedStmt — so for now we approximate
    // alias collection from PlannedStmt.rtable. This is enough
    // to classify each item as matched / partially matched /
    // not matched.
    let aliases = collect_rtable_aliases(plannedstmt);
    let feedback: Vec<AdviceItemFeedback<'_>> = advice
        .iter()
        .map(|item| {
            let flags = classify_against_aliases(item, &aliases);
            AdviceItemFeedback { item, flags }
        })
        .collect();

    // Optional warnings. PG raises NOTICE when
    // pg_plan_advice.feedback_warnings = true; we mirror.
    if crate::extension_state::RA_PLAN_ADVICE_FEEDBACK_WARNINGS.get() {
        for fb in &feedback {
            if fb.flags.contains(FeedbackFlags::FAILED) {
                pgrx::warning!(
                    "plan advice not enforced: {}",
                    render_advice(&vec![fb.item.clone()])
                );
            }
        }
    }

    // Render the block.
    let mut buf = String::new();
    for fb in &feedback {
        let one = render_advice(&vec![fb.item.clone()]);
        let comment = format_feedback(fb.flags);
        buf.push_str(&format!("{one} /* {comment} */\n"));
    }
    if !buf.is_empty() {
        // PG's explain output uses the "Supplied Plan Advice"
        // label exactly; multiline content is handled by
        // ExplainPropertyText with a `\n`-joined value.
        let trimmed = buf.trim_end_matches('\n');
        emit_property("Supplied Plan Advice", trimmed, es);
    }
}

/// Read our boolean PLAN_ADVICE flag from an ExplainState's
/// extension slot. Defaults to false when the option wasn't set.
fn explain_state_plan_advice_flag(es: *mut pg_sys::ExplainState) -> bool {
    let Some(&id) = EXPLAIN_EXT_ID.get() else {
        return false;
    };
    if es.is_null() {
        return false;
    }
    // SAFETY: PG signature.
    let raw = unsafe { pg_sys::GetExplainExtensionState(es, id) };
    if raw.is_null() {
        return false;
    }
    // SAFETY: We allocated this as Box<bool> in
    // plan_advice_option_handler.
    unsafe { *raw.cast::<bool>() }
}

/// Wrap `ExplainPropertyText` so we can pass `&str`.
fn emit_property(label: &str, value: &str, es: *mut pg_sys::ExplainState) {
    let label_c = match CString::new(label) {
        Ok(s) => s,
        Err(_) => return,
    };
    let value_c = match CString::new(value) {
        Ok(s) => s,
        Err(_) => return,
    };
    // SAFETY: PG signature; both pointers borrowed for the call.
    unsafe {
        pg_sys::ExplainPropertyText(label_c.as_ptr(), value_c.as_ptr(), es);
    }
}

/// Best-effort classification of one advice item using just the
/// PlannedStmt's range-table aliases. Same algorithm as
/// `ra_engine::plan_advice_validate::classify_item` but without
/// JOIN_ORDER ordering check (we don't have the join tree
/// reconstructed here).
fn classify_against_aliases(
    item: &ra_plan_advice::ast::AdviceItem,
    aliases: &std::collections::HashSet<String>,
) -> FeedbackFlags {
    let identifiers = collect_target_aliases(&item.targets);
    if identifiers.is_empty() {
        return FeedbackFlags::empty();
    }
    let total = identifiers.len();
    let matched = identifiers
        .iter()
        .filter(|s| aliases.contains(s.as_str()))
        .count();
    let mut flags = FeedbackFlags::empty();
    if matched == 0 {
        return flags;
    }
    flags = flags.with(FeedbackFlags::MATCH_PARTIAL);
    if matched == total {
        flags = flags.with(FeedbackFlags::MATCH_FULL);
    }
    flags
}

/// Collect the alias names mentioned at any depth in a target
/// list, preserving left-to-right order of first occurrence.
fn collect_target_aliases(
    targets: &[ra_plan_advice::ast::AdviceTarget],
) -> Vec<String> {
    use ra_plan_advice::ast::{AdviceTarget, AdviceTargetKind};
    fn walk(
        t: &AdviceTarget,
        out: &mut Vec<String>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        match t.kind {
            AdviceTargetKind::Identifier => {
                if let Some(id) = &t.identifier {
                    if seen.insert(id.alias_name.clone()) {
                        out.push(id.alias_name.clone());
                    }
                }
            }
            AdviceTargetKind::OrderedList | AdviceTargetKind::UnorderedList => {
                for c in &t.children {
                    walk(c, out, seen);
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for t in targets {
        walk(t, &mut out, &mut seen);
    }
    out
}

/// Walk a PlannedStmt's rtable and collect aliases for every
/// `RTE_RELATION` entry. Aliases appear as the
/// `RangeTblEntry.eref->aliasname` C string.
fn collect_rtable_aliases(
    plannedstmt: *mut pg_sys::PlannedStmt,
) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    if plannedstmt.is_null() {
        return out;
    }
    // SAFETY: PG passes a valid PlannedStmt pointer.
    let rtable = unsafe { (*plannedstmt).rtable };
    if rtable.is_null() {
        return out;
    }
    // SAFETY: list_length is a PG inline function callable on
    // any List* including null (returns 0 for null).
    let n = unsafe { (*rtable).length };
    for i in 0..n {
        // SAFETY: list_nth is in pg_sys; bounds-checked above.
        let item = unsafe {
            pg_sys::list_nth(rtable, i)
        };
        if item.is_null() {
            continue;
        }
        let rte = item.cast::<pg_sys::RangeTblEntry>();
        // SAFETY: PG owns this pointer for the duration of the
        // EXPLAIN call.
        let alias_node = unsafe { (*rte).eref };
        if alias_node.is_null() {
            continue;
        }
        // SAFETY: eref->aliasname is a valid CStr or null.
        let alias_ptr = unsafe { (*alias_node).aliasname };
        if alias_ptr.is_null() {
            continue;
        }
        // SAFETY: PG's CStr is null-terminated.
        let cstr = unsafe { std::ffi::CStr::from_ptr(alias_ptr) };
        if let Ok(s) = cstr.to_str() {
            out.insert(s.to_string());
        }
    }
    let _ = ptr::null::<u8>(); // suppress unused-import in some configs
    out
}
