# Rule: Single-Partition Optimization (VoltDB)

**Category:** database-specific/voltdb
**File:** `rules/database-specific/voltdb/single-partition-optimization.rra`

## Metadata

- **ID:** `voltdb-single-partition-optimization`
- **Version:** "1.0.0"
- **Databases:** voltdb
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Single-Partition Optimization (VoltDB)

## Metadata
- **Rule ID**: `voltdb-single-partition`
- **Category**: Database-Specific / VoltDB
- **Source**: VoltDB

## Description

VoltDB detects single-partition transactions (all data on one partition) and executes them without distributed coordination, achieving ~100x speedup.

## Relational Algebra

```
// Multi-partition transaction (slow)
BEGIN;
UPDATE accounts SET balance = balance - 100 WHERE id = 1;  // Partition A
UPDATE accounts SET balance = balance + 100 WHERE id = 2;  // Partition B  
COMMIT;

// Single-partition transaction (fast)
BEGIN;
UPDATE accounts SET balance = balance - 100 WHERE id = 1;  // Partition A
UPDATE accounts SET balance = balance + 100 WHERE id = 3;  // Partition A (same partition\!)
COMMIT;
```

## Test Cases

### Test 1: Single-partition stored procedure
```java
@ProcInfo(partitionInfo = "accounts.id: 0")
public class TransferWithinPartition extends VoltProcedure {
    public VoltTable[] run(long account1, long account2, double amount) {
        // Both accounts on same partition
        voltQueueSQL(debit, amount, account1);
        voltQueueSQL(credit, amount, account2);
        return voltExecuteSQL();
    }
}

-- Executes in ~1ms (no coordination)
-- vs multi-partition: ~10-100ms (2PC required)
```

## References
1. **VoltDB Docs**: "Designing for Partitioning"

## Tags
`database-specific`, `voltdb`, `partitioning`, `single-partition`, `distributed`
