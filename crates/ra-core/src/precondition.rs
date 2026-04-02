//! Formal pre-condition system for optimization rules.
//!
//! This module provides a declarative language for expressing rule pre-conditions
//! that can reference system facts (statistics, hardware, schema, runtime state).
//!
//! # Pre-Condition Categories
//!
//! - **Pattern Constraints**: Structural patterns that must (or must not) match
//! - **Predicates**: Boolean conditions on pattern variables
//! - **Facts**: Comparisons against system facts (statistics, hardware, schema)
//! - **Capabilities**: Database feature requirements
//!
//! # Example
//!
//! ```yaml
//! preconditions:
//!   - type: pattern
//!     must_match: "(filter ?pred (project ?cols ?rel))"
//!   - type: predicate
//!     condition: "pred_columns_subset_of(?pred, ?cols)"
//!   - type: fact
//!     fact_type: statistics.cardinality
//!     table: "?rel"
//!     comparator: ">"
//!     threshold: 10000
//!     optional: true
//! ```

use serde::{Deserialize, Serialize};

/// A pre-condition that must be satisfied for a rule to apply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PreCondition {
    /// Pattern matching constraint
    Pattern {
        /// Pattern that must match (egg s-expression)
        #[serde(skip_serializing_if = "Option::is_none")]
        must_match: Option<String>,
        /// Pattern that must not match
        #[serde(skip_serializing_if = "Option::is_none")]
        must_not_match: Option<String>,
        /// Human-readable description
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Whether this precondition is optional
        #[serde(default)]
        optional: bool,
    },

    /// Structural predicate evaluation
    Predicate {
        /// Predicate function call (e.g., `pred_columns_subset_of(?pred, ?cols)`)
        condition: String,
        /// Human-readable description
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Whether this precondition is optional
        #[serde(default)]
        optional: bool,
    },

    /// System fact check
    Fact {
        /// Fact type (e.g., "statistics.cardinality", "hardware.memory")
        fact_type: String,
        /// Table name or pattern variable (e.g., "?rel")
        #[serde(skip_serializing_if = "Option::is_none")]
        table: Option<String>,
        /// Column name or pattern variable (e.g., "?col")
        #[serde(skip_serializing_if = "Option::is_none")]
        column: Option<String>,
        /// Comparison operator (">", ">=", "<", "<=", "==", "!=", "contains")
        comparator: String,
        /// Threshold value to compare against
        threshold: FactValue,
        /// Minimum confidence level (for statistics)
        #[serde(skip_serializing_if = "Option::is_none")]
        confidence: Option<f64>,
        /// Human-readable description
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Whether this precondition is optional
        #[serde(default)]
        optional: bool,
    },

    /// Database capability requirement
    Capability {
        /// Database name ("current" means the target database)
        database: String,
        /// Feature name (e.g., `lateral_join`, `cte_recursive`, `bitmap_index`)
        requires: String,
        /// Human-readable description
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Whether this precondition is optional
        #[serde(default)]
        optional: bool,
    },

    /// Composite pre-condition (AND/OR/NOT)
    Composite {
        /// Logical operator
        operator: LogicalOperator,
        /// Sub-conditions
        conditions: Vec<PreCondition>,
        /// Human-readable description
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Whether this precondition is optional
        #[serde(default)]
        optional: bool,
    },
}

/// Logical operators for composite pre-conditions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogicalOperator {
    /// All conditions must be true
    And,
    /// At least one condition must be true
    Or,
    /// Negation (single condition)
    Not,
}

/// Value type for fact comparisons
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FactValue {
    /// Integer value
    Int(i64),
    /// Float value
    Float(f64),
    /// String value
    String(String),
    /// Boolean value
    Bool(bool),
    /// Expression (e.g., `hash_table_size(?left)`)
    Expression(String),
}

impl FactValue {
    /// Compare this value against another using the given operator
    ///
    /// # Errors
    ///
    /// Returns error if types are incompatible or operator is unknown
    #[expect(clippy::cast_precision_loss, reason = "i64 to f64 conversion needed for numeric comparisons; precision loss acceptable for threshold comparisons")]
    pub fn compare(&self, operator: &str, other: &Self) -> Result<bool, String> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => Self::compare_numeric(*a as f64, operator, *b as f64),
            (Self::Float(a), Self::Float(b)) => Self::compare_numeric(*a, operator, *b),
            (Self::Int(a), Self::Float(b)) => Self::compare_numeric(*a as f64, operator, *b),
            (Self::Float(a), Self::Int(b)) => Self::compare_numeric(*a, operator, *b as f64),
            (Self::String(a), Self::String(b)) => Self::compare_string(a, operator, b),
            (Self::Bool(a), Self::Bool(b)) => Self::compare_bool(*a, operator, *b),
            _ => Err(format!("Type mismatch: cannot compare {self:?} with {other:?}")),
        }
    }

    fn compare_numeric(a: f64, operator: &str, b: f64) -> Result<bool, String> {
        match operator {
            ">" => Ok(a > b),
            ">=" => Ok(a >= b),
            "<" => Ok(a < b),
            "<=" => Ok(a <= b),
            "==" | "=" => Ok((a - b).abs() < f64::EPSILON),
            "!=" => Ok((a - b).abs() >= f64::EPSILON),
            _ => Err(format!("Unknown numeric comparator: {operator}")),
        }
    }

    fn compare_string(a: &str, operator: &str, b: &str) -> Result<bool, String> {
        match operator {
            "==" | "=" => Ok(a == b),
            "!=" => Ok(a != b),
            "contains" => Ok(a.contains(b)),
            "starts_with" => Ok(a.starts_with(b)),
            "ends_with" => Ok(a.ends_with(b)),
            _ => Err(format!("Unknown string comparator: {operator}")),
        }
    }

    fn compare_bool(a: bool, operator: &str, b: bool) -> Result<bool, String> {
        match operator {
            "==" | "=" => Ok(a == b),
            "!=" => Ok(a != b),
            _ => Err(format!("Unknown boolean comparator: {operator}")),
        }
    }
}

/// Known fact types that can be referenced in pre-conditions
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactType {
    // Statistics facts
    /// Table cardinality (row count)
    StatisticsCardinality,
    /// Number of distinct values in a column
    StatisticsNdv,
    /// Predicate selectivity estimate
    StatisticsSelectivity,
    /// Fraction of null values in a column
    StatisticsNullFraction,
    /// Correlation between columns
    StatisticsCorrelation,

    // Hardware facts
    /// Available memory in bytes
    HardwareMemory,
    /// Number of CPU cores
    HardwareCpuCores,
    /// SIMD width in bits
    HardwareSimdWidth,
    /// GPU availability
    HardwareHasGpu,
    /// Cache size in bytes
    HardwareCacheSize,

    // Schema facts
    /// Column data type
    SchemaColumnType,
    /// Index existence check
    SchemaIndexExists,
    /// Primary key existence
    SchemaHasPrimaryKey,
    /// Foreign key constraints
    SchemaForeignKeys,
    /// Table size in bytes
    SchemaTableSize,

    // Runtime facts
    /// Cardinality estimation error
    RuntimeCardinalityError,
    /// Operator execution time
    RuntimeExecutionTime,
    /// Memory usage
    RuntimeMemoryUsage,
    /// Data skew detection
    RuntimeSkewDetected,

    // Database capabilities
    /// Feature support check
    DatabaseFeature,
    /// SQL dialect
    DatabaseDialect,
    /// Database version
    DatabaseVersion,
}

impl FactType {
    /// Parse a fact type from its string representation
    ///
    /// # Errors
    ///
    /// Returns error if the fact type string is not recognized
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "statistics.cardinality" => Ok(Self::StatisticsCardinality),
            "statistics.ndv" => Ok(Self::StatisticsNdv),
            "statistics.selectivity" => Ok(Self::StatisticsSelectivity),
            "statistics.null_fraction" => Ok(Self::StatisticsNullFraction),
            "statistics.correlation" => Ok(Self::StatisticsCorrelation),

            "hardware.memory" => Ok(Self::HardwareMemory),
            "hardware.cpu_cores" => Ok(Self::HardwareCpuCores),
            "hardware.simd_width" => Ok(Self::HardwareSimdWidth),
            "hardware.has_gpu" => Ok(Self::HardwareHasGpu),
            "hardware.cache_size" => Ok(Self::HardwareCacheSize),

            "schema.column_type" => Ok(Self::SchemaColumnType),
            "schema.index_exists" => Ok(Self::SchemaIndexExists),
            "schema.has_primary_key" => Ok(Self::SchemaHasPrimaryKey),
            "schema.foreign_keys" => Ok(Self::SchemaForeignKeys),
            "schema.table_size" => Ok(Self::SchemaTableSize),

            "runtime.cardinality_error" => Ok(Self::RuntimeCardinalityError),
            "runtime.execution_time" => Ok(Self::RuntimeExecutionTime),
            "runtime.memory_usage" => Ok(Self::RuntimeMemoryUsage),
            "runtime.skew_detected" => Ok(Self::RuntimeSkewDetected),

            "database.feature" => Ok(Self::DatabaseFeature),
            "database.dialect" => Ok(Self::DatabaseDialect),
            "database.version" => Ok(Self::DatabaseVersion),

            _ => Err(format!("Unknown fact type: {s}")),
        }
    }

    /// Return the string representation of this fact type
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::StatisticsCardinality => "statistics.cardinality",
            Self::StatisticsNdv => "statistics.ndv",
            Self::StatisticsSelectivity => "statistics.selectivity",
            Self::StatisticsNullFraction => "statistics.null_fraction",
            Self::StatisticsCorrelation => "statistics.correlation",

            Self::HardwareMemory => "hardware.memory",
            Self::HardwareCpuCores => "hardware.cpu_cores",
            Self::HardwareSimdWidth => "hardware.simd_width",
            Self::HardwareHasGpu => "hardware.has_gpu",
            Self::HardwareCacheSize => "hardware.cache_size",

            Self::SchemaColumnType => "schema.column_type",
            Self::SchemaIndexExists => "schema.index_exists",
            Self::SchemaHasPrimaryKey => "schema.has_primary_key",
            Self::SchemaForeignKeys => "schema.foreign_keys",
            Self::SchemaTableSize => "schema.table_size",

            Self::RuntimeCardinalityError => "runtime.cardinality_error",
            Self::RuntimeExecutionTime => "runtime.execution_time",
            Self::RuntimeMemoryUsage => "runtime.memory_usage",
            Self::RuntimeSkewDetected => "runtime.skew_detected",

            Self::DatabaseFeature => "database.feature",
            Self::DatabaseDialect => "database.dialect",
            Self::DatabaseVersion => "database.version",
        }
    }
}

/// Result of evaluating a pre-condition
#[derive(Debug, Clone, PartialEq)]
pub enum EvaluationResult {
    /// All preconditions satisfied
    Satisfied,
    /// A required precondition was not satisfied
    NotSatisfied {
        /// The precondition that failed
        condition: PreCondition,
        /// Reason for failure
        reason: String,
    },
    /// Error during evaluation
    Error {
        /// The precondition that caused the error
        condition: PreCondition,
        /// Error message
        error: String,
    },
}

impl EvaluationResult {
    /// Check if the result is satisfied
    #[must_use]
    pub fn is_satisfied(&self) -> bool {
        matches!(self, Self::Satisfied)
    }

    /// Check if the result is an error
    #[must_use]
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }
}

/// Helper for building pre-conditions
pub struct PreConditionBuilder {
    conditions: Vec<PreCondition>,
}

impl PreConditionBuilder {
    /// Create a new builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
        }
    }

    /// Add a pattern constraint
    #[must_use]
    pub fn pattern(mut self, pattern: impl Into<String>) -> Self {
        self.conditions.push(PreCondition::Pattern {
            must_match: Some(pattern.into()),
            must_not_match: None,
            description: None,
            optional: false,
        });
        self
    }

    /// Add a predicate constraint
    #[must_use]
    pub fn predicate(mut self, condition: impl Into<String>) -> Self {
        self.conditions.push(PreCondition::Predicate {
            condition: condition.into(),
            description: None,
            optional: false,
        });
        self
    }

    /// Add a fact constraint
    #[must_use]
    pub fn fact(
        mut self,
        fact_type: impl Into<String>,
        comparator: impl Into<String>,
        threshold: FactValue,
    ) -> Self {
        self.conditions.push(PreCondition::Fact {
            fact_type: fact_type.into(),
            table: None,
            column: None,
            comparator: comparator.into(),
            threshold,
            confidence: None,
            description: None,
            optional: false,
        });
        self
    }

    /// Add a capability constraint
    #[must_use]
    pub fn capability(mut self, database: impl Into<String>, feature: impl Into<String>) -> Self {
        self.conditions.push(PreCondition::Capability {
            database: database.into(),
            requires: feature.into(),
            description: None,
            optional: false,
        });
        self
    }

    /// Build the preconditions list
    #[must_use]
    pub fn build(self) -> Vec<PreCondition> {
        self.conditions
    }
}

impl Default for PreConditionBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code uses unwrap for assertions")]
mod tests {
    use super::*;

    #[test]
    fn fact_value_compare_numeric() {
        let a = FactValue::Int(100);
        let b = FactValue::Int(50);

        assert!(a.compare(">", &b).unwrap());
        assert!(a.compare(">=", &b).unwrap());
        assert!(!a.compare("<", &b).unwrap());
        assert!(!a.compare("<=", &b).unwrap());
        assert!(!a.compare("==", &b).unwrap());
        assert!(a.compare("!=", &b).unwrap());
    }

    #[test]
    fn fact_value_compare_string() {
        let a = FactValue::String("postgresql".into());
        let b = FactValue::String("post".into());

        assert!(a.compare("contains", &b).unwrap());
        assert!(a.compare("starts_with", &b).unwrap());
        assert!(!a.compare("==", &b).unwrap());
    }

    #[test]
    fn fact_type_parse() {
        assert_eq!(
            FactType::parse("statistics.cardinality").unwrap(),
            FactType::StatisticsCardinality
        );
        assert_eq!(
            FactType::parse("hardware.memory").unwrap(),
            FactType::HardwareMemory
        );
        assert!(FactType::parse("invalid.fact").is_err());
    }

    #[test]
    fn precondition_builder() {
        let preconditions = PreConditionBuilder::new()
            .pattern("(filter ?pred (project ?cols ?rel))")
            .predicate("pred_columns_subset_of(?pred, ?cols)")
            .fact("statistics.cardinality", ">", FactValue::Int(10000))
            .capability("current", "lateral_join")
            .build();

        assert_eq!(preconditions.len(), 4);
    }

    #[test]
    fn precondition_serialization() {
        let precond = PreCondition::Fact {
            fact_type: "statistics.cardinality".into(),
            table: Some("?rel".into()),
            column: None,
            comparator: ">".into(),
            threshold: FactValue::Int(10000),
            confidence: Some(0.8),
            description: Some("Table must be large".into()),
            optional: false,
        };

        let yaml = serde_yaml::to_string(&precond).unwrap();
        let deserialized: PreCondition = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(precond, deserialized);
    }
}
