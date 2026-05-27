# Ballista (Distributed Execution)

[Apache DataFusion Ballista][ballista] is a distributed SQL query engine
built on Apache Arrow and DataFusion. Ra's relationship with Ballista
is symmetric to Ra's relationship with PostgreSQL: Ra plans the query,
the host engine executes it.

[ballista]: https://datafusion.apache.org/ballista/

## Status

**Design phase.** No code yet. The design is captured in
[RFC 0086 — Ballista Plan Emission](../../rfcs/text/0086-ballista-plan-emission.md).

## What it would look like

```rust
use ra_parser::sql_to_relexpr;
use ra_engine::Optimizer;
use ra_ballista::ToLogicalPlan;          // crate proposed by RFC 0086

let optimised = Optimizer::new().optimize(
    &sql_to_relexpr("SELECT customer_id, SUM(amount) FROM orders \
                     WHERE created_at > '2026-01-01' \
                     GROUP BY customer_id")?
)?;
let logical_plan = optimised.to_datafusion_logical_plan(&catalog)?;

let ctx = ballista::prelude::BallistaContext::remote(
    "scheduler.example.com", 50050, &Default::default()
).await?;
ctx.create_dataframe_from_logical_plan(logical_plan)?
   .collect().await?;
```

## Mapping summary

`RelExpr` and DataFusion's `LogicalPlan` are nearly isomorphic.

| Concern | Approach |
|---|---|
| Relational operators | Direct: `Scan/Filter/Project/Join/Aggregate/Sort/Limit/Union/...` map 1:1 to DataFusion `LogicalPlan` variants |
| Scalar functions | Function-name dictionary with three policies: stdlib, equivalent-name rewrite, register-as-UDF or surface error |
| Distribution decisions (v1) | None — emit a logical plan without `Repartition` nodes; let Ballista's scheduler stage |
| Distribution decisions (v2) | Emit `Repartition`/broadcast hints driven by Ra's existing distributed cost model |
| Wire format | DataFusion-proto (protobuf), pinned to a specific DataFusion version. Substrait possibly later |

## See also

- [RFC 0086 — Ballista Plan Emission](../../rfcs/text/0086-ballista-plan-emission.md) — full design
- [RFC 0006 — Distributed Query Optimization](../../rfcs/0006-distributed-optimization.md) — Ra's existing distributed cost model
- [`docs/rules/distributed/`](../rules/distributed/) — distribution-aware rewrite rules
- [Ballista architecture overview][ballista-arch]

[ballista-arch]: https://datafusion.apache.org/ballista/contributors-guide/architecture.html
