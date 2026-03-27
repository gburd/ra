#!/bin/bash
set -euo pipefail

# Test verbose mode with a simple query that should trigger optimization

echo "Testing verbose mode with --rules-applied --verbose"
echo "=================================================="
echo

./target/release/ra-cli optimize --rules-applied --verbose --resource-budget standard \
  "SELECT * FROM orders WHERE status = 'pending' AND year = 2024"

echo
echo "Testing normal mode with --rules-applied (no verbose)"
echo "======================================================="
echo

./target/release/ra-cli optimize --rules-applied --resource-budget standard \
  "SELECT * FROM orders WHERE status = 'pending' AND year = 2024"
