use egg::{define_language, Id};

define_language! {
    /// S-expression language for relational algebra in the e-graph.
    ///
    /// Each variant maps to a relational or scalar operator. Children
    /// are represented as [`Id`] references into the e-graph.
    pub enum RelLang {
        // -- Relational operators --
        "scan" = Scan([Id; 1]),
        "scan-alias" = ScanAlias([Id; 2]),
        "filter" = Filter([Id; 2]),
        "project" = Project([Id; 2]),
        "join" = Join([Id; 4]),
        // Physical join variants (RFC 0089 / RFC 0090 Phase 3). Same children as
        // `join` ([type, cond, left, right]); produced by cost-driven physical
        // lowering rules and chosen by the cost extractor. `from_rec` maps them
        // back to the logical `RelExpr::Join`; the chosen method is carried to
        // plan-builder via the PhysicalChoices sidecar.
        "hash-join" = HashJoinOp([Id; 4]),
        "merge-join" = MergeJoinOp([Id; 4]),
        "nest-loop" = NestLoopOp([Id; 4]),
        "index-nest-loop" = IndexNestLoopOp([Id; 4]),
        "aggregate" = Aggregate([Id; 3]),
        "sort" = Sort([Id; 2]),
        "incremental-sort" = IncrementalSort([Id; 3]),
        "limit" = Limit([Id; 3]),
        "union" = Union([Id; 3]),
        "intersect" = Intersect([Id; 3]),
        "except" = Except([Id; 3]),
        "recursive-cte" = RecursiveCTE([Id; 4]),
        "cte" = CTE([Id; 3]),
        "window" = Window([Id; 2]),
        "distinct-rel" = DistinctRel([Id; 1]),
        "values" = Values(Box<[Id]>),
        "values-row" = ValuesRow(Box<[Id]>),

        // -- Metadata shortcut operators --
        "metadata-lookup" = MetadataLookup([Id; 2]),
        "row-count" = RowCount,

        // -- MIN/MAX index scan optimization --
        // Children: [table, column]
        "index-scan" = IndexScan([Id; 2]),

        // -- Index-only scan (covering index) --
        // Children: [table, index_name, projected_cols, predicate]
        "index-only-scan" = IndexOnlyScan([Id; 4]),

        // -- Materialized view scan --
        // Children: [view_name, alias, group_by_list, agg_list]
        "mv-scan" = MvScan([Id; 4]),

        // -- Bitmap index operators --
        "bitmap-index-scan" = BitmapIndexScan([Id; 3]),
        "bitmap-and" = BitmapAnd(Box<[Id]>),
        "bitmap-or" = BitmapOr(Box<[Id]>),
        "bitmap-heap-scan" = BitmapHeapScan([Id; 3]),

        // -- Window function expression --
        "window-expr" = WindowExprNode([Id; 6]),
        "window-fn" = WindowFn([Id; 1]),
        "window-frame" = WindowFrameNode([Id; 3]),
        "frame-rows" = FrameRows,
        "frame-range" = FrameRange,
        "frame-groups" = FrameGroups,
        "frame-unbounded-preceding" = FrameUnboundedPreceding,
        "frame-preceding" = FramePreceding([Id; 1]),
        "frame-current-row" = FrameCurrentRow,
        "frame-following" = FrameFollowing([Id; 1]),
        "frame-unbounded-following" = FrameUnboundedFollowing,

        // -- Join types --
        "inner" = Inner,
        "left-outer" = LeftOuter,
        "right-outer" = RightOuter,
        "full-outer" = FullOuter,
        "cross" = Cross,
        "semi" = Semi,
        "anti" = Anti,

        // -- Boolean flags --
        "true" = True,
        "false" = False,

        // -- Scalar expressions --
        "col" = Col([Id; 1]),
        "qcol" = QCol([Id; 2]),
        "const-null" = ConstNull,
        "const-bool" = ConstBool([Id; 1]),
        "const-int" = ConstInt([Id; 1]),
        "const-float" = ConstFloat([Id; 1]),
        "const-str" = ConstStr([Id; 1]),

        // -- Binary operators --
        "add" = Add([Id; 2]),
        "sub" = Sub([Id; 2]),
        "mul" = Mul([Id; 2]),
        "div" = Div([Id; 2]),
        "mod" = Mod([Id; 2]),
        "eq" = Eq([Id; 2]),
        "ne" = Ne([Id; 2]),
        "lt" = Lt([Id; 2]),
        "le" = Le([Id; 2]),
        "gt" = Gt([Id; 2]),
        "ge" = Ge([Id; 2]),
        "and" = And([Id; 2]),
        "or" = Or([Id; 2]),
        "concat" = Concat([Id; 2]),
        "json-access" = JsonAccess([Id; 2]),

        // -- Unary operators --
        "not" = Not([Id; 1]),
        "is-null" = IsNull([Id; 1]),
        "is-not-null" = IsNotNull([Id; 1]),
        "neg" = Neg([Id; 1]),

        // -- Function call --
        "func" = Func(Box<[Id]>),

        // -- Aggregate functions --
        "count" = Count([Id; 1]),
        "sum" = Sum([Id; 1]),
        "avg" = Avg([Id; 1]),
        "min" = Min([Id; 1]),
        "max" = Max([Id; 1]),

        // -- Lists --
        "list" = List(Box<[Id]>),
        "nil" = Nil,

        // -- Projection column --
        "proj-col" = ProjCol([Id; 1]),
        "proj-alias" = ProjAlias([Id; 2]),

        // -- Sort keys --
        "sort-key" = SortKey([Id; 3]),
        "asc" = Asc,
        "desc" = Desc,
        "nulls-first" = NullsFirst,
        "nulls-last" = NullsLast,

        // -- Aggregate expression --
        "agg-expr" = AggExpr([Id; 3]),
        "distinct" = Distinct,
        "all" = All,

        // -- Vector search operators (RFC 0064) --
        // Children: [metric, column, target_vector]
        "vector-distance" = VectorDistance([Id; 3]),
        // Children: [table, column, target_vector, k]
        "vector-knn" = VectorKNN([Id; 4]),
        // Children: [table, column, target_vector, threshold, metric]
        "vector-range-scan" = VectorRangeScan([Id; 5]),

        // -- Full-text search operators (RFC 0073) --
        // Children: [vendor, columns, query, mode]
        "fts-match" = FtsMatch([Id; 4]),
        // Children: [column, query, algorithm]
        "fts-rank" = FtsRank([Id; 3]),
        // Children: [table, index_type, predicate]
        "fts-index-scan" = FtsIndexScan([Id; 3]),
        // Children: [table, index_type, query, k, algorithm]
        "fts-ranked-scan" = FtsRankedScan([Id; 5]),
        // Children: [table, pred1, pred2]
        "fts-skip-list-and" = FtsSkipListAnd([Id; 3]),

        // -- Hybrid search operators (RFC 0073) --
        // Children: [fts_score, vector_score, alpha, beta, method]
        "hybrid-score" = HybridScore([Id; 5]),
        // Children: [table, fts_args, vector_args, strategy, k, limit]
        "hybrid-scan" = HybridScan([Id; 6]),

        // -- Type casting operator --
        // Children: [expr, target_type]
        "cast" = Cast([Id; 2]),

        // -- Leaf symbols (table names, column names, strings) --
        Symbol(egg::Symbol),
    }
}
