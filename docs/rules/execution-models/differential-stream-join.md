# Rule: Differential Incremental Stream Join

**Category:** execution-models
**File:** `rules/execution-models/differential/differential-stream-join.rra`

## Metadata

- **ID:** `differential-stream-join`
- **Version:** 1.0.0
- **Databases:** Materialize, differential-dataflow, Noria
- **Tags:** execution, differential, streaming, join, incremental, delta
- **SQL Standard:** differential-dataflow
- **Authors:** Frank McSherry


# Differential Incremental Stream Join

## Description

Incremental stream join maintains the state of both inputs as arrangements and processes changes using delta rules. When a change arrives on one input, it probes the arrangement of the other input and emits the resulting join changes. The join output is a changelog that correctly reflects insertions, deletions, and updates in both input relations.

**Delta join for R JOIN S on R.a = S.b:**
- Change on R: `dR JOIN S` -- probe S arrangement by key
- Change on S: `R JOIN dS` -- probe R arrangement by key
- Output: union of both delta results

**Key characteristics:**
- **Two arrangements**: One per input, indexed by join key
- **Symmetric delta**: Changes on either side produce output
- **Retraction propagation**: DELETE on one side retracts matching joins
- **Multiset-correct**: Handles duplicate keys correctly via diff multiplication

## Implementation

```rust
pub struct IncrementalJoin {
    left_arrangement: Arrangement<JoinKey, Row>,
    right_arrangement: Arrangement<JoinKey, Row>,
    join_key_left: ColumnId,
    join_key_right: ColumnId,
}

impl IncrementalJoin {
    pub fn process_left_change(
        &mut self,
        change: Change,
    ) -> Vec<Change> {
        let key = extract_key(&change.data, self.join_key_left);
        let mut output = Vec::new();

        // Probe right arrangement
        for (right_val, right_diff) in
            self.right_arrangement.lookup(&key, &change.time)
        {
            let combined = join_rows(&change.data, &right_val);
            let combined_diff = change.diff * right_diff;
            output.push(Change {
                data: combined,
                time: change.time.clone(),
                diff: combined_diff,
            });
        }

        // Update left arrangement
        self.left_arrangement.append(vec![(
            key, change.data, change.time, change.diff,
        )]);

        output
    }

    pub fn process_right_change(
        &mut self,
        change: Change,
    ) -> Vec<Change> {
        let key = extract_key(&change.data, self.join_key_right);
        let mut output = Vec::new();

        // Probe left arrangement
        for (left_val, left_diff) in
            self.left_arrangement.lookup(&key, &change.time)
        {
            let combined = join_rows(&left_val, &change.data);
            let combined_diff = left_diff * change.diff;
            output.push(Change {
                data: combined,
                time: change.time.clone(),
                diff: combined_diff,
            });
        }

        // Update right arrangement
        self.right_arrangement.append(vec![(
            key, change.data, change.time, change.diff,
        )]);

        output
    }
}

/// Diff multiplication for multiset correctness
/// If R has 3 copies of key K, and S gets +1 for key K:
///   output: 3 new join results (diff = 3 * 1 = 3)
/// If R retracts 1 copy (diff=-1), and S has 2 copies:
///   output: retract 2 join results (diff = -1 * 2 = -2)
```

## Cost Model

**Per-Change:** O(log A + M) where A = arrangement size, M = matches
**State:** Two arrangements, O(|R| + |S|) memory
**Output amplification:** Proportional to matches per key
**vs. Full recomputation:** O(|dR| x matches) vs O(|R| x |S|)

## Test Cases

```sql
-- Test 1: Insert on probe side
CREATE MATERIALIZED VIEW v AS SELECT * FROM R JOIN S ON R.a = S.b;
INSERT INTO R VALUES (1, 'x'); -- Probes S for key=1
-- Output: all matching S rows joined with new R row

-- Test 2: Delete propagation
DELETE FROM S WHERE b = 1;
-- Retracts all join results where S.b = 1
-- Output: (joined_row, time, -1) for each match

-- Test 3: Diff multiplication
-- R has 3 rows with key=5, S gets 1 new row with key=5
-- Output: 3 new join results (3 x 1 = 3)

-- Test 4: Multi-way join (cascading deltas)
-- R JOIN S JOIN T: delta on R probes S, then probes T
-- Each delta rule is itself an incremental join
```

## References

1. **McSherry, Frank et al**. "differential-dataflow." CIDR 2013.
2. **Koch, Christoph et al**. "DBToaster." VLDB 2014. (Delta join rules)
3. **Materialize Documentation**. "Join Execution."
4. **Chirkova, Rada; Yang, Jun**. "Materialized Views." Foundations and Trends in Databases 2012.
