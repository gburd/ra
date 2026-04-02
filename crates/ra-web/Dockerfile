# Multi-stage Dockerfile for ra-web

# Stage 1: Prepare cargo chef for dependency caching
FROM rust:alpine AS chef
RUN apk add --no-cache musl-dev pkgconfig openssl-dev
RUN cargo install cargo-chef
WORKDIR /app

# Stage 2: Plan dependencies
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Build dependencies (cached layer)
FROM chef AS dependencies
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Stage 4: Build application
FROM rust:alpine AS builder
RUN apk add --no-cache musl-dev pkgconfig openssl-dev
WORKDIR /app

# Copy dependency artifacts from cache
COPY --from=dependencies /app/target target
COPY --from=dependencies /usr/local/cargo /usr/local/cargo

# Copy source code
COPY . .

# Build ra-web binary
RUN cargo build --release --bin ra-web

# Stage 5: Production runtime
FROM alpine:3.21

# Install runtime dependencies
RUN apk add --no-cache ca-certificates libgcc wget

# Create non-root user
RUN addgroup -g 1000 ra && \
    adduser -D -u 1000 -G ra ra

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/ra-web /usr/local/bin/ra-web

# Copy static files and rules
COPY --chown=ra:ra crates/ra-web/static /app/static
COPY --chown=ra:ra rules /app/rules

# Switch to non-root user
USER ra

# Expose ports
EXPOSE 8000

# Health check
HEALTHCHECK --interval=30s --timeout=3s --retries=3 \
  CMD wget --quiet --tries=1 --spider http://localhost:8000/health || exit 1

# Start ra-web
CMD ["ra-web"]
