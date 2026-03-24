# Rule Dependency Graph

This graph shows the 1,327+ transformation rules organized by category, with dependency edges showing how rule categories interact during query optimization.

<div class="dependency-graph-container">
  <object type="image/svg+xml" data="/ra/images/rule-dependency-graph.svg" class="dependency-graph">
    <img src="/ra/images/rule-dependency-graph.svg" alt="Rule Dependency Graph" />
  </object>
</div>

<style>
.dependency-graph-container {
  overflow-x: auto;
  margin: 1.5rem 0;
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  padding: 1rem;
  background: var(--vp-c-bg);
}
.dependency-graph {
  width: 100%;
  min-width: 600px;
  height: auto;
}
.dark .dependency-graph-container {
  background: var(--vp-c-bg-soft);
}
</style>

## Categories

| Category | Rules | Description |
|----------|------:|-------------|
| [Database-Specific](./database-specific/) | 357 | System-specific optimizations for 25+ databases |
| [Logical](./logical/) | 210 | Predicate pushdown, join reordering, expression simplification |
| [Physical](./physical/) | 109 | Join algorithms, index selection, access path selection |
| [Execution Models](./execution-models/) | 91 | Vectorized, compiled, push-based, morsel-driven execution |
| [Distributed](./distributed/) | 58 | Partition pruning, co-located joins, exchange placement |
| [Experimental](./experimental/) | 46 | Adaptive, ML-guided, worst-case optimal joins |
| [Cost Models](./cost-models/) | 36 | System R, general cost estimation strategies |
| [Multi-Model](./multi-model/) | 30 | Document, graph, and time-series query optimization |
| [Hardware](./hardware/) | 22 | GPU, FPGA, accelerator-specific optimizations |

## Dependency Relationships

The dashed edges in the graph show how rule categories depend on each other:

- **Logical to Physical** -- Logical rewrites (join reordering, predicate pushdown) feed into physical plan selection
- **Physical to Distributed** -- Physical operators are distributed across nodes via exchange placement
- **Physical to Hardware** -- Physical access paths can be offloaded to GPU/FPGA accelerators
- **Cost Models to Physical/Logical** -- Cost estimation guides both physical operator selection and logical rewrite decisions
- **Experimental to Logical/Physical** -- Research techniques introduce new rewrite patterns and operators
- **Multi-Model to Logical** -- Document and graph models require model-specific logical rewrites

## Source

The dependency graph is generated from [`rules/DEPENDENCY_GRAPH.dot`](https://codeberg.org/gregburd/ra/src/tag/v0.1.0/rules/DEPENDENCY_GRAPH.dot) using Graphviz:

```bash
dot -Tsvg rules/DEPENDENCY_GRAPH.dot -o docs/public/images/rule-dependency-graph.svg
```

Click any node in the graph to navigate to its rule category documentation.
