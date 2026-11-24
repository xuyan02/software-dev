//! Design-space merge requests.
//!
//! The merge request is a thin record over branch refs. Its review is an
//! ordinary Task: creating a request creates a `design_space_merge` Task and
//! binds one human reviewer. Approval, rejection, close and merge are all Task
//! operations; the request itself stores only branch and merge metadata.

use crate::{
    AccountRepository, RepoError, ServiceError, TaskDispatcher, TaskUseCases, TaskViewer,
    WorkflowCatalog, WorkflowLookup,
};
use async_trait::async_trait;
use foundry_domain::{
    AccountKind, Actor, DESIGN_SPACE_MERGE_OPERATION, DesignSpaceMergeRequestDAO, DomainEvent,
    DomainEventKind, MergeRequestDetail, MergeRequestDiff, MergeRequestReview, OperationName,
    Project, RoleKey, TaskActorDAO, TaskDAO, TaskState, WorkflowId,
};
use serde_json::json;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

pub const MERGE_REQUEST_WORKFLOW: &str = "design_space_merge";

#[derive(Debug, Clone)]
pub struct CreateDesignSpaceMergeRequest {
    pub title: String,
    pub description: String,
    pub source_branch: String,
    pub target_branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesignSpaceGatewayError {
    Conflict(String),
    NotFound(String),
    Backend(String),
}

impl std::fmt::Display for DesignSpaceGatewayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict(message) | Self::NotFound(message) | Self::Backend(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl From<DesignSpaceGatewayError> for ServiceError {
    fn from(error: DesignSpaceGatewayError) -> Self {
        match error {
            DesignSpaceGatewayError::Conflict(message)
            | DesignSpaceGatewayError::NotFound(message) => Self::Validation(message),
            DesignSpaceGatewayError::Backend(message) => Self::Internal(message),
        }
    }
}

/// Branch and diff access for the design-space server.
#[async_trait]
pub trait DesignSpaceGateway: Send + Sync {
    async fn branches(&self, project: &Project) -> Result<Vec<String>, DesignSpaceGatewayError>;

    async fn branch_head(
        &self,
        project: &Project,
        branch: &str,
    ) -> Result<Option<String>, DesignSpaceGatewayError>;

    /// Digest of design content at `commit`, excluding review comments.
    async fn content_digest(
        &self,
        project: &Project,
        commit: &str,
    ) -> Result<String, DesignSpaceGatewayError>;

    async fn is_ancestor(
        &self,
        project: &Project,
        ancestor: &str,
        descendant: &str,
    ) -> Result<bool, DesignSpaceGatewayError>;

    async fn diff(
        &self,
        project: &Project,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<MergeRequestDiff, DesignSpaceGatewayError>;

    /// Count design-document comments reachable from `commit`.
    async fn comment_count(
        &self,
        project: &Project,
        commit: &str,
    ) -> Result<usize, DesignSpaceGatewayError>;

    /// Move `target_branch` to `source_commit` only when the current target
    /// still equals `expected_target` and target is an ancestor of source.
    async fn fast_forward(
        &self,
        project: &Project,
        target_branch: &str,
        expected_target: &str,
        source_commit: &str,
    ) -> Result<(), DesignSpaceGatewayError>;

    async fn delete_branch(
        &self,
        project: &Project,
        branch: &str,
    ) -> Result<(), DesignSpaceGatewayError>;
}

/// Gateway used when Foundry was started against a design-space server it
/// cannot read locally. Listing and merging are unavailable until a remote
/// gateway is configured.
pub struct UnavailableDesignSpaceGateway;

#[async_trait]
impl DesignSpaceGateway for UnavailableDesignSpaceGateway {
    async fn branches(&self, _project: &Project) -> Result<Vec<String>, DesignSpaceGatewayError> {
        Err(unavailable())
    }

    async fn branch_head(
        &self,
        _project: &Project,
        _branch: &str,
    ) -> Result<Option<String>, DesignSpaceGatewayError> {
        Err(unavailable())
    }

    async fn content_digest(
        &self,
        _project: &Project,
        _commit: &str,
    ) -> Result<String, DesignSpaceGatewayError> {
        Err(unavailable())
    }

    async fn is_ancestor(
        &self,
        _project: &Project,
        _ancestor: &str,
        _descendant: &str,
    ) -> Result<bool, DesignSpaceGatewayError> {
        Err(unavailable())
    }

    async fn diff(
        &self,
        _project: &Project,
        _from_commit: &str,
        _to_commit: &str,
    ) -> Result<MergeRequestDiff, DesignSpaceGatewayError> {
        Err(unavailable())
    }

    async fn comment_count(
        &self,
        _project: &Project,
        _commit: &str,
    ) -> Result<usize, DesignSpaceGatewayError> {
        Err(unavailable())
    }

    async fn fast_forward(
        &self,
        _project: &Project,
        _target_branch: &str,
        _expected_target: &str,
        _source_commit: &str,
    ) -> Result<(), DesignSpaceGatewayError> {
        Err(unavailable())
    }

    async fn delete_branch(
        &self,
        _project: &Project,
        _branch: &str,
    ) -> Result<(), DesignSpaceGatewayError> {
        Err(unavailable())
    }
}

fn unavailable() -> DesignSpaceGatewayError {
    DesignSpaceGatewayError::Backend(
        "本地设计空间不可用；合并请求需要可读写的设计空间服务器".to_string(),
    )
}

#[derive(Debug, Clone)]
pub struct MergeRequestHead {
    pub source_branch: String,
    pub target_branch: String,
    pub source_commit: String,
    pub target_commit: String,
}

#[async_trait]
pub trait MergeRequestRepository: Send + Sync {
    async fn insert_with_task(
        &self,
        merge_request: &DesignSpaceMergeRequestDAO,
        task: &TaskDAO,
        actors: &[TaskActorDAO],
        events: &[DomainEvent],
    ) -> Result<(), RepoError>;

    async fn insert(&self, merge_request: &DesignSpaceMergeRequestDAO) -> Result<(), RepoError>;

    async fn find_for_task(
        &self,
        project_id: Uuid,
        task_id: Uuid,
    ) -> Result<Option<DesignSpaceMergeRequestDAO>, RepoError>;

    async fn update_commits(
        &self,
        project_id: Uuid,
        id: Uuid,
        head: &MergeRequestHead,
        now: OffsetDateTime,
    ) -> Result<bool, RepoError>;

    async fn get(
        &self,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<Option<DesignSpaceMergeRequestDAO>, RepoError>;

    async fn list(&self, project_id: Uuid) -> Result<Vec<DesignSpaceMergeRequestDAO>, RepoError>;

    async fn save_merge_commit(
        &self,
        project_id: Uuid,
        id: Uuid,
        merge_commit: &str,
        now: OffsetDateTime,
    ) -> Result<bool, RepoError>;

    async fn save_approval(
        &self,
        project_id: Uuid,
        id: Uuid,
        content_digest: &str,
        source_commit: &str,
        approved_by: Uuid,
        now: OffsetDateTime,
    ) -> Result<bool, RepoError>;
}

pub struct MergeRequestService {
    repository: Arc<dyn MergeRequestRepository>,
    tasks: Arc<dyn TaskUseCases>,
    dispatcher: Arc<dyn TaskDispatcher>,
    workflows: Arc<dyn WorkflowCatalog>,
    accounts: Arc<dyn AccountRepository>,
    design_space: Arc<dyn DesignSpaceGateway>,
}

impl MergeRequestService {
    pub fn new(
        repository: Arc<dyn MergeRequestRepository>,
        tasks: Arc<dyn TaskUseCases>,
        dispatcher: Arc<dyn TaskDispatcher>,
        workflows: Arc<dyn WorkflowCatalog>,
        accounts: Arc<dyn AccountRepository>,
        design_space: Arc<dyn DesignSpaceGateway>,
    ) -> Self {
        Self {
            repository,
            tasks,
            dispatcher,
            workflows,
            accounts,
            design_space,
        }
    }

    pub async fn branches(&self, project: &Project) -> Result<Vec<String>, ServiceError> {
        Ok(self.design_space.branches(project).await?)
    }

    /// Create or refresh the MR for an existing task. The task is the review
    /// task; no second `design_space_merge` task is created.
    pub async fn create_for_task(
        &self,
        project: &Project,
        task_id: Uuid,
        account_id: Uuid,
        command: CreateDesignSpaceMergeRequest,
    ) -> Result<MergeRequestReview, ServiceError> {
        let task = self
            .tasks
            .get(project.id, task_id, TaskViewer::member(account_id))
            .await?;
        if !task
            .actors
            .iter()
            .any(|actor| actor.account_id == account_id && actor.active)
        {
            return Err(ServiceError::Forbidden(
                "当前账号不是这个任务的成员".to_string(),
            ));
        }
        let title = command.title.trim().to_string();
        if title.is_empty() {
            return Err(ServiceError::Validation(
                "merge request title is required".to_string(),
            ));
        }
        let source_branch = command.source_branch.trim().to_string();
        let target_branch = command.target_branch.trim().to_string();
        if source_branch.is_empty() || target_branch.is_empty() {
            return Err(ServiceError::Validation(
                "source and target branch are required".to_string(),
            ));
        }
        if source_branch == target_branch {
            return Err(ServiceError::Validation(
                "source and target branch must be different".to_string(),
            ));
        }
        let source_commit = self
            .design_space
            .branch_head(project, &source_branch)
            .await?
            .ok_or_else(|| ServiceError::Validation(format!("分支不存在: {source_branch}")))?;
        let target_commit = self
            .design_space
            .branch_head(project, &target_branch)
            .await?
            .ok_or_else(|| ServiceError::Validation(format!("分支不存在: {target_branch}")))?;
        if !self
            .design_space
            .is_ancestor(project, &target_commit, &source_commit)
            .await?
        {
            return Err(ServiceError::Validation(
                "source branch does not contain the target branch; rebase before opening the merge request"
                    .to_string(),
            ));
        }
        let now = OffsetDateTime::now_utc();
        if let Some(existing) = self.repository.find_for_task(project.id, task_id).await? {
            self.repository
                .update_commits(
                    project.id,
                    existing.id,
                    &MergeRequestHead {
                        source_branch: source_branch.clone(),
                        target_branch: target_branch.clone(),
                        source_commit: source_commit.clone(),
                        target_commit: target_commit.clone(),
                    },
                    now,
                )
                .await?;
            let request = self.load(project.id, existing.id).await?;
            return self
                .view(project, &request, TaskViewer::member(account_id))
                .await;
        }
        let merge_request = DesignSpaceMergeRequestDAO {
            id: Uuid::now_v7(),
            project_id: project.id,
            title,
            description: command.description.trim().to_string(),
            source_branch,
            target_branch,
            source_commit,
            target_commit,
            task_id,
            merge_commit: None,
            approved_content_digest: None,
            approved_source_commit: None,
            approved_by: None,
            approved_at: None,
            created_by: account_id,
            created_at: now,
            updated_at: now,
        };
        self.repository.insert(&merge_request).await?;
        self.view(project, &merge_request, TaskViewer::member(account_id))
            .await
    }

    pub async fn create(
        &self,
        project: &Project,
        account_id: Uuid,
        command: CreateDesignSpaceMergeRequest,
    ) -> Result<MergeRequestReview, ServiceError> {
        let title = command.title.trim().to_string();
        if title.is_empty() {
            return Err(ServiceError::Validation(
                "merge request title is required".to_string(),
            ));
        }
        let source_branch = command.source_branch.trim().to_string();
        let target_branch = command.target_branch.trim().to_string();
        if source_branch.is_empty() || target_branch.is_empty() {
            return Err(ServiceError::Validation(
                "source and target branch are required".to_string(),
            ));
        }
        if source_branch == target_branch {
            return Err(ServiceError::Validation(
                "source and target branch must be different".to_string(),
            ));
        }
        let source_commit = self
            .design_space
            .branch_head(project, &source_branch)
            .await?
            .ok_or_else(|| ServiceError::Validation(format!("分支不存在: {source_branch}")))?;
        let target_commit = self
            .design_space
            .branch_head(project, &target_branch)
            .await?
            .ok_or_else(|| ServiceError::Validation(format!("分支不存在: {target_branch}")))?;
        if source_commit == target_commit {
            return Err(ServiceError::Validation(
                "source branch has no new commit".to_string(),
            ));
        }
        if !self
            .design_space
            .is_ancestor(project, &target_commit, &source_commit)
            .await?
        {
            return Err(ServiceError::Validation(
                "source branch does not contain the target branch; rebase before opening the merge request"
                    .to_string(),
            ));
        }

        let reference = match self
            .workflows
            .latest_visible(None, &WorkflowId::new(MERGE_REQUEST_WORKFLOW))
            .await?
        {
            WorkflowLookup::Published(reference) => reference,
            WorkflowLookup::Unpublished(id) => {
                return Err(ServiceError::Unpublished(id.to_string()));
            }
            WorkflowLookup::NotFound => {
                return Err(ServiceError::Internal(format!(
                    "workflow {MERGE_REQUEST_WORKFLOW} is not registered"
                )));
            }
        };

        let reviewer = self.reviewer_account(project, account_id).await?;
        let parent_id = project.root_task_id.ok_or_else(|| {
            ServiceError::Validation("项目还没有根任务，不能创建合并请求".to_string())
        })?;
        let now = OffsetDateTime::now_utc();
        let merge_request_id = Uuid::now_v7();
        let task = TaskDAO::new(
            project.id,
            Some(parent_id),
            reference.workflow_id,
            reference.version,
            title.clone(),
            now,
        );
        let actor = TaskActorDAO {
            task_id: task.id,
            role_key: RoleKey::new("reviewer"),
            account_id: reviewer,
            active: false,
            work: None,
        };
        let events = [DomainEvent::new(
            DomainEventKind::TaskCreated,
            task.id,
            Actor::System,
            json!({
                "workflow": task.workflow,
                "workflow_version": task.workflow_version,
                "title": task.title,
                "parent_id": task.parent_id,
                "dependencies": [],
            }),
        )];
        let merge_request = DesignSpaceMergeRequestDAO {
            id: merge_request_id,
            project_id: project.id,
            title,
            description: command.description.trim().to_string(),
            source_branch,
            target_branch,
            source_commit,
            target_commit,
            task_id: task.id,
            merge_commit: None,
            approved_content_digest: None,
            approved_source_commit: None,
            approved_by: None,
            approved_at: None,
            created_by: account_id,
            created_at: now,
            updated_at: now,
        };
        self.repository
            .insert_with_task(&merge_request, &task, &[actor], &events)
            .await?;

        // The request and Task are committed. Initialization is best effort in
        // the same way as project root creation; the TaskCreated event is
        // durable and the dispatcher retries it.
        if let Err(error) = self.dispatcher.process(task.id, None).await {
            tracing::warn!(
                task_id = %task.id,
                error = %error,
                "merge request created but review task initialization failed"
            );
        }
        if project.skip_human_review {
            self.approve(project, merge_request_id, reviewer).await?;
            let mut updated = self.load(project.id, merge_request_id).await?;
            if updated.merge_commit.is_none() {
                let task = self
                    .tasks
                    .get(project.id, updated.task_id, TaskViewer::member(reviewer))
                    .await?;
                if task.stage.as_ref().map(|stage| stage.as_str()) == Some("merge") {
                    self.merge(project, merge_request_id, reviewer).await?;
                    updated = self.load(project.id, merge_request_id).await?;
                }
            }
            return self
                .view(project, &updated, TaskViewer::member(account_id))
                .await;
        }
        self.view(project, &merge_request, TaskViewer::member(account_id))
            .await
    }

    pub async fn list(
        &self,
        project: &Project,
        viewer: TaskViewer,
    ) -> Result<Vec<MergeRequestReview>, ServiceError> {
        let mut requests = self.repository.list(project.id).await?;
        requests.sort_by_key(|request| std::cmp::Reverse(request.created_at));
        let mut views = Vec::with_capacity(requests.len());
        for request in requests {
            views.push(self.view(project, &request, viewer).await?);
        }
        Ok(views)
    }

    pub async fn has_merge_request(
        &self,
        project_id: Uuid,
        task_id: Uuid,
    ) -> Result<bool, ServiceError> {
        Ok(self
            .repository
            .find_for_task(project_id, task_id)
            .await?
            .is_some())
    }

    /// Compact task-scoped view: MR metadata, review Task and status, without
    /// the full design diff. The diff can contain tens of thousands of object
    /// changes and is not needed by review agents that only check branch/head.
    pub async fn for_task_review(
        &self,
        project: &Project,
        task_id: Uuid,
        viewer: TaskViewer,
    ) -> Result<Option<MergeRequestReview>, ServiceError> {
        let Some(request) = self.repository.find_for_task(project.id, task_id).await? else {
            return Ok(None);
        };
        let task = self.tasks.get(project.id, request.task_id, viewer).await?;
        let status = request.view().status_from(
            task.state,
            task.stage.as_ref().map(|stage| stage.as_str()),
            task.result.as_ref().map(|result| result.as_str()),
        );
        Ok(Some(MergeRequestReview {
            merge_request: request.view(),
            task,
            status,
        }))
    }

    pub async fn for_task(
        &self,
        project: &Project,
        task_id: Uuid,
        viewer: TaskViewer,
    ) -> Result<Option<MergeRequestDetail>, ServiceError> {
        let Some(request) = self.repository.find_for_task(project.id, task_id).await? else {
            return Ok(None);
        };
        self.get(project, request.id, viewer).await.map(Some)
    }

    /// MR metadata and review status without computing the design diff. The
    /// diff is loaded separately when the page's changes section is expanded.
    pub async fn review(
        &self,
        project: &Project,
        id: Uuid,
        viewer: TaskViewer,
    ) -> Result<MergeRequestReview, ServiceError> {
        let request = self.load(project.id, id).await?;
        let task = self.tasks.get(project.id, request.task_id, viewer).await?;
        let status = request.view().status_from(
            task.state,
            task.stage.as_ref().map(|stage| stage.as_str()),
            task.result.as_ref().map(|result| result.as_str()),
        );
        Ok(MergeRequestReview {
            merge_request: request.view(),
            task,
            status,
        })
    }

    pub async fn get(
        &self,
        project: &Project,
        id: Uuid,
        viewer: TaskViewer,
    ) -> Result<MergeRequestDetail, ServiceError> {
        let request = self.load(project.id, id).await?;
        let task = self.tasks.get(project.id, request.task_id, viewer).await?;
        let status = request.view().status_from(
            task.state,
            task.stage.as_ref().map(|stage| stage.as_str()),
            task.result.as_ref().map(|result| result.as_str()),
        );
        let diff = self
            .design_space
            .diff(project, &request.target_commit, &request.source_commit)
            .await?;
        Ok(MergeRequestDetail {
            merge_request: request.view(),
            task,
            status,
            changes: diff.changes,
            documents: diff.documents,
        })
    }

    /// Record a human approval of the current design content and send the
    /// approved operation on the review Task.
    ///
    /// The approval is bound to a content digest that excludes comments. The
    /// subsequent mandatory comment cleanup is therefore allowed to create a
    /// new commit without asking the human to approve the same content twice.
    pub async fn approve(
        &self,
        project: &Project,
        id: Uuid,
        account_id: Uuid,
    ) -> Result<MergeRequestReview, ServiceError> {
        let request = self.load(project.id, id).await?;
        let viewer = TaskViewer::member(account_id);
        let task = self.tasks.get(project.id, request.task_id, viewer).await?;
        if request.merge_commit.is_some() || task.state == TaskState::Completed {
            return Err(ServiceError::Validation(
                "合并请求已经结束，不能再审批".to_string(),
            ));
        }
        let operation_suffix = |operation: &foundry_domain::AllowedOperation| {
            operation
                .name
                .as_str()
                .rsplit_once(':')
                .map(|(_, name)| name.to_string())
                .unwrap_or_else(|| operation.name.as_str().to_string())
        };
        let mechanical_merge = task
            .allowed_operations
            .iter()
            .find(|operation| operation_suffix(operation) == DESIGN_SPACE_MERGE_OPERATION)
            .map(|operation| operation.name.as_str().to_string());
        let approval_operation = task
            .allowed_operations
            .iter()
            .find(|operation| {
                let name = operation.name.as_str();
                name.ends_with(":approved") || name.ends_with(":review_approved")
            })
            .map(|operation| operation.name.as_str().to_string());
        if mechanical_merge.is_none() && approval_operation.is_none() {
            return Err(ServiceError::Forbidden(
                "当前账号不是这个合并请求的真人审核人，或当前阶段不能审批".to_string(),
            ));
        }
        let conflict_operation = task
            .allowed_operations
            .iter()
            .find(|operation| operation_suffix(operation) == "conflict")
            .map(|operation| operation.name.as_str().to_string());
        let source_head = self
            .design_space
            .branch_head(project, &request.source_branch)
            .await?
            .ok_or_else(|| {
                ServiceError::Validation(format!("源分支不存在: {}", request.source_branch))
            })?;
        if source_head != request.source_commit {
            return Err(ServiceError::Validation(
                "源分支在合并请求刷新后又有新提交；请先刷新合并请求".to_string(),
            ));
        }
        let digest = self
            .design_space
            .content_digest(project, &request.source_commit)
            .await?;
        if !self
            .repository
            .save_approval(
                project.id,
                request.id,
                &digest,
                &request.source_commit,
                account_id,
                OffsetDateTime::now_utc(),
            )
            .await?
        {
            return Err(ServiceError::Validation(
                "合并请求已经结束，不能再审批".to_string(),
            ));
        }
        if mechanical_merge.is_some() {
            // Human approval triggers the mechanical merge immediately. A
            // failed merge (conflict or remaining review comments) is handed
            // to the conflict resolver; the human approves again afterwards.
            match self.merge(project, id, account_id).await {
                Ok(view) => return Ok(view),
                Err(error) => {
                    let current = self.load(project.id, id).await?;
                    if current.merge_commit.is_some() {
                        return self.view(project, &current, viewer).await;
                    }
                    let Some(conflict_operation) = conflict_operation else {
                        return Err(error);
                    };
                    self.tasks
                        .send_operation(
                            project.id,
                            request.task_id,
                            account_id,
                            OperationName::new(conflict_operation),
                            format!(
                                "design-space-conflict:{}:{}",
                                request.id, request.source_commit
                            ),
                        )
                        .await?;
                    let updated = self.load(project.id, id).await?;
                    return self.view(project, &updated, viewer).await;
                }
            }
        }
        let Some(approval_operation) = approval_operation else {
            return Err(ServiceError::Forbidden(
                "当前账号不是这个合并请求的真人审核人，或当前阶段不能审批".to_string(),
            ));
        };
        self.tasks
            .send_operation(
                project.id,
                request.task_id,
                account_id,
                OperationName::new(approval_operation),
                format!(
                    "design-space-approval:{}:{}",
                    request.id, request.source_commit
                ),
            )
            .await?;
        let updated = self.load(project.id, id).await?;
        self.view(project, &updated, viewer).await
    }

    /// Fast-forward the target branch, then record the merge as a Task
    /// operation. The branch move is the only external side effect; the Task
    /// remains the approval and audit source.
    pub async fn merge(
        &self,
        project: &Project,
        id: Uuid,
        account_id: Uuid,
    ) -> Result<MergeRequestReview, ServiceError> {
        let request = self.load(project.id, id).await?;
        let viewer = TaskViewer::member(account_id);
        let task = self.tasks.get(project.id, request.task_id, viewer).await?;
        let merge_operation = task
            .allowed_operations
            .iter()
            .find(|operation| {
                operation
                    .name
                    .as_str()
                    .rsplit_once(':')
                    .map(|(_, name)| name)
                    .unwrap_or(operation.name.as_str())
                    == DESIGN_SPACE_MERGE_OPERATION
            })
            .map(|operation| operation.name.as_str().to_string());
        if task.state == foundry_domain::TaskState::Completed {
            let result = task.result.as_ref().map(|result| result.as_str());
            if result != Some("merged") {
                return Err(ServiceError::Validation(
                    "合并请求已经结束，不能再合并".to_string(),
                ));
            }
        }
        if task.state != foundry_domain::TaskState::Completed && merge_operation.is_none() {
            return Err(ServiceError::Forbidden(
                "当前账号不能合并这个合并请求".to_string(),
            ));
        }

        if request.merge_commit.is_none() {
            let source_head = self
                .design_space
                .branch_head(project, &request.source_branch)
                .await?
                .ok_or_else(|| {
                    ServiceError::Validation(format!("源分支不存在: {}", request.source_branch))
                })?;
            if source_head != request.source_commit {
                return Err(ServiceError::Validation(
                    "源分支在合并请求刷新后又有新提交；请先刷新合并请求".to_string(),
                ));
            }
            // Integration branches are temporary review branches: comments are
            // allowed to travel with them. Only a protected branch requires a
            // human-approved, comment-free source, because comments must never
            // enter it.
            if project.is_protected_design_branch(&request.target_branch) {
                if !project.skip_human_review {
                    let approved = request.approved_content_digest.as_deref().ok_or_else(|| {
                        ServiceError::Validation(
                            "合并到受保护分支前需要人类审批设计内容".to_string(),
                        )
                    })?;
                    let current = self
                        .design_space
                        .content_digest(project, &request.source_commit)
                        .await?;
                    if current != approved {
                        return Err(ServiceError::Validation(
                            "审批后设计内容发生变化；需要重新审批".to_string(),
                        ));
                    }
                }
                if !project.skip_human_review {
                    let comments = self
                        .design_space
                        .comment_count(project, &request.source_commit)
                        .await?;
                    if comments > 0 {
                        return Err(ServiceError::Validation(format!(
                            "源分支还有 {comments} 条设计评论；合并到受保护分支前请清理评论并刷新合并请求"
                        )));
                    }
                }
            }
            self.design_space
                .fast_forward(
                    project,
                    &request.target_branch,
                    &request.target_commit,
                    &request.source_commit,
                )
                .await?;
            if !self
                .repository
                .save_merge_commit(
                    project.id,
                    request.id,
                    &request.source_commit,
                    OffsetDateTime::now_utc(),
                )
                .await?
            {
                return Err(ServiceError::Validation(
                    "合并请求已经被其他操作合并".to_string(),
                ));
            }
        }

        if let Some(operation) = merge_operation {
            self.tasks
                .send_operation(
                    project.id,
                    request.task_id,
                    account_id,
                    OperationName::new(operation),
                    format!("design-space-merge:{}", request.id),
                )
                .await?;
        }
        let updated = self.load(project.id, id).await?;
        self.view(project, &updated, viewer).await
    }

    pub async fn delete_branch(&self, project: &Project, branch: &str) -> Result<(), ServiceError> {
        if project.is_protected_design_branch(branch) {
            return Err(ServiceError::Forbidden(format!(
                "受保护分支不能删除: {branch}"
            )));
        }
        Ok(self.design_space.delete_branch(project, branch).await?)
    }

    pub async fn load(
        &self,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<DesignSpaceMergeRequestDAO, ServiceError> {
        self.repository
            .get(project_id, id)
            .await?
            .ok_or_else(|| ServiceError::not_found("merge request", id))
    }

    async fn view(
        &self,
        project: &Project,
        request: &DesignSpaceMergeRequestDAO,
        viewer: TaskViewer,
    ) -> Result<MergeRequestReview, ServiceError> {
        let task = self.tasks.get(project.id, request.task_id, viewer).await?;
        let status = request.view().status_from(
            task.state,
            task.stage.as_ref().map(|stage| stage.as_str()),
            task.result.as_ref().map(|result| result.as_str()),
        );
        Ok(MergeRequestReview {
            merge_request: request.view(),
            task,
            status,
        })
    }

    async fn reviewer_account(
        &self,
        project: &Project,
        creator: Uuid,
    ) -> Result<Uuid, ServiceError> {
        let mut candidates = Vec::new();
        for member in &project.members {
            let Some(account) = self.accounts.get(member.account_id).await? else {
                continue;
            };
            if account.kind != AccountKind::Human || !account.is_active() {
                continue;
            }
            candidates.push((member.role, account.id));
        }
        candidates.sort_by(|left, right| right.0.cmp(&left.0));
        if let Some((_, account_id)) = candidates.iter().find(|(_, id)| *id != creator) {
            return Ok(*account_id);
        }
        candidates
            .into_iter()
            .next()
            .map(|(_, account_id)| account_id)
            .ok_or_else(|| {
                ServiceError::Forbidden("项目没有可用的真人审核人；请先添加项目成员".to_string())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_domain::ProjectRole;

    #[test]
    fn prefers_a_reviewer_other_than_the_creator() {
        let roles = [
            (ProjectRole::Owner, Uuid::now_v7()),
            (ProjectRole::Maintainer, Uuid::now_v7()),
        ];
        let creator = roles[0].1;
        let selected = roles
            .iter()
            .find(|(_, id)| *id != creator)
            .map(|(_, id)| *id)
            .expect("another reviewer");
        assert_ne!(selected, creator);
    }
}
