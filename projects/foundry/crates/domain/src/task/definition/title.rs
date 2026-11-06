use super::validate::{ValidationIssue, is_valid_key};
use crate::{ChildState, TaskError};

pub const MAX_TITLE_TEMPLATE_LEN: usize = 500;
pub const MAX_GENERATED_TITLE_LEN: usize = 200;

/// A title template such as `"Iteration {count_children('iteration') + 1}"`.
///
/// It is serialized as its source string, so the AST stays human-readable and
/// version diffs show the template the author wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleTemplate {
    raw: String,
    segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Text(String),
    TaskTitle,
    Count { workflow: String, offset: i64 },
}

pub struct TitleContext<'a> {
    pub task_title: &'a str,
    pub children: &'a [ChildState],
}

impl TitleTemplate {
    pub fn parse(raw: impl Into<String>) -> Result<Self, String> {
        let raw = raw.into();
        if raw.chars().count() > MAX_TITLE_TEMPLATE_LEN {
            return Err(format!(
                "title template is longer than {MAX_TITLE_TEMPLATE_LEN} characters"
            ));
        }
        let mut segments = Vec::new();
        let mut text = String::new();
        let mut chars = raw.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    text.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    text.push('}');
                }
                '{' => {
                    if !text.is_empty() {
                        segments.push(Segment::Text(std::mem::take(&mut text)));
                    }
                    let mut expression = String::new();
                    let mut closed = false;
                    for inner in chars.by_ref() {
                        if inner == '}' {
                            closed = true;
                            break;
                        }
                        expression.push(inner);
                    }
                    if !closed {
                        return Err("unclosed { in title template".to_string());
                    }
                    segments.push(parse_expression(&expression)?);
                }
                '}' => return Err("unmatched } in title template".to_string()),
                other => text.push(other),
            }
        }
        if !text.is_empty() {
            segments.push(Segment::Text(text));
        }
        Ok(Self { raw, segments })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn render(&self, ctx: &TitleContext<'_>) -> Result<String, TaskError> {
        let mut title = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Text(text) => title.push_str(text),
                Segment::TaskTitle => title.push_str(ctx.task_title),
                Segment::Count { workflow, offset } => {
                    let count = ctx
                        .children
                        .iter()
                        .filter(|child| child.workflow.as_str() == workflow)
                        .count() as i64;
                    title.push_str(&(count + offset).to_string());
                }
            }
        }
        if title.trim().is_empty() {
            return Err(TaskError::Workflow(
                "title template produced an empty title".to_string(),
            ));
        }
        if title.chars().count() > MAX_GENERATED_TITLE_LEN {
            return Err(TaskError::Workflow(format!(
                "title template produced a title longer than {MAX_GENERATED_TITLE_LEN} characters"
            )));
        }
        Ok(title)
    }

    pub(crate) fn validate(
        &self,
        is_known: &dyn Fn(&str) -> bool,
        path: &str,
        errors: &mut Vec<ValidationIssue>,
    ) {
        for (index, segment) in self.segments.iter().enumerate() {
            let segment_path = format!("{path}.segments[{index}]");
            match segment {
                Segment::Text(_) | Segment::TaskTitle => {}
                Segment::Count { workflow, .. } => {
                    if !is_valid_key(workflow) {
                        errors.push(ValidationIssue::error(
                            &segment_path,
                            "invalid_workflow_id",
                            format!(
                                "count_children argument {workflow:?} is not a valid workflow id"
                            ),
                        ));
                    } else if !is_known(workflow) {
                        errors.push(ValidationIssue::error(
                            &segment_path,
                            "unknown_workflow",
                            format!("count_children references unknown workflow {workflow:?}"),
                        ));
                    }
                }
            }
        }
    }
}

impl serde::Serialize for TitleTemplate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.raw)
    }
}

impl<'de> serde::Deserialize<'de> for TitleTemplate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(raw).map_err(serde::de::Error::custom)
    }
}

fn parse_expression(expression: &str) -> Result<Segment, String> {
    match expression.trim() {
        "task.title" => Ok(Segment::TaskTitle),
        other => parse_count(other),
    }
}

fn parse_count(expression: &str) -> Result<Segment, String> {
    let rest = expression
        .strip_prefix("count_children(")
        .ok_or_else(|| format!("unsupported title expression {{{expression}}}"))?;
    let close = rest
        .find(')')
        .ok_or_else(|| "count_children is missing )".to_string())?;
    let argument = rest[..close].trim();
    let workflow = argument
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .ok_or_else(|| "count_children argument must be a quoted workflow id".to_string())?;
    if !is_valid_key(workflow) {
        return Err(format!("invalid workflow id {workflow:?}"));
    }
    let tail = rest[close + 1..].trim();
    let offset = if tail.is_empty() {
        0
    } else {
        let number = tail
            .strip_prefix('+')
            .ok_or_else(|| "only integer addition is supported in title expressions".to_string())?
            .trim();
        number
            .parse::<i64>()
            .map_err(|_| format!("invalid integer {number:?} in title expression"))?
    };
    Ok(Segment::Count {
        workflow: workflow.to_string(),
        offset,
    })
}
