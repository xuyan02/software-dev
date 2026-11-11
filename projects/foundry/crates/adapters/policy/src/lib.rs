//! Policy adapters.
//!
//! `DemoPolicy` encodes the MVP separation of duties:
//!
//! * humans may do anything the domain state machine allows;
//! * agents may receive, classify, discuss and work issues, but may not reject
//!   a product-level issue or confirm it closed;
//! * the system itself may not initiate user-facing changes.
//!
//! Replace it with an OPA/Cedar client when the policy outgrows code.

use foundry_app::{Action, ActionContext, PolicyDenied, PolicyEngine};
use foundry_domain::{Actor, IssueStatus};

#[derive(Debug, Clone, Copy, Default)]
pub struct DemoPolicy;

impl PolicyEngine for DemoPolicy {
    fn check(&self, context: &ActionContext) -> Result<(), PolicyDenied> {
        match (&context.actor, &context.action) {
            (Actor::Human { .. }, _) => Ok(()),
            (Actor::Agent { .. }, Action::CreateIssue | Action::CommentIssue) => Ok(()),
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

/// Agents can start and move work forward, but cannot make the disposition
/// decisions (`Rejected`, `Duplicate`, `Closed`) or reopen a closed issue.
fn agent_issue_transition_allowed(from: IssueStatus, to: IssueStatus) -> bool {
    use IssueStatus::*;
    matches!(
        (from, to),
        (Open, InProgress) | (Open, Resolved) | (InProgress, Resolved)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_app::Resource;
    use uuid::Uuid;

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
            .check(&issue_context(
                Actor::agent("fixer", "developer"),
                IssueStatus::Open,
                IssueStatus::InProgress,
            ))
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
                    Actor::agent("reviewer", "fixer"),
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
