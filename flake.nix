{
  description = "Relational Algebra Rule System";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
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

            # Deployment
            flyctl

            # System libraries
            libiconv
            darwin.apple_sdk.frameworks.Security
            darwin.apple_sdk.frameworks.SystemConfiguration
          ] ++ lib.optionals stdenv.isDarwin [
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
            echo "  cargo run --bin ra-cli -- <args>"
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
      }
    );
}
