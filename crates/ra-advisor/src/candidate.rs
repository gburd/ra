//! Index candidate generation and representation

use ra_core::Expr;
use serde::{Deserialize, Serialize};

/// An index candidate that could be created
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IndexCandidate {
    /// Table name
    pub table: String,
    /// Ordered list of columns in the index
    pub columns: Vec<String>,
    /// Type of index (BTree, Hash, GIN, etc.)
    pub index_type: IndexType,
    /// Whether this is a unique index
    pub unique: bool,
    /// Optional partial index predicate
    pub partial_predicate: Option<Expr>,
}

/// Type of index structure
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IndexType {
    /// B-Tree index (default, good for range queries and equality)
    BTree,
    /// Hash index (equality only, faster for point lookups)
    Hash,
    /// GIN index (generalized inverted index, for full-text search and arrays)
    GIN,
    /// GiST index (generalized search tree, for geometric data)
    GiST,
    /// BRIN index (block range index, for large sorted tables)
    BRIN,
}

impl Default for IndexType {
    fn default() -> Self {
        Self::BTree
    }
}

impl IndexCandidate {
    /// Create a simple single-column B-Tree index
    pub fn simple(table: impl Into<String>, column: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            columns: vec![column.into()],
            index_type: IndexType::BTree,
            unique: false,
            partial_predicate: None,
        }
    }

    /// Create a composite B-Tree index
    pub fn composite(table: impl Into<String>, columns: Vec<String>) -> Self {
        Self {
            table: table.into(),
            columns,
            index_type: IndexType::BTree,
            unique: false,
            partial_predicate: None,
        }
    }

    /// Generate index name
    pub fn index_name(&self) -> String {
        format!("idx_{}_{}", self.table, self.columns.join("_"))
    }

    /// Generate CREATE INDEX SQL statement
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

        // Index method
        match self.index_type {
            IndexType::BTree => {} // Default, no need to specify
            IndexType::Hash => sql.push_str(" USING HASH"),
            IndexType::GIN => sql.push_str(" USING GIN"),
            IndexType::GiST => sql.push_str(" USING GIST"),
            IndexType::BRIN => sql.push_str(" USING BRIN"),
        }

        // Columns
        sql.push('(');
        sql.push_str(&self.columns.join(", "));
        sql.push(')');

        // Partial index predicate
        if let Some(predicate) = &self.partial_predicate {
            sql.push_str(" WHERE ");
            sql.push_str(&predicate.to_string());
        }

        sql.push(';');
        sql
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_index() {
        let idx = IndexCandidate::simple("users", "email");
        assert_eq!(idx.table, "users");
        assert_eq!(idx.columns, vec!["email"]);
        assert_eq!(idx.index_name(), "idx_users_email");
        assert_eq!(idx.to_sql(), "CREATE INDEX idx_users_email ON users(email);");
    }

    #[test]
    fn test_composite_index() {
        let idx = IndexCandidate::composite("orders", vec!["user_id".into(), "created_at".into()]);
        assert_eq!(idx.index_name(), "idx_orders_user_id_created_at");
        assert_eq!(idx.to_sql(), "CREATE INDEX idx_orders_user_id_created_at ON orders(user_id, created_at);");
    }

    #[test]
    fn test_unique_index() {
        let mut idx = IndexCandidate::simple("users", "username");
        idx.unique = true;
        assert_eq!(idx.to_sql(), "CREATE UNIQUE INDEX idx_users_username ON users(username);");
    }

    #[test]
    fn test_hash_index() {
        let mut idx = IndexCandidate::simple("products", "sku");
        idx.index_type = IndexType::Hash;
        assert_eq!(idx.to_sql(), "CREATE INDEX idx_products_sku ON products USING HASH(sku);");
    }
}