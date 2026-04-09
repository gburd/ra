# Developer Guide

This directory contains comprehensive documentation for RA developers.

## Quick Start

1. **[Contributing Guide](contributing.md)** - Start here
   - Development setup and prerequisites
   - Code style guidelines
   - Testing requirements
   - Pull request process

2. **[Architecture Overview](architecture.md)** - Understand the system
   - System architecture and component overview
   - Core types and data flow
   - Frontend and backend architecture
   - Technology stack

3. **[Parser System](parsers.md)** - Add database support
   - How the parser system works
   - Adding a new database engine
   - Profile system and dialect detection
   - Testing parsers

## Document Overview

### architecture.md

Comprehensive system architecture documentation covering:
- System components (ra-core, ra-parser, ra-engine, ra-adapters, ra-web)
- Frontend architecture (React, TypeScript, D3.js visualizations)
- Backend architecture (Rocket API, Redis caching)
- Data flow diagrams
- Technology stack
- Deployment architecture
- Performance characteristics
- Extension points

**Read this to:** Understand how RA works end-to-end.

### parsers.md

Complete guide to the parser system:
- Profile-based dialect support
- Grammar extension system
- Adding new database engines (step-by-step)
- SQL to RelExpr conversion rules
- Testing strategies
- Complete example: PostgreSQL extension support

**Read this to:** Add support for a new database or extension.

### contributing.md

Development workflow and contribution guidelines:
- Development environment setup
- Code style standards (Rust and TypeScript)
- Testing requirements (unit, integration, property, regression)
- Pull request process
- Bug report format
- Feature request format
- Common development tasks
- CI/CD pipeline

**Read this to:** Start contributing to RA.

## Additional Resources

### Documentation

- [User Guide](../user-guide/) - User-facing documentation
- [API Reference](../reference/) - API endpoint documentation
- [RFCs](../rfcs/) - Design documents for major features
- [Examples](../examples/) - Usage examples and tutorials

### Codebase

- [Cargo.toml](/Cargo.toml) - Workspace configuration
- [ra-core](/crates/ra-core/) - Core types and traits
- [ra-parser](/crates/ra-parser/) - SQL parser
- [ra-engine](/crates/ra-engine/) - Optimization engine
- [ra-adapters](/crates/ra-adapters/) - Database adapters
- [ra-web](/crates/ra-web/) - Web API and frontend

### External Resources

- [sqlparser-rs](https://docs.rs/sqlparser/) - SQL parser library
- [egg](https://docs.rs/egg/) - E-graph library for equality saturation
- [Rocket](https://rocket.rs/) - Web framework
- [React](https://react.dev/) - Frontend framework
- [D3.js](https://d3js.org/) - Data visualization

## Quick Reference

### Build Commands

```bash
# Build all crates
cargo build

# Build specific crate
cargo build -p ra-engine

# Build frontend
cd crates/ra-web/frontend && npm run build

# Build for release
cargo build --release
```

### Test Commands

```bash
# Run all tests
cargo test --workspace

# Run specific crate tests
cargo test -p ra-parser

# Run integration tests
cargo test --workspace -- --ignored

# Run frontend tests
cd crates/ra-web/frontend && npm test
```

### Lint Commands

```bash
# Rust
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings

# TypeScript
cd crates/ra-web/frontend
npx oxfmt .
npx oxlint .
```

### Development Server

```bash
# Backend
cd crates/ra-web && cargo run

# Frontend (with hot reload)
cd crates/ra-web/frontend && npm run dev

# Redis
redis-server

# Test databases
docker compose -f docker/docker-compose.test.yml up
```

## Common Workflows

### Adding a New Feature

1. Create feature branch: `git checkout -b feature/my-feature`
2. Implement feature with tests
3. Run all checks: `cargo test && cargo clippy && cargo fmt`
4. Update documentation
5. Submit pull request

### Fixing a Bug

1. Create bug fix branch: `git checkout -b fix/bug-description`
2. Add regression test that reproduces the bug
3. Fix the bug
4. Verify test passes
5. Submit pull request

### Adding a Database Engine

1. Read [parsers.md](parsers.md) section "Adding a New Database Engine"
2. Create vendor profile in `ra-parser/profiles/vendors/`
3. Implement adapter in `ra-adapters/src/`
4. Add tests
5. Register in web API and frontend
6. Submit pull request

### Optimizing Performance

1. Write benchmark in `benches/`
2. Run baseline: `cargo bench`
3. Implement optimization
4. Run benchmark again and compare
5. Document performance improvement in PR

## Getting Help

- **Questions:** Open a GitHub Discussion
- **Bugs:** Open a GitHub Issue with reproduction steps
- **Features:** Open a GitHub Issue with use case and design proposal
- **Code review:** Tag maintainers in your PR

## Standards and Guidelines

All code must adhere to the standards in `~/.claude/CLAUDE.md`:

- Zero warnings policy
- Maximum 100 lines per function
- Cyclomatic complexity ≤ 8
- Comprehensive documentation on public APIs
- Test coverage for all new code
- No speculative features
- Clear, actionable error messages

## Project Status

RA is under active development. Current focus areas:

- **Phase 4:** Docker deployment and database test fixtures
- **Phase 5:** Backend API improvements and frontend enhancements
- **Phase 6:** Advanced optimization features (timeline, adaptive, federated)

See [GitHub Issues](https://github.com/gregburd/ra/issues) for current priorities.

## License

RA is dual-licensed under MIT OR Apache-2.0.

---

**Last updated:** 2026-04-08
