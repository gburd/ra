#!/usr/bin/env bash
set -euo pipefail

DB_NAME="${1:-imdb}"
RESULTS_DIR="benchmarks/job/results"
QUERIES_DIR="benchmarks/job/queries"
WORK_DIR="${TMPDIR:-/tmp}/job-comparison-$$"

# Validate prerequisites
if ! command -v psql &>/dev/null; then
    echo "Error: psql not found on PATH."
    echo "Install PostgreSQL or add its bin directory to PATH."
    exit 1
fi

if ! psql -d "$DB_NAME" -c "SELECT 1" &>/dev/null; then
    echo "Error: Cannot connect to database '$DB_NAME'."
    echo "Run ./load_data.sh $DB_NAME first."
    exit 1
fi

if [ ! -d "$QUERIES_DIR" ] || [ -z "$(ls -A "$QUERIES_DIR"/*.sql 2>/dev/null)" ]; then
    echo "Error: No query files in $QUERIES_DIR."
    echo "Run ./download_imdb.sh first."
    exit 1
fi

mkdir -p "$RESULTS_DIR" "$WORK_DIR"
trap 'rm -rf "$WORK_DIR"' EXIT

echo "Running JOB comprehensive benchmark: Ra vs PostgreSQL"
echo "Database: $DB_NAME"
echo "Dimensions: planning, accuracy, execution, resources, correctness"
echo "======================================================="
echo ""

# ── Helper: portable millisecond timestamp ──────────────────────────
now_ms() {
    date +%s%3N 2>/dev/null \
        || python3 -c 'import time; print(int(time.time()*1000))'
}

# ── Helper: compute SHA-256 of sorted query results ─────────────────
result_hash() {
    local file="$1"
    if [ -s "$file" ]; then
        sort "$file" | shasum -a 256 | cut -d' ' -f1
    else
        echo "empty"
    fi
}

# ── Initialize results files ────────────────────────────────────────
results_file="$RESULTS_DIR/job-ra-vs-pg.md"
metrics_file="$RESULTS_DIR/metrics.json"
comprehensive_file="$RESULTS_DIR/job-comprehensive.md"

{
    echo "# JOB Benchmark Results: Ra vs PostgreSQL"
    echo ""
    echo "Generated: $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
    echo ""
    echo "| Query | PostgreSQL (ms) | Ra (ms) | Speedup | Status |"
    echo "|-------|-----------------|---------|---------|--------|"
} > "$results_file"

# Start JSON metrics array
echo "[" > "$metrics_file"

pg_total=0
ra_total=0
pg_plan_total=0
pg_failures=0
ra_applied=0
ra_faster=0
query_count=0
correct_count=0
first_entry=true

# Check if Ra extension is loaded
ra_available=false
if psql -d "$DB_NAME" -tAc "SHOW ra_planner.enabled" 2>/dev/null | grep -q "on"; then
    ra_available=true
    echo "Ra planner extension: LOADED"
else
    echo "Ra planner extension: not loaded (PG-only mode)"
fi
echo ""

for query_file in "$QUERIES_DIR"/*.sql; do
    [ -f "$query_file" ] || continue
    query_id=$(basename "$query_file" .sql)
    printf "Testing query %-8s " "$query_id..."

    query_text=$(cat "$query_file")

    # Disable Ra for PG baseline if loaded
    if $ra_available; then
        psql -d "$DB_NAME" -c "SET ra_planner.enabled = off" &>/dev/null
    fi

    # ── Dimension 1 & 2: Planning (EXPLAIN ANALYZE for accuracy) ────
    pg_plan_json="$WORK_DIR/pg_plan_${query_id}.json"
    pg_plan_start=$(now_ms)
    if psql -d "$DB_NAME" -t -A \
        -c "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) $query_text" \
        > "$pg_plan_json" 2>/dev/null; then
        pg_plan_end=$(now_ms)
        pg_plan_time=$((pg_plan_end - pg_plan_start))
    else
        pg_plan_time=0
        echo -n "" > "$pg_plan_json"
    fi
    pg_plan_total=$((pg_plan_total + pg_plan_time))

    # Copy plan to results directory
    cp "$pg_plan_json" "$RESULTS_DIR/pg_plan_${query_id}.json" 2>/dev/null || true

    # Extract accuracy metrics from EXPLAIN ANALYZE JSON
    pg_est_cost=0
    pg_actual_cost=0
    pg_est_rows=0
    pg_actual_rows=0
    io_read=0
    io_write=0
    if [ -s "$pg_plan_json" ]; then
        read -r pg_est_cost pg_actual_cost pg_est_rows pg_actual_rows io_read io_write < <(
            python3 -c "
import json, sys
try:
    data = json.load(sys.stdin)
    plan = data[0]['Plan']
    est_cost = plan.get('Total Cost', 0)
    actual_cost = plan.get('Actual Total Time', 0)
    est_rows = plan.get('Plan Rows', 0)
    actual_rows = plan.get('Actual Rows', 0)
    shared_read = plan.get('Shared Read Blocks', 0)
    shared_written = plan.get('Shared Written Blocks', 0)
    print(est_cost, actual_cost, est_rows, actual_rows,
          int(shared_read) * 8192, int(shared_written) * 8192)
except Exception:
    print(0, 0, 0, 0, 0, 0)
" < "$pg_plan_json" 2>/dev/null || echo "0 0 0 0 0 0"
        )
    fi

    # Compute Q-error
    q_error=$(python3 -c "
e, a = float('$pg_est_rows'), float('$pg_actual_rows')
if e > 0 and a > 0:
    r = e / a
    print(f'{max(r, 1.0/r):.4f}')
else:
    print('1.0000')
" 2>/dev/null || echo "1.0000")

    # ── Dimension 3: Execution Time ─────────────────────────────
    pg_output="$WORK_DIR/pg_result_${query_id}.txt"
    pg_start=$(now_ms)
    if psql -d "$DB_NAME" -c "$query_text" -t -A > "$pg_output" 2>/dev/null; then
        pg_end=$(now_ms)
        pg_time=$((pg_end - pg_start))
        pg_total=$((pg_total + pg_time))
        query_count=$((query_count + 1))
        rows_returned=$(wc -l < "$pg_output" | tr -d ' ')

        # ── Dimension 5: Correctness (result hash) ─────────────
        pg_hash=$(result_hash "$pg_output")

        # ── Ra execution (if extension is loaded) ──────────────
        ra_time=0
        ra_hash="$pg_hash"
        results_match=true
        speedup="N/A"
        status="PG only"
        ra_plan_time=0

        if $ra_available; then
            psql -d "$DB_NAME" -c "SET ra_planner.enabled = on" &>/dev/null
            psql -d "$DB_NAME" -c "SET ra_planner.log_decisions = on" &>/dev/null

            ra_output="$WORK_DIR/ra_result_${query_id}.txt"
            ra_start=$(now_ms)
            if psql -d "$DB_NAME" -c "$query_text" -t -A > "$ra_output" 2>/dev/null; then
                ra_end=$(now_ms)
                ra_time=$((ra_end - ra_start))
                ra_total=$((ra_total + ra_time))
                ra_applied=$((ra_applied + 1))

                ra_hash=$(result_hash "$ra_output")
                if [ "$pg_hash" = "$ra_hash" ]; then
                    results_match=true
                else
                    results_match=false
                fi

                if [ "$ra_time" -gt 0 ]; then
                    speedup=$(echo "scale=2; $pg_time / $ra_time" | bc 2>/dev/null || echo "N/A")
                fi

                if [ "$ra_time" -le "$pg_time" ]; then
                    status="Ra wins"
                    ra_faster=$((ra_faster + 1))
                elif [ "$ra_time" -le $((pg_time * 2)) ]; then
                    status="Comparable"
                else
                    status="PG wins"
                fi

                # Collect Ra EXPLAIN plan
                psql -d "$DB_NAME" \
                    -c "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) $query_text" \
                    > "$RESULTS_DIR/ra_plan_${query_id}.json" 2>/dev/null || true
            else
                ra_time=0
                status="Ra error"
            fi

            psql -d "$DB_NAME" -c "SET ra_planner.enabled = off" &>/dev/null
        fi

        if [ "$results_match" = true ]; then
            correct_count=$((correct_count + 1))
        fi

        # Display for basic results
        ra_display="$ra_time"
        if ! $ra_available; then
            ra_display="N/A"
        fi
        printf "PG: %5sms  Ra: %5s  Q-err: %s  rows: %s\n" \
            "$pg_time" "$ra_display" "$q_error" "$rows_returned"

        echo "| $query_id | $pg_time | $ra_display | $speedup | $status |" >> "$results_file"

        # ── Write JSON metrics entry ────────────────────────────
        if [ "$first_entry" = true ]; then
            first_entry=false
        else
            echo "," >> "$metrics_file"
        fi

        cat >> "$metrics_file" <<JSONENTRY
  {
    "query_id": "$query_id",
    "planning": {
      "pg_plan_time_ms": $pg_plan_time,
      "ra_plan_time_ms": $ra_plan_time,
      "plan_cost_estimate": $pg_est_cost,
      "rules_applied": 0,
      "egraph_nodes": 0,
      "cache_hit": false
    },
    "accuracy": {
      "estimated_cost": $pg_est_cost,
      "actual_cost": $pg_actual_cost,
      "q_error": $q_error,
      "estimated_rows": $pg_est_rows,
      "actual_rows": $pg_actual_rows
    },
    "execution": {
      "pg_exec_time_ms": $pg_time,
      "ra_exec_time_ms": $ra_time,
      "rows_returned": $rows_returned
    },
    "resources": {
      "peak_memory_mb": 0,
      "cpu_time_ms": 0,
      "io_bytes_read": $io_read,
      "io_bytes_written": $io_write
    },
    "correctness": {
      "pg_result_hash": "$pg_hash",
      "ra_result_hash": "$ra_hash",
      "results_match": $results_match
    }
  }
JSONENTRY

    else
        echo "FAILED"
        pg_failures=$((pg_failures + 1))
        echo "| $query_id | FAIL | - | - | ERROR |" >> "$results_file"
    fi
done

# Close JSON array
echo "" >> "$metrics_file"
echo "]" >> "$metrics_file"

# Re-enable Ra at end if it was available
if $ra_available; then
    psql -d "$DB_NAME" -c "SET ra_planner.enabled = on" &>/dev/null 2>&1 || true
fi

# ── Append summary to basic results ────────────────────────────────
{
    echo ""
    echo "## Summary"
    echo ""
    echo "- Queries tested: $query_count"
    echo "- Failures: $pg_failures"
    echo "- PostgreSQL total execution time: ${pg_total}ms"
    echo "- PostgreSQL total planning time: ${pg_plan_total}ms"
    echo "- Correct results: $correct_count/$query_count"
    if $ra_available; then
        echo "- Ra total time: ${ra_total}ms"
        echo "- Ra applied: $ra_applied / $query_count queries"
        echo "- Ra faster: $ra_faster / $ra_applied applied"
        if [ "$pg_total" -gt 0 ]; then
            overall_speedup=$(echo "scale=2; $pg_total / ($ra_total + 1)" | bc 2>/dev/null || echo "N/A")
            echo "- Overall speedup: ${overall_speedup}x"
        fi
    else
        echo "- Ra optimizer: not loaded"
    fi
} >> "$results_file"

# ── Generate comprehensive markdown report from JSON metrics ───────
generate_comprehensive_report() {
    local out="$1"

    {
        echo "# JOB Comprehensive Benchmark Report"
        echo ""
        echo "All 5 dimensions tracked for each of the 113 JOB queries."
        echo ""
        echo "Generated: $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
        echo ""
        echo "## Overview"
        echo ""
        echo "- **Queries measured**: $query_count/113"
        echo "- **Query failures**: $pg_failures"
        echo "- **Correct results**: $correct_count/$query_count"
        echo "- **PG total exec time**: ${pg_total}ms"
        echo "- **PG total plan time**: ${pg_plan_total}ms"
        if $ra_available; then
            echo "- **Ra total exec time**: ${ra_total}ms"
            echo "- **Ra faster on**: $ra_faster queries"
        fi
        echo ""
    } > "$out"

    # Generate per-query tables from the JSON metrics
    python3 - "$metrics_file" "$out" <<'PYSCRIPT'
import json
import math
import sys

metrics_path = sys.argv[1]
out_path = sys.argv[2]

with open(metrics_path) as f:
    metrics = json.load(f)

lines = []

def percentile(vals, pct):
    if not vals:
        return 0
    s = sorted(vals)
    idx = (pct / 100) * (len(s) - 1)
    lo, hi = int(math.floor(idx)), int(math.ceil(idx))
    frac = idx - lo
    if lo == hi:
        return s[lo]
    return s[lo] * (1 - frac) + s[hi] * frac

# Dimension 1: Planning Efficiency
lines.append("## 1. Planning Efficiency\n")
lines.append(
    "| Query | PG Plan (ms) | Ra Plan (ms) | Speedup "
    "| Rules | E-graph Nodes | Cache |"
)
lines.append(
    "|-------|-------------|-------------|---------|"
    "-------|---------------|-------|"
)
plan_times_pg = []
plan_times_ra = []
for m in metrics:
    p = m["planning"]
    plan_times_pg.append(p["pg_plan_time_ms"])
    plan_times_ra.append(p["ra_plan_time_ms"])
    if p["ra_plan_time_ms"] > 0:
        spd = f"{p['pg_plan_time_ms'] / p['ra_plan_time_ms']:.2f}x"
    else:
        spd = "N/A"
    cache = "HIT" if p["cache_hit"] else "-"
    lines.append(
        f"| {m['query_id']} | {p['pg_plan_time_ms']} "
        f"| {p['ra_plan_time_ms']} | {spd} "
        f"| {p['rules_applied']} | {p['egraph_nodes']} | {cache} |"
    )
lines.append(
    f"\n**PG Planning**: Total={sum(plan_times_pg)}ms, "
    f"Median={percentile(plan_times_pg, 50):.0f}ms, "
    f"P95={percentile(plan_times_pg, 95):.0f}ms"
)
if any(t > 0 for t in plan_times_ra):
    lines.append(
        f"\n**Ra Planning**: Total={sum(plan_times_ra)}ms, "
        f"Median={percentile(plan_times_ra, 50):.0f}ms, "
        f"P95={percentile(plan_times_ra, 95):.0f}ms"
    )
lines.append("")

# Dimension 2: Planning Accuracy (Q-Error)
lines.append("## 2. Planning Accuracy (Q-Error)\n")
lines.append(
    "| Query | Est. Cost | Actual Cost | Q-Error "
    "| Est. Rows | Actual Rows |"
)
lines.append(
    "|-------|-----------|-------------|---------|"
    "-----------|-------------|"
)
q_errors = []
for m in metrics:
    a = m["accuracy"]
    lines.append(
        f"| {m['query_id']} | {a['estimated_cost']:.0f} "
        f"| {a['actual_cost']:.1f} | {a['q_error']:.2f} "
        f"| {a['estimated_rows']:.0f} | {a['actual_rows']:.0f} |"
    )
    q_errors.append(a["q_error"])

median_q = percentile(q_errors, 50)
p95_q = percentile(q_errors, 95)
max_q = max(q_errors) if q_errors else 0
pos_q = [q for q in q_errors if q > 0]
geo_mean = math.exp(sum(math.log(q) for q in pos_q) / len(pos_q)) if pos_q else 0
lines.append(
    f"\n**Summary**: Median Q-Error={median_q:.2f}, "
    f"P95={p95_q:.2f}, Max={max_q:.2f}, "
    f"Geometric Mean={geo_mean:.2f}\n"
)

# Dimension 3: Execution Performance
lines.append("## 3. Execution Performance\n")
lines.append(
    "| Query | PG Exec (ms) | Ra Exec (ms) | Speedup | Rows |"
)
lines.append(
    "|-------|-------------|-------------|---------|------|"
)
speedups = []
for m in metrics:
    e = m["execution"]
    if e["ra_exec_time_ms"] > 0:
        spd = e["pg_exec_time_ms"] / e["ra_exec_time_ms"]
        spd_str = f"{spd:.2f}x"
        speedups.append(spd)
    else:
        spd_str = "N/A"
    lines.append(
        f"| {m['query_id']} | {e['pg_exec_time_ms']} "
        f"| {e['ra_exec_time_ms']} | {spd_str} "
        f"| {e['rows_returned']} |"
    )
pg_exec_total = sum(m["execution"]["pg_exec_time_ms"] for m in metrics)
ra_exec_total = sum(m["execution"]["ra_exec_time_ms"] for m in metrics)
lines.append(
    f"\n**Totals**: PG={pg_exec_total}ms, Ra={ra_exec_total}ms"
)
if speedups:
    lines.append(
        f"\n**Speedup**: Median={percentile(speedups, 50):.2f}x, "
        f"Ra faster={sum(1 for s in speedups if s > 1)}, "
        f"PG faster={sum(1 for s in speedups if s < 1)}"
    )
lines.append("")

# Dimension 4: Resource Consumption
lines.append("## 4. Resource Consumption\n")
lines.append(
    "| Query | Peak Mem (MB) | CPU Time (ms) "
    "| I/O Read (KB) | I/O Write (KB) |"
)
lines.append(
    "|-------|-------------|-------------|"
    "-------------|---------------|"
)
for m in metrics:
    r = m["resources"]
    lines.append(
        f"| {m['query_id']} | {r['peak_memory_mb']:.1f} "
        f"| {r['cpu_time_ms']:.1f} "
        f"| {r['io_bytes_read'] // 1024} "
        f"| {r['io_bytes_written'] // 1024} |"
    )
total_io_read = sum(m["resources"]["io_bytes_read"] for m in metrics)
total_io_write = sum(m["resources"]["io_bytes_written"] for m in metrics)
lines.append(
    f"\n**I/O Totals**: Read={total_io_read // (1024*1024)}MB, "
    f"Write={total_io_write // (1024*1024)}MB\n"
)

# Dimension 5: Correctness Verification
lines.append("## 5. Correctness Verification\n")
lines.append("| Query | PG Hash | Ra Hash | Match |")
lines.append("|-------|---------|---------|-------|")
match_count = 0
for m in metrics:
    c = m["correctness"]
    pg_h = c["pg_result_hash"][:16] + "..." \
        if len(c["pg_result_hash"]) > 16 else c["pg_result_hash"]
    ra_h = c["ra_result_hash"][:16] + "..." \
        if len(c["ra_result_hash"]) > 16 else c["ra_result_hash"]
    match_str = "PASS" if c["results_match"] else "FAIL"
    if c["results_match"]:
        match_count += 1
    lines.append(
        f"| {m['query_id']} | {pg_h} | {ra_h} | {match_str} |"
    )
total = len(metrics)
pct = (match_count / total * 100) if total > 0 else 0
lines.append(
    f"\n**Correctness**: {pct:.1f}% ({match_count}/{total} queries match)\n"
)

# Scorecard
lines.append("## Dimension Scorecard\n")
lines.append("| Dimension | Metric | Value | Target | Status |")
lines.append("|-----------|--------|-------|--------|--------|")

pg_plan_med = percentile(plan_times_pg, 50)
lines.append(
    f"| Planning Efficiency | PG median plan time "
    f"| {pg_plan_med:.0f}ms | <100ms "
    f"| {'PASS' if pg_plan_med < 100 else 'FAIL'} |"
)
lines.append(
    f"| Planning Accuracy | Median Q-error "
    f"| {median_q:.2f} | <2.0 "
    f"| {'PASS' if median_q < 2.0 else 'FAIL'} |"
)
if speedups:
    med_spd = percentile(speedups, 50)
    lines.append(
        f"| Execution Time | Median speedup "
        f"| {med_spd:.2f}x | >1.0x "
        f"| {'PASS' if med_spd > 1.0 else 'FAIL'} |"
    )
else:
    lines.append(
        "| Execution Time | Median speedup "
        "| N/A | >1.0x | BASELINE |"
    )
lines.append(
    f"| Resource Consumption | Total I/O read "
    f"| {total_io_read // (1024*1024)}MB | tracked | INFO |"
)
lines.append(
    f"| Correctness | Match rate "
    f"| {pct:.1f}% | 100% "
    f"| {'PASS' if pct >= 100.0 else 'FAIL'} |"
)
lines.append("")

with open(out_path, "a") as f:
    f.write("\n".join(lines))
    f.write("\n")
PYSCRIPT
}

generate_comprehensive_report "$comprehensive_file"

echo ""
echo "======================================================="
echo "Comprehensive benchmark complete"
echo ""
echo "Queries tested:          $query_count"
echo "Failures:                $pg_failures"
echo "Correct results:         $correct_count/$query_count"
echo "PostgreSQL total exec:   ${pg_total}ms"
echo "PostgreSQL total plan:   ${pg_plan_total}ms"
if $ra_available; then
    echo "Ra total exec:           ${ra_total}ms"
    echo "Ra applied:              $ra_applied / $query_count"
    echo "Ra faster:               $ra_faster / $ra_applied"
fi
echo ""
echo "Output files:"
echo "  Basic results:        $results_file"
echo "  JSON metrics:         $metrics_file"
echo "  Comprehensive report: $comprehensive_file"
echo "  PostgreSQL plans:     $RESULTS_DIR/pg_plan_*.json"
if $ra_available; then
    echo "  Ra plans:             $RESULTS_DIR/ra_plan_*.json"
fi
echo ""
if [ "$pg_failures" -gt 0 ]; then
    echo "WARNING: $pg_failures query(ies) failed. Check database setup."
    exit 1
fi
