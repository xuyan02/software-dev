//! Project brief: the project-level commitment produced by clarification.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{AnnotationStatus, Validation, ValidationIssue};

pub const PROJECT_BRIEF_SCHEMA_VERSION: u16 = 1;
pub const MAX_PROJECT_BRIEF_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectBriefBody {
    pub schema_version: u16,
    #[serde(default)]
    pub problem: String,
    #[serde(default)]
    pub goals: Vec<String>,
    #[serde(default)]
    pub non_goals: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub risks: Vec<Risk>,
    /// Terms, acronyms and jargon that need an explicit explanation.
    #[serde(default)]
    pub terms: Vec<Term>,
}

impl Default for ProjectBriefBody {
    fn default() -> Self {
        Self {
            schema_version: PROJECT_BRIEF_SCHEMA_VERSION,
            problem: String::new(),
            goals: Vec::new(),
            non_goals: Vec::new(),
            constraints: Vec::new(),
            risks: Vec::new(),
            terms: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Risk {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub probability: RiskLevel,
    #[serde(default)]
    pub impact: RiskLevel,
    #[serde(default)]
    pub mitigation: String,
    #[serde(default)]
    pub owner: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    #[default]
    Medium,
    Low,
    High,
}

/// One term or acronym explained for readers of the brief.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Term {
    #[serde(default)]
    pub term: String,
    #[serde(default)]
    pub definition: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectBriefStatus {
    #[default]
    Draft,
    InReview,
    ChangesRequested,
    Approved,
    Superseded,
}

impl ProjectBriefStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::InReview => "in_review",
            Self::ChangesRequested => "changes_requested",
            Self::Approved => "approved",
            Self::Superseded => "superseded",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "in_review" => Self::InReview,
            "changes_requested" => Self::ChangesRequested,
            "approved" => Self::Approved,
            "superseded" => Self::Superseded,
            _ => Self::Draft,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectBriefVersionDAO {
    pub project_id: Uuid,
    pub version: u32,
    pub status: ProjectBriefStatus,
    pub body: ProjectBriefBody,
    pub created_by: Uuid,
    pub approved_by: Option<Uuid>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectBriefDraftDAO {
    pub id: Uuid,
    pub project_id: Uuid,
    pub revision: u64,
    pub body: ProjectBriefBody,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

/// One structured edit to a project brief draft.
///
/// Each request applies exactly one variant. Section-level setters keep the
/// request small without offering a single "replace the whole document" API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProjectBriefDraftOperation {
    SetProblem {
        value: String,
    },
    SetGoals {
        values: Vec<String>,
    },
    SetNonGoals {
        values: Vec<String>,
    },
    SetConstraints {
        values: Vec<String>,
    },
    SetRisks {
        values: Vec<Risk>,
    },
    SetTerms {
        values: Vec<Term>,
    },
}

impl ProjectBriefDraftOperation {
    pub fn apply(self, body: &mut ProjectBriefBody) {
        match self {
            Self::SetProblem { value } => body.problem = value,
            Self::SetGoals { values } => body.goals = values,
            Self::SetNonGoals { values } => body.non_goals = values,
            Self::SetConstraints { values } => body.constraints = values,
            Self::SetRisks { values } => body.risks = values,
            Self::SetTerms { values } => body.terms = values,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectBriefAnnotationTarget {
    Document,
    Problem,
    Goals,
    NonGoals,
    Constraints,
    Risks,
    Terms,
}

impl ProjectBriefAnnotationTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Problem => "problem",
            Self::Goals => "goals",
            Self::NonGoals => "non_goals",
            Self::Constraints => "constraints",
            Self::Risks => "risks",
            Self::Terms => "terms",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "document" => Some(Self::Document),
            "problem" => Some(Self::Problem),
            "goals" => Some(Self::Goals),
            "non_goals" => Some(Self::NonGoals),
            "constraints" => Some(Self::Constraints),
            "risks" => Some(Self::Risks),
            "terms" => Some(Self::Terms),
            _ => None,
        }
    }
}

/// A review note anchored to a project brief section or object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectBriefAnnotation {
    pub id: Uuid,
    pub project_id: Uuid,
    pub target_type: ProjectBriefAnnotationTarget,
    pub target_id: Option<String>,
    /// Optional field inside the target, such as `target`, `mitigation` or
    /// `decision_rights`.
    pub field: Option<String>,
    pub body: String,
    pub author_id: Uuid,
    pub author_handle: String,
    pub author_name: String,
    pub status: AnnotationStatus,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub resolved_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub resolved_by: Option<Uuid>,
}

pub fn validate_project_brief(body: &ProjectBriefBody) -> Validation {
    let mut validation = Validation::default();
    if body.problem.trim().is_empty() {
        validation.errors.push(ValidationIssue::error(
            "problem",
            "empty_problem",
            "the project problem must not be empty",
        ));
    }
    if body.goals.is_empty() {
        validation.errors.push(ValidationIssue::error(
            "goals",
            "no_goals",
            "the project must declare at least one goal",
        ));
    }
    if body.non_goals.is_empty() {
        validation.errors.push(ValidationIssue::error(
            "non_goals",
            "no_non_goals",
            "the project must declare at least one non-goal",
        ));
    }
    let mut term_names = std::collections::HashSet::new();
    for (index, term) in body.terms.iter().enumerate() {
        let path = format!("terms[{index}]");
        if term.term.trim().is_empty() {
            validation.errors.push(ValidationIssue::error(
                format!("{path}.term"),
                "empty_term",
                "term must not be empty",
            ));
        }
        if term.definition.trim().is_empty() {
            validation.errors.push(ValidationIssue::error(
                format!("{path}.definition"),
                "empty_definition",
                "term definition must not be empty",
            ));
        }
        let key = term.term.trim().to_lowercase();
        if !key.is_empty() && !term_names.insert(key) {
            validation.errors.push(ValidationIssue::error(
                format!("{path}.term"),
                "duplicate_term",
                format!("duplicate term {:?}", term.term),
            ));
        }
    }
    if body.constraints.is_empty() {
        validation.warnings.push(ValidationIssue::error(
            "constraints",
            "no_constraints",
            "the project declares no constraints",
        ));
    }
    if body.risks.is_empty() {
        validation.warnings.push(ValidationIssue::error(
            "risks",
            "no_risks",
            "the project declares no risks",
        ));
    }
    let mut risk_ids = std::collections::HashSet::new();
    for (index, risk) in body.risks.iter().enumerate() {
        let path = format!("risks[{index}]");
        if !risk_ids.insert(risk.id.clone()) {
            validation.errors.push(ValidationIssue::error(
                format!("{path}.id"),
                "duplicate_id",
                format!("duplicate risk id {}", risk.id),
            ));
        }
        if risk.description.trim().is_empty() {
            validation.warnings.push(ValidationIssue::error(
                format!("{path}.description"),
                "empty_description",
                "risk description is empty",
            ));
        }
        if risk.owner.trim().is_empty() {
            validation.warnings.push(ValidationIssue::error(
                format!("{path}.owner"),
                "no_owner",
                "risk has no owner",
            ));
        }
        if risk.mitigation.trim().is_empty() {
            validation.warnings.push(ValidationIssue::error(
                format!("{path}.mitigation"),
                "no_mitigation",
                "risk has no later resolution plan",
            ));
        }
    }
    validation
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete() -> ProjectBriefBody {
        ProjectBriefBody {
            problem: "客服只能整单退款".to_string(),
            goals: vec!["支持部分退款".to_string()],
            non_goals: vec!["多次退款合并".to_string()],
            ..ProjectBriefBody::default()
        }
    }

    #[test]
    fn complete_brief_is_valid() {
        let validation = validate_project_brief(&complete());
        assert!(validation.is_valid(), "{:?}", validation.errors);
    }

    #[test]
    fn terms_need_a_definition_and_unique_names() {
        let mut body = complete();
        body.terms = vec![
            Term {
                term: "SLA".to_string(),
                definition: String::new(),
            },
            Term {
                term: "sla".to_string(),
                definition: "服务等级协议".to_string(),
            },
        ];
        let validation = validate_project_brief(&body);
        assert!(
            validation
                .errors
                .iter()
                .any(|issue| issue.code == "empty_definition")
        );
        assert!(
            validation
                .errors
                .iter()
                .any(|issue| issue.code == "duplicate_term")
        );
    }

    #[test]
    fn approval_requires_non_goals() {
        let mut body = complete();
        body.non_goals.clear();
        let validation = validate_project_brief(&body);
        assert!(!validation.is_valid());
        assert!(
            validation
                .errors
                .iter()
                .any(|issue| issue.code == "no_non_goals")
        );
    }
}
