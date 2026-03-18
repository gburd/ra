//! Index type modeling for query optimization.
//!
//! Provides comprehensive index type definitions covering the major
//! index structures found across `PostgreSQL`, `MySQL`, SQL Server,
//! Oracle, and other database systems. Each index type carries
//! metadata and cost factors used by the physical plan optimizer
//! to select appropriate access paths.

use serde::{Deserialize, Serialize};

use crate::types::ColumnId;

/// Supported index types across database systems.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IndexType {
    /// B-tree index where leaf pages store the actual table rows.
    /// Data is physically ordered by the index key.
    /// (`InnoDB` primary key, SQL Server clustered index, Oracle IOT)
    Clustered {
        /// Columns forming the clustering key.
        columns: Vec<ColumnId>,
    },

    /// Standard B-tree index with pointers back to heap rows.
    NonClustered {
        /// Indexed columns (key columns).
        columns: Vec<ColumnId>,
        /// Non-key columns stored in leaf pages (SQL Server INCLUDE).
        included_columns: Vec<ColumnId>,
    },

    /// Multi-column B-tree index with defined column ordering.
    Composite {
        /// Columns in index order (leftmost prefix matters).
        columns: Vec<ColumnId>,
        /// Sort direction per column.
        column_order: Vec<SortDirection>,
    },

    /// Full-text search index for natural language queries.
    FullText {
        /// Text columns indexed.
        columns: Vec<ColumnId>,
        /// Language configuration for stemming/stopwords.
        language: String,
        /// Whether custom stopwords are configured.
        custom_stopwords: bool,
    },

    /// B-tree index with a uniqueness constraint.
    Unique {
        /// Columns forming the unique key.
        columns: Vec<ColumnId>,
    },

    /// Partial/filtered index that only indexes rows matching a predicate.
    /// (`PostgreSQL` WHERE clause, SQL Server filtered index)
    Filtered {
        /// Indexed columns.
        columns: Vec<ColumnId>,
        /// Filter predicate (SQL expression).
        filter_predicate: String,
    },

    /// R-tree or similar spatial index for geometric data.
    /// (`PostGIS` `GiST`, `MySQL` SPATIAL, SQL Server spatial)
    Spatial {
        /// Geometry/geography column.
        column: ColumnId,
        /// Spatial reference system identifier.
        srid: u32,
    },

    /// Column-oriented index for analytical workloads.
    /// (SQL Server columnstore, `ClickHouse` primary index)
    Columnstore {
        /// Columns stored in columnar format.
        columns: Vec<ColumnId>,
    },

    /// Hash index for exact-match lookups.
    /// (`PostgreSQL` hash index, memory-optimized tables)
    Hash {
        /// Hashed columns.
        columns: Vec<ColumnId>,
    },

    /// Generalized Inverted Index for composite/array/fulltext values.
    /// (`PostgreSQL` GIN)
    Gin {
        /// Indexed column.
        column: ColumnId,
        /// Operator class (e.g., `jsonb_ops`, `tsvector_ops`).
        opclass: String,
    },

    /// Generalized Search Tree for range-overlapping types.
    /// (`PostgreSQL` `GiST` for ranges, geometries, full-text)
    Gist {
        /// Indexed column.
        column: ColumnId,
        /// Operator class (e.g., `gist_trgm_ops`).
        opclass: String,
    },
}

/// Sort direction for composite index columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    /// Ascending order (default).
    Ascending,
    /// Descending order.
    Descending,
}

/// Metadata describing an index instance on a table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexMetadata {
    /// Unique identifier for this index.
    pub index_id: String,
    /// Name of the table this index belongs to.
    pub table_name: String,
    /// The index type and its structural parameters.
    pub index_type: IndexType,
    /// Whether this index enforces a primary key constraint.
    pub is_primary: bool,
    /// Whether this index enforces a uniqueness constraint.
    pub is_unique: bool,
    /// Whether the index is valid and usable (not in a failed build state).
    pub is_valid: bool,
    /// Estimated size of the index in bytes.
    pub size_bytes: u64,
    /// Number of leaf pages (B-tree) or equivalent storage units.
    pub leaf_pages: u64,
    /// Tree height / number of levels.
    pub levels: u32,
    /// Fraction of leaf pages that are filled (0.0 to 1.0).
    pub fill_factor: f64,
    /// Number of distinct key values.
    pub distinct_keys: u64,
    /// Clustering factor: how well index order matches heap order.
    /// 1.0 = perfectly correlated, higher = more random I/O.
    pub clustering_factor: f64,
}

/// Cost factors used by the optimizer when evaluating index access paths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexCostFactors {
    /// Cost of a single random I/O page fetch.
    pub random_page_cost: f64,
    /// Cost of a single sequential I/O page fetch.
    pub sequential_page_cost: f64,
    /// Per-tuple CPU cost for index evaluation.
    pub cpu_index_tuple_cost: f64,
    /// Per-tuple CPU cost for heap fetch after index lookup.
    pub cpu_heap_tuple_cost: f64,
    /// Cost multiplier for this index type relative to a plain B-tree.
    /// B-tree = 1.0, Hash = 0.8, GIN = 1.5, `GiST` = 2.0, etc.
    pub type_multiplier: f64,
    /// Estimated cache hit ratio for this index (0.0 to 1.0).
    pub cache_hit_ratio: f64,
}

impl Default for IndexCostFactors {
    fn default() -> Self {
        Self {
            random_page_cost: 4.0,
            sequential_page_cost: 1.0,
            cpu_index_tuple_cost: 0.005,
            cpu_heap_tuple_cost: 0.01,
            type_multiplier: 1.0,
            cache_hit_ratio: 0.0,
        }
    }
}

impl IndexCostFactors {
    /// Cost factors tuned for `PostgreSQL` defaults.
    pub fn postgresql() -> Self {
        Self {
            random_page_cost: 4.0,
            sequential_page_cost: 1.0,
            cpu_index_tuple_cost: 0.005,
            cpu_heap_tuple_cost: 0.01,
            type_multiplier: 1.0,
            cache_hit_ratio: 0.0,
        }
    }

    /// Cost factors for hash index (cheaper lookups, no range scan).
    pub fn hash_index() -> Self {
        Self {
            type_multiplier: 0.8,
            ..Self::default()
        }
    }

    /// Cost factors for GIN index (higher maintenance, fast containment).
    pub fn gin_index() -> Self {
        Self {
            type_multiplier: 1.5,
            cpu_index_tuple_cost: 0.01,
            ..Self::default()
        }
    }

    /// Cost factors for `GiST` index (range/overlap queries).
    pub fn gist_index() -> Self {
        Self {
            type_multiplier: 2.0,
            cpu_index_tuple_cost: 0.02,
            ..Self::default()
        }
    }

    /// Cost factors for columnstore index (batch-oriented).
    pub fn columnstore_index() -> Self {
        Self {
            sequential_page_cost: 0.5,
            cpu_index_tuple_cost: 0.001,
            cpu_heap_tuple_cost: 0.0,
            type_multiplier: 0.3,
            ..Self::default()
        }
    }

    /// Cost factors for full-text index.
    pub fn fulltext_index() -> Self {
        Self {
            type_multiplier: 3.0,
            cpu_index_tuple_cost: 0.05,
            ..Self::default()
        }
    }

    /// Cost factors for spatial index.
    pub fn spatial_index() -> Self {
        Self {
            type_multiplier: 2.5,
            cpu_index_tuple_cost: 0.03,
            ..Self::default()
        }
    }
}

impl IndexMetadata {
    /// Estimate the cost of an index scan returning `selectivity` fraction
    /// of the table with `table_pages` total heap pages.
    pub fn estimate_scan_cost(
        &self,
        selectivity: f64,
        table_pages: u64,
        cost_factors: &IndexCostFactors,
    ) -> IndexScanCost {
        let matching_pages =
            (selectivity * self.leaf_pages as f64).max(1.0);
        let matching_heap_pages =
            (selectivity * table_pages as f64).max(1.0);

        let index_io = matching_pages
            * cost_factors.random_page_cost
            * cost_factors.type_multiplier
            * (1.0 - cost_factors.cache_hit_ratio);

        let heap_io = if self.is_index_only_possible() {
            0.0
        } else {
            let random_fraction =
                (self.clustering_factor / table_pages.max(1) as f64)
                    .min(1.0);
            let seq_io = matching_heap_pages
                * cost_factors.sequential_page_cost
                * (1.0 - random_fraction);
            let rand_io = matching_heap_pages
                * cost_factors.random_page_cost
                * random_fraction;
            (seq_io + rand_io)
                * (1.0 - cost_factors.cache_hit_ratio)
        };

        let matching_rows =
            selectivity * self.distinct_keys as f64;
        let cpu_cost = matching_rows
            * (cost_factors.cpu_index_tuple_cost
                + cost_factors.cpu_heap_tuple_cost);

        IndexScanCost {
            index_io_cost: index_io,
            heap_io_cost: heap_io,
            cpu_cost,
            total_cost: index_io + heap_io + cpu_cost,
        }
    }

    /// Whether this index can satisfy a query without heap access.
    fn is_index_only_possible(&self) -> bool {
        matches!(
            &self.index_type,
            IndexType::Clustered { .. }
                | IndexType::Columnstore { .. }
        )
    }

    /// Returns the columns that form the index key.
    pub fn key_columns(&self) -> Vec<&ColumnId> {
        match &self.index_type {
            IndexType::Clustered { columns }
            | IndexType::NonClustered { columns, .. }
            | IndexType::Composite { columns, .. }
            | IndexType::FullText { columns, .. }
            | IndexType::Unique { columns }
            | IndexType::Filtered { columns, .. }
            | IndexType::Columnstore { columns }
            | IndexType::Hash { columns } => {
                columns.iter().collect()
            }
            IndexType::Spatial { column, .. }
            | IndexType::Gin { column, .. }
            | IndexType::Gist { column, .. } => {
                vec![column]
            }
        }
    }

    /// Whether this index supports range scans.
    pub fn supports_range_scan(&self) -> bool {
        matches!(
            &self.index_type,
            IndexType::Clustered { .. }
                | IndexType::NonClustered { .. }
                | IndexType::Composite { .. }
                | IndexType::Unique { .. }
                | IndexType::Filtered { .. }
                | IndexType::Gist { .. }
        )
    }

    /// Whether this index supports exact-match (equality) lookups.
    pub fn supports_equality_lookup(&self) -> bool {
        !matches!(&self.index_type, IndexType::FullText { .. })
    }

    /// Whether this index supports ordering (ORDER BY elimination).
    pub fn supports_ordering(&self) -> bool {
        matches!(
            &self.index_type,
            IndexType::Clustered { .. }
                | IndexType::NonClustered { .. }
                | IndexType::Composite { .. }
                | IndexType::Unique { .. }
        )
    }

    /// Default cost factors for this index type.
    pub fn default_cost_factors(&self) -> IndexCostFactors {
        match &self.index_type {
            IndexType::Clustered { .. }
            | IndexType::NonClustered { .. }
            | IndexType::Composite { .. }
            | IndexType::Unique { .. }
            | IndexType::Filtered { .. } => {
                IndexCostFactors::default()
            }
            IndexType::Hash { .. } => {
                IndexCostFactors::hash_index()
            }
            IndexType::Gin { .. } => {
                IndexCostFactors::gin_index()
            }
            IndexType::Gist { .. } => {
                IndexCostFactors::gist_index()
            }
            IndexType::Columnstore { .. } => {
                IndexCostFactors::columnstore_index()
            }
            IndexType::FullText { .. } => {
                IndexCostFactors::fulltext_index()
            }
            IndexType::Spatial { .. } => {
                IndexCostFactors::spatial_index()
            }
        }
    }
}

/// Breakdown of index scan costs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexScanCost {
    /// I/O cost of reading index pages.
    pub index_io_cost: f64,
    /// I/O cost of fetching heap pages after index lookup.
    pub heap_io_cost: f64,
    /// CPU cost of processing index and heap tuples.
    pub cpu_cost: f64,
    /// Total estimated cost.
    pub total_cost: f64,
}

/// Recommendation from the index advisor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexRecommendation {
    /// Suggested index type.
    pub index_type: IndexType,
    /// Table to create the index on.
    pub table_name: String,
    /// Estimated improvement ratio (0.0 to 1.0).
    pub estimated_benefit: f64,
    /// Reason for the recommendation.
    pub reason: String,
}

/// Select the best index from candidates for a given query pattern.
pub fn select_best_index<'a>(
    candidates: &'a [IndexMetadata],
    required_columns: &[ColumnId],
    predicate_columns: &[ColumnId],
    needs_ordering: bool,
    selectivity: f64,
    table_pages: u64,
) -> Option<&'a IndexMetadata> {
    let mut best: Option<(&IndexMetadata, f64)> = None;

    for idx in candidates {
        if !idx.is_valid {
            continue;
        }

        let key_cols = idx.key_columns();
        let covers_predicate = predicate_columns
            .iter()
            .all(|pc| key_cols.contains(&pc));

        if !covers_predicate {
            continue;
        }

        if needs_ordering && !idx.supports_ordering() {
            continue;
        }

        let cost_factors = idx.default_cost_factors();
        let scan_cost = idx.estimate_scan_cost(
            selectivity,
            table_pages,
            &cost_factors,
        );

        let covers_projection = required_columns
            .iter()
            .all(|rc| key_cols.contains(&rc));

        let adjusted_cost = if covers_projection {
            scan_cost.total_cost * 0.5
        } else {
            scan_cost.total_cost
        };

        match &best {
            None => best = Some((idx, adjusted_cost)),
            Some((_, best_cost)) => {
                if adjusted_cost < *best_cost {
                    best = Some((idx, adjusted_cost));
                }
            }
        }
    }

    best.map(|(idx, _)| idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_btree_index(
        id: &str,
        columns: Vec<&str>,
    ) -> IndexMetadata {
        IndexMetadata {
            index_id: id.to_string(),
            table_name: "orders".to_string(),
            index_type: IndexType::NonClustered {
                columns: columns
                    .into_iter()
                    .map(String::from)
                    .collect(),
                included_columns: vec![],
            },
            is_primary: false,
            is_unique: false,
            is_valid: true,
            size_bytes: 10_000_000,
            leaf_pages: 1000,
            levels: 3,
            fill_factor: 0.9,
            distinct_keys: 100_000,
            clustering_factor: 5000.0,
        }
    }

    #[test]
    fn test_index_type_variants() {
        let clustered = IndexType::Clustered {
            columns: vec!["id".to_string()],
        };
        assert!(matches!(clustered, IndexType::Clustered { .. }));

        let hash = IndexType::Hash {
            columns: vec!["email".to_string()],
        };
        assert!(matches!(hash, IndexType::Hash { .. }));

        let gin = IndexType::Gin {
            column: "tags".to_string(),
            opclass: "jsonb_ops".to_string(),
        };
        assert!(matches!(gin, IndexType::Gin { .. }));
    }

    #[test]
    fn test_index_metadata_key_columns() {
        let idx = make_btree_index("idx1", vec!["a", "b"]);
        let keys = idx.key_columns();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], "a");
        assert_eq!(keys[1], "b");
    }

    #[test]
    fn test_supports_range_scan() {
        let btree = make_btree_index("idx1", vec!["a"]);
        assert!(btree.supports_range_scan());

        let hash_idx = IndexMetadata {
            index_type: IndexType::Hash {
                columns: vec!["a".to_string()],
            },
            ..make_btree_index("idx2", vec!["a"])
        };
        assert!(!hash_idx.supports_range_scan());
    }

    #[test]
    fn test_supports_equality() {
        let btree = make_btree_index("idx1", vec!["a"]);
        assert!(btree.supports_equality_lookup());

        let fulltext = IndexMetadata {
            index_type: IndexType::FullText {
                columns: vec!["body".to_string()],
                language: "english".to_string(),
                custom_stopwords: false,
            },
            ..make_btree_index("idx2", vec!["body"])
        };
        assert!(!fulltext.supports_equality_lookup());
    }

    #[test]
    fn test_default_cost_factors() {
        let btree = make_btree_index("idx1", vec!["a"]);
        let factors = btree.default_cost_factors();
        assert!((factors.type_multiplier - 1.0).abs() < f64::EPSILON);

        let gin_idx = IndexMetadata {
            index_type: IndexType::Gin {
                column: "tags".to_string(),
                opclass: "jsonb_ops".to_string(),
            },
            ..btree
        };
        let gin_factors = gin_idx.default_cost_factors();
        assert!((gin_factors.type_multiplier - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_estimate_scan_cost() {
        let idx = make_btree_index("idx1", vec!["a"]);
        let factors = IndexCostFactors::default();
        let cost = idx.estimate_scan_cost(0.01, 10_000, &factors);
        assert!(cost.total_cost > 0.0);
        assert!(cost.index_io_cost > 0.0);
        assert!(cost.heap_io_cost > 0.0);
        assert!(cost.cpu_cost > 0.0);
    }

    #[test]
    fn test_clustered_no_heap_io() {
        let idx = IndexMetadata {
            index_type: IndexType::Clustered {
                columns: vec!["id".to_string()],
            },
            ..make_btree_index("idx_clustered", vec!["id"])
        };
        let factors = IndexCostFactors::default();
        let cost = idx.estimate_scan_cost(0.01, 10_000, &factors);
        assert!(
            cost.heap_io_cost.abs() < f64::EPSILON,
            "Clustered index should have zero heap I/O"
        );
    }

    #[test]
    fn test_select_best_index_prefers_covering() {
        let idx_a = make_btree_index("idx_a", vec!["customer_id"]);
        let idx_b = IndexMetadata {
            index_id: "idx_b".to_string(),
            index_type: IndexType::NonClustered {
                columns: vec!["customer_id".to_string()],
                included_columns: vec![
                    "order_date".to_string(),
                ],
            },
            ..make_btree_index("idx_b", vec!["customer_id"])
        };

        let candidates = vec![idx_a, idx_b];
        let result = select_best_index(
            &candidates,
            &[
                "customer_id".to_string(),
                "order_date".to_string(),
            ],
            &["customer_id".to_string()],
            false,
            0.01,
            10_000,
        );
        assert!(result.is_some());
    }

    #[test]
    fn test_select_best_index_skips_invalid() {
        let mut idx = make_btree_index("idx1", vec!["a"]);
        idx.is_valid = false;

        let candidates = [idx];
        let result = select_best_index(
            &candidates,
            &["a".to_string()],
            &["a".to_string()],
            false,
            0.01,
            10_000,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_select_best_index_requires_ordering() {
        let hash_idx = IndexMetadata {
            index_type: IndexType::Hash {
                columns: vec!["a".to_string()],
            },
            ..make_btree_index("idx_hash", vec!["a"])
        };

        let candidates = [hash_idx];
        let result = select_best_index(
            &candidates,
            &["a".to_string()],
            &["a".to_string()],
            true,
            0.01,
            10_000,
        );
        assert!(
            result.is_none(),
            "Hash index cannot satisfy ordering"
        );
    }

    #[test]
    fn test_composite_index_sort_direction() {
        let idx = IndexType::Composite {
            columns: vec![
                "a".to_string(),
                "b".to_string(),
            ],
            column_order: vec![
                SortDirection::Ascending,
                SortDirection::Descending,
            ],
        };
        if let IndexType::Composite { column_order, .. } = &idx {
            assert_eq!(column_order[0], SortDirection::Ascending);
            assert_eq!(column_order[1], SortDirection::Descending);
        }
    }

    #[test]
    fn test_filtered_index() {
        let idx = IndexType::Filtered {
            columns: vec!["status".to_string()],
            filter_predicate: "status = 'active'".to_string(),
        };
        if let IndexType::Filtered {
            filter_predicate, ..
        } = &idx
        {
            assert_eq!(filter_predicate, "status = 'active'");
        }
    }

    #[test]
    fn test_spatial_index() {
        let idx = IndexType::Spatial {
            column: "geom".to_string(),
            srid: 4326,
        };
        if let IndexType::Spatial { srid, .. } = &idx {
            assert_eq!(*srid, 4326);
        }
    }
}
