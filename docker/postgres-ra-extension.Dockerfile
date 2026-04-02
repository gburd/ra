# Multi-stage Dockerfile for PostgreSQL 16 with Ra planner extension

# Stage 1: Build Ra extension
FROM rust:1.88-bookworm AS builder

# Install prerequisites for adding PostgreSQL repository
RUN apt-get update && apt-get install -y \
    wget \
    gnupg \
    lsb-release \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Add PostgreSQL APT repository
RUN wget --quiet -O - https://www.postgresql.org/media/keys/ACCC4CF8.asc | apt-key add - \
    && echo "deb http://apt.postgresql.org/pub/repos/apt bookworm-pgdg main" > /etc/apt/sources.list.d/pgdg.list

# Install PostgreSQL development packages and pgrx dependencies
RUN apt-get update && apt-get install -y \
    postgresql-server-dev-16 \
    postgresql-16 \
    libpq-dev \
    pkg-config \
    clang \
    libclang-dev \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-pgrx
RUN cargo install --locked cargo-pgrx --version 0.17.0

# Initialize pgrx for PostgreSQL 16
RUN cargo pgrx init --pg16 /usr/bin/pg_config

WORKDIR /build

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY rules ./rules

# Build ra-pg-extension
WORKDIR /build/crates/ra-pg-extension
RUN cargo pgrx package --pg-config /usr/bin/pg_config

# Stage 2: Production PostgreSQL 16 with Ra extension
FROM postgres:16-bookworm

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libclang1 \
    && rm -rf /var/lib/apt/lists/*

# Copy built extension from builder
COPY --from=builder /build/crates/ra-pg-extension/target/release/pg_ra_planner-pg16 /usr/share/postgresql/16/extension/

# Copy extension control and SQL files
COPY --from=builder /build/crates/ra-pg-extension/pg_ra_planner.control /usr/share/postgresql/16/extension/
COPY --from=builder /build/crates/ra-pg-extension/sql/*.sql /usr/share/postgresql/16/extension/

# Configure PostgreSQL to load Ra extension
RUN echo "shared_preload_libraries = 'pg_ra_planner'" >> /usr/share/postgresql/postgresql.conf.sample

# Add initialization script to enable extension
RUN mkdir -p /docker-entrypoint-initdb.d
COPY docker/postgres-ra-extension-init.sql /docker-entrypoint-initdb.d/02-ra-extension.sql

EXPOSE 5432

CMD ["postgres"]
