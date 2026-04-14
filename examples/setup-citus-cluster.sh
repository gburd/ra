#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# Citus Cluster Setup Script
# ============================================================================
# This script configures a Citus cluster with 1 coordinator and 3 workers
# Run this once after starting the Docker containers
# ============================================================================

COORD_HOST="localhost"
COORD_PORT="5432"
COORD_USER="citus_demo"
COORD_DB="citus_demo"

WORKER_HOSTS=("localhost" "localhost" "localhost")
WORKER_PORTS=("5433" "5434" "5435")
WORKER_USER="citus_demo"
WORKER_DB="citus_demo"

echo "🏗️  Setting up Citus cluster..."
echo ""

# Function to wait for a database to be ready
wait_for_db() {
    local host=$1
    local port=$2
    local user=$3
    local db=$4

    echo "⏳ Waiting for database $host:$port to be ready..."
    for i in {1..30}; do
        if PGPASSWORD=citus_demo psql -h "$host" -p "$port" -U "$user" -d "$db" -c "SELECT 1" >/dev/null 2>&1; then
            echo "✅ Database $host:$port is ready"
            return 0
        fi
        echo "   Attempt $i/30 - still waiting..."
        sleep 2
    done
    echo "❌ Database $host:$port failed to become ready"
    exit 1
}

# Wait for all databases to be ready
wait_for_db "$COORD_HOST" "$COORD_PORT" "$COORD_USER" "$COORD_DB"
for i in "${!WORKER_HOSTS[@]}"; do
    wait_for_db "${WORKER_HOSTS[$i]}" "${WORKER_PORTS[$i]}" "$WORKER_USER" "$WORKER_DB"
done

echo ""
echo "🔧 Configuring Citus cluster..."

# Enable Citus extension on coordinator
echo "📋 Enabling Citus extension on coordinator..."
PGPASSWORD=citus_demo psql -h "$COORD_HOST" -p "$COORD_PORT" -U "$COORD_USER" -d "$COORD_DB" << 'EOF'
CREATE EXTENSION IF NOT EXISTS citus;
EOF

# Enable Citus extension on all workers
for i in "${!WORKER_HOSTS[@]}"; do
    echo "📋 Enabling Citus extension on worker $((i+1))..."
    PGPASSWORD=citus_demo psql -h "${WORKER_HOSTS[$i]}" -p "${WORKER_PORTS[$i]}" -U "$WORKER_USER" -d "$WORKER_DB" << 'EOF'
CREATE EXTENSION IF NOT EXISTS citus;
EOF
done

# Add worker nodes to the coordinator
echo ""
echo "🔗 Adding worker nodes to coordinator..."
for i in "${!WORKER_HOSTS[@]}"; do
    echo "   Adding worker $((i+1)): ${WORKER_HOSTS[$i]}:${WORKER_PORTS[$i]}"
    PGPASSWORD=citus_demo psql -h "$COORD_HOST" -p "$COORD_PORT" -U "$COORD_USER" -d "$COORD_DB" << EOF
SELECT citus_add_node('${WORKER_HOSTS[$i]}', ${WORKER_PORTS[$i]});
EOF
done

# Verify cluster configuration
echo ""
echo "✅ Verifying cluster configuration..."
PGPASSWORD=citus_demo psql -h "$COORD_HOST" -p "$COORD_PORT" -U "$COORD_USER" -d "$COORD_DB" << 'EOF'
SELECT nodename, nodeport, isactive FROM citus_get_active_worker_nodes() ORDER BY nodeport;
EOF

echo ""
echo "🎉 Citus cluster setup complete!"
echo ""
echo "Cluster configuration:"
echo "  📡 Coordinator: $COORD_HOST:$COORD_PORT"
for i in "${!WORKER_HOSTS[@]}"; do
    echo "  🔧 Worker $((i+1)):     ${WORKER_HOSTS[$i]}:${WORKER_PORTS[$i]}"
done
echo ""
echo "Next steps:"
echo "  1. Load the distributed schema: docker exec -i citus-coord psql -U citus_demo -d citus_demo < examples/citus-distributed-schema.sql"
echo "  2. Run the demo: bash examples/ra-cli-citus-demo.sh"
echo ""