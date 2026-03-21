# Contributing to RA

Contributions are welcome. This document covers development standards,
testing requirements, and the contribution workflow.

## Development Setup

```bash
# Clone and build
git clone https://github.com/gregburd/ra.git
cd ra
cargo build
cargo test --all-features
```

## Code Standards

### Rust Style

- Rust 1.75+ with edition 2021
- Zero warnings: `cargo clippy --all-targets --all-features -- -D warnings`
- Format: `cargo fmt`
- Functions under 100 lines, cyclomatic complexity under 8
- 5 or fewer positional parameters
- `thiserror` for library errors, `anyhow` for application errors
- `tracing` for logging (not `println!`)

### Lints

The workspace enforces strict clippy lints including `pedantic` (warn),
`unwrap_used` (deny), `panic` (deny), `dbg_macro` (deny), `todo`
(deny), and `print_stdout`/`print_stderr` (deny). See the root
`Cargo.toml` for the full list.

## Areas of Contribution

1. **Rule Extraction** -- Extract optimization rules from database
   source code (PostgreSQL, MySQL, DuckDB, SQLite)
2. **Rule Writing** -- Document optimizations in `.rra` literate format
3. **Testing** -- Add test cases, property-based tests (`proptest`),
   and differential tests against reference databases
4. **Verification** -- Write TLA+ specifications for rule correctness
5. **Documentation** -- Improve guides, examples, and API docs
6. **Dialect Support** -- Add SQL dialect translations
7. **Hardware Rules** -- Add rules for new accelerators (GPU, FPGA)

## Writing Rules

Rules use the `.rra` literate format. See the
[Rule Authoring Guide](guides/rule-authoring.md) for the full
specification. Each rule file must include:

- YAML frontmatter with `id`, `name`, `category`
- Description of the transformation
- Formal relational algebra notation
- Implementation (egg rewrite rules)
- Test cases (positive and negative)

## Testing

```bash
# Run all tests
cargo test --all-features

# Run specific crate tests
cargo test -p ra-core

# Run benchmarks
cargo bench

# Validate all rules
cargo run --bin ra-cli -- validate rules/

# Run TLA+ formal verification
./scripts/run-tla.sh
```

### Test Requirements

- Test behavior, not implementation
- Test edge cases and error paths
- Mock only external boundaries (network, filesystem)
- Use `proptest` for parser and algorithm tests

## Documentation

- Update docs when changing public APIs or adding features
- Follow the documentation structure in [docs/README.md](README.md)
- Place guides in `docs/guides/`, concepts in `docs/concepts/`,
  feature docs in `docs/features/`

### Building Documentation Locally

```bash
cd docs
npm install
npm run dev  # Development server at http://localhost:5173
npm run build  # Production build
```

### Documentation Deployment

Documentation is automatically deployed to:
- **GitHub Pages**: https://gregburd.github.io/ra/ (automatic on push to main)
- **Codeberg Pages**: https://[username].codeberg.page/ra/ (requires setup, see [deployment.md](deployment.md))

See [deployment.md](deployment.md) for detailed deployment configuration.

## Commit Standards

- Imperative mood, 72-character subject line limit
- One logical change per commit
- Use feature branches and pull requests
- Never push directly to main

## License

Contributions are dual-licensed under MIT and Apache 2.0.
