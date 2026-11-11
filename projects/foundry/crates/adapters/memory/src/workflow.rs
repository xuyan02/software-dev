//! In-memory workflow definitions, versions, drafts and layouts.
//!
//! The draft and publish paths mirror the Postgres adapter's optimistic
//! concurrency contract: revision CAS, `client_save_id` idempotency and a
//! revision bump on publish.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use foundry_app::{
    DraftSaveOutcome, PublishOutcome, RepoError, WorkflowCatalog, WorkflowDefinitionRow,
    WorkflowLookup, WorkflowRepository, WorkflowVersionRow,
};
use foundry_domain::{
    DefinitionWorkflow, TaskWorkflow, WorkflowBody, WorkflowDefinitionDAO, WorkflowDraftDAO,
    WorkflowId, WorkflowLayout, WorkflowLayoutDAO, WorkflowOption, WorkflowOwner,
    WorkflowVersionDAO, WorkflowVersionRef, builtin_title, builtin_workflows,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::InMemoryStore;

#[derive(Default)]
pub(crate) struct MemoryWorkflows {
    definitions: HashMap<WorkflowId, WorkflowDefinitionDAO>,
    versions: HashMap<WorkflowId, BTreeMap<u32, WorkflowVersionDAO>>,
    drafts: HashMap<WorkflowId, WorkflowDraftDAO>,
    layouts: HashMap<WorkflowId, WorkflowLayoutDAO>,
}

/// Latest published version visible in `account_id`'s scope.
///
/// Platform definitions are visible to everyone; account definitions only to
/// their owner. `None` is the platform scope used to resolve children of
/// platform definitions.
fn latest_in(
    workflows: &MemoryWorkflows,
    account_id: Option<Uuid>,
    id: &WorkflowId,
) -> WorkflowLookup {
    let Some(definition) = workflows.definitions.get(id) else {
        return WorkflowLookup::NotFound;
    };
    let visible = match definition.owner_account_id {
        None => true,
        Some(owner) => Some(owner) == account_id,
    };
    if !visible {
        return WorkflowLookup::NotFound;
    }
    match workflows
        .versions
        .get(id)
        .and_then(|versions| versions.keys().next_back())
    {
        Some(version) => WorkflowLookup::Published(WorkflowVersionRef {
            workflow_id: id.clone(),
            version: *version,
        }),
        None => WorkflowLookup::Unpublished(id.clone()),
    }
}

impl InMemoryStore {
    /// Register the nine platform workflows at version 1.
    ///
    /// Existing platform definitions are kept; unlike the Postgres seeder this
    /// does not verify that an existing v1 still matches the embedded body,
    /// because nothing can have edited it in a fresh test store.
    pub fn seed_builtin_workflows(&self) -> Result<(), RepoError> {
        let now = OffsetDateTime::now_utc();
        let mut workflows = self.workflows.write().expect("lock poisoned");
        for workflow in builtin_workflows() {
            let id = WorkflowId::new(workflow.id());
            if let Some(existing) = workflows.definitions.get(&id) {
                if existing.owner_account_id.is_some() {
                    return Err(RepoError::Backend(format!(
                        "builtin workflow id {id} is already owned by an account"
                    )));
                }
                continue;
            }
            workflows.definitions.insert(
                id.clone(),
                WorkflowDefinitionDAO {
                    id: id.clone(),
                    owner_account_id: None,
                    title: builtin_title(workflow.id())
                        .unwrap_or(workflow.id())
                        .to_string(),
                    created_at: now,
                    updated_at: now,
                },
            );
            workflows.versions.entry(id.clone()).or_default().insert(
                1,
                WorkflowVersionDAO {
                    workflow_id: id,
                    version: 1,
                    body: workflow.body_ref().clone(),
                    published_by_account_id: None,
                    published_at: now,
                },
            );
        }
        Ok(())
    }

    fn row(&self, workflows: &MemoryWorkflows, id: &WorkflowId) -> Option<WorkflowDefinitionRow> {
        let definition = workflows.definitions.get(id)?.clone();
        Some(WorkflowDefinitionRow {
            latest_version: workflows
                .versions
                .get(id)
                .and_then(|versions| versions.keys().next_back().copied()),
            draft_revision: workflows.drafts.get(id).map(|draft| draft.revision),
            definition,
        })
    }
}

impl InMemoryStore {
    /// Exact published version, for synchronous callers such as the Task
    /// dispatcher.
    pub(crate) fn pinned_workflow(
        &self,
        id: &WorkflowId,
        version: u32,
    ) -> Option<Arc<dyn TaskWorkflow>> {
        let workflows = self.workflows.read().expect("lock poisoned");
        let body = workflows.versions.get(id)?.get(&version)?.body.clone();
        Some(Arc::new(DefinitionWorkflow::new(id.to_string(), body)))
    }

    /// Child workflow in the parent definition owner's scope.
    pub(crate) fn child_version(
        &self,
        parent: &WorkflowId,
        child: &WorkflowId,
    ) -> Option<WorkflowVersionRef> {
        let workflows = self.workflows.read().expect("lock poisoned");
        let owner = workflows.definitions.get(parent)?.owner_account_id;
        match latest_in(&workflows, owner, child) {
            WorkflowLookup::Published(reference) => Some(reference),
            WorkflowLookup::Unpublished(_) | WorkflowLookup::NotFound => None,
        }
    }
}

#[async_trait]
impl WorkflowCatalog for InMemoryStore {
    async fn pinned(
        &self,
        reference: &WorkflowVersionRef,
    ) -> Result<Option<Arc<dyn TaskWorkflow>>, RepoError> {
        Ok(self.pinned_workflow(&reference.workflow_id, reference.version))
    }

    async fn latest_visible(
        &self,
        account_id: Option<Uuid>,
        workflow_id: &WorkflowId,
    ) -> Result<WorkflowLookup, RepoError> {
        let workflows = self.workflows.read().expect("lock poisoned");
        Ok(latest_in(&workflows, account_id, workflow_id))
    }

    async fn resolve_child(
        &self,
        parent: &WorkflowId,
        child: &WorkflowId,
    ) -> Result<Option<WorkflowVersionRef>, RepoError> {
        Ok(self.child_version(parent, child))
    }

    async fn root_candidates(&self, account_id: Uuid) -> Result<Vec<WorkflowOption>, RepoError> {
        let rows: Vec<(WorkflowId, String, Option<Uuid>)> = {
            let workflows = self.workflows.read().expect("lock poisoned");
            workflows
                .definitions
                .values()
                .filter(|definition| {
                    definition
                        .owner_account_id
                        .is_none_or(|owner| owner == account_id)
                })
                .filter(|definition| {
                    workflows
                        .versions
                        .get(&definition.id)
                        .is_some_and(|versions| !versions.is_empty())
                })
                .map(|definition| {
                    (
                        definition.id.clone(),
                        definition.title.clone(),
                        definition.owner_account_id,
                    )
                })
                .collect()
        };
        let accounts = self.accounts.read().expect("lock poisoned");
        let mut options: Vec<WorkflowOption> = rows
            .into_iter()
            .map(|(id, title, owner_account_id)| WorkflowOption {
                id,
                title,
                owner: match owner_account_id {
                    None => WorkflowOwner::Platform,
                    Some(owner) => WorkflowOwner::Account {
                        account_id: owner,
                        display_name: accounts
                            .get(&owner)
                            .map(|account| account.display_name.clone())
                            .unwrap_or_default(),
                    },
                },
            })
            .collect();
        options.sort_by(|left, right| {
            owner_rank(&left.owner)
                .cmp(&owner_rank(&right.owner))
                .then_with(|| left.title.cmp(&right.title))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(options)
    }
}

fn owner_rank(owner: &WorkflowOwner) -> u8 {
    match owner {
        WorkflowOwner::Platform => 0,
        WorkflowOwner::Account { .. } => 1,
    }
}

#[async_trait]
impl WorkflowRepository for InMemoryStore {
    async fn insert_definition(
        &self,
        definition: &WorkflowDefinitionDAO,
        draft: &WorkflowDraftDAO,
        layout: Option<&WorkflowLayoutDAO>,
    ) -> Result<(), RepoError> {
        let mut workflows = self.workflows.write().expect("lock poisoned");
        if workflows.definitions.contains_key(&definition.id) {
            return Err(RepoError::Duplicate(format!(
                "workflow id {}",
                definition.id
            )));
        }
        workflows
            .definitions
            .insert(definition.id.clone(), definition.clone());
        workflows
            .drafts
            .insert(definition.id.clone(), draft.clone());
        if let Some(layout) = layout {
            workflows
                .layouts
                .insert(definition.id.clone(), layout.clone());
        }
        Ok(())
    }

    async fn definition(
        &self,
        id: &WorkflowId,
    ) -> Result<Option<WorkflowDefinitionDAO>, RepoError> {
        Ok(self
            .workflows
            .read()
            .expect("lock poisoned")
            .definitions
            .get(id)
            .cloned())
    }

    async fn definition_row(
        &self,
        id: &WorkflowId,
    ) -> Result<Option<WorkflowDefinitionRow>, RepoError> {
        let workflows = self.workflows.read().expect("lock poisoned");
        Ok(self.row(&workflows, id))
    }

    async fn list_definitions(
        &self,
        account_id: Uuid,
        is_admin: bool,
    ) -> Result<Vec<WorkflowDefinitionRow>, RepoError> {
        let workflows = self.workflows.read().expect("lock poisoned");
        let mut rows: Vec<WorkflowDefinitionRow> = workflows
            .definitions
            .values()
            .filter(|definition| match definition.owner_account_id {
                None => {
                    is_admin
                        || workflows
                            .versions
                            .get(&definition.id)
                            .is_some_and(|versions| !versions.is_empty())
                }
                Some(owner) => is_admin || owner == account_id,
            })
            .filter_map(|definition| self.row(&workflows, &definition.id))
            .collect();
        rows.sort_by(|a, b| {
            a.definition
                .owner_account_id
                .is_some()
                .cmp(&b.definition.owner_account_id.is_some())
                .then_with(|| a.definition.title.cmp(&b.definition.title))
                .then_with(|| a.definition.id.cmp(&b.definition.id))
        });
        Ok(rows)
    }

    async fn rename(
        &self,
        id: &WorkflowId,
        title: &str,
        now: OffsetDateTime,
    ) -> Result<(), RepoError> {
        let mut workflows = self.workflows.write().expect("lock poisoned");
        let definition = workflows
            .definitions
            .get_mut(id)
            .ok_or(RepoError::NotFound)?;
        definition.title = title.to_string();
        definition.updated_at = now;
        Ok(())
    }

    async fn versions(&self, id: &WorkflowId) -> Result<Vec<WorkflowVersionRow>, RepoError> {
        let workflows = self.workflows.read().expect("lock poisoned");
        Ok(workflows
            .versions
            .get(id)
            .into_iter()
            .flat_map(|versions| versions.values().rev())
            .map(|version| WorkflowVersionRow {
                version: version.version,
                published_by_account_id: version.published_by_account_id,
                published_at: version.published_at,
            })
            .collect())
    }

    async fn version(
        &self,
        id: &WorkflowId,
        version: u32,
    ) -> Result<Option<WorkflowVersionDAO>, RepoError> {
        Ok(self
            .workflows
            .read()
            .expect("lock poisoned")
            .versions
            .get(id)
            .and_then(|versions| versions.get(&version))
            .cloned())
    }

    async fn latest_version(
        &self,
        id: &WorkflowId,
    ) -> Result<Option<WorkflowVersionDAO>, RepoError> {
        Ok(self
            .workflows
            .read()
            .expect("lock poisoned")
            .versions
            .get(id)
            .and_then(|versions| versions.values().next_back())
            .cloned())
    }

    async fn published_ids(
        &self,
        owner_account_id: Option<Uuid>,
    ) -> Result<Vec<WorkflowId>, RepoError> {
        let workflows = self.workflows.read().expect("lock poisoned");
        Ok(workflows
            .definitions
            .values()
            // Platform definitions are in every scope; an account scope also
            // contains that account's own definitions.
            .filter(|definition| match owner_account_id {
                None => definition.owner_account_id.is_none(),
                Some(owner) => definition
                    .owner_account_id
                    .is_none_or(|current| current == owner),
            })
            .filter(|definition| {
                workflows
                    .versions
                    .get(&definition.id)
                    .is_some_and(|versions| !versions.is_empty())
            })
            .map(|definition| definition.id.clone())
            .collect())
    }

    async fn draft(&self, id: &WorkflowId) -> Result<Option<WorkflowDraftDAO>, RepoError> {
        Ok(self
            .workflows
            .read()
            .expect("lock poisoned")
            .drafts
            .get(id)
            .cloned())
    }

    async fn open_draft(
        &self,
        id: &WorkflowId,
        fallback: WorkflowBody,
        account_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<WorkflowDraftDAO, RepoError> {
        let draft = self
            .workflows
            .write()
            .expect("lock poisoned")
            .drafts
            .entry(id.clone())
            .or_insert_with(|| WorkflowDraftDAO {
                workflow_id: id.clone(),
                revision: 1,
                body: fallback,
                last_client_save_id: None,
                updated_by_account_id: account_id,
                updated_at: now,
            })
            .clone();
        Ok(draft)
    }

    async fn save_draft(
        &self,
        id: &WorkflowId,
        body: &WorkflowBody,
        expected_revision: u64,
        client_save_id: Uuid,
        account_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<DraftSaveOutcome, RepoError> {
        let mut workflows = self.workflows.write().expect("lock poisoned");
        let draft = workflows.drafts.get_mut(id).ok_or(RepoError::NotFound)?;
        if draft.last_client_save_id == Some(client_save_id) {
            return Ok(DraftSaveOutcome::Saved {
                revision: draft.revision,
                updated_at: draft.updated_at,
            });
        }
        if draft.revision != expected_revision {
            return Ok(DraftSaveOutcome::Conflict {
                current_revision: draft.revision,
                current_updated_at: draft.updated_at,
            });
        }
        draft.body = body.clone();
        draft.revision += 1;
        draft.last_client_save_id = Some(client_save_id);
        draft.updated_by_account_id = account_id;
        draft.updated_at = now;
        Ok(DraftSaveOutcome::Saved {
            revision: draft.revision,
            updated_at: draft.updated_at,
        })
    }

    async fn publish(
        &self,
        id: &WorkflowId,
        expected_revision: u64,
        publisher: Uuid,
        now: OffsetDateTime,
    ) -> Result<PublishOutcome, RepoError> {
        let mut workflows = self.workflows.write().expect("lock poisoned");
        let Some(draft) = workflows.drafts.get(id) else {
            return Ok(PublishOutcome::NoDraft);
        };
        if draft.revision != expected_revision {
            return Ok(PublishOutcome::Conflict {
                current_revision: draft.revision,
                current_updated_at: draft.updated_at,
            });
        }
        let body = draft.body.clone();
        let versions = workflows.versions.entry(id.clone()).or_default();
        let version = versions.keys().next_back().map_or(1, |latest| latest + 1);
        versions.insert(
            version,
            WorkflowVersionDAO {
                workflow_id: id.clone(),
                version,
                body,
                published_by_account_id: Some(publisher),
                published_at: now,
            },
        );
        let draft = workflows
            .drafts
            .get_mut(id)
            .expect("draft exists while locked");
        draft.revision += 1;
        draft.last_client_save_id = None;
        draft.updated_by_account_id = publisher;
        draft.updated_at = now;
        Ok(PublishOutcome::Published {
            version,
            revision: draft.revision,
        })
    }

    async fn layout(&self, id: &WorkflowId) -> Result<Option<WorkflowLayoutDAO>, RepoError> {
        Ok(self
            .workflows
            .read()
            .expect("lock poisoned")
            .layouts
            .get(id)
            .cloned())
    }

    async fn save_layout(
        &self,
        id: &WorkflowId,
        layout: &WorkflowLayout,
        account_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<WorkflowLayoutDAO, RepoError> {
        let stored = WorkflowLayoutDAO {
            workflow_id: id.clone(),
            layout: layout.clone(),
            updated_by_account_id: account_id,
            updated_at: now,
        };
        self.workflows
            .write()
            .expect("lock poisoned")
            .layouts
            .insert(id.clone(), stored.clone());
        Ok(stored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_app::WorkflowRepository;

    fn body() -> WorkflowBody {
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "roles": [{"name": "actor", "kind": "any", "min_actors": 1, "max_actors": null}],
            "initial_stage": "work",
            "stages": [{"name": "work", "active_roles": ["actor"]}]
        }))
        .unwrap()
    }

    async fn insert(store: &InMemoryStore, id: &str, owner: Option<Uuid>, publish: bool) {
        let now = OffsetDateTime::now_utc();
        let id = WorkflowId::new(id);
        store
            .insert_definition(
                &WorkflowDefinitionDAO {
                    id: id.clone(),
                    owner_account_id: owner,
                    title: id.to_string(),
                    created_at: now,
                    updated_at: now,
                },
                &WorkflowDraftDAO {
                    workflow_id: id.clone(),
                    revision: 1,
                    body: body(),
                    last_client_save_id: None,
                    updated_by_account_id: owner.unwrap_or_else(Uuid::now_v7),
                    updated_at: now,
                },
                None,
            )
            .await
            .unwrap();
        if publish {
            store
                .publish(&id, 1, owner.unwrap_or_else(Uuid::now_v7), now)
                .await
                .unwrap();
        }
    }

    /// The platform catalog plus one published workflow per account and one
    /// unpublished account workflow.
    async fn store() -> (InMemoryStore, Uuid, Uuid) {
        let store = InMemoryStore::new();
        store.seed_builtin_workflows().unwrap();
        let alice = Uuid::now_v7();
        let bob = Uuid::now_v7();
        insert(&store, "alice_flow", Some(alice), true).await;
        insert(&store, "bob_flow", Some(bob), true).await;
        insert(&store, "alice_draft", Some(alice), false).await;
        (store, alice, bob)
    }

    #[tokio::test]
    async fn latest_visible_scopes_account_definitions() {
        let (store, alice, bob) = store().await;
        let alice_flow = WorkflowId::new("alice_flow");
        assert_eq!(
            store
                .latest_visible(Some(alice), &alice_flow)
                .await
                .unwrap(),
            WorkflowLookup::Published(WorkflowVersionRef {
                workflow_id: alice_flow.clone(),
                version: 1,
            })
        );
        assert_eq!(
            store.latest_visible(Some(bob), &alice_flow).await.unwrap(),
            WorkflowLookup::NotFound
        );
        assert_eq!(
            store.latest_visible(None, &alice_flow).await.unwrap(),
            WorkflowLookup::NotFound
        );
        assert_eq!(
            store
                .latest_visible(Some(alice), &WorkflowId::new("alice_draft"))
                .await
                .unwrap(),
            WorkflowLookup::Unpublished(WorkflowId::new("alice_draft"))
        );
        assert_eq!(
            store
                .latest_visible(Some(bob), &WorkflowId::new("iteration"))
                .await
                .unwrap(),
            WorkflowLookup::Published(WorkflowVersionRef {
                workflow_id: WorkflowId::new("iteration"),
                version: 1,
            })
        );
    }

    #[tokio::test]
    async fn root_candidates_only_offer_platform_and_owned_published_workflows() {
        let (store, alice, _) = store().await;
        let candidates = store.root_candidates(alice).await.unwrap();
        let ids: Vec<&str> = candidates.iter().map(|option| option.id.as_str()).collect();
        assert_eq!(ids.len(), 12, "eleven platform workflows plus alice_flow");
        assert!(ids.contains(&"alice_flow"));
        assert!(!ids.contains(&"bob_flow"));
        assert!(!ids.contains(&"alice_draft"));
        assert!(
            candidates[..11]
                .iter()
                .all(|option| option.owner == WorkflowOwner::Platform)
        );
        assert_eq!(
            candidates[11].owner,
            WorkflowOwner::Account {
                account_id: alice,
                display_name: String::new(),
            }
        );
    }

    #[tokio::test]
    async fn child_resolution_uses_the_parent_owner_scope() {
        let (store, alice, _) = store().await;
        insert(&store, "alice_parent", Some(alice), true).await;
        assert_eq!(
            store.child_version(
                &WorkflowId::new("alice_parent"),
                &WorkflowId::new("iteration")
            ),
            Some(WorkflowVersionRef {
                workflow_id: WorkflowId::new("iteration"),
                version: 1,
            })
        );
        assert_eq!(
            store.child_version(
                &WorkflowId::new("alice_parent"),
                &WorkflowId::new("bob_flow")
            ),
            None
        );
        assert_eq!(
            store.child_version(
                &WorkflowId::new("alice_parent"),
                &WorkflowId::new("alice_draft")
            ),
            None
        );
        assert_eq!(
            store.child_version(&WorkflowId::new("project"), &WorkflowId::new("alice_flow")),
            None
        );
    }
}
