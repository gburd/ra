#!/usr/bin/env bash
#
# ra_compare.sh — Ra vs PostgreSQL native planner A/B harness.
#
# For every shape in a suite file it reports, with the Ra extension toggled
# on vs off against the SAME PostgreSQL instance:
#   * CORRECT  — are the result rows identical (order-independent)?
#   * BUILDS   — did Ra build the plan natively (RA) or fall back to PG (PG)?
#   * RA_ms    — Ra planning time: min "total=" from Ra's own per-query log
#                (parse+optimize+translate), over N warm runs. NA if Ra fell
#                back or no log is available. (EXPLAIN is not used for Ra: its
#                deparse can hang on some Ra-built plans, e.g. covering index
#                scans.)
#   * PG_ms    — PG planning time: min "Planning Time" from EXPLAIN (SUMMARY ON)
#                over N warm runs (measured WITHOUT executing).
#   * WINNER   — whichever planned faster
#
# Note the slight asymmetry: RA_ms includes Ra's parse, PG_ms is PG's planner
# only (PG parses before planning). Ra's parse is ~0.02ms, negligible here.
#
# The first run in each session absorbs the one-time per-backend e-graph cold
# start and is discarded; the min of the rest is taken.
#
# The harness is crash-resilient: if a backend dies (e.g. the known 0x7f7f
# fault) it waits for automatic recovery and records the shape as CRASH rather
# than aborting the whole run.
#
# Usage:
#   scripts/ra_compare.sh [suite_file]
# Environment (all optional):
#   PGHOST (/tmp) PGPORT (5433) PGDATABASE (tpch) PGUSER (postgres)
#   RA_GUC (ra_planner.enabled)  N (warm EXPLAINs per session, default 6)
#   RA_LOG (server log path, for fallback detection; auto-skipped if unset)
set -uo pipefail

SUITE="${1:-scripts/ra_suite.txt}"
PGHOST="${PGHOST:-/tmp}"
PGPORT="${PGPORT:-5433}"
PGDATABASE="${PGDATABASE:-tpch}"
PGUSER="${PGUSER:-postgres}"
RA_GUC="${RA_GUC:-ra_planner.enabled}"
N="${N:-6}"
RA_LOG="${RA_LOG:-/tmp/pg19_p4.log}"
PSQL=(psql -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d "$PGDATABASE" -qtA)
WARM="SELECT 1 AS a UNION ALL SELECT 2"

if [ ! -f "$SUITE" ]; then
  echo "suite file not found: $SUITE" >&2
  exit 1
fi

# Wait (up to ~30s) for the server to accept connections after a crash/recovery.
wait_for_db() {
  local _
  for _ in $(seq 1 15); do
    if "${PSQL[@]}" -c 'SELECT 1' </dev/null >/dev/null 2>&1; then return 0; fi
    sleep 2
  done
  return 1
}

# Order-independent hash of a query's result rows under a given GUC value.
result_hash() { # $1=on|off  $2=sql
  PGOPTIONS="-c ${RA_GUC}=$1" timeout 30 "${PSQL[@]}" -c "$2" </dev/null 2>/dev/null \
    | grep -vE '^Time:' | LC_ALL=C sort | shasum | cut -d' ' -f1
}

# Min "Planning Time" (ms) for PG native, over N warm EXPLAIN (SUMMARY ON) runs
# in one session. Prints empty on failure.
pg_plan_ms() { # $1=sql
  {
    echo "EXPLAIN (SUMMARY ON) $WARM;"
    local _i
    for _i in $(seq 1 "$N"); do echo "EXPLAIN (SUMMARY ON) $1;"; done
  } | PGOPTIONS="-c ${RA_GUC}=off" timeout 30 "${PSQL[@]}" 2>/dev/null \
    | awk '/Planning Time:/{print $3}' | tail -n +2 | LC_ALL=C sort -g | head -1
}

# Min Ra planning time (ms) from Ra's own per-query "total=" log line, over N
# warm runs of the actual query. This avoids EXPLAIN under Ra, whose deparse
# can hang on some Ra-built plans (e.g. covering index scans). Needs RA_LOG.
ra_plan_ms() { # $1=sql
  if [ ! -r "$RA_LOG" ]; then echo ''; return; fi
  local before _i
  before=$(wc -l <"$RA_LOG")
  PGOPTIONS="-c ${RA_GUC}=on" timeout 30 "${PSQL[@]}" -c "$WARM" </dev/null >/dev/null 2>&1
  for _i in $(seq 1 "$N"); do
    PGOPTIONS="-c ${RA_GUC}=on -c ra_planner.log_decisions=on" timeout 30 "${PSQL[@]}" \
      -c "$1" </dev/null >/dev/null 2>&1
  done
  tail -n +$((before + 1)) "$RA_LOG" \
    | sed -nE 's/.*ra_planner: OK .*total=([0-9.]+)ms.*/\1/p' \
    | LC_ALL=C sort -g | head -1
}

# Did Ra fall back to PG for this query? Matches a "fell back ... query:" log
# line whose echoed text starts with this query's first 50 chars. Using a
# fixed prefix (the log truncates the END) and grep -F (no regex escaping)
# makes this robust to long queries; the leading SELECT prefix is unique per
# shape and never matches background monitor/catalog queries. The unattributed
# "inner panic" lines are deliberately NOT used here (they fire from background
# refreshes too). Returns: 1 fallback, 0 native, ? unknown.
fellback() { # $1=sql
  if [ ! -r "$RA_LOG" ]; then echo '?'; return; fi
  local before prefix
  before=$(wc -l <"$RA_LOG")
  PGOPTIONS="-c ${RA_GUC}=on -c ra_planner.log_decisions=on" timeout 30 "${PSQL[@]}" \
    -c "$1" </dev/null >/dev/null 2>&1
  prefix=$(printf '%s' "$1" | cut -c1-50)
  if tail -n +$((before + 1)) "$RA_LOG" | grep -i 'fell back' \
    | grep -qF "$prefix"; then echo 1; else echo 0; fi
}

printf '%-20s %-7s %-6s %10s %10s %7s\n' SHAPE CORRECT BUILDS RA_ms PG_ms WINNER
printf -- '----------------------------------------------------------------\n'

total=0 correct=0 ndiff=0 ncrash=0 nbuilt=0 nfallback=0
ra_wins=0 pg_wins=0 ratio_log=0 ratio_n=0
diff_list="" crash_list=""

while IFS='|' read -r name sql; do
  case "$name" in '' | \#*) continue ;; esac
  total=$((total + 1))

  h_on=$(result_hash on "$sql")
  h_off=$(result_hash off "$sql")
  if ! wait_for_db; then echo "DB down, aborting" >&2; exit 1; fi

  if [ -z "$h_on" ] || [ -z "$h_off" ]; then
    c=CRASH
    ncrash=$((ncrash + 1))
    crash_list="$crash_list $name"
  elif [ "$h_on" = "$h_off" ]; then
    c=OK
    correct=$((correct + 1))
  else
    c=DIFF
    ndiff=$((ndiff + 1))
    diff_list="$diff_list $name"
  fi

  fb=$(fellback "$sql")
  case "$fb" in
    1) b=PG; nfallback=$((nfallback + 1)) ;;
    0) b=RA; nbuilt=$((nbuilt + 1)) ;;
    *) b='?' ;;
  esac

  ra=$(ra_plan_ms "$sql"); ra=${ra:-NA}
  pg=$(pg_plan_ms "$sql"); pg=${pg:-NA}
  wait_for_db || true

  win='-'
  if [ "$ra" != NA ] && [ "$pg" != NA ]; then
    if awk "BEGIN{exit !($ra < $pg)}"; then
      win=RA; ra_wins=$((ra_wins + 1))
    else
      win=PG; pg_wins=$((pg_wins + 1))
    fi
    ratio_log=$(awk "BEGIN{print $ratio_log + log($pg/$ra)}")
    ratio_n=$((ratio_n + 1))
  fi

  printf '%-20s %-7s %-6s %10s %10s %7s\n' "$name" "$c" "$b" "$ra" "$pg" "$win"
done <"$SUITE"

printf -- '----------------------------------------------------------------\n'
geomean=$(awk "BEGIN{ if($ratio_n>0) printf \"%.2f\", exp($ratio_log/$ratio_n); else print \"NA\" }")
echo "Correctness : $correct OK / $ndiff DIFF / $ncrash CRASH  (of $total)"
[ -n "$diff_list" ] && echo "  DIFF :$diff_list"
[ -n "$crash_list" ] && echo "  CRASH:$crash_list"
echo "Coverage    : $nbuilt RA-BUILT / $nfallback FALLBACK"
echo "Planning    : Ra faster on $ra_wins, PG faster on $pg_wins (geomean PG/Ra = ${geomean}x; >1 = Ra faster)"
