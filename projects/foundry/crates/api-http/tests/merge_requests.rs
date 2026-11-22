//! Merge requests are approved and merged through their review Task.

use async_trait::async_trait;
use foundry_adapter_memory::{InMemoryEventSink, InMemoryStore, InMemoryTaskDispatcher};
use foundry_app::{
    AccountService, CreateAccount, CreateDesignSpaceMergeRequest, CreateProject,
    DesignSpaceGateway, DesignSpaceGatewayError, MergeRequestRepository, MergeRequestService,
    ProjectService, TaskActorRow, TaskRepository, TaskService, TaskViewer, WorkflowCatalog,
};
use foundry_domain::{
    AccountKind, AccountRole, MergeRequestChange, MergeRequestChangeStatus, MergeRequestStatus,
    OperationName, ProjectRole, WorkflowId,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TARGET: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OTHER: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[derive(Default)]
struct FakeGateway {
    target: Mutex<String>,
    source: Mutex<String>,
    digest: Mutex<String>,
    comments: AtomicUsize,
}

#[async_trait]
impl DesignSpaceGateway for FakeGateway {
    async fn branches(
        &self,
        _project: &foundry_domain::Project,
    ) -> Result<Vec<String>, DesignSpaceGatewayError> {
        Ok(vec![
            "main".to_string(),
            "feature".to_string(),
            "integration".to_string(),
        ])
    }

    async fn branch_head(
        &self,
        _project: &foundry_domain::Project,
        branch: &str,
    ) -> Result<Option<String>, DesignSpaceGatewayError> {
        Ok(Some(match branch {
            "main" | "integration" => self.target.lock().unwrap().clone(),
            "feature" => self.source.lock().unwrap().clone(),
            _ => return Ok(None),
        }))
    }

    async fn content_digest(
        &self,
        _project: &foundry_domain::Project,
        _commit: &str,
    ) -> Result<String, DesignSpaceGatewayError> {
        Ok(self.digest.lock().unwrap().clone())
    }

    async fn is_ancestor(
        &self,
        _project: &foundry_domain::Project,
        ancestor: &str,
        descendant: &str,
    ) -> Result<bool, DesignSpaceGatewayError> {
        Ok(ancestor == TARGET || ancestor == SOURCE || descendant == SOURCE)
    }

    async fn diff(
        &self,
        _project: &foundry_domain::Project,
        _from_commit: &str,
        _to_commit: &str,
    ) -> Result<Vec<MergeRequestChange>, DesignSpaceGatewayError> {
        Ok(vec![MergeRequestChange {
            status: MergeRequestChangeStatus::Modified,
            qname: "namespaces/app/Service".to_string(),
            kind: "class".to_string(),
        }])
    }

    async fn comment_count(
        &self,
        _project: &foundry_domain::Project,
        _commit: &str,
    ) -> Result<usize, DesignSpaceGatewayError> {
        Ok(self.comments.load(Ordering::SeqCst))
    }

    async fn fast_forward(
        &self,
        _project: &foundry_domain::Project,
        _target_branch: &str,
        expected_target: &str,
        source_commit: &str,
    ) -> Result<(), DesignSpaceGatewayError> {
        let mut target = self.target.lock().unwrap();
        if target.as_str() != expected_target {
            return Err(DesignSpaceGatewayError::Conflict(
                "target moved".to_string(),
            ));
        }
        *target = source_commit.to_string();
        Ok(())
    }

    async fn delete_branch(
        &self,
        _project: &foundry_domain::Project,
        _branch: &str,
    ) -> Result<(), DesignSpaceGatewayError> {
        Ok(())
    }
}

struct Harness {
    project: foundry_domain::Project,
    admin: foundry_domain::Account,
    reviewer: foundry_domain::Account,
    service: MergeRequestService,
    tasks: Arc<TaskService>,
    gateway: Arc<FakeGateway>,
    store: Arc<InMemoryStore>,
}

async fn harness() -> Harness {
    harness_with_skip(false).await
}

async fn harness_with_skip(skip_human_review: bool) -> Harness {
    let store = Arc::new(InMemoryStore::new());
    store.seed_builtin_workflows().unwrap();
    let events = Arc::new(InMemoryEventSink::new());
    let accounts = Arc::new(AccountService::new(Arc::clone(&store), Arc::clone(&events)));
    let admin = accounts
        .create_first_admin("admin", "Administrator", "admin-password")
        .await
        .unwrap();
    let reviewer = accounts
        .create(
            admin.actor(),
            CreateAccount {
                handle: "reviewer".to_string(),
                display_name: "Reviewer".to_string(),
                kind: AccountKind::Human,
                role: AccountRole::Member,
                email: None,
                password: "reviewer-password".to_string(),
            },
        )
        .await
        .unwrap();

    let catalog: Arc<dyn WorkflowCatalog> = store.clone();
    let dispatcher = Arc::new(InMemoryTaskDispatcher::new(Arc::clone(&store)));
    let projects = Arc::new(ProjectService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&events),
        Arc::clone(&catalog),
        dispatcher.clone(),
        Arc::new(TestDesignSpaces),
    ));
    let mut project = projects
        .create(
            admin.actor(),
            CreateProject {
                name: "Merge requests".to_string(),
                description: None,
                git_repository: "git@example.com:test/repo.git".to_string(),
                git_branch: None,
                root_workflow_id: WorkflowId::new("project"),
            },
        )
        .await
        .unwrap();
    project = projects
        .add_member_by_handle(
            admin.actor(),
            foundry_app::AddProjectMemberByHandle {
                project_id: project.id,
                handle: reviewer.handle.clone(),
                role: ProjectRole::Maintainer,
            },
        )
        .await
        .unwrap();

    let task_repository: Arc<dyn TaskRepository> = store.clone();
    let tasks = Arc::new(TaskService::new(
        task_repository,
        dispatcher.clone(),
        Arc::clone(&catalog),
    ));
    let gateway = Arc::new(FakeGateway {
        target: Mutex::new(TARGET.to_string()),
        source: Mutex::new(SOURCE.to_string()),
        digest: Mutex::new("test-content-digest".to_string()),
        comments: AtomicUsize::new(0),
    });
    project.skip_human_review = skip_human_review;
    let service = MergeRequestService::new(
        Arc::clone(&store) as Arc<dyn MergeRequestRepository>,
        tasks.clone(),
        dispatcher,
        catalog,
        Arc::clone(&store) as Arc<dyn foundry_app::AccountRepository>,
        gateway.clone(),
    );
    Harness {
        project,
        admin,
        reviewer,
        service,
        tasks,
        gateway,
        store,
    }
}

struct TestDesignSpaces;

#[async_trait]
impl foundry_app::DesignSpaceProvisioner for TestDesignSpaces {
    async fn create_design_space(
        &self,
        repo: &str,
        _branch: &str,
        _source_url: &str,
    ) -> Result<String, String> {
        Ok(format!("http://test/{repo}"))
    }
}

#[tokio::test]
async fn skip_human_review_auto_approves_and_merges_a_new_request() {
    let harness = harness_with_skip(true).await;
    let view = harness
        .service
        .create(
            &harness.project,
            harness.admin.id,
            CreateDesignSpaceMergeRequest {
                title: "Skip review".to_string(),
                description: String::new(),
                source_branch: "feature".to_string(),
                target_branch: "main".to_string(),
            },
        )
        .await
        .unwrap();
    assert_eq!(view.status, MergeRequestStatus::Merged);
    assert_eq!(view.task.state, foundry_domain::TaskState::Completed);
    assert_eq!(view.task.result.as_ref().unwrap().as_str(), "merged");
    assert_eq!(harness.gateway.target.lock().unwrap().as_str(), SOURCE);
}

#[tokio::test]
async fn approval_and_merge_are_task_operations() {
    let harness = harness().await;
    let view = harness
        .service
        .create(
            &harness.project,
            harness.admin.id,
            CreateDesignSpaceMergeRequest {
                title: "Land feature".to_string(),
                description: "Move the design space forward".to_string(),
                source_branch: "feature".to_string(),
                target_branch: "main".to_string(),
            },
        )
        .await
        .unwrap();
    assert_eq!(view.status, MergeRequestStatus::Reviewing);
    let request_id = view.merge_request.id;
    let task_id = view.merge_request.task_id;

    let actors: Vec<TaskActorRow> = TaskRepository::actors(&*harness.store, task_id)
        .await
        .unwrap();
    assert_eq!(actors.len(), 1);
    assert_eq!(actors[0].account_id, harness.reviewer.id);
    assert_eq!(actors[0].role_key.as_str(), "reviewer");

    // The creator cannot merge before the reviewer approves.
    assert!(
        harness
            .service
            .merge(&harness.project, request_id, harness.admin.id)
            .await
            .is_err()
    );

    harness
        .service
        .approve(&harness.project, request_id, harness.reviewer.id)
        .await
        .unwrap();
    let detail = harness
        .service
        .get(
            &harness.project,
            request_id,
            TaskViewer::member(harness.reviewer.id),
        )
        .await
        .unwrap();
    assert_eq!(detail.status, MergeRequestStatus::Approved);
    assert!(detail.task.allowed_operations.iter().any(|operation| {
        operation
            .name
            .as_str()
            .rsplit_once(':')
            .map(|(_, name)| name)
            .unwrap_or(operation.name.as_str())
            == "merge"
    }));

    let merged = harness
        .service
        .merge(&harness.project, request_id, harness.reviewer.id)
        .await
        .unwrap();
    assert_eq!(merged.status, MergeRequestStatus::Merged);
    assert_eq!(merged.task.state, foundry_domain::TaskState::Completed);
    assert_eq!(merged.task.result.as_ref().unwrap().as_str(), "merged");
    assert_eq!(merged.merge_request.merge_commit.as_deref(), Some(SOURCE));
    assert_eq!(harness.gateway.target.lock().unwrap().as_str(), SOURCE);

    // The merge request status comes from the Task, not a separate column.
    let stored = MergeRequestRepository::get(&*harness.store, harness.project.id, request_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.view().status_from(
            merged.task.state,
            merged.task.stage.as_ref().map(|stage| stage.as_str()),
            merged.task.result.as_ref().map(|result| result.as_str()),
        ),
        MergeRequestStatus::Merged
    );
}

#[tokio::test]
async fn merge_refuses_a_source_branch_with_comments() {
    let harness = harness().await;
    let view = harness
        .service
        .create(
            &harness.project,
            harness.admin.id,
            CreateDesignSpaceMergeRequest {
                title: "Land with comments".to_string(),
                description: String::new(),
                source_branch: "feature".to_string(),
                target_branch: "main".to_string(),
            },
        )
        .await
        .unwrap();
    harness
        .service
        .approve(&harness.project, view.merge_request.id, harness.reviewer.id)
        .await
        .unwrap();
    harness.gateway.comments.store(2, Ordering::SeqCst);
    let error = harness
        .service
        .merge(&harness.project, view.merge_request.id, harness.reviewer.id)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("设计评论"), "{error}");
    assert_eq!(harness.gateway.target.lock().unwrap().as_str(), TARGET);

    harness.gateway.comments.store(0, Ordering::SeqCst);
    let merged = harness
        .service
        .merge(&harness.project, view.merge_request.id, harness.reviewer.id)
        .await
        .unwrap();
    assert_eq!(merged.status, MergeRequestStatus::Merged);
}

#[tokio::test]
async fn merge_rejects_design_content_changed_after_approval() {
    let harness = harness().await;
    let view = harness
        .service
        .create(
            &harness.project,
            harness.admin.id,
            CreateDesignSpaceMergeRequest {
                title: "Approved then changed".to_string(),
                description: String::new(),
                source_branch: "feature".to_string(),
                target_branch: "main".to_string(),
            },
        )
        .await
        .unwrap();
    harness
        .service
        .approve(&harness.project, view.merge_request.id, harness.reviewer.id)
        .await
        .unwrap();
    *harness.gateway.digest.lock().unwrap() = "changed-content-digest".to_string();
    harness.gateway.comments.store(0, Ordering::SeqCst);
    let error = harness
        .service
        .merge(&harness.project, view.merge_request.id, harness.reviewer.id)
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("审批后设计内容发生变化"),
        "{error}"
    );
    assert_eq!(harness.gateway.target.lock().unwrap().as_str(), TARGET);
}

#[tokio::test]
async fn merge_allows_comments_when_target_is_an_integration_branch() {
    let harness = harness().await;
    let view = harness
        .service
        .create(
            &harness.project,
            harness.admin.id,
            CreateDesignSpaceMergeRequest {
                title: "Group summary".to_string(),
                description: String::new(),
                source_branch: "feature".to_string(),
                target_branch: "integration".to_string(),
            },
        )
        .await
        .unwrap();
    harness
        .tasks
        .send_operation(
            harness.project.id,
            view.merge_request.task_id,
            harness.reviewer.id,
            OperationName::new("review_approved"),
            "approve-integration".to_string(),
        )
        .await
        .unwrap();
    harness.gateway.comments.store(3, Ordering::SeqCst);
    let merged = harness
        .service
        .merge(&harness.project, view.merge_request.id, harness.reviewer.id)
        .await
        .unwrap();
    assert_eq!(merged.status, MergeRequestStatus::Merged);
    assert_eq!(harness.gateway.target.lock().unwrap().as_str(), SOURCE);
}

#[tokio::test]
async fn merge_refuses_a_source_branch_that_moved_after_the_request_was_refreshed() {
    let harness = harness().await;
    let view = harness
        .service
        .create(
            &harness.project,
            harness.admin.id,
            CreateDesignSpaceMergeRequest {
                title: "Stale source".to_string(),
                description: String::new(),
                source_branch: "feature".to_string(),
                target_branch: "main".to_string(),
            },
        )
        .await
        .unwrap();
    harness
        .service
        .approve(&harness.project, view.merge_request.id, harness.reviewer.id)
        .await
        .unwrap();
    *harness.gateway.source.lock().unwrap() = OTHER.to_string();
    let error = harness
        .service
        .merge(&harness.project, view.merge_request.id, harness.reviewer.id)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("刷新"), "{error}");
    assert_eq!(harness.gateway.target.lock().unwrap().as_str(), TARGET);
}

#[tokio::test]
async fn protected_design_branches_cannot_be_deleted() {
    let harness = harness().await;
    for branch in ["source", "main", "master"] {
        let error = harness
            .service
            .delete_branch(&harness.project, branch)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("受保护分支"), "{error}");
    }
    harness
        .service
        .delete_branch(&harness.project, "feature")
        .await
        .unwrap();
}

#[tokio::test]
async fn rejection_is_a_task_operation() {
    let harness = harness().await;
    let view = harness
        .service
        .create(
            &harness.project,
            harness.admin.id,
            CreateDesignSpaceMergeRequest {
                title: "Reject feature".to_string(),
                description: String::new(),
                source_branch: "feature".to_string(),
                target_branch: "main".to_string(),
            },
        )
        .await
        .unwrap();
    harness
        .tasks
        .send_operation(
            harness.project.id,
            view.merge_request.task_id,
            harness.reviewer.id,
            OperationName::new("review_rejected"),
            "reject-1".to_string(),
        )
        .await
        .unwrap();
    let detail = harness
        .service
        .get(
            &harness.project,
            view.merge_request.id,
            TaskViewer::member(harness.reviewer.id),
        )
        .await
        .unwrap();
    assert_eq!(detail.status, MergeRequestStatus::Rejected);
    assert_eq!(detail.task.result.as_ref().unwrap().as_str(), "rejected");
    assert!(
        harness
            .service
            .merge(&harness.project, view.merge_request.id, harness.reviewer.id)
            .await
            .is_err(),
        "a rejected request cannot be merged"
    );
    assert_eq!(
        harness.gateway.target.lock().unwrap().as_str(),
        TARGET,
        "a rejected request must not move the target branch"
    );
}
