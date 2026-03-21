# RA Test Utilities Integration Tests

This directory contains integration tests for the RA project.

## Link Validation Test

The `link_validation.rs` test validates that all documentation links are not broken.

### Coverage

The test scans the following locations for markdown links:

- **Documentation files**: `./docs/**/*.md`
- **Rule files**: `./rules/**/*.rra`, `./rules/**/*.md`
- **Research files**: `./research/**/*.md`
- **Root markdown files**: `README.md`, `CONTRIBUTING.md`, etc.
- **Rust doc comments**: `./crates/**/*.rs` (doc comment links to `.md` files)

### Link Types Validated

- `[text](path/to/file.md)` - Standard markdown links
- `[text](../relative/path.md)` - Relative path links
- `[text](/absolute/from/root.md)` - Absolute path links from repo root
- `` [`code`](path.md) `` - Rust doc comment links

### Link Types Ignored

- External URLs: `[text](https://example.com)`
- Anchor links: `[text](#section)`
- HTTP/HTTPS URLs

### Running the Test

```bash
# Run link validation test
cargo test --package ra-test-utils --test link_validation

# Run with output
cargo test --package ra-test-utils --test link_validation -- --nocapture

# Run only the main test
cargo test --package ra-test-utils --test link_validation -- test_documentation_links --nocapture
```

### CI Integration

The test runs automatically in CI via:

1. **Main CI workflow** (`ci.yml`): Runs as part of `cargo test --all-features`
2. **Dedicated workflow** (`docs-link-validation.yml`): Runs on documentation changes

The test generates warnings but does not fail the build when broken links are found.

### Output Format

When broken links are found, the test outputs:

```
⚠ Found X broken links in Y files:
================================================================================

path/to/file.md:
  Line 42: [Link Text](broken/path.md) - Target does not exist: /full/path/to/broken/path.md
  Line 55: [Another](../missing.md) - Target does not exist: /full/path/to/missing.md

path/to/another.md:
  Line 10: [Bad Link](dir) - Target is a directory: /full/path/to/dir

================================================================================
Summary: Y/Z files have broken links
```

### Implementation Details

The test:

1. Discovers all documentation files in the repository
2. Parses each file for markdown links using regex
3. Resolves relative and absolute paths
4. Checks if target files exist
5. Reports broken links with file:line information
6. Generates warnings (does not fail)

### Adding New Scan Locations

To scan additional directories, edit the `find_documentation_files()` function in `link_validation.rs`:

```rust
let scan_dirs = vec!["docs", "research", "rules", "your-new-dir"];
```
