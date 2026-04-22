//! Educational explanations for optimization rules.
//!
//! Provides detailed, human-readable descriptions of what each rule does,
//! including before/after examples and real-world impact.

/// Get a detailed explanation for a rule.
pub fn explain_rule(rule_name: &str) -> RuleExplanation {
    match rule_name {
        // Runtime filters
        "runtime-filter-hash-to-semi" => RuleExplanation {
            summary: "Adds a bloom filter to pre-filter the probe side before hashing",
            impact: "Can eliminate 90%+ of rows before the expensive hash join operation. \
                     Creates a compact bitmap from the build side that quickly tests whether \
                     probe-side rows could possibly match.",
            before_example: Some("HashJoin(build_table, probe_table)"),
            after_example: Some("HashJoin(build_table, SemiJoin(probe_table, build_table))"),
            why_no_cost_change: Some("Cost model doesn't capture bloom filter benefits"),
        },

        "runtime-filter-through-filter" => RuleExplanation {
            summary: "Pushes runtime bloom filters through other filter operations",
            impact: "Allows bloom filter to eliminate rows even earlier in the pipeline, \
                     before other predicates are evaluated.",
            before_example: None,
            after_example: None,
            why_no_cost_change: Some("Benefit depends on runtime selectivity"),
        },

        // Null-aware join optimizations
        "filter-null-join-key-left" | "filter-null-join-key-right" => RuleExplanation {
            summary: "Explicitly filters NULL values from join keys before joining",
            impact: "Since NULL != NULL in SQL, these rows never match anyway. Benefits:\n\
                     • Smaller hash tables (NULLs excluded)\n\
                     • Better cardinality estimates\n\
                     • Enables null-aware join algorithms\n\
                     • Avoids wasted comparisons",
            before_example: Some("orders JOIN customers ON orders.customer_id = customers.id"),
            after_example: Some(
                "(orders WHERE customer_id IS NOT NULL) JOIN (customers WHERE id IS NOT NULL)",
            ),
            why_no_cost_change: Some("Cost model sees added Filter operators as overhead"),
        },

        // Semi-join conversions
        "filter-into-semi-join-condition" => RuleExplanation {
            summary: "Pushes filter predicates into semi-join (EXISTS) conditions",
            impact: "Reduces the amount of work done in the subquery by filtering earlier. \
                     The subquery can stop as soon as it finds any matching row.",
            before_example: Some(
                "SELECT * FROM (SELECT * FROM orders WHERE EXISTS (...)) WHERE condition",
            ),
            after_example: Some("SELECT * FROM orders WHERE EXISTS (... AND condition)"),
            why_no_cost_change: Some("Plan structure appears unchanged in the extracted tree"),
        },

        "filter-semi-join-merge" => RuleExplanation {
            summary: "Merges adjacent filter and semi-join operations",
            impact: "Combines multiple existence checks into a single operation, \
                     reducing overhead and enabling better optimization.",
            before_example: None,
            after_example: None,
            why_no_cost_change: Some("Logical optimization - execution benefit not modeled"),
        },

        // Join transformations
        "join-commutativity" => RuleExplanation {
            summary: "Swaps the left and right sides of a join",
            impact: "Essential for exploring different join orders. The smaller table should \
                     typically be on the build (right) side of a hash join. This rule enables \
                     the optimizer to find that arrangement.",
            before_example: Some("large_table JOIN small_table"),
            after_example: Some("small_table JOIN large_table"),
            why_no_cost_change: Some("By itself does nothing, but enables other rules to fire"),
        },

        "join-associativity-left" | "join-associativity-right" => RuleExplanation {
            summary: "Regroups multi-way joins to enable different join orders",
            impact: "Transforms (A JOIN B) JOIN C into A JOIN (B JOIN C). Critical for \
                     optimizing multi-way joins - enables the optimizer to find the best \
                     join order among all possibilities.",
            before_example: Some("(orders JOIN customers) JOIN products"),
            after_example: Some("orders JOIN (customers JOIN products)"),
            why_no_cost_change: Some(
                "Enables other rules; cost benefit comes from subsequent optimizations",
            ),
        },

        // Filter optimizations
        "filter-through-join-left" | "filter-through-join-right" => RuleExplanation {
            summary: "Pushes filter predicates through join operations (filter pushdown)",
            impact: "Applies filters as early as possible, reducing the size of intermediate \
                     results. If a filter can be evaluated on one side of a join, do it \
                     before joining - this is one of the most powerful optimizations.",
            before_example: Some(
                "SELECT * FROM (orders JOIN customers) WHERE customers.country = 'USA'",
            ),
            after_example: Some(
                "SELECT * FROM orders JOIN (SELECT * FROM customers WHERE country = 'USA')",
            ),
            why_no_cost_change: Some(
                "May reorganize join tree without changing extracted plan yet",
            ),
        },

        "filter-into-join-condition" => RuleExplanation {
            summary: "Moves filter predicates directly into the join condition",
            impact: "Allows the join algorithm to evaluate the filter during joining rather \
                     than in a separate pass. Often enables more efficient join algorithms \
                     and better cardinality estimates.",
            before_example: Some(
                "SELECT * FROM orders JOIN customers WHERE orders.status = 'active'",
            ),
            after_example: Some(
                "SELECT * FROM orders JOIN customers ON (... AND orders.status = 'active')",
            ),
            why_no_cost_change: Some("Eliminates Filter operator but adds complexity to join"),
        },

        "filter-merge" => RuleExplanation {
            summary: "Combines multiple consecutive filters into a single filter",
            impact: "Reduces operator overhead and enables better predicate optimization. \
                     Multiple ANDed predicates can be reordered and evaluated more efficiently \
                     as a single filter.",
            before_example: Some("SELECT * FROM orders WHERE status = 'active' AND amount > 100"),
            after_example: Some(
                "SELECT * FROM orders WHERE status = 'active' AND amount > 100  -- single filter",
            ),
            why_no_cost_change: Some("Combines operators without changing selectivity"),
        },

        "filter-split-and" => RuleExplanation {
            summary: "Splits a filter with AND into multiple separate filters",
            impact: "Counter-intuitively useful! Enables individual predicates to be pushed \
                     down through joins independently. A split filter can move further down \
                     the plan tree than a combined one.",
            before_example: Some("Filter(a AND b AND c)"),
            after_example: Some("Filter(a) -> Filter(b) -> Filter(c)"),
            why_no_cost_change: Some("Enables other rules; benefit comes from subsequent pushdown"),
        },

        // Boolean algebra
        "and-commutative" | "or-commutative" => RuleExplanation {
            summary: "Reorders terms in AND/OR expressions",
            impact: "Puts predicates in the right order for other rules to match. For example, \
                     cheap predicates should be evaluated first (short-circuit evaluation). \
                     Also enables extract-equijoin rules to find join conditions.",
            before_example: Some("WHERE (expensive_func(x) AND y = 5)"),
            after_example: Some(
                "WHERE (y = 5 AND expensive_func(x))  -- evaluate cheap predicate first",
            ),
            why_no_cost_change: Some(
                "Logical reordering - enables pattern matching for other rules",
            ),
        },

        "eq-commutative" => RuleExplanation {
            summary: "Flips equality comparisons (a = b becomes b = a)",
            impact: "Essential for recognizing join conditions and enabling index usage. \
                     Allows the optimizer to match patterns regardless of operand order.",
            before_example: Some("WHERE 42 = customer_id"),
            after_example: Some("WHERE customer_id = 42  -- standard form for index lookup"),
            why_no_cost_change: Some("Logical equivalence - enables other optimizations"),
        },

        // Join condition extraction
        "extract-equijoin-from-and-left" | "extract-equijoin-from-and-right" => RuleExplanation {
            summary: "Pulls equality conditions out of complex predicates to create equijoins",
            impact: "Equijoins (a.x = b.x) can use hash joins and merge joins, which are much \
                     faster than nested loops. This rule finds equijoin conditions hidden in \
                     complex predicates and makes them explicit.",
            before_example: Some(
                "JOIN ON (a.status = 'active' AND a.id = b.id AND b.country = 'USA')",
            ),
            after_example: Some(
                "JOIN ON (a.id = b.id) WHERE a.status = 'active' AND b.country = 'USA'",
            ),
            why_no_cost_change: Some(
                "Restructures predicates to enable hash/merge join algorithms",
            ),
        },

        // Covering indexes
        name if name.contains("covering-index") => RuleExplanation {
            summary: "Uses an index that contains all required columns (index-only scan)",
            impact: "Avoids fetching rows from the main table entirely. The index contains \
                     all needed data, eliminating expensive random I/O to heap pages. Can be \
                     100-1000x faster than regular index scans.",
            before_example: Some("IndexScan(users, idx_email) -> Fetch heap rows"),
            after_example: Some("IndexOnlyScan(users, idx_email_name)  -- no heap access needed"),
            why_no_cost_change: None,
        },

        // Bitmap scans
        name if name.contains("bitmap") => RuleExplanation {
            summary: "Uses bitmap index scan for better I/O patterns",
            impact: "Builds a bitmap of matching row IDs from the index, then scans heap \
                     in physical order. Much more efficient than random access for medium \
                     selectivity (5-30%). Can combine multiple indexes with AND/OR.",
            before_example: Some("Multiple random heap fetches"),
            after_example: Some("Sequential heap scan using bitmap of matching rows"),
            why_no_cost_change: None,
        },

        // Default explanation
        _ => RuleExplanation {
            summary: "Applies a transformation to improve query execution",
            impact: "This rule explores alternative execution strategies that may be \
                     more efficient depending on data statistics and hardware capabilities.",
            before_example: None,
            after_example: None,
            why_no_cost_change: Some(
                "Adds alternative representations to the optimization search space",
            ),
        },
    }
}

/// Detailed explanation of a rule's purpose and impact.
pub struct RuleExplanation {
    /// One-sentence summary of what the rule does
    pub summary: &'static str,
    /// Detailed explanation of the real-world impact
    pub impact: &'static str,
    /// Optional before example (can be SQL or plan pseudo-code)
    pub before_example: Option<&'static str>,
    /// Optional after example
    pub after_example: Option<&'static str>,
    /// Explanation for why cost model shows no change
    pub why_no_cost_change: Option<&'static str>,
}

/// Group related rules together for cleaner output
pub fn should_group_with_previous(current_rule: &str, previous_rule: &str) -> bool {
    // Group commutative rules together
    if current_rule.contains("commutative") && previous_rule.contains("commutative") {
        return true;
    }

    // Group null filter rules
    if current_rule.contains("filter-null") && previous_rule.contains("filter-null") {
        return true;
    }

    // Group runtime filter rules
    if current_rule.contains("runtime-filter") && previous_rule.contains("runtime-filter") {
        return true;
    }

    // Group semi-join rules
    if current_rule.contains("semi") && previous_rule.contains("semi") {
        return true;
    }

    false
}
