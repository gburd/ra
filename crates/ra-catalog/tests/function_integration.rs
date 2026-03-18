//! Integration tests for function catalog, properties, and optimization rules.
#![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]

use ra_catalog::{
    DatabaseSystem, FunctionCatalog, FunctionCategory, FunctionDefinition,
    FunctionProperties, FunctionSignature, SqlType,
};

// ===========================================================================
// 1. Catalog size and completeness
// ===========================================================================

#[test]
fn test_catalog_has_at_least_200_functions() {
    let catalog = FunctionCatalog::with_builtins();
    assert!(
        catalog.len() >= 200,
        "Expected 200+ functions, got {}",
        catalog.len()
    );
}

#[test]
fn test_catalog_has_all_categories() {
    let catalog = FunctionCatalog::with_builtins();
    let scalars = catalog.by_category(FunctionCategory::Scalar);
    let aggregates = catalog.by_category(FunctionCategory::Aggregate);
    let windows = catalog.by_category(FunctionCategory::Window);
    let table_valued = catalog.by_category(FunctionCategory::TableValued);

    assert!(!scalars.is_empty(), "Should have scalar functions");
    assert!(!aggregates.is_empty(), "Should have aggregate functions");
    assert!(!windows.is_empty(), "Should have window functions");
    assert!(!table_valued.is_empty(), "Should have table-valued functions");
}

#[test]
fn test_scalar_functions_dominate_catalog() {
    let catalog = FunctionCatalog::with_builtins();
    let scalars = catalog.by_category(FunctionCategory::Scalar);
    let total = catalog.len();
    assert!(
        scalars.len() > total / 2,
        "Scalars should be majority: {} scalars out of {} total",
        scalars.len(),
        total
    );
}

// ===========================================================================
// 2. Function lookup
// ===========================================================================

#[test]
fn test_lookup_case_insensitive() {
    let catalog = FunctionCatalog::with_builtins();
    assert!(catalog.lookup("abs").is_some());
    assert!(catalog.lookup("ABS").is_some());
    assert!(catalog.lookup("Abs").is_some());
    assert!(catalog.lookup("aBs").is_some());
}

#[test]
fn test_lookup_nonexistent_returns_none() {
    let catalog = FunctionCatalog::with_builtins();
    assert!(catalog.lookup("DEFINITELY_NOT_A_FUNCTION").is_none());
}

#[test]
fn test_lookup_all_math_functions() {
    let catalog = FunctionCatalog::with_builtins();
    let math_fns = [
        "ABS", "CEIL", "CEILING", "FLOOR", "ROUND", "TRUNC",
        "SQRT", "POWER", "EXP", "LN", "LOG", "MOD", "SIGN",
        "PI", "SIN", "COS", "TAN", "ASIN", "ACOS", "ATAN",
        "GREATEST", "LEAST", "RANDOM",
    ];
    for name in &math_fns {
        assert!(
            catalog.lookup(name).is_some(),
            "Missing math function: {name}"
        );
    }
}

#[test]
fn test_lookup_all_string_functions() {
    let catalog = FunctionCatalog::with_builtins();
    let string_fns = [
        "UPPER", "LOWER", "LENGTH", "TRIM", "SUBSTRING", "CONCAT",
        "REPLACE", "POSITION", "REVERSE", "REPEAT", "MD5", "LIKE",
    ];
    for name in &string_fns {
        assert!(
            catalog.lookup(name).is_some(),
            "Missing string function: {name}"
        );
    }
}

#[test]
fn test_lookup_all_datetime_functions() {
    let catalog = FunctionCatalog::with_builtins();
    let dt_fns = [
        "NOW", "CURRENT_TIMESTAMP", "DATE_TRUNC", "EXTRACT",
        "DATE_ADD", "TO_CHAR", "TO_TIMESTAMP", "YEAR", "MONTH", "DAY",
    ];
    for name in &dt_fns {
        assert!(
            catalog.lookup(name).is_some(),
            "Missing datetime function: {name}"
        );
    }
}

#[test]
fn test_lookup_all_aggregate_functions() {
    let catalog = FunctionCatalog::with_builtins();
    let agg_fns = [
        "COUNT", "SUM", "AVG", "MIN", "MAX", "STDDEV",
        "VARIANCE", "PERCENTILE_CONT", "ARRAY_AGG", "JSON_AGG",
    ];
    for name in &agg_fns {
        assert!(
            catalog.lookup(name).is_some(),
            "Missing aggregate function: {name}"
        );
    }
}

#[test]
fn test_lookup_all_window_functions() {
    let catalog = FunctionCatalog::with_builtins();
    let win_fns = [
        "ROW_NUMBER", "RANK", "DENSE_RANK", "NTILE",
        "LAG", "LEAD", "FIRST_VALUE", "LAST_VALUE",
        "NTH_VALUE", "PERCENT_RANK", "CUME_DIST",
    ];
    for name in &win_fns {
        assert!(
            catalog.lookup(name).is_some(),
            "Missing window function: {name}"
        );
    }
}

#[test]
fn test_lookup_json_functions() {
    let catalog = FunctionCatalog::with_builtins();
    let json_fns = [
        "JSON_EXTRACT", "JSON_BUILD_OBJECT", "JSONB_AGG",
        "JSON_TABLE", "JSONB_SET", "JSON_TYPEOF",
    ];
    for name in &json_fns {
        assert!(
            catalog.lookup(name).is_some(),
            "Missing JSON function: {name}"
        );
    }
}

#[test]
fn test_lookup_geospatial_functions() {
    let catalog = FunctionCatalog::with_builtins();
    let geo_fns = [
        "ST_DISTANCE", "ST_CONTAINS", "ST_INTERSECTS",
        "ST_BUFFER", "ST_UNION", "ST_TRANSFORM",
        "ST_MAKEPOINT", "ST_ASGEOJSON",
    ];
    for name in &geo_fns {
        assert!(
            catalog.lookup(name).is_some(),
            "Missing geospatial function: {name}"
        );
    }
}

#[test]
fn test_lookup_text_search_functions() {
    let catalog = FunctionCatalog::with_builtins();
    let ts_fns = [
        "TO_TSVECTOR", "TO_TSQUERY", "TS_RANK", "TS_HEADLINE",
    ];
    for name in &ts_fns {
        assert!(
            catalog.lookup(name).is_some(),
            "Missing text search function: {name}"
        );
    }
}

// ===========================================================================
// 3. Function properties - determinism
// ===========================================================================

#[test]
fn test_deterministic_math_functions() {
    let catalog = FunctionCatalog::with_builtins();
    let det_fns = ["ABS", "CEIL", "FLOOR", "ROUND", "SQRT", "POWER"];
    for name in &det_fns {
        let f = catalog.lookup(name).unwrap_or_else(|| panic!("Missing {name}"));
        assert!(
            f.properties.deterministic,
            "{name} should be deterministic"
        );
        assert!(f.properties.pure, "{name} should be pure");
        assert!(
            f.properties.constant_foldable,
            "{name} should be constant foldable"
        );
    }
}

#[test]
fn test_random_is_nondeterministic() {
    let catalog = FunctionCatalog::with_builtins();
    let random = catalog.lookup("RANDOM").expect("RANDOM should exist");
    assert!(!random.properties.deterministic);
    assert!(!random.properties.pure);
    assert!(!random.properties.constant_foldable);
}

#[test]
fn test_now_is_nondeterministic() {
    let catalog = FunctionCatalog::with_builtins();
    let now = catalog.lookup("NOW").expect("NOW should exist");
    assert!(!now.properties.deterministic);
    assert!(!now.properties.constant_foldable);
}

#[test]
fn test_current_timestamp_is_nondeterministic() {
    let catalog = FunctionCatalog::with_builtins();
    let ct = catalog
        .lookup("CURRENT_TIMESTAMP")
        .expect("CURRENT_TIMESTAMP should exist");
    assert!(!ct.properties.deterministic);
}

#[test]
fn test_nextval_is_volatile() {
    let catalog = FunctionCatalog::with_builtins();
    let nv = catalog.lookup("NEXTVAL").expect("NEXTVAL should exist");
    assert!(!nv.properties.deterministic);
    assert!(!nv.properties.pure);
    assert!(!nv.properties.constant_foldable);
}

#[test]
fn test_gen_random_uuid_is_volatile() {
    let catalog = FunctionCatalog::with_builtins();
    let uuid = catalog
        .lookup("GEN_RANDOM_UUID")
        .expect("GEN_RANDOM_UUID should exist");
    assert!(!uuid.properties.deterministic);
    assert!(!uuid.properties.pure);
}

// ===========================================================================
// 4. Function properties - expensive
// ===========================================================================

#[test]
fn test_regex_functions_are_expensive() {
    let catalog = FunctionCatalog::with_builtins();
    for name in &["REGEXP_REPLACE", "REGEXP_MATCHES", "REGEXP_SUBSTR"] {
        if let Some(f) = catalog.lookup(name) {
            assert!(
                f.properties.expensive,
                "{name} should be marked expensive"
            );
            assert!(f.properties.cost_multiplier >= 5.0);
        }
    }
}

#[test]
fn test_crypto_functions_are_expensive() {
    let catalog = FunctionCatalog::with_builtins();
    let md5 = catalog.lookup("MD5").expect("MD5 should exist");
    assert!(md5.properties.expensive);
    assert!(md5.properties.cost_multiplier >= 10.0);
}

#[test]
fn test_spatial_functions_are_expensive() {
    let catalog = FunctionCatalog::with_builtins();
    let expensive_geo = ["ST_DISTANCE", "ST_CONTAINS", "ST_BUFFER", "ST_UNION"];
    for name in &expensive_geo {
        let f = catalog.lookup(name).unwrap_or_else(|| panic!("Missing {name}"));
        assert!(
            f.properties.expensive,
            "{name} should be expensive"
        );
        assert!(
            f.properties.cost_multiplier >= 5.0,
            "{name} cost_multiplier should be >= 5.0, got {}",
            f.properties.cost_multiplier
        );
    }
}

#[test]
fn test_text_search_functions_are_expensive() {
    let catalog = FunctionCatalog::with_builtins();
    let ts = catalog
        .lookup("TO_TSVECTOR")
        .expect("TO_TSVECTOR should exist");
    assert!(ts.properties.expensive);
    assert!(ts.properties.cost_multiplier >= 10.0);
}

#[test]
fn test_basic_functions_are_not_expensive() {
    let catalog = FunctionCatalog::with_builtins();
    let cheap = ["ABS", "UPPER", "LOWER", "LENGTH", "SIGN"];
    for name in &cheap {
        let f = catalog.lookup(name).unwrap_or_else(|| panic!("Missing {name}"));
        assert!(
            !f.properties.expensive,
            "{name} should not be expensive"
        );
        assert!(
            f.properties.cost_multiplier <= 2.0,
            "{name} cost should be low"
        );
    }
}

// ===========================================================================
// 5. Function properties - order sensitivity
// ===========================================================================

#[test]
fn test_order_sensitive_aggregates() {
    let catalog = FunctionCatalog::with_builtins();
    let order_sensitive = ["ARRAY_AGG", "JSON_AGG"];
    for name in &order_sensitive {
        let f = catalog.lookup(name).unwrap_or_else(|| panic!("Missing {name}"));
        assert!(
            f.properties.order_sensitive,
            "{name} should be order sensitive"
        );
    }
}

#[test]
fn test_order_insensitive_aggregates() {
    let catalog = FunctionCatalog::with_builtins();
    let order_insensitive = ["COUNT", "SUM", "AVG", "MIN", "MAX"];
    for name in &order_insensitive {
        let f = catalog.lookup(name).unwrap_or_else(|| panic!("Missing {name}"));
        assert!(
            !f.properties.order_sensitive,
            "{name} should not be order sensitive"
        );
    }
}

#[test]
fn test_window_functions_are_order_sensitive() {
    let catalog = FunctionCatalog::with_builtins();
    let windows = ["ROW_NUMBER", "RANK", "DENSE_RANK", "LAG", "LEAD"];
    for name in &windows {
        let f = catalog.lookup(name).unwrap_or_else(|| panic!("Missing {name}"));
        assert!(
            f.properties.order_sensitive,
            "{name} should be order sensitive"
        );
    }
}

// ===========================================================================
// 6. Database system filtering
// ===========================================================================

#[test]
fn test_postgresql_has_most_functions() {
    let catalog = FunctionCatalog::with_builtins();
    let pg = catalog.by_database(DatabaseSystem::PostgreSQL);
    let mysql = catalog.by_database(DatabaseSystem::MySQL);
    let sqlite = catalog.by_database(DatabaseSystem::SQLite);

    assert!(
        pg.len() > mysql.len(),
        "PostgreSQL ({}) should have more functions than MySQL ({})",
        pg.len(),
        mysql.len()
    );
    assert!(
        mysql.len() > sqlite.len(),
        "MySQL ({}) should have more functions than SQLite ({})",
        mysql.len(),
        sqlite.len()
    );
}

#[test]
fn test_pg_exclusive_functions() {
    let catalog = FunctionCatalog::with_builtins();
    let pg_only = [
        "ARRAY_AGG", "JSONB_AGG", "STRING_AGG", "BOOL_AND",
    ];
    for name in &pg_only {
        if let Some(f) = catalog.lookup(name) {
            assert!(
                f.available_in.contains(&DatabaseSystem::PostgreSQL),
                "{name} should be available in PostgreSQL"
            );
        }
    }
}

#[test]
fn test_universal_functions() {
    let catalog = FunctionCatalog::with_builtins();
    let universal = ["COUNT", "SUM", "AVG", "MIN", "MAX", "ABS", "UPPER", "LOWER"];
    for name in &universal {
        let f = catalog.lookup(name).unwrap_or_else(|| panic!("Missing {name}"));
        assert!(
            f.available_in.len() >= 4,
            "{name} should be available in at least 4 databases, got {}",
            f.available_in.len()
        );
    }
}

// ===========================================================================
// 7. Filtering methods
// ===========================================================================

#[test]
fn test_deterministic_filter() {
    let catalog = FunctionCatalog::with_builtins();
    let det = catalog.deterministic_functions();
    assert!(!det.is_empty());
    for f in &det {
        assert!(
            f.properties.deterministic,
            "{} returned by deterministic_functions but is not deterministic",
            f.name
        );
    }
}

#[test]
fn test_expensive_filter() {
    let catalog = FunctionCatalog::with_builtins();
    let exp = catalog.expensive_functions();
    assert!(!exp.is_empty());
    for f in &exp {
        assert!(
            f.properties.expensive,
            "{} returned by expensive_functions but is not expensive",
            f.name
        );
    }
}

#[test]
fn test_pure_filter() {
    let catalog = FunctionCatalog::with_builtins();
    let pure = catalog.pure_functions();
    assert!(!pure.is_empty());
    for f in &pure {
        assert!(
            f.properties.pure,
            "{} returned by pure_functions but is not pure",
            f.name
        );
    }
}

// ===========================================================================
// 8. Custom function registration
// ===========================================================================

#[test]
fn test_register_and_lookup_custom() {
    let mut catalog = FunctionCatalog::new();
    assert!(catalog.is_empty());

    catalog.register(FunctionDefinition {
        name: "MY_CUSTOM_FN".to_string(),
        category: FunctionCategory::Scalar,
        signatures: vec![FunctionSignature {
            parameters: vec![SqlType::Integer, SqlType::Integer],
            return_type: SqlType::Integer,
            variadic: false,
            min_args: 2,
            max_args: Some(2),
        }],
        properties: FunctionProperties {
            deterministic: true,
            pure: true,
            expensive: false,
            cost_multiplier: 1.5,
            ..FunctionProperties::default()
        },
        available_in: vec![DatabaseSystem::PostgreSQL],
        description: "Custom test function".to_string(),
    });

    assert_eq!(catalog.len(), 1);
    let f = catalog.lookup("my_custom_fn").expect("Should find custom fn");
    assert_eq!(f.name, "MY_CUSTOM_FN");
    assert!(f.properties.deterministic);
    assert!((f.properties.cost_multiplier - 1.5).abs() < f64::EPSILON);
}

#[test]
fn test_custom_function_in_filters() {
    let mut catalog = FunctionCatalog::new();
    catalog.register(FunctionDefinition {
        name: "EXPENSIVE_CUSTOM".to_string(),
        category: FunctionCategory::Scalar,
        signatures: vec![],
        properties: FunctionProperties {
            expensive: true,
            cost_multiplier: 20.0,
            ..FunctionProperties::default()
        },
        available_in: vec![DatabaseSystem::PostgreSQL],
        description: "Expensive custom function".to_string(),
    });

    let exp = catalog.expensive_functions();
    assert_eq!(exp.len(), 1);
    assert_eq!(exp[0].name, "EXPENSIVE_CUSTOM");
}

// ===========================================================================
// 9. Cost multiplier ranges
// ===========================================================================

#[test]
fn test_all_cost_multipliers_positive() {
    let catalog = FunctionCatalog::with_builtins();
    for f in catalog.iter() {
        assert!(
            f.properties.cost_multiplier > 0.0,
            "{} has non-positive cost_multiplier: {}",
            f.name,
            f.properties.cost_multiplier
        );
    }
}

#[test]
fn test_expensive_functions_have_high_cost() {
    let catalog = FunctionCatalog::with_builtins();
    for f in catalog.iter() {
        if f.properties.expensive {
            assert!(
                f.properties.cost_multiplier >= 3.0,
                "Expensive function {} has low cost_multiplier: {}",
                f.name,
                f.properties.cost_multiplier
            );
        }
    }
}

#[test]
fn test_table_valued_functions_have_cardinality() {
    let catalog = FunctionCatalog::with_builtins();
    let tv = catalog.by_category(FunctionCategory::TableValued);
    for f in &tv {
        assert!(
            f.properties.cardinality_ratio.is_some(),
            "Table-valued function {} should have cardinality_ratio",
            f.name
        );
    }
}

// ===========================================================================
// 10. Constant folding candidates
// ===========================================================================

#[test]
fn test_constant_foldable_functions_are_deterministic() {
    let catalog = FunctionCatalog::with_builtins();
    for f in catalog.iter() {
        if f.properties.constant_foldable {
            assert!(
                f.properties.deterministic,
                "{} is constant_foldable but not deterministic",
                f.name
            );
        }
    }
}

#[test]
fn test_aggregates_not_constant_foldable() {
    let catalog = FunctionCatalog::with_builtins();
    let aggs = catalog.by_category(FunctionCategory::Aggregate);
    for f in &aggs {
        assert!(
            !f.properties.constant_foldable,
            "Aggregate {} should not be constant_foldable",
            f.name
        );
    }
}

#[test]
fn test_window_functions_not_constant_foldable() {
    let catalog = FunctionCatalog::with_builtins();
    let wins = catalog.by_category(FunctionCategory::Window);
    for f in &wins {
        assert!(
            !f.properties.constant_foldable,
            "Window function {} should not be constant_foldable",
            f.name
        );
    }
}

// ===========================================================================
// 11. Iterator and default
// ===========================================================================

#[test]
fn test_default_catalog_is_builtin() {
    let default_catalog = FunctionCatalog::default();
    let builtin_catalog = FunctionCatalog::with_builtins();
    assert_eq!(default_catalog.len(), builtin_catalog.len());
}

#[test]
fn test_iter_covers_all_functions() {
    let catalog = FunctionCatalog::with_builtins();
    let count = catalog.iter().count();
    assert_eq!(count, catalog.len());
}
