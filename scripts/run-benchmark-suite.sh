#!/usr/bin/env bash
# Ra vs PostgreSQL end-to-end benchmark suite.
#
# Runs the complete benchmark pipeline:
#   1. Offline corpus benchmark (Ra optimizer only, no Postgres required)
#   2. JOB (Join Order Benchmark) — join ordering quality
#   3. TPROC-C (OLTP) — short transaction performance
#   4. TPC-H OLAP queries (if Postgres baseline provided)
#   5. Analysis report generation
#
# Usage:
#   # Offline only (no Postgres required):
#   ./scripts/run-benchmark-suite.sh
#
#   # With Postgres baseline:
#   RA_DB=postgres://localhost/bench \
#   STD_DB=postgres://localhost:5433/bench \
#   ./scripts/run-benchmark-suite.sh
#
# Output:
#   results/              — per-suite BenchmarkReport JSON files
#   reports/              — executive Markdown + JSON summary
#
# Environment variables:
#   RA_DB         Postgres connection string for Ra-enabled instance
#   STD_DB        Postgres connection string for standard instance (baseline)
#   JOB_REPS      Repetitions per JOB query        (default: 10)
#   OLTP_REPS     Repetitions per OLTP query        (default: 10)
#   CORPUS_FUZZ   Extra fuzz queries in corpus mode (default: 200)
#   RESULTS_DIR   Output directory for results      (default: results/)
#   REPORTS_DIR   Output directory for reports      (default: reports/)
#   CARGO_FLAGS   Extra flags for cargo run         (default: --release)

set -euo pipefail

RA_DB="${RA_DB:-}"
STD_DB="${STD_DB:-}"
JOB_REPS="${JOB_REPS:-10}"
OLTP_REPS="${OLTP_REPS:-10}"
CORPUS_FUZZ="${CORPUS_FUZZ:-200}"
RESULTS_DIR="${RESULTS_DIR:-results}"
REPORTS_DIR="${REPORTS_DIR:-reports}"
CARGO_FLAGS="${CARGO_FLAGS:---release}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TS="$(date +%Y%m%d_%H%M%S)"

log() { echo "[benchmark-suite] $*"; }
run_bench() { cargo run -p ra-bench $CARGO_FLAGS -- "$@"; }

mkdir -p "${RESULTS_DIR}" "${REPORTS_DIR}"

# ---------------------------------------------------------------------------
# Step 1: Offline corpus benchmark (always runs, no Postgres needed)
# ---------------------------------------------------------------------------
log "Step 1: Offline corpus benchmark"
run_bench bench \
  --mode both \
  --fuzz-count "${CORPUS_FUZZ}" \
  --output "${RESULTS_DIR}/corpus_${TS}.json" \
  --quiet

log "  → ${RESULTS_DIR}/corpus_${TS}.json"

# ---------------------------------------------------------------------------
# Step 2: JOB (Join Order Benchmark)
# ---------------------------------------------------------------------------
log "Step 2: JOB benchmark (${JOB_REPS} reps per query)"
JOB_ARGS=(benchmark-job
  --repetitions "${JOB_REPS}"
  --output "${RESULTS_DIR}/job_${TS}.json"
)
if [[ -n "${RA_DB}" ]]; then
  JOB_ARGS+=(--db "${RA_DB}")
fi
run_bench "${JOB_ARGS[@]}"
log "  → ${RESULTS_DIR}/job_${TS}.json"

# ---------------------------------------------------------------------------
# Step 3: TPROC-C (OLTP)
# ---------------------------------------------------------------------------
log "Step 3: TPROC-C OLTP benchmark (${OLTP_REPS} reps per query)"
OLTP_ARGS=(benchmark-oltp
  --repetitions "${OLTP_REPS}"
  --output "${RESULTS_DIR}/oltp_${TS}.json"
)
if [[ -n "${RA_DB}" ]]; then
  OLTP_ARGS+=(--db "${RA_DB}")
fi
run_bench "${OLTP_ARGS[@]}"
log "  → ${RESULTS_DIR}/oltp_${TS}.json"

# ---------------------------------------------------------------------------
# Step 4: TPC-H OLAP queries (requires Postgres baselines)
# ---------------------------------------------------------------------------
if [[ -n "${RA_DB}" && -n "${STD_DB}" ]]; then
  log "Step 4: TPC-H OLAP benchmark (Ra vs Standard Postgres)"

  # Collect Ra timings via benchmark harness
  run_bench bench \
    --mode corpus \
    --db "${RA_DB}" \
    --output "${RESULTS_DIR}/tpch_ra_${TS}.json" \
    --quiet

  # Collect Standard Postgres timings
  run_bench bench \
    --mode corpus \
    --db "${STD_DB}" \
    --output "${RESULTS_DIR}/tpch_std_${TS}.json" \
    --quiet

  log "  → ${RESULTS_DIR}/tpch_ra_${TS}.json"
  log "  → ${RESULTS_DIR}/tpch_std_${TS}.json"
else
  log "Step 4: Skipped (set RA_DB and STD_DB for Postgres comparison)"
fi

# ---------------------------------------------------------------------------
# Step 5: Analysis report
# ---------------------------------------------------------------------------
log "Step 5: Generating analysis report"
ANALYZE_INPUTS=()
for f in "${RESULTS_DIR}"/{job,oltp}_${TS}.json; do
  [[ -f "$f" ]] && ANALYZE_INPUTS+=("$f")
done
if [[ -f "${RESULTS_DIR}/tpch_ra_${TS}.json" ]]; then
  ANALYZE_INPUTS+=("${RESULTS_DIR}/tpch_ra_${TS}.json")
fi

if [[ ${#ANALYZE_INPUTS[@]} -gt 0 ]]; then
  run_bench analyze \
    "${ANALYZE_INPUTS[@]}" \
    --output "${REPORTS_DIR}/executive_summary_${TS}.md" \
    --json "${REPORTS_DIR}/executive_summary_${TS}.json"
  log "  → ${REPORTS_DIR}/executive_summary_${TS}.md"
else
  log "  No result files to analyze."
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "============================================================"
echo " BENCHMARK SUITE COMPLETE"
echo "============================================================"
echo ""
echo "Results:   ${RESULTS_DIR}/"
echo "Report:    ${REPORTS_DIR}/executive_summary_${TS}.md"
echo ""

if [[ -f "${REPORTS_DIR}/executive_summary_${TS}.md" ]]; then
  # Print the executive summary section
  sed -n '/^## Executive Summary/,/^## /p' \
    "${REPORTS_DIR}/executive_summary_${TS}.md" | head -20
fi
