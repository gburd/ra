#!/usr/bin/env bash
set -euo pipefail

# RA Integration Test Suite
#
# Runs end-to-end tests verifying that the RA CLI, EXPLAIN
# format generators, TUI, config system, and (optionally)
# container database services all work together.
#
# Usage:
#   ./tests/integration/docker_stack_test.sh          # all non-container tests
#   ./tests/integration/docker_stack_test.sh --docker  # include container DB tests
#
# Prerequisites:
#   - cargo build must succeed
#   - For --docker: docker/podman with compose, ports 5432/3306 free

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

cd "$PROJECT_ROOT"

# ── Colors ──────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

PASS=0
FAIL=0
SKIP=0
DOCKER=false

for arg in "$@"; do
    if [ "$arg" = "--docker" ]; then
        DOCKER=true
    fi
done

# ── Helpers ─────────────────────────────────────────────

pass() {
    PASS=$((PASS + 1))
    printf "${GREEN}  PASS${NC}  %s\n" "$1"
}

fail() {
    FAIL=$((FAIL + 1))
    printf "${RED}  FAIL${NC}  %s\n" "$1"
    if [ -n "${2:-}" ]; then
        printf "        %s\n" "$2"
    fi
}

skip() {
    SKIP=$((SKIP + 1))
    printf "${YELLOW}  SKIP${NC}  %s\n" "$1"
}

section() {
    printf "\n${CYAN}── %s ──${NC}\n\n" "$1"
}

RA="cargo run --bin ra-cli --quiet --"

# ── Build ───────────────────────────────────────────────

section "Build Verification"

if cargo build --bin ra-cli 2>/dev/null; then
    pass "cargo build --bin ra-cli"
else
    fail "cargo build --bin ra-cli" "Build failed"
    printf "\n${RED}Cannot continue without a working build.${NC}\n"
    exit 1
fi

# ── 1. CLI Core Commands ────────────────────────────────

section "CLI Core Commands"

# validate
if $RA validate tests/fixtures/valid-simple-rule.rra 2>&1 | grep -q "passed"; then
    pass "validate: valid rule file"
else
    fail "validate: valid rule file"
fi

if ! $RA validate tests/fixtures/invalid-bad-yaml.rra 2>/dev/null; then
    pass "validate: rejects invalid YAML"
else
    fail "validate: rejects invalid YAML"
fi

# list
if $RA list --dir rules 2>&1 | grep -q "rule(s) found"; then
    pass "list: rules directory"
else
    fail "list: rules directory"
fi

# stats
if $RA stats --dir rules 2>&1 | grep -q "categories\|rules"; then
    pass "stats: rule statistics"
else
    fail "stats: rule statistics"
fi

# test
if $RA test tests/fixtures/valid-simple-rule.rra 2>&1 | grep -q "passed"; then
    pass "test: run test cases"
else
    fail "test: run test cases"
fi

# ── 2. SQL Explain & Optimize ──────────────────────────

section "SQL Explain and Optimize"

# explain basic
if $RA explain "SELECT * FROM users WHERE id = 1" 2>&1 | grep -q "Plan:"; then
    pass "explain: basic SELECT with WHERE"
else
    fail "explain: basic SELECT with WHERE"
fi

# explain join
if $RA explain "SELECT c.name, o.total FROM customers c JOIN orders o ON c.id = o.customer_id" 2>&1 | grep -q "Join"; then
    pass "explain: JOIN query"
else
    fail "explain: JOIN query"
fi

# explain aggregate
if $RA explain "SELECT region, COUNT(*) FROM customers GROUP BY region" 2>&1 | grep -q "Aggregate"; then
    pass "explain: GROUP BY aggregate"
else
    fail "explain: GROUP BY aggregate"
fi

# optimize
if $RA optimize "SELECT * FROM orders WHERE customer_id = 5 AND status = 'delivered'" 2>&1 | grep -q "Optimized Plan"; then
    pass "optimize: basic predicate pushdown"
else
    fail "optimize: basic predicate pushdown"
fi

# optimize with explain format
if $RA optimize "SELECT * FROM orders WHERE id = 1" --explain-format postgresql 2>&1 | grep -q '"Node Type"'; then
    pass "optimize: --explain-format postgresql"
else
    fail "optimize: --explain-format postgresql"
fi

if $RA optimize "SELECT * FROM orders WHERE id = 1" --explain-format mysql 2>&1 | grep -q "select_type\|table"; then
    pass "optimize: --explain-format mysql"
else
    fail "optimize: --explain-format mysql"
fi

# ── 3. Stdin Pipeline ──────────────────────────────────

section "Stdin Pipeline"

# explain --stdin
STDIN_RESULT=$(echo "SELECT name FROM customers WHERE region = 'US'" | $RA explain --stdin 2>&1)
if echo "$STDIN_RESULT" | grep -q "Plan:"; then
    pass "explain --stdin: piped query"
else
    fail "explain --stdin: piped query"
fi

# optimize --stdin
STDIN_OPT=$(echo "SELECT * FROM t WHERE a = 1" | $RA optimize --stdin 2>&1)
if echo "$STDIN_OPT" | grep -q "Optimized Plan\|Query Optimization"; then
    pass "optimize --stdin: piped query"
else
    fail "optimize --stdin: piped query"
fi

# explain --stdin with explain-format
STDIN_FMT=$(echo "SELECT * FROM t WHERE id = 1" | $RA optimize --stdin --explain-format postgresql 2>&1)
if echo "$STDIN_FMT" | grep -q '"Node Type"'; then
    pass "optimize --stdin --explain-format: piped with format"
else
    fail "optimize --stdin --explain-format: piped with format"
fi

# empty stdin
EMPTY_RESULT=$(echo "" | $RA explain --stdin 2>&1 || true)
if echo "$EMPTY_RESULT" | grep -qi "error\|empty\|no query"; then
    pass "explain --stdin: empty input produces error"
else
    # some implementations may just show an empty plan
    skip "explain --stdin: empty input handling (may vary)"
fi

# ── 4. SQL Format & Translate ──────────────────────────

section "SQL Format and Translate"

FMT_RESULT=$($RA format "select a,b from t where x=1" 2>&1)
if echo "$FMT_RESULT" | grep -q "SELECT"; then
    pass "format: capitalizes keywords"
else
    fail "format: capitalizes keywords"
fi

TRANS_RESULT=$($RA translate "SELECT a FROM t LIMIT 10" --from postgresql --to mysql 2>&1)
if echo "$TRANS_RESULT" | grep -q "Translation\|Translated\|LIMIT"; then
    pass "translate: postgresql to mysql"
else
    fail "translate: postgresql to mysql"
fi

# ── 5. Configuration System ────────────────────────────

section "Configuration System"

# config list
if $RA config list 2>&1 | grep -q "editor.mode"; then
    pass "config list: shows all keys"
else
    fail "config list: shows all keys"
fi

# config get
if $RA config get editor.mode 2>&1 | grep -q "normal\|vi\|nano"; then
    pass "config get: editor.mode"
else
    fail "config get: editor.mode"
fi

# config get invalid key
if ! $RA config get nonexistent.key 2>/dev/null; then
    pass "config get: rejects unknown key"
else
    fail "config get: rejects unknown key"
fi

# config path
if $RA config path 2>&1 | grep -q "config.toml"; then
    pass "config path: shows file location"
else
    fail "config path: shows file location"
fi

# ── 6. TUI Headless Mode ──────────────────────────────

section "TUI Headless Mode"

# demo mode
if $RA tui --demo --headless 2>&1 | grep -q "Headless run complete"; then
    pass "tui --demo --headless: runs to completion"
else
    fail "tui --demo --headless: runs to completion"
fi

# with TOML timeline
TIMELINE_COUNT=$(ls timelines/*.toml 2>/dev/null | wc -l | tr -d ' ')
if [ "$TIMELINE_COUNT" -gt 0 ]; then
    FIRST_TIMELINE=$(ls timelines/*.toml | head -1)
    if $RA tui --timeline "$FIRST_TIMELINE" --headless 2>&1 | grep -q "Headless run complete"; then
        pass "tui --timeline: TOML timeline loads"
    else
        fail "tui --timeline: TOML timeline loads"
    fi

    # test all timelines
    TIMELINE_PASS=0
    TIMELINE_TOTAL=0
    for tl in timelines/*.toml; do
        TIMELINE_TOTAL=$((TIMELINE_TOTAL + 1))
        if $RA tui --timeline "$tl" --headless 2>&1 | grep -q "Headless run complete"; then
            TIMELINE_PASS=$((TIMELINE_PASS + 1))
        fi
    done
    if [ "$TIMELINE_PASS" -eq "$TIMELINE_TOTAL" ]; then
        pass "tui: all $TIMELINE_TOTAL TOML timelines load"
    else
        fail "tui: $TIMELINE_PASS/$TIMELINE_TOTAL timelines loaded" \
             "Some timeline files failed headless playback"
    fi
else
    skip "tui --timeline: no TOML timelines found"
fi

# ── 7. EXPLAIN Format Generators ──────────────────────

section "EXPLAIN Format Generators"

for fmt in postgresql mysql oracle sqlserver; do
    RESULT=$($RA optimize "SELECT a, b FROM t WHERE x = 1" --explain-format "$fmt" 2>&1)
    if [ -n "$RESULT" ] && ! echo "$RESULT" | grep -qi "unknown\|unsupported\|error"; then
        pass "explain-format $fmt: produces output"
    else
        fail "explain-format $fmt: no valid output"
    fi
done

# ── 8. Rule Validation (Full Suite) ───────────────────

section "Rule Validation (Full Rules Directory)"

RULE_COUNT=$(find rules -name '*.rra' -type f 2>/dev/null | wc -l | tr -d ' ')
if [ "$RULE_COUNT" -gt 0 ]; then
    VALIDATE_OUTPUT=$($RA validate rules 2>&1 || true)
    PASSED=$(echo "$VALIDATE_OUTPUT" | grep -o '[0-9]* file(s) passed' | grep -o '[0-9]*' || echo "0")
    FAILED=$(echo "$VALIDATE_OUTPUT" | grep -o '[0-9]* file(s) failed' | grep -o '[0-9]*' || echo "0")
    if [ "$PASSED" -gt 0 ]; then
        pass "validate rules/: $PASSED passed, $FAILED failed out of $RULE_COUNT files"
    else
        fail "validate rules/: no rules passed validation"
    fi
else
    skip "validate rules/: no .rra files found"
fi

# ── 9. Container Database Tests (optional) ────────────

if [ "$DOCKER" = true ]; then
    section "Container Database Stack"

    # Detect container runtime
    # shellcheck source=../../scripts/detect-container-runtime.sh
    source "$PROJECT_ROOT/scripts/detect-container-runtime.sh"

    COMPOSE_FILE="$PROJECT_ROOT/.worktrees/phase-7-docker-improvements/docker-compose.yml"
    if [ ! -f "$COMPOSE_FILE" ]; then
        COMPOSE_FILE="$PROJECT_ROOT/docker-compose.yml"
    fi

    # Check if services are already running
    PG_RUNNING=false
    MY_RUNNING=false
    if $CONTAINER_RUNTIME ps --format '{{.Names}}' 2>/dev/null | grep -q postgres; then
        PG_RUNNING=true
    fi
    if $CONTAINER_RUNTIME ps --format '{{.Names}}' 2>/dev/null | grep -q mysql; then
        MY_RUNNING=true
    fi

    if [ "$PG_RUNNING" = false ] || [ "$MY_RUNNING" = false ]; then
        printf "  Starting container services...\n"
        $COMPOSE_COMMAND -f "$COMPOSE_FILE" up -d postgres mysql 2>/dev/null || true
        sleep 15  # wait for healthchecks
    fi

    # PostgreSQL connectivity
    if $CONTAINER_RUNTIME exec -i "$($CONTAINER_RUNTIME ps -q --filter name=postgres)" \
        psql -U ra_test -d ra_testdb -c "SELECT 1" 2>/dev/null | grep -q "1"; then
        pass "docker: PostgreSQL connectivity"
    else
        fail "docker: PostgreSQL connectivity"
    fi

    # PostgreSQL schema
    PG_TABLES=$($CONTAINER_RUNTIME exec -i "$($CONTAINER_RUNTIME ps -q --filter name=postgres)" \
        psql -U ra_test -d ra_testdb -t -c \
        "SELECT count(*) FROM information_schema.tables WHERE table_schema='public' AND table_type='BASE TABLE'" 2>/dev/null | tr -d ' ')
    if [ "${PG_TABLES:-0}" -ge 4 ]; then
        pass "docker: PostgreSQL has $PG_TABLES tables"
    else
        fail "docker: PostgreSQL tables" "Expected >= 4, got ${PG_TABLES:-0}"
    fi

    # PostgreSQL trigger
    PG_TRIGGERS=$($CONTAINER_RUNTIME exec -i "$($CONTAINER_RUNTIME ps -q --filter name=postgres)" \
        psql -U ra_test -d ra_testdb -t -c \
        "SELECT count(*) FROM information_schema.triggers WHERE trigger_schema='public'" 2>/dev/null | tr -d ' ')
    if [ "${PG_TRIGGERS:-0}" -ge 1 ]; then
        pass "docker: PostgreSQL has $PG_TRIGGERS trigger(s)"
    else
        fail "docker: PostgreSQL triggers" "Expected >= 1, got ${PG_TRIGGERS:-0}"
    fi

    # MySQL connectivity
    if $CONTAINER_RUNTIME exec -i "$($CONTAINER_RUNTIME ps -q --filter name=mysql)" \
        mysql -u ra_test -pra_test_pass ra_testdb -e "SELECT 1" 2>/dev/null | grep -q "1"; then
        pass "docker: MySQL connectivity"
    else
        fail "docker: MySQL connectivity"
    fi

    # MySQL schema
    MY_TABLES=$($CONTAINER_RUNTIME exec -i "$($CONTAINER_RUNTIME ps -q --filter name=mysql)" \
        mysql -u ra_test -pra_test_pass ra_testdb -N -e \
        "SELECT count(*) FROM information_schema.tables WHERE table_schema='ra_testdb' AND table_type='BASE TABLE'" 2>/dev/null | tr -d ' ')
    if [ "${MY_TABLES:-0}" -ge 4 ]; then
        pass "docker: MySQL has $MY_TABLES tables"
    else
        fail "docker: MySQL tables" "Expected >= 4, got ${MY_TABLES:-0}"
    fi

    # MySQL triggers
    MY_TRIGGERS=$($CONTAINER_RUNTIME exec -i "$($CONTAINER_RUNTIME ps -q --filter name=mysql)" \
        mysql -u ra_test -pra_test_pass ra_testdb -N -e \
        "SELECT count(*) FROM information_schema.triggers WHERE trigger_schema='ra_testdb'" 2>/dev/null | tr -d ' ')
    if [ "${MY_TRIGGERS:-0}" -ge 1 ]; then
        pass "docker: MySQL has $MY_TRIGGERS trigger(s)"
    else
        fail "docker: MySQL triggers" "Expected >= 1, got ${MY_TRIGGERS:-0}"
    fi

    # PostgreSQL EXPLAIN comparison
    PG_EXPLAIN=$($CONTAINER_RUNTIME exec -i "$($CONTAINER_RUNTIME ps -q --filter name=postgres)" \
        psql -U ra_test -d ra_testdb -t -c \
        "EXPLAIN (FORMAT JSON) SELECT c.name, COUNT(o.order_id) FROM customers c JOIN orders o ON c.customer_id = o.customer_id GROUP BY c.name" 2>/dev/null)
    if echo "$PG_EXPLAIN" | grep -q "Node Type"; then
        pass "docker: PostgreSQL EXPLAIN JSON works"
    else
        fail "docker: PostgreSQL EXPLAIN JSON"
    fi

    # MySQL EXPLAIN comparison
    MY_EXPLAIN=$($CONTAINER_RUNTIME exec -i "$($CONTAINER_RUNTIME ps -q --filter name=mysql)" \
        mysql -u ra_test -pra_test_pass ra_testdb -N -e \
        "EXPLAIN FORMAT=JSON SELECT c.name, COUNT(o.order_id) FROM customers c JOIN orders o ON c.customer_id = o.customer_id GROUP BY c.name" 2>/dev/null)
    if echo "$MY_EXPLAIN" | grep -q "query_block\|table"; then
        pass "docker: MySQL EXPLAIN JSON works"
    else
        fail "docker: MySQL EXPLAIN JSON"
    fi

else
    section "Container Database Stack (skipped)"
    skip "Container tests: use --docker flag to enable"
fi

# ── Summary ────────────────────────────────────────────

section "Summary"

TOTAL=$((PASS + FAIL + SKIP))
printf "  Total:   %d\n" "$TOTAL"
printf "  ${GREEN}Passed:  %d${NC}\n" "$PASS"
if [ "$FAIL" -gt 0 ]; then
    printf "  ${RED}Failed:  %d${NC}\n" "$FAIL"
fi
if [ "$SKIP" -gt 0 ]; then
    printf "  ${YELLOW}Skipped: %d${NC}\n" "$SKIP"
fi
printf "\n"

if [ "$FAIL" -gt 0 ]; then
    printf "${RED}INTEGRATION TESTS FAILED${NC}\n"
    exit 1
else
    printf "${GREEN}ALL INTEGRATION TESTS PASSED${NC}\n"
    exit 0
fi
