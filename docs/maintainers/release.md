# Release Process

This document describes how to cut a new release of RA, including versioning, changelog generation, CI validation, and publishing.

---

## Versioning

RA follows [Semantic Versioning](https://semver.org/):

- **Major** (`X.0.0`): Breaking API changes to `ra-core` public types
- **Minor** (`0.X.0`): New features, new crates, new rules, non-breaking API additions
- **Patch** (`0.0.X`): Bug fixes, documentation updates, rule corrections

The workspace version is defined in the root `Cargo.toml`:

```toml
[workspace.package]
version = "0.1.0"
```

All workspace crates inherit this version via `version.workspace = true`. The excluded crate `ra-pg-extension` maintains its own version in its `Cargo.toml`.

---

## Pre-Release Checklist

Before cutting a release, verify the following:

### 1. All CI Checks Pass

```bash
# Formatting
cargo fmt -- --check

# Linting (zero warnings)
cargo clippy --all-targets --all-features -- -D warnings

# Tests
cargo test --all-features

# Rule validation
ra-cli validate rules/
```

### 2. No Open P0 Bugs

Check [bugs.md](./bugs.md) and the [issue tracker](https://codeberg.org/gregburd/ra/issues) for any P0 (critical) issues.

### 3. Documentation is Current

- API docs reflect current public types
- Changelog is updated (see below)
- Breaking changes are documented in migration notes

### 4. Benchmarks Show No Regressions

```bash
# Run benchmarks and compare against baseline
cargo bench -p ra-engine

# Check specific benchmark suites
cargo bench -p ra-engine --bench optimizer
cargo bench -p ra-engine --bench resource_budgets
cargo bench -p ra-engine --bench tpch_distributed
```

### 5. PostgreSQL Extension (if applicable)

If the release includes changes to `ra-pg-extension`:

```bash
cd crates/ra-pg-extension
cargo pgrx test pg17
```

---

## Changelog

### Format

The changelog follows [Keep a Changelog](https://keepachangelog.com/) format in `CHANGELOG.md`:

```markdown
## [0.2.0] - 2026-04-01

### Added
- New semi-join reduction rules (#42)
- Runtime filter pushdown with Bloom filters (RFC 0045)

### Changed
- Cost model now uses histogram data for selectivity estimates

### Fixed
- Incorrect cardinality estimate for LEFT JOIN with NULL predicates

### Removed
- Deprecated `legacy_cost_model` feature flag
```

### Categories

- **Added**: New features, rules, crates
- **Changed**: Non-breaking changes to existing functionality
- **Deprecated**: Features marked for removal (avoid when possible; prefer direct removal)
- **Removed**: Features removed in this release
- **Fixed**: Bug fixes
- **Security**: Vulnerability fixes

### Generating the Changelog

Review the commit log since the last release:

```bash
# List commits since last tag
git log v0.1.0..HEAD --oneline

# Group by conventional commit type
git log v0.1.0..HEAD --oneline | grep "^.*feat:" | sort
git log v0.1.0..HEAD --oneline | grep "^.*fix:" | sort
```

Write the changelog entries manually. Automated generation can miss context; human review ensures accuracy.

---

## Release Steps

### 1. Create a Release Branch

```bash
git checkout main
git pull origin main
git checkout -b release/v0.2.0
```

### 2. Update Version Numbers

Update the workspace version in the root `Cargo.toml`:

```toml
[workspace.package]
version = "0.2.0"
```

Update `ra-pg-extension/Cargo.toml` if it has changes:

```toml
[package]
version = "0.2.0"
```

Verify the version propagated:

```bash
cargo metadata --format-version 1 | jq '.packages[] | select(.name | startswith("ra-")) | {name, version}'
```

### 3. Update Changelog

Add the new version section to `CHANGELOG.md` with today's date and all changes since the last release.

### 4. Run Full Validation

```bash
# Full test suite
cargo test --all-features

# Clippy
cargo clippy --all-targets --all-features -- -D warnings

# Build release binaries
cargo build --release

# Documentation build
cd docs && npm run build && cd ..
```

### 5. Commit and Tag

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md crates/ra-pg-extension/Cargo.toml
git commit -m "release: v0.2.0"

# Create annotated tag
git tag -a v0.2.0 -m "Release v0.2.0"
```

### 6. Merge to Main

```bash
# Push the release branch
git push origin release/v0.2.0

# Create PR, get review, merge
# After merge:
git checkout main
git pull origin main
git push origin v0.2.0
```

### 7. Build Release Artifacts

Build binaries for each target platform:

```bash
# Linux (x86_64)
cargo build --release --target x86_64-unknown-linux-gnu

# macOS (ARM)
cargo build --release --target aarch64-apple-darwin

# macOS (Intel)
cargo build --release --target x86_64-apple-darwin

# WASM
cd crates/ra-wasm-docs
wasm-pack build --target web --release
```

Or use the Nix package for reproducible builds:

```bash
nix build
```

### 8. Create Release on Codeberg/GitHub

1. Go to the repository releases page
2. Create a new release from the `v0.2.0` tag
3. Title: `v0.2.0`
4. Body: Copy the changelog section for this version
5. Attach binary artifacts

### 9. Publish Crates (when ready for crates.io)

Publish in dependency order. Workspace crates must be published individually:

```bash
cargo publish -p ra-core
cargo publish -p ra-parser
cargo publish -p ra-compiler
cargo publish -p ra-stats
cargo publish -p ra-hardware
cargo publish -p ra-metadata
cargo publish -p ra-ml
cargo publish -p ra-engine
cargo publish -p ra-codegen
cargo publish -p ra-dialect
cargo publish -p ra-config
cargo publish -p ra-cache
cargo publish -p ra-catalog
cargo publish -p ra-adaptive
cargo publish -p ra-isolation
cargo publish -p ra-multimodel
cargo publish -p ra-synthesis
cargo publish -p ra-discovery
cargo publish -p ra-adapters
cargo publish -p ra-regression
cargo publish -p ra-test-utils
cargo publish -p ra-pg-monitor
cargo publish -p ra-pg-advisor
cargo publish -p ra-wasm
cargo publish -p ra-cli
cargo publish -p ra-tui
cargo publish -p ra-web
```

::: warning
As of v0.1.0, RA is not yet published to crates.io. This section documents the intended process for when the project reaches that stage.
:::

### 10. Deploy Documentation

Documentation deploys automatically on push to `main`:

- **GitHub Pages**: triggered by `.github/workflows/deploy-docs.yml`
- **Netlify**: triggered by git push (configured in `netlify.toml`)

Verify the deployed documentation reflects the new version.

---

## Hotfix Process

For urgent fixes to a released version:

```bash
# Branch from the release tag
git checkout -b hotfix/v0.2.1 v0.2.0

# Make the fix
# ...

# Bump patch version
# Update Cargo.toml: version = "0.2.1"
# Update CHANGELOG.md

git commit -m "release: v0.2.1"
git tag -a v0.2.1 -m "Hotfix release v0.2.1"
git push origin hotfix/v0.2.1 v0.2.1

# Merge the fix back to main
git checkout main
git merge hotfix/v0.2.1
git push origin main
```

---

## PostgreSQL Extension Release

The PostgreSQL extension follows a separate release cycle because it depends on specific PostgreSQL major versions.

### Building the Extension

```bash
cd crates/ra-pg-extension

# Build for a specific PostgreSQL version
cargo pgrx package --pg-config $(which pg_config)
```

This produces a directory with the shared library and SQL extension files, suitable for packaging as a `.deb`, `.rpm`, or direct installation.

### Installing the Extension

```bash
# Install directly into the current PostgreSQL installation
cargo pgrx install --release

# Then in PostgreSQL:
# CREATE EXTENSION ra;
```

### Version Compatibility Matrix

| RA Version | PostgreSQL Versions | pgrx Version |
|------------|-------------------|--------------|
| 0.1.x | 13, 14, 15, 16, 17, 18 | 0.17.0 |

---

## Release Cadence

RA does not follow a fixed release schedule. Releases are cut when:

- A significant set of new features or rules is ready
- A critical bug fix is needed
- An RFC implementation is complete and tested

---

## Related Resources

- **[Build & Install](./build.md)** - Building from source
- **[Bugs & Issues](./bugs.md)** - Bug tracking
- **[Chores & Tasks](./chores.md)** - Task tracking
- **[RFCs](./rfcs/)** - Feature proposals and their implementation status
