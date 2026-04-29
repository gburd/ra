//! Grammar extension trait for vendor-specific and extension-specific syntax.

use sqlparser::ast::Statement;
use std::error::Error;

/// Trait for grammar extensions that add vendor-specific or extension-specific syntax.
///
/// Extensions can provide additional keywords, operators, functions, and
/// statement types that are not part of standard SQL.
///
/// # Examples
///
/// ```ignore
/// use ra_parser::grammar::GrammarExtension;
///
/// struct PostGISExtension;
///
/// impl GrammarExtension for PostGISExtension {
///     fn name(&self) -> &str {
///         "postgis"
///     }
///
///     fn keywords(&self) -> Vec<&str> {
///         vec!["GEOMETRY", "GEOGRAPHY", "POINT", "LINESTRING", "POLYGON"]
///     }
///
///     fn operators(&self) -> Vec<&str> {
///         vec!["&&", "&<", "&>", "<<", ">>", "@", "~", "~="]
///     }
///
///     fn functions(&self) -> Vec<&str> {
///         vec!["ST_Contains", "ST_Intersects", "ST_Distance", "ST_DWithin"]
///     }
/// }
/// ```
pub trait GrammarExtension: Send + Sync {
    /// Return the name of this extension.
    fn name(&self) -> &'static str;

    /// Return additional keywords introduced by this extension.
    fn keywords(&self) -> Vec<&str> {
        Vec::new()
    }

    /// Return additional operators introduced by this extension.
    fn operators(&self) -> Vec<&str> {
        Vec::new()
    }

    /// Return additional functions introduced by this extension.
    fn functions(&self) -> Vec<&str> {
        Vec::new()
    }

    /// Parse extension-specific statement syntax.
    ///
    /// Returns `None` if this extension doesn't recognize the statement,
    /// allowing other extensions to try parsing.
    ///
    /// # Errors
    ///
    /// Returns an error if the SQL is syntactically invalid for this extension.
    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        Ok(None)
    }

    /// Documentation URL for this extension (if available).
    fn documentation_url(&self) -> Option<&str> {
        None
    }

    /// Minimum version required for this extension (if applicable).
    fn min_version(&self) -> Option<&str> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestExtension;

    impl GrammarExtension for TestExtension {
        fn name(&self) -> &'static str {
            "test"
        }

        fn keywords(&self) -> Vec<&str> {
            vec!["TEST_KEYWORD"]
        }
    }

    #[test]
    fn test_extension_trait() {
        let ext = TestExtension;
        assert_eq!(ext.name(), "test");
        assert_eq!(ext.keywords(), vec!["TEST_KEYWORD"]);
        assert!(ext.operators().is_empty());
        assert!(ext.functions().is_empty());
    }
}
