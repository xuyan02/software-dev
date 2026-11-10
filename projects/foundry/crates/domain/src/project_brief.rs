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
    pub success_metrics: Vec<SuccessMetric>,
    #[serde(default)]
    pub scope: ProjectScope,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub risks: Vec<Risk>,
    #[serde(default)]
    pub stakeholders: Vec<Stakeholder>,
    #[serde(default)]
    pub budget: Option<Budget>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub open_questions: Vec<String>,
}

impl Default for ProjectBriefBody {
    fn default() -> Self {
        Self {
            schema_version: PROJECT_BRIEF_SCHEMA_VERSION,
            problem: String::new(),
            goals: Vec::new(),
            success_metrics: Vec::new(),
            scope: ProjectScope::default(),
            constraints: Vec::new(),
            risks: Vec::new(),
            stakeholders: Vec::new(),
            budget: None,
            assumptions: Vec::new(),
            open_questions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuccessMetric {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub baseline: Option<f64>,
    #[serde(default)]
    pub target: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub measurement: String,
    #[serde(default)]
    pub window: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectScope {
    #[serde(default)]
    pub in_scope: Vec<String>,
    #[serde(default)]
    pub out_of_scope: Vec<String>,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stakeholder {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub decision_rights: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    #[serde(default)]
    pub amount: f64,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectBriefStatus {
    #[default]
    Draft,
    InReview,
    Approved,
    Superseded,
}

impl ProjectBriefStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::InReview => "in_review",
            Self::Approved => "approved",
            Self::Superseded => "superseded",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "in_review" => Self::InReview,
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
    SetSuccessMetrics {
        values: Vec<SuccessMetric>,
    },
    SetScope {
        in_scope: Vec<String>,
        out_of_scope: Vec<String>,
    },
    SetConstraints {
        values: Vec<String>,
    },
    SetRisks {
        values: Vec<Risk>,
    },
    SetStakeholders {
        values: Vec<Stakeholder>,
    },
    SetBudget {
        value: Option<Budget>,
    },
    SetAssumptions {
        values: Vec<String>,
    },
    SetOpenQuestions {
        values: Vec<String>,
    },
}

impl ProjectBriefDraftOperation {
    pub fn apply(self, body: &mut ProjectBriefBody) {
        match self {
            Self::SetProblem { value } => body.problem = value,
            Self::SetGoals { values } => body.goals = values,
            Self::SetSuccessMetrics { values } => body.success_metrics = values,
            Self::SetScope {
                in_scope,
                out_of_scope,
            } => {
                body.scope = ProjectScope {
                    in_scope,
                    out_of_scope,
                }
            }
            Self::SetConstraints { values } => body.constraints = values,
            Self::SetRisks { values } => body.risks = values,
            Self::SetStakeholders { values } => body.stakeholders = values,
            Self::SetBudget { value } => body.budget = value,
            Self::SetAssumptions { values } => body.assumptions = values,
            Self::SetOpenQuestions { values } => body.open_questions = values,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectBriefAnnotationTarget {
    Document,
    Problem,
    Goals,
    SuccessMetrics,
    Scope,
    Constraints,
    Risks,
    Stakeholders,
    Budget,
    Assumptions,
    OpenQuestions,
}

impl ProjectBriefAnnotationTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Problem => "problem",
            Self::Goals => "goals",
            Self::SuccessMetrics => "success_metrics",
            Self::Scope => "scope",
            Self::Constraints => "constraints",
            Self::Risks => "risks",
            Self::Stakeholders => "stakeholders",
            Self::Budget => "budget",
            Self::Assumptions => "assumptions",
            Self::OpenQuestions => "open_questions",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "document" => Some(Self::Document),
            "problem" => Some(Self::Problem),
            "goals" => Some(Self::Goals),
            "success_metrics" => Some(Self::SuccessMetrics),
            "scope" => Some(Self::Scope),
            "constraints" => Some(Self::Constraints),
            "risks" => Some(Self::Risks),
            "stakeholders" => Some(Self::Stakeholders),
            "budget" => Some(Self::Budget),
            "assumptions" => Some(Self::Assumptions),
            "open_questions" => Some(Self::OpenQuestions),
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
    if body.success_metrics.is_empty() {
        validation.errors.push(ValidationIssue::error(
            "success_metrics",
            "no_success_metrics",
            "the project must declare at least one success metric",
        ));
    }
    let mut metric_ids = std::collections::HashSet::new();
    for (index, metric) in body.success_metrics.iter().enumerate() {
        let path = format!("success_metrics[{index}]");
        if !metric_ids.insert(metric.id.clone()) {
            validation.errors.push(ValidationIssue::error(
                format!("{path}.id"),
                "duplicate_id",
                format!("duplicate success metric id {}", metric.id),
            ));
        }
        if metric.name.trim().is_empty() {
            validation.errors.push(ValidationIssue::error(
                format!("{path}.name"),
                "empty_name",
                "success metric name must not be empty",
            ));
        }
        if metric.measurement.trim().is_empty() {
            validation.errors.push(ValidationIssue::error(
                format!("{path}.measurement"),
                "no_measurement",
                "success metric needs a measurement method",
            ));
        }
        if metric.unit.trim().is_empty() {
            validation.errors.push(ValidationIssue::error(
                format!("{path}.unit"),
                "no_unit",
                "success metric needs a unit",
            ));
        }
        if metric.window.trim().is_empty() {
            validation.warnings.push(ValidationIssue::error(
                format!("{path}.window"),
                "no_window",
                "success metric has no measurement window",
            ));
        }
    }
    if body.scope.out_of_scope.is_empty() {
        validation.errors.push(ValidationIssue::error(
            "scope.out_of_scope",
            "no_non_goals",
            "the project must declare at least one non-goal",
        ));
    }
    if body.stakeholders.is_empty() {
        validation.errors.push(ValidationIssue::error(
            "stakeholders",
            "no_stakeholders",
            "the project must declare at least one stakeholder",
        ));
    }
    let mut stakeholder_ids = std::collections::HashSet::new();
    for (index, stakeholder) in body.stakeholders.iter().enumerate() {
        let path = format!("stakeholders[{index}]");
        if !stakeholder_ids.insert(stakeholder.id.clone()) {
            validation.errors.push(ValidationIssue::error(
                format!("{path}.id"),
                "duplicate_id",
                format!("duplicate stakeholder id {}", stakeholder.id),
            ));
        }
        if stakeholder.role.trim().is_empty() {
            validation.errors.push(ValidationIssue::error(
                format!("{path}.role"),
                "empty_role",
                "stakeholder role must not be empty",
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
    }
    if !body.open_questions.is_empty() {
        validation.warnings.push(ValidationIssue::error(
            "open_questions",
            "open_questions",
            "the brief still has open questions",
        ));
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
            success_metrics: vec![SuccessMetric {
                id: "m1".to_string(),
                name: "人工处理量".to_string(),
                baseline: Some(1000.0),
                target: 700.0,
                unit: "件/月".to_string(),
                measurement: "客服看板".to_string(),
                window: "上线后 3 个月".to_string(),
            }],
            scope: ProjectScope {
                in_scope: vec!["部分退款".to_string()],
                out_of_scope: vec!["多次退款合并".to_string()],
            },
            stakeholders: vec![Stakeholder {
                id: "s1".to_string(),
                role: "产品".to_string(),
                account: "product-owner".to_string(),
                decision_rights: vec!["目标/范围".to_string()],
            }],
            ..ProjectBriefBody::default()
        }
    }

    #[test]
    fn complete_brief_is_valid() {
        let validation = validate_project_brief(&complete());
        assert!(validation.is_valid(), "{:?}", validation.errors);
    }

    #[test]
    fn approval_requires_metrics_non_goals_and_stakeholders() {
        let mut body = complete();
        body.success_metrics.clear();
        body.scope.out_of_scope.clear();
        body.stakeholders.clear();
        let validation = validate_project_brief(&body);
        assert!(!validation.is_valid());
        assert!(
            validation
                .errors
                .iter()
                .any(|issue| issue.code == "no_success_metrics")
        );
        assert!(
            validation
                .errors
                .iter()
                .any(|issue| issue.code == "no_non_goals")
        );
        assert!(
            validation
                .errors
                .iter()
                .any(|issue| issue.code == "no_stakeholders")
        );
    }
}
