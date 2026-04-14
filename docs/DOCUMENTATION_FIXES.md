# Documentation Routing Fixes

## Issues Fixed

### 1. **URL Routing Problems**
- **Problem**: URLs like `http://localhost:5173/ra/ra/rules/...` (double `/ra/`)
- **Fix**: Use single `/ra/` prefix: `http://localhost:5173/ra/rules/...`

### 2. **Rule File Access**
- **Problem**: Trying to access `.rra.html` files
- **Fix**: Use correct extensions:
  - Raw rule files: `.rra` (e.g., `/ra/rules/cost-models/cardinality-estimation.rra`)
  - Documentation pages: `.html` (e.g., `/ra/rules/cost-models/cardinality-estimation.html`)

### 3. **Missing Rule Navigation**
- **Problem**: No sidebar navigation for individual rule files
- **Fix**: Added automatic navigation generator (`generate-rule-nav.js`)

## Correct URLs

### **For Rule Files (.rra)**
```
http://localhost:5173/ra/rules/cost-models/cardinality-estimation.rra
http://localhost:5173/ra/rules/database-specific/postgresql/...
```

### **For Documentation Pages (.md → .html)**
```
http://localhost:5173/ra/rules/cost-models/cardinality-estimation.html
http://localhost:5173/ra/getting-started.html
```

## Files Created

1. **`docs/.vitepress/generate-rule-nav.js`** - Automatic navigation generator
2. **`docs/.vitepress/rule-nav.json`** - Generated navigation structure

## Build Process

The navigation is now automatically generated during:
- `npm run dev` (development)
- `npm run build` (production)

## Testing

1. **Start the dev server:**
   ```bash
   cd docs && npm run dev
   ```

2. **Test rule access:**
   - Documentation: `http://localhost:5173/ra/rules/cost-models/cardinality-estimation`
   - Raw rule file: `http://localhost:5173/ra/rules/cost-models/cardinality-estimation.rra`

3. **Check navigation:**
   - Look for "Rules Reference" in the sidebar
   - It should now include all rule categories and individual rules

## Next Steps

If issues persist:
1. Clear browser cache
2. Restart the dev server
3. Check console for JavaScript errors
4. Verify file paths match the generated navigation