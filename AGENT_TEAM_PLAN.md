# Agent Team Worktree Plan

## Overview

Three parallel tracks using git worktrees, each with a dedicated agent:

1. **Track A (ra-web):** Quick win - complete web interface (~8 hours)
2. **Track C (coverage):** Test coverage to >90% (~1-2 weeks)
3. **Track B (parser):** Comprehensive parser redesign (~28 weeks)

## Worktree Setup

### Track A: ra-web Completion
```bash
# Branch: track-a-ra-web
# Directory: .claude/worktrees/track-a-ra-web
git worktree add .claude/worktrees/track-a-ra-web -b track-a-ra-web main
```

**Agent Tasks:**
1. Build WASM bindings (`wasm-pack build`) - 30 min
2. Create 5 HTML demos:
   - `/static/index-selection.html`
   - `/static/subquery-unnesting.html`
   - `/static/parallel-query.html`
   - `/static/gpu-offloading.html`
   - `/static/distributed-query.html`
3. Backend integration (connect to real optimizer) - 1 hour
4. Plan visualization (D3.js/Mermaid) - 3 hours
5. Testing & polish - 2 hours

**Success Criteria:**
- 10/10 interactive demos functional
- WASM bindings working
- Real optimizer backend integrated
- Plan visualization with interactive nodes
- All 29 tests passing

---

### Track C: Test Coverage Improvement
```bash
# Branch: track-c-coverage
# Directory: .claude/worktrees/track-c-coverage
git worktree add .claude/worktrees/track-c-coverage -b track-c-coverage main
```

**Agent Tasks:**
1. Measure current coverage: `cargo llvm-cov --html --open`
2. Identify gaps in coverage report
3. Add tests for:
   - Untested branches in dialect handling
   - Error paths
   - Hardware profile configurations
   - Cost model edge cases
   - Selectivity estimation
4. Focus areas:
   - `ra-dialect/` - dialect-specific features
   - `ra-engine/` - cost model, calibration
   - `ra-stats/` - statistics handling
5. Verify >90% coverage across all crates

**Success Criteria:**
- >90% line coverage
- >85% branch coverage
- All error paths tested
- Edge cases covered
- Coverage report generated

---

### Track B: Parser Redesign
```bash
# Branch: track-b-parser
# Directory: .claude/worktrees/track-b-parser
git worktree add .claude/worktrees/track-b-parser -b track-b-parser main
```

**Agent Tasks - Week 1:**
1. Write RFC 0106: Comprehensive SQL Parser Architecture (~3000 lines)
   - Standards-based parsing (SQL-86 through SQL:2023)
   - Profile system architecture
   - Grammar extension mechanism
   - Dialect inference algorithm
   - Build-time composition
   - Performance targets

2. Write RFC 0111: Configuration Externalization (~1200 lines)
   - Selectivity defaults
   - Staleness factors
   - Base operator costs
   - Calibration parameters
   - Query complexity thresholds
   - Resource budget profiles
   - Rule priority benefit ranges

**Agent Tasks - Week 2-3:**
1. Create directory structure:
   ```
   crates/ra-parser/src/
   ├── parser/
   │   ├── mod.rs
   │   ├── ra_parser.rs
   │   └── inference.rs
   ├── profile/
   │   ├── mod.rs
   │   ├── loader.rs
   │   └── registry.rs
   └── grammar/
       ├── mod.rs
       └── extension.rs
   ```

2. Implement profile system:
   - Profile TOML format
   - Profile loader with validation
   - Initial profiles (universal, postgresql-17, mysql-8.4)

3. Implement RaParser facade

**Success Criteria:**
- RFC 0106 and 0111 complete and reviewed
- Directory structure in place
- Profile system working with 3 initial profiles
- RaParser facade functional
- All existing tests pass
- Backward compatibility maintained (initially)

---

## Agent Coordination

### Communication
- Each agent works independently in its own worktree
- Progress reports via task updates
- Conflicts resolved by main coordinator (you)

### Conflict Resolution
- Track A and C unlikely to conflict (different files)
- Track B isolated until ready to merge
- Main branch updates pulled into worktrees periodically

### Merge Strategy
1. **Track A (ra-web):** Merge first (~1-2 days)
2. **Track C (coverage):** Merge second (~1-2 weeks)
3. **Track B (parser):** Long-running, periodic RFC reviews

---

## Execution Plan

1. Wait for docs build to complete
2. Create all three worktrees
3. Spawn three agents in parallel:
   - Agent A (general-purpose) → Track A worktree
   - Agent C (general-purpose) → Track C worktree
   - Agent B (Plan agent for RFCs, then general-purpose) → Track B worktree
4. Monitor progress via task updates
5. Merge completed tracks in order

---

## Current Status

- ✅ Phase 0 complete (compilation, tests, clippy)
- 🔄 Docs build in progress (VitePress compiling 40 RFCs)
- ⏳ Waiting to start agent team
