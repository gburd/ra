#!/usr/bin/env bash
set -euo pipefail

# Standalone JOB benchmark runner for the nuc FreeBSD server.
# Runs all 113 JOB queries against PostgreSQL and captures timing
# and EXPLAIN ANALYZE output.
#
# Prerequisites:
#   - PostgreSQL running with IMDB data loaded
#   - JOB query files in ~/job-queries/
#
# Usage: ./run_nuc_benchmark.sh [queries_dir] [output_dir]

PSQL="${HOME}/ws/postgres/build/bin/psql"
DB_NAME="imdb"
QUERIES_DIR="${1:-${HOME}/job-queries}"
OUTPUT_DIR="${2:-${HOME}/job-results}"
TIMESTAMP=$(date -u '+%Y%m%d_%H%M%S')
RESULTS_FILE="${OUTPUT_DIR}/baseline_${TIMESTAMP}.md"
PLANS_DIR="${OUTPUT_DIR}/plans"
TIMING_CSV="${OUTPUT_DIR}/timing_${TIMESTAMP}.csv"

if ! "${PSQL}" -d "${DB_NAME}" -c "SELECT 1" > /dev/null 2>&1; then
    echo "Error: Cannot connect to database '${DB_NAME}'." >&2
    echo "Start PostgreSQL or check the database name." >&2
    exit 1
fi

if [ ! -d "${QUERIES_DIR}" ]; then
    echo "Error: Query directory not found: ${QUERIES_DIR}" >&2
    echo "Copy queries: scp -r benchmarks/job/queries/ nuc:~/job-queries/" >&2
    exit 1
fi

query_count=$(find "${QUERIES_DIR}" -maxdepth 1 -name '*.sql' 2>/dev/null | wc -l | tr -d ' ')
if [ "${query_count}" -eq 0 ]; then
    echo "Error: No .sql files in ${QUERIES_DIR}" >&2
    exit 1
fi

mkdir -p "${OUTPUT_DIR}" "${PLANS_DIR}"

echo "JOB Benchmark on nuc"
echo "====================="
echo "Database:    ${DB_NAME}"
echo "Queries:     ${QUERIES_DIR} (${query_count} files)"
echo "Output:      ${OUTPUT_DIR}"
echo "Timestamp:   ${TIMESTAMP}"
echo ""

# Portable millisecond timer
now_ms() {
    python3 -c 'import time; print(int(time.time()*1000))' 2>/dev/null \
        || date +%s000
}

# Initialize results
{
    echo "# JOB PostgreSQL Baseline -- nuc"
    echo ""
    echo "Generated: $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
    echo "Server: $(uname -n) ($(uname -s) $(uname -r))"
    echo ""
    echo "| Query | Exec (ms) | Plan (ms) | Rows | Status |"
    echo "|-------|-----------|-----------|------|--------|"
} > "${RESULTS_FILE}"

echo "query_id,exec_ms,plan_ms,rows,status" > "${TIMING_CSV}"

total_exec=0
total_plan=0
failures=0
tested=0

for query_file in "${QUERIES_DIR}"/*.sql; do
    [ -f "${query_file}" ] || continue
    query_id=$(basename "${query_file}" .sql)
    query_text=$(cat "${query_file}")

    printf "%-8s " "${query_id}"

    # Execute with timing
    exec_start=$(now_ms)
    output=$("${PSQL}" -d "${DB_NAME}" -t -A -c "${query_text}" 2>&1) \
        && exec_ok=true || exec_ok=false
    exec_end=$(now_ms)
    exec_ms=$((exec_end - exec_start))

    if [ "${exec_ok}" = true ]; then
        rows=$(echo "${output}" | wc -l | tr -d ' ')
        total_exec=$((total_exec + exec_ms))
        tested=$((tested + 1))

        # Capture EXPLAIN ANALYZE
        plan_start=$(now_ms)
        "${PSQL}" -d "${DB_NAME}" -t -A \
            -c "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) ${query_text}" \
            > "${PLANS_DIR}/${query_id}.json" 2>/dev/null || true
        plan_end=$(now_ms)
        plan_ms=$((plan_end - plan_start))
        total_plan=$((total_plan + plan_ms))

        printf "exec=%5dms  plan=%5dms  rows=%s\n" \
            "${exec_ms}" "${plan_ms}" "${rows}"

        echo "| ${query_id} | ${exec_ms} | ${plan_ms} | ${rows} | OK |" \
            >> "${RESULTS_FILE}"
        echo "${query_id},${exec_ms},${plan_ms},${rows},OK" \
            >> "${TIMING_CSV}"
    else
        failures=$((failures + 1))
        printf "FAILED\n"
        echo "| ${query_id} | - | - | - | FAIL |" >> "${RESULTS_FILE}"
        echo "${query_id},0,0,0,FAIL" >> "${TIMING_CSV}"
    fi
done

# Append summary
{
    echo ""
    echo "## Summary"
    echo ""
    echo "- Queries tested: ${tested}"
    echo "- Failures: ${failures}"
    echo "- Total execution time: ${total_exec}ms"
    echo "- Total planning time: ${total_plan}ms"
    if [ "${tested}" -gt 0 ]; then
        avg_exec=$((total_exec / tested))
        avg_plan=$((total_plan / tested))
        echo "- Average execution time: ${avg_exec}ms"
        echo "- Average planning time: ${avg_plan}ms"
    fi
} >> "${RESULTS_FILE}"

echo ""
echo "====================="
echo "Benchmark complete"
echo "  Tested:     ${tested}"
echo "  Failures:   ${failures}"
echo "  Total exec: ${total_exec}ms"
echo "  Total plan: ${total_plan}ms"
echo ""
echo "Output:"
echo "  Results:  ${RESULTS_FILE}"
echo "  Timing:   ${TIMING_CSV}"
echo "  Plans:    ${PLANS_DIR}/"

if [ "${failures}" -gt 0 ]; then
    exit 1
fi
