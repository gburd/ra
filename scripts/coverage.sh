#!/usr/bin/env bash
set -euo pipefail

# Generate test coverage reports for the ra workspace.
#
# Usage:
#   ./scripts/coverage.sh              # full workspace coverage (text)
#   ./scripts/coverage.sh --html       # generate HTML report
#   ./scripts/coverage.sh --lcov       # generate lcov.info
#   ./scripts/coverage.sh -p ra-core   # single crate coverage
#
# Requires: cargo-llvm-cov (included in nix devShell)

PACKAGE_ARGS=("--workspace" "--exclude" "ra-pg-extension")
OUTPUT_MODE="--text"

while [[ $# -gt 0 ]]; do
  case $1 in
    --html)
      OUTPUT_MODE="--html"
      shift
      ;;
    --lcov)
      OUTPUT_MODE="--lcov --output-path lcov.info"
      shift
      ;;
    -p|--package)
      PACKAGE_ARGS=("--package" "$2")
      shift 2
      ;;
    *)
      echo "Unknown option: $1" >&2
      echo "Usage: $0 [--html|--lcov] [-p <crate>]" >&2
      exit 1
      ;;
  esac
done

echo "Running coverage with: ${PACKAGE_ARGS[*]}"
echo ""

# shellcheck disable=SC2086
cargo llvm-cov \
  --all-features \
  "${PACKAGE_ARGS[@]}" \
  $OUTPUT_MODE

if [[ "$OUTPUT_MODE" == "--html" ]]; then
  echo ""
  echo "HTML report generated at: target/llvm-cov/html/index.html"
fi
