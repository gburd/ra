# Chores & Small Tasks

This page tracks small tasks that don't require a full RFC but are necessary for project completion.

::: info
The canonical chores list is maintained at [`/rfcs/CHORES.md`](https://codeberg.org/gregburd/ra/src/branch/main/rfcs/CHORES.md).
View it directly for the most up-to-date task list.
:::

---

## Priority Levels

- **P0**: Blocking / Critical - Must be done before next release
- **P1**: High Priority - Should be done soon
- **P2**: Medium Priority - Nice to have in next few releases
- **P3**: Low Priority / Nice to Have - Can wait

---

## Quick Summary by Subsystem

### P0 Critical Items (Blocking)

**ra-cli**:
- Fix 3 failing tests in `migrate_commands.rs`
- Re-enable or properly implement regression commands

**ra-pg-extension**:
- Run integration tests: `cargo pgrx test pg17`
- Test with TPC-H queries
- Verify MVCC/HOT statistics gathering
- Test crash recovery

**ra-core**:
- Verify all `RelExpr` pattern matches (no `_` wildcards)
- Document public API

**Build System**:
- Set up continuous benchmarking
- Add coverage tracking (>80% goal)

---

## Component Breakdown

### CLI Tool (`ra-cli`)
- 7 P0 items, 4 P1, 3 P2, 3 P3

### PostgreSQL Extension (`ra-pg-extension`)
- 4 P0 items, 4 P1, 4 P2, 3 P3

### Core Library (`ra-core`)
- 3 P0 items, 4 P1, 4 P2, 3 P3

### Optimization Engine (`ra-engine`)
- 3 P0 items, 4 P1, 4 P2, 3 P3

### SQL Parser (`ra-parser`)
- 3 P0 items, 4 P1, 3 P2, 3 P3

### Documentation
- 3 P0 items, 4 P1, 3 P2, 3 P3

### Testing
- 4 P0 items, 4 P1, 3 P2, 2 P3

### Rules & Optimizations
- 3 P0 items, 4 P1, 3 P2, 3 P3

[View detailed chore list ->](https://codeberg.org/gregburd/ra/src/branch/main/rfcs/CHORES.md)

---

## Missing RFCs

Some gaps identified in research need RFCs (too large for chores):

### High Priority
1. **Semi-Join Reduction** (Gap #2) - 2-3 weeks
2. **Distinct Aggregation Rewrite** (Gap #4) - 1-2 weeks
3. **Partial Aggregation (Two-Phase)** (Gap #6) - verify existing, or 2-3 weeks

### Medium Priority
4. **Decorrelation Improvements** (Gap #10) - verify & RFC if needed
5. **CMU Video Research** - Extract 5-10 RFCs from lectures

[View gap analysis ->](https://codeberg.org/gregburd/ra/src/branch/main/research/gap-analysis/missing-optimizations.md)

---

## How to Pick a Chore

1. **Check priority**: Start with P0 (blocking) items
2. **Pick your subsystem**: Focus on area you know best
3. **Small wins**: P3 tasks are great for new contributors
4. **Ask questions**: Open an issue if unclear

---

## Claiming a Chore

When you start working on a chore:

1. Comment on related issue (or create one)
2. Assign issue to yourself
3. Create feature branch: `git checkout -b chore/subsystem-description`
4. Work on it
5. Submit PR when done

---

## Completing a Chore

When your PR is merged:

1. Chore is marked as complete in CHORES.md
2. It may be moved to a "Completed" section
3. Or simply checked off: `- [x] Task description`

---

## Related Resources

- **[RFCs](./rfcs/)** - Major features requiring design docs
- **[Bugs](./bugs.md)** - Bug tracking and resolution
- **[Component APIs](./components.md)** - Understanding subsystems
