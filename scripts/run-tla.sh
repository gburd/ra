#!/usr/bin/env bash
set -euo pipefail

# TLA+ Model Checking Script
# Runs TLC model checker on all specifications

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TLA_DIR="$PROJECT_ROOT/tla"
MODELS_DIR="$TLA_DIR/models"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if TLC is installed
if ! command -v tlc &> /dev/null; then
    echo -e "${RED}Error: TLC not found${NC}"
    echo "Please install TLA+ Toolbox from: https://lamport.azurewebsites.net/tla/toolbox.html"
    echo "Or install via homebrew: brew install tla-plus-toolbox"
    exit 1
fi

# Function to run TLC on a specification
run_tlc() {
    local spec_name=$1
    local spec_file="$TLA_DIR/${spec_name}.tla"
    local cfg_file="$MODELS_DIR/${spec_name}.cfg"

    echo -e "${YELLOW}Checking ${spec_name}...${NC}"

    if [ ! -f "$spec_file" ]; then
        echo -e "${RED}Error: Specification file not found: $spec_file${NC}"
        return 1
    fi

    if [ ! -f "$cfg_file" ]; then
        echo -e "${RED}Error: Configuration file not found: $cfg_file${NC}"
        return 1
    fi

    # Run TLC model checker
    # -workers auto: use all available CPU cores
    # -config: specify configuration file
    # -cleanup: remove temporary files
    # -deadlock: check for deadlocks
    if tlc -workers auto -config "$cfg_file" -cleanup -deadlock "$spec_file" 2>&1 | tee "$MODELS_DIR/${spec_name}.log"; then
        echo -e "${GREEN}✓ ${spec_name} verification passed${NC}\n"
        return 0
    else
        echo -e "${RED}✗ ${spec_name} verification failed${NC}\n"
        return 1
    fi
}

# Main execution
echo "=========================================="
echo "TLA+ Model Checking for RA Optimizer"
echo "=========================================="
echo ""

failed=0

# Check each specification
for spec in RuleComposition CostMonotonicity Equivalence; do
    if ! run_tlc "$spec"; then
        ((failed++))
    fi
done

echo "=========================================="
if [ $failed -eq 0 ]; then
    echo -e "${GREEN}All verification checks passed!${NC}"
    exit 0
else
    echo -e "${RED}$failed verification check(s) failed${NC}"
    echo "Check log files in $MODELS_DIR for details"
    exit 1
fi
