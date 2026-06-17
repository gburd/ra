#!/usr/bin/env bash
# Comprehensive PG-vs-Ra planner comparison.
# Per query: correctness (Ra==PG), EXPLAIN-works (no hang/crash), and median
# Planning Time / Execution Time / shared-buffer hits for both planners.
set -uo pipefail
SUITE="${1:-scripts/ra_suite.txt}"
PGHOST="${PGHOST:-/tmp}"; PGPORT="${PGPORT:-5433}"
PGDATABASE="${PGDATABASE:-tpch}"; PGUSER="${PGUSER:-postgres}"
N="${N:-3}"
export PGHOST PGPORT PGDATABASE PGUSER

run() { timeout 90 env PGOPTIONS="-c ra_planner.enabled=$1" psql -qtA -c "$2" </dev/null 2>&1; }
median() { sort -n | awk '{a[NR]=$1} END{if(NR==0){print "NA"}else if(NR%2){print a[(NR+1)/2]}else{printf "%.3f",(a[NR/2]+a[NR/2+1])/2}}'; }
pick() { grep -m1 "$2" <<<"$1" | awk '{print $3}'; }
pickbuf() { grep -m1 'shared hit=' <<<"$1" | grep -oE 'hit=[0-9]+' | head -1 | cut -d= -f2; }

printf "%-18s | %-4s | %-6s | %8s %8s | %8s %8s | %7s %7s\n" \
  shape corr expl ra_plan pg_plan ra_exec pg_exec ra_buf pg_buf
echo "------------------------------------------------------------------------------------------"

ndiff=0; nhang=0; total=0
while IFS='|' read -r name sql; do
  [ -z "${name:-}" ] && continue
  case "$name" in \#*) continue;; esac
  total=$((total+1))
  case "$name" in *order*|*limit*|window*|tpch-q1|tpch-q3|ordered-set) sc="cat";; *) sc="sort";; esac
  on=$(run on "$sql" | grep -viE '^Time:' | LC_ALL=C $sc | md5)
  off=$(run off "$sql" | grep -viE '^Time:' | LC_ALL=C $sc | md5)
  if [ "$on" = "$off" ]; then corr="OK"; else corr="DIFF"; ndiff=$((ndiff+1)); fi
  timeout 12 env PGOPTIONS='-c ra_planner.enabled=on' psql -qtA -c "EXPLAIN (COSTS OFF) $sql" </dev/null >/tmp/_ex 2>&1
  erc=$?
  if [ $erc -eq 124 ]; then expl="HANG"; nhang=$((nhang+1))
  elif grep -qiE "closed|recovery|lost" /tmp/_ex; then expl="CRASH"
  elif grep -qiE "^ERROR|error:" /tmp/_ex; then expl="ERR"
  else expl="ok"; fi
  rp=""; re=""; rb=""; pp=""; pe=""; pb=""
  for _ in $(seq 1 "$N"); do
    o=$(run on  "EXPLAIN (ANALYZE, BUFFERS, TIMING OFF, SUMMARY ON) $sql")
    f=$(run off "EXPLAIN (ANALYZE, BUFFERS, TIMING OFF, SUMMARY ON) $sql")
    rp="$rp $(pick "$o" 'Planning Time:')"; re="$re $(pick "$o" 'Execution Time:')"; rb="$rb $(pickbuf "$o")"
    pp="$pp $(pick "$f" 'Planning Time:')"; pe="$pe $(pick "$f" 'Execution Time:')"; pb="$pb $(pickbuf "$f")"
  done
  ram=$(echo $rp|tr ' ' '\n'|grep -E '^[0-9]'|median); pam=$(echo $pp|tr ' ' '\n'|grep -E '^[0-9]'|median)
  rem=$(echo $re|tr ' ' '\n'|grep -E '^[0-9]'|median); pem=$(echo $pe|tr ' ' '\n'|grep -E '^[0-9]'|median)
  rbm=$(echo $rb|tr ' ' '\n'|grep -E '^[0-9]'|median); pbm=$(echo $pb|tr ' ' '\n'|grep -E '^[0-9]'|median)
  printf "%-18s | %-4s | %-6s | %8s %8s | %8s %8s | %7s %7s\n" \
    "$name" "$corr" "$expl" "$ram" "$pam" "$rem" "$pem" "$rbm" "$pbm"
done < "$SUITE"
echo "------------------------------------------------------------------------------------------"
echo "TOTAL=$total  DIFF=$ndiff  EXPLAIN_HANG=$nhang  (times ms, buf=shared blocks)"
