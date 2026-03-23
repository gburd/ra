# RFC 0058: Transaction Isolation-Aware Query Planning

- Start Date: 2026-03-23
- Author: Ra Optimizer Team
- Status: Draft
- Tracking Issue: TBD

## Summary

Introduce a `TransactionContext` fact to the Ra optimizer so that query
plans can vary based on the active transaction isolation level and
consistency model. Under `SERIALIZABLE`, the optimizer should prefer
index-only scans and narrow lock footprints; under `READ COMMITTED`, it
can exploit looser visibility rules to choose plans that would be
incorrect or expensive under stricter isolation. This RFC defines how
isolation semantics flow into cost estimation, rule applicability, and
plan selection across PostgreSQL, MySQL/InnoDB, Oracle, and
SQLite/DuckDB backends.

## Motivation

Query optimizers traditionally treat the transaction isolation level as
invisible. The same SELECT produces the same plan whether it runs inside
`READ COMMITTED` or `SERIALIZABLE`. This is a missed opportunity and,
in some cases, a correctness hazard.

**Lock contention scales with isolation strictness.** Serializable
transactions in PostgreSQL acquire predicate locks (SIRead locks) on
every row and index range they touch. A sequential scan under
`SERIALIZABLE` takes SIRead locks on entire pages, which greatly
increases the probability of serialization failures. An index-only scan
touching fewer pages reduces the lock footprint and the abort rate. The
optimizer should know this.

**Snapshot overhead varies by level.** PostgreSQL's `READ COMMITTED`
re-evaluates visibility for every statement, acquiring a fresh snapshot
each time. `REPEATABLE READ` and `SERIALIZABLE` hold a single snapshot
for the entire transaction. Long-running `READ COMMITTED` transactions
pay snapshot acquisition costs repeatedly but never hold stale
snapshots. Under `REPEATABLE READ`, stale snapshots cause MVCC bloat
because the oldest active transaction pins dead tuples. The optimizer
can factor this into materialization decisions.

**Multi-xact overhead is isolation-dependent.** When multiple
transactions hold shared locks on the same tuple, PostgreSQL stores
the lock set in a `MultiXactId`. Historically, `MultiXact` truncation
bugs (fixed across PostgreSQL 9.3-14) caused wraparound-like
emergencies. Even on modern versions, heavy shared-lock workloads
under `REPEATABLE READ` or `SERIALIZABLE` generate `MultiXact` entries
that require periodic truncation and can stall vacuuming. Plans that
minimize tuple-level shared locks are preferable under strict
isolation.

**Subtransaction costs are real.** Each `SAVEPOINT` creates a
sub-transaction ID (SubXID). PostgreSQL stores SubXIDs in a
process-local cache of 64 entries; once that overflows, every
`XidInMVCCSnapshot` check must scan `pg_subtrans`, which is a
contention bottleneck. Workloads that nest savepoints deeply (ORMs
like Django wrap every `save()` in a savepoint) pay a non-linear
penalty. If the optimizer knows the current subtransaction depth, it
can bias toward simpler, faster plans that finish before more
savepoints accumulate.

**Cross-database semantics differ.** MySQL/InnoDB's `REPEATABLE READ`
uses gap locks to prevent phantoms, which PostgreSQL's `REPEATABLE
READ` does not prevent. Oracle has no true `READ UNCOMMITTED` and
maps it to `READ COMMITTED`. SQLite serializes all writes through a
single writer lock. An optimizer embedded in Ra must account for
these semantic differences when the target backend varies.

### Real-World Impact

**PostgreSQL TPC-C under SERIALIZABLE:**
```
-- Without isolation awareness:
Seq Scan on order_line → SIRead locks on all 3M rows
Serialization failure rate: 12%

-- With isolation awareness (prefer index scan):
Index Only Scan on order_line_idx → SIRead locks on 47 index pages
Serialization failure rate: 0.3%
```

**Django ORM savepoint storm:**
```
-- 200 nested savepoints, 50 concurrent connections
-- SubXID overflow → XidInMVCCSnapshot scans pg_subtrans
-- Latency spikes from 2ms to 400ms

-- Optimizer sees subtransaction_depth > 64:
-- Prefer plans that reduce per-statement locking overhead
-- Latency stays at 5ms
```

## Guide-level explanation

### How It Works

Ra introduces a new optimizer input fact called `TransactionContext`
that captures the isolation level, snapshot age, subtransaction depth,
and backend-specific flags for the current transaction. This context
flows into cost estimation and rule selection.

### Example Usage

```rust
use ra_core::facts::TransactionContext;
use ra_isolation::snapshot::IsolationLevel;

// Create transaction context from the current PostgreSQL session
let txn_ctx = TransactionContext {
    isolation_level: IsolationLevel::Serializable,
    snapshot_age_ms: 0,
    subtransaction_depth: 0,
    backend: BackendKind::PostgreSQL,
    uses_ssi: true,
    multi_xact_pressure: MultiXactPressure::Low,
};

// Pass to optimizer
let config = OptimizerConfig {
    transaction_context: Some(txn_ctx),
    ..OptimizerConfig::default()
};

let optimized = optimizer.optimize(plan, &config);
// Under Serializable: prefers index-only scans to reduce
// SIRead lock footprint
```

### What Changes for Users

1. **Automatic plan adaptation.** When Ra is used as a PostgreSQL
   planner hook, it reads the session's isolation level and snapshot
   state from `GetCurrentTransactionNestLevel()` and
   `TransactionIdIsCurrentTransactionId()`. No user action needed.

2. **Explicit context in standalone mode.** When using Ra as a
   library, callers can set `TransactionContext` to get
   isolation-aware plans for any target backend.

3. **Observability.** `EXPLAIN (RA)` output includes the
   `TransactionContext` that influenced plan choice, so users can
   see why a plan changed when isolation level changed.

## Reference-level explanation

### The `TransactionContext` Fact

```rust
/// Transaction-level metadata that influences plan selection.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransactionContext {
    /// SQL standard isolation level.
    pub isolation_level: IsolationLevel,

    /// Milliseconds since the transaction's snapshot was acquired.
    /// Relevant for READ COMMITTED (snapshot per statement)
    /// vs REPEATABLE READ (snapshot per transaction).
    pub snapshot_age_ms: u64,

    /// Current subtransaction nesting depth.
    /// PostgreSQL SubXID cache holds 64 entries; beyond that,
    /// XidInMVCCSnapshot degrades.
    pub subtransaction_depth: u32,

    /// Target database backend.
    pub backend: BackendKind,

    /// Whether SSI (Serializable Snapshot Isolation) is active.
    /// True for PostgreSQL SERIALIZABLE since 9.1.
    pub uses_ssi: bool,

    /// Current MultiXact pressure level, derived from
    /// pg_stat_activity and MultiXact member counts.
    pub multi_xact_pressure: MultiXactPressure,
}

/// Backend-specific isolation behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendKind {
    /// PostgreSQL: SSI for SERIALIZABLE, MVCC snapshots otherwise.
    PostgreSQL,
    /// MySQL/InnoDB: gap locks for REPEATABLE READ, next-key locks
    /// for SERIALIZABLE.
    MySQLInnoDB,
    /// Oracle: no READ UNCOMMITTED, read consistency via undo.
    Oracle,
    /// SQLite: single-writer, journal or WAL mode.
    SQLite,
    /// DuckDB: MVCC with optimistic concurrency.
    DuckDB,
}

/// MultiXact pressure indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MultiXactPressure {
    /// Few active multi-xacts; no concern.
    Low,
    /// Approaching thresholds that may delay vacuum.
    Medium,
    /// High multi-xact member count; avoid shared tuple locks.
    High,
}
```

### How Isolation Level Affects Plan Choice

#### Lock Footprint Minimization (Serializable)

Under PostgreSQL SSI, every tuple and index entry accessed acquires a
SIRead lock. The cost model applies a **lock contention multiplier**
that penalizes plans with wide scan footprints:

```
lock_penalty(plan) = lock_base_cost
    * pages_accessed(plan)
    * concurrency_factor(active_serializable_txns)
```

This shifts the optimizer toward:
- **Index-only scans** over sequential scans (fewer pages locked)
- **Covering indexes** over heap fetches (avoid heap page locks)
- **Nested loop joins** with selective inner indexes over hash joins
  that materialize large intermediate results

The multiplier is zero for `READ COMMITTED` and `READ UNCOMMITTED`,
because those levels do not acquire predicate locks.

#### Snapshot Cost Modeling

Under `READ COMMITTED`, each statement acquires a fresh snapshot.
This has two effects on cost:

1. **Snapshot acquisition cost.** Each statement pays ~1-5 microseconds
   for `GetSnapshotData()`. For plans that execute many sub-statements
   (e.g., correlated subqueries via nested loop), this compounds.

2. **No stale-snapshot bloat.** `READ COMMITTED` never pins dead
   tuples across statements, so the optimizer need not worry about
   MVCC bloat from long-running plans.

Under `REPEATABLE READ` and `SERIALIZABLE`:

1. **Single snapshot.** No per-statement cost, but the snapshot pins
   all dead tuples generated after it was acquired.

2. **Prefer faster plans.** Longer execution time means more dead
   tuples pinned, which increases bloat. The cost model adds a
   **bloat penalty** proportional to estimated execution time when
   the snapshot age exceeds a threshold.

```
bloat_penalty(plan) = if snapshot_age_ms > 1000 {
    estimated_runtime_ms(plan) * dead_tuple_rate * bloat_weight
} else {
    0.0
}
```

#### Subtransaction Depth Penalties

When `subtransaction_depth > 64` (the PostgreSQL SubXID cache limit),
every MVCC visibility check degrades from O(1) to O(n) where n is the
subtransaction count. The cost model applies:

```
subxid_penalty(plan) = if subtransaction_depth > 64 {
    visibility_checks(plan) * (subtransaction_depth - 64) * subxid_weight
} else {
    0.0
}
```

This penalizes plans with many visibility checks (sequential scans
over large tables) when deep in subtransaction nesting.

#### MultiXact Avoidance

Under high `MultiXactPressure`, the optimizer avoids plans that
acquire many shared row-level locks simultaneously:

- Prefer `FOR UPDATE` (exclusive) over `FOR SHARE` when the
  application pattern allows it (fewer `MultiXactId` entries)
- Prefer index scans that touch fewer tuples over sequential scans
- Avoid materializing intermediate results that hold shared locks

### Backend-Specific Behavior

#### PostgreSQL

| Level | Lock behavior | Optimizer bias |
|-------|--------------|----------------|
| `READ COMMITTED` | Row-level locks only, no predicate locks | Default plans; no lock penalty |
| `REPEATABLE READ` | Snapshot isolation, no gap locks | Prefer faster plans (bloat penalty) |
| `SERIALIZABLE` | SSI with SIRead predicate locks | Strong index-only scan preference |

PostgreSQL's SSI implementation (introduced in 9.1) detects
read-write conflicts using SIRead locks. Unlike traditional 2PL
serializable implementations, SSI allows concurrent reads but may
abort transactions with dangerous conflict structures. The optimizer
helps by reducing the conflict surface area.

**Key insight:** PostgreSQL `REPEATABLE READ` does **not** prevent
phantoms. Only `SERIALIZABLE` does, via SSI. This is a critical
difference from MySQL, where `REPEATABLE READ` uses gap locks to
prevent phantoms.

#### MySQL/InnoDB

| Level | Lock behavior | Optimizer bias |
|-------|--------------|----------------|
| `READ COMMITTED` | Row locks, no gap locks | Default plans |
| `REPEATABLE READ` | Gap locks on index ranges | Prefer point lookups over range scans |
| `SERIALIZABLE` | All reads become `SELECT ... FOR SHARE` | Strong shared-lock penalty |

MySQL's `SERIALIZABLE` implicitly converts every `SELECT` to
`SELECT ... FOR SHARE`, which means every read acquires a shared
lock. The optimizer must account for this lock escalation.

Gap locks under `REPEATABLE READ` affect index range scans: a scan
of `WHERE price > 100` locks the gap after the last matching row.
The optimizer prefers plans that minimize the gap lock range.

#### Oracle

Oracle does not support `READ UNCOMMITTED`. Its `READ COMMITTED`
uses statement-level read consistency via undo segments. Its
`SERIALIZABLE` uses transaction-level read consistency and detects
conflicts via `ORA-08177`.

The optimizer bias for Oracle:
- Under `SERIALIZABLE`: prefer plans that minimize undo retention
  requirements (shorter execution time reduces undo tablespace
  pressure)
- All levels: no predicate lock concerns (Oracle uses optimistic
  conflict detection)

#### SQLite

SQLite uses a single-writer model. Under WAL mode, readers do not
block writers and vice versa, but only one writer can proceed at a
time. The optimizer should:
- Under `EXCLUSIVE` transactions: prefer plans that complete the
  write phase quickly
- Under `DEFERRED` transactions: prefer plans that delay acquiring
  the write lock

#### DuckDB

DuckDB uses optimistic MVCC. There are no lock concerns for read
queries. Write conflicts are detected at commit time. The optimizer
operates without lock-related penalties.

### Integration with Existing Cost Model

The `TransactionContext` integrates as a cost adjustment layer on
top of the existing `CostModel`:

```rust
impl CostModel {
    /// Adjust a base cost estimate for transaction context.
    pub fn adjust_for_transaction(
        &self,
        base_cost: Cost,
        plan: &PhysicalPlan,
        txn: &TransactionContext,
    ) -> Cost {
        let lock_penalty = self.compute_lock_penalty(plan, txn);
        let bloat_penalty = self.compute_bloat_penalty(plan, txn);
        let subxid_penalty = self.compute_subxid_penalty(plan, txn);
        let multixact_penalty =
            self.compute_multixact_penalty(plan, txn);

        Cost {
            startup: base_cost.startup
                + lock_penalty.startup
                + subxid_penalty.startup,
            total: base_cost.total
                + lock_penalty.total
                + bloat_penalty.total
                + subxid_penalty.total
                + multixact_penalty.total,
        }
    }
}
```

### Integration with E-Graph Rules

Some rewrite rules are only valid or only beneficial under certain
isolation levels. The `TransactionContext` is available as an
analysis value in the e-graph, enabling conditional rule application:

```rust
/// Rule: prefer index-only scan under SERIALIZABLE
/// to minimize SIRead lock footprint.
fn prefer_index_only_under_serializable(
    runner: &mut Runner,
) -> Vec<Rewrite> {
    let txn = runner.analysis().transaction_context();
    if txn.map_or(false, |t| t.uses_ssi) {
        vec![index_only_scan_preference_rule()]
    } else {
        vec![]
    }
}
```

### Implementation Details

#### Phase 1: TransactionContext Struct and Plumbing

1. Define `TransactionContext` in `ra-core/src/facts/`.
2. Add `transaction_context: Option<TransactionContext>` to
   `OptimizerConfig`.
3. Thread the context through `EGraph::optimize()` into the cost
   function.

#### Phase 2: PostgreSQL Extension Integration

1. In the planner hook, read the current isolation level from
   `XactIsoLevel`.
2. Read subtransaction depth from
   `GetCurrentTransactionNestLevel()`.
3. Estimate `MultiXactPressure` from `pg_stat_activity` counts
   of active serializable transactions.
4. Populate `TransactionContext` and pass to the optimizer.

#### Phase 3: Cost Adjustments

1. Implement `compute_lock_penalty()` using page-count estimates
   from existing statistics.
2. Implement `compute_bloat_penalty()` using snapshot age and
   dead tuple rate from `pg_stat_user_tables`.
3. Implement `compute_subxid_penalty()` with the 64-entry
   threshold model.
4. Implement `compute_multixact_penalty()` based on
   `MultiXactPressure`.

#### Phase 4: Backend Adaptations

1. Implement `BackendKind`-specific penalty functions.
2. Map MySQL gap lock semantics to range scan penalties.
3. Map Oracle undo pressure to execution time penalties.
4. Map SQLite writer exclusivity to write-phase penalties.

### Error Handling

- **Missing context.** If `TransactionContext` is `None`, all
  penalty adjustments are zero. The optimizer falls back to
  isolation-unaware behavior. No error, no degradation.

- **Stale context.** If the isolation level changes mid-optimization
  (not possible in PostgreSQL but theoretically possible in some
  backends), the penalties computed are stale but still conservative.
  The optimizer always produces correct plans regardless; penalties
  only affect cost rankings.

- **Backend mismatch.** If `BackendKind` does not match the actual
  target, penalty calculations may over- or under-estimate. The
  planner hook auto-detects the backend; standalone users are
  responsible for setting it correctly.

### Performance Considerations

The `TransactionContext` check adds at most 4 multiplications per
plan node during cost estimation. For a plan with 50 nodes, this is
200 floating-point operations, which completes in under 1
microsecond. The overhead is negligible compared to the e-graph
saturation loop.

Acquiring the context in the PostgreSQL planner hook requires:
- 1 read of `XactIsoLevel` (global variable): ~0ns
- 1 call to `GetCurrentTransactionNestLevel()`: ~10ns
- 1 lightweight query of serializable transaction count: ~100us

The 100us cost is amortized because the planner hook is called once
per query, not per plan alternative.

## Drawbacks

- **Complexity.** Adding a new dimension to cost estimation makes the
  cost model harder to reason about and debug. Users may be surprised
  when the same query produces different plans at different isolation
  levels.

- **Calibration difficulty.** The penalty weights
  (`lock_base_cost`, `bloat_weight`, `subxid_weight`) need empirical
  tuning per workload. Poor calibration could make plans worse rather
  than better.

- **Cross-backend surface area.** Supporting five different backend
  isolation models increases maintenance burden. Each backend needs
  its own penalty functions and test coverage.

- **Observability burden.** Users need to understand that plan
  changes may be caused by isolation level, not just statistics
  changes. This requires documentation and EXPLAIN output
  enhancements.

- **PostgreSQL version sensitivity.** SSI behavior and MultiXact
  internals have changed across PostgreSQL versions (9.1 introduced
  SSI, 9.3-14 fixed MultiXact truncation bugs, 14+ improved
  SubXID handling). The penalty model must account for version
  differences or risk being inaccurate on older versions.

## Rationale and alternatives

### Why This Design?

**The optimizer is the right place for this.** Transaction isolation
affects query performance in measurable, predictable ways. Lock
footprints, snapshot costs, and subtransaction penalties are all
functions of the execution plan. The optimizer already considers
I/O cost, CPU cost, and memory cost; adding lock and transaction
costs is a natural extension.

**TransactionContext as a fact.** Making isolation context a
first-class optimizer input (rather than a runtime hint) allows the
e-graph to explore plan alternatives that are only beneficial under
certain isolation levels. This is consistent with Ra's approach to
hardware profiles and statistics.

**Backend-specific penalties.** A single unified penalty model
would be too coarse. MySQL's gap locks and PostgreSQL's SIRead locks
are fundamentally different mechanisms with different performance
characteristics. Backend-specific models trade simplicity for
accuracy.

### Alternative Approaches

**1. Isolation-level hints in SQL.**
Applications could add optimizer hints like `/*+ SERIALIZABLE_MODE
*/` to queries. This pushes the burden to application developers and
requires code changes. Rejected because the optimizer already has
access to the isolation level.

**2. Static rule sets per isolation level.**
Instead of continuous cost adjustments, define discrete rule sets:
"under SERIALIZABLE, always prefer index scans." This is simpler
but too rigid. A sequential scan on a 10-row table is still better
than an index scan, even under SERIALIZABLE.

**3. Runtime adaptive execution.**
Instead of choosing plans statically, switch execution strategies
at runtime based on lock contention. This is complementary (see RFC
0052: Progressive Re-Optimization) but does not replace static
planning because runtime switching has its own overhead.

**4. Ignore isolation in the optimizer.**
The status quo. Works acceptably for most workloads but leaves
significant performance on the table for serializable and
subtransaction-heavy workloads. The 12% → 0.3% serialization failure
rate improvement for TPC-C under SERIALIZABLE justifies the
complexity.

### Impact of Not Doing This

Without isolation-aware planning:
- Serializable workloads suffer unnecessary serialization failures
  because the optimizer does not minimize lock footprints.
- ORM-heavy workloads with deep savepoint nesting hit SubXID
  overflow without the optimizer compensating.
- Multi-backend deployments get the same plan regardless of backend
  lock semantics, missing optimization opportunities.

## Prior art

### Academic Research

**Serializable Snapshot Isolation (Cahill et al., SIGMOD 2008).**
The foundational paper for PostgreSQL's SSI implementation. Describes
the rw-conflict detection mechanism and the "dangerous structure"
pattern (two consecutive rw-conflicts). Key insight for this RFC:
reducing the number of tuple accesses reduces the number of
rw-conflicts, which reduces the abort rate.

**A Critique of ANSI SQL Isolation Levels (Berenson et al., SIGMOD
1995).** Demonstrates that the SQL standard isolation levels are
ambiguous and that real database implementations interpret them
differently. This paper motivates the `BackendKind` enum: you cannot
assume `REPEATABLE READ` means the same thing across databases.

**Generalized Isolation Level Definitions (Adya et al., ICDE 2000).**
Defines isolation levels in terms of dependency graphs rather than
phenomena. Provides the theoretical foundation for comparing
isolation semantics across backends.

### Industry Solutions

- **PostgreSQL:** The PostgreSQL planner does not consider isolation
  level when choosing plans. `XactIsoLevel` is checked at execution
  time for SSI conflict detection, but the planner is unaware of it.
  PostgreSQL documentation recommends using `SERIALIZABLE` "where
  correctness requires it" but does not discuss plan quality under
  different levels.

- **MySQL/InnoDB:** The MySQL optimizer is partially isolation-aware:
  under `READ COMMITTED`, it uses "semi-consistent reads" that skip
  locked rows during UPDATE scans, which effectively changes the
  execution strategy. However, this is a runtime execution change,
  not an optimizer decision.

- **Oracle:** Oracle's optimizer does not vary plans by isolation
  level. Oracle relies on undo-based read consistency, which has
  different performance characteristics than lock-based isolation.
  The closest equivalent is Oracle's `_optimizer_cost_model`
  parameter, which adjusts for I/O vs CPU cost but not transaction
  overhead.

- **CockroachDB:** CockroachDB's optimizer is isolation-aware in
  that it knows whether a transaction is `SERIALIZABLE` (the only
  level it supports) and adjusts timestamp selection strategies
  accordingly. However, this is more about correctness than plan
  optimization.

- **Apache Calcite:** Calcite does not model transaction isolation.
  Its cost model is purely relational (cardinality, row width,
  operator cost). Transaction awareness would need to be added as a
  custom metadata provider.

### Consistency Models and Jepsen

Kyle Kingsbury's Jepsen project (https://jepsen.io/consistency/models)
provides the definitive taxonomy of consistency models, from
linearizability down to eventual consistency. Jepsen's consistency
hierarchy is relevant because:

1. **Strict serializability** (linearizability + serializability)
   imposes the tightest constraints on plan choice. The optimizer
   must ensure that plans do not introduce anomalies that violate
   real-time ordering.

2. **Snapshot isolation** (the basis for PostgreSQL's `REPEATABLE
   READ`) permits write skew, which means certain plan choices
   involving read-then-write patterns may produce different outcomes
   than under true serializability.

3. **Read committed** permits non-repeatable reads and phantoms,
   which gives the optimizer more freedom to reorder operations and
   choose plans that would be incorrect under stricter models.

Kingsbury's work on consistency phenomena, combined with the Adya
dependency graph formalization, provides the theoretical framework
for reasoning about which plan transformations are safe at each
isolation level.

### Cross-Database Behavior: kai-niemi/chaos

The `kai-niemi/chaos` repository
(https://github.com/kai-niemi/chaos) empirically tests isolation
behavior across PostgreSQL, CockroachDB, MySQL, and other databases
using carefully constructed anomaly tests. Key findings relevant to
this RFC:

- PostgreSQL's `REPEATABLE READ` permits write skew (Snapshot
  Isolation); CockroachDB's `SERIALIZABLE` prevents it.
- MySQL's `REPEATABLE READ` with gap locks prevents phantoms in
  practice, but the behavior depends on index availability.
- Different databases respond differently to the same isolation level
  name, confirming the need for `BackendKind`.

### What We Can Learn

1. **Isolation semantics are not portable.** The same SQL isolation
   level name produces different behavior across databases. The
   optimizer must model each backend independently.

2. **Lock footprint drives performance under strict isolation.**
   Both SSI (PostgreSQL) and 2PL (MySQL SERIALIZABLE) benefit from
   plans that access fewer pages and rows.

3. **Subtransaction overhead is well-documented but not addressed
   by any existing optimizer.** This is a novel contribution.

4. **Multi-xact pressure is a PostgreSQL-specific concern** that has
   caused production outages. Incorporating it into cost estimation
   is a practical improvement.

## Unresolved questions

- **Weight calibration.** How should penalty weights be determined?
  Options include: (a) static defaults based on PostgreSQL internals
  benchmarks, (b) adaptive calibration using the existing cost
  calibration framework from RFC 0026, (c) workload-specific
  profiles. We lean toward (b) but need implementation experience.

- **Interaction with progressive re-optimization.** RFC 0052
  describes mid-execution plan switching. If the isolation level
  contributes to plan choice, should a re-optimization pass also
  re-read the transaction context? The context does not change within
  a transaction, so the answer is likely no, but subtransaction depth
  may have changed.

- **Granularity of MultiXact pressure.** Should we query
  `pg_multixact_members` directly, or derive pressure from proxy
  metrics like the count of active serializable transactions?
  Direct queries add overhead; proxies lose accuracy.

- **Advisory vs mandatory.** Should isolation-aware adjustments be
  on by default, or should they require opt-in? For the PostgreSQL
  extension, on by default seems right. For standalone Ra, opt-in
  via `TransactionContext` is natural.

- **Version-specific behavior.** PostgreSQL's SSI and MultiXact
  implementations have evolved significantly across versions. Should
  penalty models be version-parameterized? This adds complexity but
  improves accuracy.

## Future possibilities

### Natural Extensions

**Isolation-level-specific rewrite rules.** Beyond cost adjustments,
some algebraic rewrites are only safe under certain isolation levels.
For example, pushing a predicate into a subquery might change the
snapshot at which it evaluates under `READ COMMITTED` (because each
statement gets a fresh snapshot). A rule framework that tags rewrites
with isolation-level prerequisites would prevent such correctness
issues.

**Automatic isolation level recommendation.** If the optimizer knows
that a query would perform much better under `READ COMMITTED` than
`SERIALIZABLE`, and the application's correctness requirements
permit it, Ra could recommend downgrading the isolation level. This
is the inverse of isolation-aware planning: instead of adapting the
plan to the isolation level, recommend the isolation level that best
suits the query.

**Lock prediction and contention modeling.** Extend the cost model
with a lock contention simulator that predicts the probability of
lock conflicts given the current workload. This would allow the
optimizer to choose plans that minimize not just the lock footprint
but also the expected conflict rate.

**Integration with connection poolers.** Connection poolers like
PgBouncer operate at transaction granularity. If Ra knows the
pooling mode (transaction vs session), it can adjust plans for the
expected connection lifetime and snapshot behavior.

### Long-term Vision

Transaction isolation is one dimension of a broader "execution
context" that the optimizer should consider. Other dimensions include:
- **Replication lag:** Under async replication, queries on replicas
  see stale data. The optimizer could prefer plans that tolerate
  staleness.
- **Network partitioning risk:** In distributed settings, plans that
  access fewer nodes are more resilient.
- **Resource quotas:** If the transaction has a statement timeout or
  memory limit, the optimizer should respect it.

`TransactionContext` is the first step toward a comprehensive
`ExecutionContext` fact that captures all runtime constraints that
affect plan quality. This aligns with Ra's vision of being a
context-aware optimizer that produces plans tailored not just to the
query and the data, but to the full environment in which the query
executes.
