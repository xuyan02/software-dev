use crate::state_machine::StateMachine;
use crate::{Actor, Link};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// What kind of thing an issue is.
///
/// Intake is deliberately cheap: the reporter picks the closest kind when the
/// issue is created. `Issue` stays the discussion object; it is not a
/// requirement, a task or a defect record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    #[default]
    Feedback,
    Bug,
    Question,
    Idea,
    TaskRequest,
    Incident,
}

impl IssueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Feedback => "feedback",
            Self::Bug => "bug",
            Self::Question => "question",
            Self::Idea => "idea",
            Self::TaskRequest => "task_request",
            Self::Incident => "incident",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "feedback" => Some(Self::Feedback),
            "bug" => Some(Self::Bug),
            "question" => Some(Self::Question),
            "idea" => Some(Self::Idea),
            "task_request" => Some(Self::TaskRequest),
            "incident" => Some(Self::Incident),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueStatus {
    /// Reported, not yet acted on.
    Open,
    /// Triaged and waiting in a work pool.
    Pooled,
    /// Work is underway.
    InProgress,
    /// Delivered, awaiting confirmation.
    Resolved,
    /// Confirmed done.
    Closed,
    /// Deliberately not acted on.
    Rejected,
    /// Same as another issue.
    Duplicate,
}

impl IssueStatus {
    pub fn allowed_transitions(self) -> &'static [IssueStatus] {
        use IssueStatus::*;
        match self {
            Open => &[Pooled, InProgress, Resolved, Rejected, Duplicate],
            Pooled => &[Pooled, InProgress, Open, Resolved, Rejected, Duplicate],
            InProgress => &[Pooled, Resolved, Rejected],
            Resolved => &[Closed, InProgress],
            Closed => &[Open],
            Rejected => &[Open],
            Duplicate => &[Open],
        }
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        self.allowed_transitions().contains(&next)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Pooled => "pooled",
            Self::InProgress => "in_progress",
            Self::Resolved => "resolved",
            Self::Closed => "closed",
            Self::Rejected => "rejected",
            Self::Duplicate => "duplicate",
        }
    }
}

impl StateMachine for IssueStatus {
    fn can_transition_to(self, next: Self) -> bool {
        self.allowed_transitions().contains(&next)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssuePool {
    Design,
    Development,
    Requirements,
    Tests,
}

impl IssuePool {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Design => "design",
            Self::Development => "development",
            Self::Requirements => "requirements",
            Self::Tests => "tests",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "design" => Some(Self::Design),
            "development" => Some(Self::Development),
            "requirements" => Some(Self::Requirements),
            "tests" => Some(Self::Tests),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    S1,
    S2,
    S3,
    S4,
}

impl IssueSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::S1 => "s1",
            Self::S2 => "s2",
            Self::S3 => "s3",
            Self::S4 => "s4",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IssueSourceKind {
    #[default]
    Internal,
    Customer,
    Support,
    Monitoring,
    Analytics,
    Incident,
    Other,
}

impl IssueSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::Customer => "customer",
            Self::Support => "support",
            Self::Monitoring => "monitoring",
            Self::Analytics => "analytics",
            Self::Incident => "incident",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct IssueSource {
    pub kind: IssueSourceKind,
    /// Free-form pointer: a ticket id, alert URL, customer name, commit.
    #[serde(default)]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueComment {
    pub id: Uuid,
    pub author: Actor,
    pub body: String,
    /// Internal notes are hidden from external reporters.
    #[serde(default)]
    pub internal: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueStatusChange {
    pub from: IssueStatus,
    pub to: IssueStatus,
    pub actor: Actor,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("issue validation failed: {}", .0.join("; "))]
pub struct IssueValidationError(pub Vec<String>);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IssueTransitionError {
    #[error("invalid issue transition from {from:?} to {to:?}")]
    Invalid { from: IssueStatus, to: IssueStatus },
    #[error("issue not ready: {0}")]
    NotReady(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransitionEvidence {
    /// Required for Resolved and Rejected; becomes the resolution record.
    pub resolution: Option<String>,
    /// Required for Duplicate.
    pub duplicate_of: Option<Uuid>,
    /// Required for Pooled; the pool the Issue waits in.
    pub pool: Option<IssuePool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueClassification {
    CodeDefect,
    DesignDefect,
    RequirementDefect,
    TestDefect,
    NotAProblem,
    Duplicate,
    NeedsInfo,
    Enhancement,
    Incident,
}

impl IssueClassification {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CodeDefect => "code_defect",
            Self::DesignDefect => "design_defect",
            Self::RequirementDefect => "requirement_defect",
            Self::TestDefect => "test_defect",
            Self::NotAProblem => "not_a_problem",
            Self::Duplicate => "duplicate",
            Self::NeedsInfo => "needs_info",
            Self::Enhancement => "enhancement",
            Self::Incident => "incident",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueConfidence {
    High,
    Medium,
    Low,
}

impl IssueConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueTriage {
    pub classification: IssueClassification,
    pub root_cause: String,
    pub confidence: IssueConfidence,
    pub decision: String,
    #[serde(default)]
    pub notes: Option<String>,
}

impl IssueTriage {
    pub fn validate(&self) -> Result<(), IssueValidationError> {
        let mut errors = Vec::new();
        if self.root_cause.trim().is_empty() {
            errors.push("triage root cause is required".to_string());
        }
        if self.decision.trim().is_empty() {
            errors.push("triage decision is required".to_string());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(IssueValidationError(errors))
        }
    }
}

/// A discussion object in problem space: something happened, someone wants
/// something, or something needs an answer.
///
/// An issue is not a requirement. Work starts when it transitions to
/// `InProgress`, optionally through a task created alongside it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub description: String,
    pub kind: IssueKind,
    pub status: IssueStatus,
    /// The work pool an Issue waits in while `status` is `Pooled`.
    #[serde(default)]
    pub pool: Option<IssuePool>,
    pub source: IssueSource,
    pub reporter: Actor,
    #[serde(default)]
    pub assignee: Option<Actor>,
    #[serde(default)]
    pub severity: Option<IssueSeverity>,
    #[serde(default)]
    pub impact: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub triage: Option<IssueTriage>,
    // Bug-specific fields. Present only when relevant.
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub affected_versions: Vec<String>,
    #[serde(default)]
    pub reproduction: Option<String>,
    #[serde(default)]
    pub expected: Option<String>,
    #[serde(default)]
    pub actual: Option<String>,
    #[serde(default)]
    pub duplicate_of: Option<Uuid>,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(default)]
    pub comments: Vec<IssueComment>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub history: Vec<IssueStatusChange>,
    pub version: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl Issue {
    pub fn new(
        project_id: Uuid,
        title: impl Into<String>,
        description: impl Into<String>,
        kind: IssueKind,
        source: IssueSource,
        reporter: Actor,
    ) -> Result<Self, IssueValidationError> {
        let now = OffsetDateTime::now_utc();
        let issue = Self {
            id: Uuid::now_v7(),
            project_id,
            title: title.into(),
            description: description.into(),
            kind,
            status: IssueStatus::Open,
            pool: None,
            source,
            reporter,
            assignee: None,
            severity: None,
            impact: None,
            area: None,
            labels: Vec::new(),
            triage: None,
            environment: None,
            affected_versions: Vec::new(),
            reproduction: None,
            expected: None,
            actual: None,
            duplicate_of: None,
            resolution: None,
            comments: Vec::new(),
            links: Vec::new(),
            history: Vec::new(),
            version: 1,
            created_at: now,
            updated_at: now,
        };
        issue.validate()?;
        Ok(issue)
    }

    pub fn validate(&self) -> Result<(), IssueValidationError> {
        let mut errors = Vec::new();
        if self.project_id.is_nil() {
            errors.push("project is required".to_string());
        }
        if self.title.trim().is_empty() {
            errors.push("title is required".to_string());
        }
        if self.description.trim().is_empty() {
            errors.push("description is required".to_string());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(IssueValidationError(errors))
        }
    }

    pub fn comment(
        &mut self,
        author: Actor,
        body: impl Into<String>,
        internal: bool,
    ) -> Result<(), IssueTransitionError> {
        let body = body.into();
        if body.trim().is_empty() {
            return Err(IssueTransitionError::NotReady(
                "comment body is required".to_string(),
            ));
        }
        self.comments.push(IssueComment {
            id: Uuid::now_v7(),
            author,
            body,
            internal,
            created_at: OffsetDateTime::now_utc(),
        });
        self.touch();
        Ok(())
    }

    pub fn transition_to(
        &mut self,
        next: IssueStatus,
        actor: &Actor,
        evidence: TransitionEvidence,
    ) -> Result<(), IssueTransitionError> {
        if !self.status.can_transition_to(next) {
            return Err(IssueTransitionError::Invalid {
                from: self.status,
                to: next,
            });
        }

        let resolution = evidence.resolution.map(|text| text.trim().to_string());
        match next {
            IssueStatus::Resolved | IssueStatus::Rejected => {
                if resolution.as_deref().is_none_or(str::is_empty) {
                    return Err(IssueTransitionError::NotReady(
                        "a resolution note is required".to_string(),
                    ));
                }
            }
            IssueStatus::Duplicate => {
                if evidence.duplicate_of.is_none() {
                    return Err(IssueTransitionError::NotReady(
                        "duplicate_of is required".to_string(),
                    ));
                }
            }
            IssueStatus::Pooled => {
                if evidence.pool.is_none() {
                    return Err(IssueTransitionError::NotReady(
                        "a pool is required".to_string(),
                    ));
                }
            }
            IssueStatus::Closed => {
                if self.resolution.is_none() {
                    return Err(IssueTransitionError::NotReady(
                        "an issue can only be closed after resolution".to_string(),
                    ));
                }
            }
            _ => {}
        }

        let from = self.status;
        match next {
            IssueStatus::Pooled => {
                self.pool = evidence.pool;
            }
            IssueStatus::Resolved | IssueStatus::Rejected => {
                self.resolution = resolution.clone();
                self.pool = None;
            }
            IssueStatus::Duplicate => {
                self.duplicate_of = evidence.duplicate_of;
                self.pool = None;
            }
            IssueStatus::Open => {
                self.resolution = None;
                self.duplicate_of = None;
                self.pool = None;
            }
            IssueStatus::InProgress | IssueStatus::Closed => {
                self.pool = None;
            }
        }

        self.status = next;
        self.history.push(IssueStatusChange {
            from,
            to: next,
            actor: actor.clone(),
            note: resolution,
            at: OffsetDateTime::now_utc(),
        });
        self.touch();
        Ok(())
    }

    fn touch(&mut self) {
        self.version = self.version.saturating_add(1);
        self.updated_at = OffsetDateTime::now_utc();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue() -> Issue {
        Issue::new(
            Uuid::now_v7(),
            "部分退款失败",
            "申请 30 元退款时返回 500",
            IssueKind::Bug,
            IssueSource {
                kind: IssueSourceKind::Support,
                reference: Some("TICKET-42".to_string()),
            },
            Actor::human("support-1"),
        )
        .unwrap()
    }

    #[test]
    fn new_issue_is_open() {
        let issue = issue();
        assert_eq!(issue.status, IssueStatus::Open);
        assert_eq!(issue.version, 1);
    }

    #[test]
    fn pooled_requires_a_pool_and_clears_on_progress() {
        let mut issue = issue();
        let error = issue
            .transition_to(
                IssueStatus::Pooled,
                &Actor::human("triager"),
                TransitionEvidence::default(),
            )
            .expect_err("pooled without a pool must fail");
        assert!(matches!(error, IssueTransitionError::NotReady(_)));

        issue
            .transition_to(
                IssueStatus::Pooled,
                &Actor::human("triager"),
                TransitionEvidence {
                    pool: Some(IssuePool::Design),
                    ..TransitionEvidence::default()
                },
            )
            .expect("open -> pooled");
        assert_eq!(issue.status, IssueStatus::Pooled);
        assert_eq!(issue.pool, Some(IssuePool::Design));

        issue
            .transition_to(
                IssueStatus::InProgress,
                &Actor::agent("designer", "designer"),
                TransitionEvidence::default(),
            )
            .expect("pooled -> in_progress");
        assert_eq!(issue.status, IssueStatus::InProgress);
        assert!(issue.pool.is_none());
    }

    #[test]
    fn open_can_start_work() {
        let mut issue = issue();
        issue
            .transition_to(
                IssueStatus::InProgress,
                &Actor::human("pm"),
                TransitionEvidence::default(),
            )
            .expect("open -> in_progress");
        assert_eq!(issue.status, IssueStatus::InProgress);
        assert_eq!(issue.version, 2);
    }

    #[test]
    fn resolve_requires_resolution_note() {
        let mut issue = issue();
        issue
            .transition_to(
                IssueStatus::InProgress,
                &Actor::agent("fixer", "developer"),
                TransitionEvidence::default(),
            )
            .expect("open -> in_progress is allowed without evidence");
        assert_eq!(issue.status, IssueStatus::InProgress);

        let error = issue
            .transition_to(
                IssueStatus::Resolved,
                &Actor::agent("fixer", "developer"),
                TransitionEvidence::default(),
            )
            .expect_err("resolve without a note must fail");
        assert!(matches!(error, IssueTransitionError::NotReady(_)));
    }

    #[test]
    fn duplicate_requires_target() {
        let mut issue = issue();
        let error = issue
            .transition_to(
                IssueStatus::Duplicate,
                &Actor::human("pm"),
                TransitionEvidence::default(),
            )
            .expect_err("duplicate without target must fail");
        assert!(matches!(error, IssueTransitionError::NotReady(_)));
    }

    #[test]
    fn closed_issue_can_be_reopened() {
        let mut issue = issue();
        issue
            .transition_to(
                IssueStatus::InProgress,
                &Actor::human("dev"),
                TransitionEvidence::default(),
            )
            .unwrap();
        issue
            .transition_to(
                IssueStatus::Resolved,
                &Actor::human("dev"),
                TransitionEvidence {
                    resolution: Some("fixed".to_string()),
                    duplicate_of: None,
                    pool: None,
                },
            )
            .unwrap();
        issue
            .transition_to(
                IssueStatus::Closed,
                &Actor::human("pm"),
                TransitionEvidence::default(),
            )
            .unwrap();
        issue
            .transition_to(
                IssueStatus::Open,
                &Actor::human("pm"),
                TransitionEvidence::default(),
            )
            .unwrap();
        assert_eq!(issue.status, IssueStatus::Open);
        assert!(issue.resolution.is_none());
    }
}
