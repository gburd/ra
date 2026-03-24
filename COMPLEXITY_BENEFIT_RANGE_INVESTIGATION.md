# Rule Complexity and Benefit Range Investigation

**Date:** 2026-03-24
**Status:** COMPLETE
**Finding:** Metadata declared in rule files but NOT used by optimizer

## Executive Summary

Rule files (`.rra` format) include optional metadata fields:
- `complexity: O(1)` - Time complexity of rule application (lines 67-69 of rule-authoring guide)
- `benefit_range: [0.0, 0.8]` - Min/max estimated benefit (0-1 scale)

**Current Status:** These fields are documented in the authoring guide and present in example rule files, but **the optimizer does not read, parse, or use them**. The infrastructure to support these is entirely absent from the codebase.

## Detailed Investigation

### 1. Rule Metadata Structure

**Location:** `/Users/gregburd/src/ra/crates/ra-engine/src/rule_metadata.rs:14-41`

The `RuleMetadata` struct parses rule file YAML frontmatter:

```rust
pub struct RuleMetadata {
    pub id: String,
    pub name: String,
    pub category: String,
    pub databases: Vec<String>,
    pub standard: Option<String>,
    pub version: String,
    pub authors: Vec<String>,
    pub tags: Vec<String>,
    pub preconditions: Vec<Precondition>,
}
```

**Finding:** The struct does NOT contain fields for `complexity` or `benefit_range`. These fields are accepted by the YAML parser (via serde default behavior) but then silently discarded.

### 2. Example Rule Files

**File:** `/Users/gregburd/src/ra/tests/fixtures/valid-complex-rule.rra:1-12`

```yaml
id: join-commutativity
name: Join Commutativity
category: logical/join-reordering
complexity: O(1)
benefit_range: [0.0, 0.8]
```

The rule file declares complexity and benefit_range, but these are never extracted or used.

**File:** `/Users/gregburd/src/ra/tests/fixtures/valid-simple-rule.rra:1-5`

```yaml
id: filter-pushdown-basic
name: Basic Filter Pushdown
category: logical/predicate-pushdown
```

(No complexity/benefit_range - optional fields)

### 3. Rule Loading and Parsing

**Location:** `/Users/gregburd/src/ra/crates/ra-engine/src/rule_metadata.rs:97-121`

The `parse_rra_file()` function:
1. Splits YAML frontmatter from markdown content
2. Deserializes frontmatter with `serde_yaml::from_str()`
3. Returns `ParsedRule { metadata, content }`

The serde deserializer will **silently ignore** unknown fields during deserialization (default behavior). Any YAML fields not matching struct members are dropped.

**Confirmation:** Test at line 1098-1128 parses a rule with preconditions but doesn't test complexity/benefit_range.

### 4. Rule Application in Optimizer

**Location:** `/Users/gregburd/src/ra/crates/ra-engine/src/egraph.rs` (~3,800 lines)

The optimizer uses a multi-pass e-graph saturation strategy:
- Loads rules from `.rra` files
- Applies rules repeatedly until saturation
- No prioritization based on rule metadata

Search results for "complexity" in egraph.rs reveal:
- Line 1052+: Query complexity classification (adaptive iteration limits)
- Line 1060+: Complexity-based timeout selection
- **NO references to rule complexity or benefit_range**

### 5. Rule Registry

**Location:** `/Users/gregburd/src/ra/crates/ra-engine/src/rule_registry.rs:1-100`

```rust
pub struct RuleInfo {
    pub id: RuleId,
    pub name: &'static str,
    pub category: &'static str,
}
```

**Finding:** RuleInfo contains only id, name, and category. No complexity or benefit metrics.

### 6. Precondition System (Related but Different)

**Location:** `/Users/gregburd/src/ra/crates/ra-engine/src/rule_metadata.rs:43-72`

The system DOES have a sophisticated precondition framework:
- Hardware requirements (GPU, CPU cores, memory)
- Database compatibility
- Feature flags
- Predicate conditions

This is fully implemented and used to filter applicable rules before optimization. However, rule prioritization is NOT based on complexity or benefit_range.

## Architecture Gap Analysis

### What's Implemented
1. ✅ YAML parsing of rule files with serde
2. ✅ Precondition filtering based on runtime facts
3. ✅ Rule application in e-graph saturation
4. ✅ Query complexity classification (separate from rule complexity)

### What's Missing
1. ❌ Rule metadata struct fields for complexity and benefit_range
2. ❌ Parsing/extraction of these fields from YAML
3. ❌ Rule prioritization algorithm based on complexity/benefit
4. ❌ Integration with optimizer's rule application order
5. ❌ Cost-benefit analysis during optimization

## Why This Matters

Without complexity/benefit_range usage:
- All rules are treated equally (same priority)
- High-benefit rules may be skipped if saturation occurs early
- Low-complexity rules aren't prioritized for time-limited optimization
- Can't implement "apply high-benefit, low-complexity rules first" strategy
- Related to RFC 0052 (Progressive Reoptimization) which needs intelligent rule ordering

## Recommendations

### Option A: Remove Unused Metadata (Minimal Change)
- Delete `complexity` and `benefit_range` from rule-authoring guide
- Update rule examples to remove these fields
- Avoids confusion about unsupported features

### Option B: Implement Full Support (Recommended)
Create RFC 0058: "Rule Complexity Prioritization" with:

1. **Metadata Extension**
   - Add fields to `RuleMetadata` struct:
     ```rust
     pub complexity: Option<String>, // "O(1)", "O(n)", "O(n²)"
     pub benefit_range: Option<(f64, f64)>, // (min, max), normalized 0-1
     ```
   - Update serde deserialization

2. **Prioritization Algorithm**
   - Sort rules by priority score: `benefit_range.min / (complexity_weight)`
   - Apply high-score rules first in each saturation iteration
   - Respect existing precondition filters

3. **Integration Points**
   - `egraph.rs`: Modify rule application loop to sort rules before iteration
   - `rule_registry.rs`: Include complexity/benefit in RuleInfo
   - Tests: Verify prioritization affects rule application order

4. **Backwards Compatibility**
   - Make fields optional (default to neutral priority)
   - Existing rules without metadata continue to work
   - New rules can opt into prioritization

## Code References

| Component | File | Lines | Status |
|-----------|------|-------|--------|
| Rule metadata struct | rule_metadata.rs | 14-41 | Missing complexity/benefit_range fields |
| Parse YAML | rule_metadata.rs | 97-121 | Silently discards unknown fields |
| Example rule | valid-complex-rule.rra | 1-12 | Has fields but not parsed |
| Optimizer loop | egraph.rs | 1000+ | No rule prioritization |
| Rule registry | rule_registry.rs | 90-94 | No complexity/benefit tracking |
| Guide (outdated) | rule-authoring.md | 67-69 | Documents unused fields |

## Next Steps

1. **If removing:** Update documentation to remove complexity/benefit_range mentions
2. **If implementing:** Create RFC with algorithm design
3. **Either way:** Add validation to prevent confusion about what's supported

