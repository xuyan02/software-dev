use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

/// Declared type of a variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariableType {
    Null,
    Bool,
    Int,
    Float,
    String,
    Array,
    /// Object fields. An empty map means any object shape is accepted.
    Object(BTreeMap<String, VariableType>),
}

/// A named, typed variable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    name: String,
    ty: VariableType,
    value: Value,
}

impl Variable {
    /// Infer the type from the JSON value.
    pub fn new(name: impl Into<String>, value: Value) -> Result<Self, VariableError> {
        let name = name.into();
        if !is_identifier(&name) {
            return Err(VariableError::InvalidName(name));
        }

        let ty = infer_type(&value);
        Ok(Self { name, ty, value })
    }

    /// Declare an explicit type and validate the value against it.
    pub fn typed(
        name: impl Into<String>,
        ty: VariableType,
        value: Value,
    ) -> Result<Self, VariableError> {
        let name = name.into();
        if !is_identifier(&name) {
            return Err(VariableError::InvalidName(name));
        }

        validate_type(&ty, &value)?;
        Ok(Self { name, ty, value })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn ty(&self) -> &VariableType {
        &self.ty
    }

    pub fn value(&self) -> &Value {
        &self.value
    }
}

/// A source of variables outside the graph module.
///
/// Sources are synchronous snapshots or in-memory views. Load external data
/// before starting a run and wrap it in a source; graph execution and routing
/// are synchronous.
pub trait VariableSource: Send + Sync {
    /// Returns `Ok(None)` when this source does not provide the variable.
    fn get(&self, name: &str) -> Result<Option<Variable>, VariableError>;
}

/// A simple [`VariableSource`] backed by a map of typed variables.
#[derive(Debug, Clone, Default)]
pub struct MapSource {
    values: BTreeMap<String, Variable>,
}

impl MapSource {
    pub fn new(values: impl IntoIterator<Item = Variable>) -> Self {
        Self {
            values: values
                .into_iter()
                .map(|variable| (variable.name.clone(), variable))
                .collect(),
        }
    }

    pub fn from_json<K>(values: impl IntoIterator<Item = (K, Value)>) -> Result<Self, VariableError>
    where
        K: Into<String>,
    {
        let mut source = Self::default();
        for (name, value) in values {
            let variable = Variable::new(name, value)?;
            source.values.insert(variable.name.clone(), variable);
        }
        Ok(source)
    }
}

impl VariableSource for MapSource {
    fn get(&self, name: &str) -> Result<Option<Variable>, VariableError> {
        Ok(self.values.get(name).cloned())
    }
}

/// A parsed variable path such as `task.title` or
/// `nodes.load.output.requirement`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VariablePath {
    segments: Vec<String>,
}

impl VariablePath {
    pub fn parse(input: &str) -> Result<Self, VariableError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(VariableError::InvalidPath(input.to_owned()));
        }

        let mut segments = Vec::new();
        for segment in input.split('.') {
            let segment = segment.trim();
            if !is_identifier(segment) {
                return Err(VariableError::InvalidPath(input.to_owned()));
            }
            segments.push(segment.to_owned());
        }

        Ok(Self { segments })
    }

    pub fn segments(&self) -> &[String] {
        &self.segments
    }
}

impl fmt::Display for VariablePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.segments.join("."))
    }
}

impl std::str::FromStr for VariablePath {
    type Err = VariableError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

pub(crate) fn traverse(
    variable: &Variable,
    path: &[String],
    full_path: &str,
) -> Result<Variable, VariableError> {
    let mut current = variable.clone();

    for segment in path {
        let fields = match &current.ty {
            VariableType::Object(fields) => fields,
            _ => {
                return Err(VariableError::NotObject {
                    path: full_path.to_owned(),
                    segment: segment.clone(),
                });
            }
        };

        let object = current
            .value
            .as_object()
            .ok_or_else(|| VariableError::NotObject {
                path: full_path.to_owned(),
                segment: segment.clone(),
            })?;

        let value = object
            .get(segment)
            .ok_or_else(|| VariableError::Unknown(full_path.to_owned()))?;

        let ty = fields
            .get(segment)
            .cloned()
            .unwrap_or_else(|| infer_type(value));

        current = Variable {
            name: full_path.to_owned(),
            ty,
            value: value.clone(),
        };
    }

    Ok(current)
}

pub(crate) fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }

    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn infer_type(value: &Value) -> VariableType {
    match value {
        Value::Null => VariableType::Null,
        Value::Bool(_) => VariableType::Bool,
        Value::Number(number) if number.is_i64() || number.is_u64() => VariableType::Int,
        Value::Number(_) => VariableType::Float,
        Value::String(_) => VariableType::String,
        Value::Array(_) => VariableType::Array,
        Value::Object(map) => VariableType::Object(
            map.iter()
                .map(|(name, value)| (name.clone(), infer_type(value)))
                .collect(),
        ),
    }
}

fn validate_type(ty: &VariableType, value: &Value) -> Result<(), VariableError> {
    let matches = match (ty, value) {
        (VariableType::Null, Value::Null) => true,
        (VariableType::Bool, Value::Bool(_)) => true,
        (VariableType::Int, Value::Number(number)) => number.is_i64() || number.is_u64(),
        (VariableType::Float, Value::Number(_)) => true,
        (VariableType::String, Value::String(_)) => true,
        (VariableType::Array, Value::Array(_)) => true,
        (VariableType::Object(fields), Value::Object(map)) => {
            for (name, field_ty) in fields {
                let field = map.get(name).ok_or_else(|| VariableError::TypeMismatch {
                    expected: ty.clone(),
                    actual: infer_type(value),
                })?;
                validate_type(field_ty, field)?;
            }
            true
        }
        _ => false,
    };

    if matches {
        Ok(())
    } else {
        Err(VariableError::TypeMismatch {
            expected: ty.clone(),
            actual: infer_type(value),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VariableError {
    #[error("invalid variable name: {0}")]
    InvalidName(String),

    #[error("invalid variable path: {0}")]
    InvalidPath(String),

    #[error("unknown variable: {0}")]
    Unknown(String),

    #[error("cannot traverse {segment} in non-object value at {path}")]
    NotObject { path: String, segment: String },

    #[error("type mismatch: expected {expected:?}, got {actual:?}")]
    TypeMismatch {
        expected: VariableType,
        actual: VariableType,
    },

    #[error("variable source error: {0}")]
    Source(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_variable_paths() {
        let path = VariablePath::parse("nodes.load.output.requirement").unwrap();
        assert_eq!(path.segments(), ["nodes", "load", "output", "requirement"]);

        assert!(VariablePath::parse("").is_err());
        assert!(VariablePath::parse("task..title").is_err());
        assert!(VariablePath::parse("1task").is_err());
    }

    #[test]
    fn infers_variable_types() {
        assert_eq!(
            Variable::new("task", json!("login")).unwrap().ty(),
            &VariableType::String
        );
        assert_eq!(
            Variable::new("count", json!(3)).unwrap().ty(),
            &VariableType::Int
        );
        assert_eq!(
            Variable::new("ratio", json!(0.5)).unwrap().ty(),
            &VariableType::Float
        );
        assert_eq!(
            Variable::new("enabled", json!(true)).unwrap().ty(),
            &VariableType::Bool
        );
        assert_eq!(
            Variable::new("items", json!([1, 2])).unwrap().ty(),
            &VariableType::Array
        );
    }

    #[test]
    fn validates_explicit_types() {
        let variable = Variable::typed(
            "project",
            VariableType::Object(BTreeMap::from([("name".to_owned(), VariableType::String)])),
            json!({"name": "foundry"}),
        )
        .unwrap();

        assert_eq!(variable.name(), "project");

        let error = Variable::typed(
            "project",
            VariableType::Object(BTreeMap::from([("name".to_owned(), VariableType::String)])),
            json!({"name": 3}),
        )
        .unwrap_err();

        assert!(matches!(error, VariableError::TypeMismatch { .. }));
    }

    #[test]
    fn map_source_resolves_named_variables() {
        let source = MapSource::from_json([
            ("project".to_owned(), json!({"name": "foundry"})),
            ("count".to_owned(), json!(3)),
        ])
        .unwrap();

        assert_eq!(
            source.get("project").unwrap().unwrap().value(),
            &json!({"name": "foundry"})
        );
        assert_eq!(
            source.get("count").unwrap().unwrap().ty(),
            &VariableType::Int
        );
        assert!(source.get("missing").unwrap().is_none());
    }
}
