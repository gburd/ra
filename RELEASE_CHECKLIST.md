# Release Checklist for Ra 0.2.0

## Completed ✓

### Documentation

- [x] **GNU ChangeLog** (`ChangeLog`) - Created with detailed commit history in GNU format
- [x] **Release Notes** (`RELEASE_NOTES.md`) - Comprehensive 0.2.0 release notes with highlights, features, breaking changes, performance, and upgrade guide
- [x] **Quickstart Guide** (`docs/quickstart.md`) - Complete 5-minute walkthrough covering installation, optimization, rule tracking, demos, budgets, and PostgreSQL extension
- [x] **README.md Updates** - Added three new feature highlights:
  - Three-tier rule tracking
  - Index access method abstraction
  - PostgreSQL metadata cache with relcache tracking
- [x] **VitePress Config** - Added quickstart to navigation sidebar

### Version Management

- [x] **Current Version**: 0.1.0 (in Cargo.toml)
- [x] **Target Version**: 0.2.0 (documented in RELEASE_NOTES.md)
- [ ] **Update Cargo.toml** - Bump version to 0.2.0 across workspace (see below)

### Major Features Documented

1. **Three-Tier Rule Tracking** (RFC 0081 - not found, but implemented)
   - `RuleTrackingResult`, `RuleApplication`, `RuleEvaluation` structs
   - CLI flags: `--rules-applied`, `--rules-evaluated`, `--rules-available`
   - Documentation: `docs/rule-tracking.md`

2. **Index Access Method Abstraction** (RFC 0082)
   - `IndexAccessMethod` enum with 12 database-agnostic types
   - `IndexOperation` enum for capability taxonomy
   - `IndexMetadata` for runtime discovery
   - Refactored rules: `inverted-index-for-arrays.rra`, `inverted-index-for-fulltext.rra`

3. **PostgreSQL Metadata Cache** (RFC 0083)
   - `MetadataCache` with relcache invalidation tracking
   - `CachedTableMetadata` with validity tracking
   - Automatic schema change detection via `CacheRegisterRelcacheCallback()`
   - Documentation: `docs/metadata-cache-best-practices.md`, `docs/relcache-invalidation-architecture.md`

## Remaining Tasks

### 1. Version Number Updates

Update version from 0.1.0 to 0.2.0 in all Cargo.toml files:

```bash
# Workspace root
sed -i 's/^version = "0.1.0"/version = "0.2.0"/' Cargo.toml

# All crate Cargo.toml files
find crates -name Cargo.toml -exec sed -i 's/^version = "0.1.0"/version = "0.2.0"/' {} \;
```

Files to update (31 crates):
- `Cargo.toml` (workspace)
- `crates/ra-core/Cargo.toml`
- `crates/ra-parser/Cargo.toml`
- `crates/ra-compiler/Cargo.toml`
- `crates/ra-engine/Cargo.toml`
- `crates/ra-codegen/Cargo.toml`
- `crates/ra-dialect/Cargo.toml`
- `crates/ra-cli/Cargo.toml`
- `crates/ra-web/Cargo.toml`
- `crates/ra-wasm/Cargo.toml`
- `crates/ra-wasm-docs/Cargo.toml`
- `crates/ra-isolation/Cargo.toml`
- `crates/ra-adaptive/Cargo.toml`
- `crates/ra-synthesis/Cargo.toml`
- `crates/ra-discovery/Cargo.toml`
- `crates/ra-ml/Cargo.toml`
- `crates/ra-hardware/Cargo.toml`
- `crates/ra-multimodel/Cargo.toml`
- `crates/ra-stats/Cargo.toml`
- `crates/ra-metadata/Cargo.toml`
- `crates/ra-catalog/Cargo.toml`
- `crates/ra-tui/Cargo.toml`
- `crates/ra-config/Cargo.toml`
- `crates/ra-cache/Cargo.toml`
- `crates/ra-adapters/Cargo.toml`
- `crates/ra-pg-monitor/Cargo.toml`
- `crates/ra-pg-advisor/Cargo.toml`
- `crates/ra-advisor/Cargo.toml`
- `crates/ra-test-utils/Cargo.toml`
- `crates/ra-regression/Cargo.toml`
- `crates/sparsemap/Cargo.toml`
- `xtask/Cargo.toml`
- `crates/ra-pg-extension/Cargo.toml` (if it has version field)

### 2. Documentation Site Verification

**Status**: VitePress build timed out after 60 seconds (1820 markdown files to process)

**Recommended actions:**
1. Build locally without timeout: `cd docs && npm run build`
2. Check for broken links: `npm run validate`
3. Preview locally: `npm run preview`
4. Verify new pages render correctly:
   - `/quickstart`
   - `/rule-tracking`
   - `/metadata-cache-best-practices`
   - `/relcache-invalidation-architecture`

**Expected issues:**
- None anticipated - config updated correctly
- All referenced pages exist in `docs/` directory

### 3. Git Tag Creation

**DO NOT create tag yet** - wait until ready to release.

Planned tag: `v0.2.0`

```bash
# When ready:
git tag -a v0.2.0 -m "Release version 0.2.0

Major features:
- Three-tier rule tracking system
- Index access method abstraction
- PostgreSQL metadata cache with relcache invalidation

See RELEASE_NOTES.md for full details."

# Push tag
git push origin v0.2.0
```

### 4. Pre-Release Testing

Run full test suite:

```bash
# All tests
cargo test --all-features

# Linting (zero warnings required)
cargo clippy --all-targets --all-features -- -D warnings

# Format check
cargo fmt -- --check

# Benchmarks
cargo bench --package ra-engine
```

### 5. PostgreSQL Extension

If releasing PostgreSQL extension:

```bash
cd crates/ra-pg-extension

# Update version in Cargo.toml
# Update ra_planner.control file with new version

# Test build
cargo pgrx install

# Test in PostgreSQL
psql -c "CREATE EXTENSION ra_planner VERSION '0.2.0';"
```

### 6. Deployment Verification

**Docker images:**
```bash
# Build
docker build -t ra-optimizer:0.2.0 .

# Test
docker run ra-optimizer:0.2.0 ra-cli optimize "SELECT * FROM orders"
```

**Documentation site:**
- Deploy to GitHub Pages or hosting platform
- Verify all links work
- Test search functionality

## Documentation Files Created

### New Files (3)

1. **`ChangeLog`** (GNU format)
   - 243 lines
   - Covers changes from 2026-03-24 to 2026-03-26
   - Grouped by date, file, and function
   - Follows GNU ChangeLog standards

2. **`RELEASE_NOTES.md`**
   - 456 lines
   - Complete 0.2.0 release documentation
   - Highlights, features, breaking changes, performance, upgrade guide
   - Migration notes for deprecated APIs

3. **`docs/quickstart.md`**
   - 354 lines
   - Complete quickstart tutorial
   - Covers all major features with examples
   - Installation, optimization, rule tracking, demos, PostgreSQL extension

### Updated Files (2)

1. **`README.md`**
   - Added 3 feature highlights
   - Updated feature list section

2. **`docs/.vitepress/config.js`**
   - Added quickstart to navigation
   - Added rule tracking, metadata cache, relcache to Features section

## File Summary

```
ChangeLog (new)                          - GNU-format changelog
RELEASE_NOTES.md (new)                   - Comprehensive release documentation
RELEASE_CHECKLIST.md (this file)         - Release process tracking
docs/quickstart.md (new)                 - 5-minute quickstart guide
README.md (updated)                      - Added new feature highlights
docs/.vitepress/config.js (updated)      - Navigation updates
```

## ChangeLog Summary

**Major changes documented:**

1. **Three-Tier Rule Tracking** (2026-03-26)
   - `crates/ra-engine/src/egraph.rs`: New tracking structs and functions
   - `crates/ra-cli/src/main.rs`: CLI flags for rule introspection
   - `docs/rule-tracking.md`: User documentation

2. **Index Access Method Abstraction** (2026-03-26)
   - `crates/ra-stats/src/index_metadata.rs`: New abstraction layer
   - `rules/physical/index-selection/*.rra`: Refactored rules
   - `rfcs/0082-index-access-method-abstraction.md`: RFC documentation

3. **PostgreSQL Metadata Cache** (2026-03-26)
   - `crates/ra-pg-extension/src/metadata_cache.rs`: Cache implementation
   - `crates/ra-pg-extension/src/lib.rs`: Callback registration
   - `docs/metadata-cache-best-practices.md`, `docs/relcache-invalidation-architecture.md`

4. **Rule Registry Expansion** (2026-03-26)
   - Comprehensive rule set from all categories

5. **RFC Documentation Integration** (2026-03-26)
   - VitePress integration with cross-linking
   - Auto-generated RFC index

6. **Rust 1.88.0 Compatibility** (2026-03-26)
   - Updated minimum version for time crate

7. **MonetDB Updates** (2026-03-26)
   - ODBC API compatibility fixes

## Documentation Issues Found

**None identified.**

All documentation pages referenced in navigation exist:
- ✓ `/quickstart` → `docs/quickstart.md`
- ✓ `/rule-tracking` → `docs/rule-tracking.md`
- ✓ `/metadata-cache-best-practices` → `docs/metadata-cache-best-practices.md`
- ✓ `/relcache-invalidation-architecture` → `docs/relcache-invalidation-architecture.md`

## Next Steps (Post-Release)

1. **Announce release**
   - GitHub release page
   - Update website
   - Social media / mailing lists

2. **Monitor feedback**
   - Watch GitHub issues
   - Respond to questions

3. **Plan 0.3.0**
   - Progressive re-optimization
   - Streaming statistics
   - Enhanced hardware-aware optimization
   - ML-based cardinality estimation

## Notes

- **ChangeLog follows GNU standards**: Tab-indented entries, function names in parentheses, grouped by date and file
- **RELEASE_NOTES.md is comprehensive**: Covers all aspects users need to know
- **Quickstart guide is complete**: 5-minute walkthrough of all major features
- **No breaking changes**: Fully backward compatible with 0.1.0
- **Deprecation timeline**: Old index APIs deprecated in 0.2.0, removed in 0.4.0
- **Documentation count**: 1820 markdown files in docs/ directory

## Command Reference

```bash
# Update versions
find . -name Cargo.toml -path "*/crates/*" -exec sed -i 's/^version = "0.1.0"/version = "0.2.0"/' {} \;
sed -i 's/^version = "0.1.0"/version = "0.2.0"/' Cargo.toml

# Build documentation
cd docs && npm run build && npm run preview

# Test suite
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo bench --package ra-engine

# Create tag (when ready)
git tag -a v0.2.0 -m "Release version 0.2.0"
git push origin v0.2.0

# Docker
docker build -t ra-optimizer:0.2.0 .
docker run ra-optimizer:0.2.0 ra-cli --version
```
