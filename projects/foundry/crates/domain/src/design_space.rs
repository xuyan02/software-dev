//! Design-space merge requests.
//!
//! A merge request is branch/commit metadata plus the review Task that decides
//! it. The Task is the source of truth for reviewers, operations, comments and
//! the audit trail. The approved design content digest is stored here because
//! it binds the human approval to the content, not to comment metadata.

use crate::{Task, TaskDAO, TaskState};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// The Task operation that completes a merge request. It is only sent by the
/// merge-request service after the branch ref moved; the generic Task
/// operation endpoint rejects it so the branch side effect cannot be skipped.
pub const DESIGN_SPACE_MERGE_OPERATION: &str = "merge";

/// Persisted branch and merge metadata of one review request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignSpaceMergeRequestDAO {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub source_branch: String,
    pub target_branch: String,
    /// Source branch head captured when the request was created.
    pub source_commit: String,
    /// Target branch head the request was created against.
    pub target_commit: String,
    /// The review Task. Approval, comments and operations live there.
    pub task_id: Uuid,
    /// Set only after the fast-forward merge succeeded.
    #[serde(default)]
    pub merge_commit: Option<String>,
    /// Design content digest approved by the human reviewer. Comments are
    /// excluded so a cleanup commit does not invalidate the approval.
    #[serde(default)]
    pub approved_content_digest: Option<String>,
    /// Source commit whose content was approved.
    #[serde(default)]
    pub approved_source_commit: Option<String>,
    #[serde(default)]
    pub approved_by: Option<Uuid>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub approved_at: Option<OffsetDateTime>,
    pub created_by: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl DesignSpaceMergeRequestDAO {
    pub fn view(&self) -> DesignSpaceMergeRequest {
        DesignSpaceMergeRequest::from(self.clone())
    }
}

/// Read model for a merge request: the persisted row with storage details
/// removed and stable field names for the API and console.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignSpaceMergeRequest {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub description: String,
    pub source_branch: String,
    pub target_branch: String,
    pub source_commit: String,
    pub target_commit: String,
    pub task_id: Uuid,
    pub merge_commit: Option<String>,
    pub approved_content_digest: Option<String>,
    pub approved_source_commit: Option<String>,
    pub approved_by: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub approved_at: Option<OffsetDateTime>,
    pub created_by: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl From<DesignSpaceMergeRequestDAO> for DesignSpaceMergeRequest {
    fn from(value: DesignSpaceMergeRequestDAO) -> Self {
        Self {
            id: value.id,
            project_id: value.project_id,
            title: value.title,
            description: value.description,
            source_branch: value.source_branch,
            target_branch: value.target_branch,
            source_commit: value.source_commit,
            target_commit: value.target_commit,
            task_id: value.task_id,
            merge_commit: value.merge_commit,
            approved_content_digest: value.approved_content_digest,
            approved_source_commit: value.approved_source_commit,
            approved_by: value.approved_by,
            approved_at: value.approved_at,
            created_by: value.created_by,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

impl DesignSpaceMergeRequest {
    pub fn status(&self, task: &TaskDAO) -> MergeRequestStatus {
        self.status_from(
            task.state,
            task.stage.as_ref().map(|stage| stage.as_str()),
            task.result.as_ref().map(|result| result.as_str()),
        )
    }

    pub fn status_from(
        &self,
        state: TaskState,
        stage: Option<&str>,
        result: Option<&str>,
    ) -> MergeRequestStatus {
        if self.merge_commit.is_some() {
            return MergeRequestStatus::Merged;
        }
        if state == TaskState::Completed {
            return match result {
                Some("merged") => MergeRequestStatus::Merged,
                Some("rejected") => MergeRequestStatus::Rejected,
                _ => MergeRequestStatus::Closed,
            };
        }
        if stage == Some("merge") {
            MergeRequestStatus::Approved
        } else {
            MergeRequestStatus::Reviewing
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeRequestStatus {
    Reviewing,
    Approved,
    Merged,
    Rejected,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeRequestChangeStatus {
    Added,
    Modified,
    Removed,
}

/// One design-space object difference between the target and source commits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeRequestChange {
    pub status: MergeRequestChangeStatus,
    pub qname: String,
    pub kind: String,
}

/// Read model for merge-request lists and detail pages.
#[derive(Debug, Clone, Serialize)]
pub struct MergeRequestReview {
    pub merge_request: DesignSpaceMergeRequest,
    pub task: Task,
    pub status: MergeRequestStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct MergeRequestDetail {
    pub merge_request: DesignSpaceMergeRequest,
    pub task: Task,
    pub status: MergeRequestStatus,
    pub changes: Vec<MergeRequestChange>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StageKey, TaskResult, WorkflowId};

    fn task(state: TaskState, stage: &str, result: Option<&str>) -> TaskDAO {
        let now = OffsetDateTime::now_utc();
        let mut task = TaskDAO::new(
            Uuid::now_v7(),
            None,
            WorkflowId::new("design_space_merge"),
            1,
            "MR",
            now,
        );
        task.state = state;
        task.stage = Some(StageKey::new(stage));
        task.result = result.map(TaskResult::new);
        task
    }

    fn request() -> DesignSpaceMergeRequest {
        let now = OffsetDateTime::now_utc();
        DesignSpaceMergeRequest {
            id: Uuid::now_v7(),
            project_id: Uuid::now_v7(),
            title: "MR".to_string(),
            description: String::new(),
            source_branch: "feature".to_string(),
            target_branch: "main".to_string(),
            source_commit: "aa".to_string(),
            target_commit: "bb".to_string(),
            task_id: Uuid::now_v7(),
            merge_commit: None,
            approved_content_digest: None,
            approved_source_commit: None,
            approved_by: None,
            approved_at: None,
            created_by: Uuid::now_v7(),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn status_follows_the_review_task() {
        let mut request = request();
        assert_eq!(
            request.status(&task(TaskState::InProgress, "review", None)),
            MergeRequestStatus::Reviewing
        );
        assert_eq!(
            request.status(&task(TaskState::InProgress, "merge", None)),
            MergeRequestStatus::Approved
        );
        assert_eq!(
            request.status(&task(TaskState::Completed, "merge", Some("rejected"))),
            MergeRequestStatus::Rejected
        );
        assert_eq!(
            request.status(&task(TaskState::Completed, "merge", Some("merged"))),
            MergeRequestStatus::Merged
        );
        request.merge_commit = Some("cc".to_string());
        assert_eq!(
            request.status(&task(TaskState::Completed, "merge", Some("closed"))),
            MergeRequestStatus::Merged
        );
    }
}
