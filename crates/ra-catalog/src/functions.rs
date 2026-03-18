//! Function catalog for SQL query optimization.
//!
//! Every function known to the optimizer is registered here with its
//! signatures, behavioral properties, and cost metadata.  The optimizer
//! uses this information to:
//!
//! - Constant-fold pure, deterministic expressions at plan time.
//! - Decide whether to push expensive functions below or above joins.
//! - Match function calls to expression indexes.
//! - Estimate per-row evaluation cost via `cost_multiplier`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// SQL data types used in function signatures.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataType {
    /// Arbitrary-precision or machine-width integer.
    Integer,
    /// IEEE-754 double-precision floating point.
    Float,
    /// Fixed-precision decimal (e.g. NUMERIC(p,s)).
    Decimal,
    /// Variable-length character string.
    Text,
    /// Boolean (TRUE / FALSE / NULL).
    Boolean,
    /// Calendar date without time.
    Date,
    /// Timestamp with or without time zone.
    Timestamp,
    /// Time interval.
    Interval,
    /// Binary large object.
    Blob,
    /// JSON / JSONB.
    Json,
    /// Typed array (e.g. INTEGER[]).
    Array(Box<DataType>),
    /// Geometry / geography (PostGIS).
    Geometry,
    /// Full-text search vector (PostgreSQL tsvector).
    TsVector,
    /// Full-text search query (PostgreSQL tsquery).
    TsQuery,
    /// Accepts any type (polymorphic argument).
    Any,
}

/// Broad classification of SQL functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FunctionCategory {
    /// Row-level scalar function (e.g. ABS, UPPER).
    Scalar,
    /// Aggregate function consuming a group (e.g. SUM, COUNT).
    Aggregate,
    /// Window function evaluated over a frame (e.g. ROW_NUMBER).
    Window,
    /// Table-valued function returning a set of rows.
    TableValued,
}

/// One overload of a function (argument types -> return type).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionSignature {
    /// Positional argument types.
    pub args: Vec<DataType>,
    /// Return type.
    pub return_type: DataType,
    /// Whether the last argument repeats (e.g. COALESCE).
    pub variadic: bool,
}

/// Behavioral and cost properties attached to a function.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionProperties {
    /// Same inputs always produce the same output (no hidden state).
    pub deterministic: bool,
    /// The function body can be inlined into the calling expression.
    pub inlineable: bool,
    /// The function is computationally expensive relative to simple
    /// arithmetic (affects join ordering and pushdown decisions).
    pub expensive: bool,
    /// No observable side effects and no dependency on external state.
    pub pure: bool,
    /// Can be evaluated once at plan time when all arguments are constants.
    pub constant_foldable: bool,
    /// Multiplicative cost factor relative to a baseline scalar comparison.
    /// 1.0 = same cost as `=`, 10.0 = ten times more expensive.
    pub cost_multiplier: f64,
}

impl FunctionProperties {
    /// Properties for a cheap, pure, deterministic scalar function.
    pub fn cheap_pure() -> Self {
        Self {
            deterministic: true,
            inlineable: true,
            expensive: false,
            pure: true,
            constant_foldable: true,
            cost_multiplier: 1.0,
        }
    }

    /// Properties for a non-deterministic function like RANDOM().
    pub fn non_deterministic() -> Self {
        Self {
            deterministic: false,
            inlineable: false,
            expensive: false,
            pure: false,
            constant_foldable: false,
            cost_multiplier: 1.0,
        }
    }

    /// Properties for an expensive function (e.g. regex, geo distance).
    pub fn expensive_pure() -> Self {
        Self {
            deterministic: true,
            inlineable: false,
            expensive: true,
            pure: true,
            constant_foldable: true,
            cost_multiplier: 10.0,
        }
    }

    /// Properties for a standard aggregate function.
    pub fn aggregate() -> Self {
        Self {
            deterministic: true,
            inlineable: false,
            expensive: false,
            pure: true,
            constant_foldable: false,
            cost_multiplier: 1.0,
        }
    }

    /// Properties for a window function.
    pub fn window() -> Self {
        Self {
            deterministic: true,
            inlineable: false,
            expensive: false,
            pure: true,
            constant_foldable: false,
            cost_multiplier: 2.0,
        }
    }
}

/// Complete definition of a catalog function.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionDefinition {
    /// Canonical upper-case name (e.g. "ABS", "ST_DISTANCE").
    pub name: String,
    /// Category (scalar / aggregate / window / table-valued).
    pub category: FunctionCategory,
    /// One or more overloaded signatures.
    pub signatures: Vec<FunctionSignature>,
    /// Behavioral and cost properties.
    pub properties: FunctionProperties,
}

/// In-memory function catalog backed by a name -> definition map.
#[derive(Debug, Clone)]
pub struct FunctionCatalog {
    functions: HashMap<String, FunctionDefinition>,
}

impl FunctionCatalog {
    /// Build an empty catalog.
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    /// Register a function definition.
    pub fn register(&mut self, def: FunctionDefinition) {
        self.functions.insert(def.name.to_uppercase(), def);
    }

    /// Look up a function by name (case-insensitive).
    pub fn lookup(&self, name: &str) -> Option<&FunctionDefinition> {
        self.functions.get(&name.to_uppercase())
    }

    /// Number of registered functions.
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Whether the catalog is empty.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// Iterate over all function definitions.
    pub fn iter(&self) -> impl Iterator<Item = &FunctionDefinition> {
        self.functions.values()
    }

    /// All functions in a given category.
    pub fn by_category(
        &self,
        cat: FunctionCategory,
    ) -> Vec<&FunctionDefinition> {
        self.functions
            .values()
            .filter(|f| f.category == cat)
            .collect()
    }

    /// All functions that are constant-foldable.
    pub fn constant_foldable(&self) -> Vec<&FunctionDefinition> {
        self.functions
            .values()
            .filter(|f| f.properties.constant_foldable)
            .collect()
    }

    /// All functions marked as expensive.
    pub fn expensive_functions(&self) -> Vec<&FunctionDefinition> {
        self.functions
            .values()
            .filter(|f| f.properties.expensive)
            .collect()
    }
}

impl Default for FunctionCatalog {
    fn default() -> Self {
        Self::new()
    }
}

/// Intermediate TOML representation for deserialization.
#[derive(Debug, Deserialize)]
pub struct FunctionToml {
    /// List of function entries.
    pub function: Vec<FunctionEntry>,
}

/// One entry in the TOML function catalog file.
#[derive(Debug, Deserialize)]
pub struct FunctionEntry {
    /// Function name.
    pub name: String,
    /// Category string (Scalar, Aggregate, Window, TableValued).
    pub category: String,
    /// Whether the function is deterministic.
    #[serde(default = "default_true")]
    pub deterministic: bool,
    /// Whether the function can be inlined.
    #[serde(default)]
    pub inlineable: bool,
    /// Whether the function is expensive.
    #[serde(default)]
    pub expensive: bool,
    /// Whether the function is pure.
    #[serde(default = "default_true")]
    pub pure: bool,
    /// Whether the function is constant-foldable.
    #[serde(default)]
    pub constant_foldable: bool,
    /// Cost multiplier.
    #[serde(default = "default_cost")]
    pub cost_multiplier: f64,
    /// Overloaded signatures.
    #[serde(default)]
    pub signature: Vec<SignatureEntry>,
}

fn default_true() -> bool {
    true
}

fn default_cost() -> f64 {
    1.0
}

/// One signature entry in the TOML file.
#[derive(Debug, Deserialize)]
pub struct SignatureEntry {
    /// Argument type names.
    pub args: Vec<String>,
    /// Return type name.
    pub return_type: String,
    /// Whether the last argument repeats.
    #[serde(default)]
    pub variadic: bool,
}

/// Parse a `DataType` from its TOML string representation.
pub fn parse_data_type(s: &str) -> DataType {
    match s {
        "Integer" => DataType::Integer,
        "Float" => DataType::Float,
        "Decimal" => DataType::Decimal,
        "Text" => DataType::Text,
        "Boolean" => DataType::Boolean,
        "Date" => DataType::Date,
        "Timestamp" => DataType::Timestamp,
        "Interval" => DataType::Interval,
        "Blob" => DataType::Blob,
        "Json" => DataType::Json,
        "Geometry" => DataType::Geometry,
        "TsVector" => DataType::TsVector,
        "TsQuery" => DataType::TsQuery,
        "Any" => DataType::Any,
        other if other.ends_with("[]") => {
            let inner = &other[..other.len() - 2];
            DataType::Array(Box::new(parse_data_type(inner)))
        }
        _ => DataType::Any,
    }
}

/// Parse a `FunctionCategory` from its TOML string representation.
pub fn parse_category(s: &str) -> FunctionCategory {
    match s {
        "Aggregate" => FunctionCategory::Aggregate,
        "Window" => FunctionCategory::Window,
        "TableValued" => FunctionCategory::TableValued,
        _ => FunctionCategory::Scalar,
    }
}

/// Load a `FunctionCatalog` from TOML text.
///
/// # Errors
///
/// Returns an error if the TOML text cannot be parsed.
pub fn load_catalog_from_toml(
    toml_text: &str,
) -> Result<FunctionCatalog, toml::de::Error> {
    let parsed: FunctionToml = toml::from_str(toml_text)?;
    let mut catalog = FunctionCatalog::new();

    for entry in parsed.function {
        let category = parse_category(&entry.category);
        let signatures: Vec<FunctionSignature> = entry
            .signature
            .iter()
            .map(|s| FunctionSignature {
                args: s.args.iter().map(|a| parse_data_type(a)).collect(),
                return_type: parse_data_type(&s.return_type),
                variadic: s.variadic,
            })
            .collect();

        let def = FunctionDefinition {
            name: entry.name,
            category,
            signatures,
            properties: FunctionProperties {
                deterministic: entry.deterministic,
                inlineable: entry.inlineable,
                expensive: entry.expensive,
                pure: entry.pure,
                constant_foldable: entry.constant_foldable,
                cost_multiplier: entry.cost_multiplier,
            },
        };
        catalog.register(def);
    }
    Ok(catalog)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    // -- DataType --

    #[test]
    fn parse_basic_types() {
        assert_eq!(parse_data_type("Integer"), DataType::Integer);
        assert_eq!(parse_data_type("Float"), DataType::Float);
        assert_eq!(parse_data_type("Text"), DataType::Text);
        assert_eq!(parse_data_type("Boolean"), DataType::Boolean);
        assert_eq!(parse_data_type("Geometry"), DataType::Geometry);
    }

    #[test]
    fn parse_array_type() {
        assert_eq!(
            parse_data_type("Integer[]"),
            DataType::Array(Box::new(DataType::Integer))
        );
    }

    #[test]
    fn parse_unknown_type_falls_back_to_any() {
        assert_eq!(parse_data_type("UnknownType"), DataType::Any);
    }

    // -- FunctionProperties --

    #[test]
    fn cheap_pure_properties() {
        let p = FunctionProperties::cheap_pure();
        assert!(p.deterministic);
        assert!(p.pure);
        assert!(p.constant_foldable);
        assert!(!p.expensive);
        assert_eq!(p.cost_multiplier, 1.0);
    }

    #[test]
    fn non_deterministic_properties() {
        let p = FunctionProperties::non_deterministic();
        assert!(!p.deterministic);
        assert!(!p.constant_foldable);
    }

    #[test]
    fn expensive_pure_properties() {
        let p = FunctionProperties::expensive_pure();
        assert!(p.expensive);
        assert!(p.deterministic);
        assert_eq!(p.cost_multiplier, 10.0);
    }

    // -- FunctionCatalog --

    fn sample_abs() -> FunctionDefinition {
        FunctionDefinition {
            name: "ABS".into(),
            category: FunctionCategory::Scalar,
            signatures: vec![
                FunctionSignature {
                    args: vec![DataType::Integer],
                    return_type: DataType::Integer,
                    variadic: false,
                },
                FunctionSignature {
                    args: vec![DataType::Float],
                    return_type: DataType::Float,
                    variadic: false,
                },
            ],
            properties: FunctionProperties::cheap_pure(),
        }
    }

    #[test]
    fn catalog_register_and_lookup() {
        let mut cat = FunctionCatalog::new();
        cat.register(sample_abs());
        assert_eq!(cat.len(), 1);
        assert!(cat.lookup("ABS").is_some());
        assert!(cat.lookup("abs").is_some()); // case-insensitive
    }

    #[test]
    fn catalog_lookup_missing() {
        let cat = FunctionCatalog::new();
        assert!(cat.lookup("NONEXISTENT").is_none());
    }

    #[test]
    fn catalog_is_empty() {
        let cat = FunctionCatalog::new();
        assert!(cat.is_empty());
    }

    #[test]
    fn catalog_by_category() {
        let mut cat = FunctionCatalog::new();
        cat.register(sample_abs());
        cat.register(FunctionDefinition {
            name: "SUM".into(),
            category: FunctionCategory::Aggregate,
            signatures: vec![],
            properties: FunctionProperties::aggregate(),
        });
        assert_eq!(cat.by_category(FunctionCategory::Scalar).len(), 1);
        assert_eq!(cat.by_category(FunctionCategory::Aggregate).len(), 1);
        assert!(cat.by_category(FunctionCategory::Window).is_empty());
    }

    #[test]
    fn catalog_constant_foldable() {
        let mut cat = FunctionCatalog::new();
        cat.register(sample_abs());
        cat.register(FunctionDefinition {
            name: "RANDOM".into(),
            category: FunctionCategory::Scalar,
            signatures: vec![],
            properties: FunctionProperties::non_deterministic(),
        });
        let foldable = cat.constant_foldable();
        assert_eq!(foldable.len(), 1);
        assert_eq!(foldable[0].name, "ABS");
    }

    #[test]
    fn catalog_expensive_functions() {
        let mut cat = FunctionCatalog::new();
        cat.register(sample_abs());
        cat.register(FunctionDefinition {
            name: "ST_DISTANCE".into(),
            category: FunctionCategory::Scalar,
            signatures: vec![],
            properties: FunctionProperties::expensive_pure(),
        });
        let expensive = cat.expensive_functions();
        assert_eq!(expensive.len(), 1);
        assert_eq!(expensive[0].name, "ST_DISTANCE");
    }

    // -- TOML loading --

    #[test]
    fn load_simple_toml() {
        let toml = r#"
[[function]]
name = "ABS"
category = "Scalar"
deterministic = true
inlineable = true
expensive = false
pure = true
constant_foldable = true
cost_multiplier = 1.0

[[function.signature]]
args = ["Integer"]
return_type = "Integer"

[[function.signature]]
args = ["Float"]
return_type = "Float"

[[function]]
name = "RANDOM"
category = "Scalar"
deterministic = false
inlineable = false
expensive = false
pure = false
constant_foldable = false
cost_multiplier = 1.0
"#;
        let cat = load_catalog_from_toml(toml).expect("parse TOML");
        assert_eq!(cat.len(), 2);

        let abs = cat.lookup("ABS").expect("ABS");
        assert_eq!(abs.signatures.len(), 2);
        assert!(abs.properties.deterministic);

        let rand = cat.lookup("RANDOM").expect("RANDOM");
        assert!(!rand.properties.deterministic);
    }

    #[test]
    fn load_aggregate_from_toml() {
        let toml = r#"
[[function]]
name = "SUM"
category = "Aggregate"
deterministic = true
pure = true
constant_foldable = false
cost_multiplier = 1.0

[[function.signature]]
args = ["Integer"]
return_type = "Integer"
"#;
        let cat = load_catalog_from_toml(toml).expect("parse");
        let sum = cat.lookup("SUM").expect("SUM");
        assert_eq!(sum.category, FunctionCategory::Aggregate);
    }

    #[test]
    fn load_variadic_from_toml() {
        let toml = r#"
[[function]]
name = "COALESCE"
category = "Scalar"

[[function.signature]]
args = ["Any"]
return_type = "Any"
variadic = true
"#;
        let cat = load_catalog_from_toml(toml).expect("parse");
        let coal = cat.lookup("COALESCE").expect("COALESCE");
        assert!(coal.signatures[0].variadic);
    }

    #[test]
    fn load_with_defaults() {
        let toml = r#"
[[function]]
name = "UPPER"
category = "Scalar"

[[function.signature]]
args = ["Text"]
return_type = "Text"
"#;
        let cat = load_catalog_from_toml(toml).expect("parse");
        let upper = cat.lookup("UPPER").expect("UPPER");
        assert!(upper.properties.deterministic); // default true
        assert!(upper.properties.pure);          // default true
        assert_eq!(upper.properties.cost_multiplier, 1.0); // default
    }

    // -- Serialization --

    #[test]
    fn function_definition_serde_roundtrip() {
        let def = sample_abs();
        let json = serde_json::to_string(&def).expect("serialize");
        let back: FunctionDefinition =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(def, back);
    }

    #[test]
    fn category_values() {
        assert_eq!(parse_category("Scalar"), FunctionCategory::Scalar);
        assert_eq!(parse_category("Aggregate"), FunctionCategory::Aggregate);
        assert_eq!(parse_category("Window"), FunctionCategory::Window);
        assert_eq!(
            parse_category("TableValued"),
            FunctionCategory::TableValued
        );
        assert_eq!(parse_category("Unknown"), FunctionCategory::Scalar);
    }

    // -- Full catalog integration --

    #[test]
    fn load_builtin_catalog() {
        let toml_text = include_str!("../data/functions.toml");
        let cat = load_catalog_from_toml(toml_text)
            .expect("built-in catalog should parse");
        assert!(
            cat.len() >= 200,
            "expected 200+ functions, got {}",
            cat.len()
        );
    }

    #[test]
    fn builtin_catalog_has_all_categories() {
        let toml_text = include_str!("../data/functions.toml");
        let cat = load_catalog_from_toml(toml_text)
            .expect("parse");
        assert!(!cat.by_category(FunctionCategory::Scalar).is_empty());
        assert!(!cat.by_category(FunctionCategory::Aggregate).is_empty());
        assert!(!cat.by_category(FunctionCategory::Window).is_empty());
        assert!(!cat.by_category(FunctionCategory::TableValued).is_empty());
    }

    #[test]
    fn builtin_catalog_key_functions() {
        let toml_text = include_str!("../data/functions.toml");
        let cat = load_catalog_from_toml(toml_text)
            .expect("parse");
        for name in &[
            "ABS", "UPPER", "NOW", "CAST", "COALESCE",
            "COUNT", "SUM", "AVG", "MIN", "MAX",
            "ROW_NUMBER", "RANK", "LAG", "LEAD",
            "JSON_EXTRACT", "ST_DISTANCE", "UNNEST",
        ] {
            assert!(
                cat.lookup(name).is_some(),
                "missing function: {name}"
            );
        }
    }

    #[test]
    fn builtin_random_not_foldable() {
        let toml_text = include_str!("../data/functions.toml");
        let cat = load_catalog_from_toml(toml_text)
            .expect("parse");
        let random = cat.lookup("RANDOM").expect("RANDOM");
        assert!(!random.properties.deterministic);
        assert!(!random.properties.constant_foldable);
    }

    #[test]
    fn builtin_st_distance_is_expensive() {
        let toml_text = include_str!("../data/functions.toml");
        let cat = load_catalog_from_toml(toml_text)
            .expect("parse");
        let st = cat.lookup("ST_DISTANCE").expect("ST_DISTANCE");
        assert!(st.properties.expensive);
        assert!(st.properties.cost_multiplier > 5.0);
    }
}
