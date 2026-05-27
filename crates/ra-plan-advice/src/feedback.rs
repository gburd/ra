//! Plan-advice feedback flags and rendering.
//!
//! Mirrors `PGPA_FB_*` flags from
//! `contrib/pg_plan_advice/pg_plan_advice.h` and the rendering
//! logic in `pgpa_trove_append_flags()`
//! (`contrib/pg_plan_advice/pgpa_trove.c:340-365`).
//!
//! The output strings ("matched", "partially matched",
//! "not matched", "inapplicable", "conflicting", "failed") match
//! PG byte-for-byte so log filtering and tooling between Ra and PG
//! is interoperable.

use serde::{Deserialize, Serialize};

/// Feedback flags accumulated for one supplied advice item.
///
/// Bit values match `PGPA_FB_*` in `pg_plan_advice.h:42-46`. PG
/// stores these as plain `int` flags; we use a `u8`-backed
/// bitflags-style struct because the set is small and flags don't
/// extend in width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[repr(transparent)]
pub struct FeedbackFlags(pub u8);

impl FeedbackFlags {
    /// At least some part of the query matched the target — e.g.
    /// for `JOIN_ORDER(a b)`, this fires if any joinrel including
    /// either `a` or `b` was seen during planning.
    pub const MATCH_PARTIAL: u8 = 0x01;
    /// An exact match for the target was found — e.g. for
    /// `JOIN_ORDER(a b)`, a joinrel containing exactly `a` and
    /// `b` and nothing else.
    pub const MATCH_FULL: u8 = 0x02;
    /// The advice tag couldn't be applied to the target — e.g.
    /// `INDEX_SCAN(foo bar_idx)` where `bar_idx` doesn't exist.
    pub const INAPPLICABLE: u8 = 0x04;
    /// Two or more advice items request incompatible behaviors —
    /// e.g. seq-scan + index-scan on the same table.
    pub const CONFLICTING: u8 = 0x08;
    /// The resulting plan does not conform to the advice. Only
    /// occurs alongside `MATCH_PARTIAL` or `MATCH_FULL`.
    pub const FAILED: u8 = 0x10;

    /// Empty feedback set.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Test whether a particular flag is set.
    #[must_use]
    pub const fn contains(self, flag: u8) -> bool {
        (self.0 & flag) != 0
    }

    /// Set a flag; returns the new value.
    #[must_use]
    pub const fn with(self, flag: u8) -> Self {
        Self(self.0 | flag)
    }

    /// Clear a flag; returns the new value.
    #[must_use]
    pub const fn without(self, flag: u8) -> Self {
        Self(self.0 & !flag)
    }
}

/// Render feedback flags using PG's exact wording.
///
/// Matches `pgpa_trove_append_flags` in
/// `contrib/pg_plan_advice/pgpa_trove.c:345-364` byte-for-byte.
/// The leading match-status word is one of `matched` /
/// `partially matched` / `not matched`, followed by zero or more
/// comma-separated qualifiers `inapplicable`, `conflicting`,
/// `failed`.
///
/// # Examples
///
/// ```
/// use ra_plan_advice::feedback::{format_feedback, FeedbackFlags};
///
/// // Empty flags -> "not matched" (PG's default state)
/// assert_eq!(format_feedback(FeedbackFlags::empty()), "not matched");
///
/// // Full match
/// let f = FeedbackFlags::empty()
///     .with(FeedbackFlags::MATCH_PARTIAL)
///     .with(FeedbackFlags::MATCH_FULL);
/// assert_eq!(format_feedback(f), "matched");
///
/// // Match plus failed-to-enforce
/// let f = f.with(FeedbackFlags::FAILED);
/// assert_eq!(format_feedback(f), "matched, failed");
/// ```
#[must_use]
pub fn format_feedback(flags: FeedbackFlags) -> String {
    let mut s = String::with_capacity(40);
    // The MATCH_FULL bit's PG-side invariant is that
    // MATCH_PARTIAL is also set; the renderer trusts that.
    if flags.contains(FeedbackFlags::MATCH_FULL) {
        s.push_str("matched");
    } else if flags.contains(FeedbackFlags::MATCH_PARTIAL) {
        s.push_str("partially matched");
    } else {
        s.push_str("not matched");
    }
    if flags.contains(FeedbackFlags::INAPPLICABLE) {
        s.push_str(", inapplicable");
    }
    if flags.contains(FeedbackFlags::CONFLICTING) {
        s.push_str(", conflicting");
    }
    if flags.contains(FeedbackFlags::FAILED) {
        s.push_str(", failed");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pg_wording_for_match_state_pivot() {
        // Three states: full, partial, none.
        let none = FeedbackFlags::empty();
        let partial = FeedbackFlags::empty().with(FeedbackFlags::MATCH_PARTIAL);
        let full = partial.with(FeedbackFlags::MATCH_FULL);

        assert_eq!(format_feedback(none), "not matched");
        assert_eq!(format_feedback(partial), "partially matched");
        assert_eq!(format_feedback(full), "matched");
    }

    #[test]
    fn qualifiers_appear_in_pg_order() {
        // PG renders inapplicable, conflicting, failed in that
        // exact order. Verify by setting them all on a "matched"
        // base and checking the comma-separated list.
        let f = FeedbackFlags::empty()
            .with(FeedbackFlags::MATCH_PARTIAL)
            .with(FeedbackFlags::MATCH_FULL)
            .with(FeedbackFlags::INAPPLICABLE)
            .with(FeedbackFlags::CONFLICTING)
            .with(FeedbackFlags::FAILED);
        assert_eq!(
            format_feedback(f),
            "matched, inapplicable, conflicting, failed",
        );
    }

    #[test]
    fn qualifier_subset_renders_only_set_qualifiers() {
        // From the PG docs example:
        // INDEX_SCAN(f no_such_index) /* matched, inapplicable, failed */
        let f = FeedbackFlags::empty()
            .with(FeedbackFlags::MATCH_PARTIAL)
            .with(FeedbackFlags::MATCH_FULL)
            .with(FeedbackFlags::INAPPLICABLE)
            .with(FeedbackFlags::FAILED);
        assert_eq!(format_feedback(f), "matched, inapplicable, failed");
    }

    #[test]
    fn flag_bit_values_match_pg() {
        // From pg_plan_advice.h:42-46
        assert_eq!(FeedbackFlags::MATCH_PARTIAL, 0x01);
        assert_eq!(FeedbackFlags::MATCH_FULL,    0x02);
        assert_eq!(FeedbackFlags::INAPPLICABLE,  0x04);
        assert_eq!(FeedbackFlags::CONFLICTING,   0x08);
        assert_eq!(FeedbackFlags::FAILED,        0x10);
    }
}
