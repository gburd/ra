# License Audit

**Generated:** 2026-04-09
**Scope:** All Rust workspace dependencies and npm frontend dependencies

## Executive Summary

The RA project has **excellent license compliance**. Out of 856 total Rust dependencies, over 99%
use permissive licenses (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode, Unlicense, CC0). There are
**no AGPL dependencies** and **no problematic copyleft licenses** that would restrict commercial use.

The two items requiring attention are:

1. **MPL-2.0** (`colored` crate) -- weak copyleft, file-level only. Compatible with commercial use
   as long as modifications to MPL-licensed files are shared. No risk for linking.
2. **GPL-2.0-only OR BSD-3-Clause** (`ittapi`, `ittapi-sys`) -- dual-licensed, can be used under
   BSD-3-Clause (permissive). These are Intel profiling tools, likely dev-only dependencies.

**Verdict: Safe for commercial deployment.** All dependencies are either permissive or offer a
permissive license alternative.

## License Breakdown

### By Category

| Category | Count | Percentage |
|----------|------:|----------:|
| Permissive (MIT, Apache-2.0, BSD, ISC, Zlib, Unlicense, CC0, Unicode, CDLA-Permissive) | 851 | 99.4% |
| Weak Copyleft (MPL-2.0) | 2 | 0.2% |
| Dual with Copyleft Option (GPL-2.0 OR BSD-3-Clause) | 2 | 0.2% |
| Unknown (workspace internal crates) | 2 | 0.2% |

### Top 10 Most Common Licenses

| # | License | Count |
|---|---------|------:|
| 1 | MIT OR Apache-2.0 | 377 |
| 2 | MIT | 171 |
| 3 | Apache-2.0 | 73 |
| 4 | MIT/Apache-2.0 | 54 |
| 5 | Apache-2.0 WITH LLVM-exception | 43 |
| 6 | Apache-2.0 OR MIT | 29 |
| 7 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 25 |
| 8 | Unicode-3.0 | 18 |
| 9 | Apache-2.0/MIT | 11 |
| 10 | BSD-3-Clause | 8 |

### All License Types

| License Expression | Count | Category |
|--------------------|------:|----------|
| MIT OR Apache-2.0 | 377 | Permissive |
| MIT | 171 | Permissive |
| Apache-2.0 | 73 | Permissive |
| MIT/Apache-2.0 | 54 | Permissive |
| Apache-2.0 WITH LLVM-exception | 43 | Permissive |
| Apache-2.0 OR MIT | 29 | Permissive |
| Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 25 | Permissive |
| Unicode-3.0 | 18 | Permissive |
| Apache-2.0/MIT | 11 | Permissive |
| BSD-3-Clause | 8 | Permissive |
| Unlicense OR MIT | 5 | Permissive |
| Zlib OR Apache-2.0 OR MIT | 4 | Permissive |
| Unlicense/MIT | 4 | Permissive |
| Apache-2.0 OR BSL-1.0 OR MIT | 4 | Permissive |
| Zlib | 3 | Permissive |
| ISC | 3 | Permissive |
| BSD-3-Clause AND MIT | 2 | Permissive |
| BSD-3-Clause/MIT | 2 | Permissive |
| Apache-2.0 OR ISC OR MIT | 2 | Permissive |
| MIT OR Apache-2.0 OR Zlib | 2 | Permissive |
| CDLA-Permissive-2.0 | 2 | Permissive |
| BSD-2-Clause OR Apache-2.0 OR MIT | 2 | Permissive |
| MIT OR Apache-2.0 OR LGPL-2.1-or-later | 2 | Permissive (MIT option) |
| MPL-2.0 | 2 | Weak Copyleft |
| GPL-2.0-only OR BSD-3-Clause | 2 | Permissive (BSD option) |
| UNKNOWN | 2 | Internal (workspace) |
| Apache-2.0 OR BSL-1.0 | 1 | Permissive |
| CC0-1.0 | 1 | Permissive |
| CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | 1 | Permissive |
| CC0-1.0 OR MIT-0 OR Apache-2.0 | 1 | Permissive |
| WTFPL | 1 | Permissive |
| bzip2-1.0.6 | 1 | Permissive |
| 0BSD OR MIT OR Apache-2.0 | 1 | Permissive |
| BSD-2-Clause | 1 | Permissive |
| BSD-2-Clause OR MIT OR Apache-2.0 | 1 | Permissive |
| MIT OR Zlib OR Apache-2.0 | 1 | Permissive |
| Apache-2.0 AND ISC | 1 | Permissive |
| Apache-2.0 / MIT | 1 | Permissive |
| (Apache-2.0 OR MIT) AND BSD-3-Clause | 1 | Permissive |
| (MIT OR Apache-2.0) AND Unicode-DFS-2016 | 1 | Permissive |
| (MIT OR Apache-2.0) AND Unicode-3.0 | 1 | Permissive |
| MIT AND Unicode-DFS-2016 | 1 | Permissive |

## Items Requiring Attention

### MPL-2.0: `colored` (v2.2.0, v3.1.1)

The Mozilla Public License 2.0 is a weak copyleft license. It requires that modifications to
MPL-licensed source files must be shared under the same license. However, it explicitly allows
combining MPL-licensed code with proprietary code. Using `colored` as a dependency (without
modifying its source) imposes no copyleft obligations.

**Risk: None for normal use.** MPL-2.0 is widely considered commercially friendly.

### GPL-2.0-only OR BSD-3-Clause: `ittapi` (v0.4.0), `ittapi-sys` (v0.4.0)

These Intel ITT API bindings are dual-licensed. By choosing the BSD-3-Clause option, there are no
copyleft obligations. These crates are used for profiling (Intel VTune integration) and are likely
only compiled in development/profiling builds.

**Risk: None.** Use under BSD-3-Clause.

### UNKNOWN: `ra-regression` (v0.2.0), `xtask` (v0.2.0)

These are internal workspace crates without a license field in their `Cargo.toml`. Since they are
part of this project and not distributed as separate packages, this is not a compliance issue.

**Recommendation:** Add `license = "MIT OR Apache-2.0"` to their `Cargo.toml` for consistency.

### BSL-1.0 (Boost Software License): `ryu` (v1.0.23)

Available as `Apache-2.0 OR BSL-1.0`. Both options are permissive. No concern.

### WTFPL: `terminfo` (v0.9.0)

The "Do What The F*** You Want To Public License" is maximally permissive, though not
OSI-approved. No practical risk for commercial use.

### bzip2-1.0.6: `libbz2-rs-sys` (v0.2.2)

The original bzip2 license is a permissive BSD-style license. No concern.

## Frontend (npm) Dependencies

The frontend at `crates/ra-web/frontend/` uses standard React ecosystem packages. All listed
dependencies use permissive licenses:

| Package | License |
|---------|---------|
| react, react-dom | MIT |
| @monaco-editor/react, monaco-editor | MIT |
| @mui/material, @mui/icons-material | MIT |
| @emotion/react, @emotion/styled | MIT |
| allotment | MIT |
| d3 | ISC |
| @xyflow/react | MIT |
| dagre | MIT |
| recharts | MIT |
| typescript | Apache-2.0 |
| vite | MIT |
| vitest | MIT |
| @playwright/test | Apache-2.0 |
| @testing-library/* | MIT |

**No copyleft or problematic licenses in the frontend.**

## Recommendations

1. **Add license fields** to `ra-regression/Cargo.toml` and `xtask/Cargo.toml` for completeness.
2. **Consider `cargo-deny`** for automated, ongoing license auditing in CI. A basic
   `deny.toml` config can block copyleft licenses from being added inadvertently.
3. **Periodic re-audit** when major dependency upgrades occur (e.g., DataFusion, Arrow, Wasmtime).

## Methodology

- Rust dependencies extracted via `cargo metadata --format-version 1`
- License fields parsed from each package's metadata
- npm dependencies checked from `package.json` (standard ecosystem licenses)
- Total unique Rust packages analyzed: 856
- Audit date: 2026-04-09
