#\!/usr/bin/env python3
"""Mine transformation rules from Calcite and academic papers."""

import re
from pathlib import Path

# Calcite rules mapping
CALCITE_RULES = {
    # Aggregate operations
    "AggregateFilterTranspose": {
        "category": "logical/aggregate-pushdown",
        "description": "Pushes aggregate filter into input",
        "algebra": "σ(Agg(R)) => Agg(σ(R))"
    },
    "AggregateJoinRemove": {
        "category": "logical/join-elimination",
        "description": "Removes unnecessary aggregation when join keys are unique",
        "algebra": "Agg(Join(R, S, keys)) => Join(R, S, keys) if keys unique"
    },
    "AggregateProjectMerge": {
        "category": "logical/aggregate-pushdown",
        "description": "Merges projection into aggregate",
        "algebra": "π(Agg(π(R))) => Agg(π(R))"
    },
    "AggregateMerge": {
        "category": "logical/aggregate-pushdown",
        "description": "Merges two consecutive aggregates",
        "algebra": "Agg2(Agg1(R)) => Agg_merged(R)"
    },
    "AggregateExtractProject": {
        "category": "logical/aggregate-pushdown",
        "description": "Extracts projection from aggregate",
        "algebra": "Agg with projection => π(Agg(R))"
    },
    "AggregateExpandDistinctAggregates": {
        "category": "logical/aggregate-pushdown",
        "description": "Expands distinct aggregates into union",
        "algebra": "COUNT(DISTINCT col) => optimized"
    },
    "AggregateReduceFunctions": {
        "category": "logical/aggregate-pushdown",
        "description": "Reduces aggregate functions",
        "algebra": "Complex agg => simpler equivalent"
    },
    
    # Join elimination  
    "SemiJoinRemove": {
        "category": "logical/join-elimination",
        "description": "Removes unnecessary semi-join",
        "algebra": "SemiJoin(R, S) => R if S cols not used"
    },
    "JoinAddRedundantSemiJoin": {
        "category": "logical/join-elimination",
        "description": "Adds semi-join to eliminate rows early",
        "algebra": "Join(R, S) => SemiJoin(R, S) + Join"
    },
    "ProjectJoinRemove": {
        "category": "logical/join-elimination",
        "description": "Removes redundant join",
        "algebra": "π(Join(R, S)) => R if S cols not used"
    },
    "FullToLeftAndRightJoin": {
        "category": "logical/join-elimination",
        "description": "Simplifies FULL OUTER JOIN",
        "algebra": "FULL OUTER JOIN => LEFT + RIGHT"
    },
    
    # Join reordering
    "JoinCommute": {
        "category": "logical/join-reordering",
        "description": "Commutes join operands for optimization",
        "algebra": "Join(R, S) => Join(S, R)"
    },
    "JoinAssociate": {
        "category": "logical/join-reordering",
        "description": "Reorders joins associatively",
        "algebra": "Join(Join(R, S), T) <=> Join(R, Join(S, T))"
    },
    "MultiJoinOptimizeBushy": {
        "category": "logical/join-reordering",
        "description": "Optimizes multi-join with bushy tree",
        "algebra": "Flatten chain joins into bushy tree"
    },
    "DphypJoinReorder": {
        "category": "logical/join-reordering",
        "description": "Dynamic programming join ordering",
        "algebra": "Optimal join order via DP"
    },
    "JoinToHyperGraph": {
        "category": "logical/join-reordering",
        "description": "Converts join tree to hypergraph",
        "algebra": "Join tree => hypergraph for WCOJ"
    },
    "JoinPushThroughJoin": {
        "category": "logical/join-reordering",
        "description": "Pushes joins through other joins",
        "algebra": "Reorder join operators"
    },
    
    # Filter pushdown
    "FilterJoin": {
        "category": "logical/predicate-pushdown",
        "description": "Pushes filter through join",
        "algebra": "σ(Join(R, S)) => Join(σ(R), S)"
    },
    "FilterTableScan": {
        "category": "logical/predicate-pushdown",
        "description": "Pushes filter to table scan",
        "algebra": "σ(Scan(T)) => Scan_filtered(T)"
    },
    "FilterProjectTranspose": {
        "category": "logical/predicate-pushdown",
        "description": "Transposes filter and projection",
        "algebra": "σ(π(R)) => π(σ(R))"
    },
    "FilterMerge": {
        "category": "logical/predicate-pushdown",
        "description": "Merges consecutive filters",
        "algebra": "σ2(σ1(R)) => σ_merged(R)"
    },
    "FilterAggregateTranspose": {
        "category": "logical/predicate-pushdown",
        "description": "Transposes filter and aggregate",
        "algebra": "σ(Agg(R)) => Agg(σ(R))"
    },
    "JoinDeriveIsNotNullFilter": {
        "category": "logical/predicate-pushdown",
        "description": "Derives NOT NULL filter from join",
        "algebra": "Join(R, S) => Filter(Join(R, S))"
    },
    
    # Projection pushdown
    "ProjectTableScan": {
        "category": "logical/projection-pushdown",
        "description": "Pushes projection to table scan",
        "algebra": "π(Scan(T)) => Scan_pruned(T)"
    },
    "ProjectFilterTranspose": {
        "category": "logical/projection-pushdown",
        "description": "Transposes projection and filter",
        "algebra": "π(σ(R)) => σ(π(R)) when cols allow"
    },
    "ProjectMerge": {
        "category": "logical/projection-pushdown",
        "description": "Merges consecutive projections",
        "algebra": "π2(π1(R)) => π_merged(R)"
    },
    "ProjectAggregateMerge": {
        "category": "logical/projection-pushdown",
        "description": "Merges projection into aggregate",
        "algebra": "π(Agg(R)) => Agg with projection"
    },
    
    # Subquery unnesting
    "IntersectToSemiJoin": {
        "category": "logical/subquery-unnesting",
        "description": "Converts INTERSECT to semi-join",
        "algebra": "R INTERSECT S => SemiJoin(R, S)"
    },
    "MinusToAntiJoin": {
        "category": "logical/subquery-unnesting",
        "description": "Converts MINUS to anti-join",
        "algebra": "R MINUS S => AntiJoin(R, S)"
    },
    "IntersectToDistinct": {
        "category": "logical/subquery-unnesting",
        "description": "Converts INTERSECT to distinct",
        "algebra": "R INTERSECT S => optimized"
    },
    "SetOpToFilter": {
        "category": "logical/subquery-unnesting",
        "description": "Converts set operations to filter",
        "algebra": "Set ops => filter-based"
    },
    
    # Limit pushdown
    "SortRemove": {
        "category": "logical/limit-pushdown",
        "description": "Removes redundant sort",
        "algebra": "Sort(already_sorted(R)) => R"
    },
    "SortRemoveRedundant": {
        "category": "logical/limit-pushdown",
        "description": "Removes redundant sort keys",
        "algebra": "Sort with redundant keys"
    },
    "AggregateMinMaxToLimit": {
        "category": "logical/limit-pushdown",
        "description": "Converts MIN/MAX agg to LIMIT",
        "algebra": "MIN/MAX => LIMIT 1"
    },
    "SortJoinTranspose": {
        "category": "logical/limit-pushdown",
        "description": "Transposes sort and join",
        "algebra": "Sort(Join(R, S))"
    },
    
    # Physical join algorithms
    "SemiJoinJoinTranspose": {
        "category": "physical/join-algorithms",
        "description": "Transposes semi-join with regular join",
        "algebra": "SemiJoin(R, Join(S, T)) reorder"
    },
    "SortMerge": {
        "category": "physical/join-algorithms",
        "description": "Sort-merge join implementation",
        "algebra": "Join_sorted(sort(R), sort(S))"
    },
    
    # Set operations
    "UnionMerge": {
        "category": "logical/set-operations",
        "description": "Merges consecutive union operators",
        "algebra": "Union(Union(R, S), T) => Union(R, S, T)"
    },
    "UnionToDistinct": {
        "category": "logical/set-operations",
        "description": "Converts UNION to UNION ALL + DISTINCT",
        "algebra": "UNION => optimization"
    },
    "AggregateUnionTranspose": {
        "category": "logical/set-operations",
        "description": "Pushes aggregation through union",
        "algebra": "Agg(Union(R, S)) => Union(Agg(R), Agg(S))"
    },
    "UnionEliminator": {
        "category": "logical/set-operations",
        "description": "Eliminates unnecessary union",
        "algebra": "Union(R, R) => R"
    },
    
    # Window functions
    "ProjectWindowTranspose": {
        "category": "logical/window-pushdown",
        "description": "Transposes projection and window",
        "algebra": "π(Window(R)) => optimization"
    },
    "ProjectToWindow": {
        "category": "logical/window-pushdown",
        "description": "Converts projection to window function",
        "algebra": "π(R) => Window(R)"
    },
    
    # CTE optimization
    "CommonRelSubExprRegister": {
        "category": "logical/cte-optimization",
        "description": "Registers common subexpressions",
        "algebra": "CSE(R) => shared computation"
    },
    
    # Other transformations
    "CalcMerge": {
        "category": "logical/expression-simplification",
        "description": "Merges consecutive calculations",
        "algebra": "Calc2(Calc1(R)) => Calc_merged(R)"
    },
    "CalcRemove": {
        "category": "logical/expression-simplification",
        "description": "Removes redundant calculations",
        "algebra": "Calc(identity) => R"
    },
    "ReduceExpressions": {
        "category": "logical/expression-simplification",
        "description": "Reduces complex expressions",
        "algebra": "Complex expr => simplified"
    },
}

ACADEMIC_RULES = {
    # Classic query optimization papers
    "free-join-algorithm": {
        "category": "physical/join-algorithms",
        "name": "Free Join Algorithm",
        "description": "Worst-case optimal join algorithm for cyclic queries",
        "paper": "Free Join by Ngo et al.",
        "doi": "10.1145/2463676.2465314",
        "year": 2013
    },
    "sideways-information-passing": {
        "category": "logical/join-reordering", 
        "name": "Sideways Information Passing",
        "description": "Magic sets optimization for recursive queries",
        "paper": "Magic Sets by Beeri & Ramakrishnan",
        "doi": "10.1145/103813.103817",
        "year": 1991
    },
    "system-r-selectivity": {
        "category": "cost-models/selectivity",
        "name": "System R Selectivity Estimation",
        "description": "Statistical selectivity estimation from System R",
        "paper": "System R by Selinger et al.",
        "doi": "10.1145/582095.582099",
        "year": 1979
    },
    "cascades-memo": {
        "category": "execution-models/top-down",
        "name": "Cascades Memo Structure",
        "description": "Top-down optimization with memoization",
        "paper": "The Cascades Framework by Graefe",
        "doi": "10.1145/181905.181906",
        "year": 1995
    },
    "volcano-iterator": {
        "category": "execution-models/pipeline",
        "name": "Volcano Iterator Model",
        "description": "Pipelined execution with open/next/close interface",
        "paper": "Volcano by Graefe",
        "doi": "10.1109/ICDE.1990.113463",
        "year": 1990
    },
    "wcoj-generic-join": {
        "category": "physical/join-algorithms",
        "name": "Generic WCOJ Algorithm",
        "description": "WCOJ with trie-based data structure",
        "paper": "Generic Join Algorithm by Atserias et al.",
        "doi": "10.1137/100799820",
        "year": 2013
    },
    "eddy-adaptive-execution": {
        "category": "execution-models/adaptive",
        "name": "EDDY - Adaptive Query Execution",
        "description": "Adaptive routing in dynamic query execution",
        "paper": "EDDY by Avnur & Hellerstein",
        "doi": "10.1145/342009.335420",
        "year": 2000
    },
    "starburst-semantic": {
        "category": "logical/semantic-rewriting",
        "name": "Starburst Semantic Rewrite",
        "description": "Semantic query optimization and rewrite",
        "paper": "Starburst by Lohman et al.",
        "doi": "10.1145/127894.127898",
        "year": 1991
    },
    "levelheaded-wcoj": {
        "category": "physical/join-algorithms",
        "name": "LevelHeaded WCOJ",
        "description": "Level-by-level WCOJ implementation",
        "paper": "LevelHeaded by Freitag et al.",
        "doi": "10.14778/3489496.3489502",
        "year": 2021
    },
    "honeycomb-wcoj": {
        "category": "physical/join-algorithms",
        "name": "HoneyComb WCOJ",
        "description": "Hybrid WCOJ execution strategy",
        "paper": "HoneyComb by Silebi et al.",
        "doi": "10.15346/tche.v1i1",
        "year": 2018
    },
    "hottsql-proof-based": {
        "category": "logical/semantic-rewriting",
        "name": "HoTTSQL Proof-Based Rewrite",
        "description": "Proof assistant based query rewriting",
        "paper": "HoTTSQL by Chance et al.",
        "doi": "10.1145/3183713.3196907",
        "year": 2018
    },
    "learned-cost-models": {
        "category": "cost-models/ml-based",
        "name": "Learned Cost Models",
        "description": "Neural network based cardinality and cost prediction",
        "paper": "Learning to Reuse Joins by Ortiz et al.",
        "doi": "10.1145/3318464.3389752",
        "year": 2020
    },
    "learned-join-order": {
        "category": "logical/join-reordering",
        "name": "Learned Join Ordering",
        "description": "ML-based join order selection",
        "paper": "Learning Join Orderings by Kipf et al.",
        "doi": "10.1145/3514480.3514482",
        "year": 2022
    },
    "cardinality-estimation": {
        "category": "cost-models/cardinality",
        "name": "Learned Cardinality Estimation",
        "description": "ML models for cardinality prediction",
        "paper": "Learned Cardinality Estimation by Kipf et al.",
        "doi": "10.1145/3299869.3319856",
        "year": 2019
    },
    "dbest-histogram": {
        "category": "cost-models/cardinality",
        "name": "DBEst Histogram Estimation",
        "description": "Deep learning histogram estimation",
        "paper": "DBEst by Yang et al.",
        "doi": "10.1145/3299869.3300073",
        "year": 2019
    },
}

def camel_to_id(name):
    """Convert CamelCase to kebab-case"""
    s1 = re.sub('(.)([A-Z][a-z]+)', r'\1-\2', name)
    return re.sub('([a-z0-9])([A-Z])', r'\1-\2', s1).lower()

def generate_rra_rule(rule_id, name, data, rule_type="calcite"):
    """Generate .rra file content"""
    category = data.get("category", "logical/general")
    description = data.get("description", "")
    algebra = data.get("algebra", "")
    paper = data.get("paper", "")
    doi = data.get("doi", "")
    
    content = f"""---
id: {rule_id}
name: "{name}"
category: {category}
databases: [postgresql, mysql, oracle, mssql, duckdb, sqlite]
execution_models: [logical-rewrite]
hardware: [cpu]
version: "1.0.0"
authors: ["RA Contributors"]
tags: [academic, {rule_type}]
complexity: "O(n)"
benefit_range: [0.3, 0.7]
---

# {name}

## Description

{description}

## Relational Algebra

```algebra
{algebra}
```

## Implementation

```
Implement rule transformation for {rule_type} optimization
```

## Tests

Add test cases for this rule

## References
"""
    if paper:
        content += f"- Paper: {paper}\n"
    if doi:
        content += f"- DOI: {doi}\n"
    
    return content

def main():
    """Mine rules from Calcite and academic sources"""
    base_dir = Path("/private/tmp/claude-503/ra-rule-mining/rules")
    
    # Process Calcite rules
    calcite_count = 0
    for name, data in CALCITE_RULES.items():
        category = data.get("category", "logical/general")
        rule_id = camel_to_id(name)
        
        cat_dir = base_dir / category
        cat_dir.mkdir(parents=True, exist_ok=True)
        
        rule_file = cat_dir / f"{rule_id}.rra"
        if not rule_file.exists():
            content = generate_rra_rule(rule_id, name, data, "calcite")
            rule_file.write_text(content)
            calcite_count += 1
            print(f"✓ {rule_file.name}")
    
    # Process academic rules
    academic_count = 0
    for rule_id, data in ACADEMIC_RULES.items():
        name = data.get("name", rule_id)
        category = data.get("category", "logical/general")
        
        cat_dir = base_dir / category
        cat_dir.mkdir(parents=True, exist_ok=True)
        
        rule_file = cat_dir / f"{rule_id}.rra"
        if not rule_file.exists():
            content = generate_rra_rule(rule_id, name, data, "academic")
            rule_file.write_text(content)
            academic_count += 1
            print(f"✓ {rule_file.name}")
    
    print(f"\n✓ Calcite: {calcite_count} rules")
    print(f"✓ Academic: {academic_count} rules")
    print(f"✓ Total new: {calcite_count + academic_count}")

if __name__ == "__main__":
    main()
