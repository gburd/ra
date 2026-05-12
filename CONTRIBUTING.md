# Contributing to Relational Algebra Rule System

Thank you for your interest in contributing! This project aims to be the definitive resource for relational algebra transformation rules, and we welcome contributions of all kinds.

## Ways to Contribute

### 1. Add or Improve Rules

The most valuable contributions are new rules or improvements to existing ones:

- Extract rules from database source code
- Document undocumented optimizations
- Add test cases to existing rules
- Improve rule documentation
- Fix incorrect rules

See [docs/guides/rule-authoring.md](docs/guides/rule-authoring.md) for the complete guide.

### 2. Propose Major Features (RFC Process)

Major features, breaking changes, and architectural decisions require an RFC (Request for Comments). See the [RFC Process Guide](rfcs/README.md) for full details.

Quick steps:
1. Copy `rfcs/TEMPLATE.md` to `rfcs/NNNN-feature-name.md`
2. Fill out the template and submit a PR with `[RFC]` prefix
3. Participate in discussion (minimum 7 days)
4. Implement once accepted

See the [RFC Index](rfcs/INDEX.md) for all existing RFCs.

### 3. Code Contributions

- Implement features from the [ROADMAP](ROADMAP.md)
- Fix bugs
- Improve performance
- Add tests
- Improve documentation

### 4. Documentation

- Write tutorials
- Create examples
- Improve API documentation
- Translate documentation
- Record video tutorials

### 5. Testing and Verification

- Write property-based tests
- Add integration tests
- Create benchmarks
- Write TLA+ specifications
- Perform differential testing

### 6. Community Support

- Answer questions in discussions
- Review pull requests
- Help with issue triage
- Share the project
- Write blog posts

## Getting Started

### Prerequisites

- Rust 1.88+ (or use Nix for automatic setup)
- Git
- Basic understanding of relational algebra

### Development Setup

#### Using Nix (Recommended)

```bash
# Clone the repository
git clone https://github.com/gregburd/ra.git
cd ra

# Enter development environment
nix develop

# Build and test
cargo build
cargo test
```

#### Without Nix

Install required tools:
- Rust toolchain: https://rustup.rs/
- PostgreSQL, DuckDB, SQLite (for testing)

```bash
git clone https://github.com/gregburd/ra.git
cd ra
cargo build
cargo test
```

### Project Structure

```
ra/
|---- crates/         # Rust crates
|---- rules/          # Rule definitions (.rra files)
|---- docs/           # Documentation
|---- tests/          # Integration tests
|---- web/            # Web explorer frontend
`---- tla/            # TLA+ specifications
```

## Development Workflow

### 1. Find or Create an Issue

- Check existing issues: https://github.com/gregburd/ra/issues
- If your work doesn't have an issue, create one
- Comment that you're working on it

### 2. Fork and Branch

```bash
# Fork the repository on GitHub

# Clone your fork
git clone https://github.com/YOUR_USERNAME/ra.git
cd ra

# Add upstream remote
git remote add upstream https://github.com/gregburd/ra.git

# Create a branch
git checkout -b feature/your-feature-name
```

Branch naming conventions:
- `feature/` - New features
- `fix/` - Bug fixes
- `docs/` - Documentation changes
- `test/` - Test additions
- `refactor/` - Code refactoring

### 3. Make Changes

Follow these guidelines:

**Code Style:**
- Run `cargo fmt` before committing
- Run `cargo clippy` and fix all warnings
- Follow Rust API guidelines: https://rust-lang.github.io/api-guidelines/
- Use meaningful variable names
- Add doc comments for public APIs

**Testing:**
- Add tests for new features
- Ensure all tests pass: `cargo test`
- Add integration tests when appropriate
- Consider property-based tests for algorithms

**Documentation:**
- Update relevant documentation
- Add examples for new features
- Keep comments up to date

**Commits:**
- Write clear commit messages
- Use imperative mood: "Add feature" not "Added feature"
- Keep commits focused (one logical change per commit)
- Reference issues: "Fix #123: Description"

### 4. Validate Your Changes

```bash
# Format code
cargo fmt

# Run linter
cargo clippy --all-targets --all-features -- -D warnings

# Run tests
cargo test --all-features

# Validate rules (if you added/modified rules)
cargo run --bin ra-cli -- validate rules/

# Run benchmarks (if performance-critical)
cargo bench
```

### 5. Push and Create Pull Request

```bash
# Commit your changes
git add .
git commit -m "Add feature: description"

# Push to your fork
git push origin feature/your-feature-name
```

Then create a pull request on GitHub.

## Pull Request Guidelines

### PR Description

Include:
- **What**: Brief description of changes
- **Why**: Motivation and context
- **How**: Technical approach (if not obvious)
- **Testing**: How you tested the changes
- **Related Issues**: Link to related issues

Example:
```markdown
## What
Adds predicate pushdown through left outer joins.

## Why
Extends the existing filter-through-join rule to handle left outer joins
when the predicate is on the preserved side.

## How
- Modified the `filter-through-join` rule to check join type
- Added guards for outer join semantics
- Added comprehensive test cases

## Testing
- Added unit tests in `test_filter_pushdown`
- Verified against PostgreSQL behavior
- All existing tests pass

Fixes #42
```

### PR Checklist

Before submitting, ensure:

- [ ] Code follows project style guidelines
- [ ] All tests pass locally
- [ ] New code has tests
- [ ] Documentation is updated
- [ ] Commit messages are clear
- [ ] Branch is up to date with main
- [ ] No merge conflicts

### Review Process

1. **Automated Checks**: CI will run tests and linters
2. **Maintainer Review**: A maintainer will review your code
3. **Revisions**: Make requested changes
4. **Approval**: Once approved, a maintainer will merge

## Adding Rules

### Step-by-Step Guide

1. **Research the optimization**:
   - Find implementation in database source code
   - Read relevant papers
   - Understand when it applies

2. **Create the .rra file**:
   ```bash
   # Use the appropriate category directory
   touch rules/logical/predicate-pushdown/my-new-rule.rra
   ```

3. **Write the rule** following [docs/guides/rule-authoring.md](docs/guides/rule-authoring.md):
   - Complete YAML frontmatter
   - Clear description
   - Formal relational algebra notation
   - Rust implementation
   - Preconditions
   - Cost model
   - Test cases (positive and negative)
   - References to sources

4. **Validate**:
   ```bash
   ra-cli validate rules/logical/predicate-pushdown/my-new-rule.rra
   ```

5. **Test**:
   ```bash
   ra-cli test rules/logical/predicate-pushdown/my-new-rule.rra
   ```

6. **Update index** (automatic in CI, but you can test):
   ```bash
   ./scripts/generate-index.sh
   ```

### Rule Quality Standards

Your rule should:
- [x] Have complete frontmatter
- [x] Include clear description
- [x] Use correct mathematical notation
- [x] Have working Rust implementation
- [x] Include preconditions
- [x] Have cost model
- [x] Include positive and negative test cases
- [x] Reference source implementations
- [x] Follow naming conventions
- [x] Pass validation

## Code Review Guidelines

When reviewing PRs:

1. **Be Respectful**: Assume good intent, be constructive
2. **Be Specific**: Provide clear, actionable feedback
3. **Explain Why**: Don't just point out issues, explain reasoning
4. **Suggest Solutions**: Offer alternatives when requesting changes
5. **Approve Quickly**: Don't block on minor style issues

Example good review comment:
```
This looks good overall! A few suggestions:

1. In `filter-through-join.rra:45`, the cost model assumes uniform
   distribution. Consider documenting this assumption or using histogram
   statistics if available.

2. The test case for NULL values would be stronger if it also checked
   the case where the filter predicate is `IS NULL`.

3. Minor: The reference link on line 89 is broken.

Otherwise, this is ready to merge once these are addressed.
```

## Testing Guidelines

### Unit Tests

Test individual functions and modules:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_pushdown() {
        let expr = /* ... */;
        let result = apply_filter_pushdown(expr);
        assert_eq!(result, expected);
    }
}
```

### Integration Tests

Test end-to-end functionality in `tests/`:

```rust
#[test]
fn test_optimize_query() {
    let rules = load_rules("rules/").unwrap();
    let query = parse_sql("SELECT * FROM t WHERE x > 10").unwrap();
    let optimized = optimize(query, &rules).unwrap();
    // Assert properties of optimized plan
}
```

### Property-Based Tests

Use `proptest` for algorithms:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_optimization_preserves_semantics(query in arb_query()) {
        let optimized = optimize(query.clone());
        assert!(semantically_equivalent(&query, &optimized));
    }
}
```

### Differential Tests

Compare against reference databases:

```rust
#[test]
fn test_matches_postgres() {
    let query = "SELECT * FROM t WHERE x > 10";
    let our_result = execute(optimize(parse(query)));
    let pg_result = postgres_execute(query);
    assert_eq!(our_result, pg_result);
}
```

## Performance Benchmarks

Add benchmarks in `benchmarks/`:

```rust
use criterion::{criterion_group, criterion_main, Criterion};

fn benchmark_optimization(c: &mut Criterion) {
    let query = /* ... */;
    c.bench_function("optimize complex query", |b| {
        b.iter(|| optimize(query.clone()))
    });
}

criterion_group!(benches, benchmark_optimization);
criterion_main!(benches);
```

Run benchmarks:
```bash
cargo bench
```

## Documentation Standards

### Code Documentation

```rust
/// Applies filter pushdown through join operators.
///
/// This transformation pushes selection predicates through joins when
/// the predicate only references columns from one side of the join.
///
/// # Arguments
///
/// * `expr` - The relational expression to optimize
/// * `stats` - Statistics for cost estimation
///
/// # Returns
///
/// Optimized expression or None if rule doesn't apply
///
/// # Examples
///
/// ```
/// let expr = Filter { /* ... */ };
/// let result = push_filter_through_join(expr, &stats);
/// ```
pub fn push_filter_through_join(
    expr: RelExpr,
    stats: &Statistics,
) -> Option<RelExpr> {
    // Implementation
}
```

### README Updates

When adding features, update relevant README sections:
- Features list
- Quick start examples
- API examples

### Documentation Site

Major features should have entries in `docs/`:
- Architecture documentation
- User guides
- Examples

## Community Guidelines

### Code of Conduct

- Be respectful and inclusive
- Welcome newcomers
- Assume good intent
- Focus on constructive feedback
- Respect different perspectives

### Communication Channels

- **GitHub Issues**: Bug reports, feature requests
- **GitHub Discussions**: Questions, ideas, general discussion
- **Pull Requests**: Code review and collaboration

### Asking Questions

Good questions include:
- What you're trying to do
- What you've already tried
- Error messages (full text)
- Relevant code snippets
- System information

## Legal

### Contributor License Agreement

All contributors must acknowledge the
[Contributor License Agreement](CONTRIBUTOR_AGREEMENT.md) before their
first contribution can be merged. The CLA confirms that:

1. Your contribution is your original work (or you have the right to
   submit it)
2. You grant the project a license to use your contribution under the
   project's dual license (MIT OR Apache-2.0)
3. You are not violating any third party's intellectual property rights

You can acknowledge the CLA by any of these methods:

- **PR checkbox**: Check the CLA box in the pull request template
- **PR comment**: Add a comment stating you agree to the CLA
- **Signed commits**: Use `git commit -s` to add a DCO sign-off

You only need to acknowledge the CLA once. It applies to all future
contributions.

### License

By contributing, you agree that your contributions will be licensed under
the same terms as the project (MIT OR Apache-2.0).

## Recognition

Contributors are recognized in:
- CONTRIBUTORS.md file
- GitHub contributors page
- Release notes
- Academic papers (for major contributions)

## Questions?

- Check existing documentation
- Search closed issues
- Ask in GitHub Discussions
- Tag maintainers if urgent

## Thank You!

Every contribution helps make this project better. Whether you're fixing a typo,
adding a rule, or implementing a major feature, your work is appreciated!

---

**Maintainers:**
- @gregburd - Project Lead

**Response Time:**
- Issues: Usually within 2-3 days
- PRs: Initial review within 1 week
- Critical bugs: Same day

Last Updated: May 12, 2026
