FROM node:22-alpine AS frontend-build
WORKDIR /app
COPY crates/ra-web/frontend/package.json crates/ra-web/frontend/package-lock.json ./
RUN npm ci
COPY crates/ra-web/frontend/ ./
RUN npm run build

FROM rust:1.88-slim AS server-build
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY xtask/ xtask/
COPY rules/ rules/
RUN cargo build --release --bin ra-web

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=server-build /app/target/release/ra-web /app/ra-web
COPY --from=frontend-build /app/dist /app/frontend
COPY crates/ra-web/static /app/static
COPY rules/ /app/rules/

ENV ROCKET_PORT=8000
ENV ROCKET_ADDRESS=0.0.0.0
ENV FRONTEND_DIR=/app/frontend
ENV STATIC_DIR=/app/static

EXPOSE 8000
CMD ["/app/ra-web"]
