//! Executor for table-valued functions.
//!
//! Implements set-returning functions like `generate_series`,
//! `json_array_elements`, and `json_each`. Each function produces
//! a set of rows from its arguments.

use ra_core::expr::{Const, Expr};

use crate::recursive::{ExecutionError, Row};

/// Executor for set-returning table functions.
#[derive(Debug)]
pub struct TableFunctionExecutor {
    /// Function name (lowercase).
    name: String,
    /// Function arguments as expressions.
    args: Vec<Expr>,
}

impl TableFunctionExecutor {
    /// Create a table function executor.
    #[must_use]
    pub fn new(name: impl Into<String>, args: Vec<Expr>) -> Self {
        Self {
            name: name.into(),
            args,
        }
    }

    /// Execute the table function, producing output rows.
    ///
    /// # Errors
    ///
    /// Returns an error if the function is unsupported or
    /// arguments are invalid.
    pub fn execute(
        &self,
        _input: Option<&[Row]>,
    ) -> Result<Vec<Row>, ExecutionError> {
        match self.name.as_str() {
            "generate_series" => self.execute_generate_series(),
            "json_array_elements"
            | "json_array_elements_text" => {
                self.execute_json_array_elements()
            }
            "json_each" | "jsonb_each" => {
                self.execute_json_each()
            }
            "json_object_keys"
            | "jsonb_object_keys" => {
                self.execute_json_object_keys()
            }
            "json_populate_recordset" => {
                self.execute_json_populate_recordset()
            }
            "json_to_recordset"
            | "jsonb_to_recordset" => {
                self.execute_json_to_recordset()
            }
            "jsonb_array_elements"
            | "jsonb_array_elements_text" => {
                self.execute_json_array_elements()
            }
            other => Err(ExecutionError::EvalError(format!(
                "unsupported table function: {other}"
            ))),
        }
    }

    /// Execute `generate_series(start, stop[, step])`.
    ///
    /// Produces integer rows from `start` to `stop` inclusive,
    /// incrementing by `step` (default 1).
    fn execute_generate_series(
        &self,
    ) -> Result<Vec<Row>, ExecutionError> {
        if self.args.len() < 2 {
            return Err(ExecutionError::EvalError(
                "generate_series requires at least 2 arguments"
                    .to_owned(),
            ));
        }

        let start = eval_arg_as_int(&self.args[0])?;
        let stop = eval_arg_as_int(&self.args[1])?;
        let step = if self.args.len() >= 3 {
            eval_arg_as_int(&self.args[2])?
        } else {
            1
        };

        if step == 0 {
            return Err(ExecutionError::EvalError(
                "generate_series step cannot be zero".to_owned(),
            ));
        }

        let mut rows = Vec::new();
        let mut current = start;

        // Limit output to prevent runaway generation.
        const MAX_ROWS: usize = 1_000_000;

        while (step > 0 && current <= stop)
            || (step < 0 && current >= stop)
        {
            if rows.len() >= MAX_ROWS {
                return Err(ExecutionError::EvalError(format!(
                    "generate_series exceeded {MAX_ROWS} row limit"
                )));
            }
            rows.push(Row::new(vec![Const::Int(current)]));
            current += step;
        }

        Ok(rows)
    }

    /// Execute `json_array_elements(json_text)`.
    ///
    /// Parses a JSON array string and produces one row per element.
    fn execute_json_array_elements(
        &self,
    ) -> Result<Vec<Row>, ExecutionError> {
        if self.args.is_empty() {
            return Err(ExecutionError::EvalError(
                "json_array_elements requires 1 argument"
                    .to_owned(),
            ));
        }
        let json_str = eval_arg_as_string(&self.args[0])?;
        let parsed: serde_json::Value =
            serde_json::from_str(&json_str).map_err(|e| {
                ExecutionError::EvalError(format!(
                    "invalid JSON: {e}"
                ))
            })?;

        match parsed {
            serde_json::Value::Array(elements) => {
                let mut rows =
                    Vec::with_capacity(elements.len());
                for elem in elements {
                    let value = json_value_to_const(&elem);
                    rows.push(Row::new(vec![value]));
                }
                Ok(rows)
            }
            _ => Err(ExecutionError::EvalError(
                "json_array_elements requires a JSON array"
                    .to_owned(),
            )),
        }
    }

    /// Execute `json_each(json_text)`.
    ///
    /// Parses a JSON object and produces one row per key-value pair.
    fn execute_json_each(
        &self,
    ) -> Result<Vec<Row>, ExecutionError> {
        if self.args.is_empty() {
            return Err(ExecutionError::EvalError(
                "json_each requires 1 argument".to_owned(),
            ));
        }
        let json_str = eval_arg_as_string(&self.args[0])?;
        let parsed: serde_json::Value =
            serde_json::from_str(&json_str).map_err(|e| {
                ExecutionError::EvalError(format!(
                    "invalid JSON: {e}"
                ))
            })?;

        match parsed {
            serde_json::Value::Object(map) => {
                let mut rows = Vec::with_capacity(map.len());
                for (key, val) in map {
                    let key_const =
                        Const::String(key);
                    let val_const =
                        json_value_to_const(&val);
                    rows.push(Row::new(vec![
                        key_const, val_const,
                    ]));
                }
                Ok(rows)
            }
            _ => Err(ExecutionError::EvalError(
                "json_each requires a JSON object".to_owned(),
            )),
        }
    }

    /// Execute `json_object_keys(json_text)`.
    ///
    /// Parses a JSON object and produces one row per key.
    fn execute_json_object_keys(
        &self,
    ) -> Result<Vec<Row>, ExecutionError> {
        if self.args.is_empty() {
            return Err(ExecutionError::EvalError(
                "json_object_keys requires 1 argument"
                    .to_owned(),
            ));
        }
        let json_str = eval_arg_as_string(&self.args[0])?;
        let parsed: serde_json::Value =
            serde_json::from_str(&json_str).map_err(|e| {
                ExecutionError::EvalError(format!(
                    "invalid JSON: {e}"
                ))
            })?;

        match parsed {
            serde_json::Value::Object(map) => {
                let mut rows = Vec::with_capacity(map.len());
                for key in map.keys() {
                    rows.push(Row::new(vec![Const::String(
                        key.clone(),
                    )]));
                }
                Ok(rows)
            }
            _ => Err(ExecutionError::EvalError(
                "json_object_keys requires a JSON object"
                    .to_owned(),
            )),
        }
    }

    /// Execute `json_populate_recordset(json_array, type_def)`.
    ///
    /// Parses a JSON array of objects and produces typed rows.
    /// The type definition string `(col1 type1, col2 type2, ...)`
    /// determines column order and coercion.
    fn execute_json_populate_recordset(
        &self,
    ) -> Result<Vec<Row>, ExecutionError> {
        if self.args.len() < 2 {
            return Err(ExecutionError::EvalError(
                "json_populate_recordset requires 2 arguments"
                    .to_owned(),
            ));
        }
        let json_str = eval_arg_as_string(&self.args[0])?;
        let type_def = eval_arg_as_string(&self.args[1])?;

        let col_names = parse_type_definition(&type_def)?;
        let parsed: serde_json::Value =
            serde_json::from_str(&json_str).map_err(|e| {
                ExecutionError::EvalError(format!(
                    "invalid JSON: {e}"
                ))
            })?;

        let serde_json::Value::Array(elements) = parsed else {
            return Err(ExecutionError::EvalError(
                "json_populate_recordset requires a JSON array"
                    .to_owned(),
            ));
        };

        let mut rows = Vec::with_capacity(elements.len());
        for elem in &elements {
            let serde_json::Value::Object(obj) = elem else {
                return Err(ExecutionError::EvalError(
                    "json_populate_recordset: each element \
                     must be a JSON object"
                        .to_owned(),
                ));
            };
            let mut values = Vec::with_capacity(col_names.len());
            for (col_name, col_type) in &col_names {
                let val = obj
                    .get(col_name.as_str())
                    .map(|v| {
                        coerce_json_value(v, col_type)
                    })
                    .unwrap_or(Const::Null);
                values.push(val);
            }
            rows.push(Row::new(values));
        }

        Ok(rows)
    }

    /// Execute `json_to_recordset(json_array)`.
    ///
    /// Parses a JSON array of objects and infers column types
    /// from the first object's keys. Produces one row per object.
    fn execute_json_to_recordset(
        &self,
    ) -> Result<Vec<Row>, ExecutionError> {
        if self.args.is_empty() {
            return Err(ExecutionError::EvalError(
                "json_to_recordset requires 1 argument"
                    .to_owned(),
            ));
        }
        let json_str = eval_arg_as_string(&self.args[0])?;
        let parsed: serde_json::Value =
            serde_json::from_str(&json_str).map_err(|e| {
                ExecutionError::EvalError(format!(
                    "invalid JSON: {e}"
                ))
            })?;

        let serde_json::Value::Array(elements) = parsed else {
            return Err(ExecutionError::EvalError(
                "json_to_recordset requires a JSON array"
                    .to_owned(),
            ));
        };

        if elements.is_empty() {
            return Ok(vec![]);
        }

        // Infer column names from first object.
        let serde_json::Value::Object(first) = &elements[0]
        else {
            return Err(ExecutionError::EvalError(
                "json_to_recordset: elements must be objects"
                    .to_owned(),
            ));
        };
        let col_names: Vec<String> =
            first.keys().cloned().collect();

        let mut rows = Vec::with_capacity(elements.len());
        for elem in &elements {
            let serde_json::Value::Object(obj) = elem else {
                return Err(ExecutionError::EvalError(
                    "json_to_recordset: each element \
                     must be a JSON object"
                        .to_owned(),
                ));
            };
            let mut values = Vec::with_capacity(col_names.len());
            for col in &col_names {
                let val = obj
                    .get(col.as_str())
                    .map(json_value_to_const)
                    .unwrap_or(Const::Null);
                values.push(val);
            }
            rows.push(Row::new(values));
        }

        Ok(rows)
    }
}

/// Evaluate an expression as an integer.
fn eval_arg_as_int(
    expr: &Expr,
) -> Result<i64, ExecutionError> {
    match expr {
        Expr::Const(Const::Int(i)) => Ok(*i),
        Expr::Const(Const::Float(f)) => {
            Ok(*f as i64)
        }
        other => Err(ExecutionError::EvalError(format!(
            "expected integer argument, got: {other:?}"
        ))),
    }
}

/// Evaluate an expression as a string.
fn eval_arg_as_string(
    expr: &Expr,
) -> Result<String, ExecutionError> {
    match expr {
        Expr::Const(Const::String(s)) => Ok(s.clone()),
        other => Err(ExecutionError::EvalError(format!(
            "expected string argument, got: {other:?}"
        ))),
    }
}

/// Parse a type definition string like `(id int, name text)`.
///
/// Returns `(column_name, type_name)` pairs.
fn parse_type_definition(
    def: &str,
) -> Result<Vec<(String, String)>, ExecutionError> {
    let trimmed = def.trim();
    let inner = trimmed
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(trimmed);

    let mut cols = Vec::new();
    for part in inner.split(',') {
        let tokens: Vec<&str> =
            part.trim().split_whitespace().collect();
        if tokens.len() < 2 {
            return Err(ExecutionError::EvalError(format!(
                "invalid type definition part: {part}"
            )));
        }
        cols.push((
            tokens[0].to_owned(),
            tokens[1..].join(" ").to_lowercase(),
        ));
    }
    Ok(cols)
}

/// Coerce a JSON value to a [`Const`] based on a target type.
fn coerce_json_value(
    val: &serde_json::Value,
    target_type: &str,
) -> Const {
    match target_type {
        "int" | "integer" | "int4" | "int8" | "bigint" => {
            match val {
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        Const::Int(i)
                    } else {
                        Const::Int(
                            n.as_f64().unwrap_or(0.0) as i64
                        )
                    }
                }
                serde_json::Value::String(s) => {
                    s.parse::<i64>()
                        .map_or(Const::Null, Const::Int)
                }
                serde_json::Value::Null => Const::Null,
                _ => Const::Null,
            }
        }
        "float" | "double" | "real" | "numeric"
        | "float4" | "float8" => match val {
            serde_json::Value::Number(n) => {
                Const::Float(n.as_f64().unwrap_or(0.0))
            }
            serde_json::Value::String(s) => {
                s.parse::<f64>()
                    .map_or(Const::Null, Const::Float)
            }
            serde_json::Value::Null => Const::Null,
            _ => Const::Null,
        },
        "bool" | "boolean" => match val {
            serde_json::Value::Bool(b) => Const::Bool(*b),
            serde_json::Value::Null => Const::Null,
            _ => Const::Null,
        },
        _ => json_value_to_const(val),
    }
}

/// Convert a serde JSON value to a [`Const`].
fn json_value_to_const(val: &serde_json::Value) -> Const {
    match val {
        serde_json::Value::Null => Const::Null,
        serde_json::Value::Bool(b) => Const::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Const::Int(i)
            } else {
                Const::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => {
            Const::String(s.clone())
        }
        other => Const::String(other.to_string()),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn generate_series_basic() {
        let exec = TableFunctionExecutor::new(
            "generate_series",
            vec![
                Expr::Const(Const::Int(1)),
                Expr::Const(Const::Int(5)),
            ],
        );
        let rows =
            exec.execute(None).expect("should succeed");
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].values, vec![Const::Int(1)]);
        assert_eq!(rows[4].values, vec![Const::Int(5)]);
    }

    #[test]
    fn generate_series_with_step() {
        let exec = TableFunctionExecutor::new(
            "generate_series",
            vec![
                Expr::Const(Const::Int(0)),
                Expr::Const(Const::Int(10)),
                Expr::Const(Const::Int(3)),
            ],
        );
        let rows =
            exec.execute(None).expect("should succeed");
        assert_eq!(rows.len(), 4); // 0, 3, 6, 9
        assert_eq!(rows[0].values, vec![Const::Int(0)]);
        assert_eq!(rows[1].values, vec![Const::Int(3)]);
        assert_eq!(rows[2].values, vec![Const::Int(6)]);
        assert_eq!(rows[3].values, vec![Const::Int(9)]);
    }

    #[test]
    fn generate_series_descending() {
        let exec = TableFunctionExecutor::new(
            "generate_series",
            vec![
                Expr::Const(Const::Int(5)),
                Expr::Const(Const::Int(1)),
                Expr::Const(Const::Int(-1)),
            ],
        );
        let rows =
            exec.execute(None).expect("should succeed");
        assert_eq!(rows.len(), 5); // 5,4,3,2,1
        assert_eq!(rows[0].values, vec![Const::Int(5)]);
        assert_eq!(rows[4].values, vec![Const::Int(1)]);
    }

    #[test]
    fn generate_series_empty_range() {
        let exec = TableFunctionExecutor::new(
            "generate_series",
            vec![
                Expr::Const(Const::Int(5)),
                Expr::Const(Const::Int(1)),
                Expr::Const(Const::Int(1)),
            ],
        );
        let rows =
            exec.execute(None).expect("should succeed");
        assert!(rows.is_empty());
    }

    #[test]
    fn generate_series_zero_step_error() {
        let exec = TableFunctionExecutor::new(
            "generate_series",
            vec![
                Expr::Const(Const::Int(1)),
                Expr::Const(Const::Int(5)),
                Expr::Const(Const::Int(0)),
            ],
        );
        let result = exec.execute(None);
        assert!(result.is_err());
    }

    #[test]
    fn json_array_elements_basic() {
        let exec = TableFunctionExecutor::new(
            "json_array_elements",
            vec![Expr::Const(Const::String(
                "[1,2,3]".into(),
            ))],
        );
        let rows =
            exec.execute(None).expect("should succeed");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].values, vec![Const::Int(1)]);
        assert_eq!(rows[1].values, vec![Const::Int(2)]);
        assert_eq!(rows[2].values, vec![Const::Int(3)]);
    }

    #[test]
    fn json_array_elements_mixed_types() {
        let exec = TableFunctionExecutor::new(
            "json_array_elements",
            vec![Expr::Const(Const::String(
                r#"[1, "hello", null, true]"#.into(),
            ))],
        );
        let rows =
            exec.execute(None).expect("should succeed");
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].values, vec![Const::Int(1)]);
        assert_eq!(
            rows[1].values,
            vec![Const::String("hello".into())]
        );
        assert_eq!(rows[2].values, vec![Const::Null]);
        assert_eq!(rows[3].values, vec![Const::Bool(true)]);
    }

    #[test]
    fn json_each_basic() {
        let exec = TableFunctionExecutor::new(
            "json_each",
            vec![Expr::Const(Const::String(
                r#"{"a":1,"b":"two"}"#.into(),
            ))],
        );
        let rows =
            exec.execute(None).expect("should succeed");
        assert_eq!(rows.len(), 2);
        // JSON object iteration order is not guaranteed,
        // so check that both keys appear.
        let keys: Vec<&Const> =
            rows.iter().map(|r| &r.values[0]).collect();
        assert!(keys.contains(&&Const::String("a".into())));
        assert!(keys.contains(&&Const::String("b".into())));
    }

    #[test]
    fn json_object_keys_basic() {
        let exec = TableFunctionExecutor::new(
            "json_object_keys",
            vec![Expr::Const(Const::String(
                r#"{"x":1,"y":2,"z":3}"#.into(),
            ))],
        );
        let rows =
            exec.execute(None).expect("should succeed");
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn unsupported_function_error() {
        let exec = TableFunctionExecutor::new(
            "unknown_func",
            vec![],
        );
        let result = exec.execute(None);
        assert!(result.is_err());
    }

    #[test]
    fn json_populate_recordset_basic() {
        let exec = TableFunctionExecutor::new(
            "json_populate_recordset",
            vec![
                Expr::Const(Const::String(
                    r#"[{"id":1,"name":"Alice"},{"id":2,"name":"Bob"}]"#
                        .into(),
                )),
                Expr::Const(Const::String(
                    "(id int, name text)".into(),
                )),
            ],
        );
        let rows =
            exec.execute(None).expect("should succeed");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].values[0], Const::Int(1));
        assert_eq!(
            rows[0].values[1],
            Const::String("Alice".into())
        );
        assert_eq!(rows[1].values[0], Const::Int(2));
        assert_eq!(
            rows[1].values[1],
            Const::String("Bob".into())
        );
    }

    #[test]
    fn json_populate_recordset_missing_field() {
        let exec = TableFunctionExecutor::new(
            "json_populate_recordset",
            vec![
                Expr::Const(Const::String(
                    r#"[{"id":1}]"#.into(),
                )),
                Expr::Const(Const::String(
                    "(id int, name text)".into(),
                )),
            ],
        );
        let rows =
            exec.execute(None).expect("should succeed");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].values[0], Const::Int(1));
        assert_eq!(rows[0].values[1], Const::Null);
    }

    #[test]
    fn json_to_recordset_basic() {
        let exec = TableFunctionExecutor::new(
            "json_to_recordset",
            vec![Expr::Const(Const::String(
                r#"[{"a":1,"b":"x"},{"a":2,"b":"y"}]"#
                    .into(),
            ))],
        );
        let rows =
            exec.execute(None).expect("should succeed");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].values.len(), 2);
    }

    #[test]
    fn json_to_recordset_empty_array() {
        let exec = TableFunctionExecutor::new(
            "json_to_recordset",
            vec![Expr::Const(Const::String("[]".into()))],
        );
        let rows =
            exec.execute(None).expect("should succeed");
        assert!(rows.is_empty());
    }

    #[test]
    fn jsonb_array_elements_basic() {
        let exec = TableFunctionExecutor::new(
            "jsonb_array_elements",
            vec![Expr::Const(Const::String(
                r#"[10,20,30]"#.into(),
            ))],
        );
        let rows =
            exec.execute(None).expect("should succeed");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].values, vec![Const::Int(10)]);
        assert_eq!(rows[2].values, vec![Const::Int(30)]);
    }
}
