#!/usr/bin/env bash
set -euo pipefail

# Validation script for Ra benchmark system
# Checks that all required files and components are present

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Colors
readonly GREEN='\033[0;32m'
readonly RED='\033[0;31m'
readonly BLUE='\033[0;34m'
readonly NC='\033[0m'

check_file() {
    local file=$1
    local desc=$2

    if [[ -f "${PROJECT_ROOT}/${file}" ]]; then
        echo -e "${GREEN}✓${NC} ${desc}: ${file}"
        return 0
    else
        echo -e "${RED}✗${NC} ${desc}: ${file} (MISSING)"
        return 1
    fi
}

check_dir() {
    local dir=$1
    local desc=$2

    if [[ -d "${PROJECT_ROOT}/${dir}" ]]; then
        echo -e "${GREEN}✓${NC} ${desc}: ${dir}"
        return 0
    else
        echo -e "${RED}✗${NC} ${desc}: ${dir} (MISSING)"
        return 1
    fi
}

main() {
    echo -e "${BLUE}Validating Ra Benchmark System${NC}"
    echo "================================"
    echo

    local errors=0

    echo "Core Implementation:"
    check_file "crates/ra-cli/src/commands/mod.rs" "Commands module" || ((errors++))
    check_file "crates/ra-cli/src/commands/benchmark.rs" "Benchmark module" || ((errors++))
    check_file "crates/ra-cli/templates/comparison_dashboard_template.html" "HTML template" || ((errors++))
    echo

    echo "Automation:"
    check_file "scripts/run-all-benchmarks.sh" "Benchmark automation script" || ((errors++))
    echo

    echo "Documentation:"
    check_file "docs/benchmarks/README.md" "Benchmarks README" || ((errors++))
    check_file "docs/benchmarks/COMPARISON_METHODOLOGY.md" "Methodology doc" || ((errors++))
    check_file "docs/benchmarks/SAMPLE_COMPARISON_REPORT.md" "Sample report" || ((errors++))
    check_file "docs/benchmarks/comparison-dashboard.html" "Demo dashboard" || ((errors++))
    check_file "docs/benchmarks/.gitignore" "Results gitignore" || ((errors++))
    echo

    echo "Directories:"
    check_dir "crates/ra-cli/src/commands" "Commands directory" || ((errors++))
    check_dir "crates/ra-cli/templates" "Templates directory" || ((errors++))
    check_dir "docs/benchmarks" "Benchmarks documentation" || ((errors++))
    echo

    echo "Checking file executability:"
    if [[ -x "${PROJECT_ROOT}/scripts/run-all-benchmarks.sh" ]]; then
        echo -e "${GREEN}✓${NC} run-all-benchmarks.sh is executable"
    else
        echo -e "${RED}✗${NC} run-all-benchmarks.sh is not executable"
        ((errors++))
    fi
    echo

    if [[ $errors -eq 0 ]]; then
        echo -e "${GREEN}✓ All validation checks passed!${NC}"
        echo
        echo "Next steps:"
        echo "  1. cargo check --bin ra-cli"
        echo "  2. cargo run --bin ra-cli -- benchmark --help"
        echo "  3. cargo run --bin ra-cli -- benchmark --database sqlite --workload joins"
        echo "  4. ./scripts/run-all-benchmarks.sh"
        exit 0
    else
        echo -e "${RED}✗ Validation failed with ${errors} error(s)${NC}"
        exit 1
    fi
}

main "$@"
