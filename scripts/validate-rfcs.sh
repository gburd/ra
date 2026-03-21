#!/usr/bin/env bash
set -euo pipefail

# Validates RFC directory structure, numbering, and required sections.

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RFC_DIR="${REPO_ROOT}/rfcs"
TEXT_DIR="${RFC_DIR}/text"
INDEX_FILE="${RFC_DIR}/INDEX.md"

errors=0
warnings=0

error() {
    echo "ERROR: $1"
    ((errors++)) || true
}

warn() {
    echo "WARN:  $1"
    ((warnings++)) || true
}

info() {
    echo "INFO:  $1"
}

# Check directory structure
if [[ ! -d "${TEXT_DIR}" ]]; then
    error "Missing rfcs/text/ directory"
fi
if [[ ! -d "${RFC_DIR}/_accepted" ]]; then
    error "Missing rfcs/_accepted/ directory"
fi
if [[ ! -d "${RFC_DIR}/_rejected" ]]; then
    error "Missing rfcs/_rejected/ directory"
fi
if [[ ! -f "${INDEX_FILE}" ]]; then
    error "Missing rfcs/INDEX.md"
fi
if [[ ! -f "${RFC_DIR}/README.md" ]]; then
    error "Missing rfcs/README.md"
fi
if [[ ! -f "${RFC_DIR}/TEMPLATE.md" ]]; then
    error "Missing rfcs/TEMPLATE.md"
fi

# Check sequential numbering
info "Checking RFC numbering sequence..."
prev=0
for f in "${TEXT_DIR}"/????-*.md; do
    num=$(basename "$f" | cut -c1-4 | sed 's/^0*//')
    if [[ -z "${num}" ]]; then
        num=0
    fi
    expected=$((prev + 1))
    if [[ "${num}" -ne "${expected}" ]]; then
        warn "Numbering gap: expected $(printf '%04d' "${expected}"), found $(printf '%04d' "${num}") ($(basename "$f"))"
    fi
    prev="${num}"
done

# Check required sections in each RFC
info "Checking required sections..."
for f in "${TEXT_DIR}"/????-*.md; do
    name=$(basename "$f")

    # Summary can be "Summary" or "Executive Summary"
    if ! grep -qi "^##.*[Ss]ummary" "$f"; then
        error "${name}: missing required section 'Summary'"
    fi

    # Motivation can be "Motivation" or "Background"
    if ! grep -qi "^##.*\(Motivation\|Background\)" "$f"; then
        error "${name}: missing required section 'Motivation'"
    fi

    # Check for status header (various formats)
    if ! grep -qi "Status" "$f"; then
        warn "${name}: missing Status field in header"
    fi

    # Check title format
    first_line=$(head -1 "$f")
    if [[ ! "${first_line}" =~ ^#\ RFC\ [0-9]+ ]]; then
        warn "${name}: title should start with '# RFC NNNN:'"
    fi
done

# Check INDEX.md references match text/ directory
info "Checking INDEX.md references..."
for f in "${TEXT_DIR}"/????-*.md; do
    name=$(basename "$f")
    num=$(basename "$f" | cut -c1-4)
    if ! grep -q "${name}" "${INDEX_FILE}"; then
        warn "RFC ${num} (${name}) not listed in INDEX.md"
    fi
done

# Check for orphaned root-level RFC files
info "Checking for orphaned root-level RFC files..."
for f in "${RFC_DIR}"/????-*.md; do
    name=$(basename "$f")
    if [[ -f "${TEXT_DIR}/${name}" ]]; then
        error "Duplicate RFC at root level: ${name} (canonical copy is in text/)"
    fi
done

# Summary
echo ""
echo "=== RFC Validation Summary ==="
total=$(find "${TEXT_DIR}" -name '????-*.md' | wc -l | tr -d ' ')
echo "Total RFCs: ${total}"
echo "Errors:     ${errors}"
echo "Warnings:   ${warnings}"

if [[ "${errors}" -gt 0 ]]; then
    echo ""
    echo "FAILED: ${errors} error(s) found"
    exit 1
fi

if [[ "${warnings}" -gt 0 ]]; then
    echo ""
    echo "PASSED with ${warnings} warning(s)"
    exit 0
fi

echo ""
echo "PASSED: all checks passed"
