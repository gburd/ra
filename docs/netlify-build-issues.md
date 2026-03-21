# Netlify Build Issues

## VitePress Vue Template Parser Conflict

### Problem

VitePress build fails when processing SQL code blocks containing patterns like `AS t(col)` where single-letter identifiers are followed by parentheses. The Vue template compiler treats these as HTML tags (e.g., `<t(...)`), causing "Element is missing end tag" errors.

### Affected Files

- `rules/unnest/merge-unnests.md:165`
- `rules/unnest/unnest-pushdown-scan.md`
- `rules/unnest/unnest-array-literal.md`
- `rules/unnest/lateral-to-semi-join.md`
- `rules/unnest/filter-through-unnest.md`
- `rules/database-specific/aggregate-values.md`
- `rules/database-specific/values-reduce.md`
- `rules/logical/array-unnest-pushdown.md`
- `rules/logical/filter-table-function-transpose.md`
- `rules/logical/project-values-reduction.md`
- `rules/logical/apply-to-join.md`
- `rules/logical/constant-fold-aggregate.md`
- `rules/logical/predicate-pushdown/filter-table-function-transpose.md`
- `rules/logical/gap-fill-optimization.md`
- `rules/logical/unnest-decorrelate.md`

### Root Cause

VitePress processing pipeline:
1. Markdown → HTML conversion
2. Vue template compilation
3. Static site generation

Single-letter patterns in code blocks (`AS t(`, `AS r(`, etc.) trigger Vue's HTML parser before they're recognized as code content.

### Attempted Solutions

1. **Renaming aliases** (`t` → `result`): Incomplete, aliases have semantic meaning
2. **Adding spaces** (`AS t (col)`): Formatters automatically remove spaces
3. **Vue `isCustomElement` config**: Doesn't affect markdown processing stage
4. **`v-pre` wrappers**: Causes additional parser errors with closing tags
5. **Disabling Vue in markdown**: No VitePress configuration option found

### Potential Solutions

#### Option A: Exclude Problematic Files (Temporary)

Modify `netlify.toml`:

```toml
[build]
  command = """
    find docs/rules -name '*unnest*.md' -delete && \\
    npm install && \\
    npm run docs:build
  """
```

**Pros**: Immediate deployment of working docs
**Cons**: Missing unnest rule documentation

#### Option B: Use Literal Code Blocks

Replace triple-backtick code blocks with indented code blocks (4 spaces):

```markdown
### Example

    SELECT * FROM unnest(arr) AS t(value)
```

**Pros**: May bypass Vue template parsing
**Cons**: Loses syntax highlighting; requires rewriting many files

#### Option C: Escape HTML-like Patterns

Use HTML entities or zero-width spaces:

```sql
SELECT * FROM unnest(arr) AS t​(value)  -- Zero-width space before (
```

**Pros**: Preserves semantic meaning
**Cons**: Hard to maintain, copy-paste issues

#### Option D: Switch Documentation Generator

Replace VitePress with:
- **Docusaurus**: React-based, better code block handling
- **mkdocs-material**: Python-based, no Vue conflicts
- **mdBook**: Rust-based, simpler processing

**Pros**: Eliminates Vue template issues
**Cons**: Major migration effort, lose VitePress features

#### Option E: Patch VitePress Markdown Plugin

Fork VitePress and modify markdown-it plugin to mark code blocks as raw content before Vue compilation.

**Pros**: Proper fix at the source
**Cons**: Requires maintaining fork, upstreaming changes

#### Option F: Use Raw HTML Code Blocks

```markdown
<pre><code class="language-sql">
SELECT * FROM unnest(arr) AS t(value)
</code></pre>
```

**Pros**: Bypasses markdown processing
**Cons**: Verbose, loses markdown simplicity

### Recommended Approach

**Short-term**: Option A (exclude unnest files)
- Deploy docs immediately
- Note missing sections in deployment guide
- Create GitHub issue tracking the problem

**Medium-term**: Option F (raw HTML) for affected sections
- Minimal file changes
- Preserves all documentation
- Acceptable trade-off for code-heavy docs

**Long-term**: Option E (patch VitePress) or D (switch generator)
- Investigate VitePress issue tracker for similar reports
- Contribute upstream fix if feasible
- Evaluate alternative generators if pattern persists

### Implementation Status

- ✅ `netlify.toml` created with build configuration
- ✅ Deployment guide updated with Netlify instructions
- ⏳ Build still fails on affected files
- ⏳ Awaiting decision on short-term workaround

### Related Issues

- VitePress GitHub: [Search for Vue template issues](https://github.com/vuejs/vitepress/issues)
- Vue 3 Compiler: Known issue with single-letter HTML tags in templates

### Testing Locally

To reproduce the build failure:

```bash
cd docs
npm install
npm run docs:build
```

Error output:
```
[vite:vue] [plugin vite:vue] rules/unnest/merge-unnests.md (165:26): Element is missing end tag.
```

To test with excluded files:

```bash
cd docs
rm rules/unnest/merge-unnests.md
npm run docs:build  # Should succeed
```

### Contact

For questions about this issue:
- Create an issue in the repository
- Tag `@team-lead` or `@netlify-deployer`
