# RFC 0083: XPath and XQuery Optimization

- Start Date: 2026-03-25
- Author: Ra Research Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Ra should optimize SQL/XML queries that embed XPath and XQuery expressions
by understanding XML index structures, XPath predicate pushdown, XQuery
FLWOR expression rewriting, and platform-specific XML processing functions.
This RFC extracts optimization principles from Berkeley DB XML (dbxml) and
adapts them to relational SQL contexts where XML data is stored in typed
columns (PostgreSQL `xml`, Oracle `XMLType`, SQL Server `xml`).

## Motivation

XML processing in relational databases suffers from opaque cost estimation.
When a query contains `xpath('/doc/items/item[@price > 100]', data)`, the
planner treats the entire XPath evaluation as a black-box function call
with a fixed cost. It cannot:

1. Push predicates from the XPath expression into the relational plan
2. Exploit XML indexes (path indexes, value indexes) for selective access
3. Rewrite XQuery FLWOR expressions to avoid materialization
4. Estimate cardinality of XPath results for join ordering
5. Simplify redundant path navigation

**Key optimization gaps:**

| Gap | Impact |
|-----|--------|
| Black-box XPath cost | Wrong join ordering when XML data is involved |
| No XML index awareness | Sequential parse of entire documents |
| No predicate extraction | Filters applied after full XML traversal |
| No FLWOR rewriting | Unnecessary intermediate sequences |
| No path simplification | Redundant descendant-or-self navigation |

**Databases with XML support:**

| Database | XML Type | XPath/XQuery Support | Index Types |
|----------|----------|---------------------|-------------|
| PostgreSQL | `xml` | `xpath()`, `xmlexists()`, `xmltable()` | None (parse on access) |
| Oracle | `XMLType` | `XMLQuery()`, `XMLTable()`, `existsNode()` | `XMLIndex`, path/value |
| SQL Server | `xml` | `.value()`, `.query()`, `.exist()`, `.nodes()` | Primary/secondary XML indexes |
| Berkeley DB XML | Native | Full XQuery 1.0 | Path, value, presence, edge, substring |

## Guide-level explanation

### XPath expression classification

Ra classifies XPath expressions into cost tiers based on their
structure and available indexes:

**Tier 1 - Index-only evaluation** (cheapest):
- Simple path with equality predicate: `/doc/id[. = 'ABC']`
- Presence check: `xmlexists('/doc/status')`
- These can be answered entirely from an XML path or value index.

**Tier 2 - Index scan + filter** (moderate):
- Path with range predicate: `/doc/items/item[@price > 100]`
- Path with substring: `/doc/name[contains(., 'Corp')]`
- Index narrows to matching paths; filter evaluates on document.

**Tier 3 - Full document parse** (expensive):
- Wildcard axes: `//*/name`
- Computed predicates: `/doc/items/item[position() mod 2 = 0]`
- No index can accelerate; full XML parse required.

### XQuery FLWOR optimization

XQuery FLWOR expressions (for-let-where-order by-return) map to
relational operations:

```
for $i in /doc/items/item
where $i/@price > 100
order by $i/@price descending
return $i/name
```

Maps to:
```sql
SELECT item.name
FROM xmltable('/doc/items/item' PASSING data
  COLUMNS name text PATH 'name',
          price numeric PATH '@price') AS item
WHERE item.price > 100
ORDER BY item.price DESC
```

Ra rewrites the FLWOR to push the WHERE predicate into the XPath
evaluation, avoiding materialization of all items before filtering.

### Platform-specific optimization

**PostgreSQL:**
```sql
-- Before: black-box xpath call
SELECT * FROM docs
WHERE (xpath('/doc/status/text()', data))[1]::text = 'active';

-- After: Ra rewrites to xmlexists for index potential
SELECT * FROM docs
WHERE xmlexists('/doc/status[text()="active"]' PASSING data);
```

**Oracle:**
```sql
-- Before: full document parse
SELECT * FROM docs
WHERE XMLQuery('/doc/items/item[@price > 100]'
  PASSING data RETURNING CONTENT) IS NOT NULL;

-- After: Ra exploits XMLIndex
SELECT * FROM docs d
WHERE existsNode(d.data, '/doc/items/item[@price > 100]') = 1;
```

**SQL Server:**
```sql
-- Before: .query() returns XML fragment, then checks
SELECT * FROM docs
WHERE data.query('/doc/status') IS NOT NULL;

-- After: Ra uses .exist() which leverages XML index
SELECT * FROM docs
WHERE data.exist('/doc/status') = 1;
```

## Reference-level explanation

### Implementation: xml_optimizer.rs

The module provides three categories of functionality:

#### 1. XPath Expression Analysis

```rust
pub struct XPathExpr {
    pub steps: Vec<XPathStep>,
    pub predicates: Vec<XPathPredicate>,
}

pub struct XPathStep {
    pub axis: XPathAxis,
    pub node_test: NodeTest,
}

pub enum XPathAxis {
    Child, Descendant, DescendantOrSelf, Self_, Parent,
    Ancestor, AncestorOrSelf, Attribute, Following,
    FollowingSibling, Preceding, PrecedingSibling,
}
```

Parsing XPath from SQL string literals into a structured form
enables predicate extraction, cost estimation, and rewriting.

#### 2. XML Index Model

```rust
pub enum XmlIndexType {
    Path,          // Index on path structure (node existence)
    Value,         // Index on path + value (equality/range)
    Presence,      // Index on element/attribute existence
    FullText,      // Full-text index on text content
    Property,      // Computed property index
}

pub struct XmlIndexInfo {
    pub index_type: XmlIndexType,
    pub paths: Vec<String>,
    pub value_type: Option<XmlValueType>,
}
```

#### 3. Rewrite Rules

The module provides egg rewrite rules that integrate with the
existing equality saturation framework:

- **xpath-predicate-extract**: Extract predicates from XPath
  expressions and convert to relational filters
- **xpath-index-scan**: Replace full document parse with index
  lookup when a matching XML index exists
- **flwor-where-pushdown**: Push WHERE clause predicates into
  the XPath navigation step
- **flwor-order-to-sort**: Convert XQuery order-by to relational
  sort operator
- **xpath-path-simplify**: Simplify `/descendant-or-self::node()/`
  to `//` and eliminate redundant self axes
- **xpath-existence-to-xmlexists**: Convert xpath() result null
  checks to xmlexists() calls
- **xmltable-filter-pushdown**: Push filters on xmltable columns
  into the XPath expression

### Cost Model

XPath evaluation cost depends on:

| Factor | Weight | Source |
|--------|--------|--------|
| Document size (avg bytes) | High | Table statistics |
| Path selectivity | High | XML index stats or heuristic |
| Number of navigation steps | Medium | XPath structure |
| Predicate complexity | Medium | Predicate analysis |
| XML index availability | High | Catalog metadata |

Cost formula:

```
xpath_cost = parse_cost + navigation_cost + predicate_cost

parse_cost = avg_doc_bytes * PARSE_COST_PER_BYTE  (if no index)
           = 0                                     (if index covers)

navigation_cost = n_steps * step_cost(axis_type)
  where step_cost(Child) = 1.0
        step_cost(Descendant) = 10.0
        step_cost(Attribute) = 0.5
        step_cost(Parent) = 2.0

predicate_cost = n_predicates * avg_predicate_cost
  where avg_predicate_cost = 1.0 (comparison)
                           = 5.0 (function call)
                           = 10.0 (nested path)
```

### Integration Points

- **ra-core**: No changes needed; XML functions are represented as
  `Expr::Function` nodes already
- **ra-engine/egraph.rs**: The existing `func` node in RelLang
  handles XML function calls
- **ra-engine/rewrite.rs**: `all_rules_unsorted()` extends with
  `xml_optimizer::xml_optimization_rules()`
- **ra-dialect**: Platform-specific XML syntax differences are
  handled in dialect translation

### Error Handling

All XPath parsing is best-effort. Malformed XPath strings are left
as opaque function calls with default costs. The optimizer never
rejects a query due to XML parsing failure.

### Performance Considerations

XPath parsing adds overhead only for queries containing XML
functions. The parser is invoked during e-graph construction,
not during every rewrite iteration. Expected overhead: <1ms
per XPath expression.

## Drawbacks

- XPath parsing adds code complexity (~700 lines)
- XPath is a complex language; the parser handles common patterns,
  not the full XPath 3.1 specification
- XML index metadata may not be available in all catalog
  implementations
- Benefits are limited to workloads that use XML features

## Rationale and alternatives

### Why This Design?

Berkeley DB XML demonstrated that XPath optimization produces
substantial speedups (10-100x) when XML indexes can be exploited.
The two-phase approach (structural navigation + value filter)
maps naturally to relational index scan + filter patterns that
Ra already optimizes.

### Alternative Approaches

**Full XPath engine**: Embed a complete XPath evaluator to compute
exact results. Rejected because Ra is an optimizer, not an executor;
it should rewrite plans, not evaluate XPath.

**String pattern matching**: Use regex to detect XPath patterns
without parsing. Rejected because this is fragile and cannot handle
nested predicates or complex path expressions.

### Impact of Not Doing This

XML-heavy workloads will continue to treat XPath/XQuery as black
boxes, missing 10-100x optimization opportunities when XML indexes
are available.

## Prior art

### Berkeley DB XML (dbxml)

The primary inspiration. Key design elements extracted:

- **Two-phase optimization**: AST rewriting (ASTReplaceOptimizer)
  followed by query plan optimization (QueryPlanOptimizer)
- **Index-aware planning**: QueryPlanGenerator checks
  `isSuitableForIndex()` before choosing structural joins vs
  sequential steps
- **Structural joins**: Parent-child and ancestor-descendant
  relationships resolved via index lookups rather than tree traversal
- **Predicate reversal**: Non-numeric predicates are "reversed"
  into index-friendly query plans
- **Statistics**: KeyStatistics tracks numIndexedKeys, numUniqueKeys,
  and sumKeyValueSize for cost estimation
- **Partial evaluation**: XQilla's PartialEvaluator performs constant
  folding, arithmetic simplification, boolean short-circuiting,
  function inlining, and dead code elimination at compile time

### PostgreSQL

- `xpath()` returns `xml[]`, parsed on every call
- No XML-specific indexes (relies on functional B-tree indexes
  on xpath results, or expression indexes)
- `xmlexists()` returns boolean, slightly cheaper than full xpath

### Oracle

- XMLType with structured/unstructured storage
- XMLIndex provides path-decomposed relational storage
- XQuery rewrite into relational operations via `XMLTable`

### SQL Server

- Native `xml` data type with typed/untyped variants
- Primary XML index (B+ tree of shredded XML)
- Secondary indexes: PATH, VALUE, PROPERTY
- `.exist()` method leverages XML indexes directly

## Unresolved questions

- Should Ra attempt to parse the full XPath 3.1 grammar, or
  limit to the XPath 1.0 subset that covers 90%+ of real usage?
  (Recommendation: XPath 1.0 subset)
- How should XML index metadata be represented in ra-metadata?
  (Deferred to catalog implementation)
- Should the module handle JSON path expressions too, given
  the structural similarity? (Deferred to RFC 0084)

## Future possibilities

- **JSON path optimization**: The same structural approach applies
  to SQL/JSON path expressions (`jsonb_path_query`)
- **XML schema-aware optimization**: When XML schemas are known,
  tighter cardinality estimates become possible
- **Cross-database XML migration**: Rewrite Oracle XMLType queries
  to PostgreSQL xml equivalents
- **XML shredding recommendations**: Suggest converting XML columns
  to relational tables when access patterns are regular
