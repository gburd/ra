# Restoring the pgrx pg19 fork

`crates/ra-pg-extension` depends (path dependency) on a **forked pgrx** at
`/Users/gregburd/src/pgrx` that adds PostgreSQL 19 support, which upstream pgrx
(≤ 0.18.1) does not have. If that directory is deleted, the PG extension cannot
build (`cargo build --features pg19` fails with "unable to update
/Users/gregburd/src/pgrx/pgrx"). This file records exactly how to rebuild it.

## Rebuild steps

```bash
# 1. Clone upstream pgrx at the version cargo-pgrx is pinned to (0.18.0).
cd /Users/gregburd/src
git clone --branch v0.18.0 --depth 1 https://github.com/pgcentralfoundation/pgrx.git pgrx
```

Then apply the pg19 patches below. PG19 (19devel) is ABI-compatible with PG18
for everything the extension touches, so **pg19 mirrors pg18** throughout.

### 1. Version support — `pgrx-pg-config/src/lib.rs`
- `enum PgMinorVersion`: add a `Devel` variant (PG19 reports `19devel`, which has
  no numeric minor). Add it to `Display` (`"devel"`), to `version()`
  (`Devel => None`), and to `parse_version_str` (detect `"devel"` like
  `"beta"`/`"rc"` and `return Ok((major, PgMinorVersion::Devel))` before the
  numeric-minor parse).
- `SUPPORTED_VERSIONS()`: add `PgVersion::new(19, PgMinorVersion::Latest, None)`.

### 2. Feature flags
- `pgrx-pg-sys/Cargo.toml`: `pg19 = []`
- `pgrx/Cargo.toml`: `pg19 = ["pgrx-pg-sys/pg19"]`
- `pgrx-tests/Cargo.toml`: `pg19 = ["pgrx/pg19"]`

### 3. bindgen input — `pgrx-pg-sys/include/pg19.h`
`cp pgrx-pg-sys/include/pg18.h pgrx-pg-sys/include/pg19.h` (same header set).
bindgen auto-discovers `pg{major}.h` by major version, so this is all it needs.

### 4. bindings gating — `pgrx-pg-sys/src/include.rs`
Mirror every `pg18` block to `pg19` (6 spots): the `mod pg18 { include!(...pg18.rs) }`
+ docsrs variant, `pub use pg18::*;`, the `pg18_oids` module + `pub use`, and the
`internal::pg18` module (`IndexBuildHeapScan` / `QTW_EXAMINE_RTES` /
`AllocSetContextCreateExtended`) + its `pub use`.

### 5. feature guard — `pgrx-pg-sys/src/lib.rs`
Add `feature = "pg19"` to the `not(any(...))` "exactly one feature" `compile_error!`
guard (and its message).

### 6. ABI shims — add pg19 wherever pg18 appears in a `cfg`
In `pgrx-pg-sys/src/{libpq.rs, port.rs, submodules/htup.rs}` and across
`pgrx/src/**`: standalone `#[cfg(feature = "pg18")]` → `#[cfg(any(feature = "pg18",
feature = "pg19"))]`; `feature = "pg18"` inside an `any(...)` → add `feature =
"pg19"`. Do **not** add pg19 to `cfg`s that omit pg18 (those are pre-18 only) and
fix `not(feature = "pg18")` to `not(any(feature = "pg18", feature = "pg19"))`.

### 7. PG19 inlined transaction-id functions — `pgrx-pg-sys/src/port.rs`
PG19 changed `TransactionIdPrecedes` / `PrecedesOrEquals` / `Follows` /
`FollowsOrEquals` from extern functions to `static inline`, so bindgen no longer
emits them. Add `#[cfg(feature = "pg19")]` Rust shims mirroring the modulo-2^32
logic in `access/transam.h` (use `TransactionIdIsNormal` + `TransactionId::into_inner`).

### 8. `pg_module_magic!` — `pgrx/src/lib.rs`
PG18/PG19 use the new `Pg_magic_struct` with an `abi_fields: Pg_abi_values` block
and `name`/`version` pointers instead of the flat `version`/`funcmaxargs`/
`indexmaxkeys`/`namedatalen`/`float8byval` fields. The pg18 arms already handle
this; just ensure pg19 is grouped with pg18 (`any(feature = "pg18", feature =
"pg19")`) and the pre-18 fields are gated `not(any(feature = "pg18", feature =
"pg19"))`.

### 9. macOS bindgen `-isysroot` fix — `pgrx-bindgen/src/build.rs`
PG19-devel may have been compiled against an SDK path that no longer exists.
In `extra_bindgen_clang_args`, when a `-isysroot` path doesn't exist, rewrite it
in the emitted clang args to `xcrun --show-sdk-path` (otherwise bindgen and the
cshim `cc` fail with "assert.h not found").

## Build + install

```bash
cd /Users/gregburd/src/ra/crates/ra-pg-extension
PGC=/usr/local/pgsql/bin/pg_config
export PGRX_PG_CONFIG_PATH=$PGC LIBCLANG_PATH=/opt/homebrew/opt/llvm/lib PATH="$HOME/.cargo/bin:$PATH"
cargo pgrx install --no-default-features --features pg19 --pg-config $PGC --release
/usr/local/pgsql/bin/pg_ctl -D /Volumes/scratch/ra/pg19data -o "-p 5435 -k /tmp" -l /tmp/pg19.log restart
```

`cargo-pgrx` itself must be the 0.18.0 build from this fork
(`cargo install --path /Users/gregburd/src/pgrx/cargo-pgrx --locked`); the
installed binary already is.

## Note
This fork is not version-controlled with a remote. Consider pushing it to
`codeberg.org/gregburd/pgrx` (branch `feature/pg19-devel`) so a delete is
recoverable without re-deriving these patches.
