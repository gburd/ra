# Custom Syntax Highlighting

This directory contains TextMate grammar definitions for Ra-specific languages that are not supported by default in VitePress/Shiki.

## Languages Defined

### Core Languages (Full Grammars)

1. **rra** (Ra Rule Language) - `rra.tmLanguage.json`
   - Syntax highlighting for `.rra` rule files
   - Keywords: pattern, rewrite, precondition, cost_model
   - Metadata: name, category, complexity, benefit_range, databases
   - Operators: Scan, Project, Filter, Join, Aggregate, etc.

2. **algebra** (Relational Algebra) - `algebra.tmLanguage.json`
   - Mathematical relational algebra notation
   - Operators: σ, π, ρ, ⋈, ∪, ∩, −, ×
   - Functions: count, sum, avg, min, max, group

3. **sql-interactive** - `sql-interactive.tmLanguage.json`
   - Interactive SQL examples with annotations
   - Input/Output markers
   - Result sections

4. **cron** - `cron.tmLanguage.json`
   - Cron expression syntax
   - Time/date fields
   - Weekday/month names

### Fallback Languages

13 additional languages that inherit from base languages:
- `statistics-editor`, `cost-model`, `statistics-viewer`, `statistics-lab`, `optimization-trace` → JSON
- `facts-editor`, `hardware-simulator` → YAML
- `query-tuner`, `schema-explorer`, `aggregation-analyzer`, `dialect-translator`, `window-explorer` → SQL
- `feature-matrix` → Markdown

## Current Status

**Integrated.** Grammars are registered with Shiki via `markdown.languages`
in `config.js`. Code fences using these language IDs render with syntax
highlighting.

## How It Works

VitePress (1.6+) with Shiki v2 supports the `markdown.languages` config
option. Each grammar JSON file is imported in `config.js` and passed with
a `name` property matching the code fence language ID:

```javascript
import algebraGrammar from './syntax/algebra.tmLanguage.json'
// ...
markdown: {
  languages: [
    { ...algebraGrammar, name: 'algebra' },
    // ...
  ],
}
```

## Adding a New Language

1. Create `language.tmLanguage.json` in this directory
2. Follow TextMate grammar format
3. Import it in `config.js` and add to the `languages` array with the
   correct `name` matching the code fence identifier
4. Run `npx vitepress build` to verify no "not loaded" warnings
