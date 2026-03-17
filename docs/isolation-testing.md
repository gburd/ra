# Cross-Database Isolation Testing

This document describes the ra-isolation crate, which provides a
framework for testing transaction isolation behavior across databases.

## Overview

The isolation testing framework adapts PostgreSQL's `isolationtester`
infrastructure to work across multiple database engines. It parses
`.spec` files that define concurrent transaction scenarios, schedules
step execution across sessions, and detects isolation anomalies.

## Concepts

### Isolation Levels

| Level            | Dirty Read | Non-Repeatable Read | Phantom Read |
|------------------|------------|---------------------|--------------|
| Read Uncommitted | Possible   | Possible            | Possible     |
| Read Committed   | No         | Possible            | Possible     |
| Repeatable Read  | No         | No                  | Possible     |
| Serializable     | No         | No                  | No           |

### Anomaly Types

- **Dirty Read** -- Transaction reads uncommitted data from another
  transaction that later rolls back.
- **Non-Repeatable Read** -- A row read twice within the same
  transaction returns different values because another transaction
  modified it between reads.
- **Phantom Read** -- A range query returns different row sets when
  repeated within the same transaction because another transaction
  inserted or deleted rows.
- **Write Skew** -- Two transactions read overlapping data, make
  disjoint updates based on those reads, and the combined result
  violates an invariant.
- **Lost Update** -- Two transactions read the same row, both
  compute a new value based on the read, and one update overwrites
  the other.

## Spec File Format

Spec files follow PostgreSQL's `.spec` format:

```
setup {
    CREATE TABLE accounts (
        id INTEGER PRIMARY KEY,
        balance INTEGER NOT NULL
    );
    INSERT INTO accounts VALUES (1, 1000), (2, 1000);
}

teardown {
    DROP TABLE accounts;
}

session "s1" {
    step "read1" {
        SELECT balance FROM accounts WHERE id = 1;
    }
    step "write1" {
        UPDATE accounts SET balance = balance - 100 WHERE id = 1;
    }
}

session "s2" {
    step "read2" {
        SELECT balance FROM accounts WHERE id = 1;
    }
    step "write2" {
        UPDATE accounts SET balance = balance + 100 WHERE id = 1;
    }
}

permutation "read1" "read2" "write1" "write2"
permutation "read1" "write2" "read2" "write1"
```

### Sections

- **setup** -- SQL executed once before sessions start.
- **teardown** -- SQL executed after all sessions complete.
- **session** -- Named session with ordered steps.
- **step** -- Named step containing SQL statements.
- **permutation** -- Explicit step ordering. If omitted, all
  permutations of steps are tested (preserving intra-session order).

### Marker Directives

Steps can include synchronization markers:

```sql
-- @marker checkpoint_reached
SELECT * FROM accounts;
```

```sql
-- @wait checkpoint_reached
UPDATE accounts SET balance = 0 WHERE id = 1;
```

Markers allow one session to wait until another session reaches a
specific point before proceeding.

## Architecture

```
.spec file
    |
    v
[spec_parser] --> SpecFile
    |
    v
[scheduler] --> StepOrder (permutations)
    |
    v
[executor]
    |--- session 1 --> DatabaseAdapter
    |--- session 2 --> DatabaseAdapter
    |--- ...
    |
    v
[events] --> TestEventLog
    |
    v
[locks] --> deadlock detection
[snapshot] --> visibility verification
    |
    v
TestResult (pass/fail + anomalies detected)
```

### Components

| Module        | Purpose                                      |
|---------------|----------------------------------------------|
| spec_parser   | Parse `.spec` files into `SpecFile` structs  |
| scheduler     | Generate and manage step orderings           |
| session       | Manage database sessions and transactions    |
| executor      | Coordinate sessions and run complete tests   |
| locks         | Monitor locks and detect deadlocks           |
| snapshot      | Query snapshot visibility for verification   |
| markers       | Synchronization between concurrent sessions  |
| events        | Record test events for diagnostics           |
| wasm_bridge   | Bridge to WASM database backends             |

## Usage

### Running a Spec File

```rust
use ra_isolation::{SpecFile, TestExecutor};

let spec = SpecFile::parse_file("tests/dirty-read.spec")?;
let executor = TestExecutor::new(database_adapter);
let result = executor.run(&spec)?;

if result.passed {
    println!("All permutations passed");
} else {
    for anomaly in &result.anomalies {
        println!("Anomaly: {anomaly:?}");
    }
}
```

### With WASM Databases

The framework can test WASM-compiled databases (SQLite, DuckDB)
through the `wasm_bridge` module or the optional `wasm` feature:

```rust
#[cfg(feature = "wasm")]
use ra_isolation::wasm_adapters;

let adapter = wasm_adapters::sqlite_adapter()?;
let executor = TestExecutor::new(adapter);
```

### Custom Database Adapter

Implement the `DatabaseAdapter` trait to test any database:

```rust
use ra_isolation::DatabaseAdapter;

struct MyDatabaseAdapter { /* ... */ }

impl DatabaseAdapter for MyDatabaseAdapter {
    fn execute(&mut self, sql: &str) -> Result<QueryResult>;
    fn begin_transaction(&mut self, level: IsolationLevel) -> Result<()>;
    fn commit(&mut self) -> Result<()>;
    fn rollback(&mut self) -> Result<()>;
}
```

## Included Spec Files

The `tests/specs/` directory contains isolation tests covering:

- **dirty-read.spec** -- Verifies dirty reads are prevented
- **non-repeatable-read.spec** -- Tests read stability
- **phantom-read.spec** -- Tests range query stability
- **write-skew.spec** -- Tests serializable isolation
- **lost-update.spec** -- Tests concurrent update handling
- **deadlock.spec** -- Tests deadlock detection and resolution

## Lock Monitoring

The `LockMonitor` tracks held and waiting locks across sessions:

```rust
use ra_isolation::{LockMonitor, LockType};

let monitor = LockMonitor::new();
monitor.acquire(session_id, resource, LockType::Exclusive)?;
// Returns Err if deadlock detected
```

Lock types: `Shared`, `Exclusive`, `Update`, `Intent`.

## References

- PostgreSQL isolationtester:
  https://www.postgresql.org/docs/current/regress-isolation.html
- Berenson et al., "A Critique of ANSI SQL Isolation Levels,"
  SIGMOD 1995
- Adya et al., "Generalized Isolation Level Definitions,"
  ICDE 2000
