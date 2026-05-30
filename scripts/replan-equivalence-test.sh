#!/usr/bin/env bash
# Replan-equivalence property test for the Ra planner_hook extension.
#
# Property: for one fixed query, replanning it many times while varying
# the inputs that drive plan choice (ra_planner.plan_advice and table
# statistics) must change the PLAN but never the RESULT ROWS. Execution
# time may vary wildly; that is expected and not checked.
#
# This is the correctness gate for ra_planner.enabled = on: if Ra ever
# emits a plan that returns different rows than PostgreSQL's native
# planner — for any stats/advice combination — this test fails.
#
# Usage: replan-equivalence-test.sh [PGHOST] [PGPORT] [PGDATABASE] [PGUSER]
set -euo pipefail

PGHOST="${1:-/tmp}"; PGPORT="${2:-5433}"; PGDATABASE="${3:-postgres}"; PGUSER="${4:-postgres}"
PSQL=(psql -h "$PGHOST" -p "$PGPORT" -d "$PGDATABASE" -U "$PGUSER" -X -q -t -A)
q() { "${PSQL[@]}" -c "$1"; }

# Deterministic dataset with an index (so advice can pick seq/bitmap/tid/index).
q "SET ra_planner.enabled=off;
   DROP TABLE IF EXISTS peq;
   CREATE TABLE peq (id int primary key, grp int, payload text);
   INSERT INTO peq SELECT g, g%7, 'row'||g FROM generate_series(1,5000) g;
   ANALYZE peq;" >/dev/null

# The query under test: Ra-translatable (Scan+Filter+Project — no ORDER BY,
# which would introduce a Sort node that defers to the native planner and
# make this test trivial). Multi-row and stable. Output order may vary by
# scan strategy, so the row MULTISET is compared after sorting in the shell.
QUERY="SELECT id, grp, payload FROM peq WHERE id BETWEEN 100 AND 4000 AND grp = 3"

# Ground truth: native planner (sorted for an order-independent compare).
TRUTH="$(q "SET ra_planner.enabled=off; $QUERY" | LC_ALL=C sort)"
TRUTH_MD5="$(printf '%s' "$TRUTH" | md5)"
echo "ground truth: $(printf '%s\n' "$TRUTH" | grep -c . ) rows (md5 $TRUTH_MD5)"

ADVICE=( "" "SEQ_SCAN(peq)" "BITMAP_HEAP_SCAN(peq)" "TID_SCAN(peq)" "INDEX_SCAN(peq peq_pkey)" )
STATS=( "ANALYZE peq;"
        "ALTER TABLE peq ALTER COLUMN grp SET (n_distinct=1); ANALYZE peq;"
        "ALTER TABLE peq ALTER COLUMN grp SET (n_distinct=5000); ANALYZE peq;" )

fail=0; n=0
for s in "${STATS[@]}"; do
  q "SET ra_planner.enabled=off; $s" >/dev/null
  for a in "${ADVICE[@]}"; do
    n=$((n+1))
    out="$(q "SET ra_planner.enabled=on; SET ra_planner.plan_advice='$a'; $QUERY" 2>&1 | LC_ALL=C sort)" || true
    # server crash?
    if ! pg_isready -h "$PGHOST" -p "$PGPORT" >/dev/null 2>&1; then
      echo "FAIL [advice='$a' stats='${s%% *}']: BACKEND CRASH"; fail=1; break 2
    fi
    if [ "$(printf '%s' "$out" | md5)" != "$TRUTH_MD5" ]; then
      echo "FAIL [advice='$a' stats='${s%% *}']: rows differ from native"
      diff <(printf '%s\n' "$TRUTH") <(printf '%s\n' "$out") | head -6
      fail=1
    fi
  done
done

if [ "$fail" = 0 ]; then
  echo "PASS: $n replans (varied advice x stats), all row-identical to native planner"
else
  echo "FAILED: replan-equivalence violated"
fi
exit "$fail"
