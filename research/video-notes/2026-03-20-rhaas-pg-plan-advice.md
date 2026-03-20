# Robert Haas: pg_plan_advice - Plan Stability and User Planner Control

**Source:** https://rhaas.blogspot.com/2026/03/pgplanadvice-plan-stability-and-user.html
**Date:** 2026-03-04
**Speaker:** Robert Haas

## Key Points
- PostgreSQL 19 proposal for three contrib modules for plan control
- Separates mechanism from policy - infrastructure for plan stability
- Generates and applies "plan advice" strings
- Addresses the long-standing debate about optimizer hints in PostgreSQL

## The Three Modules

### pg_plan_advice (Core)
- Generates plan advice strings from EXPLAIN output
- Advice strings describe plan structure: JOIN_ORDER, HASH_JOIN, SEQ_SCAN, etc.
- Users apply selective constraints via GUC settings
- Foundation layer that other extensions build upon

### pg_collect_advice
- Extends core with sophisticated advice collection
- Demonstrates pluggable architecture
- Can collect advice across workloads

### pg_stash_advice
- Automatic plan advice application by query identifier
- Stores advice in dynamic shared memory
- System-wide planner control without application modification
- Administrators configure via ALTER SYSTEM

## Design Philosophy
- "Mechanism, not policy" - provides primitives
- Extensions build higher-level plan management
- Addresses plan regression across PostgreSQL upgrades
- Avoids the problems PostgreSQL community identified with traditional hints

## Optimization Techniques Discussed
1. **Plan stability**: Prevent plan regression across upgrades/statistics changes
2. **Join order control**: Force specific join sequences
3. **Access method selection**: Force seq scan, index scan, bitmap scan
4. **Join method selection**: Force hash join, merge join, nested loop
5. **Plan advice composition**: Combine multiple advice constraints

## Applicable to RA
- Gap: No plan stability / plan pinning mechanism
- Gap: No "plan advice" generation from optimized plans
- Gap: No mechanism to constrain optimizer search space per query
- Gap: No plan regression detection (compare plan quality across optimizer versions)
- Gap: No workload-aware plan caching

## References
- PostgreSQL wiki: OptimizerHintsDiscussion
- pg_hint_plan extension (third-party)
- Oracle SQL Plan Management
