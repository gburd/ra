# Getting Started

This guide covers installation, running your first optimization, and
understanding the output.

## Prerequisites

- Rust 1.75+ with cargo
- (Optional) Nix for reproducible builds

## Installation

### Using Nix (Recommended)

```bash
nix develop
cargo build
```

### Without Nix

```bash
cargo build
```

Verify the build:

```bash
cargo test --all-features
```

## First Optimization

Optimize a SQL query:

```bash
cargo run --bin ra-cli -- optimize \
  "SELECT * FROM orders WHERE amount > 1000"
```

The optimizer parses the SQL into a relational algebra expression,
applies transformation rules via equality saturation, and extracts the
lowest-cost plan.

## Explaining Transformations

See what rules were applied:

```bash
cargo run --bin ra-cli -- explain \
  "SELECT c.name FROM customers c JOIN orders o ON c.id = o.cid WHERE o.amount > 1000"
```

This shows each transformation step, the rule that was applied, and
the estimated cost reduction.

## Viewing Plan Diffs

Compare the original and optimized plans side by side:

```bash
cargo run --bin ra-cli -- optimize \
  "SELECT * FROM t1 WHERE x > 10" --diff colored
```

Available diff formats: `colored`, `plain`, `side-by-side`, `compact`.

## Resource Budgets

Control optimizer time and memory usage:

```bash
# Use a predefined budget profile
cargo run --bin ra-cli -- optimize "SELECT * FROM t1" \
  --resource-budget interactive

# Custom limits
cargo run --bin ra-cli -- optimize "SELECT * FROM t1" \
  --max-time 500ms --max-iterations 5
```

Predefined profiles: `interactive`, `standard`, `batch`,
`memory-constrained`.

## Working with Rules

List available transformation rules:

```bash
cargo run --bin ra-cli -- list
```

Validate rule files:

```bash
cargo run --bin ra-cli -- validate rules/
```

Run test cases embedded in a rule:

```bash
cargo run --bin ra-cli -- test \
  rules/logical/predicate-pushdown/filter-through-join.rra
```

## Web Explorer

Run the interactive web interface:

```bash
# Docker
./scripts/docker-run.sh

# Or Docker Compose
./scripts/docker-compose-up.sh
```

Then open http://localhost:8000.

## Next Steps

- [Architecture](architecture.md) -- Understand how the system works
- [Rule Authoring](guides/rule-authoring.md) -- Write your own rules
- [Cost Models](guides/cost-models.md) -- How plans are ranked
- [Examples](examples/simple-optimization.md) -- Worked examples
