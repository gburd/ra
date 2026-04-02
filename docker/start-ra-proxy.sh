#!/bin/bash
set -e

# Ra Proxy Startup Script
# Starts PostgreSQL 19 and Ra proxy side-by-side

echo "Starting Ra proxy and PostgreSQL 19..."

# Initialize PostgreSQL database if not already initialized
if [ ! -s "$PGDATA/PG_VERSION" ]; then
    echo "Initializing PostgreSQL database..."
    initdb -D "$PGDATA" --auth=trust --username="${POSTGRES_USER:-postgres}"

    # Configure PostgreSQL
    cat >> "$PGDATA/postgresql.conf" <<EOF

# Ra proxy configuration
listen_addresses = '*'
port = 5432
max_connections = 100

# Enable pg_plan_advice extension (if available)
shared_preload_libraries = 'pg_plan_advice'

# Logging for plan analysis
log_min_duration_statement = 0
log_statement = 'all'
log_duration = on

# Performance settings
shared_buffers = 256MB
effective_cache_size = 1GB
maintenance_work_mem = 64MB
work_mem = 16MB

# Query planner settings
enable_seqscan = on
enable_indexscan = on
enable_bitmapscan = on
enable_hashjoin = on
enable_mergejoin = on
enable_nestloop = on
EOF

    # Configure client authentication
    cat >> "$PGDATA/pg_hba.conf" <<EOF

# Allow connections from any host
host    all             all             0.0.0.0/0               scram-sha-256
host    all             all             ::/0                    scram-sha-256
EOF

    echo "Database initialized successfully"
fi

# Start PostgreSQL in background
echo "Starting PostgreSQL 19..."
pg_ctl -D "$PGDATA" -o "-c listen_addresses='*'" -w start

# Create database and user if specified
if [ -n "$POSTGRES_DB" ] && [ "$POSTGRES_DB" != "postgres" ]; then
    psql -v ON_ERROR_STOP=1 --username "${POSTGRES_USER:-postgres}" <<-EOSQL
        SELECT 'CREATE DATABASE $POSTGRES_DB'
        WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = '$POSTGRES_DB')\gexec
EOSQL
fi

# Set password if specified
if [ -n "$POSTGRES_PASSWORD" ]; then
    psql -v ON_ERROR_STOP=1 --username "${POSTGRES_USER:-postgres}" <<-EOSQL
        ALTER USER ${POSTGRES_USER:-postgres} WITH PASSWORD '$POSTGRES_PASSWORD';
EOSQL
fi

# Try to create pg_plan_advice extension
psql -v ON_ERROR_STOP=0 --username "${POSTGRES_USER:-postgres}" -d "${POSTGRES_DB:-postgres}" <<-EOSQL
    CREATE EXTENSION IF NOT EXISTS pg_plan_advice;
EOSQL

echo "PostgreSQL 19 started successfully"

# Start Ra proxy in background
echo "Starting Ra proxy on port ${RA_PROXY_PORT:-8001}..."
export RUST_LOG="${RA_PROXY_LOG_LEVEL:-info}"
/usr/local/bin/ra-proxy &
RA_PROXY_PID=$!

echo "Ra proxy started with PID $RA_PROXY_PID"

# Function to handle shutdown
shutdown() {
    echo "Shutting down Ra proxy and PostgreSQL..."
    kill "$RA_PROXY_PID" 2>/dev/null || true
    pg_ctl -D "$PGDATA" -m fast -w stop
    exit 0
}

# Trap signals for graceful shutdown
trap shutdown SIGTERM SIGINT

# Wait for processes
wait "$RA_PROXY_PID"
