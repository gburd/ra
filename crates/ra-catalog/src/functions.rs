//! Function catalog types for query optimization.
//!
//! Models SQL function metadata including signatures, properties,
//! and cost information used by the optimizer for constant folding,
//! function pushdown, and index matching decisions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Category of a SQL function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FunctionCategory {
    /// Scalar function returning one value per row.
    Scalar,
    /// Aggregate function combining multiple rows.
    Aggregate,
    /// Window function operating over a frame.
    Window,
    /// Table-valued function returning a result set.
    TableValued,
}

/// SQL data type for function signatures.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SqlType {
    /// Integer types (SMALLINT, INT, BIGINT).
    Integer,
    /// Floating-point types (FLOAT, DOUBLE, REAL).
    Float,
    /// Exact numeric (NUMERIC, DECIMAL).
    Numeric,
    /// Boolean.
    Boolean,
    /// Character strings (VARCHAR, TEXT, CHAR).
    Text,
    /// Binary data (BYTEA, BLOB).
    Binary,
    /// Date without time.
    Date,
    /// Time without date.
    Time,
    /// Timestamp with or without timezone.
    Timestamp,
    /// Time interval.
    Interval,
    /// JSON or JSONB.
    Json,
    /// Array of another type.
    Array,
    /// Geometry/geography spatial type.
    Geometry,
    /// Full-text search vector.
    TsVector,
    /// Full-text search query.
    TsQuery,
    /// Any type (polymorphic).
    Any,
    /// Void (no return value).
    Void,
    /// Record/row type.
    Record,
}

/// Function signature describing parameters and return type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionSignature {
    /// Ordered parameter types.
    pub parameters: Vec<SqlType>,
    /// Return type.
    pub return_type: SqlType,
    /// Whether the function accepts variadic arguments.
    pub variadic: bool,
    /// Minimum number of required arguments.
    pub min_args: usize,
    /// Maximum number of arguments (None = unlimited for variadic).
    pub max_args: Option<usize>,
}

/// Properties that affect optimizer decisions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct FunctionProperties {
    /// Returns the same output for the same input (no side effects).
    pub deterministic: bool,
    /// Can be inlined/expanded at plan time.
    pub inlineable: bool,
    /// Computationally expensive (affects pushdown decisions).
    pub expensive: bool,
    /// Pure function (no side effects, no dependency on external state).
    pub pure: bool,
    /// Can be evaluated at compile time if all args are constants.
    pub constant_foldable: bool,
    /// Whether NULL input always produces NULL output.
    pub strict: bool,
    /// Whether the aggregate/window function is order-sensitive.
    pub order_sensitive: bool,
    /// Cost multiplier relative to a simple comparison (1.0).
    pub cost_multiplier: f64,
    /// Estimated output cardinality ratio (for table-valued functions).
    pub cardinality_ratio: Option<f64>,
}

impl Default for FunctionProperties {
    fn default() -> Self {
        Self {
            deterministic: true,
            inlineable: false,
            expensive: false,
            pure: true,
            constant_foldable: true,
            strict: true,
            order_sensitive: false,
            cost_multiplier: 1.0,
            cardinality_ratio: None,
        }
    }
}

/// Database systems where a function is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[allow(clippy::doc_markdown)]
pub enum DatabaseSystem {
    /// PostgreSQL.
    PostgreSQL,
    /// MySQL / MariaDB.
    MySQL,
    /// SQLite.
    SQLite,
    /// SQL Server.
    SqlServer,
    /// Oracle.
    Oracle,
    /// DuckDB.
    DuckDB,
    /// ClickHouse.
    ClickHouse,
    /// CockroachDB.
    CockroachDB,
}

/// Complete function definition in the catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionDefinition {
    /// Canonical function name (uppercase).
    pub name: String,
    /// Function category.
    pub category: FunctionCategory,
    /// Function signature(s) -- overloads.
    pub signatures: Vec<FunctionSignature>,
    /// Optimizer-relevant properties.
    pub properties: FunctionProperties,
    /// Databases where this function is available.
    pub available_in: Vec<DatabaseSystem>,
    /// Brief description of what the function does.
    pub description: String,
}

/// The function catalog holding all known function definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCatalog {
    /// Functions indexed by canonical name.
    functions: HashMap<String, FunctionDefinition>,
}

impl FunctionCatalog {
    /// Create an empty catalog.
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    /// Create a catalog pre-populated with all built-in functions.
    pub fn with_builtins() -> Self {
        let mut catalog = Self::new();
        register_math_functions(&mut catalog);
        register_string_functions(&mut catalog);
        register_datetime_functions(&mut catalog);
        register_aggregate_functions(&mut catalog);
        register_window_functions(&mut catalog);
        register_json_functions(&mut catalog);
        register_array_functions(&mut catalog);
        register_conditional_functions(&mut catalog);
        register_type_conversion_functions(&mut catalog);
        register_geospatial_functions(&mut catalog);
        register_text_search_functions(&mut catalog);
        register_system_functions(&mut catalog);
        catalog
    }

    /// Register a function definition.
    pub fn register(&mut self, func: FunctionDefinition) {
        self.functions.insert(func.name.clone(), func);
    }

    /// Look up a function by name (case-insensitive).
    pub fn lookup(&self, name: &str) -> Option<&FunctionDefinition> {
        self.functions.get(&name.to_uppercase())
    }

    /// Get all functions in a category.
    pub fn by_category(
        &self,
        category: FunctionCategory,
    ) -> Vec<&FunctionDefinition> {
        self.functions
            .values()
            .filter(|f| f.category == category)
            .collect()
    }

    /// Get all functions available in a specific database.
    pub fn by_database(
        &self,
        db: DatabaseSystem,
    ) -> Vec<&FunctionDefinition> {
        self.functions
            .values()
            .filter(|f| f.available_in.contains(&db))
            .collect()
    }

    /// Get all deterministic functions (candidates for constant folding).
    pub fn deterministic_functions(
        &self,
    ) -> Vec<&FunctionDefinition> {
        self.functions
            .values()
            .filter(|f| f.properties.deterministic)
            .collect()
    }

    /// Get all expensive functions (candidates for pushdown avoidance).
    pub fn expensive_functions(&self) -> Vec<&FunctionDefinition> {
        self.functions
            .values()
            .filter(|f| f.properties.expensive)
            .collect()
    }

    /// Get all pure functions.
    pub fn pure_functions(&self) -> Vec<&FunctionDefinition> {
        self.functions
            .values()
            .filter(|f| f.properties.pure)
            .collect()
    }

    /// Total number of registered functions.
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Whether the catalog is empty.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// Iterate over all functions.
    pub fn iter(
        &self,
    ) -> impl Iterator<Item = &FunctionDefinition> {
        self.functions.values()
    }
}

impl Default for FunctionCatalog {
    fn default() -> Self {
        Self::with_builtins()
    }
}

// Helper to build common function signatures

fn scalar_sig(params: Vec<SqlType>, ret: SqlType) -> FunctionSignature {
    let min_args = params.len();
    FunctionSignature {
        parameters: params,
        return_type: ret,
        variadic: false,
        min_args,
        max_args: Some(min_args),
    }
}

fn variadic_sig(
    params: Vec<SqlType>,
    ret: SqlType,
    min: usize,
) -> FunctionSignature {
    FunctionSignature {
        parameters: params,
        return_type: ret,
        variadic: true,
        min_args: min,
        max_args: None,
    }
}

fn agg_sig(param: SqlType, ret: SqlType) -> FunctionSignature {
    FunctionSignature {
        parameters: vec![param],
        return_type: ret,
        variadic: false,
        min_args: 1,
        max_args: Some(1),
    }
}

const ALL_MAJOR: [DatabaseSystem; 5] = [
    DatabaseSystem::PostgreSQL,
    DatabaseSystem::MySQL,
    DatabaseSystem::SQLite,
    DatabaseSystem::SqlServer,
    DatabaseSystem::Oracle,
];

const PG_ONLY: [DatabaseSystem; 1] = [DatabaseSystem::PostgreSQL];

const PG_MYSQL: [DatabaseSystem; 2] = [
    DatabaseSystem::PostgreSQL,
    DatabaseSystem::MySQL,
];


fn register_math_functions(catalog: &mut FunctionCatalog) {
    let math_fns = vec![
        ("ABS", "Absolute value"),
        ("CEIL", "Ceiling (round up)"),
        ("CEILING", "Ceiling (round up)"),
        ("FLOOR", "Floor (round down)"),
        ("ROUND", "Round to nearest integer or decimal places"),
        ("TRUNC", "Truncate to integer or decimal places"),
        ("TRUNCATE", "Truncate to integer or decimal places"),
        ("SQRT", "Square root"),
        ("CBRT", "Cube root"),
        ("POWER", "Raise to power"),
        ("POW", "Raise to power"),
        ("EXP", "Exponential (e^x)"),
        ("LN", "Natural logarithm"),
        ("LOG", "Logarithm (base 10 or custom)"),
        ("LOG2", "Base-2 logarithm"),
        ("LOG10", "Base-10 logarithm"),
        ("MOD", "Modulo / remainder"),
        ("SIGN", "Sign of number (-1, 0, 1)"),
        ("PI", "Mathematical constant pi"),
        ("DEGREES", "Radians to degrees"),
        ("RADIANS", "Degrees to radians"),
        ("SIN", "Sine"),
        ("COS", "Cosine"),
        ("TAN", "Tangent"),
        ("ASIN", "Arc sine"),
        ("ACOS", "Arc cosine"),
        ("ATAN", "Arc tangent"),
        ("ATAN2", "Two-argument arc tangent"),
        ("COT", "Cotangent"),
        ("GREATEST", "Largest of a list of values"),
        ("LEAST", "Smallest of a list of values"),
        ("RANDOM", "Random value between 0 and 1"),
        ("DIV", "Integer division"),
        ("GCD", "Greatest common divisor"),
        ("LCM", "Least common multiple"),
        ("WIDTH_BUCKET", "Histogram bucket for value"),
    ];

    for (name, desc) in math_fns {
        let is_random = name == "RANDOM";
        let is_pi = name == "PI";
        let needs_two = matches!(
            name,
            "POWER"
                | "POW"
                | "ATAN2"
                | "MOD"
                | "DIV"
                | "GCD"
                | "LCM"
                | "LOG"
        );
        let is_variadic =
            matches!(name, "GREATEST" | "LEAST");

        let sig = if is_pi {
            scalar_sig(vec![], SqlType::Float)
        } else if is_variadic {
            variadic_sig(
                vec![SqlType::Numeric],
                SqlType::Numeric,
                2,
            )
        } else if needs_two {
            scalar_sig(
                vec![SqlType::Numeric, SqlType::Numeric],
                SqlType::Numeric,
            )
        } else {
            scalar_sig(
                vec![SqlType::Numeric],
                SqlType::Numeric,
            )
        };

        catalog.register(FunctionDefinition {
            name: name.to_string(),
            category: FunctionCategory::Scalar,
            signatures: vec![sig],
            properties: FunctionProperties {
                deterministic: !is_random,
                pure: !is_random,
                constant_foldable: !is_random,
                cost_multiplier: if matches!(
                    name,
                    "SQRT"
                        | "POWER"
                        | "POW"
                        | "EXP"
                        | "LN"
                        | "LOG"
                        | "LOG2"
                        | "LOG10"
                        | "SIN"
                        | "COS"
                        | "TAN"
                        | "ASIN"
                        | "ACOS"
                        | "ATAN"
                        | "ATAN2"
                        | "COT"
                ) {
                    2.0
                } else {
                    1.0
                },
                ..FunctionProperties::default()
            },
            available_in: if matches!(
                name,
                "CBRT" | "GCD" | "LCM" | "WIDTH_BUCKET"
            ) {
                PG_ONLY.to_vec()
            } else {
                ALL_MAJOR.to_vec()
            },
            description: desc.to_string(),
        });
    }
}

fn register_string_functions(catalog: &mut FunctionCatalog) {
    let string_fns = vec![
        ("UPPER", "Convert to uppercase", 1.0),
        ("LOWER", "Convert to lowercase", 1.0),
        ("LENGTH", "String length in characters", 1.0),
        ("CHAR_LENGTH", "Character length", 1.0),
        ("OCTET_LENGTH", "Length in bytes", 1.0),
        ("BIT_LENGTH", "Length in bits", 1.0),
        ("TRIM", "Remove leading/trailing whitespace", 1.5),
        ("LTRIM", "Remove leading whitespace", 1.5),
        ("RTRIM", "Remove trailing whitespace", 1.5),
        ("LPAD", "Pad string on left", 1.5),
        ("RPAD", "Pad string on right", 1.5),
        ("SUBSTRING", "Extract substring", 1.5),
        ("SUBSTR", "Extract substring", 1.5),
        ("LEFT", "Extract leftmost characters", 1.0),
        ("RIGHT", "Extract rightmost characters", 1.0),
        ("CONCAT", "Concatenate strings", 2.0),
        ("CONCAT_WS", "Concatenate with separator", 2.0),
        ("REPLACE", "Replace occurrences of substring", 3.0),
        ("TRANSLATE", "Character-by-character translation", 3.0),
        ("REVERSE", "Reverse string", 1.5),
        ("REPEAT", "Repeat string N times", 2.0),
        ("POSITION", "Find substring position", 2.0),
        ("STRPOS", "Find substring position", 2.0),
        ("LOCATE", "Find substring position", 2.0),
        ("ASCII", "ASCII code of first character", 1.0),
        ("CHR", "Character from ASCII code", 1.0),
        ("INITCAP", "Capitalize first letter of each word", 1.5),
        ("MD5", "MD5 hash as hex string", 10.0),
        ("SHA256", "SHA-256 hash", 15.0),
        ("ENCODE", "Encode binary to text", 3.0),
        ("DECODE", "Decode text to binary", 3.0),
        ("QUOTE_LITERAL", "Quote string as SQL literal", 1.5),
        ("QUOTE_IDENT", "Quote string as SQL identifier", 1.5),
        ("FORMAT", "Format string with arguments", 3.0),
        ("REGEXP_REPLACE", "Regex-based replacement", 10.0),
        ("REGEXP_MATCHES", "Regex pattern matching", 10.0),
        ("REGEXP_SUBSTR", "Regex substring extraction", 10.0),
        ("LIKE", "Pattern matching with wildcards", 3.0),
        ("ILIKE", "Case-insensitive LIKE", 4.0),
        ("SPLIT_PART", "Split string and return part", 2.0),
        ("STRING_AGG", "Aggregate strings with delimiter", 2.0),
        ("OVERLAY", "Replace substring at position", 2.0),
    ];

    for (name, desc, cost) in string_fns {
        let is_agg = name == "STRING_AGG";
        let is_variadic =
            matches!(name, "CONCAT" | "CONCAT_WS");
        let is_regex = matches!(
            name,
            "REGEXP_REPLACE"
                | "REGEXP_MATCHES"
                | "REGEXP_SUBSTR"
        );

        let sig = if is_agg {
            agg_sig(SqlType::Text, SqlType::Text)
        } else if is_variadic {
            variadic_sig(
                vec![SqlType::Text],
                SqlType::Text,
                1,
            )
        } else {
            scalar_sig(vec![SqlType::Text], SqlType::Text)
        };

        catalog.register(FunctionDefinition {
            name: name.to_string(),
            category: if is_agg {
                FunctionCategory::Aggregate
            } else {
                FunctionCategory::Scalar
            },
            signatures: vec![sig],
            properties: FunctionProperties {
                expensive: is_regex
                    || matches!(
                        name,
                        "MD5" | "SHA256" | "REPLACE"
                    ),
                constant_foldable: !is_agg,
                order_sensitive: is_agg,
                cost_multiplier: cost,
                ..FunctionProperties::default()
            },
            available_in: if matches!(
                name,
                "ILIKE"
                    | "STRPOS"
                    | "INITCAP"
                    | "STRING_AGG"
                    | "SPLIT_PART"
                    | "OVERLAY"
            ) {
                PG_ONLY.to_vec()
            } else if is_regex {
                PG_MYSQL.to_vec()
            } else {
                ALL_MAJOR.to_vec()
            },
            description: desc.to_string(),
        });
    }
}

fn register_datetime_functions(catalog: &mut FunctionCatalog) {
    let dt_fns = vec![
        ("NOW", "Current timestamp", false, true, 1.0),
        (
            "CURRENT_TIMESTAMP",
            "Current timestamp",
            false,
            true,
            1.0,
        ),
        ("CURRENT_DATE", "Current date", false, true, 1.0),
        ("CURRENT_TIME", "Current time", false, true, 1.0),
        (
            "CLOCK_TIMESTAMP",
            "Current timestamp (changes during statement)",
            false,
            false,
            1.0,
        ),
        ("DATE_TRUNC", "Truncate timestamp to precision", true, true, 2.0),
        ("EXTRACT", "Extract field from timestamp", true, true, 1.5),
        ("DATE_PART", "Extract field from timestamp", true, true, 1.5),
        ("DATE_ADD", "Add interval to date", true, true, 2.0),
        ("DATE_SUB", "Subtract interval from date", true, true, 2.0),
        ("DATEADD", "Add interval to date", true, true, 2.0),
        ("DATEDIFF", "Difference between dates", true, true, 2.0),
        ("AGE", "Interval between timestamps", true, true, 2.0),
        ("MAKE_DATE", "Construct date from parts", true, true, 1.5),
        ("MAKE_TIME", "Construct time from parts", true, true, 1.5),
        (
            "MAKE_TIMESTAMP",
            "Construct timestamp from parts",
            true,
            true,
            1.5,
        ),
        ("TO_CHAR", "Format timestamp as string", true, true, 3.0),
        ("TO_DATE", "Parse string to date", true, true, 3.0),
        (
            "TO_TIMESTAMP",
            "Parse string to timestamp",
            true,
            true,
            3.0,
        ),
        (
            "TIMEZONE",
            "Convert timestamp between timezones",
            true,
            true,
            2.0,
        ),
        (
            "AT_TIME_ZONE",
            "Convert to specified timezone",
            true,
            true,
            2.0,
        ),
        ("EPOCH", "Extract Unix epoch", true, true, 1.0),
        ("DATE", "Extract date from timestamp", true, true, 1.0),
        ("TIME", "Extract time from timestamp", true, true, 1.0),
        ("YEAR", "Extract year", true, true, 1.0),
        ("MONTH", "Extract month", true, true, 1.0),
        ("DAY", "Extract day", true, true, 1.0),
        ("HOUR", "Extract hour", true, true, 1.0),
        ("MINUTE", "Extract minute", true, true, 1.0),
        ("SECOND", "Extract second", true, true, 1.0),
        (
            "DAYOFWEEK",
            "Day of week (1=Sunday)",
            true,
            true,
            1.0,
        ),
        (
            "DAYOFYEAR",
            "Day of year (1-366)",
            true,
            true,
            1.0,
        ),
        ("WEEK", "ISO week number", true, true, 1.0),
        ("QUARTER", "Quarter (1-4)", true, true, 1.0),
        (
            "LAST_DAY",
            "Last day of the month",
            true,
            true,
            1.5,
        ),
        (
            "MONTHS_BETWEEN",
            "Number of months between dates",
            true,
            true,
            2.0,
        ),
        (
            "GENERATE_SERIES",
            "Generate series of timestamps",
            true,
            true,
            5.0,
        ),
    ];

    for (name, desc, det, pure, cost) in dt_fns {
        let is_now = matches!(
            name,
            "NOW"
                | "CURRENT_TIMESTAMP"
                | "CURRENT_DATE"
                | "CURRENT_TIME"
                | "CLOCK_TIMESTAMP"
        );
        let is_table_valued = name == "GENERATE_SERIES";

        let sig = if is_now {
            scalar_sig(vec![], SqlType::Timestamp)
        } else if is_table_valued {
            scalar_sig(
                vec![
                    SqlType::Timestamp,
                    SqlType::Timestamp,
                    SqlType::Interval,
                ],
                SqlType::Timestamp,
            )
        } else {
            scalar_sig(
                vec![SqlType::Timestamp],
                SqlType::Timestamp,
            )
        };

        catalog.register(FunctionDefinition {
            name: name.to_string(),
            category: if is_table_valued {
                FunctionCategory::TableValued
            } else {
                FunctionCategory::Scalar
            },
            signatures: vec![sig],
            properties: FunctionProperties {
                deterministic: det,
                pure,
                constant_foldable: det && !is_now,
                cost_multiplier: cost,
                cardinality_ratio: if is_table_valued {
                    Some(100.0)
                } else {
                    None
                },
                ..FunctionProperties::default()
            },
            available_in: if matches!(
                name,
                "AGE"
                    | "MAKE_DATE"
                    | "MAKE_TIME"
                    | "MAKE_TIMESTAMP"
                    | "CLOCK_TIMESTAMP"
                    | "DATE_PART"
                    | "GENERATE_SERIES"
            ) {
                PG_ONLY.to_vec()
            } else {
                ALL_MAJOR.to_vec()
            },
            description: desc.to_string(),
        });
    }
}

fn register_aggregate_functions(catalog: &mut FunctionCatalog) {
    let agg_fns = vec![
        ("COUNT", "Count rows or non-null values", SqlType::Integer, 1.0),
        ("SUM", "Sum of values", SqlType::Numeric, 1.0),
        ("AVG", "Average of values", SqlType::Float, 1.5),
        ("MIN", "Minimum value", SqlType::Any, 1.0),
        ("MAX", "Maximum value", SqlType::Any, 1.0),
        ("STDDEV", "Population standard deviation", SqlType::Float, 3.0),
        ("STDDEV_POP", "Population standard deviation", SqlType::Float, 3.0),
        ("STDDEV_SAMP", "Sample standard deviation", SqlType::Float, 3.0),
        ("VARIANCE", "Population variance", SqlType::Float, 3.0),
        ("VAR_POP", "Population variance", SqlType::Float, 3.0),
        ("VAR_SAMP", "Sample variance", SqlType::Float, 3.0),
        ("COVAR_POP", "Population covariance", SqlType::Float, 3.0),
        ("COVAR_SAMP", "Sample covariance", SqlType::Float, 3.0),
        ("CORR", "Correlation coefficient", SqlType::Float, 4.0),
        ("REGR_SLOPE", "Slope of least-squares-fit linear equation", SqlType::Float, 5.0),
        ("REGR_INTERCEPT", "Y-intercept of least-squares-fit", SqlType::Float, 5.0),
        ("REGR_R2", "R-squared of least-squares-fit", SqlType::Float, 5.0),
        ("BOOL_AND", "Logical AND of all values", SqlType::Boolean, 1.0),
        ("BOOL_OR", "Logical OR of all values", SqlType::Boolean, 1.0),
        ("EVERY", "True if all values are true", SqlType::Boolean, 1.0),
        ("BIT_AND", "Bitwise AND of all values", SqlType::Integer, 1.0),
        ("BIT_OR", "Bitwise OR of all values", SqlType::Integer, 1.0),
        ("BIT_XOR", "Bitwise XOR of all values", SqlType::Integer, 1.0),
        ("ARRAY_AGG", "Aggregate values into array", SqlType::Array, 2.0),
        ("JSON_AGG", "Aggregate values into JSON array", SqlType::Json, 3.0),
        ("JSONB_AGG", "Aggregate values into JSONB array", SqlType::Json, 3.0),
        ("JSON_OBJECT_AGG", "Aggregate key-value pairs into JSON object", SqlType::Json, 4.0),
        ("JSONB_OBJECT_AGG", "Aggregate key-value pairs into JSONB object", SqlType::Json, 4.0),
        ("PERCENTILE_CONT", "Continuous percentile", SqlType::Float, 5.0),
        ("PERCENTILE_DISC", "Discrete percentile", SqlType::Any, 5.0),
        ("MODE", "Most frequent value", SqlType::Any, 5.0),
        ("MEDIAN", "Median value (50th percentile)", SqlType::Float, 5.0),
        ("LISTAGG", "Concatenate values with delimiter", SqlType::Text, 2.0),
        ("GROUP_CONCAT", "Concatenate values (MySQL)", SqlType::Text, 2.0),
        ("COUNT_DISTINCT", "Count distinct values", SqlType::Integer, 2.0),
        ("APPROX_COUNT_DISTINCT", "Approximate distinct count (HLL)", SqlType::Integer, 1.5),
    ];

    for (name, desc, ret_type, cost) in agg_fns {
        let is_statistical = matches!(
            name,
            "STDDEV"
                | "STDDEV_POP"
                | "STDDEV_SAMP"
                | "VARIANCE"
                | "VAR_POP"
                | "VAR_SAMP"
                | "COVAR_POP"
                | "COVAR_SAMP"
                | "CORR"
                | "REGR_SLOPE"
                | "REGR_INTERCEPT"
                | "REGR_R2"
        );

        catalog.register(FunctionDefinition {
            name: name.to_string(),
            category: FunctionCategory::Aggregate,
            signatures: vec![agg_sig(SqlType::Any, ret_type)],
            properties: FunctionProperties {
                deterministic: true,
                pure: true,
                constant_foldable: false,
                expensive: is_statistical
                    || matches!(
                        name,
                        "PERCENTILE_CONT"
                            | "PERCENTILE_DISC"
                            | "MODE"
                            | "MEDIAN"
                    ),
                order_sensitive: matches!(
                    name,
                    "ARRAY_AGG"
                        | "JSON_AGG"
                        | "JSONB_AGG"
                        | "LISTAGG"
                        | "GROUP_CONCAT"
                        | "STRING_AGG"
                ),
                cost_multiplier: cost,
                ..FunctionProperties::default()
            },
            available_in: if matches!(
                name,
                "BOOL_AND"
                    | "BOOL_OR"
                    | "ARRAY_AGG"
                    | "JSONB_AGG"
                    | "JSONB_OBJECT_AGG"
                    | "MODE"
            ) {
                PG_ONLY.to_vec()
            } else if name == "GROUP_CONCAT" {
                [DatabaseSystem::MySQL].to_vec()
            } else if name == "LISTAGG" {
                [DatabaseSystem::Oracle].to_vec()
            } else {
                ALL_MAJOR.to_vec()
            },
            description: desc.to_string(),
        });
    }
}

fn register_window_functions(catalog: &mut FunctionCatalog) {
    let win_fns = vec![
        ("ROW_NUMBER", "Sequential row number within partition"),
        ("RANK", "Rank with gaps for ties"),
        ("DENSE_RANK", "Rank without gaps for ties"),
        ("NTILE", "Distribute rows into N buckets"),
        ("LAG", "Access previous row value"),
        ("LEAD", "Access next row value"),
        ("FIRST_VALUE", "First value in window frame"),
        ("LAST_VALUE", "Last value in window frame"),
        ("NTH_VALUE", "Nth value in window frame"),
        ("PERCENT_RANK", "Relative rank as percentage"),
        ("CUME_DIST", "Cumulative distribution"),
    ];

    for (name, desc) in win_fns {
        catalog.register(FunctionDefinition {
            name: name.to_string(),
            category: FunctionCategory::Window,
            signatures: vec![scalar_sig(
                vec![SqlType::Any],
                SqlType::Any,
            )],
            properties: FunctionProperties {
                deterministic: true,
                pure: true,
                constant_foldable: false,
                order_sensitive: true,
                cost_multiplier: 2.0,
                ..FunctionProperties::default()
            },
            available_in: ALL_MAJOR.to_vec(),
            description: desc.to_string(),
        });
    }
}

fn register_json_functions(catalog: &mut FunctionCatalog) {
    let json_fns = vec![
        ("JSON_EXTRACT", "Extract value from JSON", 3.0),
        ("JSON_EXTRACT_PATH", "Extract JSON value by path", 3.0),
        (
            "JSON_EXTRACT_PATH_TEXT",
            "Extract JSON text by path",
            3.0,
        ),
        (
            "JSONB_EXTRACT_PATH",
            "Extract JSONB value by path",
            2.0,
        ),
        (
            "JSONB_EXTRACT_PATH_TEXT",
            "Extract JSONB text by path",
            2.0,
        ),
        (
            "JSON_BUILD_OBJECT",
            "Build JSON object from key-value pairs",
            4.0,
        ),
        (
            "JSON_BUILD_ARRAY",
            "Build JSON array from values",
            3.0,
        ),
        ("JSON_ARRAY_LENGTH", "Length of JSON array", 1.5),
        ("JSONB_ARRAY_LENGTH", "Length of JSONB array", 1.0),
        ("JSON_TYPEOF", "Type of JSON value", 1.0),
        ("JSONB_TYPEOF", "Type of JSONB value", 1.0),
        ("JSON_KEYS", "Keys of JSON object", 2.0),
        ("JSONB_KEYS", "Keys of JSONB object", 1.5),
        ("JSON_EACH", "Expand JSON object to rows", 5.0),
        ("JSONB_EACH", "Expand JSONB object to rows", 4.0),
        ("JSON_EACH_TEXT", "Expand JSON object to text rows", 5.0),
        (
            "JSONB_EACH_TEXT",
            "Expand JSONB object to text rows",
            4.0,
        ),
        (
            "JSON_ARRAY_ELEMENTS",
            "Expand JSON array to rows",
            5.0,
        ),
        (
            "JSONB_ARRAY_ELEMENTS",
            "Expand JSONB array to rows",
            4.0,
        ),
        (
            "JSONB_PRETTY",
            "Pretty-print JSONB value",
            3.0,
        ),
        ("JSONB_SET", "Set value in JSONB at path", 3.0),
        (
            "JSONB_INSERT",
            "Insert value into JSONB at path",
            3.0,
        ),
        ("JSONB_STRIP_NULLS", "Remove null values from JSONB", 3.0),
        (
            "JSON_CONTAINS",
            "Check if JSON contains value",
            3.0,
        ),
        (
            "JSON_CONTAINS_PATH",
            "Check if JSON path exists",
            2.0,
        ),
        (
            "JSON_VALUE",
            "Extract scalar from JSON (SQL standard)",
            3.0,
        ),
        (
            "JSON_QUERY",
            "Extract JSON fragment (SQL standard)",
            3.0,
        ),
        (
            "JSON_EXISTS",
            "Check if JSON path yields value",
            2.0,
        ),
        ("JSON_TABLE", "Map JSON to relational table", 10.0),
    ];

    for (name, desc, cost) in json_fns {
        let is_table_valued = matches!(
            name,
            "JSON_EACH"
                | "JSONB_EACH"
                | "JSON_EACH_TEXT"
                | "JSONB_EACH_TEXT"
                | "JSON_ARRAY_ELEMENTS"
                | "JSONB_ARRAY_ELEMENTS"
                | "JSON_TABLE"
        );

        catalog.register(FunctionDefinition {
            name: name.to_string(),
            category: if is_table_valued {
                FunctionCategory::TableValued
            } else {
                FunctionCategory::Scalar
            },
            signatures: vec![scalar_sig(
                vec![SqlType::Json],
                SqlType::Json,
            )],
            properties: FunctionProperties {
                deterministic: true,
                pure: true,
                expensive: cost >= 5.0,
                cost_multiplier: cost,
                cardinality_ratio: if is_table_valued {
                    Some(10.0)
                } else {
                    None
                },
                ..FunctionProperties::default()
            },
            available_in: if name.starts_with("JSONB") {
                PG_ONLY.to_vec()
            } else if matches!(
                name,
                "JSON_VALUE" | "JSON_QUERY" | "JSON_EXISTS" | "JSON_TABLE"
            ) {
                vec![
                    DatabaseSystem::PostgreSQL,
                    DatabaseSystem::MySQL,
                    DatabaseSystem::SqlServer,
                    DatabaseSystem::Oracle,
                ]
            } else {
                PG_MYSQL.to_vec()
            },
            description: desc.to_string(),
        });
    }
}

fn register_array_functions(catalog: &mut FunctionCatalog) {
    let arr_fns = vec![
        ("ARRAY_LENGTH", "Length of array", 1.0),
        ("ARRAY_DIMS", "Text representation of array dimensions", 1.0),
        ("ARRAY_LOWER", "Lower bound of array dimension", 1.0),
        ("ARRAY_UPPER", "Upper bound of array dimension", 1.0),
        ("ARRAY_NDIMS", "Number of array dimensions", 1.0),
        ("ARRAY_POSITION", "Position of element in array", 2.0),
        ("ARRAY_POSITIONS", "All positions of element", 3.0),
        ("ARRAY_REMOVE", "Remove all occurrences of element", 3.0),
        ("ARRAY_REPLACE", "Replace all occurrences", 3.0),
        ("ARRAY_APPEND", "Append element to array", 1.5),
        ("ARRAY_PREPEND", "Prepend element to array", 1.5),
        ("ARRAY_CAT", "Concatenate arrays", 2.0),
        ("ARRAY_TO_STRING", "Convert array to string", 2.0),
        ("STRING_TO_ARRAY", "Split string into array", 3.0),
        ("UNNEST", "Expand array to rows", 5.0),
        ("CARDINALITY", "Total number of elements", 1.0),
        ("ARRAY_FILL", "Create array filled with value", 2.0),
    ];

    for (name, desc, cost) in arr_fns {
        let is_table_valued = name == "UNNEST";

        catalog.register(FunctionDefinition {
            name: name.to_string(),
            category: if is_table_valued {
                FunctionCategory::TableValued
            } else {
                FunctionCategory::Scalar
            },
            signatures: vec![scalar_sig(
                vec![SqlType::Array],
                if is_table_valued {
                    SqlType::Any
                } else if name == "ARRAY_TO_STRING" {
                    SqlType::Text
                } else if matches!(
                    name,
                    "ARRAY_LENGTH"
                        | "ARRAY_LOWER"
                        | "ARRAY_UPPER"
                        | "ARRAY_NDIMS"
                        | "ARRAY_POSITION"
                        | "CARDINALITY"
                ) {
                    SqlType::Integer
                } else {
                    SqlType::Array
                },
            )],
            properties: FunctionProperties {
                deterministic: true,
                pure: true,
                expensive: cost >= 5.0,
                cost_multiplier: cost,
                cardinality_ratio: if is_table_valued {
                    Some(10.0)
                } else {
                    None
                },
                ..FunctionProperties::default()
            },
            available_in: PG_ONLY.to_vec(),
            description: desc.to_string(),
        });
    }
}

fn register_conditional_functions(catalog: &mut FunctionCatalog) {
    let cond_fns = vec![
        ("COALESCE", "First non-null argument"),
        ("NULLIF", "Return NULL if arguments are equal"),
        ("IFNULL", "Return second arg if first is NULL"),
        ("NVL", "Return second arg if first is NULL (Oracle)"),
        ("NVL2", "Conditional on NULL (Oracle)"),
        ("IIF", "Inline IF expression"),
        ("CASE", "Conditional expression"),
        ("DECODE", "Pattern matching (Oracle)"),
        ("IF", "Conditional (MySQL)"),
    ];

    for (name, desc) in cond_fns {
        catalog.register(FunctionDefinition {
            name: name.to_string(),
            category: FunctionCategory::Scalar,
            signatures: vec![variadic_sig(
                vec![SqlType::Any],
                SqlType::Any,
                2,
            )],
            properties: FunctionProperties {
                deterministic: true,
                pure: true,
                inlineable: true,
                cost_multiplier: 1.0,
                strict: false,
                ..FunctionProperties::default()
            },
            available_in: if name == "NVL" || name == "NVL2" || name == "DECODE" {
                [DatabaseSystem::Oracle].to_vec()
            } else if name == "IIF" {
                [DatabaseSystem::SqlServer].to_vec()
            } else if name == "IF" || name == "IFNULL" {
                [DatabaseSystem::MySQL].to_vec()
            } else {
                ALL_MAJOR.to_vec()
            },
            description: desc.to_string(),
        });
    }
}

fn register_type_conversion_functions(catalog: &mut FunctionCatalog) {
    let conv_fns = vec![
        ("CAST", "Convert expression to target type", 2.0),
        ("CONVERT", "Convert expression to target type", 2.0),
        ("TO_NUMBER", "Convert string to number", 3.0),
        ("TO_CHAR", "Convert value to string", 3.0),
        ("COERCE", "Implicit type coercion", 1.0),
        ("TRY_CAST", "Cast that returns NULL on failure", 2.5),
        ("PARSE", "Parse string to target type", 5.0),
        ("TRY_PARSE", "Parse that returns NULL on failure", 5.5),
    ];

    for (name, desc, cost) in conv_fns {
        catalog.register(FunctionDefinition {
            name: name.to_string(),
            category: FunctionCategory::Scalar,
            signatures: vec![scalar_sig(
                vec![SqlType::Any],
                SqlType::Any,
            )],
            properties: FunctionProperties {
                deterministic: true,
                pure: true,
                cost_multiplier: cost,
                ..FunctionProperties::default()
            },
            available_in: if matches!(name, "TRY_CAST" | "PARSE" | "TRY_PARSE") {
                [DatabaseSystem::SqlServer].to_vec()
            } else if name == "TO_NUMBER" {
                [DatabaseSystem::Oracle, DatabaseSystem::PostgreSQL].to_vec()
            } else {
                ALL_MAJOR.to_vec()
            },
            description: desc.to_string(),
        });
    }
}

fn register_geospatial_functions(catalog: &mut FunctionCatalog) {
    let geo_fns = vec![
        ("ST_DISTANCE", "Distance between geometries", 5.0),
        ("ST_DWITHIN", "Whether geometries are within distance", 5.0),
        ("ST_CONTAINS", "Whether geometry A contains B", 5.0),
        ("ST_WITHIN", "Whether geometry A is within B", 5.0),
        ("ST_INTERSECTS", "Whether geometries intersect", 5.0),
        ("ST_OVERLAPS", "Whether geometries overlap", 5.0),
        ("ST_CROSSES", "Whether geometries cross", 5.0),
        ("ST_TOUCHES", "Whether geometries touch", 5.0),
        ("ST_DISJOINT", "Whether geometries are disjoint", 5.0),
        ("ST_EQUALS", "Whether geometries are equal", 5.0),
        ("ST_AREA", "Area of geometry", 3.0),
        ("ST_LENGTH", "Length of geometry", 3.0),
        ("ST_PERIMETER", "Perimeter of geometry", 3.0),
        ("ST_CENTROID", "Centroid of geometry", 3.0),
        ("ST_BUFFER", "Buffer around geometry", 8.0),
        ("ST_UNION", "Union of geometries", 10.0),
        ("ST_INTERSECTION", "Intersection of geometries", 10.0),
        ("ST_DIFFERENCE", "Difference of geometries", 10.0),
        ("ST_CONVEXHULL", "Convex hull of geometry", 8.0),
        ("ST_SIMPLIFY", "Simplify geometry", 6.0),
        ("ST_TRANSFORM", "Transform geometry to different SRID", 5.0),
        ("ST_SETSRID", "Set SRID on geometry", 1.0),
        ("ST_SRID", "Get SRID of geometry", 1.0),
        ("ST_ASTEXT", "Geometry as WKT text", 2.0),
        ("ST_ASGEOJSON", "Geometry as GeoJSON", 3.0),
        ("ST_GEOMFROMTEXT", "Parse WKT to geometry", 3.0),
        ("ST_GEOMFROMGEOJSON", "Parse GeoJSON to geometry", 3.0),
        ("ST_MAKEPOINT", "Create point from coordinates", 1.0),
        ("ST_MAKELINE", "Create line from points", 2.0),
        ("ST_MAKEPOLYGON", "Create polygon from ring", 3.0),
        ("ST_X", "X coordinate of point", 1.0),
        ("ST_Y", "Y coordinate of point", 1.0),
        ("ST_STARTPOINT", "First point of linestring", 1.0),
        ("ST_ENDPOINT", "Last point of linestring", 1.0),
        ("ST_NUMPOINTS", "Number of points in geometry", 1.0),
        ("ST_NRINGS", "Number of rings in polygon", 1.0),
        ("ST_GEOMETRYTYPE", "Type of geometry", 1.0),
        ("ST_DIMENSION", "Topological dimension", 1.0),
        ("ST_COORDDIM", "Coordinate dimension", 1.0),
        ("ST_ISVALID", "Whether geometry is valid", 3.0),
        ("ST_ISEMPTY", "Whether geometry is empty", 1.0),
    ];

    for (name, desc, cost) in geo_fns {
        let is_expensive = cost >= 5.0;
        let is_predicate = matches!(
            name,
            "ST_CONTAINS"
                | "ST_WITHIN"
                | "ST_INTERSECTS"
                | "ST_OVERLAPS"
                | "ST_CROSSES"
                | "ST_TOUCHES"
                | "ST_DISJOINT"
                | "ST_EQUALS"
                | "ST_DWITHIN"
        );

        catalog.register(FunctionDefinition {
            name: name.to_string(),
            category: FunctionCategory::Scalar,
            signatures: vec![scalar_sig(
                vec![SqlType::Geometry],
                if is_predicate {
                    SqlType::Boolean
                } else if matches!(
                    name,
                    "ST_ASTEXT"
                        | "ST_ASGEOJSON"
                        | "ST_GEOMETRYTYPE"
                ) {
                    SqlType::Text
                } else if matches!(
                    name,
                    "ST_AREA"
                        | "ST_LENGTH"
                        | "ST_PERIMETER"
                        | "ST_DISTANCE"
                        | "ST_X"
                        | "ST_Y"
                ) {
                    SqlType::Float
                } else if matches!(
                    name,
                    "ST_SRID"
                        | "ST_NUMPOINTS"
                        | "ST_NRINGS"
                        | "ST_DIMENSION"
                        | "ST_COORDDIM"
                ) {
                    SqlType::Integer
                } else {
                    SqlType::Geometry
                },
            )],
            properties: FunctionProperties {
                deterministic: true,
                pure: true,
                expensive: is_expensive,
                cost_multiplier: cost,
                ..FunctionProperties::default()
            },
            available_in: PG_ONLY.to_vec(),
            description: desc.to_string(),
        });
    }
}

fn register_text_search_functions(catalog: &mut FunctionCatalog) {
    let ts_fns = vec![
        ("TO_TSVECTOR", "Convert text to tsvector", 10.0),
        ("TO_TSQUERY", "Convert text to tsquery", 5.0),
        ("PLAINTO_TSQUERY", "Convert plain text to tsquery", 5.0),
        ("PHRASETO_TSQUERY", "Convert phrase to tsquery", 5.0),
        ("WEBSEARCH_TO_TSQUERY", "Convert web-style search to tsquery", 5.0),
        ("TS_RANK", "Rank document relevance", 8.0),
        ("TS_RANK_CD", "Rank with cover density", 8.0),
        ("TS_HEADLINE", "Generate search result headline", 15.0),
        ("TSVECTOR_LENGTH", "Number of lexemes in tsvector", 1.0),
        ("TS_REWRITE", "Rewrite tsquery using rules", 5.0),
        ("SETWEIGHT", "Set weight on tsvector", 2.0),
        ("STRIP", "Remove positions and weights from tsvector", 1.0),
    ];

    for (name, desc, cost) in ts_fns {
        catalog.register(FunctionDefinition {
            name: name.to_string(),
            category: FunctionCategory::Scalar,
            signatures: vec![scalar_sig(
                vec![SqlType::Text],
                if matches!(
                    name,
                    "TO_TSVECTOR" | "SETWEIGHT" | "STRIP"
                ) {
                    SqlType::TsVector
                } else if matches!(
                    name,
                    "TO_TSQUERY"
                        | "PLAINTO_TSQUERY"
                        | "PHRASETO_TSQUERY"
                        | "WEBSEARCH_TO_TSQUERY"
                        | "TS_REWRITE"
                ) {
                    SqlType::TsQuery
                } else if matches!(
                    name,
                    "TSVECTOR_LENGTH"
                ) {
                    SqlType::Integer
                } else if matches!(name, "TS_HEADLINE") {
                    SqlType::Text
                } else {
                    SqlType::Float
                },
            )],
            properties: FunctionProperties {
                deterministic: true,
                pure: true,
                expensive: cost >= 5.0,
                cost_multiplier: cost,
                ..FunctionProperties::default()
            },
            available_in: PG_ONLY.to_vec(),
            description: desc.to_string(),
        });
    }
}

fn register_system_functions(catalog: &mut FunctionCatalog) {
    let sys_fns = vec![
        ("CURRENT_USER", "Current session user"),
        ("SESSION_USER", "Session user name"),
        ("CURRENT_SCHEMA", "Current schema name"),
        ("CURRENT_DATABASE", "Current database name"),
        ("VERSION", "Database version string"),
        ("PG_TYPEOF", "Type of expression"),
        ("PG_COLUMN_SIZE", "Bytes used by column value"),
        ("PG_TOTAL_RELATION_SIZE", "Total relation size including indexes"),
        ("PG_TABLE_SIZE", "Table size excluding indexes"),
        ("PG_INDEXES_SIZE", "Total size of indexes on table"),
        ("NEXTVAL", "Next value from sequence"),
        ("CURRVAL", "Current value of sequence"),
        ("SETVAL", "Set sequence value"),
        ("GEN_RANDOM_UUID", "Generate random UUID"),
        ("UUID_GENERATE_V4", "Generate random UUID"),
        ("TXID_CURRENT", "Current transaction ID"),
    ];

    for (name, desc) in sys_fns {
        let is_volatile = matches!(
            name,
            "NEXTVAL"
                | "CURRVAL"
                | "SETVAL"
                | "GEN_RANDOM_UUID"
                | "UUID_GENERATE_V4"
                | "TXID_CURRENT"
        );

        catalog.register(FunctionDefinition {
            name: name.to_string(),
            category: FunctionCategory::Scalar,
            signatures: vec![scalar_sig(
                vec![],
                SqlType::Text,
            )],
            properties: FunctionProperties {
                deterministic: !is_volatile,
                pure: !is_volatile,
                constant_foldable: false,
                cost_multiplier: 1.0,
                ..FunctionProperties::default()
            },
            available_in: PG_ONLY.to_vec(),
            description: desc.to_string(),
        });
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_has_200_plus_functions() {
        let catalog = FunctionCatalog::with_builtins();
        assert!(
            catalog.len() >= 200,
            "Expected 200+ functions, got {}",
            catalog.len()
        );
    }

    #[test]
    fn test_lookup_case_insensitive() {
        let catalog = FunctionCatalog::with_builtins();
        assert!(catalog.lookup("abs").is_some());
        assert!(catalog.lookup("ABS").is_some());
        assert!(catalog.lookup("Abs").is_some());
    }

    #[test]
    fn test_math_functions_registered() {
        let catalog = FunctionCatalog::with_builtins();
        for name in &[
            "ABS", "CEIL", "FLOOR", "ROUND", "SQRT", "POWER",
            "EXP", "LN", "LOG", "SIN", "COS", "TAN",
        ] {
            assert!(
                catalog.lookup(name).is_some(),
                "Missing math function: {name}"
            );
        }
    }

    #[test]
    fn test_string_functions_registered() {
        let catalog = FunctionCatalog::with_builtins();
        for name in &[
            "UPPER", "LOWER", "SUBSTRING", "CONCAT",
            "LENGTH", "TRIM", "REPLACE", "POSITION",
        ] {
            assert!(
                catalog.lookup(name).is_some(),
                "Missing string function: {name}"
            );
        }
    }

    #[test]
    fn test_aggregate_functions_registered() {
        let catalog = FunctionCatalog::with_builtins();
        for name in &[
            "COUNT", "SUM", "AVG", "MIN", "MAX", "STDDEV",
            "PERCENTILE_CONT", "ARRAY_AGG",
        ] {
            assert!(
                catalog.lookup(name).is_some(),
                "Missing aggregate function: {name}"
            );
        }
    }

    #[test]
    fn test_window_functions_registered() {
        let catalog = FunctionCatalog::with_builtins();
        for name in &[
            "ROW_NUMBER",
            "RANK",
            "DENSE_RANK",
            "LAG",
            "LEAD",
            "FIRST_VALUE",
            "LAST_VALUE",
        ] {
            assert!(
                catalog.lookup(name).is_some(),
                "Missing window function: {name}"
            );
        }
    }

    #[test]
    fn test_json_functions_registered() {
        let catalog = FunctionCatalog::with_builtins();
        for name in &[
            "JSON_EXTRACT",
            "JSON_BUILD_OBJECT",
            "JSONB_AGG",
            "JSON_TABLE",
        ] {
            assert!(
                catalog.lookup(name).is_some(),
                "Missing JSON function: {name}"
            );
        }
    }

    #[test]
    fn test_geospatial_functions_registered() {
        let catalog = FunctionCatalog::with_builtins();
        for name in &[
            "ST_DISTANCE",
            "ST_CONTAINS",
            "ST_INTERSECTS",
            "ST_BUFFER",
            "ST_UNION",
        ] {
            assert!(
                catalog.lookup(name).is_some(),
                "Missing geospatial function: {name}"
            );
        }
    }

    #[test]
    fn test_random_is_non_deterministic() {
        let catalog = FunctionCatalog::with_builtins();
        let random = catalog.lookup("RANDOM");
        assert!(random.is_some());
        let random = random.expect("RANDOM should exist");
        assert!(!random.properties.deterministic);
        assert!(!random.properties.pure);
        assert!(!random.properties.constant_foldable);
    }

    #[test]
    fn test_abs_is_deterministic() {
        let catalog = FunctionCatalog::with_builtins();
        let abs = catalog
            .lookup("ABS")
            .expect("ABS should exist");
        assert!(abs.properties.deterministic);
        assert!(abs.properties.pure);
        assert!(abs.properties.constant_foldable);
    }

    #[test]
    fn test_now_is_not_constant_foldable() {
        let catalog = FunctionCatalog::with_builtins();
        let now = catalog
            .lookup("NOW")
            .expect("NOW should exist");
        assert!(!now.properties.deterministic);
        assert!(!now.properties.constant_foldable);
    }

    #[test]
    fn test_expensive_functions() {
        let catalog = FunctionCatalog::with_builtins();
        let expensive = catalog.expensive_functions();
        assert!(!expensive.is_empty());

        let names: Vec<&str> =
            expensive.iter().map(|f| f.name.as_str()).collect();
        assert!(
            names.contains(&"REGEXP_REPLACE"),
            "REGEXP_REPLACE should be expensive"
        );
        assert!(
            names.contains(&"ST_DISTANCE"),
            "ST_DISTANCE should be expensive"
        );
    }

    #[test]
    fn test_by_category() {
        let catalog = FunctionCatalog::with_builtins();

        let scalars =
            catalog.by_category(FunctionCategory::Scalar);
        assert!(!scalars.is_empty());

        let aggregates =
            catalog.by_category(FunctionCategory::Aggregate);
        assert!(!aggregates.is_empty());

        let windows =
            catalog.by_category(FunctionCategory::Window);
        assert!(!windows.is_empty());

        let table_valued =
            catalog.by_category(FunctionCategory::TableValued);
        assert!(!table_valued.is_empty());
    }

    #[test]
    fn test_by_database() {
        let catalog = FunctionCatalog::with_builtins();

        let pg_fns =
            catalog.by_database(DatabaseSystem::PostgreSQL);
        assert!(!pg_fns.is_empty());

        let mysql_fns =
            catalog.by_database(DatabaseSystem::MySQL);
        assert!(!mysql_fns.is_empty());

        assert!(
            pg_fns.len() > mysql_fns.len(),
            "PostgreSQL should have more functions than MySQL"
        );
    }

    #[test]
    fn test_cost_multipliers() {
        let catalog = FunctionCatalog::with_builtins();

        let abs = catalog
            .lookup("ABS")
            .expect("ABS should exist");
        assert!(
            (abs.properties.cost_multiplier - 1.0).abs()
                < f64::EPSILON
        );

        let md5 = catalog
            .lookup("MD5")
            .expect("MD5 should exist");
        assert!(md5.properties.cost_multiplier > 5.0);

        let st_buffer = catalog
            .lookup("ST_BUFFER")
            .expect("ST_BUFFER should exist");
        assert!(st_buffer.properties.cost_multiplier > 5.0);
    }

    #[test]
    fn test_aggregate_order_sensitivity() {
        let catalog = FunctionCatalog::with_builtins();

        let array_agg = catalog
            .lookup("ARRAY_AGG")
            .expect("ARRAY_AGG should exist");
        assert!(array_agg.properties.order_sensitive);

        let count = catalog
            .lookup("COUNT")
            .expect("COUNT should exist");
        assert!(!count.properties.order_sensitive);
    }

    #[test]
    fn test_empty_catalog() {
        let catalog = FunctionCatalog::new();
        assert!(catalog.is_empty());
        assert_eq!(catalog.len(), 0);
        assert!(catalog.lookup("ABS").is_none());
    }

    #[test]
    fn test_register_custom_function() {
        let mut catalog = FunctionCatalog::new();
        catalog.register(FunctionDefinition {
            name: "MY_FUNC".to_string(),
            category: FunctionCategory::Scalar,
            signatures: vec![scalar_sig(
                vec![SqlType::Integer],
                SqlType::Integer,
            )],
            properties: FunctionProperties::default(),
            available_in: vec![DatabaseSystem::PostgreSQL],
            description: "Custom test function".to_string(),
        });
        assert_eq!(catalog.len(), 1);
        assert!(catalog.lookup("my_func").is_some());
    }
}
