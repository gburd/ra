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
          extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
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
            wasm-pack

            # Build tools
            pkg-config
            openssl
            cmake
            gnumake
            gcc
            stdenv.cc

            # Database tools for testing
            postgresql
            duckdb
            sqlite
            unixODBC

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
            export CC="${pkgs.gcc}/bin/gcc"
            export CXX="${pkgs.gcc}/bin/g++"
            export CMAKE="${pkgs.cmake}/bin/cmake"

            echo "🚀 Relational Algebra dev environment loaded"
            echo "Rust version: $(rustc --version)"
            echo "Cargo version: $(cargo --version)"
            echo ""
            echo "Available commands:"
            echo "  cargo build          - Build all crates"
            echo "  cargo test           - Run tests"
            echo "  cargo clippy         - Run linter"
            echo "  cargo fmt            - Format code"
            echo "  cargo run --bin ra-cli -- <args>"
            echo ""
            echo "Web & Documentation:"
            echo "  nix run .#web                - Start ra-web backend (http://localhost:8000)"
            echo "  nix run .#web-dev            - Start ra-web backend with auto-reload"
            echo "  nix run .#web-frontend-dev   - Start React frontend dev server"
            echo "  nix run .#web-frontend-build - Build React frontend for production"
            echo "  nix run .#docs               - Serve docs (http://localhost:5173/ra/)"
            echo "  nix run .#docs-build         - Build docs for deployment"
            echo ""
            echo "Docker:"
            echo "  nix run .#docker-build                   - Build all images"
            echo "  nix run .#docker-build-docs              - Build docs image"
            echo "  nix run .#docker-build-web               - Build ra-web image"
            echo "  nix run .#docker-build-postgres-extension - Build PG + Ra extension"
            echo "  nix run .#docker-build-postgres-proxy    - Build PG19 + Ra proxy (slow)"
            echo "  nix run .#docker-up                      - Start all services"
            echo "  nix run .#docker-down                    - Stop all services"
            echo ""
          '';

          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        };

        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "ra-cli";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = with pkgs; [
            pkg-config
          ];

          buildInputs = with pkgs; [
            openssl
            unixODBC
          ] ++ lib.optionals stdenv.isDarwin [
            darwin.apple_sdk.frameworks.Security
            darwin.apple_sdk.frameworks.SystemConfiguration
          ];
        };

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

              echo "📚 Starting documentation server..."
              echo ""
              echo "Documentation will be available at:"
              echo "   http://localhost:5173/ra/"
              echo ""
              echo "Press Ctrl+C to stop the server"
              echo ""

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
              NODE_OPTIONS='--max-old-space-size=16384' ${pkgs.nodejs_20}/bin/npx vitepress build

              echo "✅ Documentation built successfully!"
              echo "   Output: docs/.vitepress/dist/"
            '');
          };

          # Serve ra-web demo server
          # Usage: nix run .#web
          web = {
            type = "app";
            program = toString (pkgs.writeShellScript "serve-web" ''
              set -e

              echo "🌐 Building ra-web server..."
              ${rustToolchain}/bin/cargo build --release --bin ra-web

              echo ""
              echo "🚀 Starting ra-web server..."
              echo ""
              echo "Demo interface will be available at:"
              echo "   http://localhost:8000/"
              echo ""
              echo "API endpoints:"
              echo "   POST /api/optimize     - Optimize SQL query"
              echo "   POST /api/translate    - Translate between SQL dialects"
              echo "   GET  /api/health       - Health check"
              echo ""
              echo "Press Ctrl+C to stop the server"
              echo ""

              exec ${rustToolchain}/bin/cargo run --release --bin ra-web
            '');
          };

          # Build and run ra-web in development mode
          # Usage: nix run .#web-dev
          web-dev = {
            type = "app";
            program = toString (pkgs.writeShellScript "serve-web-dev" ''
              set -e

              echo "🌐 Starting ra-web in development mode..."
              echo ""
              echo "Demo interface: http://localhost:8000/"
              echo ""
              echo "Press Ctrl+C to stop"
              echo ""

              exec ${rustToolchain}/bin/cargo watch -x "run --bin ra-web"
            '');
          };

          # Serve ra-web React frontend dev server
          # Usage: nix run .#web-frontend-dev
          web-frontend-dev = {
            type = "app";
            program = toString (pkgs.writeShellScript "web-frontend-dev" ''
              set -e
              cd crates/ra-web/frontend

              # Install dependencies if needed
              if [ ! -d node_modules ]; then
                echo "📦 Installing dependencies with npm..."
                ${pkgs.nodejs_20}/bin/npm install
              fi

              echo "🌐 Starting ra-web React frontend dev server..."
              echo ""
              echo "Frontend will be available at:"
              echo "   http://localhost:5173/"
              echo ""
              echo "Note: Make sure ra-web backend is running on port 8000:"
              echo "   nix run .#web-dev"
              echo ""
              echo "Press Ctrl+C to stop the server"
              echo ""

              exec ${pkgs.nodejs_20}/bin/npm run dev
            '');
          };

          # Build ra-web React frontend for production
          # Usage: nix run .#web-frontend-build
          web-frontend-build = {
            type = "app";
            program = toString (pkgs.writeShellScript "web-frontend-build" ''
              set -e
              cd crates/ra-web/frontend

              # Install dependencies if needed
              if [ ! -d node_modules ]; then
                echo "📦 Installing dependencies with npm..."
                ${pkgs.nodejs_20}/bin/npm install
              fi

              echo "🌐 Building ra-web React frontend..."
              ${pkgs.nodejs_20}/bin/npm run build

              echo ""
              echo "✅ Frontend built successfully!"
              echo "   Output: crates/ra-web/frontend/dist/"
            '');
          };

          # Build all Docker images
          # Usage: nix run .#docker-build
          docker-build = {
            type = "app";
            program = toString (pkgs.writeShellScript "docker-build-all" ''
              set -e

              echo "🐳 Building all Docker images..."
              echo ""

              ${pkgs.docker}/bin/docker compose build --parallel

              echo ""
              echo "✅ All Docker images built successfully!"
              echo ""
              echo "To start services:"
              echo "   docker compose up -d"
              echo ""
              echo "To test:"
              echo "   ./scripts/docker-test.sh"
            '');
          };

          # Build docs Docker image
          # Usage: nix run .#docker-build-docs
          docker-build-docs = {
            type = "app";
            program = toString (pkgs.writeShellScript "docker-build-docs" ''
              set -e
              echo "🐳 Building docs Docker image..."
              ${pkgs.docker}/bin/docker compose build docs
              echo "✅ Docs image built!"
            '');
          };

          # Build ra-web Docker image
          # Usage: nix run .#docker-build-web
          docker-build-web = {
            type = "app";
            program = toString (pkgs.writeShellScript "docker-build-web" ''
              set -e
              echo "🐳 Building ra-web Docker image..."
              ${pkgs.docker}/bin/docker compose build ra-web
              echo "✅ Ra-web image built!"
            '');
          };

          # Build PostgreSQL + Ra extension Docker image
          # Usage: nix run .#docker-build-postgres-extension
          docker-build-postgres-extension = {
            type = "app";
            program = toString (pkgs.writeShellScript "docker-build-postgres-extension" ''
              set -e
              echo "🐳 Building PostgreSQL + Ra extension image..."
              ${pkgs.docker}/bin/docker compose build postgres-ra-extension
              echo "✅ PostgreSQL + Ra extension image built!"
            '');
          };

          # Build PostgreSQL 19 + Ra proxy Docker image
          # Usage: nix run .#docker-build-postgres-proxy
          docker-build-postgres-proxy = {
            type = "app";
            program = toString (pkgs.writeShellScript "docker-build-postgres-proxy" ''
              set -e
              echo "🐳 Building PostgreSQL 19 + Ra proxy image..."
              echo "⚠️  This build takes 30-45 minutes (PostgreSQL from source)"
              ${pkgs.docker}/bin/docker compose build postgres-ra-proxy
              echo "✅ PostgreSQL + Ra proxy image built!"
            '');
          };

          # Start all Docker services
          # Usage: nix run .#docker-up
          docker-up = {
            type = "app";
            program = toString (pkgs.writeShellScript "docker-up" ''
              set -e
              echo "🐳 Starting all Docker services..."
              ${pkgs.docker}/bin/docker compose up -d
              echo ""
              echo "✅ Services started!"
              echo ""
              echo "Check status:"
              echo "   docker compose ps"
              echo ""
              echo "View logs:"
              echo "   docker compose logs -f"
            '');
          };

          # Stop all Docker services
          # Usage: nix run .#docker-down
          docker-down = {
            type = "app";
            program = toString (pkgs.writeShellScript "docker-down" ''
              set -e
              echo "🐳 Stopping all Docker services..."
              ${pkgs.docker}/bin/docker compose down
              echo "✅ Services stopped!"
            '');
          };
        };
      }
    );
}
