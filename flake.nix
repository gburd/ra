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
            export RUST_BACKTRACE=1
            export RUST_LOG=info
            export DATABASE_URL="postgres://localhost/ra_dev"
            export TMPDIR="''${TMPDIR:-/tmp}"

            echo "🚀 Relational Algebra dev environment loaded"
            echo "Rust version: $(rustc --version)"
            echo "Cargo version: $(cargo --version)"
            echo ""
            echo "Available commands:"
            echo "  cargo build          - Build all crates"
            echo "  cargo test           - Run tests"
            echo "  cargo clippy         - Run linter"
            echo "  cargo fmt            - Format code"
            echo "  cargo docs           - Serve documentation locally"
            echo "  cargo run --bin ra-cli -- <args>"
            echo ""
            echo "Documentation:"
            echo "  cargo docs           - Serve docs at http://localhost:5173"
            echo "  nix run .#docs       - Alternative using nix"
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
              cd ${./.}/docs

              # Install npm dependencies if needed
              if [ ! -d node_modules ]; then
                echo "📦 Installing npm dependencies..."
                ${pkgs.nodejs_20}/bin/npm install
              fi

              echo "📚 Starting documentation server..."
              echo ""
              echo "Documentation will be available at:"
              echo "   http://localhost:5173/ra/"
              echo ""
              echo "Press Ctrl+C to stop the server"
              echo ""

              exec ${pkgs.nodejs_20}/bin/npm run dev
            '');
          };

          # Build documentation for deployment
          # Usage: nix run .#docs-build
          docs-build = {
            type = "app";
            program = toString (pkgs.writeShellScript "build-docs" ''
              set -e
              cd ${./.}/docs

              # Install npm dependencies if needed
              if [ ! -d node_modules ]; then
                echo "📦 Installing npm dependencies..."
                ${pkgs.nodejs_20}/bin/npm install
              fi

              echo "📚 Building documentation..."
              ${pkgs.nodejs_20}/bin/npm run build:docs

              echo "✅ Documentation built successfully!"
              echo "   Output: docs/.vitepress/dist/"
            '');
          };
        };
      }
    );
}
