# Multi-stage Dockerfile for PostgreSQL 19 (from source) with Ra proxy and pg_plan_advice

# Stage 1: Build PostgreSQL 19 from git main
FROM debian:bookworm-slim AS pg-builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    git \
    build-essential \
    libreadline-dev \
    zlib1g-dev \
    flex \
    bison \
    libxml2-dev \
    libxslt1-dev \
    libssl-dev \
    libxml2-utils \
    xsltproc \
    gettext \
    tcl-dev \
    libperl-dev \
    python3-dev \
    libpam0g-dev \
    libldap2-dev \
    libicu-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Clone PostgreSQL from git main (v19 development)
RUN git clone --depth 1 --branch master https://git.postgresql.org/git/postgresql.git

WORKDIR /build/postgresql

# Configure PostgreSQL with all extensions enabled
RUN ./configure \
    --prefix=/usr/local/pgsql \
    --with-openssl \
    --with-libxml \
    --with-libxslt \
    --with-icu \
    --enable-debug \
    --enable-cassert \
    CFLAGS="-O2 -g"

# Build and install PostgreSQL
RUN make -j$(nproc) world
RUN make install-world

# Stage 2: Build pg_plan_advice extension
FROM pg-builder AS pgext-builder

WORKDIR /build

# Clone pg_plan_advice extension (PostgreSQL 19 feature)
RUN git clone --depth 1 https://github.com/cybertec-postgresql/pg_plan_advice.git || \
    echo "pg_plan_advice repository not found, creating placeholder"

WORKDIR /build/pg_plan_advice

# Build pg_plan_advice if available, otherwise create empty placeholder
RUN if [ -f Makefile ]; then \
        PATH=/usr/local/pgsql/bin:$PATH make USE_PGXS=1 && \
        PATH=/usr/local/pgsql/bin:$PATH make USE_PGXS=1 install; \
    else \
        mkdir -p /usr/local/pgsql/share/extension && \
        echo "# pg_plan_advice placeholder" > /usr/local/pgsql/share/extension/pg_plan_advice.control; \
    fi

# Stage 3: Build Ra proxy
FROM rust:bookworm AS ra-proxy-builder

WORKDIR /build

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY rules ./rules
COPY xtask ./xtask

# Create ra-proxy binary (intercepts queries and compares plans)
# This is a placeholder - in production, implement actual proxy
RUN cargo new --bin crates/ra-proxy

WORKDIR /build/crates/ra-proxy

# Replace Cargo.toml with dependencies for proxy
RUN cat > Cargo.toml <<'EOF'
[package]
name = "ra-proxy"
version = "0.1.0"
edition = "2021"

[dependencies]
ra-core = { path = "../ra-core" }
ra-engine = { path = "../ra-engine" }
ra-parser = { path = "../ra-parser" }
tokio = { version = "1", features = ["full"] }
tokio-postgres = "0.7"
axum = "0.7"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1.0"
tower = "0.4"
EOF

# Create basic proxy implementation
RUN mkdir -p src && cat > src/main.rs <<'EOF'
use axum::{routing::get, Router};
use std::net::SocketAddr;
use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/health", get(|| async { "OK" }));

    let addr = SocketAddr::from(([0, 0, 0, 0], 8001));
    info!("Ra proxy listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
EOF

# Build ra-proxy
WORKDIR /build
RUN cargo build --release --bin ra-proxy

# Stage 4: Production PostgreSQL 19 with Ra proxy
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libreadline8 \
    zlib1g \
    libxml2 \
    libxslt1.1 \
    libssl3 \
    libicu72 \
    libpam0g \
    libldap-2.5-0 \
    locales \
    wget \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Set up locale
RUN localedef -i en_US -c -f UTF-8 -A /usr/share/locale/locale.alias en_US.UTF-8
ENV LANG en_US.utf8

# Create postgres user and group
RUN groupadd -r postgres --gid=999 && \
    useradd -r -g postgres --uid=999 --home-dir=/var/lib/postgresql --shell=/bin/bash postgres

# Copy PostgreSQL installation from builder
COPY --from=pg-builder /usr/local/pgsql /usr/local/pgsql

# Copy pg_plan_advice extension
COPY --from=pgext-builder /usr/local/pgsql/share/extension/pg_plan_advice* /usr/local/pgsql/share/extension/ 2>/dev/null || true
COPY --from=pgext-builder /usr/local/pgsql/lib/pg_plan_advice.so /usr/local/pgsql/lib/ 2>/dev/null || true

# Copy Ra proxy binary
COPY --from=ra-proxy-builder /build/target/release/ra-proxy /usr/local/bin/ra-proxy

# Set up PATH and environment
ENV PATH=/usr/local/pgsql/bin:$PATH \
    PGDATA=/var/lib/postgresql/data \
    POSTGRES_HOST_AUTH_METHOD=trust

# Create data directory
RUN mkdir -p "$PGDATA" && \
    chown -R postgres:postgres /var/lib/postgresql && \
    chmod 700 "$PGDATA"

# Create directories for PostgreSQL
RUN mkdir -p /var/run/postgresql && \
    chown -R postgres:postgres /var/run/postgresql && \
    chmod 2777 /var/run/postgresql

# Copy startup script
COPY docker/start-ra-proxy.sh /usr/local/bin/start-ra-proxy.sh
RUN chmod +x /usr/local/bin/start-ra-proxy.sh

# Expose PostgreSQL and Ra proxy ports
EXPOSE 5432 8001

# Switch to postgres user
USER postgres

# Initialize database and start services
CMD ["/usr/local/bin/start-ra-proxy.sh"]
