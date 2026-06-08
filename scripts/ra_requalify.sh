#!/usr/bin/env bash
# Comprehensive Ra-vs-PG19 requalification.
# For each query: correctness (Ra-on vs Ra-off sorted-hash), whether Ra fell
# back to PG (log_decisions), and planning+exec time. Uses </dev/null on every
# psql (avoids the while-loop stdin-consumption bug) and per-query log scan.
set -uo pipefail
PSQL="psql -h /tmp -p 5433 -U postgres -d tpch -tAq"
LOG=/tmp/pg19_p4.log

q_off() { PGOPTIONS='-c ra_planner.enabled=off' $PSQL -c "$1" </dev/null 2>/dev/null | grep -vE '^Time:' | sort | shasum | cut -d' ' -f1; }
q_on()  { PGOPTIONS='-c ra_planner.enabled=on'  $PSQL -c "$1" </dev/null 2>/dev/null | grep -vE '^Time:' | sort | shasum | cut -d' ' -f1; }
err_on(){ PGOPTIONS='-c ra_planner.enabled=on'  $PSQL -c "$1" </dev/null 2>&1 >/dev/null | grep -ciE 'ERROR|lost|recovery'; }
fellback() { # returns 1 if Ra fell back ON THIS QUERY (not background monitor)
  local before after tag
  # a distinctive token from the query to disambiguate from monitor fallbacks
  tag=$(printf '%s' "$1" | tr 'A-Z' 'a-z' | grep -oE '(orders|customer|lineitem|nation|region|supplier)' | head -1)
  before=$(wc -l < "$LOG")
  PGOPTIONS='-c ra_planner.enabled=on -c ra_planner.log_decisions=on' $PSQL -c "$1" </dev/null >/dev/null 2>&1
  tail -n +$((before+1)) "$LOG" | grep -i 'fell back' | grep -iv 'pg_stat\|pg_catalog\|pg_class\|pg_namespace' \
    | grep -qiE "query: .*${tag}" && echo 1 || echo 0
}

ok=0; diff=0; err=0; fb=0; total=0
while IFS='|' read -r name sql; do
  [ -z "${name:-}" ] && continue
  total=$((total+1))
  e=$(err_on "$sql")
  if [ "$e" -gt 0 ]; then echo "ERR      $name"; err=$((err+1)); continue; fi
  o=$(q_off "$sql"); n=$(q_on "$sql")
  f=$(fellback "$sql")
  if [ "$o" != "$n" ]; then echo "DIFF     $name"; diff=$((diff+1));
  elif [ "$f" = "1" ]; then echo "FALLBACK $name"; fb=$((fb+1));
  else echo "RA-BUILT $name"; ok=$((ok+1)); fi
done < /tmp/ra_suite.txt
echo "==================================================="
echo "TOTAL=$total  RA-BUILT(correct)=$ok  FALLBACK(correct)=$fb  DIFF=$diff  ERR=$err"
