# Build & Install

This guide covers building RA from source on Linux, macOS, and Windows, including all dependencies and optional components.

## Prerequisites

### Required

| Dependency | Version | Purpose |
|------------|---------|---------|
| Rust | 1.85+ (stable) | Core language |
| Cargo | (bundled with Rust) | Build system |
| pkg-config | any | Native dependency resolution |
| OpenSSL | 1.1+ or 3.x | TLS for web components |

### Optional

| Dependency | Version | Purpose |
|------------|---------|---------|
| Nix | 2.18+ | Reproducible builds (recommended) |
| Node.js | 20+ | Documentation site, web UI |
| npm | 10+ | JavaScript package manager |
| wasm-pack | 0.12+ | WASM module compilation |
| PostgreSQL | 13-18 | `ra-pg-extension` (pgrx) |
| cargo-pgrx | 0.17.0 | PostgreSQL extension builds |
| TLA+ tools | any | Formal verification specs |
| wasm-opt | any | WASM binary optimization |
| cargo-tarpaulin | any | Code coverage |
| cargo-mutants | any | Mutation testing |

---

## Quick Start

### With Nix (recommended)

Nix provides a fully reproducible development environment with all dependencies pinned.

```bash
git clone https://codeberg.org/gregburd/ra.git
cd ra
nix develop
cargo build --release
cargo test --all-features
```

The Nix flake (`flake.nix`) provides Rust stable (latest) with `rust-src`, `rust-analyzer`, `clippy`, `rustfmt`, and the `wasm32-unknown-unknown` target. It also includes `cargo-watch`, `cargo-edit`, `cargo-audit`, `cargo-outdated`, `cargo-mutants`, `ast-grep`, `ripgrep`, `fd`, `shellcheck`, `shfmt`, Node.js, pnpm, PostgreSQL, DuckDB, SQLite, and TLA+ tools.

### Without Nix

```bash
git clone https://codeberg.org/gregburd/ra.git
cd ra

# Install Rust if not already present
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable

# Build
cargo build --release

# Run tests
cargo test --all-features
```

---

## Platform-Specific Setup

### Linux (Ubuntu/Debian)

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev

# Optional: for WASM builds
cargo install wasm-pack
rustup target add wasm32-unknown-unknown

# Optional: for documentation site
sudo apt install -y nodejs npm

# Optional: for PostgreSQL extension
sudo apt install -y postgresql-server-dev-17
cargo install cargo-pgrx --version "=0.17.0" --locked
cargo pgrx init --pg17 $(which pg_config)
```

### Linux (Fedora/RHEL)

```bash
sudo dnf groupinstall -y "Development Tools"
sudo dnf install -y pkg-config openssl-devel

# Optional: for PostgreSQL extension
sudo dnf install -y postgresql-server-devel
cargo install cargo-pgrx --version "=0.17.0" --locked
cargo pgrx init --pg17 $(which pg_config)
```

### macOS

```bash
# Xcode command-line tools (provides compiler toolchain)
xcode-select --install

# OpenSSL (via Homebrew)
brew install openssl pkg-config

# Optional: for PostgreSQL extension
brew install postgresql@17
cargo install cargo-pgrx --version "=0.17.0" --locked
cargo pgrx init --pg17 $(brew --prefix postgresql@17)/bin/pg_config
```

macOS builds require Apple SDK frameworks (`Security`, `SystemConfiguration`, `CoreFoundation`, `CoreServices`) and `libiconv`. These are provided automatically by the Nix flake or Xcode command-line tools.

### Windows

RA builds on Windows via MSVC or WSL2.

**MSVC (native):**

1. Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/) with "Desktop development with C++"
2. Install Rust via [rustup](https://rustup.rs/)
3. Install OpenSSL via [vcpkg](https://vcpkg.io/):
   ```powershell
   vcpkg install openssl:x64-windows
   set OPENSSL_DIR=C:\vcpkg\installed\x64-windows
   ```
4. Build: `cargo build --release`

**WSL2 (recommended for full feature parity):**

Follow the Linux instructions inside a WSL2 distribution. This provides the most complete build environment, including PostgreSQL extension support.

---

## Workspace Structure

The workspace contains 30+ crates. The root `Cargo.toml` defines workspace members, shared dependencies, and lint configuration.

```bash
# Build all workspace crates
cargo build

# Build release binaries
cargo build --release

# Build a specific crate
cargo build -p ra-core
cargo build -p ra-cli
```

### Excluded Crates

Two crates are excluded from the default workspace build:

- **`ra-pg-extension`**: Requires `pg_config` and PostgreSQL development headers. Build separately with `cargo pgrx`.
- **`ra-advisor`**: Has API mismatches with current `ra-core` (needs update).

---

## Build Targets

### CLI (`ra-cli`)

```bash
cargo build --release --bin ra-cli

# Verify
./target/release/ra-cli --help
```

::: tip Running ra-cli
Documentation examples use the short form `ra-cli <args>`, which assumes the
binary is on your `PATH` (e.g. via `cargo install --path crates/ra-cli`).

During development you can run directly from the workspace with:
```bash
cargo run --bin ra-cli -- <args>
```
:::

### WASM Documentation Module

```bash
# Install WASM target
rustup target add wasm32-unknown-unknown
cargo install wasm-pack

# Build the documentation WASM module
cd crates/ra-wasm-docs
wasm-pack build --target web --out-dir ../../docs/static/wasm --release

# Optional: optimize with wasm-opt
wasm-opt -Oz docs/static/wasm/ra_wasm_docs_bg.wasm \
  -o docs/static/wasm/ra_wasm_docs_bg.wasm
```

### PostgreSQL Extension (`ra-pg-extension`)

The extension uses [pgrx](https://github.com/pgcentralfoundation/pgrx) and must be built outside the normal workspace.

```bash
# Install cargo-pgrx (must match the version in Cargo.toml)
cargo install cargo-pgrx --version "=0.17.0" --locked

# Initialize pgrx with your PostgreSQL installation
cargo pgrx init --pg17 $(which pg_config)

# Build and test
cd crates/ra-pg-extension
cargo pgrx test pg17

# Install into a running PostgreSQL instance
cargo pgrx install --release
```

Supported PostgreSQL versions: 13, 14, 15, 16, 17, 18. Select with feature flags (`--features pg17`).

---

## Documentation Site

The documentation is built with [VitePress](https://vitepress.dev/).

```bash
cd docs

# Install dependencies
npm install

# Development server (http://localhost:5173)
npm run dev

# Production build
npm run build
# Output: docs/.vitepress/dist/
```

Or use the `xtask` helper:

```bash
# Build docs
cargo xtask docs

# Build and serve locally
cargo xtask docs --serve
```

The full docs build script (`docs/build.sh`) also builds the WASM module and generates rule documentation if `ra-cli` is available.

---

## Testing

```bash
# Run all tests
cargo test --all-features

# Run tests for a specific crate
cargo test -p ra-core
cargo test -p ra-engine

# Run with tracing output
RUST_LOG=debug cargo test -- --nocapture

# Run benchmarks
cargo bench

# Run specific benchmark suite
cargo bench -p ra-engine --bench optimizer
```

### Rule Validation

```bash
# Validate all .rra rule files
ra-cli validate rules/

# Run rule test cases
ra-cli test rules/
```

### Code Coverage

```bash
cargo install cargo-tarpaulin
cargo tarpaulin --all-features --workspace --out html
# Output: tarpaulin-report.html
```

### Mutation Testing

```bash
cargo install cargo-mutants
cargo mutants --package ra-core
```

### Formal Verification

```bash
# Run TLA+ specifications (requires TLA+ tools)
./scripts/run-tla.sh
```

---

## Linting and Formatting

The workspace enforces strict clippy lints. See the `[workspace.lints.clippy]` section in `Cargo.toml` for the full list.

```bash
# Check formatting
cargo fmt -- --check

# Apply formatting
cargo fmt

# Run clippy (zero warnings required)
cargo clippy --all-targets --all-features -- -D warnings
```

Key denials: `unwrap_used`, `panic`, `panic_in_result_fn`, `unimplemented`, `allow_attributes`, `dbg_macro`, `todo`, `print_stdout`, `print_stderr`, `await_holding_lock`, `large_futures`, `exit`, `mem_forget`.

---

## Release Build Profile

The release profile in `Cargo.toml`:

```toml
[profile.release]
lto = "thin"
codegen-units = 1
strip = true
```

This produces optimized, stripped binaries. The `bench` profile inherits from `release` with debug symbols enabled for profiling.

---

## CI Pipeline

CI runs on both GitHub Actions and Forgejo Actions with identical configurations:

1. **Lint and format**: `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings`
2. **Test**: `cargo test --all-features`
3. **Build release**: `cargo build --release` (only after lint and test pass)

Both CI environments use Nix (`DeterminateSystems/nix-installer-action`) for reproducible builds.

---

## Troubleshooting

### `linker 'cc' not found`

Install a C compiler toolchain. On Linux: `sudo apt install build-essential`. On macOS: `xcode-select --install`.

### OpenSSL errors

Set the `OPENSSL_DIR` environment variable, or install via your system package manager:
- Linux: `sudo apt install libssl-dev` (Debian) or `sudo dnf install openssl-devel` (Fedora)
- macOS: `brew install openssl && export OPENSSL_DIR=$(brew --prefix openssl)`

### `pg_config` not found (pgrx builds)

Ensure PostgreSQL development headers are installed and `pg_config` is on your `PATH`:
```bash
which pg_config
pg_config --version
```

### Nix flake evaluation errors

```bash
# Update the flake lock
nix flake update

# Check that the flake evaluates
nix flake check
```

### `wasm32-unknown-unknown` target not installed

```bash
rustup target add wasm32-unknown-unknown
```

### Out-of-memory during compilation

Large crates (especially `ra-engine` with `egg`, `timely`, and `differential-dataflow`) can use significant memory. Reduce parallelism:
```bash
CARGO_BUILD_JOBS=2 cargo build
```

### Test failures in `ra-pg-extension`

The pgrx test harness starts a temporary PostgreSQL instance. Ensure no other PostgreSQL process conflicts on the test ports, and that you have sufficient permissions. Run with:
```bash
cd crates/ra-pg-extension
cargo pgrx test pg17 -- --nocapture
```

---

## Related Resources

- **[Component APIs](./components.md)** - How major subsystems interact
- **[Release Process](./release.md)** - Cutting releases
- **[Contributing](../contributing.md)** - Contribution guidelines
- **[Architecture](../architecture.md)** - System architecture overview
