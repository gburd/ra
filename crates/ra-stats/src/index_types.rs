//! Extended index type modeling for query optimizer cost estimation.
//!
//! Models the full range of index types found in modern database systems,
//! including B-tree variants, specialized indexes (full-text, spatial,
//! columnstore), and PostgreSQL-specific types (GIN, GiST, BRIN).
//!
//! Each index type carries cost factors that the optimizer uses to compare
//! access paths and choose the cheapest plan.

use serde::{Deserialize, Serialize};

use crate::types::IndexStats;
use ra_core::search_types::{DistanceMetric, FullTextParser};

/// Discriminated union of index access methods.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IndexType {
    /// B-tree where leaf order matches physical row order.
    Clustered {
        /// Indexed columns.
        columns: Vec<String>,
    },
    /// Secondary B-tree, optionally with INCLUDE columns.
    NonClustered {
        /// Key columns used for lookups.
        columns: Vec<String>,
        /// Additional payload columns stored in the leaf pages.
        included_columns: Vec<String>,
    },
    /// Multi-column B-tree with explicit key ordering.
    Composite {
        /// Indexed columns.
        columns: Vec<String>,
        /// Positional order of columns in the key.
        column_order: Vec<usize>,
    },
    /// Inverted index for natural-language search.
    FullText {
        /// Columns covered by the full-text index.
        columns: Vec<String>,
        /// Natural-language configuration (e.g. "english").
        language: String,
        /// Custom stop-word list, if any.
        stopwords: Option<Vec<String>>,
    },
    /// B-tree with a uniqueness constraint.
    Unique {
        /// Indexed columns.
        columns: Vec<String>,
    },
    /// Partial index with a WHERE-clause filter predicate.
    Filtered {
        /// Key columns.
        columns: Vec<String>,
        /// Textual representation of the filter predicate.
        filter_predicate: String,
    },
    /// R-tree / GiST index for geometric or geographic data.
    Spatial {
        /// Indexed geometry column.
        column: String,
        /// Spatial Reference Identifier.
        srid: Option<i32>,
    },
    /// Column-oriented storage index for analytics workloads.
    Columnstore {
        /// Columns included in the columnstore.
        columns: Vec<String>,
    },
    /// Hash index -- equality lookups only.
    Hash {
        /// Indexed columns.
        columns: Vec<String>,
    },
    /// PostgreSQL Generalized Inverted Index for composite values.
    GIN {
        /// Indexed column (typically array, jsonb, or tsvector).
        column: String,
        /// Operator class (e.g. "jsonb_ops", "gin_trgm_ops").
        opclass: String,
    },
    /// PostgreSQL Generalized Search Tree for spatial/range types.
    GiST {
        /// Indexed column.
        column: String,
        /// Operator class (e.g. "gist_geometry_ops_2d").
        opclass: String,
    },
    /// PostgreSQL Block Range Index for large, naturally ordered tables.
    BRIN {
        /// Indexed column.
        column: String,
        /// Number of pages summarized per range entry.
        pages_per_range: u32,
    },
    /// PostgreSQL RUM index (GIN extension with distance ordering).
    RUM {
        /// Indexed column (typically tsvector).
        column: String,
        /// Operator class (e.g. "rum_tsvector_ops",
        /// "rum_tsvector_addon_ops").
        opclass: String,
        /// Optional addon column for combined ordering
        /// (used with rum_tsvector_addon_ops).
        addon_column: Option<String>,
    },
    /// Bitmap index for low-cardinality columns.
    Bitmap {
        /// Indexed columns.
        columns: Vec<String>,
    },
    /// Expression-based index on a computed value.
    Expression {
        /// SQL expression text (e.g. "lower(email)").
        expression: String,
        /// Underlying index structure.
        backing_type: Box<IndexType>,
    },
    /// IVFFlat vector index (inverted file with flat compression).
    IVFFlat {
        /// Indexed vector column.
        column: String,
        /// Number of inverted lists (clusters).
        lists: u32,
        /// Distance metric for similarity search.
        distance_metric: DistanceMetric,
    },
    /// HNSW vector index (Hierarchical Navigable Small World).
    HNSW {
        /// Indexed vector column.
        column: String,
        /// Number of bi-directional links per node.
        m: u32,
        /// Size of dynamic candidate list during construction.
        ef_construction: u32,
        /// Distance metric for similarity search.
        distance_metric: DistanceMetric,
    },
    /// MySQL full-text index.
    MySQLFullText {
        /// Columns covered by the full-text index.
        columns: Vec<String>,
        /// Parser/tokenizer configuration.
        parser: FullTextParser,
    },
    /// SQL Server full-text index.
    SqlServerFullText {
        /// Columns covered by the full-text index.
        columns: Vec<String>,
        /// Full-text catalog name.
        catalog: String,
        /// Language for word breakers and stemmers.
        language: String,
    },
    /// SQLite FTS5 full-text index.
    SQLiteFTS5 {
        /// Columns covered by the full-text index.
        columns: Vec<String>,
        /// Tokenizer configuration.
        tokenizer: FullTextParser,
    },
    /// SQLite vector extension index.
    SQLiteVec {
        /// Indexed vector column.
        column: String,
        /// Distance metric for similarity search.
        distance_metric: DistanceMetric,
    },
}

impl IndexType {
    /// Columns participating in the index key.
    pub fn key_columns(&self) -> &[String] {
        match self {
            Self::Clustered { columns }
            | Self::NonClustered { columns, .. }
            | Self::Composite { columns, .. }
            | Self::FullText { columns, .. }
            | Self::Unique { columns }
            | Self::Filtered { columns, .. }
            | Self::Columnstore { columns }
            | Self::Hash { columns }
            | Self::Bitmap { columns }
            | Self::MySQLFullText { columns, .. }
            | Self::SqlServerFullText { columns, .. }
            | Self::SQLiteFTS5 { columns, .. } => columns,
            Self::Spatial { column, .. }
            | Self::GIN { column, .. }
            | Self::GiST { column, .. }
            | Self::BRIN { column, .. }
            | Self::RUM { column, .. }
            | Self::IVFFlat { column, .. }
            | Self::HNSW { column, .. }
            | Self::SQLiteVec { column, .. } => {
                std::slice::from_ref(column)
            }
            Self::Expression { backing_type, .. } => {
                backing_type.key_columns()
            }
        }
    }

    /// Whether this index supports range scans natively.
    pub fn supports_range_scan(&self) -> bool {
        matches!(
            self,
            Self::Clustered { .. }
                | Self::NonClustered { .. }
                | Self::Composite { .. }
                | Self::Unique { .. }
                | Self::Filtered { .. }
                | Self::BRIN { .. }
        )
    }

    /// Whether this index supports equality lookups.
    pub fn supports_equality(&self) -> bool {
        !matches!(
            self,
            Self::FullText { .. }
                | Self::Columnstore { .. }
                | Self::MySQLFullText { .. }
                | Self::SqlServerFullText { .. }
                | Self::SQLiteFTS5 { .. }
                | Self::IVFFlat { .. }
                | Self::HNSW { .. }
                | Self::SQLiteVec { .. }
        )
    }

    /// Whether the index is a covering index for the given columns.
    pub fn is_covering(&self, required: &[String]) -> bool {
        match self {
            Self::NonClustered {
                columns,
                included_columns,
            } => required
                .iter()
                .all(|c| columns.contains(c) || included_columns.contains(c)),
            Self::Clustered { .. } => true,
            _ => {
                let keys = self.key_columns();
                required.iter().all(|c| keys.contains(c))
            }
        }
    }

    /// Whether this index supports k-nearest neighbors (exact) search.
    pub fn supports_knn(&self) -> bool {
        matches!(self, Self::SQLiteVec { .. })
    }

    /// Whether this index supports approximate nearest neighbors search.
    pub fn supports_ann(&self) -> bool {
        matches!(self, Self::IVFFlat { .. } | Self::HNSW { .. })
    }

    /// Whether this index supports phrase search.
    pub fn supports_phrase_search(&self) -> bool {
        matches!(
            self,
            Self::FullText { .. }
                | Self::MySQLFullText { .. }
                | Self::SqlServerFullText { .. }
                | Self::SQLiteFTS5 { .. }
                | Self::RUM { .. }
        )
    }

    /// Whether this index supports proximity search.
    pub fn supports_proximity(&self) -> bool {
        matches!(
            self,
            Self::FullText { .. }
                | Self::MySQLFullText { .. }
                | Self::SqlServerFullText { .. }
                | Self::SQLiteFTS5 { .. }
                | Self::RUM { .. }
                | Self::GIN { .. }
        )
    }
}

/// Full metadata for a table index, including statistics and cost factors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexMetadata {
    /// Index name (e.g. "idx_orders_date").
    pub name: String,
    /// The kind of index and its structural parameters.
    pub index_type: IndexType,
    /// Table the index belongs to.
    pub table: String,
    /// Physical statistics gathered from the catalog.
    pub statistics: IndexStats,
    /// Per-operation cost multipliers for the optimizer.
    pub cost_factors: IndexCostFactors,
}

/// Per-operation cost multipliers used by the optimizer to rank access paths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexCostFactors {
    /// Cost of a single-key lookup (random I/O + tree traversal).
    pub lookup_cost: f64,
    /// Cost per page of a range scan (sequential I/O).
    pub range_scan_cost: f64,
    /// Extra cost per tuple when a heap fetch is required.
    pub tuple_fetch_cost: f64,
    /// Whether the index can satisfy the query without a heap fetch.
    pub covering: bool,
}

impl IndexCostFactors {
    /// Default cost factors for a well-tuned B-tree index.
    pub fn btree_default() -> Self {
        Self {
            lookup_cost: 4.0,
            range_scan_cost: 1.0,
            tuple_fetch_cost: 1.5,
            covering: false,
        }
    }

    /// Default cost factors for a hash index.
    pub fn hash_default() -> Self {
        Self {
            lookup_cost: 1.0,
            range_scan_cost: f64::INFINITY,
            tuple_fetch_cost: 1.5,
            covering: false,
        }
    }

    /// Default cost factors for a BRIN index.
    pub fn brin_default() -> Self {
        Self {
            lookup_cost: 0.5,
            range_scan_cost: 0.1,
            tuple_fetch_cost: 2.0,
            covering: false,
        }
    }

    /// Default cost factors for a GIN index.
    pub fn gin_default() -> Self {
        Self {
            lookup_cost: 3.0,
            range_scan_cost: 0.5,
            tuple_fetch_cost: 2.0,
            covering: false,
        }
    }

    /// Default cost factors for a RUM index.
    ///
    /// RUM extends GIN with distance ordering capability. Lookup cost
    /// is slightly higher (wider posting entries), but range scan cost
    /// is lower for ordered retrieval since results come pre-sorted.
    pub fn rum_default() -> Self {
        Self {
            lookup_cost: 3.5,
            range_scan_cost: 0.3,
            tuple_fetch_cost: 1.8,
            covering: false,
        }
    }

    /// Default cost factors for a columnstore index.
    pub fn columnstore_default() -> Self {
        Self {
            lookup_cost: 10.0,
            range_scan_cost: 0.05,
            tuple_fetch_cost: 0.0,
            covering: true,
        }
    }

    /// Default cost factors for an IVFFlat vector index.
    ///
    /// IVFFlat uses inverted file structure with flat compression.
    /// Lookup cost is moderate (cluster selection + scan), range scan
    /// is relatively expensive as it requires visiting multiple clusters.
    pub fn ivfflat_default() -> Self {
        Self {
            lookup_cost: 5.0,
            range_scan_cost: 2.0,
            tuple_fetch_cost: 1.5,
            covering: false,
        }
    }

    /// Default cost factors for an HNSW vector index.
    ///
    /// HNSW provides faster approximate nearest neighbor search
    /// via hierarchical graph structure. Lookup cost is higher
    /// due to graph traversal, but range scan is more efficient.
    pub fn hnsw_default() -> Self {
        Self {
            lookup_cost: 6.0,
            range_scan_cost: 1.0,
            tuple_fetch_cost: 1.5,
            covering: false,
        }
    }

    /// Default cost factors for a full-text search index.
    ///
    /// Full-text indexes use inverted index structure similar to GIN
    /// but optimized for text search. Lookup cost includes term
    /// dictionary access and posting list retrieval.
    pub fn fulltext_default() -> Self {
        Self {
            lookup_cost: 3.5,
            range_scan_cost: 0.4,
            tuple_fetch_cost: 2.0,
            covering: false,
        }
    }

    /// Default cost factors for SQLite vector extension.
    ///
    /// SQLite-vec provides exact k-NN search with brute force scan.
    /// Lookup cost is low but range scan is expensive (linear scan).
    pub fn sqlite_vec_default() -> Self {
        Self {
            lookup_cost: 2.0,
            range_scan_cost: 5.0,
            tuple_fetch_cost: 1.0,
            covering: false,
        }
    }

    /// Estimated total cost for a point lookup returning `rows` tuples.
    pub fn point_lookup_cost(&self, rows: u64) -> f64 {
        let fetch = if self.covering {
            0.0
        } else {
            rows as f64 * self.tuple_fetch_cost
        };
        self.lookup_cost + fetch
    }

    /// Estimated total cost for a range scan over `pages` leaf pages
    /// returning `rows` tuples.
    pub fn range_cost(&self, pages: u64, rows: u64) -> f64 {
        let scan = pages as f64 * self.range_scan_cost;
        let fetch = if self.covering {
            0.0
        } else {
            rows as f64 * self.tuple_fetch_cost
        };
        self.lookup_cost + scan + fetch
    }
}

impl IndexMetadata {
    /// Whether the index is usable for the given predicate columns.
    pub fn matches_predicate(&self, predicate_columns: &[String]) -> bool {
        let keys = self.index_type.key_columns();
        if keys.is_empty() {
            return false;
        }
        // A prefix of the key columns must match the predicate columns.
        predicate_columns
            .iter()
            .all(|pc| keys.contains(pc))
    }

    /// Whether the leading key column matches the given column.
    pub fn leading_column_matches(&self, column: &str) -> bool {
        self.index_type
            .key_columns()
            .first()
            .is_some_and(|c| c == column)
    }
}

#[cfg(test)]

mod tests {
    use super::*;

    fn sample_stats() -> IndexStats {
        IndexStats {
            index_id: "idx_test".to_string(),
            clustering_factor: 100.0,
            leaf_pages: 500,
            levels: 3,
            avg_leaf_density: 0.7,
            distinct_keys: 100_000,
        }
    }

    // -- IndexType --

    #[test]
    fn clustered_key_columns() {
        let idx = IndexType::Clustered {
            columns: vec!["id".into()],
        };
        assert_eq!(idx.key_columns(), &["id".to_string()]);
    }

    #[test]
    fn nonclustered_key_columns_excludes_included() {
        let idx = IndexType::NonClustered {
            columns: vec!["a".into()],
            included_columns: vec!["b".into()],
        };
        assert_eq!(idx.key_columns(), &["a".to_string()]);
    }

    #[test]
    fn composite_preserves_all_columns() {
        let idx = IndexType::Composite {
            columns: vec!["a".into(), "b".into(), "c".into()],
            column_order: vec![0, 1, 2],
        };
        assert_eq!(idx.key_columns().len(), 3);
    }

    #[test]
    fn hash_does_not_support_range_scan() {
        let idx = IndexType::Hash {
            columns: vec!["id".into()],
        };
        assert!(!idx.supports_range_scan());
        assert!(idx.supports_equality());
    }

    #[test]
    fn btree_supports_both_scan_types() {
        let idx = IndexType::Clustered {
            columns: vec!["id".into()],
        };
        assert!(idx.supports_range_scan());
        assert!(idx.supports_equality());
    }

    #[test]
    fn fulltext_does_not_support_equality() {
        let idx = IndexType::FullText {
            columns: vec!["body".into()],
            language: "english".into(),
            stopwords: None,
        };
        assert!(!idx.supports_equality());
    }

    #[test]
    fn nonclustered_covering_check() {
        let idx = IndexType::NonClustered {
            columns: vec!["a".into()],
            included_columns: vec!["b".into(), "c".into()],
        };
        assert!(idx.is_covering(&["a".into(), "b".into()]));
        assert!(!idx.is_covering(&["a".into(), "d".into()]));
    }

    #[test]
    fn clustered_always_covering() {
        let idx = IndexType::Clustered {
            columns: vec!["id".into()],
        };
        assert!(idx.is_covering(&["id".into(), "name".into(), "anything".into()]));
    }

    #[test]
    fn expression_delegates_to_backing() {
        let idx = IndexType::Expression {
            expression: "lower(email)".into(),
            backing_type: Box::new(IndexType::Unique {
                columns: vec!["email".into()],
            }),
        };
        assert_eq!(idx.key_columns(), &["email".to_string()]);
    }

    #[test]
    fn brin_supports_range_scan() {
        let idx = IndexType::BRIN {
            column: "created_at".into(),
            pages_per_range: 128,
        };
        assert!(idx.supports_range_scan());
    }

    #[test]
    fn gin_single_column_key() {
        let idx = IndexType::GIN {
            column: "tags".into(),
            opclass: "jsonb_ops".into(),
        };
        assert_eq!(idx.key_columns(), &["tags".to_string()]);
    }

    #[test]
    fn gist_single_column_key() {
        let idx = IndexType::GiST {
            column: "geom".into(),
            opclass: "gist_geometry_ops_2d".into(),
        };
        assert_eq!(idx.key_columns(), &["geom".to_string()]);
    }

    #[test]
    fn spatial_single_column_key() {
        let idx = IndexType::Spatial {
            column: "location".into(),
            srid: Some(4326),
        };
        assert_eq!(idx.key_columns(), &["location".to_string()]);
    }

    // -- IndexCostFactors --

    #[test]
    fn btree_default_factors() {
        let f = IndexCostFactors::btree_default();
        assert_eq!(f.lookup_cost, 4.0);
        assert!(!f.covering);
    }

    #[test]
    fn hash_default_infinite_range_cost() {
        let f = IndexCostFactors::hash_default();
        assert!(f.range_scan_cost.is_infinite());
    }

    #[test]
    fn point_lookup_cost_covering() {
        let f = IndexCostFactors {
            lookup_cost: 4.0,
            range_scan_cost: 1.0,
            tuple_fetch_cost: 1.5,
            covering: true,
        };
        assert_eq!(f.point_lookup_cost(10), 4.0);
    }

    #[test]
    fn point_lookup_cost_non_covering() {
        let f = IndexCostFactors::btree_default();
        // 4.0 + 10 * 1.5 = 19.0
        assert_eq!(f.point_lookup_cost(10), 19.0);
    }

    #[test]
    fn range_cost_covering() {
        let f = IndexCostFactors {
            covering: true,
            ..IndexCostFactors::btree_default()
        };
        // 4.0 + 100 * 1.0 + 0 = 104.0
        assert_eq!(f.range_cost(100, 1000), 104.0);
    }

    #[test]
    fn range_cost_non_covering() {
        let f = IndexCostFactors::btree_default();
        // 4.0 + 100 * 1.0 + 1000 * 1.5 = 1604.0
        assert_eq!(f.range_cost(100, 1000), 1604.0);
    }

    #[test]
    fn brin_default_cheap_range() {
        let f = IndexCostFactors::brin_default();
        assert!(f.range_scan_cost < IndexCostFactors::btree_default().range_scan_cost);
    }

    #[test]
    fn columnstore_default_is_covering() {
        let f = IndexCostFactors::columnstore_default();
        assert!(f.covering);
        assert_eq!(f.tuple_fetch_cost, 0.0);
    }

    // -- IndexMetadata --

    #[test]
    fn metadata_matches_predicate() {
        let meta = IndexMetadata {
            name: "idx_orders_date".into(),
            index_type: IndexType::NonClustered {
                columns: vec!["order_date".into(), "customer_id".into()],
                included_columns: vec![],
            },
            table: "orders".into(),
            statistics: sample_stats(),
            cost_factors: IndexCostFactors::btree_default(),
        };
        assert!(meta.matches_predicate(&["order_date".into()]));
        assert!(meta.matches_predicate(&["customer_id".into()]));
        assert!(!meta.matches_predicate(&["amount".into()]));
    }

    #[test]
    fn metadata_leading_column() {
        let meta = IndexMetadata {
            name: "idx_comp".into(),
            index_type: IndexType::Composite {
                columns: vec!["a".into(), "b".into()],
                column_order: vec![0, 1],
            },
            table: "t".into(),
            statistics: sample_stats(),
            cost_factors: IndexCostFactors::btree_default(),
        };
        assert!(meta.leading_column_matches("a"));
        assert!(!meta.leading_column_matches("b"));
    }

    #[test]
    fn metadata_empty_index_no_match() {
        let meta = IndexMetadata {
            name: "idx_empty".into(),
            index_type: IndexType::Clustered { columns: vec![] },
            table: "t".into(),
            statistics: sample_stats(),
            cost_factors: IndexCostFactors::btree_default(),
        };
        assert!(!meta.matches_predicate(&["x".into()]));
    }

    #[test]
    fn serialize_roundtrip() {
        let meta = IndexMetadata {
            name: "idx_rt".into(),
            index_type: IndexType::GIN {
                column: "data".into(),
                opclass: "jsonb_ops".into(),
            },
            table: "events".into(),
            statistics: sample_stats(),
            cost_factors: IndexCostFactors::gin_default(),
        };
        let json = serde_json::to_string(&meta).expect("serialize");
        let back: IndexMetadata =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(meta, back);
    }

    // -- Vector Index Types --

    #[test]
    fn ivfflat_single_column_key() {
        let idx = IndexType::IVFFlat {
            column: "embedding".into(),
            lists: 100,
            distance_metric: DistanceMetric::L2,
        };
        assert_eq!(idx.key_columns(), &["embedding".to_string()]);
    }

    #[test]
    fn ivfflat_supports_ann() {
        let idx = IndexType::IVFFlat {
            column: "embedding".into(),
            lists: 100,
            distance_metric: DistanceMetric::Cosine,
        };
        assert!(idx.supports_ann());
        assert!(!idx.supports_knn());
        assert!(!idx.supports_equality());
        assert!(!idx.supports_range_scan());
    }

    #[test]
    fn hnsw_single_column_key() {
        let idx = IndexType::HNSW {
            column: "vector".into(),
            m: 16,
            ef_construction: 64,
            distance_metric: DistanceMetric::InnerProduct,
        };
        assert_eq!(idx.key_columns(), &["vector".to_string()]);
    }

    #[test]
    fn hnsw_supports_ann() {
        let idx = IndexType::HNSW {
            column: "vector".into(),
            m: 16,
            ef_construction: 64,
            distance_metric: DistanceMetric::L2,
        };
        assert!(idx.supports_ann());
        assert!(!idx.supports_knn());
        assert!(!idx.supports_equality());
        assert!(!idx.supports_range_scan());
    }

    #[test]
    fn sqlite_vec_single_column_key() {
        let idx = IndexType::SQLiteVec {
            column: "vec".into(),
            distance_metric: DistanceMetric::Cosine,
        };
        assert_eq!(idx.key_columns(), &["vec".to_string()]);
    }

    #[test]
    fn sqlite_vec_supports_knn() {
        let idx = IndexType::SQLiteVec {
            column: "vec".into(),
            distance_metric: DistanceMetric::L2,
        };
        assert!(idx.supports_knn());
        assert!(!idx.supports_ann());
        assert!(!idx.supports_equality());
        assert!(!idx.supports_range_scan());
    }

    // -- Full-Text Index Types --

    #[test]
    fn mysql_fulltext_key_columns() {
        let idx = IndexType::MySQLFullText {
            columns: vec!["title".into(), "body".into()],
            parser: FullTextParser::Standard,
        };
        assert_eq!(idx.key_columns().len(), 2);
        assert!(idx.key_columns().contains(&"title".to_string()));
        assert!(idx.key_columns().contains(&"body".to_string()));
    }

    #[test]
    fn mysql_fulltext_supports_phrase_search() {
        let idx = IndexType::MySQLFullText {
            columns: vec!["content".into()],
            parser: FullTextParser::NGram { size: 2 },
        };
        assert!(idx.supports_phrase_search());
        assert!(idx.supports_proximity());
        assert!(!idx.supports_equality());
        assert!(!idx.supports_range_scan());
    }

    #[test]
    fn sqlserver_fulltext_key_columns() {
        let idx = IndexType::SqlServerFullText {
            columns: vec!["description".into()],
            catalog: "ft_catalog".into(),
            language: "English".into(),
        };
        assert_eq!(idx.key_columns(), &["description".to_string()]);
    }

    #[test]
    fn sqlserver_fulltext_supports_phrase_search() {
        let idx = IndexType::SqlServerFullText {
            columns: vec!["text".into()],
            catalog: "ft_catalog".into(),
            language: "English".into(),
        };
        assert!(idx.supports_phrase_search());
        assert!(idx.supports_proximity());
        assert!(!idx.supports_equality());
        assert!(!idx.supports_range_scan());
    }

    #[test]
    fn sqlite_fts5_key_columns() {
        let idx = IndexType::SQLiteFTS5 {
            columns: vec!["name".into(), "description".into()],
            tokenizer: FullTextParser::Porter,
        };
        assert_eq!(idx.key_columns().len(), 2);
    }

    #[test]
    fn sqlite_fts5_supports_phrase_search() {
        let idx = IndexType::SQLiteFTS5 {
            columns: vec!["content".into()],
            tokenizer: FullTextParser::Unicode,
        };
        assert!(idx.supports_phrase_search());
        assert!(idx.supports_proximity());
        assert!(!idx.supports_equality());
        assert!(!idx.supports_range_scan());
    }

    // -- Cost Factors --

    #[test]
    fn ivfflat_default_factors() {
        let f = IndexCostFactors::ivfflat_default();
        assert_eq!(f.lookup_cost, 5.0);
        assert_eq!(f.range_scan_cost, 2.0);
        assert!(!f.covering);
    }

    #[test]
    fn hnsw_default_factors() {
        let f = IndexCostFactors::hnsw_default();
        assert_eq!(f.lookup_cost, 6.0);
        assert_eq!(f.range_scan_cost, 1.0);
        assert!(!f.covering);
    }

    #[test]
    fn fulltext_default_factors() {
        let f = IndexCostFactors::fulltext_default();
        assert_eq!(f.lookup_cost, 3.5);
        assert_eq!(f.range_scan_cost, 0.4);
        assert!(!f.covering);
    }

    #[test]
    fn sqlite_vec_default_factors() {
        let f = IndexCostFactors::sqlite_vec_default();
        assert_eq!(f.lookup_cost, 2.0);
        assert_eq!(f.range_scan_cost, 5.0);
        assert!(!f.covering);
    }

    #[test]
    fn hnsw_faster_than_ivfflat_for_range() {
        let hnsw = IndexCostFactors::hnsw_default();
        let ivfflat = IndexCostFactors::ivfflat_default();
        assert!(hnsw.range_scan_cost < ivfflat.range_scan_cost);
    }

    // -- Serialization Tests --

    #[test]
    fn serialize_ivfflat() {
        let idx = IndexType::IVFFlat {
            column: "emb".into(),
            lists: 50,
            distance_metric: DistanceMetric::Cosine,
        };
        let json = serde_json::to_string(&idx).expect("serialize");
        let back: IndexType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(idx, back);
    }

    #[test]
    fn serialize_hnsw() {
        let idx = IndexType::HNSW {
            column: "vec".into(),
            m: 32,
            ef_construction: 128,
            distance_metric: DistanceMetric::L2,
        };
        let json = serde_json::to_string(&idx).expect("serialize");
        let back: IndexType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(idx, back);
    }

    #[test]
    fn serialize_mysql_fulltext() {
        let idx = IndexType::MySQLFullText {
            columns: vec!["a".into(), "b".into()],
            parser: FullTextParser::NGram { size: 3 },
        };
        let json = serde_json::to_string(&idx).expect("serialize");
        let back: IndexType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(idx, back);
    }

    #[test]
    fn serialize_sqlserver_fulltext() {
        let idx = IndexType::SqlServerFullText {
            columns: vec!["text".into()],
            catalog: "cat".into(),
            language: "en".into(),
        };
        let json = serde_json::to_string(&idx).expect("serialize");
        let back: IndexType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(idx, back);
    }

    #[test]
    fn serialize_sqlite_fts5() {
        let idx = IndexType::SQLiteFTS5 {
            columns: vec!["c1".into()],
            tokenizer: FullTextParser::Porter,
        };
        let json = serde_json::to_string(&idx).expect("serialize");
        let back: IndexType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(idx, back);
    }

    #[test]
    fn serialize_sqlite_vec() {
        let idx = IndexType::SQLiteVec {
            column: "v".into(),
            distance_metric: DistanceMetric::L1,
        };
        let json = serde_json::to_string(&idx).expect("serialize");
        let back: IndexType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(idx, back);
    }

    #[test]
    fn serialize_toml_vector_index() {
        let idx = IndexType::HNSW {
            column: "embedding".into(),
            m: 16,
            ef_construction: 64,
            distance_metric: DistanceMetric::Cosine,
        };
        let toml = toml::to_string(&idx).expect("serialize");
        let back: IndexType = toml::from_str(&toml).expect("deserialize");
        assert_eq!(idx, back);
    }

    #[test]
    fn serialize_toml_fulltext_index() {
        let idx = IndexType::MySQLFullText {
            columns: vec!["title".into(), "body".into()],
            parser: FullTextParser::Standard,
        };
        let toml = toml::to_string(&idx).expect("serialize");
        let back: IndexType = toml::from_str(&toml).expect("deserialize");
        assert_eq!(idx, back);
    }

    // -- Capability Tests --

    #[test]
    fn vector_indexes_dont_support_traditional_ops() {
        let ivfflat = IndexType::IVFFlat {
            column: "v".into(),
            lists: 100,
            distance_metric: DistanceMetric::L2,
        };
        let hnsw = IndexType::HNSW {
            column: "v".into(),
            m: 16,
            ef_construction: 64,
            distance_metric: DistanceMetric::L2,
        };
        let sqlite_vec = IndexType::SQLiteVec {
            column: "v".into(),
            distance_metric: DistanceMetric::Cosine,
        };

        for idx in &[ivfflat, hnsw, sqlite_vec] {
            assert!(!idx.supports_equality());
            assert!(!idx.supports_range_scan());
            assert!(!idx.supports_phrase_search());
            assert!(!idx.supports_proximity());
        }
    }

    #[test]
    fn fulltext_indexes_dont_support_vector_ops() {
        let mysql = IndexType::MySQLFullText {
            columns: vec!["text".into()],
            parser: FullTextParser::Standard,
        };
        let sqlserver = IndexType::SqlServerFullText {
            columns: vec!["text".into()],
            catalog: "c".into(),
            language: "en".into(),
        };
        let sqlite = IndexType::SQLiteFTS5 {
            columns: vec!["text".into()],
            tokenizer: FullTextParser::Unicode,
        };

        for idx in &[mysql, sqlserver, sqlite] {
            assert!(!idx.supports_knn());
            assert!(!idx.supports_ann());
        }
    }

    #[test]
    fn btree_supports_neither_vector_nor_fulltext() {
        let idx = IndexType::Clustered {
            columns: vec!["id".into()],
        };
        assert!(!idx.supports_knn());
        assert!(!idx.supports_ann());
        assert!(!idx.supports_phrase_search());
    }

    // -- Integration Tests --

    #[test]
    fn vector_index_metadata_roundtrip() {
        let meta = IndexMetadata {
            name: "idx_embeddings".into(),
            index_type: IndexType::HNSW {
                column: "embedding".into(),
                m: 16,
                ef_construction: 64,
                distance_metric: DistanceMetric::Cosine,
            },
            table: "documents".into(),
            statistics: sample_stats(),
            cost_factors: IndexCostFactors::hnsw_default(),
        };
        let json = serde_json::to_string(&meta).expect("serialize");
        let back: IndexMetadata =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(meta, back);
    }

    #[test]
    fn fulltext_index_metadata_roundtrip() {
        let meta = IndexMetadata {
            name: "idx_content_ft".into(),
            index_type: IndexType::MySQLFullText {
                columns: vec!["title".into(), "body".into()],
                parser: FullTextParser::NGram { size: 2 },
            },
            table: "articles".into(),
            statistics: sample_stats(),
            cost_factors: IndexCostFactors::fulltext_default(),
        };
        let json = serde_json::to_string(&meta).expect("serialize");
        let back: IndexMetadata =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(meta, back);
    }
}
