use crate::{Context, VariableError, VariablePath};
use serde_json::Value;

/// A prompt template with `{{name}}` / `{{nodes.id.output.field}}`
/// substitutions.
///
/// Supported substitutions are variable lookup only. There are no expressions,
/// conditionals, loops, or function calls. Literal `{{` is not escapable in the
/// first version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTemplate {
    source: String,
    parts: Vec<TemplatePart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TemplatePart {
    Literal(String),
    Variable(VariablePath),
}

impl PromptTemplate {
    pub fn new(input: impl Into<String>) -> Result<Self, TemplateError> {
        Self::parse(input)
    }

    pub fn parse(input: impl Into<String>) -> Result<Self, TemplateError> {
        let source = input.into();
        let mut parts = Vec::new();
        let mut rest = source.as_str();

        while let Some(start) = rest.find("{{") {
            let literal = &rest[..start];
            if !literal.is_empty() {
                parts.push(TemplatePart::Literal(literal.to_owned()));
            }

            let after = &rest[start + 2..];
            let end = after
                .find("}}")
                .ok_or(TemplateError::UnclosedVariable(start))?;
            let raw = after[..end].trim();
            if raw.is_empty() {
                return Err(TemplateError::EmptyVariable);
            }

            parts.push(TemplatePart::Variable(VariablePath::parse(raw)?));
            rest = &after[end + 2..];
        }

        if !rest.is_empty() {
            parts.push(TemplatePart::Literal(rest.to_owned()));
        }

        Ok(Self { source, parts })
    }

    pub fn as_str(&self) -> &str {
        &self.source
    }

    pub fn render(&self, ctx: &Context) -> Result<String, TemplateError> {
        let mut output = String::with_capacity(self.source.len());

        for part in &self.parts {
            match part {
                TemplatePart::Literal(text) => output.push_str(text),
                TemplatePart::Variable(path) => {
                    let variable = ctx.resolve_variable(path)?;
                    output.push_str(&render_value(variable.value())?);
                }
            }
        }

        Ok(output)
    }
}

fn render_value(value: &Value) -> Result<String, TemplateError> {
    Ok(match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value)?,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("unclosed variable starting at byte {0}")]
    UnclosedVariable(usize),

    #[error("empty variable")]
    EmptyVariable,

    #[error(transparent)]
    Variable(#[from] VariableError),

    #[error("template json: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Context, NodeId, VariableError};
    use serde_json::json;

    fn context() -> Context {
        let mut ctx = Context::new();
        ctx.set_var("task", "login").unwrap();
        ctx.set_var("meta", json!({"priority": 3, "tags": ["a", "b"]}))
            .unwrap();
        ctx.set_var("empty", serde_json::Value::Null).unwrap();
        ctx.set_current_node(Some(NodeId::new("load")));
        ctx.set_output(json!({"requirement": "auth"})).unwrap();
        ctx
    }

    #[test]
    fn renders_vars_and_node_outputs() {
        let template =
            PromptTemplate::parse("任务：{{ task }}\n需求：{{nodes.load.output.requirement}}")
                .unwrap();

        assert_eq!(
            template.render(&context()).unwrap(),
            "任务：login\n需求：auth"
        );
    }

    #[test]
    fn renders_supported_value_types() {
        let template = PromptTemplate::parse("{{meta.priority}}|{{meta.tags}}|{{empty}}").unwrap();

        assert_eq!(template.render(&context()).unwrap(), "3|[\"a\",\"b\"]|");
    }

    #[test]
    fn fails_on_missing_variable() {
        let template = PromptTemplate::parse("{{missing}}").unwrap();
        let error = template.render(&context()).unwrap_err();

        assert!(matches!(
            error,
            TemplateError::Variable(VariableError::Unknown(_))
        ));
    }

    #[test]
    fn fails_on_unclosed_variable() {
        let error = PromptTemplate::parse("{{task").unwrap_err();
        assert!(matches!(error, TemplateError::UnclosedVariable(_)));
    }

    #[test]
    fn fails_on_empty_variable() {
        let error = PromptTemplate::parse("{{  }}").unwrap_err();
        assert!(matches!(error, TemplateError::EmptyVariable));
    }
}
