# Plan Visualization

The `ra-cli optimize` command supports colorized plan diffs that highlight
structural changes between original and optimized query plans. Four output
formats are available, from detailed inline diffs to compact summaries.

## Diff Formats

Select a format with `--diff <FORMAT>`:

### `colored` (default when `--diff` is specified)

Full inline diff with ANSI colors. Unchanged nodes appear in the default
terminal color. Removed nodes are red with a `-` prefix, added nodes are
green with `+`, and modified nodes are yellow with `~`.

```bash
ra-cli optimize "SELECT ..." --diff colored
```

### `plain`

Same structure as `colored` but without ANSI escape codes. Suitable for
piping to files, log aggregators, or terminals without color support.

```bash
ra-cli optimize "SELECT ..." --diff plain > diff.txt
```

### `side-by-side`

Renders the original and optimized plan trees in two columns with a `|`
separator. Falls back to a stacked vertical layout if the terminal is
narrower than 80 columns.

```bash
ra-cli optimize "SELECT ..." --diff side-by-side
```

### `compact`

A single-line summary showing the count of removed, added, and modified
nodes. Useful for scripting and dashboards.

```bash
ra-cli optimize "SELECT ..." --diff compact
```

## Color Scheme

| Element     | Color   | Prefix | Meaning                      |
|-------------|---------|--------|------------------------------|
| Unchanged   | default | (none) | Node exists in both plans    |
| Removed     | red     | `-`    | Node was in original only    |
| Added       | green   | `+`    | Node was in optimized only   |
| Modified    | yellow  | `~`    | Node changed type or params  |
| Algorithm   | cyan    |        | Join strategy changed        |
| Summary     | bold    |        | Aggregate change counts      |

Within modified nodes, individual changes are listed as sub-items:
- `OperatorType`: the relational operator changed (e.g. Filter to Project)
- `Algorithm`: the join strategy changed (e.g. nested loop to hash join)
- `Structure`: a node was replaced by a different operator type

## Terminal Compatibility

Color output follows these conventions:

- **`NO_COLOR` environment variable**: When set, disables all color output.
  This follows the [no-color.org](https://no-color.org) convention.
- **`FORCE_COLOR` environment variable**: When set, enables color output
  regardless of terminal detection.
- **`TERM=dumb`**: Disables color output.
- **TTY detection**: Colors are enabled when stderr is connected to a
  terminal (using `atty`).
- **`--no-color` flag**: Disables color output for the current command.

The side-by-side format reads terminal width from:
1. The `COLUMNS` environment variable
2. `TIOCGWINSZ` ioctl on Unix systems
3. A default of 120 columns

## Diff Algorithm

Plan diffs use a Longest Common Subsequence (LCS) algorithm on the
flattened pre-order traversal of plan node labels. This identifies
structural changes efficiently:

1. Both plans are flattened to ordered label sequences
2. LCS finds the longest matching subsequence
3. Nodes not in the LCS are classified as added, removed, or modified
4. Modified nodes are further analyzed to identify the type of change

This approach handles operator reordering (e.g. filter pushdown through
joins) and structural changes (e.g. join type conversion) correctly.

## Combining with Resource Budgets

When `--resource-budget` is used with `--diff`, the output includes both
the resource usage report and the plan diff:

```bash
ra-cli optimize "SELECT ..." --resource-budget interactive --diff colored
```

This shows:
1. The resource usage summary (time, iterations, status)
2. The plan diff between original and optimized plans

If the optimization was incomplete (stopped by a budget limit), the status
line indicates which resource was exceeded. The diff still shows the
changes between the original plan and the best plan found before the limit.

## Exporting Diffs

To capture diffs for documentation or bug reports:

```bash
# Plain text file
ra-cli optimize "SELECT ..." --diff plain --no-color > diff.txt

# Compact summary for CI
ra-cli optimize "SELECT ..." --diff compact --no-color 2>&1 | tail -1
```

The `plain` format with `--no-color` produces clean text without ANSI
escape codes.
