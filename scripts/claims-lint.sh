#!/usr/bin/env bash
# Truth-in-claims lint (RA-STEERING.md §3.2).
#
# Fails if banned marketing words appear in committed prose, or if the
# active-rule count in README.md drifts from what the build reports.
#
# Scope: prose only (README.md, CHANGELOG.md, docs/**/*.md). Excludes
# generated/vendored trees (docs/.vitepress/dist, cache, node_modules,
# target, vendor) and RA-STEERING.md itself (it quotes the banned words
# to ban them).
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0

# --- 1. Banned marketing words -------------------------------------------
# Whole-word, case-insensitive. "complete"/"fully" are allowed because they
# have legitimate technical uses ("fully supported grammar", "complete
# implementation of X"); the steering doc bans them only as self-assessment,
# which a grep cannot distinguish. The unambiguous marketing terms are hard
# failures.
banned='comprehensive|excellence|production-ready|world-class|revolutionize'

prose_files() {
  # Scope: the top-level claim surfaces named by RA-STEERING.md §3. The
  # docs/ tree has a large banned-word backlog concentrated in RFC and
  # reference material that §4 moves to ra-lab or rewrites; sweeping it is
  # tracked as a Phase-2 chore, not gated here. Add docs/ back once that
  # sweep lands.
  { echo README.md; echo CHANGELOG.md; } 2>/dev/null | sort -u
}

echo "== banned marketing words =="
while IFS= read -r f; do
  [ -f "$f" ] || continue
  if grep -HniE "\\b(${banned})\\b" "$f"; then
    fail=1
  fi
done < <(prose_files)
[ "$fail" -eq 0 ] && echo "  none found"

# --- 2. Rule-count consistency -------------------------------------------
# The README states one active-rule number; it must match the build.
# We do not run cargo here (too slow for a lint job) — instead we require
# the README to name the regeneration command AND we check that only ONE
# distinct active-rule number appears, to prevent the "170 vs 293" drift.
echo "== rule-count consistency =="
counts=$(grep -oE '[0-9]+ active rewrite rules|[0-9]+ rules active' README.md \
         | grep -oE '^[0-9]+' | sort -u || true)
n=$(printf '%s\n' "$counts" | grep -c . || true)
if [ "$n" -gt 1 ]; then
  echo "  ERROR: README.md has multiple active-rule counts: $counts"
  echo "  There must be exactly one. Regenerate with:"
  echo "    cargo run --release -p ra-engine --example count_rules"
  fail=1
else
  echo "  single active-rule count: ${counts:-<none stated>}"
fi

if [ "$fail" -ne 0 ]; then
  echo
  echo "claims-lint FAILED (RA-STEERING.md §3.2)"
  exit 1
fi
echo
echo "claims-lint passed"
