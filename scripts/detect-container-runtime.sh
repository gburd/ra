#!/usr/bin/env bash
set -euo pipefail

# Detect container runtime (Docker or Podman) and set variables.
#
# Usage:
#   source scripts/detect-container-runtime.sh
#
# Exports:
#   CONTAINER_RUNTIME  - "docker" or "podman"
#   COMPOSE_COMMAND    - full compose command (e.g. "docker compose" or "podman-compose")

detect_container_runtime() {
    if command -v docker &> /dev/null; then
        CONTAINER_RUNTIME="docker"
    elif command -v podman &> /dev/null; then
        CONTAINER_RUNTIME="podman"
    else
        echo "Error: No container runtime found." >&2
        echo "Install one of:" >&2
        echo "  Docker:  https://docs.docker.com/get-docker/" >&2
        echo "  Podman:  https://podman.io/getting-started/installation" >&2
        exit 1
    fi
    export CONTAINER_RUNTIME
}

detect_compose_command() {
    case "$CONTAINER_RUNTIME" in
        docker)
            if docker compose version &> /dev/null; then
                COMPOSE_COMMAND="docker compose"
            elif command -v docker-compose &> /dev/null; then
                COMPOSE_COMMAND="docker-compose"
            else
                echo "Error: docker compose plugin not found." >&2
                echo "Install with:" >&2
                echo "  macOS: brew install docker-compose" >&2
                echo "  Linux: apt-get install docker-compose-plugin" >&2
                exit 1
            fi
            ;;
        podman)
            if command -v podman-compose &> /dev/null; then
                COMPOSE_COMMAND="podman-compose"
            else
                echo "Error: podman-compose not found." >&2
                echo "Install with:" >&2
                echo "  pip install podman-compose" >&2
                echo "  or: brew install podman-compose" >&2
                exit 1
            fi
            ;;
    esac
    export COMPOSE_COMMAND
}

detect_container_runtime
detect_compose_command

echo "Container runtime: $CONTAINER_RUNTIME" >&2
echo "Compose command:   $COMPOSE_COMMAND" >&2
