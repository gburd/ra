# ra Command Reference

`ra` is the primary command-line interface for the Ra relational algebra optimizer toolkit. It provides tools for analyzing, optimizing, and testing SQL queries using rewrite rules.

## Installation

```bash
cargo install --path crates/ra-cli
```

Or build from the workspace:

```bash
cargo build --release --bin ra
```

## Global Options

| Flag | Description |
|------|-------------|
| `-v, --verbose` | Show per-file results and debug information |
| `-q, --quiet` | Suppress all non-error output |
| `-V, --version` | Print version |
| `-h, --help` | Print help |

## Commands

### validate

Validate `.rra` rule files for correct YAML frontmatter, required fields (`id`, `name`, `category`, `version`), and category format.

```bash
ra validate rules/filter-pushdown.rra
ra validate rules/                    # scan directory recursively
ra --verbose validate rules/          # show per-file PASS/FAIL
```

Exits with code 1 if any file fails validation.

### test

Run embedded test cases defined in `.rra` rule files. Each test case specifies input SQL, expected plans, or expected optimizations.

```bash
ra test rules/
ra test rules/ --filter pushdown      # run only matching tests
ra test rules/join-commutativity.rra --verbose
```

### list

Display a table of all valid `.rra` rules in a directory.

```bash
ra list                                    # defaults to ./rules
ra list --dir rules/ --category logical/join
ra list --tag performance
```

| Option | Description |
|--------|-------------|
| `-d, --dir` | Rules directory (default: `./rules`) |
| `-c, --category` | Filter by category prefix |
| `-t, --tag` | Filter by tag |

### show

Look up a rule by ID and display all metadata sections: name, category, description, relational algebra, implementation notes, and test cases.

```bash
ra show filter-pushdown-basic
ra show join-commutativity --dir rules/
```

### stats

Show collection statistics for rules in a directory, including counts by category and duplicate analysis.

```bash
ra stats
ra stats --dir rules/
```

### explain

Parse SQL into relational algebra and display the unoptimized plan tree.

```bash
ra explain 'SELECT * FROM orders WHERE amount > 100'
echo 'SELECT 1' | ra explain --stdin
ra explain 'SELECT ...' --hardware-profile server
```

| Option | Description |
|--------|-------------|
| `--hardware-profile` | Cost estimation profile: `edge`, `mobile`, `laptop`, `desktop`, `server`, `gpu-server`, `auto` (default: `auto`) |
| `--stdin` | Read SQL from stdin |

### optimize

Parse SQL, apply optimization rules, and show the resulting plan.

```bash
ra optimize 'SELECT * FROM users WHERE active = true'
ra optimize 'SELECT ...' --diff side-by-side
ra optimize 'SELECT ...' --explain-format postgresql
echo 'SELECT ...' | ra optimize --stdin --trace
```

| Option | Description |
|--------|-------------|
| `--hardware-profile` | Cost estimation profile (default: `auto`) |
| `--stdin` | Read SQL from stdin |
| `--diff` | Diff format: `colored`, `plain`, `side-by-side`, `compact` |
| `--no-color` | Disable color output |
| `--resource-budget` | Budget profile: `interactive`, `standard`, `batch`, `memory-constrained`, `unlimited` |
| `--max-time` | Wall-clock timeout (e.g. `100ms`, `1s`, `10s`) |
| `--max-memory` | Memory limit (e.g. `10MB`, `500MB`, `2GB`) |
| `--max-iterations` | Maximum optimization iterations |
| `--overflow-strategy` | What to do on budget overflow: `best-so-far`, `original`, `fail` |
| `--explain-format` | Database-specific EXPLAIN: `postgresql`, `mysql`, `oracle`, `sqlserver` |
| `--trace` | Show optimizer trace (iteration details, search/apply times) |
| `--rule-advisor` | Enable the Rule Advisor pipeline for intelligent rule filtering |
| `--rule-advisor-learn` | Enable Rule Advisor learning (Stage 3). Persists effectiveness data to `~/.ra/rule-knowledge.json` |
| `--rule-advisor-db <NAME>` | Target database for context filtering (e.g., `postgresql`, `mysql`, `oracle`, `documentdb`) |

### format

Format a SQL query with configurable style options.

```bash
ra format 'select * from users where id=1'
echo 'SELECT ...' | ra format --stdin
ra format 'SELECT ...' --capitalize all --indent spaces4
```

| Option | Description |
|--------|-------------|
| `--stdin` | Read SQL from stdin |
| `--capitalize` | Keyword case: `keywords`, `all`, `none` (default: `keywords`) |
| `--indent` | Indentation: `spaces2`, `spaces4`, `tab` (default: `spaces2`) |

### translate

Dialect translation has moved out of the core toolkit to a separate repository,
[ra-lab](https://codeberg.org/gregburd/ra-lab), along with the `ra-dialect`
crate. The `translate` subcommand is no longer part of the `ra` binary.

### gather-metadata

Collect database metadata and write to a JSON file for offline analysis.

```bash
ra gather-metadata --db 'postgresql://localhost/mydb' -o schema.json
ra gather-metadata --schema existing-schema.json -o merged.json
```

### compare

Compare the Ra optimizer plan against a database EXPLAIN plan.

```bash
ra compare --sql 'SELECT ...' --db 'postgresql://localhost/mydb'
ra compare --sql 'SELECT ...' --explain-json plan.json
```

### tui

Launch the interactive terminal UI for real-time plan monitoring.

```bash
ra tui --demo              # run with built-in demo data
ra tui --timeline data.json
ra tui --record session.cast
```

### stats-timeline

Statistics timeline subcommands for replaying, simulating feedback, and visualizing cost/cardinality evolution.

```bash
ra stats-timeline play --timeline data.toml
ra stats-timeline feedback --timeline data.toml --batch-size 10
ra stats-timeline visualize --timeline data.toml --format ascii
```

### config

Manage configuration settings.

```bash
ra config list
ra config get editor.mode
ra config set editor.mode vim
ra config edit              # open in $EDITOR
ra config reset
ra config path
```

### cache

Plan cache management.

```bash
ra cache list
ra cache stats
ra cache clear
ra cache clear --table orders
ra cache reoptimize --threshold-pct 20
ra cache drift
```

### migrate

Migrate rule pre-conditions from prose descriptions to formal YAML format.

```bash
ra migrate preconditions -i rules/ -o migrated/
ra migrate preconditions -i rules/ -o migrated/ --validate --dry-run
ra migrate validate -b rules/ -m migrated/
ra migrate validate -b rules/ -m migrated/ -f facts.toml
```

### monitor

Monitor a PostgreSQL database with schema analysis and tuning advice.

```bash
ra monitor --demo                   # demo mode, no database needed
ra monitor --postgres 'host=localhost dbname=prod' --tui
ra monitor --postgres '...' --format json
```

### regression

Query regression detection: establish baselines and check for performance regressions.

```bash
ra regression baseline query.sql
ra regression check query.sql --warn-threshold 1.25 --error-threshold 2.0
ra regression report --format json --only-regressions
```

### federated

Analyze federated query execution strategies.

```bash
ra federated analyze \
  --query 'SELECT ...' \
  --remote-db postgresql \
  --remote-table remote_orders \
  --latency 50 \
  --bandwidth 100
```

### analyze-triggers

Analyze triggers on a table and estimate DML costs.

```bash
ra analyze-triggers orders --database-url 'postgresql://localhost/mydb'
ra analyze-triggers orders --schema schema.json
```

### completions

Generate shell tab-completion scripts. Source the output in your shell profile.

```bash
# Bash
ra completions bash > ~/.local/share/bash-completion/completions/ra

# Zsh
ra completions zsh > ~/.zfunc/_ra

# Fish
ra completions fish > ~/.config/fish/completions/ra.fish

# Elvish
ra completions elvish

# PowerShell
ra completions powershell
```

## Shell Completion Setup

### Bash

```bash
ra completions bash > ~/.local/share/bash-completion/completions/ra
# Or for system-wide:
ra completions bash | sudo tee /etc/bash_completion.d/ra > /dev/null
```

### Zsh

```bash
# Create completions directory if needed
mkdir -p ~/.zfunc
ra completions zsh > ~/.zfunc/_ra

# Add to .zshrc (before compinit):
fpath=(~/.zfunc $fpath)
autoload -Uz compinit && compinit
```

### Fish

```bash
ra completions fish > ~/.config/fish/completions/ra.fish
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Validation failure, test failure, or runtime error |
| 2 | Invalid arguments (clap usage error) |
