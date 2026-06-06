{
  description = "Relational Algebra Rule System";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
            "clippy"
            "rustfmt"
            "llvm-tools-preview"
          ];
          targets = [ "wasm32-unknown-unknown" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Shell (use bash 5+ to avoid syntax errors)
            bash

            # Development tools
            nix-direnv
            direnv

            # Rust toolchain
            rustToolchain
            cargo-watch
            cargo-edit
            cargo-audit
            cargo-outdated
            cargo-mutants
            cargo-deny
            cargo-tarpaulin
            wasm-pack

            # Build tools
            pkg-config
            openssl
            cmake
            gnumake
            gcc
            clang
            # libclang is required by bindgen (lime-sys, etc.).  Without it,
            # bindgen falls back to scanning the host system and can pick up
            # an incompatible libclang (e.g. /usr/lib64/llvm22) whose
            # libLLVM.so isn't on the nix library path.
            llvmPackages.libclang
            stdenv.cc
            zlib

            # Database tools for testing
            postgresql
            duckdb
            sqlite

            # TLA+ tools
            tlaplus

            # Code quality tools
            ast-grep
            ripgrep
            fd
            shellcheck
            shfmt

            # Web development
            nodejs_20
            nodePackages.pnpm
          ] ++ lib.optionals stdenv.isDarwin [
            # Darwin-specific system libraries and frameworks
            libiconv
            darwin.apple_sdk.frameworks.Security
            darwin.apple_sdk.frameworks.SystemConfiguration
            darwin.apple_sdk.frameworks.CoreFoundation
            darwin.apple_sdk.frameworks.CoreServices
          ];

          shellHook = ''
            # Use nix-provided bash to avoid compatibility issues
            export SHELL="${pkgs.bash}/bin/bash"

            export RUST_BACKTRACE=1
            export RUST_LOG=info
            export DATABASE_URL="postgres://localhost/ra_dev"
            export TMPDIR="''${TMPDIR:-/tmp}"

            # DuckDB build support
            # Use system DuckDB library to avoid bundled compilation issues
            export DUCKDB_LIB_DIR="${pkgs.duckdb}/lib"
            export DUCKDB_INCLUDE_DIR="${pkgs.duckdb}/include"
            # Fallback to bundled build with proper toolchain
            export CC="${pkgs.clang}/bin/clang"
            export CXX="${pkgs.clang}/bin/clang++"
            export CMAKE="${pkgs.cmake}/bin/cmake"

            # Point bindgen's clang-sys probe at the nix libclang so it
            # doesn't wander off into /usr/lib64/llvm* on impure shells.
            # clang-sys consults LIBCLANG_PATH *before* its filesystem
            # heuristic, so this alone avoids the host-libclang fallback.
            export LIBCLANG_PATH="${pkgs.llvmPackages.libclang.lib}/lib"

            echo "🚀 Relational Algebra dev environment loaded"
            echo "Rust version: $(rustc --version)"
            echo "Cargo version: $(cargo --version)"
            echo ""
            echo "Build:"
            echo "  cargo build            - Build core library (default members)"
            echo "  cargo build -p ra-cli  - Build CLI with database adapters"
            echo "  cargo test             - Run tests"
            echo "  cargo clippy           - Run linter"
            echo ""
            echo "Query optimization (ra-cli):"
            echo "  ra-cli explain  'SELECT ...'   - Parse SQL into relational algebra"
            echo "  ra-cli optimize 'SELECT ...'   - Optimize with rewrite rules"
            echo "  ra-cli translate --from pg --to mysql 'SELECT ...'"
            echo "  ra-cli list                    - List optimization rules"
            echo "  nix run .#cli -- <args>        - Run ra-cli via the flake"
            echo "  nix run .#bench -- <args>      - Run the ra-bench benchmark CLI"
            echo "  nix run .#difftest -- <args>   - Run the differential test harness"
            echo ""
            echo "Documentation:"
            echo "  nix run .#docs               - Serve docs (http://localhost:5173/ra/)"
            echo "  nix run .#docs-build         - Build docs for deployment"
            echo ""
            echo "Container Services (Docker/Podman):"
            echo "  nix run .#docker-build                   - Build all images"
            echo "  nix run .#docker-build-postgres-extension - PostgreSQL + Ra extension image"
            echo "  nix run .#docker-up                      - Start services"
            echo "  nix run .#docker-down                    - Stop all services"
            echo ""
          '';

          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        };

        # NOTE: nix build is not currently supported because lime-sys
        # requires a git submodule (crates/lime-sys/lime) that Nix flakes
        # cannot fetch.  Use `nix develop` + `cargo build` instead.
        # packages.default = ...;

        apps = {
          # Serve documentation locally
          # Usage: nix run .#docs
          docs = {
            type = "app";
            program = toString (pkgs.writeShellScript "serve-docs" ''
              set -e
              cd docs

              # Install dependencies if needed (check for vitepress specifically)
              if [ ! -x node_modules/.bin/vitepress ]; then
                echo "📦 Installing dependencies with npm..."
                ${pkgs.nodejs_20}/bin/npm install
              fi

              echo "🔧 Generating navigation..."
              ${pkgs.nodejs_20}/bin/node .vitepress/generate-rule-nav.js

              echo "📚 Starting documentation server..."
              echo ""
              echo "Documentation will be available at:"
              echo "   http://localhost:5173/ra/"
              echo ""
              echo "Press Ctrl+C to stop the server"
              echo ""

              export NODE_OPTIONS='--max-old-space-size=8192'
              exec ${pkgs.nodejs_20}/bin/npx vitepress dev
            '');
          };

          # Build documentation for deployment
          # Usage: nix run .#docs-build
          docs-build = {
            type = "app";
            program = toString (pkgs.writeShellScript "build-docs" ''
              set -e
              cd docs

              # Install npm dependencies if needed (check for vitepress specifically)
              if [ ! -x node_modules/.bin/vitepress ]; then
                echo "📦 Installing npm dependencies..."
                ${pkgs.nodejs_20}/bin/npm install
              fi

              echo "📚 Building documentation..."
              echo "🔧 Generating navigation..."
              ${pkgs.nodejs_20}/bin/node .vitepress/generate-rule-nav.js

              echo "🏗️  Building VitePress site..."
              export NODE_OPTIONS='--max-old-space-size=8192'
              ${pkgs.nodejs_20}/bin/npx vitepress build

              echo "✅ Documentation built successfully!"
              echo "   Output: docs/.vitepress/dist/"
            '');
          };

          # Run the ra-cli query-optimizer CLI
          # Usage: nix run .#cli -- explain 'SELECT ...'
          cli = {
            type = "app";
            program = toString (pkgs.writeShellScript "ra-cli" ''
              exec ${rustToolchain}/bin/cargo run --release -p ra-cli -- "$@"
            '');
          };

          # Run the ra-bench benchmark CLI
          # Usage: nix run .#bench -- --workload tpch
          bench = {
            type = "app";
            program = toString (pkgs.writeShellScript "ra-bench" ''
              exec ${rustToolchain}/bin/cargo run --release -p ra-bench -- "$@"
            '');
          };

          # Run the differential test harness (Ra vs a reference DB)
          # Usage: nix run .#difftest -- <args>
          difftest = {
            type = "app";
            program = toString (pkgs.writeShellScript "ra-difftest" ''
              exec ${rustToolchain}/bin/cargo run --release -p ra-difftest -- "$@"
            '');
          };

          # Build all container images
          # Usage: nix run .#docker-build
          docker-build = {
            type = "app";
            program = toString (pkgs.writeShellScript "docker-build-all" ''
              set -e

              # Detect container runtime
              source scripts/detect-container-runtime.sh

              echo "🐳 Building all container images with $CONTAINER_RUNTIME..."
              echo ""

              # Use --parallel for Docker Compose, but not for podman-compose (not supported)
              if [[ "$COMPOSE_COMMAND" == *"docker"* ]]; then
                $COMPOSE_COMMAND build --parallel
              else
                $COMPOSE_COMMAND build
              fi

              echo ""
              echo "✅ All container images built successfully!"
              echo ""
              echo "To start services:"
              echo "   $COMPOSE_COMMAND up -d"
              echo ""
              echo "To test:"
              echo "   ./scripts/docker-test.sh"
            '');
          };

          # Build docs container image
          # Usage: nix run .#docker-build-docs
          docker-build-docs = {
            type = "app";
            program = toString (pkgs.writeShellScript "docker-build-docs" ''
              set -e
              # Detect container runtime
              source scripts/detect-container-runtime.sh
              echo "🐳 Building docs container image with $CONTAINER_RUNTIME..."
              $COMPOSE_COMMAND build docs
              echo "✅ Docs image built!"
            '');
          };

          # Build PostgreSQL + Ra extension container image
          # Usage: nix run .#docker-build-postgres-extension
          docker-build-postgres-extension = {
            type = "app";
            program = toString (pkgs.writeShellScript "docker-build-postgres-extension" ''
              set -e
              # Detect container runtime
              source scripts/detect-container-runtime.sh
              echo "🐳 Building PostgreSQL + Ra extension image with $CONTAINER_RUNTIME..."
              $COMPOSE_COMMAND build postgres-ra-extension
              echo "✅ PostgreSQL + Ra extension image built!"
            '');
          };

          # Build PostgreSQL 19 + Ra proxy container image
          # Usage: nix run .#docker-build-postgres-proxy
          docker-build-postgres-proxy = {
            type = "app";
            program = toString (pkgs.writeShellScript "docker-build-postgres-proxy" ''
              set -e
              # Detect container runtime
              source scripts/detect-container-runtime.sh
              echo "🐳 Building PostgreSQL 19 + Ra proxy image with $CONTAINER_RUNTIME..."
              echo "⚠️  This build takes 30-45 minutes (PostgreSQL from source)"
              $COMPOSE_COMMAND build postgres-ra-proxy
              echo "✅ PostgreSQL + Ra proxy image built!"
            '');
          };

          # Start all container services
          # Usage: nix run .#docker-up
          docker-up = {
            type = "app";
            program = toString (pkgs.writeShellScript "docker-up" ''
              set -e
              # Detect container runtime
              source scripts/detect-container-runtime.sh
              echo "🐳 Starting all container services with $CONTAINER_RUNTIME..."
              $COMPOSE_COMMAND up -d
              echo ""
              echo "✅ Services started!"
              echo ""
              echo "Check status:"
              echo "   $COMPOSE_COMMAND ps"
              echo ""
              echo "View logs:"
              echo "   $COMPOSE_COMMAND logs -f"
            '');
          };

          # Stop all container services
          # Usage: nix run .#docker-down
          docker-down = {
            type = "app";
            program = toString (pkgs.writeShellScript "docker-down" ''
              set -e
              # Detect container runtime
              source scripts/detect-container-runtime.sh
              echo "🐳 Stopping all container services with $CONTAINER_RUNTIME..."
              $COMPOSE_COMMAND down
              echo "✅ Services stopped!"
            '');
          };
        };
      }
    );
}
