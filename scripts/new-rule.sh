#!/usr/bin/env bash
set -euo pipefail

# Creates a new .rra rule file from a template.
# Usage: ./scripts/new-rule.sh <rule-id> <category>
# Example: ./scripts/new-rule.sh filter-through-join logical/predicate-pushdown

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RULES_DIR="${REPO_ROOT}/rules"
TEMPLATES_DIR="${RULES_DIR}/templates"

usage() {
    echo "Usage: $0 <rule-id> <category>"
    echo ""
    echo "Arguments:"
    echo "  rule-id    Unique identifier in kebab-case (e.g., filter-through-join)"
    echo "  category   Category path (e.g., logical/predicate-pushdown)"
    echo ""
    echo "Valid categories:"
    echo "  logical/predicate-pushdown    logical/join-reordering"
    echo "  logical/join-elimination      logical/subquery-unnesting"
    echo "  logical/projection-pushdown   logical/aggregate-pushdown"
    echo "  logical/expression-simplification"
    echo "  logical/limit-pushdown        logical/set-operations"
    echo "  physical/join-algorithms      physical/index-selection"
    echo "  physical/aggregation-strategies"
    echo "  physical/parallelization      physical/materialization"
    echo "  database-specific/<db-name>   execution-models/<model>"
    echo "  cost-models                   experimental"
    echo ""
    echo "Examples:"
    echo "  $0 filter-through-join logical/predicate-pushdown"
    echo "  $0 hash-join-selection physical/join-algorithms"
    echo "  $0 pg-partial-index database-specific/postgresql"
    exit 1
}

if [[ $# -lt 2 ]]; then
    usage
fi

RULE_ID="$1"
CATEGORY="$2"

# Validate rule-id format (kebab-case)
if [[ ! "${RULE_ID}" =~ ^[a-z][a-z0-9-]*[a-z0-9]$ ]]; then
    echo "Error: rule-id must be kebab-case (e.g., filter-through-join)" >&2
    exit 1
fi

# Determine template based on category prefix
top_level="${CATEGORY%%/*}"
case "${top_level}" in
    logical)
        template="${TEMPLATES_DIR}/template-logical.rra"
        ;;
    physical)
        template="${TEMPLATES_DIR}/template-physical.rra"
        ;;
    database-specific)
        template="${TEMPLATES_DIR}/template-database-specific.rra"
        ;;
    execution-models|cost-models|experimental)
        template="${TEMPLATES_DIR}/template-logical.rra"
        ;;
    *)
        echo "Error: unknown category prefix '${top_level}'" >&2
        usage
        ;;
esac

if [[ ! -f "${template}" ]]; then
    echo "Error: template not found at ${template}" >&2
    exit 1
fi

# Determine output path
output_dir="${RULES_DIR}/${CATEGORY}"
output_file="${output_dir}/${RULE_ID}.rra"

if [[ -f "${output_file}" ]]; then
    echo "Error: rule already exists at ${output_file}" >&2
    exit 1
fi

mkdir -p "${output_dir}"

# Generate human-readable name from ID
rule_name=$(echo "${RULE_ID}" | sed 's/-/ /g' | awk '{for(i=1;i<=NF;i++) $i=toupper(substr($i,1,1)) substr($i,2)} 1')

# Copy template and substitute placeholders
sed \
    -e "s/RULE-ID-HERE/${RULE_ID}/g" \
    -e "s/Human-Readable Rule Name/${rule_name}/g" \
    -e "s|category: logical/SUBCATEGORY|category: ${CATEGORY}|" \
    -e "s|category: physical/SUBCATEGORY|category: ${CATEGORY}|" \
    -e "s|category: database-specific/DATABASE-NAME|category: ${CATEGORY}|" \
    -e "s/Rule Name/${rule_name}/g" \
    "${template}" > "${output_file}"

echo "Created: ${output_file}"
echo ""
echo "Next steps:"
echo "  1. Edit ${output_file}"
echo "  2. Fill in description, algebra, implementation, and test cases"
echo "  3. Validate: ./scripts/validate-all.sh"
echo "  4. Regenerate index: ./scripts/generate-index.sh"
