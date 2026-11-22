//! Policy adapters.
//!
//! `DemoPolicy` encodes the MVP separation of duties:
//!
//! * humans may do anything the domain state machine allows;
//! * agents may receive, classify, pool, work and close issues, but may not
//!   reject a product-level issue or reopen a terminal issue;
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
            (
                Actor::Agent { .. },
                Action::CreateIssue | Action::CommentIssue | Action::TriageIssue,
            ) => Ok(()),
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

/// Agents route work through the pools, resolve it, and close resolved work.
/// Rejection and reopening a terminal issue stay human decisions.
fn agent_issue_transition_allowed(from: IssueStatus, to: IssueStatus) -> bool {
    use IssueStatus::*;
    matches!(
        (from, to),
        (Open, Pooled | InProgress | Resolved | Duplicate)
            | (Pooled, Pooled | InProgress | Resolved | Duplicate)
            | (InProgress, Pooled | InProgress | Resolved | Duplicate)
            | (Resolved, Closed)
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
    fn agent_routes_issues_through_the_pools() {
        for (from, to) in [
            (IssueStatus::Open, IssueStatus::Pooled),
            (IssueStatus::Pooled, IssueStatus::InProgress),
            (IssueStatus::InProgress, IssueStatus::Pooled),
            (IssueStatus::InProgress, IssueStatus::Duplicate),
        ] {
            DemoPolicy
                .check(&issue_context(Actor::agent("worker", "worker"), from, to))
                .unwrap_or_else(|error| panic!("{from:?} -> {to:?}: {error:?}"));
        }
    }

    #[test]
    fn agent_can_close_resolved_work_but_not_reject() {
        DemoPolicy
            .check(&issue_context(
                Actor::agent("fixer", "developer"),
                IssueStatus::Resolved,
                IssueStatus::Closed,
            ))
            .unwrap();
        assert!(
            DemoPolicy
                .check(&issue_context(
                    Actor::agent("reviewer", "fixer"),
                    IssueStatus::Open,
                    IssueStatus::Rejected,
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
