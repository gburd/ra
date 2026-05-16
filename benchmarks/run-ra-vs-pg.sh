#!/bin/bash
#
# Ra vs PostgreSQL Full Benchmark Suite
#
# Starts containers, loads TPC-H data, runs queries at multiple complexity
# levels with statistical rigor, and produces a comparison report.
#
# Prerequisites:
#   - podman (or docker) with compose support
#   - psql (PostgreSQL client)
#   - python3 with numpy, scipy (for statistics)
#
# Usage:
#   ./run-ra-vs-pg.sh [--iterations N] [--scale-factor SF] [--skip-setup]
#
# Output:
#   benchmarks/results/<timestamp>/
#     ├── raw_data.jsonl          (per-iteration measurements)
#     ├── summary.json            (aggregated statistics)
#     └── REPORT.md              (human-readable comparison)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RESULTS_DIR="$SCRIPT_DIR/results/$TIMESTAMP"

# Defaults
ITERATIONS=30
SCALE_FACTOR=0.1  # TPC-H SF 0.1 = ~100MB, good for benchmarking
WARMUP_ITERATIONS=5
SKIP_SETUP=false
CONTAINER_RUNTIME="podman"

# Connection strings
PG_PORT=15432
RA_PORT=15433
PG_USER=ra_test
PG_PASS=ra_test
PG_DB=ra_test
PG_URL="postgresql://${PG_USER}:${PG_PASS}@localhost:${PG_PORT}/${PG_DB}"
RA_URL="postgresql://${PG_USER}:${PG_PASS}@localhost:${RA_PORT}/${PG_DB}"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --iterations) ITERATIONS="$2"; shift 2 ;;
        --scale-factor) SCALE_FACTOR="$2"; shift 2 ;;
        --skip-setup) SKIP_SETUP=true; shift ;;
        --docker) CONTAINER_RUNTIME="docker"; shift ;;
        *) echo "Unknown arg: $1"; exit 1 ;;
    esac
done

# Detect container runtime
if ! command -v "$CONTAINER_RUNTIME" &>/dev/null; then
    if command -v docker &>/dev/null; then
        CONTAINER_RUNTIME="docker"
    elif command -v podman &>/dev/null; then
        CONTAINER_RUNTIME="podman"
    else
        echo "ERROR: No container runtime found (docker or podman)"
        exit 1
    fi
fi
COMPOSE="$CONTAINER_RUNTIME compose"

log() { echo "[$(date '+%H:%M:%S')] $*"; }
log_ok() { echo "[$(date '+%H:%M:%S')] OK: $*"; }
log_err() { echo "[$(date '+%H:%M:%S')] ERROR: $*" >&2; }

# ─────────────────────────────────────────────────────────────────────────────
# Phase 1: Start containers
# ─────────────────────────────────────────────────────────────────────────────

start_containers() {
    log "Starting PostgreSQL containers..."
    cd "$PROJECT_ROOT"
    $COMPOSE -f docker-compose.test.yml up -d

    log "Waiting for containers to be healthy..."
    local max_wait=120
    local waited=0
    while ! pg_isready -h localhost -p $PG_PORT -U $PG_USER -q 2>/dev/null; do
        sleep 2
        waited=$((waited + 2))
        if [[ $waited -ge $max_wait ]]; then
            log_err "Timed out waiting for native PG (port $PG_PORT)"
            exit 1
        fi
    done
    log_ok "Native PostgreSQL ready on port $PG_PORT"

    waited=0
    while ! pg_isready -h localhost -p $RA_PORT -U $PG_USER -q 2>/dev/null; do
        sleep 2
        waited=$((waited + 2))
        if [[ $waited -ge $max_wait ]]; then
            log_err "Timed out waiting for Ra PG (port $RA_PORT)"
            exit 1
        fi
    done
    log_ok "Ra-enhanced PostgreSQL ready on port $RA_PORT"
}

# ─────────────────────────────────────────────────────────────────────────────
# Phase 2: Load TPC-H schema and data
# ─────────────────────────────────────────────────────────────────────────────

load_tpch_data() {
    log "Loading TPC-H schema (SF=$SCALE_FACTOR)..."

    # Create TPC-H schema on both instances
    local schema_sql="$SCRIPT_DIR/tpch_schema.sql"

    # Generate schema if not present
    if [[ ! -f "$schema_sql" ]]; then
        cat > "$schema_sql" <<'SCHEMA'
-- TPC-H Schema for benchmark comparison
DROP TABLE IF EXISTS lineitem CASCADE;
DROP TABLE IF EXISTS orders CASCADE;
DROP TABLE IF EXISTS partsupp CASCADE;
DROP TABLE IF EXISTS customer CASCADE;
DROP TABLE IF EXISTS supplier CASCADE;
DROP TABLE IF EXISTS part CASCADE;
DROP TABLE IF EXISTS nation CASCADE;
DROP TABLE IF EXISTS region CASCADE;

CREATE TABLE region (
    r_regionkey INTEGER PRIMARY KEY,
    r_name      CHAR(25) NOT NULL,
    r_comment   VARCHAR(152)
);

CREATE TABLE nation (
    n_nationkey INTEGER PRIMARY KEY,
    n_name      CHAR(25) NOT NULL,
    n_regionkey INTEGER NOT NULL REFERENCES region(r_regionkey),
    n_comment   VARCHAR(152)
);

CREATE TABLE supplier (
    s_suppkey   INTEGER PRIMARY KEY,
    s_name      CHAR(25) NOT NULL,
    s_address   VARCHAR(40) NOT NULL,
    s_nationkey INTEGER NOT NULL REFERENCES nation(n_nationkey),
    s_phone     CHAR(15) NOT NULL,
    s_acctbal   DECIMAL(15,2) NOT NULL,
    s_comment   VARCHAR(101)
);

CREATE TABLE part (
    p_partkey   INTEGER PRIMARY KEY,
    p_name      VARCHAR(55) NOT NULL,
    p_mfgr      CHAR(25) NOT NULL,
    p_brand     CHAR(10) NOT NULL,
    p_type      VARCHAR(25) NOT NULL,
    p_size      INTEGER NOT NULL,
    p_container CHAR(10) NOT NULL,
    p_retailprice DECIMAL(15,2) NOT NULL,
    p_comment   VARCHAR(23)
);

CREATE TABLE partsupp (
    ps_partkey    INTEGER NOT NULL REFERENCES part(p_partkey),
    ps_suppkey    INTEGER NOT NULL REFERENCES supplier(s_suppkey),
    ps_availqty   INTEGER NOT NULL,
    ps_supplycost DECIMAL(15,2) NOT NULL,
    ps_comment    VARCHAR(199),
    PRIMARY KEY (ps_partkey, ps_suppkey)
);

CREATE TABLE customer (
    c_custkey   INTEGER PRIMARY KEY,
    c_name      VARCHAR(25) NOT NULL,
    c_address   VARCHAR(40) NOT NULL,
    c_nationkey INTEGER NOT NULL REFERENCES nation(n_nationkey),
    c_phone     CHAR(15) NOT NULL,
    c_acctbal   DECIMAL(15,2) NOT NULL,
    c_mktsegment CHAR(10) NOT NULL,
    c_comment   VARCHAR(117)
);

CREATE TABLE orders (
    o_orderkey    INTEGER PRIMARY KEY,
    o_custkey     INTEGER NOT NULL REFERENCES customer(c_custkey),
    o_orderstatus CHAR(1) NOT NULL,
    o_totalprice  DECIMAL(15,2) NOT NULL,
    o_orderdate   DATE NOT NULL,
    o_orderpriority CHAR(15) NOT NULL,
    o_clerk       CHAR(15) NOT NULL,
    o_shippriority INTEGER NOT NULL,
    o_comment     VARCHAR(79)
);

CREATE TABLE lineitem (
    l_orderkey    INTEGER NOT NULL REFERENCES orders(o_orderkey),
    l_partkey     INTEGER NOT NULL,
    l_suppkey     INTEGER NOT NULL,
    l_linenumber  INTEGER NOT NULL,
    l_quantity    DECIMAL(15,2) NOT NULL,
    l_extendedprice DECIMAL(15,2) NOT NULL,
    l_discount    DECIMAL(15,2) NOT NULL,
    l_tax         DECIMAL(15,2) NOT NULL,
    l_returnflag  CHAR(1) NOT NULL,
    l_linestatus  CHAR(1) NOT NULL,
    l_shipdate    DATE NOT NULL,
    l_commitdate  DATE NOT NULL,
    l_receiptdate DATE NOT NULL,
    l_shipinstruct CHAR(25) NOT NULL,
    l_shipmode    CHAR(10) NOT NULL,
    l_comment     VARCHAR(44),
    PRIMARY KEY (l_orderkey, l_linenumber),
    FOREIGN KEY (l_partkey, l_suppkey) REFERENCES partsupp(ps_partkey, ps_suppkey)
);

-- Indexes for query performance
CREATE INDEX idx_lineitem_shipdate ON lineitem(l_shipdate);
CREATE INDEX idx_lineitem_orderkey ON lineitem(l_orderkey);
CREATE INDEX idx_orders_custkey ON orders(o_custkey);
CREATE INDEX idx_orders_orderdate ON orders(o_orderdate);
CREATE INDEX idx_customer_nationkey ON customer(c_nationkey);
CREATE INDEX idx_supplier_nationkey ON supplier(s_nationkey);
CREATE INDEX idx_nation_regionkey ON nation(n_regionkey);
CREATE INDEX idx_partsupp_suppkey ON partsupp(ps_suppkey);
SCHEMA
    fi

    # Load schema on both instances
    PGPASSWORD=$PG_PASS psql -h localhost -p $PG_PORT -U $PG_USER -d $PG_DB -f "$schema_sql" -q
    PGPASSWORD=$PG_PASS psql -h localhost -p $RA_PORT -U $PG_USER -d $PG_DB -f "$schema_sql" -q

    # Generate and load data using dbgen if available, otherwise use synthetic
    if command -v dbgen &>/dev/null; then
        log "Using dbgen for TPC-H data (SF=$SCALE_FACTOR)..."
        local tmpdir
        tmpdir=$(mktemp -d)
        (cd "$tmpdir" && dbgen -s "$SCALE_FACTOR" -f)
        for tbl in region nation supplier part partsupp customer orders lineitem; do
            PGPASSWORD=$PG_PASS psql -h localhost -p $PG_PORT -U $PG_USER -d $PG_DB \
                -c "\\COPY $tbl FROM '$tmpdir/$tbl.tbl' WITH (FORMAT csv, DELIMITER '|')" -q
            PGPASSWORD=$PG_PASS psql -h localhost -p $RA_PORT -U $PG_USER -d $PG_DB \
                -c "\\COPY $tbl FROM '$tmpdir/$tbl.tbl' WITH (FORMAT csv, DELIMITER '|')" -q
        done
        rm -rf "$tmpdir"
    else
        log "dbgen not found; generating synthetic TPC-H data..."
        generate_synthetic_data $PG_PORT
        generate_synthetic_data $RA_PORT
    fi

    # Run ANALYZE on both
    PGPASSWORD=$PG_PASS psql -h localhost -p $PG_PORT -U $PG_USER -d $PG_DB -c "ANALYZE" -q
    PGPASSWORD=$PG_PASS psql -h localhost -p $RA_PORT -U $PG_USER -d $PG_DB -c "ANALYZE" -q

    log_ok "TPC-H data loaded on both instances"
}

generate_synthetic_data() {
    local port=$1
    # Generate ~10K rows synthetic TPC-H data using SQL
    PGPASSWORD=$PG_PASS psql -h localhost -p "$port" -U $PG_USER -d $PG_DB -q <<'DATA'
-- Regions (5 rows)
INSERT INTO region SELECT i, 'REGION_' || i, 'comment' FROM generate_series(0,4) i;

-- Nations (25 rows)
INSERT INTO nation SELECT i, 'NATION_' || i, i % 5, 'comment' FROM generate_series(0,24) i;

-- Suppliers (100 rows)
INSERT INTO supplier SELECT i, 'Supplier#' || lpad(i::text, 9, '0'),
    'addr_' || i, i % 25, '00-000-000-0000',
    (random() * 10000)::numeric(15,2), 'comment'
FROM generate_series(1,100) i;

-- Parts (2000 rows)
INSERT INTO part SELECT i, 'Part_' || i, 'Manufacturer#' || (i%5+1),
    'Brand#' || (i%5+1) || (i%5+1), 'TYPE_' || (i%50),
    (random()*50+1)::int, 'SM CASE',
    (random()*2000)::numeric(15,2), 'comment'
FROM generate_series(1,2000) i;

-- PartSupp (8000 rows)
INSERT INTO partsupp SELECT (i-1)/4+1, ((i-1)%100)+1, (random()*9999)::int,
    (random()*1000)::numeric(15,2), 'comment'
FROM generate_series(1,8000) i;

-- Customers (1500 rows)
INSERT INTO customer SELECT i, 'Customer#' || lpad(i::text, 9, '0'),
    'addr_' || i, i % 25, '00-000-000-0000',
    (random() * 10000 - 5000)::numeric(15,2),
    (ARRAY['BUILDING','AUTOMOBILE','MACHINERY','HOUSEHOLD','FURNITURE'])[i%5+1],
    'comment'
FROM generate_series(1,1500) i;

-- Orders (15000 rows)
INSERT INTO orders SELECT i, (random()*1499+1)::int,
    (ARRAY['O','F','P'])[i%3+1],
    (random()*500000)::numeric(15,2),
    DATE '1992-01-01' + (random()*2556)::int,
    (ARRAY['1-URGENT','2-HIGH','3-MEDIUM','4-NOT SPECIFIED','5-LOW'])[i%5+1],
    'Clerk#' || lpad((random()*1000)::int::text, 9, '0'),
    0, 'comment'
FROM generate_series(1,15000) i;

-- Lineitem (60000 rows)
INSERT INTO lineitem
SELECT
    (i-1)/4+1,  -- orderkey (4 items per order)
    (random()*1999+1)::int,  -- partkey
    (random()*99+1)::int,    -- suppkey
    (i-1)%4+1,              -- linenumber
    (random()*50+1)::numeric(15,2),
    (random()*100000)::numeric(15,2),
    (random()*0.1)::numeric(15,2),
    (random()*0.08)::numeric(15,2),
    (ARRAY['N','R','A'])[i%3+1],
    (ARRAY['O','F'])[i%2+1],
    DATE '1992-01-01' + (random()*2556)::int,
    DATE '1992-01-01' + (random()*2556)::int,
    DATE '1992-01-01' + (random()*2556)::int,
    'DELIVER IN PERSON',
    (ARRAY['TRUCK','MAIL','SHIP','AIR','RAIL','REG AIR','FOB'])[i%7+1],
    'comment'
FROM generate_series(1,60000) i;
DATA
}

# ─────────────────────────────────────────────────────────────────────────────
# Phase 3: Define benchmark queries (simple → complex)
# ─────────────────────────────────────────────────────────────────────────────

declare -a QUERY_IDS
declare -a QUERY_SQLS
declare -a QUERY_CATEGORIES

add_query() {
    QUERY_IDS+=("$1")
    QUERY_CATEGORIES+=("$2")
    QUERY_SQLS+=("$3")
}

define_queries() {
    # ── Level 1: Simple scans ──
    add_query "scan_01" "simple" \
        "SELECT COUNT(*) FROM lineitem WHERE l_shipdate >= '1994-01-01'"
    add_query "scan_02" "simple" \
        "SELECT l_returnflag, l_linestatus, COUNT(*) FROM lineitem GROUP BY l_returnflag, l_linestatus"
    add_query "scan_03" "simple" \
        "SELECT COUNT(*) FROM orders WHERE o_orderdate BETWEEN '1995-01-01' AND '1995-03-31'"

    # ── Level 2: Two-table joins ──
    add_query "join2_01" "two_table_join" \
        "SELECT COUNT(*) FROM customer c JOIN orders o ON c.c_custkey = o.o_custkey WHERE o.o_orderdate >= '1995-01-01'"
    add_query "join2_02" "two_table_join" \
        "SELECT COUNT(*) FROM orders o JOIN lineitem l ON o.o_orderkey = l.l_orderkey WHERE l.l_discount > 0.05"
    add_query "join2_03" "two_table_join" \
        "SELECT n.n_name, COUNT(*) FROM nation n JOIN supplier s ON n.n_nationkey = s.s_nationkey GROUP BY n.n_name"

    # ── Level 3: Multi-table joins (3-4 tables) ──
    add_query "join3_01" "multi_join" \
        "SELECT COUNT(*) FROM customer c JOIN orders o ON c.c_custkey = o.o_custkey JOIN lineitem l ON o.o_orderkey = l.l_orderkey WHERE l.l_shipdate >= '1994-01-01'"
    add_query "join3_02" "multi_join" \
        "SELECT n.n_name, COUNT(*), SUM(l.l_extendedprice * (1 - l.l_discount)) as revenue FROM nation n JOIN supplier s ON n.n_nationkey = s.s_nationkey JOIN lineitem l ON s.s_suppkey = l.l_suppkey GROUP BY n.n_name ORDER BY revenue DESC"
    add_query "join3_03" "multi_join" \
        "SELECT r.r_name, COUNT(*) FROM region r JOIN nation n ON r.r_regionkey = n.n_regionkey JOIN customer c ON n.n_nationkey = c.c_nationkey JOIN orders o ON c.c_custkey = o.o_custkey GROUP BY r.r_name"

    # ── Level 4: Star schema joins (5+ tables) ──
    add_query "star_01" "star_join" \
        "SELECT n.n_name, SUM(l.l_extendedprice * (1 - l.l_discount)) as revenue FROM customer c JOIN orders o ON c.c_custkey = o.o_custkey JOIN lineitem l ON l.l_orderkey = o.o_orderkey JOIN supplier s ON l.l_suppkey = s.s_suppkey JOIN nation n ON s.s_nationkey = n.n_nationkey WHERE o.o_orderdate >= '1994-01-01' AND o.o_orderdate < '1995-01-01' GROUP BY n.n_name ORDER BY revenue DESC"
    add_query "star_02" "star_join" \
        "SELECT n.n_name, p.p_type, SUM(l.l_extendedprice * (1 - l.l_discount)) as revenue FROM part p JOIN lineitem l ON p.p_partkey = l.l_partkey JOIN supplier s ON l.l_suppkey = s.s_suppkey JOIN orders o ON l.l_orderkey = o.o_orderkey JOIN customer c ON o.o_custkey = c.c_custkey JOIN nation n ON c.c_nationkey = n.n_nationkey WHERE o.o_orderdate BETWEEN '1995-01-01' AND '1996-12-31' GROUP BY n.n_name, p.p_type ORDER BY revenue DESC LIMIT 20"

    # ── Level 5: Aggregations with HAVING ──
    add_query "agg_01" "aggregation" \
        "SELECT c.c_name, COUNT(o.o_orderkey) as order_count, SUM(o.o_totalprice) as total_spent FROM customer c JOIN orders o ON c.c_custkey = o.o_custkey GROUP BY c.c_name HAVING SUM(o.o_totalprice) > 100000 ORDER BY total_spent DESC LIMIT 20"
    add_query "agg_02" "aggregation" \
        "SELECT o_orderpriority, COUNT(*) as order_count FROM orders WHERE o_orderdate >= '1993-07-01' AND o_orderdate < '1993-10-01' AND EXISTS (SELECT 1 FROM lineitem l WHERE l.l_orderkey = orders.o_orderkey AND l.l_commitdate < l.l_receiptdate) GROUP BY o_orderpriority ORDER BY o_orderpriority"

    # ── Level 6: Correlated subqueries ──
    add_query "corr_01" "correlated_subquery" \
        "SELECT c.c_name, c.c_acctbal FROM customer c WHERE c.c_acctbal > (SELECT AVG(c2.c_acctbal) FROM customer c2 WHERE c2.c_nationkey = c.c_nationkey) ORDER BY c.c_acctbal DESC LIMIT 20"
    add_query "corr_02" "correlated_subquery" \
        "SELECT s.s_name FROM supplier s WHERE s.s_suppkey IN (SELECT ps.ps_suppkey FROM partsupp ps WHERE ps.ps_availqty > (SELECT 0.5 * SUM(l.l_quantity) FROM lineitem l WHERE l.l_partkey = ps.ps_partkey AND l.l_suppkey = ps.ps_suppkey AND l.l_shipdate >= '1994-01-01' AND l.l_shipdate < '1995-01-01')) ORDER BY s.s_name LIMIT 20"

    # ── Level 7: TPC-H representative queries ──
    add_query "tpch_q1" "tpch" \
        "SELECT l_returnflag, l_linestatus, SUM(l_quantity) as sum_qty, SUM(l_extendedprice) as sum_base_price, SUM(l_extendedprice * (1 - l_discount)) as sum_disc_price, SUM(l_extendedprice * (1 - l_discount) * (1 + l_tax)) as sum_charge, AVG(l_quantity) as avg_qty, AVG(l_extendedprice) as avg_price, AVG(l_discount) as avg_disc, COUNT(*) as count_order FROM lineitem WHERE l_shipdate <= DATE '1998-12-01' - INTERVAL '90 days' GROUP BY l_returnflag, l_linestatus ORDER BY l_returnflag, l_linestatus"
    add_query "tpch_q3" "tpch" \
        "SELECT l.l_orderkey, SUM(l.l_extendedprice * (1 - l.l_discount)) as revenue, o.o_orderdate, o.o_shippriority FROM customer c JOIN orders o ON c.c_custkey = o.o_custkey JOIN lineitem l ON l.l_orderkey = o.o_orderkey WHERE c.c_mktsegment = 'BUILDING' AND o.o_orderdate < DATE '1995-03-15' AND l.l_shipdate > DATE '1995-03-15' GROUP BY l.l_orderkey, o.o_orderdate, o.o_shippriority ORDER BY revenue DESC, o.o_orderdate LIMIT 10"
    add_query "tpch_q5" "tpch" \
        "SELECT n.n_name, SUM(l.l_extendedprice * (1 - l.l_discount)) as revenue FROM customer c JOIN orders o ON c.c_custkey = o.o_custkey JOIN lineitem l ON l.l_orderkey = o.o_orderkey JOIN supplier s ON l.l_suppkey = s.s_suppkey AND c.c_nationkey = s.s_nationkey JOIN nation n ON s.s_nationkey = n.n_nationkey JOIN region r ON n.n_regionkey = r.r_regionkey WHERE r.r_name = 'REGION_0' AND o.o_orderdate >= DATE '1994-01-01' AND o.o_orderdate < DATE '1995-01-01' GROUP BY n.n_name ORDER BY revenue DESC"
    add_query "tpch_q10" "tpch" \
        "SELECT c.c_custkey, c.c_name, SUM(l.l_extendedprice * (1 - l.l_discount)) as revenue, c.c_acctbal, n.n_name, c.c_address, c.c_phone, c.c_comment FROM customer c JOIN orders o ON c.c_custkey = o.o_custkey JOIN lineitem l ON l.l_orderkey = o.o_orderkey JOIN nation n ON c.c_nationkey = n.n_nationkey WHERE o.o_orderdate >= DATE '1993-10-01' AND o.o_orderdate < DATE '1994-01-01' AND l.l_returnflag = 'R' GROUP BY c.c_custkey, c.c_name, c.c_acctbal, c.c_phone, n.n_name, c.c_address, c.c_comment ORDER BY revenue DESC LIMIT 20"

    # ── Level 8: Window functions ──
    add_query "win_01" "window" \
        "SELECT c_custkey, c_acctbal, c_nationkey, RANK() OVER (PARTITION BY c_nationkey ORDER BY c_acctbal DESC) as balance_rank, AVG(c_acctbal) OVER (PARTITION BY c_nationkey) as nation_avg FROM customer ORDER BY c_nationkey, balance_rank LIMIT 50"
    add_query "win_02" "window" \
        "SELECT o_custkey, o_orderdate, o_totalprice, SUM(o_totalprice) OVER (PARTITION BY o_custkey ORDER BY o_orderdate ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) as running_total FROM orders WHERE o_orderdate >= '1995-01-01' ORDER BY o_custkey, o_orderdate LIMIT 50"

    log "Defined ${#QUERY_IDS[@]} benchmark queries across 8 complexity levels"
}

# ─────────────────────────────────────────────────────────────────────────────
# Phase 4: Run benchmarks
# ─────────────────────────────────────────────────────────────────────────────

run_benchmarks() {
    mkdir -p "$RESULTS_DIR"
    log "Results: $RESULTS_DIR"
    log "Running $ITERATIONS iterations per query (+ $WARMUP_ITERATIONS warmup)..."

    # Write header
    echo '[]' > "$RESULTS_DIR/raw_data.json"

    local total=${#QUERY_IDS[@]}
    local results_json="["
    local first=true

    for idx in $(seq 0 $((total - 1))); do
        local qid="${QUERY_IDS[$idx]}"
        local cat="${QUERY_CATEGORIES[$idx]}"
        local sql="${QUERY_SQLS[$idx]}"

        log "[$((idx+1))/$total] $qid ($cat)..."

        # Collect PG timings
        local pg_plan_times=()
        local pg_exec_times=()
        local ra_plan_times=()
        local ra_exec_times=()
        local pg_error=""
        local ra_error=""

        # Warmup (discard results)
        for _ in $(seq 1 $WARMUP_ITERATIONS); do
            PGPASSWORD=$PG_PASS psql -h localhost -p $PG_PORT -U $PG_USER -d $PG_DB \
                -c "EXPLAIN (ANALYZE, FORMAT JSON) $sql" -t -q >/dev/null 2>&1 || true
        done

        # Measured iterations — Native PostgreSQL
        for iter in $(seq 1 $ITERATIONS); do
            local result
            result=$(PGPASSWORD=$PG_PASS psql -h localhost -p $PG_PORT -U $PG_USER -d $PG_DB \
                -c "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) $sql" -t -q 2>&1) || { pg_error="execution failed"; break; }

            local plan_t exec_t
            plan_t=$(echo "$result" | python3 -c "import sys,json; d=json.load(sys.stdin)[0]; print(d.get('Planning Time',0))" 2>/dev/null) || plan_t=0
            exec_t=$(echo "$result" | python3 -c "import sys,json; d=json.load(sys.stdin)[0]; print(d.get('Execution Time',0))" 2>/dev/null) || exec_t=0
            pg_plan_times+=("$plan_t")
            pg_exec_times+=("$exec_t")
        done

        # Warmup Ra
        for _ in $(seq 1 $WARMUP_ITERATIONS); do
            PGPASSWORD=$PG_PASS psql -h localhost -p $RA_PORT -U $PG_USER -d $PG_DB \
                -c "EXPLAIN (ANALYZE, FORMAT JSON) $sql" -t -q >/dev/null 2>&1 || true
        done

        # Measured iterations — Ra-enhanced PostgreSQL
        for iter in $(seq 1 $ITERATIONS); do
            local result
            result=$(PGPASSWORD=$PG_PASS psql -h localhost -p $RA_PORT -U $PG_USER -d $PG_DB \
                -c "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) $sql" -t -q 2>&1) || { ra_error="execution failed"; break; }

            local plan_t exec_t
            plan_t=$(echo "$result" | python3 -c "import sys,json; d=json.load(sys.stdin)[0]; print(d.get('Planning Time',0))" 2>/dev/null) || plan_t=0
            exec_t=$(echo "$result" | python3 -c "import sys,json; d=json.load(sys.stdin)[0]; print(d.get('Execution Time',0))" 2>/dev/null) || exec_t=0
            ra_plan_times+=("$plan_t")
            ra_exec_times+=("$exec_t")
        done

        # Emit JSON record
        local pg_plan_arr ra_plan_arr pg_exec_arr ra_exec_arr
        pg_plan_arr=$(printf '%s\n' "${pg_plan_times[@]}" | paste -sd, -)
        pg_exec_arr=$(printf '%s\n' "${pg_exec_times[@]}" | paste -sd, -)
        ra_plan_arr=$(printf '%s\n' "${ra_plan_times[@]}" | paste -sd, -)
        ra_exec_arr=$(printf '%s\n' "${ra_exec_times[@]}" | paste -sd, -)

        if [[ "$first" == "true" ]]; then first=false; else results_json+=","; fi
        results_json+=$(cat <<ENTRY
{
  "query_id": "$qid",
  "category": "$cat",
  "sql": $(python3 -c "import json; print(json.dumps('''$sql'''))"),
  "pg_plan_ms": [$pg_plan_arr],
  "pg_exec_ms": [$pg_exec_arr],
  "ra_plan_ms": [$ra_plan_arr],
  "ra_exec_ms": [$ra_exec_arr],
  "pg_error": $(if [[ -n "$pg_error" ]]; then echo "\"$pg_error\""; else echo "null"; fi),
  "ra_error": $(if [[ -n "$ra_error" ]]; then echo "\"$ra_error\""; else echo "null"; fi)
}
ENTRY
)
        # Progress indicator
        if [[ -z "$pg_error" && -z "$ra_error" ]]; then
            local pg_med ra_med
            pg_med=$(printf '%s\n' "${pg_exec_times[@]}" | sort -n | awk 'NR==int(NR/2)+1{print}')
            ra_med=$(printf '%s\n' "${ra_exec_times[@]}" | sort -n | awk 'NR==int(NR/2)+1{print}')
            log_ok "$qid: PG median=${pg_med}ms, Ra median=${ra_med}ms"
        else
            log_err "$qid: PG=${pg_error:-ok}, Ra=${ra_error:-ok}"
        fi
    done

    results_json+="]"
    echo "$results_json" > "$RESULTS_DIR/raw_data.json"
    log_ok "Raw data written to $RESULTS_DIR/raw_data.json"
}

# ─────────────────────────────────────────────────────────────────────────────
# Phase 5: Statistical analysis and report generation
# ─────────────────────────────────────────────────────────────────────────────

generate_report() {
    log "Generating statistical analysis..."

    python3 <<PYTHON
import json
import sys
from pathlib import Path

results_dir = Path("$RESULTS_DIR")
data = json.loads((results_dir / "raw_data.json").read_text())

import numpy as np
from scipy import stats as sp_stats

summary = {
    "timestamp": "$TIMESTAMP",
    "git_commit": "$(git -C "$PROJECT_ROOT" rev-parse --short HEAD)",
    "iterations": $ITERATIONS,
    "warmup": $WARMUP_ITERATIONS,
    "queries": []
}

report_lines = [
    "# Ra vs PostgreSQL Benchmark Report",
    "",
    f"**Generated**: $TIMESTAMP",
    f"**Git commit**: {summary['git_commit']}",
    f"**Iterations**: {$ITERATIONS} (+ {$WARMUP_ITERATIONS} warmup)",
    f"**Data**: TPC-H SF {$SCALE_FACTOR}",
    "",
    "## Summary",
    "",
    "| Query | Category | PG Plan (ms) | Ra Plan (ms) | PG Exec (ms) | Ra Exec (ms) | Exec Speedup | p-value | Sig? |",
    "|-------|----------|-------------|-------------|-------------|-------------|-------------|---------|------|",
]

total_pg_exec = []
total_ra_exec = []
wins_ra = 0
wins_pg = 0
ties = 0

for entry in data:
    qid = entry["query_id"]
    cat = entry["category"]
    pg_plan = np.array(entry["pg_plan_ms"]) if entry["pg_plan_ms"] else np.array([])
    pg_exec = np.array(entry["pg_exec_ms"]) if entry["pg_exec_ms"] else np.array([])
    ra_plan = np.array(entry["ra_plan_ms"]) if entry["ra_plan_ms"] else np.array([])
    ra_exec = np.array(entry["ra_exec_ms"]) if entry["ra_exec_ms"] else np.array([])

    if len(pg_exec) == 0 or len(ra_exec) == 0:
        report_lines.append(f"| {qid} | {cat} | — | — | — | — | — | — | ERROR |")
        continue

    pg_plan_med = np.median(pg_plan)
    ra_plan_med = np.median(ra_plan)
    pg_exec_med = np.median(pg_exec)
    ra_exec_med = np.median(ra_exec)

    # Speedup: >1 means Ra is faster
    exec_speedup = pg_exec_med / ra_exec_med if ra_exec_med > 0 else float('inf')

    # Welch's t-test (two-sided) on execution times
    if len(pg_exec) > 1 and len(ra_exec) > 1 and np.std(pg_exec) + np.std(ra_exec) > 0:
        t_stat, p_value = sp_stats.ttest_ind(pg_exec, ra_exec, equal_var=False)
    else:
        t_stat, p_value = 0, 1.0

    significant = "YES" if p_value < 0.05 else "no"

    if p_value < 0.05:
        if ra_exec_med < pg_exec_med:
            wins_ra += 1
        elif pg_exec_med < ra_exec_med:
            wins_pg += 1
        else:
            ties += 1
    else:
        ties += 1

    total_pg_exec.extend(pg_exec.tolist())
    total_ra_exec.extend(ra_exec.tolist())

    summary["queries"].append({
        "query_id": qid,
        "category": cat,
        "pg_plan_median_ms": round(pg_plan_med, 3),
        "ra_plan_median_ms": round(ra_plan_med, 3),
        "pg_exec_median_ms": round(pg_exec_med, 3),
        "ra_exec_median_ms": round(ra_exec_med, 3),
        "exec_speedup": round(exec_speedup, 3),
        "p_value": round(p_value, 6),
        "significant": p_value < 0.05,
    })

    report_lines.append(
        f"| {qid} | {cat} | {pg_plan_med:.2f} | {ra_plan_med:.2f} | "
        f"{pg_exec_med:.2f} | {ra_exec_med:.2f} | "
        f"{exec_speedup:.2f}x | {p_value:.4f} | {significant} |"
    )

# Overall statistics
total_pg = np.array(total_pg_exec)
total_ra = np.array(total_ra_exec)

report_lines.extend([
    "",
    "## Overall Statistics",
    "",
    f"- **Queries tested**: {len(data)}",
    f"- **Total PG execution (median sum)**: {sum(e['pg_exec_median_ms'] for e in summary['queries']):.1f} ms",
    f"- **Total Ra execution (median sum)**: {sum(e['ra_exec_median_ms'] for e in summary['queries']):.1f} ms",
    f"- **Ra wins (statistically significant)**: {wins_ra}",
    f"- **PG wins (statistically significant)**: {wins_pg}",
    f"- **Ties / not significant**: {ties}",
    "",
])

if len(total_pg) > 0 and len(total_ra) > 0:
    overall_speedup = np.median(total_pg) / np.median(total_ra) if np.median(total_ra) > 0 else 0
    t_stat, p_value = sp_stats.ttest_ind(total_pg, total_ra, equal_var=False)
    report_lines.extend([
        f"- **Overall median speedup**: {overall_speedup:.3f}x",
        f"- **Overall t-test p-value**: {p_value:.6f}",
        f"- **Statistically significant overall**: {'YES' if p_value < 0.05 else 'no'}",
        "",
    ])
    summary["overall"] = {
        "median_speedup": round(overall_speedup, 3),
        "p_value": round(p_value, 6),
        "ra_wins": wins_ra,
        "pg_wins": wins_pg,
        "ties": ties,
    }

# By category breakdown
report_lines.extend(["## By Category", ""])
categories = {}
for e in summary["queries"]:
    categories.setdefault(e["category"], []).append(e)

report_lines.append("| Category | Queries | Median PG (ms) | Median Ra (ms) | Speedup |")
report_lines.append("|----------|---------|---------------|---------------|---------|")
for cat, entries in sorted(categories.items()):
    pg_meds = [e["pg_exec_median_ms"] for e in entries]
    ra_meds = [e["ra_exec_median_ms"] for e in entries]
    cat_speedup = np.median(pg_meds) / np.median(ra_meds) if np.median(ra_meds) > 0 else 0
    report_lines.append(f"| {cat} | {len(entries)} | {np.median(pg_meds):.2f} | {np.median(ra_meds):.2f} | {cat_speedup:.2f}x |")

report_lines.append("")

# Write outputs
(results_dir / "summary.json").write_text(json.dumps(summary, indent=2))
(results_dir / "REPORT.md").write_text("\n".join(report_lines))

print("\n".join(report_lines[-30:]))
print(f"\nFull report: {results_dir / 'REPORT.md'}")
print(f"Raw data:    {results_dir / 'raw_data.json'}")
print(f"Summary:     {results_dir / 'summary.json'}")
PYTHON
}

# ─────────────────────────────────────────────────────────────────────────────
# Main
# ─────────────────────────────────────────────────────────────────────────────

main() {
    log "═══════════════════════════════════════════════════════════"
    log " Ra vs PostgreSQL Benchmark"
    log " Iterations: $ITERATIONS  Warmup: $WARMUP_ITERATIONS  SF: $SCALE_FACTOR"
    log "═══════════════════════════════════════════════════════════"

    if [[ "$SKIP_SETUP" == "false" ]]; then
        start_containers
        load_tpch_data
    else
        log "Skipping container/data setup (--skip-setup)"
        # Verify connectivity
        if ! pg_isready -h localhost -p $PG_PORT -U $PG_USER -q 2>/dev/null; then
            log_err "Native PG not reachable on port $PG_PORT"
            exit 1
        fi
        if ! pg_isready -h localhost -p $RA_PORT -U $PG_USER -q 2>/dev/null; then
            log_err "Ra PG not reachable on port $RA_PORT"
            exit 1
        fi
    fi

    define_queries
    run_benchmarks
    generate_report

    log "═══════════════════════════════════════════════════════════"
    log " Benchmark complete! Results in: $RESULTS_DIR"
    log "═══════════════════════════════════════════════════════════"
}

main "$@"
