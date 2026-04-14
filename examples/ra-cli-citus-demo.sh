#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# Ra CLI Citus Demo: Distributed Query Processing Optimization Examples
# ============================================================================
#
# Prerequisites:
#   1. Build ra-cli:  cargo build --bin ra-cli
#   2. Start Citus cluster (coordinator + 3 workers):
#        # Start coordinator
#        docker run -d --name citus-coord \
#          -e POSTGRES_USER=citus_demo -e POSTGRES_PASSWORD=citus_demo \
#          -e POSTGRES_DB=citus_demo -p 5432:5432 \
#          citusdata/citus:12.1
#
#        # Start worker nodes
#        docker run -d --name citus-worker1 \
#          -e POSTGRES_USER=citus_demo -e POSTGRES_PASSWORD=citus_demo \
#          -e POSTGRES_DB=citus_demo -p 5433:5432 \
#          citusdata/citus:12.1
#
#        docker run -d --name citus-worker2 \
#          -e POSTGRES_USER=citus_demo -e POSTGRES_PASSWORD=citus_demo \
#          -e POSTGRES_DB=citus_demo -p 5434:5432 \
#          citusdata/citus:12.1
#
#        docker run -d --name citus-worker3 \
#          -e POSTGRES_USER=citus_demo -e POSTGRES_PASSWORD=citus_demo \
#          -e POSTGRES_DB=citus_demo -p 5435:5432 \
#          citusdata/citus:12.1
#
#   3. Configure cluster (run once):
#        ./examples/setup-citus-cluster.sh
#   4. Load distributed schema+data:
#        docker exec -i citus-coord psql -U citus_demo -d citus_demo \
#          < examples/citus-distributed-schema.sql
#   5. Run this script: bash examples/ra-cli-citus-demo.sh
#
# What you'll see:
#   - Ra's optimization of distributed vs single-node queries
#   - How sharding and co-location affect query plans
#   - Cross-node join strategies and data movement optimization
#   - Distributed aggregation and window function handling
#   - Real-world scalable analytics query patterns
# ============================================================================

RA="cargo run --bin ra-cli --"
COORD_DB="postgresql://citus_demo:citus_demo@localhost:5432/citus_demo"
SCHEMA="examples/citus-demo-schema.json"

# Color helpers
BOLD='\033[1m'
CYAN='\033[36m'
GREEN='\033[32m'
YELLOW='\033[33m'
PURPLE='\033[35m'
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

distributed_note() {
    echo -e "${PURPLE}  🌐 $1${RESET}"
}

run() {
    echo -e "${GREEN}  \$ $*${RESET}"
    echo ""
    eval "$@"
    echo ""
}

# ──────────────────────────────────────────────────────────────
# Step 0: Gather distributed metadata from Citus coordinator
# ──────────────────────────────────────────────────────────────
step "0" "Gather distributed schema & statistics from Citus cluster"

note "Connects to Citus coordinator, gathers metadata from distributed"
note "tables across worker nodes, including shard distribution info."
distributed_note "Schema includes co-located tables, reference tables, and"
distributed_note "distributed indexes across the 3-node cluster."
echo ""

run $RA gather-metadata --db "$COORD_DB" -o "$SCHEMA"

note "Metadata includes shard distribution, co-location groups,"
note "and cross-node statistics for distributed query planning."

# ──────────────────────────────────────────────────────────────
# Example 1: Simple distributed filter — single table
# ──────────────────────────────────────────────────────────────
step "1" "Simple filter on distributed table"

note "Query on events table distributed by user_id across 3 worker nodes."
note "Ra recognizes the distributed nature and plans parallel execution."
echo ""

run $RA explain \
  \"SELECT user_id, event_type, event_time \
   FROM events \
   WHERE event_time \>= \'2024-01-01\' AND event_type = \'purchase\'\"

distributed_note "This query will execute in parallel on all 3 worker nodes"
distributed_note "since the filter doesn't restrict to specific user_ids."

# ──────────────────────────────────────────────────────────────
# Example 2: Same query, optimized with distributed stats
# ──────────────────────────────────────────────────────────────
step "2" "Distributed filter — optimized with cluster statistics"

note "Ra uses distributed statistics to estimate cardinalities across"
note "all shards and optimize the execution plan accordingly."
echo ""

run $RA optimize \
  \"SELECT user_id, event_type, event_time \
   FROM events \
   WHERE event_time \>= \'2024-01-01\' AND event_type = \'purchase\'\" \
  --db "$COORD_DB" --stats

distributed_note "Optimizer considers shard-level statistics and data"
distributed_note "distribution to minimize network traffic and processing."

# ──────────────────────────────────────────────────────────────
# Example 3: Co-located join — optimal distributed pattern
# ──────────────────────────────────────────────────────────────
step "3" "Co-located join optimization"

note "Join between events and users tables, both distributed by user_id."
note "Co-location allows local joins on each worker node — no data movement!"
echo ""

run $RA optimize \
  \"SELECT u.email, u.country, e.event_type, e.event_time \
   FROM events e \
   JOIN users u ON e.user_id = u.user_id \
   WHERE u.subscription_tier = \'premium\' \
   AND e.event_time \>= \'2024-01-01\'\" \
  --db "$COORD_DB" --diff colored

distributed_note "Co-located join: each worker processes its local shards"
distributed_note "without network communication between nodes!"

# ──────────────────────────────────────────────────────────────
# Example 4: Reference table join — broadcast optimization
# ──────────────────────────────────────────────────────────────
step "4" "Reference table join with distributed table"

note "Join events (distributed) with products (reference table)."
note "Reference tables are replicated on all nodes for local access."
echo ""

run $RA optimize \
  \"SELECT p.name AS product_name, p.category, \
          COUNT(*) AS purchase_count, \
          SUM((e.properties-\>\>\'amount\')::decimal) AS total_revenue \
   FROM events e \
   JOIN products p ON (e.properties-\>\>\'product_id\')::int = p.product_id \
   WHERE e.event_type = \'purchase\' \
   AND e.event_time \>= \'2024-01-01\' \
   GROUP BY p.product_id, p.name, p.category\" \
  --db "$COORD_DB" --explain-format postgres

distributed_note "Products table is available locally on each worker"
distributed_note "— no cross-node data transfer needed for the join!"

# ──────────────────────────────────────────────────────────────
# Example 5: Distributed aggregation across nodes
# ──────────────────────────────────────────────────────────────
step "5" "Multi-level distributed aggregation"

note "Complex aggregation requiring coordination between worker nodes"
note "and final aggregation on the coordinator."
echo ""

run $RA optimize \
  \"SELECT u.country, u.subscription_tier, \
          COUNT(DISTINCT e.user_id) AS unique_users, \
          COUNT(*) AS total_events, \
          AVG(CASE WHEN e.event_type = \'purchase\' \
              THEN (e.properties-\>\>\'amount\')::decimal \
              ELSE 0 END) AS avg_purchase_amount \
   FROM events e \
   JOIN users u ON e.user_id = u.user_id \
   WHERE e.event_time \>= \'2024-01-01\' \
   GROUP BY u.country, u.subscription_tier \
   ORDER BY total_events DESC\" \
  --db "$COORD_DB" --stats

distributed_note "Partial aggregation on each worker, then coordinator"
distributed_note "combines results for final GROUP BY computation."

# ──────────────────────────────────────────────────────────────
# Example 6: Cross-shard query requiring repartitioning
# ──────────────────────────────────────────────────────────────
step "6" "Cross-shard join requiring data repartitioning"

note "Join between tables distributed on different columns requires"
note "data movement and repartitioning across the cluster."
echo ""

run $RA optimize \
  \"SELECT s.session_id, s.start_time, \
          COUNT(e.event_id) AS events_in_session, \
          ARRAY_AGG(e.event_type ORDER BY e.event_time) AS event_sequence \
   FROM user_sessions s \
   JOIN events e ON s.session_id = e.session_id \
   WHERE s.start_time \>= \'2024-01-01\' \
   GROUP BY s.session_id, s.start_time \
   HAVING COUNT(e.event_id) \> 5\" \
  --db "$COORD_DB" --diff side-by-side

distributed_note "sessions distributed by session_id, events by user_id"
distributed_note "Requires shuffle/repartition step — costly operation!"

# ──────────────────────────────────────────────────────────────
# Example 7: Distributed window functions
# ──────────────────────────────────────────────────────────────
step "7" "Window functions in distributed environment"

note "Window functions with PARTITION BY matching distribution key"
note "can execute efficiently within each worker node."
echo ""

run $RA optimize \
  \"SELECT user_id, event_time, event_type, \
          ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY event_time) AS event_sequence, \
          LAG(event_time) OVER (PARTITION BY user_id ORDER BY event_time) AS prev_event_time \
   FROM events \
   WHERE event_time \>= \'2024-01-01\' \
   AND user_id BETWEEN 1000 AND 2000\" \
  --db "$COORD_DB"

distributed_note "PARTITION BY user_id aligns with distribution key"
distributed_note "— window computation stays local to each worker!"

# ──────────────────────────────────────────────────────────────
# Example 8: Subquery decorrelation in distributed context
# ──────────────────────────────────────────────────────────────
step "8" "Distributed EXISTS subquery optimization"

note "Correlated EXISTS subquery across distributed tables."
note "Ra converts to efficient semi-join with co-located execution."
echo ""

run $RA optimize \
  \"SELECT u.user_id, u.email, u.country \
   FROM users u \
   WHERE EXISTS ( \
     SELECT 1 FROM events e \
     WHERE e.user_id = u.user_id \
     AND e.event_type = \'purchase\' \
     AND e.event_time \>= \'2024-01-01\' \
     AND (e.properties-\>\>\'amount\')::decimal \> 100 \
   )\" \
  --db "$COORD_DB" --diff colored

distributed_note "Co-located semi-join executes locally on each worker"
distributed_note "without coordinator involvement for EXISTS check."

# ──────────────────────────────────────────────────────────────
# Example 9: Real-time analytics with distributed CTEs
# ──────────────────────────────────────────────────────────────
step "9" "Complex CTE-based analytics across distributed tables"

note "Multi-stage analytics query with CTEs, testing distributed"
note "intermediate result handling and optimization."
echo ""

run $RA optimize \
  \"WITH user_activity AS ( \
     SELECT u.user_id, u.country, u.subscription_tier, \
            COUNT(e.event_id) AS total_events, \
            COUNT(DISTINCT DATE(e.event_time)) AS active_days, \
            MAX(e.event_time) AS last_activity \
     FROM users u \
     JOIN events e ON u.user_id = e.user_id \
     WHERE e.event_time \>= \'2024-01-01\' \
     GROUP BY u.user_id, u.country, u.subscription_tier \
   ), \
   purchase_behavior AS ( \
     SELECT e.user_id, \
            COUNT(*) AS purchase_count, \
            SUM((e.properties-\>\>\'amount\')::decimal) AS total_spent \
     FROM events e \
     WHERE e.event_type = \'purchase\' \
     AND e.event_time \>= \'2024-01-01\' \
     GROUP BY e.user_id \
   ) \
   SELECT ua.country, ua.subscription_tier, \
          COUNT(*) AS user_count, \
          AVG(ua.total_events) AS avg_events, \
          AVG(ua.active_days) AS avg_active_days, \
          AVG(COALESCE(pb.total_spent, 0)) AS avg_spent \
   FROM user_activity ua \
   LEFT JOIN purchase_behavior pb ON ua.user_id = pb.user_id \
   GROUP BY ua.country, ua.subscription_tier \
   ORDER BY avg_spent DESC\" \
  --db "$COORD_DB" --stats

distributed_note "CTEs execute distributed on workers, final aggregation"
distributed_note "on coordinator — multi-stage distributed execution!"

# ──────────────────────────────────────────────────────────────
# Example 10: Compare Ra vs Citus native planning
# ──────────────────────────────────────────────────────────────
step "10" "Ra distributed optimization vs Citus native planner"

note "Compare Ra's distributed query plan against Citus's native"
note "distributed execution plan for the same complex query."
echo ""

run $RA compare \
  --sql \"SELECT u.country, \
               COUNT(DISTINCT u.user_id) AS users, \
               COUNT(e.event_id) AS events, \
               COUNT(DISTINCT e.session_id) AS sessions, \
               SUM(CASE WHEN e.event_type = \'purchase\' \
                   THEN (e.properties-\>\>\'amount\')::decimal \
                   ELSE 0 END) AS revenue \
        FROM users u \
        JOIN events e ON u.user_id = e.user_id \
        WHERE e.event_time \>= \'2024-01-01\' \
        GROUP BY u.country \
        HAVING COUNT(DISTINCT u.user_id) \> 100 \
        ORDER BY revenue DESC \
        LIMIT 10\" \
  --db "$COORD_DB"

distributed_note "Ra's distributed optimizer vs Citus native planner"
distributed_note "— see how advanced rule-based optimization improves"
distributed_note "distributed query execution plans!"

# ──────────────────────────────────────────────────────────────
# Example 11: Resource-budgeted distributed optimization
# ──────────────────────────────────────────────────────────────
step "11" "Resource-budgeted optimization for large distributed query"

note "Complex distributed query with resource budget constraints"
note "to prevent excessive optimization time in production environments."
echo ""

run $RA optimize \
  \"SELECT p.category, \
          u.country, \
          DATE_TRUNC(\'month\', e.event_time) AS month, \
          COUNT(DISTINCT e.user_id) AS unique_buyers, \
          COUNT(*) AS total_purchases, \
          SUM((e.properties-\>\>\'amount\')::decimal) AS revenue, \
          AVG((e.properties-\>\>\'amount\')::decimal) AS avg_order_value, \
          COUNT(DISTINCT e.session_id) AS unique_sessions \
   FROM events e \
   JOIN users u ON e.user_id = u.user_id \
   JOIN products p ON (e.properties-\>\>\'product_id\')::int = p.product_id \
   WHERE e.event_type = \'purchase\' \
   AND e.event_time \>= \'2023-01-01\' \
   AND u.subscription_tier IN (\'premium\', \'enterprise\') \
   GROUP BY p.category, u.country, DATE_TRUNC(\'month\', e.event_time) \
   HAVING SUM((e.properties-\>\>\'amount\')::decimal) \> 1000 \
   ORDER BY revenue DESC \
   LIMIT 100\" \
  --db "$COORD_DB" \
  --resource-budget standard \
  --rules-all \
  --diff colored

distributed_note "Resource budget prevents optimization timeout while"
distributed_note "still achieving significant distributed query improvements."

echo ""
echo -e "${BOLD}${CYAN}═══════════════════════════════════════════════════════════════${RESET}"
echo -e "${BOLD}${CYAN}  Distributed Query Optimization Demo Complete!${RESET}"
echo -e "${BOLD}${CYAN}═══════════════════════════════════════════════════════════════${RESET}"
echo ""
echo "  Key distributed patterns demonstrated:"
echo "  • Co-located joins (optimal - no data movement)"
echo "  • Reference table joins (broadcast pattern)"
echo "  • Cross-shard joins (requires repartitioning)"
echo "  • Distributed aggregation (multi-level)"
echo "  • Window functions (partition alignment)"
echo "  • Subquery decorrelation (distributed semi-joins)"
echo ""
echo "  Cleanup: "
echo "    docker stop citus-coord citus-worker1 citus-worker2 citus-worker3"
echo "    docker rm citus-coord citus-worker1 citus-worker2 citus-worker3"
echo ""