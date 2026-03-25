//! Index candidate generation and representation.

use serde::{Deserialize, Serialize};

/// An index candidate that could be created.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexCandidate {
    /// Table name.
    pub table: String,
    /// Ordered list of columns in the index.
    pub columns: Vec<String>,
    /// Type of index (`BTree`, `BRIN`, `GIN`, etc.).
    pub index_type: IndexType,
    /// Whether this is a unique index.
    pub unique: bool,
    /// Optional partial index predicate (stored as SQL string).
    pub partial_predicate: Option<String>,
    /// Reason this index type was chosen (for recommendations).
    pub reason: Option<String>,
}

/// Type of index structure.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash,
    Serialize, Deserialize,
)]
pub enum IndexType {
    /// B-Tree index (default, good for range queries and equality).
    #[default]
    BTree,
    /// Hash index (equality only, faster for point lookups).
    Hash,
    /// GIN index (generalized inverted index, for full-text search
    /// and arrays).
    GIN,
    /// `GiST` index (generalized search tree, for geometric data).
    GiST,
    /// BRIN index (block range index, for large sorted tables).
    BRIN,
}

impl std::fmt::Display for IndexType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BTree => write!(f, "B-Tree"),
            Self::Hash => write!(f, "Hash"),
            Self::GIN => write!(f, "GIN"),
            Self::GiST => write!(f, "GiST"),
            Self::BRIN => write!(f, "BRIN"),
        }
    }
}

impl IndexCandidate {
    /// Create a simple single-column B-Tree index.
    pub fn btree(
        table: impl Into<String>,
        column: impl Into<String>,
    ) -> Self {
        Self {
            table: table.into(),
            columns: vec![column.into()],
            index_type: IndexType::BTree,
            unique: false,
            partial_predicate: None,
            reason: None,
        }
    }

    /// Create a BRIN index candidate for a correlated column.
    pub fn brin(
        table: impl Into<String>,
        column: impl Into<String>,
        correlation: f64,
    ) -> Self {
        Self {
            table: table.into(),
            columns: vec![column.into()],
            index_type: IndexType::BRIN,
            unique: false,
            partial_predicate: None,
            reason: Some(format!(
                "Column has high physical correlation ({correlation:.3}); \
                 BRIN is 100-1000x smaller than B-tree for correlated data"
            )),
        }
    }

    /// Create a composite B-Tree index.
    pub fn composite(
        table: impl Into<String>,
        columns: Vec<String>,
    ) -> Self {
        Self {
            table: table.into(),
            columns,
            index_type: IndexType::BTree,
            unique: false,
            partial_predicate: None,
            reason: None,
        }
    }

    /// Generate index name.
    #[must_use]
    pub fn index_name(&self) -> String {
        let suffix = match self.index_type {
            IndexType::BRIN => "_brin",
            IndexType::Hash => "_hash",
            IndexType::GIN => "_gin",
            IndexType::GiST => "_gist",
            IndexType::BTree => "",
        };
        format!(
            "idx_{}_{}{}",
            self.table,
            self.columns.join("_"),
            suffix
        )
    }

    /// Generate CREATE INDEX SQL statement.
    #[must_use]
    pub fn to_sql(&self) -> String {
        let mut sql = String::new();

        sql.push_str("CREATE ");
        if self.unique {
            sql.push_str("UNIQUE ");
        }
        sql.push_str("INDEX ");
        sql.push_str(&self.index_name());
        sql.push_str(" ON ");
        sql.push_str(&self.table);

        match self.index_type {
            IndexType::BTree => {}
            IndexType::Hash => sql.push_str(" USING HASH"),
            IndexType::GIN => sql.push_str(" USING GIN"),
            IndexType::GiST => sql.push_str(" USING GIST"),
            IndexType::BRIN => sql.push_str(" USING BRIN"),
        }

        sql.push('(');
        sql.push_str(&self.columns.join(", "));
        sql.push(')');

        if let Some(predicate) = &self.partial_predicate {
            sql.push_str(" WHERE ");
            sql.push_str(predicate);
        }

        sql.push(';');
        sql
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn simple_btree_index() {
        let idx = IndexCandidate::btree("users", "email");
        assert_eq!(idx.table, "users");
        assert_eq!(idx.columns, vec!["email"]);
        assert_eq!(idx.index_name(), "idx_users_email");
        assert_eq!(
            idx.to_sql(),
            "CREATE INDEX idx_users_email ON users(email);"
        );
    }

    #[test]
    fn composite_index() {
        let idx = IndexCandidate::composite(
            "orders",
            vec!["user_id".into(), "created_at".into()],
        );
        assert_eq!(idx.index_name(), "idx_orders_user_id_created_at");
        assert_eq!(
            idx.to_sql(),
            "CREATE INDEX idx_orders_user_id_created_at \
             ON orders(user_id, created_at);"
        );
    }

    #[test]
    fn unique_index() {
        let mut idx = IndexCandidate::btree("users", "username");
        idx.unique = true;
        assert_eq!(
            idx.to_sql(),
            "CREATE UNIQUE INDEX idx_users_username ON users(username);"
        );
    }

    #[test]
    fn hash_index() {
        let mut idx = IndexCandidate::btree("products", "sku");
        idx.index_type = IndexType::Hash;
        assert_eq!(
            idx.to_sql(),
            "CREATE INDEX idx_products_sku_hash \
             ON products USING HASH(sku);"
        );
    }

    #[test]
    fn brin_index() {
        let idx = IndexCandidate::brin("events", "created_at", 0.98);
        assert_eq!(idx.index_type, IndexType::BRIN);
        assert_eq!(idx.index_name(), "idx_events_created_at_brin");
        assert_eq!(
            idx.to_sql(),
            "CREATE INDEX idx_events_created_at_brin \
             ON events USING BRIN(created_at);"
        );
        assert!(idx.reason.is_some());
        let reason = idx.reason.as_deref().unwrap_or("");
        assert!(reason.contains("0.980"));
    }

    #[test]
    fn brin_index_name_distinct_from_btree() {
        let btree = IndexCandidate::btree("events", "ts");
        let brin = IndexCandidate::brin("events", "ts", 0.95);
        assert_ne!(btree.index_name(), brin.index_name());
    }

    #[test]
    fn index_type_display() {
        assert_eq!(IndexType::BTree.to_string(), "B-Tree");
        assert_eq!(IndexType::BRIN.to_string(), "BRIN");
        assert_eq!(IndexType::GIN.to_string(), "GIN");
    }

    #[test]
    fn partial_index_sql() {
        let mut idx = IndexCandidate::btree("orders", "customer_id");
        idx.partial_predicate =
            Some("status = 'active'".to_string());
        assert_eq!(
            idx.to_sql(),
            "CREATE INDEX idx_orders_customer_id \
             ON orders(customer_id) WHERE status = 'active';"
        );
    }
}
