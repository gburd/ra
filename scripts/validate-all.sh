#!/usr/bin/env bash
set -euo pipefail

# Validates all .rra rule files in the rules/ directory.
# Uses ra-cli validate if available, otherwise performs basic structural checks.

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RULES_DIR="${REPO_ROOT}/rules"

if [[ ! -d "${RULES_DIR}" ]]; then
    echo "Error: rules directory not found at ${RULES_DIR}" >&2
    exit 1
fi

errors=0
checked=0

validate_rule() {
    local file="$1"
    local rel_path="${file#"${RULES_DIR}/"}"

    # Check file is not empty
    if [[ ! -s "${file}" ]]; then
        echo "FAIL: ${rel_path} - file is empty" >&2
        return 1
    fi

    # Check for YAML frontmatter delimiters
    local first_line
    first_line=$(head -1 "${file}")
    if [[ "${first_line}" != "---" ]]; then
        echo "FAIL: ${rel_path} - missing opening frontmatter delimiter" >&2
        return 1
    fi

    # Check closing frontmatter delimiter exists (after line 1)
    if ! tail -n +2 "${file}" | grep -q '^---$'; then
        echo "FAIL: ${rel_path} - missing closing frontmatter delimiter" >&2
        return 1
    fi

    # Extract frontmatter
    local frontmatter
    frontmatter=$(sed -n '2,/^---$/p' "${file}" | sed '$d')

    # Check required fields
    for field in id name category; do
        if ! echo "${frontmatter}" | grep -q "^${field}:"; then
            echo "FAIL: ${rel_path} - missing required field '${field}'" >&2
            return 1
        fi
    done

    # Check category is valid
    local category
    category=$(echo "${frontmatter}" | grep '^category:' | sed 's/^category:[[:space:]]*//')
    local valid_prefixes="logical/ physical/ database-specific/ execution-models/ cost-models experimental"
    local valid=false
    for prefix in ${valid_prefixes}; do
        if [[ "${category}" == ${prefix}* ]] || [[ "${category}" == "${prefix}" ]]; then
            valid=true
            break
        fi
    done
    if [[ "${valid}" != "true" ]]; then
        echo "FAIL: ${rel_path} - invalid category '${category}'" >&2
        return 1
    fi

    echo "OK: ${rel_path}"
    return 0
}

# Try ra-cli first
if command -v ra-cli &> /dev/null || [[ -f "${REPO_ROOT}/target/debug/ra-cli" ]]; then
    ra_cli="${REPO_ROOT}/target/debug/ra-cli"
    if command -v ra-cli &> /dev/null; then
        ra_cli="ra-cli"
    fi
    echo "Using ra-cli for validation: ${ra_cli}"
    echo ""
    "${ra_cli}" validate "${RULES_DIR}/"
    exit $?
fi

echo "ra-cli not found, using basic structural validation"
echo "Run 'cargo build --bin ra-cli' for full validation"
echo ""

while IFS= read -r -d '' rra_file; do
    checked=$((checked + 1))
    if ! validate_rule "${rra_file}"; then
        errors=$((errors + 1))
    fi
done < <(find "${RULES_DIR}" -name '*.rra' -not -path '*/templates/*' -print0)

echo ""
echo "Checked ${checked} rules, ${errors} errors"

if [[ ${errors} -gt 0 ]]; then
    exit 1
fi
