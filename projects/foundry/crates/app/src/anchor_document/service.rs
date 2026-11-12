//! Use cases for schema-described anchor documents.

use foundry_domain::{
    Account, AccountStatus, Actor, AnchorAnnotation, AnchorAnnotationStatus, AnchorOperation,
    ApprovalDecision, DocumentSchema, DocumentSnapshot, ProjectRole, ProjectStatus, Validation,
};
use serde::Serialize;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    AccountRepository, AnchorApplyOutcome, AnchorApprovalOutcome, AnchorDocumentApprovalDAO,
    AnchorDocumentDraftDAO, AnchorDocumentRepository, AnchorDocumentVersionRow, DocumentReviewLink,
    ProjectRepository, RepoError, ServiceError,
};

pub fn schema_for(kind: &str) -> Option<DocumentSchema> {
    match kind {
        "project" => Some(foundry_domain::project_document_schema()),
        "requirements" => Some(foundry_domain::requirements_document_schema()),
        "architecture" => Some(foundry_domain::architecture_document_schema()),
        "design" => Some(foundry_domain::design_document_schema()),
        "development" => Some(foundry_domain::development_document_schema()),
        "test_plan" => Some(foundry_domain::test_plan_document_schema()),
        "test_report" => Some(foundry_domain::test_report_document_schema()),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorRef {
    Key(String),
    Path(String),
    Id(String),
}

impl AnchorRef {
    pub fn parse(value: &str) -> Self {
        if let Some(id) = value.strip_prefix("id:") {
            Self::Id(id.to_string())
        } else if let Some(key) = value.strip_prefix("key:") {
            Self::Key(key.to_string())
        } else if let Some(path) = value.strip_prefix("path:") {
            Self::Path(path.to_string())
        } else {
            Self::Key(value.to_string())
        }
    }
}

fn anchor_name(anchor: &AnchorRef) -> String {
    match anchor {
        AnchorRef::Key(value) | AnchorRef::Path(value) | AnchorRef::Id(value) => value.clone(),
    }
}

fn resolve<'a>(
    snapshot: &'a DocumentSnapshot,
    anchor: &AnchorRef,
) -> Option<&'a foundry_domain::AnchorNode> {
    match anchor {
        AnchorRef::Key(value) => snapshot.find(value).or_else(|| snapshot.find_path(value)),
        AnchorRef::Path(value) => snapshot.find_path(value),
        AnchorRef::Id(value) => snapshot.find_by_id(value),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AnchorDocumentView {
    pub project_id: Uuid,
    pub kind: String,
    pub title: String,
    pub document_id: Option<Uuid>,
    pub schema_version: u32,
    pub current_version: u32,
    pub latest_version: Option<u32>,
    pub draft_revision: Option<u64>,
    pub draft_version: Option<u32>,
    pub root_id: String,
    pub editable: bool,
    pub toc: String,
}

pub fn render_toc_markdown(view: &AnchorDocumentView) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", view.title));
    out.push_str("## 元数据\n\n");
    out.push_str(&format!("- kind: {}\n", view.kind));
    out.push_str(&format!("- project_id: {}\n", view.project_id));
    if let Some(document_id) = view.document_id {
        out.push_str(&format!("- document_id: {document_id}\n"));
    }
    out.push_str(&format!("- current_version: {}\n", view.current_version));
    out.push_str(&format!(
        "- latest_version: {}\n",
        view.latest_version
            .map(|version| version.to_string())
            .unwrap_or_else(|| "—".to_string())
    ));
    out.push_str(&format!(
        "- draft_revision: {}\n",
        view.draft_revision
            .map(|revision| revision.to_string())
            .unwrap_or_else(|| "—".to_string())
    ));
    out.push_str(&format!(
        "- draft_version: {}\n",
        view.draft_version
            .map(|version| version.to_string())
            .unwrap_or_else(|| "—".to_string())
    ));
    out.push_str(&format!("- root_id: {}\n", view.root_id));
    out.push_str(&format!("- editable: {}\n\n", view.editable));
    out.push_str("## 目录\n\n");
    out.push_str(&view.toc);
    out.push('\n');
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct AnchorContentView {
    pub anchor_id: String,
    pub key: String,
    pub title: String,
    pub format: String,
    pub markdown: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnchorDraftView {
    pub project_id: Uuid,
    pub kind: String,
    pub revision: u64,
    pub base_version: u32,
    pub schema_version: u32,
    pub root_id: String,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnchorApplyView {
    pub revision: u64,
    pub root_id: String,
    pub created_key: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnchorVersionView {
    pub project_id: Uuid,
    pub kind: String,
    pub version: u32,
    pub root_id: String,
    pub created_by: Uuid,
    pub approved_by: Option<Uuid>,
    pub source_revision: Option<u64>,
    pub published_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnchorApprovalResult {
    pub decision: ApprovalDecision,
    pub revision: u64,
    pub version: Option<u32>,
    pub warnings: Vec<foundry_domain::ValidationIssue>,
}

#[derive(Debug, Clone)]
pub struct ApplyAnchorOperations {
    pub project_id: Uuid,
    pub kind: String,
    pub expected_revision: u64,
    pub operations: Vec<AnchorOperation>,
}

#[derive(Debug, Clone)]
pub struct DecideAnchorDocument {
    pub project_id: Uuid,
    pub kind: String,
    pub expected_revision: u64,
    pub decision: ApprovalDecision,
    pub comment: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AddAnchorAnnotation {
    pub project_id: Uuid,
    pub kind: String,
    pub anchor: String,
    pub quote: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct ResolveAnchorAnnotation {
    pub project_id: Uuid,
    pub kind: String,
    pub annotation_id: Uuid,
    pub resolution: String,
}

pub struct AnchorDocumentService<R, A, P> {
    repository: Arc<R>,
    accounts: Arc<A>,
    projects: Arc<P>,
}

impl<R, A, P> AnchorDocumentService<R, A, P>
where
    R: AnchorDocumentRepository,
    A: AccountRepository,
    P: ProjectRepository,
{
    pub fn new(repository: Arc<R>, accounts: Arc<A>, projects: Arc<P>) -> Self {
        Self {
            repository,
            accounts,
            projects,
        }
    }

    fn schema(&self, kind: &str) -> Result<DocumentSchema, ServiceError> {
        schema_for(kind).ok_or_else(|| ServiceError::not_found_named("document kind", kind))
    }

    pub async fn get(
        &self,
        actor: Actor,
        project_id: Uuid,
        kind: &str,
        anchor: &AnchorRef,
    ) -> Result<AnchorDocumentView, ServiceError> {
        let schema = self.schema(kind)?;
        let account = self.require_account(&actor).await?;
        let role = self
            .project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        let document = self.repository.document(project_id, kind).await?;
        let draft = self.repository.draft(project_id, kind).await?;
        let latest = self.repository.latest_version(project_id, kind).await?;
        let versions = self.repository.versions(project_id, kind).await?;
        let draft_version = draft.as_ref().and_then(|draft| {
            versions
                .iter()
                .find(|version| version.source_revision == Some(draft.revision))
                .map(|version| version.version)
        });
        let root_id = if let Some(draft) = &draft {
            draft.snapshot.root_id().to_string()
        } else if let Some(latest) = &latest {
            latest.snapshot.root_id().to_string()
        } else {
            String::new()
        };
        let snapshot = if let Some(draft) = &draft {
            draft.snapshot.clone()
        } else if let Some(latest) = &latest {
            latest.snapshot.clone()
        } else {
            DocumentSnapshot::empty(&schema)
        };
        let node = resolve(&snapshot, anchor)
            .ok_or_else(|| ServiceError::not_found_named("anchor", anchor_name(anchor)))?;
        let toc = snapshot
            .toc(&node.key)
            .ok_or_else(|| ServiceError::not_found_named("anchor", anchor_name(anchor)))?;
        Ok(AnchorDocumentView {
            project_id,
            kind: kind.to_string(),
            title: node.title.clone(),
            document_id: document.as_ref().map(|document| document.id),
            schema_version: schema.schema_version,
            current_version: document
                .as_ref()
                .map(|document| document.current_version)
                .unwrap_or(0),
            latest_version: latest.as_ref().map(|version| version.version),
            draft_revision: draft.as_ref().map(|draft| draft.revision),
            draft_version,
            root_id,
            editable: role >= ProjectRole::Member,
            toc,
        })
    }

    pub async fn open_draft(
        &self,
        actor: Actor,
        project_id: Uuid,
        kind: &str,
    ) -> Result<AnchorDraftView, ServiceError> {
        self.schema(kind)?;
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Member)
            .await?;
        let draft = self
            .repository
            .draft(project_id, kind)
            .await?
            .ok_or_else(|| ServiceError::DraftNotFound(project_id.to_string()))?;
        Ok(draft_view(draft))
    }

    pub async fn create_draft(
        &self,
        actor: Actor,
        project_id: Uuid,
        kind: &str,
    ) -> Result<AnchorDraftView, ServiceError> {
        let schema = self.schema(kind)?;
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Member)
            .await?;
        if self.repository.draft(project_id, kind).await?.is_some() {
            return Err(ServiceError::DraftExists(project_id.to_string()));
        }
        let (_, draft) = self
            .repository
            .ensure_document(
                project_id,
                kind,
                &schema,
                account.id,
                OffsetDateTime::now_utc(),
            )
            .await?;
        Ok(draft_view(draft))
    }

    pub async fn content(
        &self,
        actor: Actor,
        project_id: Uuid,
        kind: &str,
        anchor: &AnchorRef,
    ) -> Result<AnchorContentView, ServiceError> {
        self.content_inner(actor, project_id, kind, anchor, false)
            .await
    }

    /// Content for the web console: same Markdown, plus invisible anchor
    /// markers so annotations can be rendered under the content they target.
    pub async fn content_with_anchors(
        &self,
        actor: Actor,
        project_id: Uuid,
        kind: &str,
        anchor: &AnchorRef,
    ) -> Result<AnchorContentView, ServiceError> {
        self.content_inner(actor, project_id, kind, anchor, true)
            .await
    }

    async fn content_inner(
        &self,
        actor: Actor,
        project_id: Uuid,
        kind: &str,
        anchor: &AnchorRef,
        with_anchors: bool,
    ) -> Result<AnchorContentView, ServiceError> {
        let schema = self.schema(kind)?;
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        let snapshot = self.snapshot(project_id, kind, &schema).await?;
        let node = resolve(&snapshot, anchor)
            .ok_or_else(|| ServiceError::not_found_named("anchor", anchor_name(anchor)))?;
        let markdown = if with_anchors {
            snapshot.markdown_with_anchors(&node.key)
        } else {
            snapshot.markdown(&node.key)
        }
        .unwrap_or_default();
        Ok(AnchorContentView {
            anchor_id: node.id.clone(),
            key: node.key.clone(),
            title: node.title.clone(),
            format: "md".to_string(),
            markdown,
        })
    }

    pub async fn apply(
        &self,
        actor: Actor,
        command: ApplyAnchorOperations,
    ) -> Result<AnchorApplyView, ServiceError> {
        let schema = self.schema(&command.kind)?;
        let account = self.require_account(&actor).await?;
        self.project_role(&account, command.project_id, ProjectRole::Member)
            .await?;
        let (_, _) = self
            .repository
            .ensure_document(
                command.project_id,
                &command.kind,
                &schema,
                account.id,
                OffsetDateTime::now_utc(),
            )
            .await?;
        let now = OffsetDateTime::now_utc();
        match self
            .repository
            .apply_operations(
                command.project_id,
                &command.kind,
                command.expected_revision,
                &command.operations,
                account.id,
                now,
                &schema,
            )
            .await
            .map_err(anchor_repo_error)?
        {
            AnchorApplyOutcome::Applied {
                revision,
                root_id,
                created_key,
                updated_at,
            } => Ok(AnchorApplyView {
                revision,
                root_id,
                created_key,
                updated_at,
            }),
            AnchorApplyOutcome::Conflict {
                current_revision,
                current_updated_at,
            } => Err(ServiceError::DraftConflict {
                expected: command.expected_revision,
                current: current_revision,
                current_updated_at,
            }),
            AnchorApplyOutcome::NoDraft => {
                Err(ServiceError::DraftNotFound(command.project_id.to_string()))
            }
            AnchorApplyOutcome::TooLarge => Err(ServiceError::AnchorDocumentTooLarge),
            AnchorApplyOutcome::Invalid(message) => Err(ServiceError::Validation(message)),
        }
    }

    pub async fn validate(
        &self,
        actor: Actor,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Validation, ServiceError> {
        let schema = self.schema(kind)?;
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        let snapshot = self.snapshot(project_id, kind, &schema).await?;
        Ok(snapshot.validate(&schema))
    }

    pub async fn decide(
        &self,
        actor: Actor,
        command: DecideAnchorDocument,
    ) -> Result<AnchorApprovalResult, ServiceError> {
        let schema = self.schema(&command.kind)?;
        let account = self.require_account(&actor).await?;
        self.require_human(&account)?;
        self.project_role(&account, command.project_id, ProjectRole::Maintainer)
            .await?;
        let draft = self
            .repository
            .draft(command.project_id, &command.kind)
            .await?
            .ok_or_else(|| ServiceError::DraftNotFound(command.project_id.to_string()))?;
        if draft.revision != command.expected_revision {
            return Err(ServiceError::DraftConflict {
                expected: command.expected_revision,
                current: draft.revision,
                current_updated_at: draft.updated_at,
            });
        }
        let comment = command
            .comment
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let now = OffsetDateTime::now_utc();
        let mut warnings = Vec::new();
        match command.decision {
            ApprovalDecision::Approved => {
                let validation = draft.snapshot.validate(&schema);
                if !validation.is_valid() {
                    return Err(ServiceError::AnchorDocumentInvalid(validation));
                }
                warnings = validation.warnings;
                match self
                    .repository
                    .approve(
                        command.project_id,
                        &command.kind,
                        command.expected_revision,
                        account.id,
                        comment,
                        now,
                        &schema,
                    )
                    .await
                    .map_err(anchor_repo_error)?
                {
                    AnchorApprovalOutcome::Approved {
                        version, revision, ..
                    } => Ok(AnchorApprovalResult {
                        decision: ApprovalDecision::Approved,
                        revision,
                        version: Some(version),
                        warnings,
                    }),
                    AnchorApprovalOutcome::AlreadyPublished { version } => {
                        Ok(AnchorApprovalResult {
                            decision: ApprovalDecision::Approved,
                            revision: command.expected_revision,
                            version: Some(version),
                            warnings,
                        })
                    }
                    AnchorApprovalOutcome::Conflict {
                        current_revision,
                        current_updated_at,
                    } => Err(ServiceError::DraftConflict {
                        expected: command.expected_revision,
                        current: current_revision,
                        current_updated_at,
                    }),
                    AnchorApprovalOutcome::NoDraft => {
                        Err(ServiceError::DraftNotFound(command.project_id.to_string()))
                    }
                    AnchorApprovalOutcome::TooLarge => Err(ServiceError::AnchorDocumentTooLarge),
                    AnchorApprovalOutcome::Invalid(validation) => {
                        Err(ServiceError::AnchorDocumentInvalid(validation))
                    }
                    AnchorApprovalOutcome::Rejected { .. } => Err(ServiceError::Internal(
                        "unexpected rejection outcome".to_string(),
                    )),
                }
            }
            ApprovalDecision::Rejected => {
                let comment = comment.unwrap_or_default();
                match self
                    .repository
                    .reject(
                        command.project_id,
                        &command.kind,
                        command.expected_revision,
                        account.id,
                        comment,
                        now,
                    )
                    .await
                    .map_err(anchor_repo_error)?
                {
                    AnchorApprovalOutcome::Rejected { .. } => Ok(AnchorApprovalResult {
                        decision: ApprovalDecision::Rejected,
                        revision: command.expected_revision,
                        version: None,
                        warnings,
                    }),
                    AnchorApprovalOutcome::AlreadyPublished { version } => {
                        Ok(AnchorApprovalResult {
                            decision: ApprovalDecision::Approved,
                            revision: command.expected_revision,
                            version: Some(version),
                            warnings,
                        })
                    }
                    AnchorApprovalOutcome::Conflict {
                        current_revision,
                        current_updated_at,
                    } => Err(ServiceError::DraftConflict {
                        expected: command.expected_revision,
                        current: current_revision,
                        current_updated_at,
                    }),
                    AnchorApprovalOutcome::NoDraft => {
                        Err(ServiceError::DraftNotFound(command.project_id.to_string()))
                    }
                    AnchorApprovalOutcome::TooLarge => Err(ServiceError::AnchorDocumentTooLarge),
                    AnchorApprovalOutcome::Invalid(validation) => {
                        Err(ServiceError::AnchorDocumentInvalid(validation))
                    }
                    AnchorApprovalOutcome::Approved { .. } => Err(ServiceError::Internal(
                        "unexpected approval outcome".to_string(),
                    )),
                }
            }
        }
    }

    pub async fn review_link(
        &self,
        actor: Actor,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Option<DocumentReviewLink>, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        self.repository
            .active_review_link(project_id, kind)
            .await
            .map_err(anchor_repo_error)
    }

    /// System-level bind used by the control plane when a task enters human
    /// review. No human actor is involved.
    pub async fn bind_review_link(
        &self,
        project_id: Uuid,
        kind: &str,
        task_id: Uuid,
    ) -> Result<DocumentReviewLink, ServiceError> {
        self.repository
            .bind_review_link(project_id, kind, task_id, OffsetDateTime::now_utc())
            .await
            .map_err(anchor_repo_error)
    }

    pub async fn release_review_link(
        &self,
        actor: Actor,
        project_id: Uuid,
        kind: &str,
        task_id: Uuid,
    ) -> Result<(), ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Member)
            .await?;
        self.repository
            .release_review_link(project_id, kind, task_id, OffsetDateTime::now_utc())
            .await
            .map_err(anchor_repo_error)
    }

    pub async fn annotations(
        &self,
        actor: Actor,
        project_id: Uuid,
        kind: &str,
        include_resolved: bool,
    ) -> Result<Vec<AnchorAnnotation>, ServiceError> {
        let schema = self.schema(kind)?;
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        let snapshot = self.snapshot(project_id, kind, &schema).await?;
        let mut annotations = self.repository.annotations(project_id, kind).await?;
        for annotation in &mut annotations {
            annotation.status = annotation.effective_status(&snapshot);
        }
        if !include_resolved {
            annotations.retain(|annotation| {
                matches!(
                    annotation.status,
                    AnchorAnnotationStatus::Open
                        | AnchorAnnotationStatus::Stale
                        | AnchorAnnotationStatus::Orphaned
                )
            });
        }
        Ok(annotations)
    }

    pub async fn annotate(
        &self,
        actor: Actor,
        command: AddAnchorAnnotation,
    ) -> Result<AnchorAnnotation, ServiceError> {
        let schema = self.schema(&command.kind)?;
        let account = self.require_account(&actor).await?;
        self.project_role(&account, command.project_id, ProjectRole::Member)
            .await?;
        let draft = self
            .repository
            .draft(command.project_id, &command.kind)
            .await?
            .ok_or_else(|| ServiceError::DraftNotFound(command.project_id.to_string()))?;
        let node = draft
            .snapshot
            .find(&command.anchor)
            .or_else(|| draft.snapshot.find_path(&command.anchor))
            .ok_or_else(|| ServiceError::not_found_named("anchor", command.anchor.clone()))?;
        let now = OffsetDateTime::now_utc();
        let annotation = AnchorAnnotation {
            id: Uuid::now_v7(),
            project_id: command.project_id,
            kind: command.kind.clone(),
            anchor_key: node.key.clone(),
            anchor_id: node.id.clone(),
            quote: command.quote.and_then(|value| {
                let trimmed = value.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            }),
            body: command.body,
            author_id: account.id,
            author_handle: account.handle.clone(),
            author_name: account.display_name.clone(),
            status: AnchorAnnotationStatus::Open,
            resolution: None,
            created_at: now,
            resolved_at: None,
            resolved_by: None,
        };
        let _ = schema;
        let mut saved = self.repository.insert_annotation(annotation).await?;
        saved.status = saved.effective_status(&draft.snapshot);
        Ok(saved)
    }

    pub async fn resolve_annotation(
        &self,
        actor: Actor,
        command: ResolveAnchorAnnotation,
    ) -> Result<AnchorAnnotation, ServiceError> {
        let schema = self.schema(&command.kind)?;
        let account = self.require_account(&actor).await?;
        self.project_role(&account, command.project_id, ProjectRole::Member)
            .await?;
        let now = OffsetDateTime::now_utc();
        let mut annotation = self
            .repository
            .resolve_annotation(
                command.project_id,
                &command.kind,
                command.annotation_id,
                &command.resolution,
                account.id,
                now,
            )
            .await?
            .ok_or_else(|| {
                ServiceError::not_found_named("annotation", command.annotation_id.to_string())
            })?;
        let snapshot = self
            .snapshot(command.project_id, &command.kind, &schema)
            .await?;
        annotation.status = annotation.effective_status(&snapshot);
        Ok(annotation)
    }

    pub async fn approvals(
        &self,
        actor: Actor,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Vec<AnchorDocumentApprovalDAO>, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        Ok(self.repository.approvals(project_id, kind).await?)
    }

    pub async fn versions(
        &self,
        actor: Actor,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Vec<AnchorDocumentVersionRow>, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        Ok(self.repository.versions(project_id, kind).await?)
    }

    pub async fn version(
        &self,
        actor: Actor,
        project_id: Uuid,
        kind: &str,
        version: u32,
    ) -> Result<AnchorVersionView, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        let version = self
            .repository
            .version(project_id, kind, version)
            .await?
            .ok_or_else(|| {
                ServiceError::not_found_named(
                    "anchor document version",
                    format!("{kind}@{version}"),
                )
            })?;
        Ok(AnchorVersionView {
            project_id,
            kind: kind.to_string(),
            version: version.version,
            root_id: version.snapshot.root_id().to_string(),
            created_by: version.created_by,
            approved_by: version.approved_by,
            source_revision: if version.source_revision == 0 {
                None
            } else {
                Some(version.source_revision)
            },
            published_at: version.published_at,
        })
    }

    async fn snapshot(
        &self,
        project_id: Uuid,
        kind: &str,
        schema: &DocumentSchema,
    ) -> Result<DocumentSnapshot, ServiceError> {
        let snapshot = if let Some(draft) = self.repository.draft(project_id, kind).await? {
            draft.snapshot
        } else if let Some(version) = self.repository.latest_version(project_id, kind).await? {
            version.snapshot
        } else {
            DocumentSnapshot::empty(schema)
        };
        Ok(snapshot)
    }

    async fn require_account(&self, actor: &Actor) -> Result<Account, ServiceError> {
        let account = self
            .accounts
            .get_by_handle(actor.id())
            .await?
            .ok_or_else(|| ServiceError::Unauthorized("unknown account".to_string()))?;
        if account.status != AccountStatus::Active {
            return Err(ServiceError::Unauthorized(
                "account is suspended".to_string(),
            ));
        }
        Ok(account)
    }

    fn require_human(&self, account: &Account) -> Result<(), ServiceError> {
        if account.kind != foundry_domain::AccountKind::Human {
            return Err(ServiceError::Forbidden(
                "agent accounts cannot approve documents".to_string(),
            ));
        }
        Ok(())
    }

    async fn project_role(
        &self,
        account: &Account,
        project_id: Uuid,
        minimum: ProjectRole,
    ) -> Result<ProjectRole, ServiceError> {
        let project = self
            .projects
            .get(project_id)
            .await?
            .ok_or_else(|| ServiceError::not_found("project", project_id))?;
        if project.status == ProjectStatus::Archived {
            return Err(ServiceError::Forbidden("project is archived".to_string()));
        }
        let role = if account.is_admin() {
            ProjectRole::Owner
        } else {
            project.role_of(account.id).ok_or_else(|| {
                ServiceError::Forbidden("you are not a member of this project".to_string())
            })?
        };
        if role < minimum {
            return Err(ServiceError::Forbidden(
                "project role does not allow this action".to_string(),
            ));
        }
        Ok(role)
    }
}

pub trait AnchorDocumentUseCases: Send + Sync {
    fn get<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
        anchor: &'a AnchorRef,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorDocumentView, ServiceError>> + Send + 'a>,
    >;
    fn open_draft<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorDraftView, ServiceError>> + Send + 'a>,
    >;
    fn create_draft<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorDraftView, ServiceError>> + Send + 'a>,
    >;
    fn content<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
        anchor: &'a AnchorRef,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorContentView, ServiceError>> + Send + 'a>,
    >;
    fn content_with_anchors<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
        anchor: &'a AnchorRef,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorContentView, ServiceError>> + Send + 'a>,
    >;
    fn apply<'a>(
        &'a self,
        actor: Actor,
        command: ApplyAnchorOperations,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorApplyView, ServiceError>> + Send + 'a>,
    >;
    fn validate<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Validation, ServiceError>> + Send + 'a>,
    >;
    fn decide<'a>(
        &'a self,
        actor: Actor,
        command: DecideAnchorDocument,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<AnchorApprovalResult, ServiceError>>
                + Send
                + 'a,
        >,
    >;
    fn review_link<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Option<DocumentReviewLink>, ServiceError>>
                + Send
                + 'a,
        >,
    >;
    fn bind_review_link<'a>(
        &'a self,
        project_id: Uuid,
        kind: &'a str,
        task_id: Uuid,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<DocumentReviewLink, ServiceError>> + Send + 'a>,
    >;
    fn release_review_link<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
        task_id: Uuid,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ServiceError>> + Send + 'a>>;
    fn annotations<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
        include_resolved: bool,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<AnchorAnnotation>, ServiceError>>
                + Send
                + 'a,
        >,
    >;
    fn annotate<'a>(
        &'a self,
        actor: Actor,
        command: AddAnchorAnnotation,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorAnnotation, ServiceError>> + Send + 'a>,
    >;
    fn resolve_annotation<'a>(
        &'a self,
        actor: Actor,
        command: ResolveAnchorAnnotation,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorAnnotation, ServiceError>> + Send + 'a>,
    >;
    fn approvals<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<AnchorDocumentApprovalDAO>, ServiceError>>
                + Send
                + 'a,
        >,
    >;
    fn versions<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<AnchorDocumentVersionRow>, ServiceError>>
                + Send
                + 'a,
        >,
    >;
    fn version<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
        version: u32,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorVersionView, ServiceError>> + Send + 'a>,
    >;
}

impl<R, A, P> AnchorDocumentUseCases for AnchorDocumentService<R, A, P>
where
    R: AnchorDocumentRepository + 'static,
    A: AccountRepository + 'static,
    P: ProjectRepository + 'static,
{
    fn get<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
        anchor: &'a AnchorRef,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorDocumentView, ServiceError>> + Send + 'a>,
    > {
        Box::pin(AnchorDocumentService::get(
            self, actor, project_id, kind, anchor,
        ))
    }

    fn open_draft<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorDraftView, ServiceError>> + Send + 'a>,
    > {
        Box::pin(AnchorDocumentService::open_draft(
            self, actor, project_id, kind,
        ))
    }

    fn create_draft<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorDraftView, ServiceError>> + Send + 'a>,
    > {
        Box::pin(AnchorDocumentService::create_draft(
            self, actor, project_id, kind,
        ))
    }

    fn content<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
        anchor: &'a AnchorRef,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorContentView, ServiceError>> + Send + 'a>,
    > {
        Box::pin(AnchorDocumentService::content(
            self, actor, project_id, kind, anchor,
        ))
    }

    fn content_with_anchors<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
        anchor: &'a AnchorRef,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorContentView, ServiceError>> + Send + 'a>,
    > {
        Box::pin(AnchorDocumentService::content_with_anchors(
            self, actor, project_id, kind, anchor,
        ))
    }

    fn apply<'a>(
        &'a self,
        actor: Actor,
        command: ApplyAnchorOperations,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorApplyView, ServiceError>> + Send + 'a>,
    > {
        Box::pin(AnchorDocumentService::apply(self, actor, command))
    }

    fn validate<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Validation, ServiceError>> + Send + 'a>,
    > {
        Box::pin(AnchorDocumentService::validate(
            self, actor, project_id, kind,
        ))
    }

    fn decide<'a>(
        &'a self,
        actor: Actor,
        command: DecideAnchorDocument,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<AnchorApprovalResult, ServiceError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(AnchorDocumentService::decide(self, actor, command))
    }

    fn review_link<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Option<DocumentReviewLink>, ServiceError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(AnchorDocumentService::review_link(
            self, actor, project_id, kind,
        ))
    }

    fn bind_review_link<'a>(
        &'a self,
        project_id: Uuid,
        kind: &'a str,
        task_id: Uuid,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<DocumentReviewLink, ServiceError>> + Send + 'a>,
    > {
        Box::pin(AnchorDocumentService::bind_review_link(
            self, project_id, kind, task_id,
        ))
    }

    fn release_review_link<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
        task_id: Uuid,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ServiceError>> + Send + 'a>>
    {
        Box::pin(AnchorDocumentService::release_review_link(
            self, actor, project_id, kind, task_id,
        ))
    }

    fn annotations<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
        include_resolved: bool,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<AnchorAnnotation>, ServiceError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(AnchorDocumentService::annotations(
            self,
            actor,
            project_id,
            kind,
            include_resolved,
        ))
    }

    fn annotate<'a>(
        &'a self,
        actor: Actor,
        command: AddAnchorAnnotation,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorAnnotation, ServiceError>> + Send + 'a>,
    > {
        Box::pin(AnchorDocumentService::annotate(self, actor, command))
    }

    fn resolve_annotation<'a>(
        &'a self,
        actor: Actor,
        command: ResolveAnchorAnnotation,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorAnnotation, ServiceError>> + Send + 'a>,
    > {
        Box::pin(AnchorDocumentService::resolve_annotation(
            self, actor, command,
        ))
    }

    fn approvals<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<AnchorDocumentApprovalDAO>, ServiceError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(AnchorDocumentService::approvals(
            self, actor, project_id, kind,
        ))
    }

    fn versions<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<AnchorDocumentVersionRow>, ServiceError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(AnchorDocumentService::versions(
            self, actor, project_id, kind,
        ))
    }

    fn version<'a>(
        &'a self,
        actor: Actor,
        project_id: Uuid,
        kind: &'a str,
        version: u32,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<AnchorVersionView, ServiceError>> + Send + 'a>,
    > {
        Box::pin(AnchorDocumentService::version(
            self, actor, project_id, kind, version,
        ))
    }
}

fn draft_view(draft: AnchorDocumentDraftDAO) -> AnchorDraftView {
    AnchorDraftView {
        project_id: draft.project_id,
        kind: draft.kind,
        revision: draft.revision,
        base_version: draft.base_version,
        schema_version: draft.schema_version,
        root_id: draft.snapshot.root_id().to_string(),
        updated_at: draft.updated_at,
    }
}

fn anchor_repo_error(error: RepoError) -> ServiceError {
    match error {
        RepoError::Invalid(message) => ServiceError::Validation(message),
        other => ServiceError::Repository(other),
    }
}
