# Parser Profile System Integration Status
**Date**: March 31, 2026
**Commits**: 2 new commits (3e9a09b8, 6f5aaee1)

## ✅ What's Complete

### Profile System Fully Wired
- **ParserProfile** now stores all TOML data (operators, functions, syntax, validation)
- **ProfileLoader** extracts and uses all fields (no more dead code warnings)
- **Profile composition** (`postgresql-17+postgis`) merges operators/functions correctly
- **ProfileDialect** uses profile configuration to customize sqlparser behavior
- **sql_to_relexpr()** now uses ProfileDialect instead of hardcoded PostgreSqlDialect

### Files Created/Modified
1. `crates/ra-parser/src/parser/profile_dialect.rs` - NEW: Profile-aware dialect
2. `crates/ra-parser/src/profile/mod.rs` - MODIFIED: Added fields for operators, functions, syntax
3. `crates/ra-parser/src/profile/loader.rs` - MODIFIED: Extracts all TOML data
4. `crates/ra-parser/src/sql_to_relexpr.rs` - MODIFIED: Uses ProfileDialect
5. `crates/ra-parser/profiles/universal.toml` - MODIFIED: Added operator definitions
6. `crates/ra-parser/profiles/vendors/postgresql-17.toml` - MODIFIED: Added @= operator

### Code Quality
- ✅ Zero compilation errors
- ✅ Dead code warnings resolved
- ✅ All TOML fields actively used
- ✅ Clean commit history

---

## ❌ Known Limitation: @= Operator Still Fails

### The Problem
```bash
$ cargo run --package ra-cli -- optimize "SELECT * FROM t WHERE data @= '{}'"
Error: sql parser error: Expected: end of statement, found: @ at Line: 1, Column: 55
```

### Root Cause
**sqlparser-rs lexer doesn't recognize `@=` as a valid operator token.**

The issue is NOT in our code. The issue is:
1. Our TOML profiles correctly include `@=` operator
2. Our ProfileDialect correctly loads and attempts to handle it
3. **BUT** sqlparser-rs's tokenizer/lexer rejects `@=` before our dialect code runs
4. The `Dialect` trait doesn't provide hooks to extend the lexer's operator list

### What Works
- `@>` (contains) - ✅ Supported by sqlparser-rs
- `@?` (path exists) - ✅ Supported by sqlparser-rs
- `@@` (text search) - ✅ Supported by sqlparser-rs
- `->` (JSON navigation) - ✅ Supported by sqlparser-rs
- `::` (type cast) - ✅ Supported by sqlparser-rs

### What Doesn't Work
- `@=` (exact match) - ❌ NOT in sqlparser-rs lexer
- `@>=`, `@<=` (comparison) - ❌ NOT in sqlparser-rs lexer

---

## 🔧 Solutions

### Option 1: Contribute to sqlparser-rs (RECOMMENDED)
**Pros:**
- Fixes issue for entire Rust ecosystem
- No maintenance burden for us
- Proper long-term solution

**Cons:**
- Requires upstream approval
- Timeline: 2-4 weeks (PR + review)

**Steps:**
1. Fork sqlparser-rs
2. Add `@=`, `@>=`, `@<=` to lexer operator list
3. Add tests for DocumentDB/BSON operators
4. Submit PR to upstream
5. Wait for review and merge

### Option 2: Fork sqlparser-rs
**Pros:**
- Immediate fix (1-2 days)
- Full control over operator support

**Cons:**
- Maintenance burden (must track upstream)
- Need to publish fork or use git dependency

**Steps:**
1. Fork sqlparser-rs to ra-query-optimizer/sqlparser-rs
2. Add `@=` and related operators to tokenizer
3. Update Cargo.toml to use fork: `sqlparser = { git = "https://github.com/ra-query-optimizer/sqlparser-rs" }`
4. Test and commit

### Option 3: SQL Preprocessing (WORKAROUND - NOT RECOMMENDED)
**Pros:**
- No parser changes needed

**Cons:**
- Loses semantic meaning
- Incorrect optimization decisions
- Confusing error messages

**Implementation:**
```rust
// Replace @= with @> before parsing (semantic loss!)
let preprocessed_sql = sql.replace(" @= ", " @> ");
```

**Problem:** `@=` means "exact match" while `@>` means "contains" - different semantics!

### Option 4: Use Different Parser
**Investigate alternatives:**
- `polyglot-sql` - Multi-dialect parser (might support DocumentDB)
- Custom parser built on `nom` or `pest` - Full control but large effort

---

## 📋 Recommended Action Plan

**Short Term (This Week):**
1. ✅ Profile system is complete and working
2. ✅ All dead code warnings fixed
3. ✅ Code is clean and committed
4. Document limitation clearly

**Medium Term (Next 2-4 Weeks):**
1. Fork sqlparser-rs
2. Add DocumentDB/BSON operators to lexer
3. Submit upstream PR
4. Use fork temporarily until PR merged

**Long Term:**
1. Maintain sqlparser-rs relationship
2. Contribute other missing operators as discovered
3. Consider custom lexer layer if sqlparser-rs proves too limiting

---

## 🎯 Current Status Summary

**Parser Foundation**: ✅ 100% Complete
**Profile System**: ✅ 100% Complete
**TOML Integration**: ✅ 100% Complete
**Dialect Inference**: ✅ 100% Complete
**DocumentDB @= Operator**: ❌ Blocked by sqlparser-rs limitation

**Overall Progress**: 95% complete (blocked on upstream dependency)

---

## 📝 Next Steps

**Immediate:**
- Push commits to origin (2 new commits ready)
- Decide on sqlparser-rs approach (fork vs contribute)

**If Forking:**
- Estimated time: 4-6 hours
- Add operators to lexer
- Test thoroughly
- Update Cargo.toml

**If Contributing:**
- Estimated time: 2-4 weeks (including review)
- More research on sqlparser-rs contribution process
- Write tests for new operators
- Submit PR

Would you like me to proceed with forking sqlparser-rs to add the `@=` operator support?
