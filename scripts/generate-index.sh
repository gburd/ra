#!/usr/bin/env bash
set -euo pipefail

# Regenerates rules/index.toml from all .rra files in the rules/ directory.
# Reads YAML frontmatter from each rule and builds the index.

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RULES_DIR="${REPO_ROOT}/rules"
INDEX_FILE="${RULES_DIR}/index.toml"

if [[ ! -d "${RULES_DIR}" ]]; then
    echo "Error: rules directory not found at ${RULES_DIR}" >&2
    exit 1
fi

rule_count=0
declare -A category_rules

while IFS= read -r -d '' rra_file; do
    # Extract YAML frontmatter between --- markers
    frontmatter=$(sed -n '/^---$/,/^---$/p' "${rra_file}" | sed '1d;$d')

    id=$(echo "${frontmatter}" | grep '^id:' | sed 's/^id:[[:space:]]*//')
    category=$(echo "${frontmatter}" | grep '^category:' | sed 's/^category:[[:space:]]*//')

    if [[ -z "${id}" || -z "${category}" ]]; then
        echo "Warning: skipping ${rra_file} (missing id or category)" >&2
        continue
    fi

    rel_path="${rra_file#"${RULES_DIR}/"}"
    category_rules["${category}"]+="\"${id}:${rel_path}\","
    rule_count=$((rule_count + 1))
done < <(find "${RULES_DIR}" -name '*.rra' -not -path '*/templates/*' -print0)

# Write index header
cat > "${INDEX_FILE}" << EOF
version = "1.0.0"
total_rules = ${rule_count}
last_updated = "$(date +%Y-%m-%d)"
EOF

# Write category sections
for category in $(echo "${!category_rules[@]}" | tr ' ' '\n' | sort); do
    top_level="${category%%/*}"
    sub_category="${category#*/}"

    # Build rules array
    rules_str="${category_rules[${category}]}"
    rules_str="${rules_str%,}"  # Remove trailing comma

    # Convert "id:path" pairs to just ids for the rules array
    ids=""
    IFS=',' read -ra pairs <<< "${rules_str}"
    for pair in "${pairs[@]}"; do
        pair="${pair//\"/}"
        rule_id="${pair%%:*}"
        ids+="\"${rule_id}\", "
    done
    ids="${ids%, }"

    {
        echo ""
        echo "[categories.${top_level}.${sub_category}]"
        echo "rules = [${ids}]"
    } >> "${INDEX_FILE}"
done

echo "Generated ${INDEX_FILE} with ${rule_count} rules"
