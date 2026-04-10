//! Index metadata and capability discovery.
//!
//! Provides a database-agnostic abstraction layer for index types and their
//! capabilities. Instead of hardcoding index types (GIN, RUM, B-tree, etc.)
//! in optimization rules, this module allows runtime discovery of index
//! capabilities from the database catalog.
//!
//! # Design Philosophy
//!
//! Rules should be generic and discover capabilities at runtime:
//! - ✓ `has_index_supporting(table, col, IndexOperation::ArrayContainment)`
//! - ✗ `has_gin_index_on(table, col)` (hardcoded index type)
//!
//! This allows:
//! 1. Rules to work across databases (PostgreSQL GIN, DocumentDB RUM fork)
//! 2. New index types to be added without changing rules
//! 3. Cost models to be database-specific without affecting rule logic
//!
//! # Example
//!
//! ```rust
//! use ra_stats::index_metadata::IndexOperation;
//!
//! // Check available index operations
//! let op = IndexOperation::ArrayContainment;
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::index_types::{IndexCostFactors, IndexType};
use crate::types::IndexStats;

// ------------------------------------------------------------------
// Index Access Methods
// ------------------------------------------------------------------

/// Database-agnostic index access method taxonomy.
///
/// This enum abstracts over different database implementations of
/// similar index structures (e.g., PostgreSQL GIN vs DocumentDB RUM).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IndexAccessMethod {
    /// B-tree or B+ tree (ordered, range scans).
    BTree,
    /// Hash index (equality only, no range scans).
    Hash,
    /// Generalized Inverted Index (PostgreSQL GIN).
    /// Used for arrays, JSONB, full-text.
    GIN,
    /// GIN extension with distance ordering (PostgreSQL RUM).
    RUM,
    /// DocumentDB's RUM fork (BSON-specific).
    DocumentDBRUM,
    /// Generalized Search Tree (PostgreSQL GiST).
    /// Used for spatial, range types, nearest-neighbor.
    GiST,
    /// Block Range Index (PostgreSQL BRIN).
    /// For large, naturally ordered tables.
    BRIN,
    /// Bloom filter index.
    Bloom,
    /// R-tree for spatial data (MySQL, SQL Server).
    RTree,
    /// Column-oriented storage index.
    Columnstore,
    /// Bitmap index for low-cardinality columns.
    Bitmap,
    /// Full-text search index (SQL Server, MySQL).
    FullText,
}

impl IndexAccessMethod {
    /// Parse an access method name from database catalogs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ra_stats::index_metadata::IndexAccessMethod;
    ///
    /// assert_eq!(
    ///     IndexAccessMethod::from_pg_amname("gin"),
    ///     Some(IndexAccessMethod::GIN)
    /// );
    /// assert_eq!(
    ///     IndexAccessMethod::from_pg_amname("rum"),
    ///     Some(IndexAccessMethod::RUM)
    /// );
    /// ```
    #[must_use]
    pub fn from_pg_amname(amname: &str) -> Option<Self> {
        match amname {
            "btree" => Some(Self::BTree),
            "hash" => Some(Self::Hash),
            "gin" => Some(Self::GIN),
            "rum" => Some(Self::RUM),
            "gist" => Some(Self::GiST),
            "brin" => Some(Self::BRIN),
            "bloom" => Some(Self::Bloom),
            _ => None,
        }
    }

    /// Whether this access method supports ordered scans.
    #[must_use]
    pub fn supports_ordered_scan(self) -> bool {
        matches!(
            self,
            Self::BTree | Self::BRIN | Self::RUM | Self::DocumentDBRUM
        )
    }

    /// Whether this access method supports bitmap scans.
    #[must_use]
    pub fn supports_bitmap_scan(self) -> bool {
        matches!(
            self,
            Self::GIN
                | Self::RUM
                | Self::DocumentDBRUM
                | Self::GiST
                | Self::BRIN
                | Self::Bitmap
        )
    }

    /// Whether this access method supports point lookups.
    #[must_use]
    pub fn supports_point_lookup(self) -> bool {
        matches!(
            self,
            Self::BTree | Self::Hash | Self::GIN | Self::RUM | Self::DocumentDBRUM
        )
    }

    /// Whether this access method supports range scans.
    #[must_use]
    pub fn supports_range_scan(self) -> bool {
        matches!(
            self,
            Self::BTree | Self::BRIN | Self::RUM | Self::DocumentDBRUM
        )
    }
}

impl std::fmt::Display for IndexAccessMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BTree => write!(f, "btree"),
            Self::Hash => write!(f, "hash"),
            Self::GIN => write!(f, "gin"),
            Self::RUM => write!(f, "rum"),
            Self::DocumentDBRUM => write!(f, "documentdb_rum"),
            Self::GiST => write!(f, "gist"),
            Self::BRIN => write!(f, "brin"),
            Self::Bloom => write!(f, "bloom"),
            Self::RTree => write!(f, "rtree"),
            Self::Columnstore => write!(f, "columnstore"),
            Self::Bitmap => write!(f, "bitmap"),
            Self::FullText => write!(f, "fulltext"),
        }
    }
}

// ------------------------------------------------------------------
// Index Operations
// ------------------------------------------------------------------

/// Database-agnostic index operations that rules can query for.
///
/// Instead of checking `has_gin_index()`, rules check
/// `has_index_supporting(ArrayContainment)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IndexOperation {
    /// Array containment operators (@>, &&, <@).
    ArrayContainment,
    /// JSONB/JSON containment operators (@>, ?).
    JsonContainment,
    /// Full-text search (@@, to_tsquery).
    FullTextSearch,
    /// Phrase search with positions (<->).
    PhraseSearch,
    /// Spatial containment (ST_Contains, ST_Within).
    SpatialContainment,
    /// Spatial intersection (ST_Intersects, &&).
    SpatialIntersection,
    /// Geospatial distance ordering (<->).
    GeospatialDistance,
    /// K-nearest-neighbor search.
    KNNSearch,
    /// JSON path extraction.
    JsonPath,
    /// Range containment (tsrange, int4range).
    RangeContainment,
    /// Equality on scalar values.
    ScalarEquality,
    /// Range scan on scalar values.
    ScalarRange,
    /// Text pattern matching (LIKE, ILIKE, ~).
    PatternMatching,
}

impl std::fmt::Display for IndexOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ArrayContainment => write!(f, "array_containment"),
            Self::JsonContainment => write!(f, "json_containment"),
            Self::FullTextSearch => write!(f, "full_text_search"),
            Self::PhraseSearch => write!(f, "phrase_search"),
            Self::SpatialContainment => write!(f, "spatial_containment"),
            Self::SpatialIntersection => write!(f, "spatial_intersection"),
            Self::GeospatialDistance => write!(f, "geospatial_distance"),
            Self::KNNSearch => write!(f, "knn_search"),
            Self::JsonPath => write!(f, "json_path"),
            Self::RangeContainment => write!(f, "range_containment"),
            Self::ScalarEquality => write!(f, "scalar_equality"),
            Self::ScalarRange => write!(f, "scalar_range"),
            Self::PatternMatching => write!(f, "pattern_matching"),
        }
    }
}

// ------------------------------------------------------------------
// Index Capabilities
// ------------------------------------------------------------------

/// Capabilities of an index for a specific access method and operator family.
///
/// This struct encodes what operations an index can perform, independent
/// of the specific index type name (GIN vs RUM vs DocumentDB RUM).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexCapabilities {
    /// Ordered scans (supports ORDER BY).
    pub supports_ordered_scan: bool,
    /// Bitmap scans (multiple index intersection).
    pub supports_bitmap_scan: bool,
    /// Point lookups (equality predicates).
    pub supports_point_lookup: bool,
    /// Range scans (inequality predicates).
    pub supports_range_scan: bool,
    /// Containment operators (@>, &&).
    pub supports_containment: bool,
    /// Distance-ordered retrieval (KNN).
    pub supports_distance_ordering: bool,
    /// Full-text search.
    pub supports_full_text: bool,
    /// Phrase search with positions.
    pub supports_phrase_search: bool,
    /// Spatial operations.
    pub supports_spatial: bool,
    /// JSON path operations.
    pub supports_json_path: bool,
    /// Cost factors for this index.
    pub cost_factors: IndexCostFactors,
    /// Additional operator-family-specific properties.
    pub properties: HashMap<String, String>,
}

impl IndexCapabilities {
    /// Create capabilities from an access method and operator family.
    ///
    /// This maps (access_method, opfamily) → capabilities, encoding
    /// knowledge about what different index types can do.
    #[must_use]
    pub fn from_access_method_and_opfamily(
        access_method: IndexAccessMethod,
        opfamily: &str,
    ) -> Self {
        match (access_method, opfamily) {
            // PostgreSQL GIN indexes
            (IndexAccessMethod::GIN, "array_ops") => Self {
                supports_ordered_scan: false,
                supports_bitmap_scan: true,
                supports_point_lookup: false,
                supports_range_scan: false,
                supports_containment: true,
                supports_distance_ordering: false,
                supports_full_text: false,
                supports_phrase_search: false,
                supports_spatial: false,
                supports_json_path: false,
                cost_factors: IndexCostFactors::gin_default(),
                properties: HashMap::new(),
            },
            (IndexAccessMethod::GIN, "jsonb_ops" | "jsonb_path_ops") => Self {
                supports_ordered_scan: false,
                supports_bitmap_scan: true,
                supports_point_lookup: false,
                supports_range_scan: false,
                supports_containment: true,
                supports_distance_ordering: false,
                supports_full_text: false,
                supports_phrase_search: false,
                supports_spatial: false,
                supports_json_path: opfamily == "jsonb_ops",
                cost_factors: IndexCostFactors::gin_default(),
                properties: HashMap::new(),
            },
            (IndexAccessMethod::GIN, f) if f.contains("tsvector") => Self {
                supports_ordered_scan: false,
                supports_bitmap_scan: true,
                supports_point_lookup: false,
                supports_range_scan: false,
                supports_containment: false,
                supports_distance_ordering: false,
                supports_full_text: true,
                supports_phrase_search: false, // GIN requires heap recheck
                supports_spatial: false,
                supports_json_path: false,
                cost_factors: IndexCostFactors::gin_default(),
                properties: HashMap::new(),
            },

            // PostgreSQL RUM indexes
            (IndexAccessMethod::RUM, "rum_tsvector_ops") => Self {
                supports_ordered_scan: true,
                supports_bitmap_scan: true,
                supports_point_lookup: false,
                supports_range_scan: false,
                supports_containment: false,
                supports_distance_ordering: true,
                supports_full_text: true,
                supports_phrase_search: true, // RUM: in-index verification
                supports_spatial: false,
                supports_json_path: false,
                cost_factors: IndexCostFactors::rum_default(),
                properties: HashMap::new(),
            },
            (IndexAccessMethod::RUM, "rum_tsvector_addon_ops") => {
                let mut props = HashMap::new();
                props.insert("addon_ordering".to_string(), "true".to_string());
                Self {
                    supports_ordered_scan: true,
                    supports_bitmap_scan: true,
                    supports_point_lookup: false,
                    supports_range_scan: false,
                    supports_containment: false,
                    supports_distance_ordering: true,
                    supports_full_text: true,
                    supports_phrase_search: true,
                    supports_spatial: false,
                    supports_json_path: false,
                    cost_factors: IndexCostFactors::rum_default(),
                    properties: props,
                }
            }
            (IndexAccessMethod::RUM, "rum_anyarray_ops") => Self {
                supports_ordered_scan: false,
                supports_bitmap_scan: true,
                supports_point_lookup: false,
                supports_range_scan: false,
                supports_containment: true,
                supports_distance_ordering: false,
                supports_full_text: false,
                supports_phrase_search: false,
                supports_spatial: false,
                supports_json_path: false,
                cost_factors: IndexCostFactors::rum_default(),
                properties: HashMap::new(),
            },

            // DocumentDB RUM (BSON-specific)
            (IndexAccessMethod::DocumentDBRUM, "bson_extended_rum_single_path_ops") => Self {
                supports_ordered_scan: true,
                supports_bitmap_scan: true,
                supports_point_lookup: false,
                supports_range_scan: true, // BSON range queries
                supports_containment: true,
                supports_distance_ordering: true,
                supports_full_text: true, // $text search
                supports_phrase_search: true,
                supports_spatial: false,
                supports_json_path: true, // BSON path extraction
                cost_factors: IndexCostFactors::rum_default(),
                properties: HashMap::new(),
            },
            (IndexAccessMethod::DocumentDBRUM, "bson_extended_rum_composite_path_ops") => {
                let mut props = HashMap::new();
                props.insert("composite".to_string(), "true".to_string());
                Self {
                    supports_ordered_scan: true,
                    supports_bitmap_scan: true,
                    supports_point_lookup: false,
                    supports_range_scan: true,
                    supports_containment: true,
                    supports_distance_ordering: true, // $near geospatial
                    supports_full_text: true,
                    supports_phrase_search: true,
                    supports_spatial: true, // $near operator
                    supports_json_path: true,
                    cost_factors: IndexCostFactors::rum_default(),
                    properties: props,
                }
            }

            // GiST indexes
            (IndexAccessMethod::GiST, f) if f.contains("geometry") || f.contains("geography") => {
                Self {
                    supports_ordered_scan: false,
                    supports_bitmap_scan: true,
                    supports_point_lookup: false,
                    supports_range_scan: false,
                    supports_containment: false,
                    supports_distance_ordering: true, // <-> operator
                    supports_full_text: false,
                    supports_phrase_search: false,
                    supports_spatial: true,
                    supports_json_path: false,
                    cost_factors: IndexCostFactors::btree_default(), // TODO: GiST-specific
                    properties: HashMap::new(),
                }
            }
            (IndexAccessMethod::GiST, _) => Self {
                supports_ordered_scan: false,
                supports_bitmap_scan: true,
                supports_point_lookup: false,
                supports_range_scan: false,
                supports_containment: true, // Range types, etc.
                supports_distance_ordering: false,
                supports_full_text: false,
                supports_phrase_search: false,
                supports_spatial: false,
                supports_json_path: false,
                cost_factors: IndexCostFactors::btree_default(),
                properties: HashMap::new(),
            },

            // B-tree (default for most scalar operations)
            (IndexAccessMethod::BTree, _) => Self {
                supports_ordered_scan: true,
                supports_bitmap_scan: true,
                supports_point_lookup: true,
                supports_range_scan: true,
                supports_containment: false,
                supports_distance_ordering: false,
                supports_full_text: false,
                supports_phrase_search: false,
                supports_spatial: false,
                supports_json_path: false,
                cost_factors: IndexCostFactors::btree_default(),
                properties: HashMap::new(),
            },

            // Hash (equality only)
            (IndexAccessMethod::Hash, _) => Self {
                supports_ordered_scan: false,
                supports_bitmap_scan: false,
                supports_point_lookup: true,
                supports_range_scan: false,
                supports_containment: false,
                supports_distance_ordering: false,
                supports_full_text: false,
                supports_phrase_search: false,
                supports_spatial: false,
                supports_json_path: false,
                cost_factors: IndexCostFactors::hash_default(),
                properties: HashMap::new(),
            },

            // BRIN (block-range)
            (IndexAccessMethod::BRIN, _) => Self {
                supports_ordered_scan: true,
                supports_bitmap_scan: true,
                supports_point_lookup: false,
                supports_range_scan: true,
                supports_containment: false,
                supports_distance_ordering: false,
                supports_full_text: false,
                supports_phrase_search: false,
                supports_spatial: false,
                supports_json_path: false,
                cost_factors: IndexCostFactors::brin_default(),
                properties: HashMap::new(),
            },

            // Fallback: use B-tree defaults
            _ => Self::from_access_method_and_opfamily(IndexAccessMethod::BTree, ""),
        }
    }

    /// Check if this index supports a specific operation.
    #[must_use]
    pub fn supports_operation(&self, op: &IndexOperation) -> bool {
        match op {
            IndexOperation::ArrayContainment => self.supports_containment,
            IndexOperation::JsonContainment => self.supports_containment || self.supports_json_path,
            IndexOperation::FullTextSearch => self.supports_full_text,
            IndexOperation::PhraseSearch => self.supports_phrase_search,
            IndexOperation::SpatialContainment => self.supports_spatial,
            IndexOperation::SpatialIntersection => self.supports_spatial,
            IndexOperation::GeospatialDistance => self.supports_distance_ordering && self.supports_spatial,
            IndexOperation::KNNSearch => self.supports_distance_ordering,
            IndexOperation::JsonPath => self.supports_json_path,
            IndexOperation::RangeContainment => self.supports_containment,
            IndexOperation::ScalarEquality => self.supports_point_lookup,
            IndexOperation::ScalarRange => self.supports_range_scan,
            IndexOperation::PatternMatching => self.supports_full_text,
        }
    }
}

// ------------------------------------------------------------------
// Index Metadata
// ------------------------------------------------------------------

/// Complete metadata for a database index.
///
/// Combines structural information (columns, type) with discovered
/// capabilities and statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexMetadata {
    /// Index name (e.g., "idx_articles_tags").
    pub name: String,
    /// Table the index belongs to.
    pub table: String,
    /// Indexed columns.
    pub columns: Vec<String>,
    /// Access method (btree, gin, rum, etc.).
    pub access_method: IndexAccessMethod,
    /// Operator family (e.g., "jsonb_ops", "rum_tsvector_ops").
    pub operator_family: String,
    /// Discovered capabilities.
    pub capabilities: IndexCapabilities,
    /// Physical statistics.
    pub statistics: IndexStats,
}

impl IndexMetadata {
    /// Check if this index supports a specific operation.
    #[must_use]
    pub fn supports_operation(&self, op: &IndexOperation) -> bool {
        self.capabilities.supports_operation(op)
    }

    /// Estimate the cost of using this index for a query.
    ///
    /// The cost model is access-method-specific and accounts for:
    /// - Tree height / posting list structure
    /// - Selectivity
    /// - Heap fetch costs (covering vs non-covering)
    #[must_use]
    pub fn estimate_scan_cost(
        &self,
        selectivity: f64,
        table_rows: u64,
        limit: Option<u64>,
    ) -> f64 {
        let matching_rows = (table_rows as f64 * selectivity).max(1.0);
        let effective_rows = match limit {
            Some(k) => (k as f64 * 1.2).min(matching_rows),
            None => matching_rows,
        };

        match self.access_method {
            IndexAccessMethod::BTree => {
                let tree_traversal = self.statistics.levels as f64 * 4.0;
                let scan = effective_rows * self.capabilities.cost_factors.range_scan_cost;
                let fetch = if self.capabilities.cost_factors.covering {
                    0.0
                } else {
                    effective_rows * self.capabilities.cost_factors.tuple_fetch_cost
                };
                tree_traversal + scan + fetch
            }
            IndexAccessMethod::Hash => {
                let hash_lookup = self.capabilities.cost_factors.lookup_cost;
                let fetch = if self.capabilities.cost_factors.covering {
                    0.0
                } else {
                    effective_rows * self.capabilities.cost_factors.tuple_fetch_cost
                };
                hash_lookup + fetch
            }
            IndexAccessMethod::GIN => {
                let posting_scan = effective_rows * self.capabilities.cost_factors.range_scan_cost;
                let fetch = effective_rows * self.capabilities.cost_factors.tuple_fetch_cost;
                self.capabilities.cost_factors.lookup_cost + posting_scan + fetch
            }
            IndexAccessMethod::RUM | IndexAccessMethod::DocumentDBRUM => {
                // RUM: benefits from limit due to distance ordering
                let posting_scan = effective_rows * self.capabilities.cost_factors.range_scan_cost;
                let fetch = effective_rows * self.capabilities.cost_factors.tuple_fetch_cost;
                self.capabilities.cost_factors.lookup_cost + posting_scan + fetch
            }
            IndexAccessMethod::GiST => {
                let tree_traversal = self.statistics.levels as f64 * 2.0;
                let node_checks = effective_rows * 0.5; // Bounding box checks
                let recheck = effective_rows * 5.0; // Exact geometry tests
                tree_traversal + node_checks + recheck
            }
            IndexAccessMethod::BRIN => {
                let block_scan = (table_rows / 1000) as f64 * 0.1; // Very cheap
                let range_checks = block_scan;
                let fetch = effective_rows * self.capabilities.cost_factors.tuple_fetch_cost;
                block_scan + range_checks + fetch
            }
            _ => {
                // Fallback: use B-tree model
                let scan = effective_rows * self.capabilities.cost_factors.range_scan_cost;
                let fetch = effective_rows * self.capabilities.cost_factors.tuple_fetch_cost;
                self.capabilities.cost_factors.lookup_cost + scan + fetch
            }
        }
    }

    /// Whether this index matches a predicate on given columns.
    #[must_use]
    pub fn matches_predicate(&self, predicate_columns: &[String]) -> bool {
        if self.columns.is_empty() || predicate_columns.is_empty() {
            return false;
        }
        // Leading column must match
        predicate_columns
            .iter()
            .all(|pc| self.columns.contains(pc))
    }

    /// Map to the old `IndexMetadata` type for backward compatibility.
    ///
    /// This allows gradual migration from hardcoded index types.
    #[must_use]
    pub fn to_legacy_index_type(&self) -> IndexType {
        match self.access_method {
            IndexAccessMethod::GIN => IndexType::GIN {
                column: self.columns.first().cloned().unwrap_or_default(),
                opclass: self.operator_family.clone(),
            },
            IndexAccessMethod::RUM => IndexType::RUM {
                column: self.columns.first().cloned().unwrap_or_default(),
                opclass: self.operator_family.clone(),
                addon_column: self
                    .capabilities
                    .properties
                    .get("addon_column")
                    .cloned(),
            },
            IndexAccessMethod::GiST => IndexType::GiST {
                column: self.columns.first().cloned().unwrap_or_default(),
                opclass: self.operator_family.clone(),
            },
            IndexAccessMethod::BRIN => IndexType::BRIN {
                column: self.columns.first().cloned().unwrap_or_default(),
                pages_per_range: 128, // Default
            },
            IndexAccessMethod::Hash => IndexType::Hash {
                columns: self.columns.clone(),
            },
            IndexAccessMethod::BTree => {
                if self.columns.len() == 1 {
                    IndexType::NonClustered {
                        columns: self.columns.clone(),
                        included_columns: vec![],
                    }
                } else {
                    IndexType::Composite {
                        columns: self.columns.clone(),
                        column_order: (0..self.columns.len()).collect(),
                    }
                }
            }
            _ => IndexType::NonClustered {
                columns: self.columns.clone(),
                included_columns: vec![],
            },
        }
    }
}

// ------------------------------------------------------------------
// Discovery Functions
// ------------------------------------------------------------------

/// Discover all indexes on a table from the database catalog.
///
/// This is a placeholder for actual catalog queries. In production, this
/// would query `pg_index`, `pg_class`, `pg_am`, `pg_opfamily`, etc.
///
/// # Example (PostgreSQL)
///
/// ```sql
/// SELECT
///     i.relname AS index_name,
///     t.relname AS table_name,
///     a.amname AS access_method,
///     opf.opfname AS operator_family,
///     array_agg(att.attname ORDER BY k.i) AS columns
/// FROM pg_index idx
/// JOIN pg_class i ON i.oid = idx.indexrelid
/// JOIN pg_class t ON t.oid = idx.indrelid
/// JOIN pg_am a ON a.oid = i.relam
/// JOIN pg_opclass opc ON opc.oid = ANY(idx.indclass)
/// JOIN pg_opfamily opf ON opf.oid = opc.opcfamily
/// CROSS JOIN LATERAL unnest(idx.indkey::int[]) WITH ORDINALITY AS k(attnum, i)
/// JOIN pg_attribute att ON att.attrelid = t.oid AND att.attnum = k.attnum
/// WHERE t.relname = $1
/// GROUP BY i.relname, t.relname, a.amname, opf.opfname;
/// ```
#[must_use]
pub fn discover_indexes_for_table(
    _connection_string: &str,
    table: &str,
) -> Vec<IndexMetadata> {
    // Placeholder: in production, this would query the catalog
    let _ = table;
    vec![]
}

/// Find indexes supporting a specific operation on a table/column.
///
/// This is the function that rules call instead of `has_gin_index_on()`.
///
/// # Example
///
/// ```rust,no_run
/// use ra_stats::index_metadata::{find_indexes_supporting, IndexOperation};
///
/// let indexes = find_indexes_supporting(
///     "postgresql://localhost/mydb",
///     "articles",
///     "tags",
///     &IndexOperation::ArrayContainment,
/// );
///
/// if let Some(best_index) = indexes.first() {
///     println!("Using {} for array containment", best_index.name);
/// }
/// ```
#[must_use]
pub fn find_indexes_supporting(
    connection_string: &str,
    table: &str,
    column: &str,
    operation: &IndexOperation,
) -> Vec<IndexMetadata> {
    discover_indexes_for_table(connection_string, table)
        .into_iter()
        .filter(|idx| {
            idx.columns.contains(&column.to_string())
                && idx.supports_operation(operation)
        })
        .collect()
}

#[cfg(test)]

mod tests {
    use super::*;

    // -- IndexAccessMethod tests --

    #[test]
    fn parse_pg_amname() {
        assert_eq!(
            IndexAccessMethod::from_pg_amname("gin"),
            Some(IndexAccessMethod::GIN)
        );
        assert_eq!(
            IndexAccessMethod::from_pg_amname("rum"),
            Some(IndexAccessMethod::RUM)
        );
        assert_eq!(
            IndexAccessMethod::from_pg_amname("btree"),
            Some(IndexAccessMethod::BTree)
        );
        assert_eq!(
            IndexAccessMethod::from_pg_amname("gist"),
            Some(IndexAccessMethod::GiST)
        );
        assert_eq!(IndexAccessMethod::from_pg_amname("unknown"), None);
    }

    #[test]
    fn access_method_capabilities() {
        assert!(IndexAccessMethod::BTree.supports_ordered_scan());
        assert!(!IndexAccessMethod::Hash.supports_ordered_scan());
        assert!(IndexAccessMethod::RUM.supports_ordered_scan());

        assert!(IndexAccessMethod::GIN.supports_bitmap_scan());
        assert!(!IndexAccessMethod::Hash.supports_bitmap_scan());

        assert!(IndexAccessMethod::BTree.supports_range_scan());
        assert!(!IndexAccessMethod::Hash.supports_range_scan());
    }

    // -- IndexCapabilities tests --

    #[test]
    fn gin_array_ops_capabilities() {
        let caps = IndexCapabilities::from_access_method_and_opfamily(
            IndexAccessMethod::GIN,
            "array_ops",
        );
        assert!(caps.supports_containment);
        assert!(!caps.supports_ordered_scan);
        assert!(!caps.supports_distance_ordering);
        assert!(caps.supports_operation(&IndexOperation::ArrayContainment));
        assert!(!caps.supports_operation(&IndexOperation::FullTextSearch));
    }

    #[test]
    fn rum_tsvector_ops_capabilities() {
        let caps = IndexCapabilities::from_access_method_and_opfamily(
            IndexAccessMethod::RUM,
            "rum_tsvector_ops",
        );
        assert!(caps.supports_full_text);
        assert!(caps.supports_phrase_search);
        assert!(caps.supports_distance_ordering);
        assert!(caps.supports_ordered_scan);
        assert!(caps.supports_operation(&IndexOperation::FullTextSearch));
        assert!(caps.supports_operation(&IndexOperation::PhraseSearch));
        assert!(caps.supports_operation(&IndexOperation::KNNSearch));
    }

    #[test]
    fn documentdb_rum_composite_capabilities() {
        let caps = IndexCapabilities::from_access_method_and_opfamily(
            IndexAccessMethod::DocumentDBRUM,
            "bson_extended_rum_composite_path_ops",
        );
        assert!(caps.supports_spatial);
        assert!(caps.supports_distance_ordering);
        assert!(caps.supports_json_path);
        assert!(caps.supports_containment);
        assert_eq!(caps.properties.get("composite"), Some(&"true".to_string()));
    }

    #[test]
    fn gist_geometry_capabilities() {
        let caps = IndexCapabilities::from_access_method_and_opfamily(
            IndexAccessMethod::GiST,
            "gist_geometry_ops_2d",
        );
        assert!(caps.supports_spatial);
        assert!(caps.supports_distance_ordering);
        assert!(caps.supports_operation(&IndexOperation::SpatialContainment));
        assert!(caps.supports_operation(&IndexOperation::GeospatialDistance));
    }

    #[test]
    fn btree_capabilities() {
        let caps = IndexCapabilities::from_access_method_and_opfamily(
            IndexAccessMethod::BTree,
            "",
        );
        assert!(caps.supports_ordered_scan);
        assert!(caps.supports_point_lookup);
        assert!(caps.supports_range_scan);
        assert!(caps.supports_operation(&IndexOperation::ScalarEquality));
        assert!(caps.supports_operation(&IndexOperation::ScalarRange));
        assert!(!caps.supports_operation(&IndexOperation::ArrayContainment));
    }

    #[test]
    fn hash_capabilities() {
        let caps = IndexCapabilities::from_access_method_and_opfamily(
            IndexAccessMethod::Hash,
            "",
        );
        assert!(caps.supports_point_lookup);
        assert!(!caps.supports_ordered_scan);
        assert!(!caps.supports_range_scan);
        assert!(caps.supports_operation(&IndexOperation::ScalarEquality));
        assert!(!caps.supports_operation(&IndexOperation::ScalarRange));
    }

    // -- IndexMetadata tests --

    #[test]
    fn index_supports_operation() {
        let idx = IndexMetadata {
            name: "idx_articles_tags".to_string(),
            table: "articles".to_string(),
            columns: vec!["tags".to_string()],
            access_method: IndexAccessMethod::GIN,
            operator_family: "array_ops".to_string(),
            capabilities: IndexCapabilities::from_access_method_and_opfamily(
                IndexAccessMethod::GIN,
                "array_ops",
            ),
            statistics: IndexStats {
                index_id: "idx_articles_tags".to_string(),
                clustering_factor: 100.0,
                leaf_pages: 500,
                levels: 3,
                avg_leaf_density: 0.7,
                distinct_keys: 10_000,
            },
        };

        assert!(idx.supports_operation(&IndexOperation::ArrayContainment));
        assert!(!idx.supports_operation(&IndexOperation::FullTextSearch));
    }

    #[test]
    fn index_cost_estimation_gin() {
        let idx = IndexMetadata {
            name: "idx_test".to_string(),
            table: "test".to_string(),
            columns: vec!["col".to_string()],
            access_method: IndexAccessMethod::GIN,
            operator_family: "array_ops".to_string(),
            capabilities: IndexCapabilities::from_access_method_and_opfamily(
                IndexAccessMethod::GIN,
                "array_ops",
            ),
            statistics: IndexStats {
                index_id: "idx_test".to_string(),
                clustering_factor: 100.0,
                leaf_pages: 1000,
                levels: 3,
                avg_leaf_density: 0.7,
                distinct_keys: 100_000,
            },
        };

        let cost = idx.estimate_scan_cost(0.01, 1_000_000, None);
        assert!(cost > 0.0);
        assert!(cost < 1_000_000.0); // Should be cheaper than seq scan
    }

    #[test]
    fn index_cost_estimation_with_limit() {
        let idx = IndexMetadata {
            name: "idx_rum".to_string(),
            table: "test".to_string(),
            columns: vec!["tsv".to_string()],
            access_method: IndexAccessMethod::RUM,
            operator_family: "rum_tsvector_ops".to_string(),
            capabilities: IndexCapabilities::from_access_method_and_opfamily(
                IndexAccessMethod::RUM,
                "rum_tsvector_ops",
            ),
            statistics: IndexStats {
                index_id: "idx_rum".to_string(),
                clustering_factor: 100.0,
                leaf_pages: 1000,
                levels: 3,
                avg_leaf_density: 0.7,
                distinct_keys: 100_000,
            },
        };

        let cost_no_limit = idx.estimate_scan_cost(0.1, 1_000_000, None);
        let cost_with_limit = idx.estimate_scan_cost(0.1, 1_000_000, Some(10));

        assert!(
            cost_with_limit < cost_no_limit,
            "RUM with limit should be cheaper due to distance ordering"
        );
    }

    #[test]
    fn index_matches_predicate() {
        let idx = IndexMetadata {
            name: "idx_comp".to_string(),
            table: "test".to_string(),
            columns: vec!["a".to_string(), "b".to_string()],
            access_method: IndexAccessMethod::BTree,
            operator_family: "btree_ops".to_string(),
            capabilities: IndexCapabilities::from_access_method_and_opfamily(
                IndexAccessMethod::BTree,
                "",
            ),
            statistics: IndexStats {
                index_id: "idx_comp".to_string(),
                clustering_factor: 100.0,
                leaf_pages: 500,
                levels: 3,
                avg_leaf_density: 0.7,
                distinct_keys: 10_000,
            },
        };

        assert!(idx.matches_predicate(&["a".to_string()]));
        assert!(idx.matches_predicate(&["a".to_string(), "b".to_string()]));
        assert!(!idx.matches_predicate(&["c".to_string()]));
    }

    #[test]
    fn to_legacy_index_type_gin() {
        let idx = IndexMetadata {
            name: "idx_gin".to_string(),
            table: "test".to_string(),
            columns: vec!["data".to_string()],
            access_method: IndexAccessMethod::GIN,
            operator_family: "jsonb_ops".to_string(),
            capabilities: IndexCapabilities::from_access_method_and_opfamily(
                IndexAccessMethod::GIN,
                "jsonb_ops",
            ),
            statistics: IndexStats {
                index_id: "idx_gin".to_string(),
                clustering_factor: 100.0,
                leaf_pages: 500,
                levels: 3,
                avg_leaf_density: 0.7,
                distinct_keys: 10_000,
            },
        };

        let legacy = idx.to_legacy_index_type();
        assert!(matches!(legacy, IndexType::GIN { .. }));
    }

    #[test]
    fn find_indexes_supporting_returns_empty_on_no_match() {
        // Without actual catalog connection, should return empty
        let indexes = find_indexes_supporting(
            "mock",
            "test_table",
            "test_col",
            &IndexOperation::ArrayContainment,
        );
        assert!(indexes.is_empty());
    }
}
