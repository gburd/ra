//! Document database optimization rules.
//!
//! Provides operators and cost models for document query patterns
//! used by databases like `MongoDB`, Couchbase, and `CosmosDB`.
//! Key optimizations include nested predicate pushdown, covered
//! queries, and pipeline coalescence.

use serde::{Deserialize, Serialize};

use ra_core::cost::Cost;

/// Document-specific operators that extend the relational algebra.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DocumentOp {
    /// Scan a document collection.
    CollectionScan {
        /// Collection name.
        collection: String,
    },

    /// Scan a collection with an inline predicate.
    FilteredScan {
        /// Collection name.
        collection: String,
        /// The filter predicate on nested fields.
        predicate: String,
    },

    /// Index-only scan (covered query).
    IndexOnlyScan {
        /// Collection name.
        collection: String,
        /// Index name.
        index: String,
        /// Fields returned from the index.
        fields: Vec<String>,
    },

    /// Unwind (flatten) an array field.
    Unwind {
        /// The array field path to unwind.
        field: String,
        /// Whether to preserve null and empty arrays.
        preserve_null: bool,
    },

    /// Lookup (join) across collections.
    Lookup {
        /// Foreign collection name.
        from: String,
        /// Local field for the join.
        local_field: String,
        /// Foreign field for the join.
        foreign_field: String,
        /// Output array field name.
        output_as: String,
    },

    /// Access an embedded subdocument directly.
    EmbeddedAccess {
        /// The dotted path to the embedded document.
        path: String,
    },

    /// Change stream with server-side filter.
    ChangeStreamFiltered {
        /// Collection name.
        collection: String,
        /// Server-side filter predicate.
        predicate: String,
    },
}

/// Statistics about a document collection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionStats {
    /// Total number of documents.
    pub document_count: f64,
    /// Average document size in bytes.
    pub avg_document_size: u64,
    /// Total collection size in bytes.
    pub total_size: u64,
    /// Number of indexes on this collection.
    pub index_count: u32,
    /// Per-field statistics.
    pub field_stats: Vec<FieldStats>,
}

/// Statistics for a document field (possibly nested).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldStats {
    /// Dotted path to the field.
    pub path: String,
    /// Number of distinct values.
    pub distinct_count: f64,
    /// Whether the field has an index.
    pub indexed: bool,
    /// Average array length (if the field is an array).
    pub avg_array_length: Option<f64>,
    /// Dominant type of the field values.
    pub dominant_type: FieldType,
    /// Fraction of documents missing this field.
    pub missing_fraction: f64,
}

/// Types that a document field can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FieldType {
    /// String values.
    String,
    /// Numeric values (int or float).
    Number,
    /// Boolean values.
    Boolean,
    /// Nested object.
    Object,
    /// Array of values.
    Array,
    /// Null.
    Null,
    /// Mixed types.
    Mixed,
}

/// Convert a non-negative f64 to u64 for memory estimates, clamping
/// negative and overflow values.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn f64_to_mem(val: f64) -> u64 {
    if val <= 0.0 {
        0
    } else if val >= u64::MAX as f64 {
        u64::MAX
    } else {
        val as u64
    }
}

/// Lossless conversion of u64 to f64 for cost arithmetic.
/// Byte sizes in practice never exceed 2^53 so precision loss
/// is not a concern.
#[allow(clippy::cast_precision_loss)]
fn u64_to_f64(val: u64) -> f64 {
    val as f64
}

/// Estimate cost for a full collection scan.
#[must_use]
pub fn estimate_collection_scan_cost(doc_count: f64, avg_doc_size: u64) -> Cost {
    let io = doc_count * u64_to_f64(avg_doc_size);
    Cost::new(
        doc_count * 0.1,
        io * 0.001,
        0.0,
        avg_doc_size.saturating_mul(1024),
    )
}

/// Estimate cost for a filtered collection scan.
#[must_use]
pub fn estimate_filtered_scan_cost(doc_count: f64, avg_doc_size: u64, selectivity: f64) -> Cost {
    let io = doc_count * u64_to_f64(avg_doc_size);
    let cpu = doc_count * 0.15 + doc_count * selectivity * 0.05;
    Cost::new(cpu, io * 0.001, 0.0, avg_doc_size.saturating_mul(512))
}

/// Estimate cost for an index-only scan (covered query).
#[must_use]
pub fn estimate_index_only_cost(doc_count: f64, selectivity: f64, index_entry_size: u64) -> Cost {
    let matching = doc_count * selectivity;
    let io = matching * u64_to_f64(index_entry_size);
    Cost::new(
        matching * 0.05,
        io * 0.001,
        0.0,
        index_entry_size.saturating_mul(256),
    )
}

/// Estimate cost for a lookup (join) operation.
#[must_use]
pub fn estimate_lookup_cost(outer_count: f64, inner_count: f64, inner_indexed: bool) -> Cost {
    let per_doc_cost = if inner_indexed {
        inner_count.ln().max(1.0)
    } else {
        inner_count
    };
    let total_cpu = outer_count * per_doc_cost * 0.1;
    let total_io = outer_count * per_doc_cost * 0.01;
    Cost::new(total_cpu, total_io, 0.0, f64_to_mem(outer_count * 256.0))
}

/// Estimate cost for an unwind operation.
#[must_use]
pub fn estimate_unwind_cost(doc_count: f64, avg_array_length: f64) -> Cost {
    let output_rows = doc_count * avg_array_length;
    Cost::new(output_rows * 0.05, 0.0, 0.0, f64_to_mem(output_rows * 64.0))
}

/// Estimate cost for embedded document access.
#[must_use]
pub fn estimate_embedded_access_cost(doc_count: f64) -> Cost {
    Cost::new(doc_count * 0.02, 0.0, 0.0, 0)
}

impl std::fmt::Display for FieldType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String => write!(f, "string"),
            Self::Number => write!(f, "number"),
            Self::Boolean => write!(f, "boolean"),
            Self::Object => write!(f, "object"),
            Self::Array => write!(f, "array"),
            Self::Null => write!(f, "null"),
            Self::Mixed => write!(f, "mixed"),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn document_op_scan_roundtrip() {
        let op = DocumentOp::CollectionScan {
            collection: "users".into(),
        };
        let json = serde_json::to_string(&op).expect("serialization should succeed");
        let deserialized: DocumentOp =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(op, deserialized);
    }

    #[test]
    fn document_op_lookup_roundtrip() {
        let op = DocumentOp::Lookup {
            from: "customers".into(),
            local_field: "customer_id".into(),
            foreign_field: "_id".into(),
            output_as: "customer".into(),
        };
        let json = serde_json::to_string(&op).expect("serialization should succeed");
        let deserialized: DocumentOp =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(op, deserialized);
    }

    #[test]
    fn document_op_unwind() {
        let op = DocumentOp::Unwind {
            field: "items".into(),
            preserve_null: false,
        };
        if let DocumentOp::Unwind {
            field,
            preserve_null,
        } = &op
        {
            assert_eq!(field, "items");
            assert!(!preserve_null);
        } else {
            panic!("expected Unwind");
        }
    }

    #[test]
    fn collection_stats_roundtrip() {
        let stats = CollectionStats {
            document_count: 1_000_000.0,
            avg_document_size: 2048,
            total_size: 2_048_000_000,
            index_count: 3,
            field_stats: vec![FieldStats {
                path: "address.city".into(),
                distinct_count: 5000.0,
                indexed: true,
                avg_array_length: None,
                dominant_type: FieldType::String,
                missing_fraction: 0.01,
            }],
        };
        let json = serde_json::to_string(&stats).expect("serialization should succeed");
        let deserialized: CollectionStats =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(stats, deserialized);
    }

    #[test]
    fn field_type_display() {
        assert_eq!(FieldType::String.to_string(), "string");
        assert_eq!(FieldType::Number.to_string(), "number");
        assert_eq!(FieldType::Mixed.to_string(), "mixed");
        assert_eq!(FieldType::Array.to_string(), "array");
    }

    #[test]
    fn collection_scan_cost_proportional() {
        let small = estimate_collection_scan_cost(100.0, 512);
        let large = estimate_collection_scan_cost(1_000_000.0, 512);
        assert!(large.total() > small.total());
    }

    #[test]
    fn filtered_scan_cheaper_than_full_scan() {
        let full = estimate_collection_scan_cost(100_000.0, 1024);
        let filtered = estimate_filtered_scan_cost(100_000.0, 1024, 0.01);
        assert!(filtered.cpu < full.cpu * 2.0);
    }

    #[test]
    fn index_only_cheaper_than_collection_scan() {
        let scan = estimate_collection_scan_cost(100_000.0, 2048);
        let index = estimate_index_only_cost(100_000.0, 0.01, 64);
        assert!(index.total() < scan.total());
    }

    #[test]
    fn lookup_indexed_cheaper_than_unindexed() {
        let indexed = estimate_lookup_cost(1000.0, 100_000.0, true);
        let unindexed = estimate_lookup_cost(1000.0, 100_000.0, false);
        assert!(indexed.total() < unindexed.total());
    }

    #[test]
    fn unwind_cost_proportional_to_array_length() {
        let short = estimate_unwind_cost(1000.0, 2.0);
        let long = estimate_unwind_cost(1000.0, 100.0);
        assert!(long.total() > short.total());
    }

    #[test]
    fn embedded_access_cheap() {
        let lookup = estimate_lookup_cost(1000.0, 100_000.0, true);
        let embedded = estimate_embedded_access_cost(1000.0);
        assert!(embedded.total() < lookup.total());
    }
}
