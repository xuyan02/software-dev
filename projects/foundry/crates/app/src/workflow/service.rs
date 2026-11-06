//! Workflow definition use cases.
//!
//! Definitions are catalog data, not project data. Reads are scoped by the
//! viewer: platform definitions are public, account definitions are visible to
//! their owner and administrators. Writes require a human account; an
//! administrator may maintain platform definitions but never another
//! account's private definition.

use async_trait::async_trait;
use foundry_domain::{
    Account, AccountKind, AccountStatus, Actor, DomainEvent, DomainEventKind, SCHEMA_VERSION,
    Validation, ValidationIssue, WorkflowBody, WorkflowDefinition, WorkflowDefinitionDAO,
    WorkflowDraftDAO, WorkflowDraftView, WorkflowId, WorkflowLayout, WorkflowLayoutDAO,
    WorkflowOwner, WorkflowVersion, WorkflowVersionInfo,
};
use serde::Serialize;
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    AccountRepository, CreateWorkflow, DraftSaveOutcome, EventSink, PublishOutcome,
    PublishWorkflow, RenameWorkflow, SaveWorkflowDraft, SaveWorkflowLayout, ServiceError,
    WorkflowDefinitionRow, WorkflowRepository, WorkflowScope, WorkflowVersionRow,
};

/// The response of a successful draft save.
#[derive(Debug, Clone, Serialize)]
pub struct DraftSaved {
    pub revision: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// The response of a successful publish.
#[derive(Debug, Clone, Serialize)]
pub struct WorkflowPublished {
    pub version: u32,
    pub revision: u64,
    pub warnings: Vec<ValidationIssue>,
}

pub struct WorkflowService<R, A, E> {
    repository: Arc<R>,
    accounts: Arc<A>,
    events: Arc<E>,
}

impl<R, A, E> WorkflowService<R, A, E>
where
    R: WorkflowRepository,
    A: AccountRepository,
    E: EventSink,
{
    pub fn new(repository: Arc<R>, accounts: Arc<A>, events: Arc<E>) -> Self {
        Self {
            repository,
            accounts,
            events,
        }
    }

    pub async fn list(&self, actor: Actor) -> Result<Vec<WorkflowDefinition>, ServiceError> {
        let account = self.require_account(&actor).await?;
        let rows = self
            .repository
            .list_definitions(account.id, account.is_admin())
            .await?;
        let mut definitions = Vec::with_capacity(rows.len());
        for row in rows {
            definitions.push(self.view(&account, row).await?);
        }
        Ok(definitions)
    }

    pub async fn get(
        &self,
        actor: Actor,
        id: WorkflowId,
    ) -> Result<WorkflowDefinition, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.visible_definition(&account, &id).await?;
        let row = self
            .repository
            .definition_row(&id)
            .await?
            .ok_or_else(|| not_found(&id))?;
        self.view(&account, row).await
    }

    pub async fn create(
        &self,
        actor: Actor,
        command: CreateWorkflow,
    ) -> Result<(WorkflowDefinition, WorkflowDraftView), ServiceError> {
        let account = self.require_account(&actor).await?;
        self.require_human(&account)?;
        if command.scope == WorkflowScope::Platform && !account.is_admin() {
            return Err(ServiceError::Forbidden(
                "only platform administrators may create platform workflows".to_string(),
            ));
        }
        let title = validate_title(&command.title)?;

        let (body, copied_layout) = match &command.copy_from {
            None => (empty_body(), None),
            Some(reference) => {
                let source = self
                    .repository
                    .definition(&reference.workflow_id)
                    .await?
                    .ok_or_else(|| {
                        ServiceError::not_found_named("workflow", reference.workflow_id.as_str())
                    })?;
                if source.owner_account_id.is_some() {
                    return Err(ServiceError::Validation(
                        "copy_from must reference a published platform workflow".to_string(),
                    ));
                }
                let version = self
                    .repository
                    .version(&reference.workflow_id, reference.version)
                    .await?
                    .ok_or_else(|| {
                        ServiceError::not_found_named(
                            "workflow version",
                            format!("{}@{}", reference.workflow_id.as_str(), reference.version),
                        )
                    })?;
                let layout = self
                    .repository
                    .layout(&reference.workflow_id)
                    .await?
                    .map(|layout| layout.layout);
                (version.body, layout)
            }
        };

        let now = OffsetDateTime::now_utc();
        let id = WorkflowId::new(format!("wf_{}", Uuid::now_v7().simple()));
        let owner_account_id = match command.scope {
            WorkflowScope::Account => Some(account.id),
            WorkflowScope::Platform => None,
        };
        let definition = WorkflowDefinitionDAO {
            id: id.clone(),
            owner_account_id,
            title: title.clone(),
            created_at: now,
            updated_at: now,
        };
        let draft = WorkflowDraftDAO {
            workflow_id: id.clone(),
            revision: 1,
            body: body.clone(),
            last_client_save_id: None,
            updated_by_account_id: account.id,
            updated_at: now,
        };
        let layout = copied_layout.map(|layout| WorkflowLayoutDAO {
            workflow_id: id.clone(),
            layout,
            updated_by_account_id: account.id,
            updated_at: now,
        });
        self.repository
            .insert_definition(&definition, &draft, layout.as_ref())
            .await?;
        self.emit(
            DomainEventKind::WorkflowDefinitionCreated,
            &account,
            json!({
                "workflow_id": id,
                "title": title,
                "owner_account_id": owner_account_id,
                "copied_from": command.copy_from,
            }),
        )
        .await?;

        let owner = self.owner(&account, owner_account_id).await?;
        Ok((
            WorkflowDefinition {
                id: id.clone(),
                title,
                owner,
                latest_version: None,
                draft_revision: Some(1),
                editable: true,
                created_at: now,
                updated_at: now,
            },
            draft_view(draft),
        ))
    }

    pub async fn rename(
        &self,
        actor: Actor,
        command: RenameWorkflow,
    ) -> Result<WorkflowDefinition, ServiceError> {
        let account = self.require_account(&actor).await?;
        let definition = self.visible_definition(&account, &command.id).await?;
        self.require_write(&account, &definition)?;
        let title = validate_title(&command.title)?;
        self.repository
            .rename(&command.id, &title, OffsetDateTime::now_utc())
            .await?;
        self.emit(
            DomainEventKind::WorkflowTitleChanged,
            &account,
            json!({ "workflow_id": command.id, "title": title }),
        )
        .await?;
        self.get(actor, command.id).await
    }

    pub async fn versions(
        &self,
        actor: Actor,
        id: WorkflowId,
    ) -> Result<Vec<WorkflowVersionInfo>, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.visible_definition(&account, &id).await?;
        let mut versions = Vec::new();
        for row in self.repository.versions(&id).await? {
            versions.push(self.version_info(row).await?);
        }
        Ok(versions)
    }

    pub async fn version(
        &self,
        actor: Actor,
        id: WorkflowId,
        version: u32,
    ) -> Result<WorkflowVersion, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.visible_definition(&account, &id).await?;
        let version = self
            .repository
            .version(&id, version)
            .await?
            .ok_or_else(|| {
                ServiceError::not_found_named(
                    "workflow version",
                    format!("{}@{version}", id.as_str()),
                )
            })?;
        let published_by_display_name = self.display_name(version.published_by_account_id).await?;
        Ok(WorkflowVersion {
            workflow_id: version.workflow_id,
            version: version.version,
            body: version.body,
            published_by_account_id: version.published_by_account_id,
            published_by_display_name,
            published_at: version.published_at,
        })
    }

    pub async fn open_draft(
        &self,
        actor: Actor,
        id: WorkflowId,
    ) -> Result<WorkflowDraftView, ServiceError> {
        let account = self.require_account(&actor).await?;
        let definition = self.visible_definition(&account, &id).await?;
        self.require_write(&account, &definition)?;
        if let Some(draft) = self.repository.draft(&id).await? {
            return Ok(draft_view(draft));
        }
        let fallback = match self.repository.latest_version(&id).await? {
            Some(version) => version.body,
            None => empty_body(),
        };
        let draft = self
            .repository
            .open_draft(&id, fallback, account.id, OffsetDateTime::now_utc())
            .await?;
        Ok(draft_view(draft))
    }

    pub async fn get_draft(
        &self,
        actor: Actor,
        id: WorkflowId,
    ) -> Result<WorkflowDraftView, ServiceError> {
        let account = self.require_account(&actor).await?;
        let definition = self.visible_definition(&account, &id).await?;
        self.require_read_draft(&account, &definition)?;
        let draft = self
            .repository
            .draft(&id)
            .await?
            .ok_or_else(|| ServiceError::not_found_named("workflow draft", id.as_str()))?;
        Ok(draft_view(draft))
    }

    pub async fn save_draft(
        &self,
        actor: Actor,
        command: SaveWorkflowDraft,
    ) -> Result<DraftSaved, ServiceError> {
        let account = self.require_account(&actor).await?;
        let definition = self.visible_definition(&account, &command.id).await?;
        self.require_write(&account, &definition)?;
        let bytes = serde_json::to_vec(&command.body)
            .map_err(|error| ServiceError::Internal(error.to_string()))?;
        if bytes.len() > foundry_domain::MAX_AST_BYTES {
            return Err(ServiceError::WorkflowTooLarge);
        }
        match self
            .repository
            .save_draft(
                &command.id,
                &command.body,
                command.expected_revision,
                command.client_save_id,
                account.id,
                OffsetDateTime::now_utc(),
            )
            .await?
        {
            DraftSaveOutcome::Saved {
                revision,
                updated_at,
            } => Ok(DraftSaved {
                revision,
                updated_at,
            }),
            DraftSaveOutcome::Conflict {
                current_revision,
                current_updated_at,
            } => Err(ServiceError::DraftConflict {
                expected: command.expected_revision,
                current: current_revision,
                current_updated_at,
            }),
        }
    }

    pub async fn validate(
        &self,
        actor: Actor,
        id: WorkflowId,
        body: WorkflowBody,
    ) -> Result<Validation, ServiceError> {
        let account = self.require_account(&actor).await?;
        let definition = self.visible_definition(&account, &id).await?;
        let known = self.known_ids(&definition, &id).await?;
        Ok(foundry_domain::validate(&body, &|candidate| {
            known.contains(candidate)
        }))
    }

    pub async fn publish(
        &self,
        actor: Actor,
        command: PublishWorkflow,
    ) -> Result<WorkflowPublished, ServiceError> {
        let account = self.require_account(&actor).await?;
        let definition = self.visible_definition(&account, &command.id).await?;
        self.require_write(&account, &definition)?;

        let draft = self
            .repository
            .draft(&command.id)
            .await?
            .ok_or_else(|| ServiceError::DraftNotFound(command.id.to_string()))?;
        if draft.revision != command.expected_revision {
            return Err(ServiceError::DraftConflict {
                expected: command.expected_revision,
                current: draft.revision,
                current_updated_at: draft.updated_at,
            });
        }
        let known = self.known_ids(&definition, &command.id).await?;
        let validation =
            foundry_domain::validate(&draft.body, &|candidate| known.contains(candidate));
        if !validation.is_valid() {
            return Err(ServiceError::WorkflowInvalid(validation));
        }

        match self
            .repository
            .publish(
                &command.id,
                command.expected_revision,
                account.id,
                OffsetDateTime::now_utc(),
            )
            .await?
        {
            PublishOutcome::Published { version, revision } => {
                self.emit(
                    DomainEventKind::WorkflowVersionPublished,
                    &account,
                    json!({
                        "workflow_id": command.id,
                        "version": version,
                        "revision": revision,
                    }),
                )
                .await?;
                Ok(WorkflowPublished {
                    version,
                    revision,
                    warnings: validation.warnings,
                })
            }
            PublishOutcome::Conflict {
                current_revision,
                current_updated_at,
            } => Err(ServiceError::DraftConflict {
                expected: command.expected_revision,
                current: current_revision,
                current_updated_at,
            }),
            PublishOutcome::NoDraft => Err(ServiceError::DraftNotFound(command.id.to_string())),
        }
    }

    pub async fn layout(
        &self,
        actor: Actor,
        id: WorkflowId,
    ) -> Result<WorkflowLayout, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.visible_definition(&account, &id).await?;
        Ok(self
            .repository
            .layout(&id)
            .await?
            .map(|layout| layout.layout)
            .unwrap_or_default())
    }

    pub async fn save_layout(
        &self,
        actor: Actor,
        command: SaveWorkflowLayout,
    ) -> Result<WorkflowLayout, ServiceError> {
        let account = self.require_account(&actor).await?;
        let definition = self.visible_definition(&account, &command.id).await?;
        self.require_write(&account, &definition)?;
        let layout = self
            .repository
            .save_layout(
                &command.id,
                &command.layout,
                account.id,
                OffsetDateTime::now_utc(),
            )
            .await?;
        Ok(layout.layout)
    }

    async fn visible_definition(
        &self,
        account: &Account,
        id: &WorkflowId,
    ) -> Result<WorkflowDefinitionDAO, ServiceError> {
        let definition = self
            .repository
            .definition(id)
            .await?
            .ok_or_else(|| not_found(id))?;
        let visible = match definition.owner_account_id {
            // Unpublished platform definitions are drafts for administrators.
            None => account.is_admin() || self.repository.latest_version(id).await?.is_some(),
            Some(owner) => owner == account.id || account.is_admin(),
        };
        if !visible {
            return Err(not_found(id));
        }
        Ok(definition)
    }

    async fn known_ids(
        &self,
        definition: &WorkflowDefinitionDAO,
        id: &WorkflowId,
    ) -> Result<HashSet<String>, ServiceError> {
        let mut known: HashSet<String> = self
            .repository
            .published_ids(definition.owner_account_id)
            .await?
            .into_iter()
            .map(|known| known.to_string())
            .collect();
        known.insert(id.to_string());
        Ok(known)
    }

    async fn view(
        &self,
        account: &Account,
        row: WorkflowDefinitionRow,
    ) -> Result<WorkflowDefinition, ServiceError> {
        let owner = self.owner(account, row.definition.owner_account_id).await?;
        let readable_draft = can_read_draft(account, &row.definition);
        let editable = can_write(account, &row.definition);
        Ok(WorkflowDefinition {
            id: row.definition.id.clone(),
            title: row.definition.title,
            owner,
            latest_version: row.latest_version,
            draft_revision: readable_draft.then_some(row.draft_revision).flatten(),
            editable,
            created_at: row.definition.created_at,
            updated_at: row.definition.updated_at,
        })
    }

    async fn owner(
        &self,
        account: &Account,
        owner_account_id: Option<Uuid>,
    ) -> Result<WorkflowOwner, ServiceError> {
        Ok(match owner_account_id {
            None => WorkflowOwner::Platform,
            Some(owner) if owner == account.id => WorkflowOwner::Account {
                account_id: owner,
                display_name: account.display_name.clone(),
            },
            Some(owner) => WorkflowOwner::Account {
                account_id: owner,
                display_name: self.display_name(Some(owner)).await?,
            },
        })
    }

    async fn display_name(&self, account_id: Option<Uuid>) -> Result<String, ServiceError> {
        match account_id {
            None => Ok("system".to_string()),
            Some(id) => Ok(self
                .accounts
                .get(id)
                .await?
                .map(|account| account.display_name)
                .unwrap_or_default()),
        }
    }

    async fn version_info(
        &self,
        row: WorkflowVersionRow,
    ) -> Result<WorkflowVersionInfo, ServiceError> {
        Ok(WorkflowVersionInfo {
            version: row.version,
            published_by_account_id: row.published_by_account_id,
            published_by_display_name: self.display_name(row.published_by_account_id).await?,
            published_at: row.published_at,
        })
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
        if !is_human(account) {
            return Err(ServiceError::Forbidden(
                "agent accounts cannot modify workflow definitions".to_string(),
            ));
        }
        Ok(())
    }

    fn require_write(
        &self,
        account: &Account,
        definition: &WorkflowDefinitionDAO,
    ) -> Result<(), ServiceError> {
        self.require_human(account)?;
        if !can_write(account, definition) {
            return Err(ServiceError::Forbidden(
                "workflow definition is not writable by this account".to_string(),
            ));
        }
        Ok(())
    }

    fn require_read_draft(
        &self,
        account: &Account,
        definition: &WorkflowDefinitionDAO,
    ) -> Result<(), ServiceError> {
        if !can_read_draft(account, definition) {
            return Err(ServiceError::Forbidden(
                "workflow draft is not readable by this account".to_string(),
            ));
        }
        Ok(())
    }

    async fn emit(
        &self,
        kind: DomainEventKind,
        account: &Account,
        payload: serde_json::Value,
    ) -> Result<(), ServiceError> {
        // Workflow ids are text, so the subject is carried in the payload.
        let event = DomainEvent::new(kind, Uuid::now_v7(), account.actor(), payload);
        self.events.publish(&[event]).await?;
        Ok(())
    }
}

#[async_trait]
pub trait WorkflowUseCases: Send + Sync {
    async fn list(&self, actor: Actor) -> Result<Vec<WorkflowDefinition>, ServiceError>;
    async fn get(&self, actor: Actor, id: WorkflowId) -> Result<WorkflowDefinition, ServiceError>;
    async fn create(
        &self,
        actor: Actor,
        command: CreateWorkflow,
    ) -> Result<(WorkflowDefinition, WorkflowDraftView), ServiceError>;
    async fn rename(
        &self,
        actor: Actor,
        command: RenameWorkflow,
    ) -> Result<WorkflowDefinition, ServiceError>;
    async fn versions(
        &self,
        actor: Actor,
        id: WorkflowId,
    ) -> Result<Vec<WorkflowVersionInfo>, ServiceError>;
    async fn version(
        &self,
        actor: Actor,
        id: WorkflowId,
        version: u32,
    ) -> Result<WorkflowVersion, ServiceError>;
    async fn open_draft(
        &self,
        actor: Actor,
        id: WorkflowId,
    ) -> Result<WorkflowDraftView, ServiceError>;
    async fn get_draft(
        &self,
        actor: Actor,
        id: WorkflowId,
    ) -> Result<WorkflowDraftView, ServiceError>;
    async fn save_draft(
        &self,
        actor: Actor,
        command: SaveWorkflowDraft,
    ) -> Result<DraftSaved, ServiceError>;
    async fn validate(
        &self,
        actor: Actor,
        id: WorkflowId,
        body: WorkflowBody,
    ) -> Result<Validation, ServiceError>;
    async fn publish(
        &self,
        actor: Actor,
        command: PublishWorkflow,
    ) -> Result<WorkflowPublished, ServiceError>;
    async fn layout(&self, actor: Actor, id: WorkflowId) -> Result<WorkflowLayout, ServiceError>;
    async fn save_layout(
        &self,
        actor: Actor,
        command: SaveWorkflowLayout,
    ) -> Result<WorkflowLayout, ServiceError>;
}

#[async_trait]
impl<R, A, E> WorkflowUseCases for WorkflowService<R, A, E>
where
    R: WorkflowRepository,
    A: AccountRepository,
    E: EventSink,
{
    async fn list(&self, actor: Actor) -> Result<Vec<WorkflowDefinition>, ServiceError> {
        WorkflowService::list(self, actor).await
    }

    async fn get(&self, actor: Actor, id: WorkflowId) -> Result<WorkflowDefinition, ServiceError> {
        WorkflowService::get(self, actor, id).await
    }

    async fn create(
        &self,
        actor: Actor,
        command: CreateWorkflow,
    ) -> Result<(WorkflowDefinition, WorkflowDraftView), ServiceError> {
        WorkflowService::create(self, actor, command).await
    }

    async fn rename(
        &self,
        actor: Actor,
        command: RenameWorkflow,
    ) -> Result<WorkflowDefinition, ServiceError> {
        WorkflowService::rename(self, actor, command).await
    }

    async fn versions(
        &self,
        actor: Actor,
        id: WorkflowId,
    ) -> Result<Vec<WorkflowVersionInfo>, ServiceError> {
        WorkflowService::versions(self, actor, id).await
    }

    async fn version(
        &self,
        actor: Actor,
        id: WorkflowId,
        version: u32,
    ) -> Result<WorkflowVersion, ServiceError> {
        WorkflowService::version(self, actor, id, version).await
    }

    async fn open_draft(
        &self,
        actor: Actor,
        id: WorkflowId,
    ) -> Result<WorkflowDraftView, ServiceError> {
        WorkflowService::open_draft(self, actor, id).await
    }

    async fn get_draft(
        &self,
        actor: Actor,
        id: WorkflowId,
    ) -> Result<WorkflowDraftView, ServiceError> {
        WorkflowService::get_draft(self, actor, id).await
    }

    async fn save_draft(
        &self,
        actor: Actor,
        command: SaveWorkflowDraft,
    ) -> Result<DraftSaved, ServiceError> {
        WorkflowService::save_draft(self, actor, command).await
    }

    async fn validate(
        &self,
        actor: Actor,
        id: WorkflowId,
        body: WorkflowBody,
    ) -> Result<Validation, ServiceError> {
        WorkflowService::validate(self, actor, id, body).await
    }

    async fn publish(
        &self,
        actor: Actor,
        command: PublishWorkflow,
    ) -> Result<WorkflowPublished, ServiceError> {
        WorkflowService::publish(self, actor, command).await
    }

    async fn layout(&self, actor: Actor, id: WorkflowId) -> Result<WorkflowLayout, ServiceError> {
        WorkflowService::layout(self, actor, id).await
    }

    async fn save_layout(
        &self,
        actor: Actor,
        command: SaveWorkflowLayout,
    ) -> Result<WorkflowLayout, ServiceError> {
        WorkflowService::save_layout(self, actor, command).await
    }
}

fn is_human(account: &Account) -> bool {
    account.kind == AccountKind::Human
}

fn can_write(account: &Account, definition: &WorkflowDefinitionDAO) -> bool {
    if !is_human(account) {
        return false;
    }
    match definition.owner_account_id {
        None => account.is_admin(),
        Some(owner) => owner == account.id,
    }
}

fn can_read_draft(account: &Account, definition: &WorkflowDefinitionDAO) -> bool {
    can_write(account, definition) || (is_human(account) && account.is_admin())
}

fn validate_title(title: &str) -> Result<String, ServiceError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(ServiceError::Validation(
            "workflow title is required".to_string(),
        ));
    }
    if title.chars().count() > 200 {
        return Err(ServiceError::Validation(
            "workflow title must be at most 200 characters".to_string(),
        ));
    }
    Ok(title.to_string())
}

fn empty_body() -> WorkflowBody {
    WorkflowBody {
        schema_version: SCHEMA_VERSION,
        roles: Vec::new(),
        vars: Default::default(),
        initial_stage: None,
        stages: Vec::new(),
    }
}

fn draft_view(draft: WorkflowDraftDAO) -> WorkflowDraftView {
    WorkflowDraftView {
        workflow_id: draft.workflow_id,
        revision: draft.revision,
        body: draft.body,
        updated_by_account_id: draft.updated_by_account_id,
        updated_at: draft.updated_at,
    }
}

fn not_found(id: &WorkflowId) -> ServiceError {
    ServiceError::not_found_named("workflow", id.as_str())
}
