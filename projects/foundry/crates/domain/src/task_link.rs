//! Task links: references from a Task to Issues, documents, artifacts or URLs.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Actor, Validation, ValidationIssue};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskLinkKind {
    Issue,
    Requirement,
    Design,
    Adr,
    Contract,
    Decision,
    Task,
    Artifact,
    Url,
}

impl TaskLinkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Issue => "issue",
            Self::Requirement => "requirement",
            Self::Design => "design",
            Self::Adr => "adr",
            Self::Contract => "contract",
            Self::Decision => "decision",
            Self::Task => "task",
            Self::Artifact => "artifact",
            Self::Url => "url",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "issue" => Some(Self::Issue),
            "requirement" => Some(Self::Requirement),
            "design" => Some(Self::Design),
            "adr" => Some(Self::Adr),
            "contract" => Some(Self::Contract),
            "decision" => Some(Self::Decision),
            "task" => Some(Self::Task),
            "artifact" => Some(Self::Artifact),
            "url" => Some(Self::Url),
            _ => None,
        }
    }

    pub fn is_external(self) -> bool {
        self == Self::Url
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskLinkRelation {
    Clarifies,
    Implements,
    DependsOn,
    References,
    Produces,
    Verifies,
    Related,
}

impl TaskLinkRelation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clarifies => "clarifies",
            Self::Implements => "implements",
            Self::DependsOn => "depends_on",
            Self::References => "references",
            Self::Produces => "produces",
            Self::Verifies => "verifies",
            Self::Related => "related",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "clarifies" => Some(Self::Clarifies),
            "implements" => Some(Self::Implements),
            "depends_on" => Some(Self::DependsOn),
            "references" => Some(Self::References),
            "produces" => Some(Self::Produces),
            "verifies" => Some(Self::Verifies),
            "related" => Some(Self::Related),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskLink {
    pub id: Uuid,
    pub task_id: Uuid,
    pub kind: TaskLinkKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub title: String,
    pub relation: TaskLinkRelation,
    pub created_by: Actor,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(default)]
    pub metadata: Value,
}

pub fn validate_task_link(link: &TaskLink) -> Validation {
    let mut validation = Validation::default();
    if link.title.trim().is_empty() {
        validation.errors.push(ValidationIssue::error(
            "title",
            "empty_title",
            "link title must not be empty",
        ));
    }
    if link.kind.is_external() {
        if link.target_id.is_some() {
            validation.errors.push(ValidationIssue::error(
                "target_id",
                "external_has_target",
                "external links must not have a target_id",
            ));
        }
        match link.url.as_deref() {
            None => validation.errors.push(ValidationIssue::error(
                "url",
                "missing_url",
                "external links must have a url",
            )),
            Some(url) if !(url.starts_with("http://") || url.starts_with("https://")) => {
                validation.errors.push(ValidationIssue::error(
                    "url",
                    "invalid_url",
                    "external links must use http:// or https://",
                ));
            }
            Some(_) => {}
        }
    } else {
        if link.target_id.is_none() {
            validation.errors.push(ValidationIssue::error(
                "target_id",
                "missing_target",
                "internal links must have a target_id",
            ));
        }
        if link.url.is_some() {
            validation.errors.push(ValidationIssue::error(
                "url",
                "internal_has_url",
                "internal links must not have a url",
            ));
        }
    }
    validation
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(kind: TaskLinkKind) -> TaskLink {
        TaskLink {
            id: Uuid::now_v7(),
            task_id: Uuid::now_v7(),
            kind,
            target_id: None,
            url: None,
            title: "澄清".to_string(),
            relation: TaskLinkRelation::Clarifies,
            created_by: Actor::human("creator"),
            created_at: OffsetDateTime::now_utc(),
            metadata: Value::Null,
        }
    }

    #[test]
    fn internal_link_requires_target() {
        let validation = validate_task_link(&link(TaskLinkKind::Issue));
        assert!(!validation.is_valid());
        assert!(
            validation
                .errors
                .iter()
                .any(|issue| issue.code == "missing_target")
        );
    }

    #[test]
    fn external_link_requires_http_url() {
        let mut value = link(TaskLinkKind::Url);
        value.url = Some("ftp://example.com".to_string());
        let validation = validate_task_link(&value);
        assert!(!validation.is_valid());
        assert!(
            validation
                .errors
                .iter()
                .any(|issue| issue.code == "invalid_url")
        );
    }

    #[test]
    fn valid_internal_link_passes() {
        let mut value = link(TaskLinkKind::Issue);
        value.target_id = Some(Uuid::now_v7());
        assert!(validate_task_link(&value).is_valid());
    }
}
