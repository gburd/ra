#!/usr/bin/env bash
# Test script for improved verbose mode output

set -euo pipefail

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}Building ra-cli...${NC}"
cargo build --package ra-cli --bin ra-cli --quiet

RA_CLI="./target/debug/ra-cli"

echo -e "\n${GREEN}Test 1: Simple filter optimization${NC}"
echo "Query: SELECT * FROM users WHERE age > 18"
echo "---"
$RA_CLI optimize --rules-applied --verbose --resource-budget standard \
  "SELECT * FROM users WHERE age > 18" 2>&1 | grep -A 20 "Intermediate Optimization Steps" || true

echo -e "\n${GREEN}Test 2: Index optimization${NC}"
echo "Query: SELECT name FROM users WHERE age > 18 LIMIT 10"
echo "---"
$RA_CLI optimize --rules-applied --verbose --resource-budget batch \
  "SELECT name FROM users WHERE age > 18 LIMIT 10" 2>&1 | grep -A 50 "Intermediate Optimization Steps" || true

echo -e "\n${GREEN}Test 3: Multiple optimizations${NC}"
echo "Query: SELECT u.id, o.total FROM users u JOIN orders o ON u.id = o.user_id WHERE u.status = 'active' AND o.amount > 100"
echo "---"
$RA_CLI optimize --rules-applied --verbose --resource-budget standard \
  "SELECT u.id, o.total FROM users u JOIN orders o ON u.id = o.user_id WHERE u.status = 'active' AND o.amount > 100" 2>&1 | \
  grep -A 80 "Intermediate Optimization Steps" || true

echo -e "\n${GREEN}Test 4: Projection pushdown${NC}"
echo "Query: SELECT name, email FROM (SELECT * FROM users WHERE age > 18) WHERE status = 'active'"
echo "---"
$RA_CLI optimize --rules-applied --verbose --resource-budget standard \
  "SELECT name, email FROM (SELECT * FROM users WHERE age > 18) WHERE status = 'active'" 2>&1 | \
  grep -A 60 "Intermediate Optimization Steps" || true

echo -e "\n${BLUE}All tests completed!${NC}"
