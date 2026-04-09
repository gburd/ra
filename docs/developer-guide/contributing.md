# Contributing to RA

This guide covers development setup, code style, testing requirements, and the contribution process.

## Development Setup

### Prerequisites

**Required:**
- Rust 1.88+ (install via [rustup](https://rustup.rs/))
- Node.js 22 LTS (install via [nvm](https://github.com/nvm-sh/nvm) or [fnm](https://github.com/Schniz/fnm))
- PostgreSQL 16+ (for testing adapters)
- Redis 7+ (for caching tests)

**Optional:**
- Docker (for database containers)
- MySQL 8.4+ (for MySQL adapter tests)
- SQLite 3.40+ (for SQLite adapter tests)

### Initial Setup

**Clone the repository:**

```bash
git clone https://github.com/gregburd/ra.git
cd ra
```

**Install Rust dependencies:**

```bash
cargo build
```

This will compile all workspace crates and download dependencies.

**Install frontend dependencies:**

```bash
cd crates/ra-web/frontend
npm install
cd ../../..
```

**Start test databases (Docker):**

```bash
docker compose -f docker/docker-compose.test.yml up -d
```

This starts PostgreSQL, MySQL, and Redis containers with test data.

**Verify setup:**

```bash
cargo test --workspace
cd crates/ra-web/frontend && npm test
```

All tests should pass on a clean checkout.

## Project Structure

```
ra/
├── crates/                  # Rust workspace crates
│   ├── ra-core/            # Core types and traits
│   ├── ra-parser/          # SQL parser
│   ├── ra-engine/          # Optimization engine
│   ├── ra-adapters/        # Database adapters
│   ├── ra-web/             # Web API server
│   │   └── frontend/       # React frontend
│   ├── ra-cli/             # CLI tool
│   └── ...                 # Other crates
├── docs/                   # Documentation
│   ├── developer-guide/    # Developer docs
│   ├── rfcs/               # Design documents
│   └── examples/           # Usage examples
├── docker/                 # Docker configurations
├── scripts/                # Build and test scripts
└── Cargo.toml             # Workspace manifest
```

## Code Style Guidelines

We follow the standards defined in `~/.claude/CLAUDE.md`. Key points:

### Rust

**Naming conventions:**
- `snake_case` for functions, variables, modules
- `CamelCase` for types, traits, enums
- `SCREAMING_SNAKE_CASE` for constants
- No abbreviations unless universally understood (SQL, HTTP, JSON)

**Function length:**
- Maximum 100 lines per function
- Cyclomatic complexity ≤ 8
- Extract helper functions for complex logic

**Error handling:**
- Libraries: use `thiserror` for typed errors
- Applications: use `anyhow` for context-rich errors
- Never silently ignore errors
- Always provide actionable error messages

**Documentation:**
- Public APIs must have doc comments
- Include examples for non-trivial functions
- Explain WHY, not WHAT (code explains what)

**Example:**

```rust
/// Optimize a relational expression using equality saturation.
///
/// This runs the e-graph optimizer with the configured rule set and
/// extracts the minimum-cost plan. If optimization exceeds the iteration
/// limit, returns the best plan found so far.
///
/// # Errors
///
/// Returns an error if:
/// - The input expression is invalid (e.g., unresolved table references)
/// - The cost function fails (e.g., missing statistics)
///
/// # Example
///
/// ```
/// use ra_engine::Optimizer;
/// use ra_core::RelExpr;
///
/// let optimizer = Optimizer::new();
/// let expr = RelExpr::scan("users");
/// let optimized = optimizer.optimize(&expr)?;
/// ```
pub fn optimize(&self, expr: &RelExpr) -> Result<RelExpr> {
    // Implementation
}
```

**Cargo.toml lints:**

All crates inherit workspace lints from `Cargo.toml`:

```toml
[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
unwrap_used = "deny"
expect_used = "warn"
panic = "deny"
dbg_macro = "deny"
todo = "deny"
```

Run `cargo clippy` before committing. Zero warnings policy.

### TypeScript

**Naming conventions:**
- `camelCase` for functions, variables
- `PascalCase` for types, interfaces, components
- `SCREAMING_SNAKE_CASE` for constants
- Descriptive names (no single-letter variables except loop indices)

**Type safety:**
- Enable all strict TypeScript options
- No `any` types (use `unknown` if needed)
- No `!` non-null assertions (check explicitly)
- No `as` type assertions (fix the types instead)

**tsconfig.json strictness:**

```json
{
  "strict": true,
  "noUncheckedIndexedAccess": true,
  "exactOptionalPropertyTypes": true,
  "noImplicitOverride": true,
  "noPropertyAccessFromIndexSignature": true,
  "verbatimModuleSyntax": true,
  "isolatedModules": true
}
```

**Example:**

```typescript
interface PlanNode {
  operator_type: string;
  cost: number;
  rows: number;
  children: PlanNode[];
}

/**
 * Parse a query execution plan from text format.
 *
 * Supports multiple database plan formats (PostgreSQL JSON,
 * MySQL text, etc.) and converts to a unified structure.
 *
 * @param text - Raw plan text from EXPLAIN output
 * @param engine - Database engine identifier
 * @returns Parsed plan tree
 * @throws Error if plan format is unrecognized
 */
export function parsePlan(text: string, engine: string): PlanNode {
  if (engine.startsWith('postgresql')) {
    return parsePostgresqlPlan(text);
  }
  // ... other engines
  throw new Error(`Unsupported engine: ${engine}`);
}
```

**Linting:**

```bash
cd crates/ra-web/frontend
npx oxlint .
npx oxfmt --check .
```

### General

- 100-character line length
- No trailing whitespace
- Unix line endings (LF)
- UTF-8 encoding
- No commented-out code (delete it)

## Testing Requirements

### Unit Tests

**Rust:**

Tests live in the same file as the code or in `tests/` subdirectory:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_cost() {
        let facts = MockFactsProvider::new();
        facts.set_table_stats("users", TableStats {
            row_count: 1000,
            page_count: 10,
        });

        let cost = scan_cost("users", &facts);
        assert_eq!(cost, 10.0);
    }
}
```

Run tests:

```bash
cargo test --workspace
cargo test -p ra-engine  # Single crate
```

**TypeScript:**

Tests live alongside source files with `.test.ts` suffix:

```typescript
import { describe, it, expect } from 'vitest';
import { parsePlan } from './planParser';

describe('parsePlan', () => {
  it('parses PostgreSQL plan', () => {
    const text = '{"Plan": {"Node Type": "Seq Scan"}}';
    const plan = parsePlan(text, 'postgresql');
    expect(plan.operator_type).toBe('Seq Scan');
  });

  it('throws on invalid engine', () => {
    expect(() => parsePlan('', 'invalid')).toThrow('Unsupported engine');
  });
});
```

Run tests:

```bash
cd crates/ra-web/frontend
npm test
npm test -- --watch  # Watch mode
```

### Integration Tests

Integration tests go in `crates/<crate>/tests/`:

```rust
// crates/ra-adapters/tests/postgres_test.rs

use ra_adapters::{DatabaseAdapter, PostgresAdapter};

#[test]
#[ignore]  // Requires PostgreSQL
fn test_postgres_statistics() {
    let mut adapter = PostgresAdapter::new();
    adapter.connect("postgresql://localhost/testdb").unwrap();

    let stats = adapter.gather_statistics().unwrap();
    assert!(!stats.is_empty());
}
```

Run integration tests:

```bash
cargo test --workspace -- --ignored
```

### Property Tests

Use `proptest` for property-based testing:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_optimizer_preserves_semantics(expr: RelExpr) {
        let optimizer = Optimizer::new();
        let optimized = optimizer.optimize(&expr)?;

        // Verify optimized plan is semantically equivalent
        assert!(semantically_equivalent(&expr, &optimized));
    }
}
```

### Regression Tests

SQL corpus for regression testing:

```
crates/ra-regression/sql/
├── postgresql/
│   ├── basic.sql
│   ├── joins.sql
│   └── window.sql
├── mysql/
│   ├── basic.sql
│   └── joins.sql
└── ...
```

Run regression tests:

```bash
cargo test -p ra-regression
```

## Pull Request Process

### Before Submitting

**1. Create a feature branch:**

```bash
git checkout -b feature/my-feature
```

Use descriptive branch names:
- `feature/add-oracle-adapter` - New feature
- `fix/parser-crash-on-cte` - Bug fix
- `refactor/simplify-cost-model` - Code refactor
- `docs/update-parser-guide` - Documentation

**2. Make incremental commits:**

```bash
git add crates/ra-parser/src/oracle.rs
git commit -m "feat: Add Oracle SQL parser support"
```

Commit message format:
- First line: `type: brief description` (≤72 chars)
- Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`
- Imperative mood: "Add feature" not "Added feature"
- Body (optional): Explain WHY, not WHAT

**3. Run all checks:**

```bash
# Format code
cargo fmt
cd crates/ra-web/frontend && npx oxfmt . && cd ../../..

# Lint
cargo clippy --all-targets --all-features -- -D warnings
cd crates/ra-web/frontend && npx oxlint . && cd ../../..

# Test
cargo test --workspace
cd crates/ra-web/frontend && npm test && cd ../../..

# Check types
cd crates/ra-web/frontend && npx tsc --noEmit && cd ../../..
```

All checks must pass before pushing.

**4. Update documentation:**

- Add/update doc comments for new APIs
- Update relevant docs in `docs/`
- Add examples if introducing new features

**5. Add tests:**

- Unit tests for new functions
- Integration tests for new adapters
- Regression tests for bug fixes

### Submitting

**1. Push to your fork:**

```bash
git push origin feature/my-feature
```

**2. Create pull request:**

Go to GitHub and click "New Pull Request".

**PR description template:**

```markdown
## Summary

Brief description of what this PR does.

## Changes

- Added Oracle SQL parser support
- Implemented OracleAdapter for statistics gathering
- Added integration tests for Oracle

## Testing

- Added unit tests for Oracle grammar
- Added integration tests (require Oracle instance)
- Verified against TPC-H queries

## Checklist

- [x] Code follows style guidelines
- [x] All tests pass
- [x] Documentation updated
- [x] No new warnings
```

**3. Address review feedback:**

- Push additional commits to the same branch
- Do not force-push unless requested
- Respond to all review comments

**4. Merge:**

Once approved, the PR will be merged by a maintainer.

## Bug Reports

### Before Reporting

1. Search existing issues
2. Verify on latest version
3. Reduce to minimal reproduction

### Report Format

```markdown
**Environment:**
- RA version: 0.2.0
- Rust version: 1.88.0
- OS: Ubuntu 24.04

**Description:**
Parser crashes on CTEs with multiple UNION clauses.

**Reproduction:**
```sql
WITH a AS (SELECT 1), b AS (SELECT 2)
SELECT * FROM a UNION SELECT * FROM b;
```

**Expected:**
Parse successfully and optimize.

**Actual:**
```
thread 'main' panicked at 'index out of bounds'
```

**Logs:**
```
[Attach full backtrace]
```
```

## Feature Requests

### Request Format

```markdown
**Feature:** Support for Oracle MATCH_RECOGNIZE

**Use case:**
Pattern matching over sequences of rows for complex event processing.

**Proposed API:**
```sql
SELECT ...
FROM events
MATCH_RECOGNIZE (
  PATTERN (A B+ C)
  DEFINE ...
)
```

**Alternatives considered:**
- Implement as rewrite rules
- Implement as separate operator
- Implement via lateral joins

**Implementation plan:**
1. Add grammar support in ra-parser
2. Add MatchRecognize operator to ra-core
3. Add rewrite rules in ra-engine
4. Add cost model for pattern matching
```

## Coding Principles

### From CLAUDE.md

**No speculative features:**
Don't add features, flags, or configuration unless users actively need them.

**No premature abstraction:**
Don't create utilities until you've written the same code three times.

**Clarity over cleverness:**
Prefer explicit, readable code over dense one-liners.

**Replace, don't deprecate:**
When a new implementation replaces an old one, remove the old one entirely.

**Zero warnings policy:**
Fix every warning from every tool. Clean output is the baseline.

**Bias toward action:**
Decide and move for anything easily reversed. Ask before committing to interfaces or destructive operations.

**Finish the job:**
Handle edge cases. Clean up what you touched. Don't stop at the minimum.

## Development Workflow

### Daily Development

**Start the dev server:**

```bash
# Terminal 1: Backend
cd crates/ra-web
cargo watch -x run

# Terminal 2: Frontend
cd crates/ra-web/frontend
npm run dev

# Terminal 3: Redis
redis-server

# Terminal 4: Test databases
docker compose -f docker/docker-compose.test.yml up
```

Visit http://localhost:5173 for the frontend (proxies to backend on 8000).

### Running Tests

**All tests:**

```bash
cargo test --workspace
cd crates/ra-web/frontend && npm test
```

**Specific crate:**

```bash
cargo test -p ra-parser
cargo test -p ra-engine
```

**Single test:**

```bash
cargo test test_optimizer_pushdown
```

**Watch mode:**

```bash
cargo watch -x test
cd crates/ra-web/frontend && npm test -- --watch
```

### Benchmarks

Run performance benchmarks:

```bash
cargo bench -p ra-engine
```

View results in `target/criterion/`.

### Profiling

**CPU profiling:**

```bash
cargo build --release --bin ra-cli
cargo flamegraph --bin ra-cli -- optimize query.sql
```

**Memory profiling:**

```bash
cargo build --release --bin ra-cli
valgrind --tool=massif target/release/ra-cli optimize query.sql
```

## Common Tasks

### Adding a New Rewrite Rule

1. Add rule to `crates/ra-engine/src/rewrite.rs`
2. Add tests to `crates/ra-engine/tests/rule_test.rs`
3. Document in rule file `rules/my-rule.rra`
4. Add regression test with SQL examples

### Adding a New Database Adapter

See [parsers.md](parsers.md) for detailed instructions.

1. Create profile in `crates/ra-parser/profiles/vendors/`
2. Implement adapter in `crates/ra-adapters/src/`
3. Add tests in `crates/ra-adapters/tests/`
4. Register in web API
5. Add to frontend engine list

### Adding a New Operator

1. Add to `RelExpr` enum in `crates/ra-core/src/algebra.rs`
2. Add parser support in `crates/ra-parser/src/sql_to_relexpr.rs`
3. Add cost function in `crates/ra-engine/src/cost.rs`
4. Add rewrite rules in `crates/ra-engine/src/rewrite.rs`
5. Add tests for parsing, optimization, and execution

### Debugging

**Enable trace logging:**

```bash
RUST_LOG=trace cargo run --bin ra-cli -- optimize query.sql
```

**Debug specific module:**

```bash
RUST_LOG=ra_engine=debug cargo run
```

**Pretty-print RelExpr:**

```rust
use ra_core::RelExpr;

let expr = RelExpr::scan("users");
dbg!(&expr);  // Don't commit dbg! calls
println!("{:#?}", expr);  // Use tracing::debug! instead
```

**Visualize e-graph:**

```rust
use ra_engine::Optimizer;

let optimizer = Optimizer::new();
optimizer.optimize_with_trace(&expr)?;
optimizer.dump_egraph("egraph.dot")?;
// View with: dot -Tpng egraph.dot -o egraph.png
```

## CI/CD

GitHub Actions runs on every push and PR:

```yaml
jobs:
  test:
    - cargo fmt --check
    - cargo clippy -- -D warnings
    - cargo test --workspace

  frontend:
    - npm run lint
    - npm run type-check
    - npm test

  integration:
    - Start PostgreSQL, MySQL, Redis
    - cargo test --workspace -- --ignored
```

All checks must pass before merge.

## Getting Help

- **Documentation:** See `docs/`
- **Examples:** See `docs/examples/`
- **Issues:** GitHub issues for bugs and features
- **Discussions:** GitHub discussions for questions

## License

RA is dual-licensed under MIT OR Apache-2.0. By contributing, you agree to license your contributions under the same terms.

## Code of Conduct

Be respectful. Be constructive. Be professional.

## Further Reading

- [architecture.md](architecture.md) - System architecture overview
- [parsers.md](parsers.md) - Parser system and adding engines
- [RFCs](../rfcs/) - Design documents for major features
- [CLAUDE.md](~/.claude/CLAUDE.md) - Global development standards
