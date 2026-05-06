#!/usr/bin/env bash
# Build PostgreSQL REL_17 with and without the Ra query optimizer planner hook.
#
# Produces two installations:
#   /opt/postgresql-ra/      — PostgreSQL + Ra planner hook (ra_planner extension loaded)
#   /opt/postgresql-standard/ — Vanilla PostgreSQL (control baseline)
#
# Usage:
#   ./scripts/build-postgres-with-ra.sh [--jobs N] [--prefix-ra PATH] [--prefix-std PATH]
#
# Prerequisites (Ubuntu/Debian):
#   sudo apt install build-essential libreadline-dev zlib1g-dev \
#        libssl-dev libxml2-dev libxslt-dev python3-dev flex bison
#
# Prerequisites (macOS/Homebrew):
#   brew install readline openssl libxml2 pkg-config icu4c
#
# Environment variables:
#   PG_BRANCH     PostgreSQL branch to build (default: REL_17_STABLE)
#   RA_REPO       Path to this Ra repository (default: parent of scripts/)
#   PG_REPO       Path to checkout PostgreSQL source (default: /tmp/postgres-src)
#   JOBS          Parallel build jobs (default: nproc)

set -euo pipefail

# ---------------------------------------------------------------------------
# Defaults and argument parsing
# ---------------------------------------------------------------------------
PG_BRANCH="${PG_BRANCH:-REL_17_STABLE}"
RA_REPO="${RA_REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
PG_REPO="${PG_REPO:-/tmp/postgres-src}"
PREFIX_RA="${PREFIX_RA:-/opt/postgresql-ra}"
PREFIX_STD="${PREFIX_STD:-/opt/postgresql-standard}"
JOBS="${JOBS:-$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)}"

while [[ $# -gt 0 ]]; do
    case $1 in
        --jobs)    JOBS="$2";       shift 2 ;;
        --prefix-ra)  PREFIX_RA="$2";  shift 2 ;;
        --prefix-std) PREFIX_STD="$2"; shift 2 ;;
        --branch)  PG_BRANCH="$2";  shift 2 ;;
        --pg-src)  PG_REPO="$2";    shift 2 ;;
        *) echo "Unknown argument: $1"; exit 1 ;;
    esac
done

log() { echo "[build-postgres-with-ra] $*"; }

# ---------------------------------------------------------------------------
# Step 1: Clone PostgreSQL source
# ---------------------------------------------------------------------------
log "Step 1: Clone PostgreSQL ${PG_BRANCH}"
if [[ ! -d "${PG_REPO}/.git" ]]; then
    git clone \
        --depth=1 \
        --branch "${PG_BRANCH}" \
        https://github.com/postgres/postgres.git \
        "${PG_REPO}"
else
    log "  Source already at ${PG_REPO} — pulling latest"
    git -C "${PG_REPO}" fetch origin "${PG_BRANCH}" --depth=1
    git -C "${PG_REPO}" checkout "origin/${PG_BRANCH}"
fi

# ---------------------------------------------------------------------------
# Step 2: Apply the Ra integration patch
# ---------------------------------------------------------------------------
log "Step 2: Apply Ra planner hook patch"

PATCH_FILE="${RA_REPO}/patches/postgres-ra-hook.patch"
if [[ -f "${PATCH_FILE}" ]]; then
    # Use the pre-generated patch from the repository
    git -C "${PG_REPO}" apply "${PATCH_FILE}" || {
        log "  NOTE: patch already applied or conflicts — continuing"
    }
else
    # Generate the minimal inline patch on the fly
    log "  No pre-built patch found; generating minimal hook inline"
    PLANNER_C="${PG_REPO}/src/backend/optimizer/plan/planner.c"

    # Insert Ra include and hook call at the top of standard_planner().
    # The patch:
    #   1. Adds #ifdef USE_RA / #include "ra_planner_hook.h" block
    #   2. Inserts ra_try_optimize() call at the start of standard_planner()
    #      with fallback to the standard path on NULL return.
    python3 - <<'PYEOF'
import re, sys

src = open("${PLANNER_C}").read()

# 1. Add include guard after existing includes
include_block = '''
#ifdef USE_RA
/* Ra query optimizer planner hook.
 * Defined by the ra_planner extension when loaded via shared_preload_libraries.
 */
#include "ra_planner_hook.h"
#endif /* USE_RA */
'''
src = re.sub(
    r'(#include "optimizer/planner.h"\n)',
    r'\1' + include_block,
    src,
    count=1,
)

# 2. Insert hook call at the top of standard_planner()
hook_call = '''#ifdef USE_RA
    {
        PlannedStmt *ra_result = ra_try_optimize(parse, query_string,
                                                 cursorOptions, boundParams);
        if (ra_result != NULL)
            return ra_result;
    }
#endif /* USE_RA */
'''
src = re.sub(
    r'(PlannedStmt \*\nstandard_planner[^{]+\{)',
    r'\1\n' + hook_call,
    src,
    count=1,
)

open("${PLANNER_C}", "w").write(src)
print("Patch applied inline.")
PYEOF
fi

# ---------------------------------------------------------------------------
# Step 3: Build standard (no-Ra) PostgreSQL
# ---------------------------------------------------------------------------
log "Step 3: Build standard PostgreSQL → ${PREFIX_STD}"
mkdir -p "${PG_REPO}/build-standard"
(
    cd "${PG_REPO}/build-standard"
    ../configure \
        --prefix="${PREFIX_STD}" \
        --with-openssl \
        --with-libxml \
        --with-libxslt \
        --enable-thread-safety \
        --enable-integer-datetimes \
        --disable-debug \
        CFLAGS="-O2" \
        CXXFLAGS="-O2"
    make -j"${JOBS}"
    sudo make install
)
log "  Standard PostgreSQL installed to ${PREFIX_STD}"

# ---------------------------------------------------------------------------
# Step 4: Build Ra-enabled PostgreSQL
# ---------------------------------------------------------------------------
log "Step 4: Build Ra-enabled PostgreSQL → ${PREFIX_RA}"
mkdir -p "${PG_REPO}/build-ra"
(
    cd "${PG_REPO}/build-ra"
    ../configure \
        --prefix="${PREFIX_RA}" \
        --with-openssl \
        --with-libxml \
        --with-libxslt \
        --enable-thread-safety \
        --enable-integer-datetimes \
        --disable-debug \
        CFLAGS="-O2 -DUSE_RA=1" \
        CXXFLAGS="-O2"
    make -j"${JOBS}"
    sudo make install
)
log "  Ra-enabled PostgreSQL installed to ${PREFIX_RA}"

# ---------------------------------------------------------------------------
# Step 5: Build and install the ra_planner extension
# ---------------------------------------------------------------------------
log "Step 5: Build ra_planner extension"
RA_EXT="${RA_REPO}/crates/ra-pg-extension"

if [[ -f "${RA_EXT}/Cargo.toml" ]]; then
    # Build the pgrx extension against the Ra-enabled PostgreSQL
    (
        cd "${RA_EXT}"
        # Set pg_config to point at our custom-built PostgreSQL
        export PATH="${PREFIX_RA}/bin:${PATH}"
        cargo pgrx package --pg-config "${PREFIX_RA}/bin/pg_config"
    )
    log "  Extension package built — see ${RA_EXT}/target/release/extension/"
else
    log "  WARNING: ra-pg-extension not found at ${RA_EXT}"
fi

# ---------------------------------------------------------------------------
# Step 6: Print setup instructions
# ---------------------------------------------------------------------------
cat <<EOF

============================================================
 BUILD COMPLETE
============================================================

Standard PostgreSQL : ${PREFIX_STD}
Ra PostgreSQL       : ${PREFIX_RA}

To start the Ra-enabled cluster:

  export PATH="${PREFIX_RA}/bin:\$PATH"
  initdb -D /var/lib/postgresql/ra-data
  # Add to postgresql.conf:
  #   shared_preload_libraries = 'ra_planner'
  #   ra_planner.enabled = on
  pg_ctl -D /var/lib/postgresql/ra-data start

To start the standard cluster (for benchmarking baseline):

  export PATH="${PREFIX_STD}/bin:\$PATH"
  initdb -D /var/lib/postgresql/standard-data
  pg_ctl -D /var/lib/postgresql/standard-data start

Then run the benchmark harness against both instances:

  cargo run -p ra-bench -- benchmark-job \
    --db postgres://localhost:5432/bench \
    --repetitions 30 \
    --output results/job_ra.json

  PGPORT=5433 cargo run -p ra-bench -- benchmark-job \
    --db postgres://localhost:5433/bench \
    --repetitions 30 \
    --output results/job_standard.json

  cargo run -p ra-bench -- analyze \
    results/job_ra.json results/job_standard.json \
    --output reports/executive_summary.md

============================================================
EOF
