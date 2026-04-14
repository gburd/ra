#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# Ra CLI Demo: 10 Progressive Query Optimization Examples
# ============================================================================
#
# Prerequisites:
#   1. Build ra-cli:  cargo build --bin ra-cli
#   2. Start PostgreSQL (podman or docker):
#        podman run -d --name ra-demo-pg \
#          -e POSTGRES_USER=ra_demo -e POSTGRES_PASSWORD=ra_demo \
#          -e POSTGRES_DB=ra_demo -p 5499:5432 \
#          docker.io/postgres:16-alpine
#   3. Load schema+data:
#        podman exec -i ra-demo-pg psql -U ra_demo -d ra_demo \
#          < test-schemas/02-ecommerce-schema.sql
#   4. Run this script: bash examples/ra-cli-demo.sh
#
# What you'll see:
#   - Ra's relational algebra plan format and PostgreSQL EXPLAIN format
#   - How optimization rules transform plans (diff view)
#   - How live database statistics affect planning
#   - Plan comparison between Ra and PostgreSQL
#   - Resource-budgeted optimization with rule tracking
# ============================================================================

RA="cargo run --bin ra-cli --"
DB="postgresql://ra_demo:ra_demo@localhost:5499/ra_demo"
SCHEMA="examples/demo-schema.json"

# Color helpers
BOLD='\033[1m'
CYAN='\033[36m'
GREEN='\033[32m'
YELLOW='\033[33m'
RESET='\033[0m'

step() {
    echo ""
    echo -e "${BOLD}${CYAN}═══════════════════════════════════════════════════════════════${RESET}"
    echo -e "${BOLD}${CYAN}  Example $1: $2${RESET}"
    echo -e "${BOLD}${CYAN}═══════════════════════════════════════════════════════════════${RESET}"
    echo ""
}

note() {
    echo -e "${YELLOW}  ▸ $1${RESET}"
}

run() {
    echo -e "${GREEN}  \$ $*${RESET}"
    echo ""
    eval "$@"
    echo ""
}

# ──────────────────────────────────────────────────────────────
# Step 0: Gather live metadata from PostgreSQL
# ──────────────────────────────────────────────────────────────
step "0" "Gather live schema & statistics from PostgreSQL"

note "Connects to the running PostgreSQL container, queries pg_stats,"
note "pg_class, and information_schema, then writes a JSON snapshot."
note "This JSON file feeds statistics into subsequent optimizations."
echo ""

run $RA gather-metadata --db "$DB" -o "$SCHEMA"

note "The JSON contains table schemas, row counts, column NDVs,"
note "histograms, index info, and foreign keys."

# ──────────────────────────────────────────────────────────────
# Example 1: Simple filter — unoptimized plan
# ──────────────────────────────────────────────────────────────
step "1" "Simple filter — parse SQL to relational algebra"

note "The 'explain' command parses SQL into an unoptimized plan tree."
note "No rewrite rules are applied yet."
echo ""

run $RA explain \
  \"SELECT \* FROM orders WHERE total_amount \> 500\"

# ──────────────────────────────────────────────────────────────
# Example 2: Same query, now optimized
# ──────────────────────────────────────────────────────────────
step "2" "Same filter — optimized with rewrite rules"

note "The optimizer converts the plan into an e-graph, applies 50+"
note "rewrite rules via equality saturation, then extracts the"
note "lowest-cost plan."
echo ""

run $RA optimize \
  \"SELECT \* FROM orders WHERE total_amount \> 500\" \
  --stats

# ──────────────────────────────────────────────────────────────
# Example 3: Diff view — see what changed
# ──────────────────────────────────────────────────────────────
step "3" "Diff view — what did the optimizer change?"

note "The --diff flag shows a structural diff between the original"
note "and optimized plans. Red = removed, green = added."
echo ""

run $RA optimize \
  \"SELECT \* FROM orders WHERE total_amount \> 500\" \
  --diff colored

# ──────────────────────────────────────────────────────────────
# Example 4: PostgreSQL EXPLAIN format
# ──────────────────────────────────────────────────────────────
step "4" "Output in PostgreSQL EXPLAIN format"

note "Ra can render its optimized plan in PostgreSQL's familiar"
note "EXPLAIN format, including cost estimates and row counts."
echo ""

run $RA optimize \
  \"SELECT \* FROM orders WHERE total_amount \> 500\" \
  --explain-format postgres

# ──────────────────────────────────────────────────────────────
# Example 5: Two-table join with live statistics
# ──────────────────────────────────────────────────────────────
step "5" "Two-table join with live PostgreSQL statistics"

note "Using --db connects to PostgreSQL to fetch live statistics"
note "(row counts, NDV, histograms). This affects join strategy"
note "and cardinality estimates."
echo ""

run $RA optimize \
  \"SELECT c.name, o.order_id, o.total_amount \
   FROM customers c \
   JOIN orders o ON c.customer_id = o.customer_id \
   WHERE c.country = \'USA\' AND o.total_amount \> 500\" \
  --db "$DB" --stats

note "Now the same query in PostgreSQL EXPLAIN format:"
echo ""

run $RA optimize \
  \"SELECT c.name, o.order_id, o.total_amount \
   FROM customers c \
   JOIN orders o ON c.customer_id = o.customer_id \
   WHERE c.country = \'USA\' AND o.total_amount \> 500\" \
  --db "$DB" --explain-format postgres

# ──────────────────────────────────────────────────────────────
# Example 6: Compare Ra plan vs PostgreSQL's own plan
# ──────────────────────────────────────────────────────────────
step "6" "Compare Ra's plan against PostgreSQL EXPLAIN"

note "The 'compare' command runs EXPLAIN on the live database and"
note "shows where Ra agrees or disagrees with PostgreSQL's planner."
echo ""

run $RA compare \
  --sql \"SELECT c.name, o.order_id, o.total_amount \
   FROM customers c \
   JOIN orders o ON c.customer_id = o.customer_id \
   WHERE c.country = \'USA\' AND o.total_amount \> 500\" \
  --db "$DB"

# ──────────────────────────────────────────────────────────────
# Example 7: Three-table join with aggregation
# ──────────────────────────────────────────────────────────────
step "7" "Three-table join with aggregation"

note "This query joins customers → orders → order_items, aggregates"
note "revenue per customer, and filters to top spenders. Watch how"
note "predicate pushdown and join reordering transform the plan."
echo ""

run $RA optimize \
  \"SELECT c.name, SUM\(oi.quantity \* oi.unit_price\) AS total_revenue \
   FROM customers c \
   JOIN orders o ON c.customer_id = o.customer_id \
   JOIN order_items oi ON o.order_id = oi.order_id \
   WHERE o.status = \'delivered\' \
   GROUP BY c.name \
   HAVING SUM\(oi.quantity \* oi.unit_price\) \> 1000 \
   ORDER BY total_revenue DESC\" \
  --db "$DB" --diff side-by-side

# ──────────────────────────────────────────────────────────────
# Example 8: Subquery with EXISTS
# ──────────────────────────────────────────────────────────────
step "8" "Subquery decorrelation (EXISTS → semi-join)"

note "Ra rewrites correlated EXISTS subqueries into semi-joins,"
note "which are more efficient for the executor."
echo ""

run $RA optimize \
  \"SELECT c.name, c.email \
   FROM customers c \
   WHERE EXISTS \( \
     SELECT 1 FROM orders o \
     WHERE o.customer_id = c.customer_id \
     AND o.total_amount \> 1000 \
   \)\" \
  --db "$DB" --stats

note "Side-by-side diff showing the subquery → semi-join rewrite:"
echo ""

run $RA optimize \
  \"SELECT c.name, c.email \
   FROM customers c \
   WHERE EXISTS \( \
     SELECT 1 FROM orders o \
     WHERE o.customer_id = c.customer_id \
     AND o.total_amount \> 1000 \
   \)\" \
  --db "$DB" --diff side-by-side

# ──────────────────────────────────────────────────────────────
# Example 9: Window function with complex predicate
# ──────────────────────────────────────────────────────────────
step "9" "Window function with running total"

note "Window functions add complexity. The optimizer must decide"
note "sort ordering, partition strategy, and whether to push"
note "filters above or below the window."
echo ""

run $RA optimize \
  \"SELECT c.name, o.order_date, o.total_amount, \
     SUM\(o.total_amount\) OVER \(PARTITION BY c.customer_id ORDER BY o.order_date\) AS running_total \
   FROM customers c \
   JOIN orders o ON c.customer_id = o.customer_id \
   WHERE c.customer_tier = \'platinum\' \
   ORDER BY c.name, o.order_date\" \
  --db "$DB"

note "PostgreSQL format:"
echo ""

run $RA optimize \
  \"SELECT c.name, o.order_date, o.total_amount, \
     SUM\(o.total_amount\) OVER \(PARTITION BY c.customer_id ORDER BY o.order_date\) AS running_total \
   FROM customers c \
   JOIN orders o ON c.customer_id = o.customer_id \
   WHERE c.customer_tier = \'platinum\' \
   ORDER BY c.name, o.order_date\" \
  --db "$DB" --explain-format postgres

# ──────────────────────────────────────────────────────────────
# Example 10: Resource-budgeted optimization with rule tracking
# ──────────────────────────────────────────────────────────────
step "10" "Resource budget + rule tracking on complex 4-table join"

note "For complex queries, Ra supports resource budgets to bound"
note "optimization time/memory. The --rules-all flag shows which"
note "rules fired, which were evaluated but rejected, and which"
note "are available in the system."
echo ""

run $RA optimize \
  \"SELECT p.name AS product, p.category, \
     COUNT\(DISTINCT o.customer_id\) AS unique_buyers, \
     SUM\(oi.quantity\) AS total_units, \
     SUM\(oi.quantity \* oi.unit_price \* \(1 - oi.discount_percent / 100\)\) AS net_revenue, \
     AVG\(oi.quantity \* oi.unit_price\) AS avg_order_value \
   FROM products p \
   JOIN order_items oi ON p.product_id = oi.product_id \
   JOIN orders o ON oi.order_id = o.order_id \
   JOIN customers c ON o.customer_id = c.customer_id \
   WHERE o.order_date \>= \'2023-06-01\' \
     AND o.status IN \(\'delivered\', \'shipped\'\) \
     AND c.country = \'USA\' \
   GROUP BY p.name, p.category \
   HAVING SUM\(oi.quantity\) \> 2 \
   ORDER BY net_revenue DESC \
   LIMIT 10\" \
  --db "$DB" \
  --resource-budget standard \
  --rules-all \
  --stats \
  --diff colored

echo ""
echo -e "${BOLD}${CYAN}═══════════════════════════════════════════════════════════════${RESET}"
echo -e "${BOLD}${CYAN}  Demo complete!${RESET}"
echo -e "${BOLD}${CYAN}═══════════════════════════════════════════════════════════════${RESET}"
echo ""
echo "  Cleanup: podman stop ra-demo-pg && podman rm ra-demo-pg"
echo ""
