use crate::state_machine::StateMachine;
use crate::work_item::{Priority, RiskClass};
use crate::{Actor, Link};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementStatus {
    Draft,
    InReview,
    Approved,
    InDesign,
    InDevelopment,
    InVerification,
    Done,
    Rejected,
    Deferred,
    Superseded,
}

impl RequirementStatus {
    pub fn allowed_transitions(self) -> &'static [RequirementStatus] {
        use RequirementStatus::*;
        match self {
            Draft => &[InReview, Rejected, Deferred],
            InReview => &[Approved, Draft, Rejected, Deferred],
            Approved => &[InDesign, Deferred, Superseded],
            InDesign => &[InDevelopment, Approved, Deferred, Superseded],
            InDevelopment => &[InVerification, InDesign, Deferred],
            InVerification => &[Done, InDevelopment, Deferred],
            Done => &[Superseded],
            Rejected => &[Draft, Superseded],
            Deferred => &[Draft, InReview, Superseded],
            Superseded => &[],
        }
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        self.allowed_transitions().contains(&next)
    }
}

impl StateMachine for RequirementStatus {
    fn can_transition_to(self, next: Self) -> bool {
        self.allowed_transitions().contains(&next)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptanceScenario {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub given: Vec<String>,
    #[serde(default)]
    pub when: Vec<String>,
    #[serde(default)]
    pub then: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusChange {
    pub from: RequirementStatus,
    pub to: RequirementStatus,
    pub actor: Actor,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("validation failed: {}", .0.join("; "))]
pub struct ValidationError(pub Vec<String>);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("invalid transition from {from:?} to {to:?}")]
    Invalid {
        from: RequirementStatus,
        to: RequirementStatus,
    },
    #[error("requirement not ready: {0}")]
    NotReady(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub problem: String,
    #[serde(default)]
    pub users: Vec<String>,
    #[serde(default)]
    pub acceptance: Vec<AcceptanceScenario>,
    #[serde(default)]
    pub non_goals: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    pub priority: Priority,
    pub risk: RiskClass,
    pub status: RequirementStatus,
    pub owner: Actor,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub history: Vec<StatusChange>,
    pub version: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl Requirement {
    pub fn new(
        project_id: Uuid,
        title: impl Into<String>,
        problem: impl Into<String>,
        acceptance: Vec<AcceptanceScenario>,
        owner: Actor,
        priority: Priority,
        risk: RiskClass,
    ) -> Result<Self, ValidationError> {
        let now = OffsetDateTime::now_utc();
        let requirement = Self {
            id: Uuid::now_v7(),
            project_id,
            title: title.into(),
            problem: problem.into(),
            users: Vec::new(),
            acceptance,
            non_goals: Vec::new(),
            constraints: Vec::new(),
            priority,
            risk,
            status: RequirementStatus::Draft,
            owner,
            links: Vec::new(),
            history: Vec::new(),
            version: 1,
            created_at: now,
            updated_at: now,
        };
        requirement.validate()?;
        Ok(requirement)
    }

    pub fn with_users(mut self, users: Vec<String>) -> Self {
        self.users = users;
        self
    }

    pub fn with_non_goals(mut self, non_goals: Vec<String>) -> Self {
        self.non_goals = non_goals;
        self
    }

    pub fn with_constraints(mut self, constraints: Vec<String>) -> Self {
        self.constraints = constraints;
        self
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        let mut errors = Vec::new();

        if self.project_id.is_nil() {
            errors.push("project is required".to_string());
        }
        if self.title.trim().is_empty() {
            errors.push("title is required".to_string());
        }
        if self.problem.trim().is_empty() {
            errors.push("problem is required".to_string());
        }
        if self.acceptance.is_empty() {
            errors.push("at least one acceptance scenario is required".to_string());
        }

        for (index, scenario) in self.acceptance.iter().enumerate() {
            if scenario.id.trim().is_empty() {
                errors.push(format!("acceptance[{index}].id is required"));
            }
            if scenario.title.trim().is_empty() {
                errors.push(format!("acceptance[{index}].title is required"));
            }
            if scenario.then.is_empty() {
                errors.push(format!("acceptance[{index}].then is required"));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ValidationError(errors))
        }
    }

    /// Extra checks required before a requirement can be approved.
    ///
    /// We require users and non-goals at approval time, not at draft time:
    /// early drafts should be cheap to create, but an approved requirement
    /// must say who it is for and what it explicitly does not do.
    pub fn validate_for_approval(&self) -> Result<(), ValidationError> {
        self.validate()?;

        let mut errors = Vec::new();
        if self.users.is_empty() {
            errors.push("approval requires at least one user/persona".to_string());
        }
        if self.non_goals.is_empty() {
            errors.push("approval requires at least one non-goal".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ValidationError(errors))
        }
    }

    pub fn transition_to(
        &mut self,
        next: RequirementStatus,
        actor: &Actor,
    ) -> Result<(), TransitionError> {
        if !self.status.can_transition_to(next) {
            return Err(TransitionError::Invalid {
                from: self.status,
                to: next,
            });
        }

        if next == RequirementStatus::Approved {
            self.validate_for_approval()
                .map_err(|error| TransitionError::NotReady(error.to_string()))?;
        }

        let from = self.status;
        self.status = next;
        self.version = self.version.saturating_add(1);
        self.updated_at = OffsetDateTime::now_utc();
        self.history.push(StatusChange {
            from,
            to: next,
            actor: actor.clone(),
            at: self.updated_at,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenario() -> AcceptanceScenario {
        AcceptanceScenario {
            id: "S-1".to_string(),
            title: "基本场景".to_string(),
            given: vec!["用户已登录".to_string()],
            when: vec!["提交请求".to_string()],
            then: vec!["请求成功".to_string()],
        }
    }

    fn draft() -> Requirement {
        Requirement::new(
            Uuid::now_v7(),
            "支持部分退款",
            "用户只能全额退款",
            vec![scenario()],
            Actor::human("alice"),
            Priority::P1,
            RiskClass::Medium,
        )
        .expect("draft should be valid")
    }

    #[test]
    fn draft_can_enter_review() {
        let mut requirement = draft();
        requirement
            .transition_to(RequirementStatus::InReview, &Actor::human("alice"))
            .expect("draft -> in_review should be allowed");
        assert_eq!(requirement.status, RequirementStatus::InReview);
    }

    #[test]
    fn draft_cannot_be_approved_directly() {
        let mut requirement = draft();
        let error = requirement
            .transition_to(RequirementStatus::Approved, &Actor::human("alice"))
            .expect_err("draft -> approved must be rejected");
        assert!(matches!(error, TransitionError::Invalid { .. }));
    }

    #[test]
    fn approval_requires_users_and_non_goals() {
        let mut requirement = draft();
        requirement
            .transition_to(RequirementStatus::InReview, &Actor::human("alice"))
            .unwrap();

        let error = requirement
            .transition_to(RequirementStatus::Approved, &Actor::human("alice"))
            .expect_err("approval without users and non-goals must fail");
        assert!(matches!(error, TransitionError::NotReady(_)));

        requirement.users = vec!["买家".to_string()];
        requirement.non_goals = vec!["不支持跨币种退款".to_string()];
        requirement
            .transition_to(RequirementStatus::Approved, &Actor::human("alice"))
            .expect("approval should now succeed");
    }

    #[test]
    fn transition_increments_version_and_records_history() {
        let mut requirement = draft();
        requirement
            .transition_to(RequirementStatus::InReview, &Actor::human("alice"))
            .unwrap();
        assert_eq!(requirement.version, 2);
        assert_eq!(requirement.history.len(), 1);
        assert_eq!(requirement.history[0].from, RequirementStatus::Draft);
        assert_eq!(requirement.history[0].to, RequirementStatus::InReview);
    }
}
