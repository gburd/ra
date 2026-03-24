# Building and Serving Documentation

This guide covers all the ways to build and serve the Ra documentation.

## Quick Start

The easiest way to serve documentation locally:

```bash
cargo docs
```

Opens at: http://localhost:5173/ra/

## Methods Overview

| Method | Command | Best For |
|--------|---------|----------|
| **Cargo** | `cargo docs` | Most developers (uses existing Rust toolchain) |
| **Nix** | `nix run .#docs` | Reproducible builds, CI/CD |
| **npm** | `npm run dev` | Frontend development, customization |

## Detailed Instructions

### 1. Using Cargo (Recommended)

Ra includes cargo aliases for documentation tasks via `xtask`:

```bash
# Serve documentation with live reload
cargo docs

# Build static site (no server)
cargo docs-build
```

**Requirements:**
- Rust toolchain
- npm and Node.js 20+

**What it does:**
1. Compiles the `xtask` binary
2. Checks for npm installation
3. Installs npm dependencies if needed
4. Runs VitePress dev server

### 2. Using Nix Flakes

If you're using Nix, the flake provides reproducible documentation builds:

```bash
# Serve documentation locally
nix run .#docs

# Build static site for deployment
nix run .#docs-build
```

**Requirements:**
- Nix with flakes enabled

**What it does:**
1. Uses pinned Node.js 20 from nixpkgs
2. Installs npm dependencies in isolation
3. Runs VitePress with consistent environment

**Benefits:**
- Reproducible builds across machines
- No global Node.js installation needed
- Same versions for all developers and CI

### 3. Using npm Directly

For frontend development or if you want more control:

```bash
cd docs

# First time setup
npm install

# Development server (with hot reload)
npm run dev

# Build for production
npm run build:docs

# Preview production build
npm run preview
```

**npm Scripts:**
- `npm run dev` - Start VitePress dev server
- `npm run build` - Build WASM + documentation
- `npm run build:wasm` - Build WebAssembly interactive components
- `npm run build:docs` - Build documentation only (no WASM)
- `npm run preview` - Preview production build locally

## Build Outputs

### Development Server

```
npm run dev
→ Serves at http://localhost:5173/ra/
→ Hot module replacement (HMR) enabled
→ Source maps enabled for debugging
```

### Production Build

```
npm run build:docs
→ Outputs to: docs/.vitepress/dist/
→ Optimized, minified static files
→ Ready for deployment to any web server
```

## Interactive Features (Optional)

The documentation includes interactive SQL query examples powered by WebAssembly. This requires building the `ra-wasm-docs` crate.

**Note:** The docs work fine without WASM -- interactive components gracefully fall back to mock implementations for demos.

### Building WASM with Nix

The nix flake provides the Rust toolchain with the `wasm32-unknown-unknown` target and `wasm-pack` already included. No additional setup is needed:

```bash
cd docs
./build-wasm.sh
```

### Building WASM without Nix

Ensure `rustup` and `wasm-pack` are installed:

```bash
# Install wasm-pack (if not already installed)
cargo install wasm-pack

# The script will auto-install the wasm32 target via rustup
cd docs
./build-wasm.sh

# Or via npm
npm run build:wasm
```

## Deployment

For production deployment, always use the static build:

```bash
npm run build:docs
```

Output in `docs/.vitepress/dist/` can be served by:
- **Codeberg Pages**: Push dist to pages branch
- **Netlify**: Deploy dist folder
- **Vercel**: Deploy dist folder
- **Static hosting**: Nginx, Apache, CDN, etc.

### Codeberg Pages Example

```bash
cd docs
npm run build:docs

cd .vitepress/dist
git init
git add .
git commit -m "Deploy documentation"
git push -f git@codeberg.org:gregburd/ra.git main:pages
```

## Troubleshooting

### Port 5173 already in use

Stop any existing VitePress servers:
```bash
pkill -f vitepress
```

Or use a different port:
```bash
npx vitepress dev --port 5174
```

### npm permission errors

If you see cache permission errors:
```bash
# Clean npm cache
npm cache clean --force

# Or fix permissions (macOS/Linux)
sudo chown -R $USER:$GROUP ~/.npm
```

### Missing dependencies

```bash
cd docs
trash node_modules package-lock.json  # Never use rm -rf
npm install
```

### VitePress version conflicts

Check installed version:
```bash
npm list vitepress
```

Update to latest:
```bash
npm update vitepress
```

## Configuration

Documentation configuration: `docs/.vitepress/config.js`

Key settings:
- `base: '/ra/'` - Base path for GitHub Pages
- `ignoreDeadLinks: true` - Ignore broken links during build
- Markdown plugins (KaTeX for math)
- Sidebar navigation structure

## Security

See [SECURITY.md](SECURITY.md) for:
- npm audit findings (moderate esbuild dev-only vulnerability)
- Production deployment best practices
- Security recommendations

## Performance

### Development
- Hot reload: <100ms for most changes
- Full rebuild: ~2-3 seconds
- Port binding: Instant

### Production Build
- Average build time: 10-15 seconds
- Minified output: ~2MB total
- Gzip compression: ~400KB

## CI/CD Integration

### GitHub Actions Example

```yaml
name: Deploy Docs

on:
  push:
    branches: [main]

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Setup Node.js
        uses: actions/setup-node@v4
        with:
          node-version: 20

      - name: Build documentation
        run: |
          cd docs
          npm ci
          npm run build:docs

      - name: Deploy to GitHub Pages
        uses: peaceiris/actions-gh-pages@v3
        with:
          github_token: ${{ secrets.GITHUB_TOKEN }}
          publish_dir: docs/.vitepress/dist
```

### Using Nix in CI

```yaml
- name: Build documentation with Nix
  run: nix run .#docs-build
```

Benefits: Reproducible builds, no version drift.

## Related Commands

```bash
# Generate Rust API docs
cargo doc --all-features --open

# Run web UI (separate from documentation)
cargo run --bin ra-web-ui

# Build all project artifacts
cargo build --all-features --release
```

## Further Reading

- [VitePress Documentation](https://vitepress.dev/)
- [Nix Flakes](https://nixos.wiki/wiki/Flakes)
- [Ra Architecture](./architecture.md)
- [Contributing Guide](./contributing.md)
