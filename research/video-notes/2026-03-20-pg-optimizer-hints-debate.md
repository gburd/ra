# PostgreSQL Optimizer Hints Debate

**Source:** https://wiki.postgresql.org/wiki/OptimizerHintsDiscussion
**Date:** Reference (ongoing)
**Speaker:** PostgreSQL community

## Key Points
- PostgreSQL officially rejects traditional hint systems
- Community position: hints cause more harm than good
- Alternative mechanisms exist: enable_* GUCs, plan_cache_mode, pg_plan_advice
- Novel approaches could merit consideration

## Problems with Hints (PostgreSQL Position)
1. **Maintenance burden**: hints in queries require refactoring during schema/version changes
2. **Upgrade incompatibility**: hints optimal for v14 may be harmful in v15
3. **Encourage bad practices**: superficial fixes instead of root causes
4. **Scale sensitivity**: hints for 1M rows fail at 1B rows
5. **Optimizer often better**: human-chosen plans frequently worse
6. **Reduced bug reporting**: hints mask optimizer issues

## Where Hints Have Value
- One-time ad-hoc queries (no maintenance concern)
- Testing different execution paths for debugging
- Working around known optimizer bugs
- Reproducing specific plans for benchmarking

## Alternative Approaches in PostgreSQL
1. **enable_* parameters**: toggle plan node types
2. **Cost parameters**: adjust seq_page_cost, random_page_cost, etc.
3. **Statistics targets**: ALTER TABLE SET STATISTICS
4. **Extended statistics**: CREATE STATISTICS for correlations
5. **pg_plan_advice (v19)**: structured plan advice system
6. **Third-party**: pg_hint_plan extension

## Applicable to RA
- Gap: No plan stability mechanism (detect and prevent plan regressions)
- Gap: No plan comparison infrastructure (compare plans across optimizer versions)
- Gap: No debugging tools for optimizer decisions (why was this plan chosen?)
- Gap: No mechanism to lock a known-good plan
- Gap: No A/B testing framework for plan alternatives

## References
- PostgreSQL wiki: OptimizerHintsDiscussion
- Haas. "pg_plan_advice" blog post (2026)
- pg_hint_plan: https://github.com/ossc-db/pg_hint_plan
