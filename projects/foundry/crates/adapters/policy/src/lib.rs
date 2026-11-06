//! Policy adapters.
//!
//! `DemoPolicy` encodes the MVP separation of duties:
//!
//! * humans may do anything the domain state machine allows;
//! * agents may create requirements and move them forward along the
//!   implementation path;
//! * agents may never approve, reject, defer, mark done or supersede a
//!   requirement;
//! * agents may receive, classify, discuss and work issues, but may not reject
//!   a product-level issue or confirm it closed;
//! * the system itself may not initiate user-facing changes.
//!
//! Replace it with an OPA/Cedar client when the policy outgrows code.

use foundry_app::{Action, ActionContext, PolicyDenied, PolicyEngine};
use foundry_domain::{Actor, IssueStatus, RequirementStatus};

#[derive(Debug, Clone, Copy, Default)]
pub struct DemoPolicy;

impl PolicyEngine for DemoPolicy {
    fn check(&self, context: &ActionContext) -> Result<(), PolicyDenied> {
        match (&context.actor, &context.action) {
            (Actor::Human { .. }, _) => Ok(()),
            (
                Actor::Agent { .. },
                Action::CreateRequirement
                | Action::CreateIssue
                | Action::CommentIssue
                | Action::TriageIssue,
            ) => Ok(()),
            (Actor::Agent { .. }, Action::TransitionRequirement { from, to }) => {
                if agent_transition_allowed(*from, *to) {
                    Ok(())
                } else {
                    Err(PolicyDenied::new(format!(
                        "agents may not move a requirement from {from:?} to {to:?}"
                    )))
                }
            }
            (Actor::Agent { .. }, Action::TransitionIssue { from, to }) => {
                if agent_issue_transition_allowed(*from, *to) {
                    Ok(())
                } else {
                    Err(PolicyDenied::new(format!(
                        "agents may not move an issue from {from:?} to {to:?}"
                    )))
                }
            }
            (Actor::System, _) => Err(PolicyDenied::new(
                "system actors cannot change work items without a human or agent identity",
            )),
        }
    }
}

fn agent_transition_allowed(from: RequirementStatus, to: RequirementStatus) -> bool {
    use RequirementStatus::*;
    matches!(
        (from, to),
        (Draft, InReview)
            | (Approved, InDesign)
            | (InDesign, InDevelopment)
            | (InDevelopment, InVerification)
    )
}

/// Agents can move work forward and report a delivery, but cannot make the
/// disposition decisions (`Rejected`, `Duplicate`, `Closed`) or reopen a
/// closed issue. `Open -> Triaged` happens through the triage action, which
/// carries the classification evidence.
fn agent_issue_transition_allowed(from: IssueStatus, to: IssueStatus) -> bool {
    use IssueStatus::*;
    matches!(
        (from, to),
        (Triaged, InProgress) | (Triaged, Resolved) | (InProgress, Resolved)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_app::Resource;
    use uuid::Uuid;

    fn context(actor: Actor, from: RequirementStatus, to: RequirementStatus) -> ActionContext {
        ActionContext {
            actor,
            action: Action::TransitionRequirement { from, to },
            resource: Resource::Requirement(Uuid::now_v7()),
        }
    }

    #[test]
    fn agent_can_move_to_review() {
        DemoPolicy
            .check(&context(
                Actor::agent("agent-1", "analyst"),
                RequirementStatus::Draft,
                RequirementStatus::InReview,
            ))
            .unwrap();
    }

    #[test]
    fn agent_cannot_approve() {
        let result = DemoPolicy.check(&context(
            Actor::agent("agent-1", "analyst"),
            RequirementStatus::InReview,
            RequirementStatus::Approved,
        ));
        assert!(result.is_err());
    }

    #[test]
    fn human_can_approve() {
        DemoPolicy
            .check(&context(
                Actor::human("alice"),
                RequirementStatus::InReview,
                RequirementStatus::Approved,
            ))
            .unwrap();
    }

    fn issue_context(actor: Actor, from: IssueStatus, to: IssueStatus) -> ActionContext {
        ActionContext {
            actor,
            action: Action::TransitionIssue { from, to },
            resource: Resource::Issue(Uuid::now_v7()),
        }
    }

    #[test]
    fn agent_can_work_an_issue_forward() {
        DemoPolicy
            .check(&ActionContext {
                actor: Actor::agent("triage-bot", "triage"),
                action: Action::TriageIssue,
                resource: Resource::Issue(Uuid::now_v7()),
            })
            .unwrap();
        DemoPolicy
            .check(&issue_context(
                Actor::agent("fixer", "developer"),
                IssueStatus::InProgress,
                IssueStatus::Resolved,
            ))
            .unwrap();
    }

    #[test]
    fn agent_cannot_reject_or_close_an_issue() {
        assert!(
            DemoPolicy
                .check(&issue_context(
                    Actor::agent("triage-bot", "triage"),
                    IssueStatus::Open,
                    IssueStatus::Rejected,
                ))
                .is_err()
        );
        assert!(
            DemoPolicy
                .check(&issue_context(
                    Actor::agent("fixer", "developer"),
                    IssueStatus::Resolved,
                    IssueStatus::Closed,
                ))
                .is_err()
        );
    }

    #[test]
    fn human_can_reject_an_issue() {
        DemoPolicy
            .check(&issue_context(
                Actor::human("pm"),
                IssueStatus::Open,
                IssueStatus::Rejected,
            ))
            .unwrap();
    }
}
