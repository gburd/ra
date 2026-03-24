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

**✅ Grammar files created and validated**
**⏸️ Integration pending** - These grammars need to be registered with Shiki

## Integration Challenge

VitePress uses Shiki for syntax highlighting, which requires grammars to be registered at build time. The challenge:

1. **Module Resolution**: VitePress uses esbuild which has specific requirements for imports
2. **Shiki Registration**: Need to call `loadLanguage()` with proper grammar format
3. **Timing**: Must register before Shiki initializes

## Attempted Approaches

### Approach 1: Direct Import (Failed)
```javascript
import rra from './rra.tmLanguage.json' assert { type: 'json' }
```
**Issue**: esbuild doesn't support JSON assertions in all contexts

### Approach 2: Dynamic Loading (Failed)
```javascript
import fs from 'fs'
const grammar = JSON.parse(fs.readFileSync('./rra.tmLanguage.json'))
```
**Issue**: Can't use Node.js APIs in VitePress config (runs in browser too)

### Approach 3: VitePress Plugin (TODO)
Need to create a proper VitePress plugin that:
1. Hooks into the build process
2. Registers grammars with Shiki before page rendering
3. Handles both dev and production builds

## Workaround

Currently, these languages fall back to plain text (`txt`) highlighting. This is harmless - users see the code, just without syntax coloring.

For relational algebra, we have a better solution: the `algebraPlugin` markdown-it plugin converts notation to Unicode symbols inline:
- `{{sigma[p](R)}}` → σ[p](R) with hover tooltips
- Works without syntax highlighting

## Future Work

To properly integrate these grammars:

1. **Create VitePress Plugin**: `syntax-loader-plugin.js`
   ```javascript
   export function syntaxLoaderPlugin() {
     return {
       name: 'syntax-loader',
       config() {
         // Register grammars with Shiki
       }
     }
   }
   ```

2. **Use Shiki's API**:
   ```javascript
   import { loadLanguage } from 'shiki'
   await loadLanguage({
     id: 'rra',
     scopeName: 'source.rra',
     grammar: /* grammar object */
   })
   ```

3. **Test in both dev and build modes**

## References

- [Shiki Documentation](https://shiki.matsu.io/)
- [VitePress Custom Config](https://vitepress.dev/reference/site-config)
- [TextMate Grammars](https://macromates.com/manual/en/language_grammars)

## Contributing

To add a new language:

1. Create `language.tmLanguage.json` in this directory
2. Follow TextMate grammar format
3. Test with a TextMate-compatible editor
4. Update `fallbackGrammars` in `index.js` if using base language
5. Once Shiki integration is fixed, add to the registration list
