# RFC 0007: Statistics Timeline System

- **Status:** Implemented
- **Type:** Retroactive
- **Author:** RA Contributors
- **Date:** 2026-03-20

---

## Summary

A TOML-based format for describing how database statistics evolve
over time, with a `TimelinePlayer` engine that steps through
snapshots to demonstrate adaptive query optimization under changing
data conditions.

## Motivation

Query optimizer decisions depend on statistics that become stale as
data changes. Demonstrating this effect requires a way to model
statistics evolution over time: how cardinality estimates drift after
bulk inserts, how selectivity changes with data distribution shifts,
and how the optimizer responds to refreshed statistics.

The timeline system serves both education (showing students how
stale statistics cause plan regressions) and testing (validating
that the optimizer adapts to statistics changes).

## What Was Built

### Timeline Format

A TOML file with four top-level sections:

- `[metadata]` -- name, description, database context
- `[[snapshots]]` -- ordered statistics snapshots by time offset
- `[[events]]` -- data modification events (inserts, deletes)
- `[[feedback]]` -- execution feedback (estimated vs actual)

Each snapshot contains per-table statistics: row counts, page counts,
column-level histograms, and selectivity estimates.

### Timeline Player

The `TimelinePlayer` engine:

1. Loads a timeline file
2. Steps through snapshots in order
3. At each snapshot, updates the statistics provider
4. Re-optimizes target queries to show plan changes
5. Records plan decisions at each step for comparison

### Statistics Profiles

Four predefined staleness profiles control how aggressively the
system gathers statistics:

| Profile | Description |
|---------|-------------|
| RealTime | Continuous gathering, never stale |
| Standard | Periodic gathering, moderate staleness |
| Lazy | Infrequent gathering, high staleness |
| Stale | No gathering, statistics frozen |

### Crate

The `ra-stats` crate provides 20+ statistics types, accuracy and
staleness models, and gathering cost estimation.

## Key Design Decisions

- TOML format chosen over JSON for human readability and comments
- Snapshots are ordered by time offset to support both real-time
  replay and fast-forward
- Feedback entries enable closed-loop validation: compare estimated
  vs actual to measure optimizer accuracy over time
- Statistics profiles are composable with hardware profiles for
  combined demonstrations

## Prior Art

- PostgreSQL's `pg_statistic` and auto-analyze system
- Oracle's automatic statistics gathering and SQL Plan Management
- Academic work on adaptive query processing (AQP)

## References

- `docs/statistics-timeline-format.md` -- format specification
- `crates/ra-stats/` -- statistics types and timeline player
- `timelines/` -- example timeline files
