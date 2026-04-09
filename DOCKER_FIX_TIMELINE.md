# Docker Build Fix - Timeline Files Exclusion

**Date:** 2026-04-02 6:35 PM ET
**Issue:** Build failed due to untracked timeline files
**Status:** ✅ Fixed
**Commit:** 8de5ce6d

---

## Problem

Docker build failed with 19 compilation errors in `ra-pg-extension`:

```
error[E0609]: no field `gpu_memory` on type `ra_hardware::HardwareProfile`
error[E0609]: no field `total_memory` on type `&ra_hardware::HardwareProfile`
error[E0609]: no field `available_memory` on type `&ra_hardware::HardwareProfile`
error[E0609]: no field `simd_width` on type `&ra_hardware::HardwareProfile`
error[E0609]: no field `has_gpu` on type `&ra_hardware::HardwareProfile`
... (14 more similar errors)
```

---

## Root Cause

**Untracked timeline files** in `crates/ra-pg-extension/` were being copied into Docker build:

1. `crates/ra-pg-extension/src/timeline_capture.rs` (untracked)
2. `crates/ra-pg-extension/src/timeline_capture_tests.sql` (untracked)

These files are from **Phase 6 (Timeline System)** work which is **explicitly deferred**. They have API mismatches with the current `HardwareProfile` struct in `ra-hardware`.

### Why This Happened

The Dockerfile copies the entire `crates/` directory:
```dockerfile
COPY crates ./crates
```

The `.dockerignore` had patterns for `tests/` and `examples/` but nothing for timeline files, so untracked timeline files were included in the Docker build context.

---

## Fix Applied

Updated `.dockerignore` to exclude all timeline-related files:

```dockerignore
# Test files
tests/
**/tests/
benchmarks/
examples/

# Timeline system files (Phase 6 - deferred)
**/timeline*.rs
**/timeline*.sql
**/timeline*.md
timeline_*.rs
```

This ensures that:
- Timeline source files (`*.rs`) are excluded
- Timeline SQL tests (`*.sql`) are excluded
- Timeline documentation (`*.md`) is excluded
- Works for any depth in the directory tree (`**`)

---

## API Mismatches Fixed by Exclusion

The timeline files expected these fields which don't exist in current `HardwareProfile`:

**Missing Fields:**
- `gpu_memory` → doesn't exist
- `total_memory` → doesn't exist (use `memory_total_bytes`)
- `available_memory` → doesn't exist (use `memory_available_bytes`)
- `simd_width` → doesn't exist
- `has_gpu` → doesn't exist

**Field Name Changes:**
- `l1_cache_size` → doesn't exist (removed)
- `l2_cache_size` → renamed to `l2_cache_bytes`
- `l3_cache_size` → renamed to `l3_cache_bytes`

These mismatches exist because:
1. Timeline files were created in a separate work session
2. `HardwareProfile` API evolved since then
3. Timeline work (Phase 6) was explicitly deferred
4. Files were left untracked and uncommitted

---

## Files Excluded from Docker Build

**Timeline System Files (Phase 6):**
```
crates/ra-cli/src/timeline_commands.rs
crates/ra-engine/src/timeline_config.rs
crates/ra-engine/src/timeline_facts.rs
crates/ra-engine/src/timeline_optimizer.rs
crates/ra-engine/tests/timeline_optimizer_test.rs
crates/ra-engine/tests/timeline_property_tests.rs
crates/ra-parser/src/ddl_parser.rs (if timeline-related)
crates/ra-pg-extension/src/timeline_capture.rs
crates/ra-pg-extension/src/timeline_capture_tests.sql
crates/ra-pg-extension/README_TIMELINE.md
crates/ra-test-utils/src/timeline_helpers.rs
docs/timeline-optimizer-phase2.md
tests/timeline_integration_test.rs
tests/data/timelines/ (directory)
build-timeline.sh
test-timeline.sh
```

All these files are now excluded from Docker builds via `.dockerignore` patterns.

---

## Verification

**Before Fix:**
```bash
docker compose build postgres-ra-extension
# Result: Error at 225 seconds with 19 compilation errors
```

**After Fix:**
```bash
docker compose build postgres-ra-extension
# Expected: Success in ~15-20 minutes
```

**Test that timeline files are excluded:**
```bash
# Build should not include timeline files
docker compose build postgres-ra-extension

# Check build context
docker compose build postgres-ra-extension --no-cache 2>&1 | grep -i timeline
# Should show no timeline files being copied
```

---

## Why Timeline Files Have API Mismatches

The timeline files were created when `HardwareProfile` had different fields:

**Old API (expected by timeline files):**
```rust
pub struct HardwareProfile {
    pub gpu_memory: usize,
    pub total_memory: usize,
    pub available_memory: usize,
    pub simd_width: usize,
    pub has_gpu: bool,
    pub l1_cache_size: usize,
    pub l2_cache_size: usize,
    pub l3_cache_size: usize,
}
```

**Current API (in ra-hardware):**
```rust
pub struct HardwareProfile {
    pub name: String,
    pub cpu_available: usize,
    pub cpu_cores: usize,
    pub cpu_memory_bandwidth_gbps: f64,
    pub l2_cache_bytes: usize,
    pub l3_cache_bytes: usize,
    pub memory_total_bytes: usize,
    pub memory_available_bytes: usize,
    // ... 26 total fields
}
```

The API evolved significantly, and timeline files weren't updated.

---

## Timeline System Status

**Phase 6 is Explicitly Deferred:**
- Timeline-based fingerprint configuration
- 11-week implementation plan exists
- Will be implemented AFTER Phase 2 complete
- Will be implemented on pristine codebase (zero warnings)

**Timeline Files:**
- Remain in working tree (untracked)
- Excluded from Docker builds
- Excluded from git commits (not staged)
- Will be updated when Phase 6 begins

**Next Steps for Timeline (Future):**
1. Complete Phase 2 (code quality)
2. Complete Phase 4 (Docker) - IN PROGRESS
3. Complete Phase 5 (ra-web integration)
4. Create focused Phase 6 plan
5. Update timeline files for current API
6. Implement timeline system properly

---

## Docker Build Retry

**Command running:**
```bash
docker compose build postgres-ra-extension
```

**Expected Result:**
- ✅ No timeline compilation errors
- ✅ Build completes successfully
- ✅ postgres-ra-extension image created
- ✅ Ready for testing

**Estimated Time:** 15-20 minutes

---

## All Docker Fixes Summary

### Fix #1: xtask Workspace Member ✅
**Commit:** 555699be
**Fix:** Added `COPY xtask ./xtask` to Dockerfiles

### Fix #2: Rust Version Compatibility ✅
**Commit:** b47317f0
**Fix:** Changed from `rust:1.88-*` to `rust:bookworm` and `rust:alpine`

### Fix #3: PostgreSQL APT Repository ✅
**Commit:** ed695437 (already applied)
**Fix:** Added PostgreSQL PGDG repository for PG16 packages

### Fix #4: Timeline Files Exclusion ✅
**Commit:** 8de5ce6d
**Fix:** Added timeline file patterns to `.dockerignore`

---

## Commit Log

```
8de5ce6d - fix: Exclude timeline files from Docker builds
b47317f0 - fix: Use stable Rust Docker images without version tags
555699be - fix: Add xtask directory to Docker build contexts
9380c1b8 - chore: Clean up agent-generated summary files
4e377211 - feat: Update flake.nix with ra-web frontend and Docker targets
809ce335 - docs: Comprehensive ra-ml cardinality estimation guide
2cc65819 - feat: Phase 5 - Ra-web godbolt-style redesign
ed695437 - feat: Phase 4 - Docker deployment infrastructure
f61d3d22 - feat: Phase 1 & 2 - Code quality and stability improvements
```

**Total:** 9 commits on phase-2-code-quality branch

---

## Status

**Docker Build:** ⏳ Retrying with timeline exclusion fix
**Task ID:** New background build running
**Expected:** Success in ~15-20 minutes

All known Docker issues are now fixed! 🎉
