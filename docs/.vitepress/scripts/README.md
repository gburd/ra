# VitePress Build Scripts

This directory contains scripts that run during the VitePress build process.

## process-rfcs.js

Automatically runs before `vitepress build` and `vitepress dev` via the `prebuild` and `predev` npm scripts.

### What it does

1. **Copies RFCs** from `rfcs/text/` to `docs/rfcs/`
2. **Adds cross-linking** - Converts RFC references like "RFC 0080" into clickable markdown links
3. **Generates backlinks** - Adds "Referenced By" sections showing which RFCs link to each RFC
4. **Creates index** - Generates `docs/maintainers/rfcs/index.md` with categorized RFC listing
5. **Generates navigation** - Creates `docs/.vitepress/rfc-nav.json` for sidebar navigation

### How it works

**Pass 1: Metadata Collection**
- Scans all RFC files in `rfcs/text/`
- Parses frontmatter to extract RFC number, title, status, author, date
- Finds all RFC references (e.g., "RFC 0080", "RFC 62")
- Builds a reference graph

**Pass 2: Processing**
- Copies each RFC to `docs/rfcs/`
- Replaces RFC text references with markdown links
- Adds "Referenced By" section at the end of each RFC
- Preserves original formatting

**Pass 3: Index Generation**
- Categorizes RFCs by topic (Core, Database-Specific, Performance, etc.)
- Generates comprehensive index with status badges
- Creates README for rfcs directory
- Generates navigation config for VitePress

### Manual execution

```bash
cd docs
node .vitepress/scripts/process-rfcs.js
```

### Output files

Generated (gitignored):
- `docs/rfcs/*.md` - Processed RFC documents with cross-links
- `docs/rfcs/README.md` - Overview of RFC directory
- `docs/maintainers/rfcs/index.md` - Comprehensive RFC index
- `docs/.vitepress/rfc-nav.json` - Navigation sidebar config

Committed to git:
- `rfcs/text/*.md` - Original RFC documents
- `docs/.vitepress/scripts/process-rfcs.js` - This script

### Cross-link format

Original:
```markdown
This extends RFC 0062 with RUM index support.
```

Processed:
```markdown
This extends [RFC 0062](/ra/maintainers/rfcs/0062-documentdb) with RUM index support.
```

### Reference tracking

The script builds a bidirectional reference graph:

- **Forward references**: RFC 0080 mentions RFC 0062, RFC 0079
- **Backlinks**: RFC 0062 is referenced by RFC 0080

This enables the "Referenced By" section:

```markdown
## Referenced By

This RFC is referenced by:

- [RFC 80: DocumentDB RUM](/ra/maintainers/rfcs/0080-documentdb-rum-bson-optimization)
```

### Categorization

RFCs are automatically categorized by keywords in their titles:

| Category | Keywords |
|----------|----------|
| Core Optimizer | (default) |
| Database-Specific | postgresql, documentdb, citus, mongodb, oracle |
| Performance & Resources | memory, resource, parallelism, numa, buffer |
| Query Features | spatial, vector, time series, full-text, xpath |
| Platform & Integration | platform, extension, dialect |

### Status badges

| Status | Badge |
|--------|-------|
| Proposed | 📋 |
| Draft | 📝 |
| Active | 🔄 |
| Complete | ✓ |
| Deprecated | ⚠️ |

### Adding a new RFC

1. Create RFC in `rfcs/text/NNNN-title.md`
2. Include standard frontmatter:
   ```markdown
   # RFC NNNN: Title

   - Start Date: YYYY-MM-DD
   - Author: Name
   - Status: Proposed
   - Tracking Issue: TBD
   ```
3. Reference other RFCs naturally: "RFC 0062", "RFC 79", etc.
4. Run `npm run build` or `npm run dev` - script runs automatically
5. Check generated `docs/rfcs/NNNN-title.md` for proper cross-linking
6. Verify RFC appears in `docs/maintainers/rfcs/` index

### Troubleshooting

**RFC references not linking:**
- Ensure RFC number exists in `rfcs/text/`
- Check frontmatter has proper "# RFC NNNN:" format
- Leading zeros are optional: "RFC 0080" and "RFC 80" both work

**Referenced By section missing:**
- Only RFCs that are referenced by others get this section
- Check that other RFCs mention this RFC number in their text

**Build fails:**
- Run `npm install` to ensure dependencies are current
- Check for syntax errors in RFC markdown
- Verify `rfcs/text/` directory exists and contains .md files

**Navigation not showing RFCs:**
- Check `docs/.vitepress/rfc-nav.json` was generated
- Verify `config.js` imports and includes `rfcNavigation`
- Look for errors in VitePress build output
