//! In-memory project brief drafts, versions and annotations.

use async_trait::async_trait;
use foundry_app::{
    ApproveBriefOutcome, DraftOperationOutcome, ProjectBriefRepository, RepoError,
    SubmitDraftOutcome,
};
use foundry_domain::{
    AnnotationStatus, ProjectBriefAnnotation, ProjectBriefBody, ProjectBriefDraftDAO,
    ProjectBriefDraftOperation, ProjectBriefStatus, ProjectBriefVersionDAO,
};
use std::collections::{HashMap, HashSet};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::InMemoryStore;

#[derive(Default)]
pub(crate) struct MemoryProjectBriefs {
    versions: HashMap<Uuid, Vec<ProjectBriefVersionDAO>>,
    current: HashMap<Uuid, u32>,
    drafts: HashMap<Uuid, ProjectBriefDraftDAO>,
    operations: HashSet<(Uuid, Uuid)>,
    annotations: Vec<ProjectBriefAnnotation>,
}

impl MemoryProjectBriefs {
    pub(crate) fn delete_project(&mut self, project_id: Uuid) {
        self.versions.remove(&project_id);
        self.current.remove(&project_id);
        self.drafts.retain(|_, draft| draft.project_id != project_id);
        self.operations
            .retain(|(draft_id, _)| self.drafts.contains_key(draft_id));
        self.annotations
            .retain(|annotation| annotation.project_id != project_id);
    }
}

#[async_trait]
impl ProjectBriefRepository for InMemoryStore {
    async fn current(
        &self,
        project_id: Uuid,
    ) -> Result<Option<ProjectBriefVersionDAO>, RepoError> {
        let briefs = self.project_briefs.read().expect("lock poisoned");
        let Some(version) = briefs.current.get(&project_id) else {
            return Ok(None);
        };
        Ok(briefs
            .versions
            .get(&project_id)
            .and_then(|versions| versions.iter().find(|item| item.version == *version))
            .cloned())
    }

    async fn versions(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<ProjectBriefVersionDAO>, RepoError> {
        let mut versions = self
            .project_briefs
            .read()
            .expect("lock poisoned")
            .versions
            .get(&project_id)
            .cloned()
            .unwrap_or_default();
        versions.sort_by(|a, b| b.version.cmp(&a.version));
        Ok(versions)
    }

    async fn version(
        &self,
        project_id: Uuid,
        version: u32,
    ) -> Result<Option<ProjectBriefVersionDAO>, RepoError> {
        Ok(self
            .project_briefs
            .read()
            .expect("lock poisoned")
            .versions
            .get(&project_id)
            .and_then(|versions| versions.iter().find(|item| item.version == version))
            .cloned())
    }

    async fn create_draft(
        &self,
        project_id: Uuid,
        body: &ProjectBriefBody,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<ProjectBriefDraftDAO, RepoError> {
        let draft = ProjectBriefDraftDAO {
            id: Uuid::now_v7(),
            project_id,
            revision: 1,
            body: body.clone(),
            created_by: actor_id,
            updated_by: actor_id,
            created_at: now,
            updated_at: now,
        };
        self.project_briefs
            .write()
            .expect("lock poisoned")
            .drafts
            .insert(draft.id, draft.clone());
        Ok(draft)
    }

    async fn drafts(
        &self,
        project_id: Uuid,
        created_by: Uuid,
    ) -> Result<Vec<ProjectBriefDraftDAO>, RepoError> {
        let mut drafts: Vec<ProjectBriefDraftDAO> = self
            .project_briefs
            .read()
            .expect("lock poisoned")
            .drafts
            .values()
            .filter(|draft| draft.project_id == project_id && draft.created_by == created_by)
            .cloned()
            .collect();
        drafts.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(drafts)
    }

    async fn draft(
        &self,
        project_id: Uuid,
        draft_id: Uuid,
    ) -> Result<Option<ProjectBriefDraftDAO>, RepoError> {
        Ok(self
            .project_briefs
            .read()
            .expect("lock poisoned")
            .drafts
            .get(&draft_id)
            .filter(|draft| draft.project_id == project_id)
            .cloned())
    }

    async fn commit_draft_operation(
        &self,
        project_id: Uuid,
        draft_id: Uuid,
        expected_revision: u64,
        op_id: Uuid,
        _operation: &ProjectBriefDraftOperation,
        body: &ProjectBriefBody,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<DraftOperationOutcome, RepoError> {
        let mut briefs = self.project_briefs.write().expect("lock poisoned");
        let Some(draft) = briefs
            .drafts
            .get(&draft_id)
            .filter(|draft| draft.project_id == project_id)
            .cloned()
        else {
            return Err(RepoError::NotFound);
        };
        if briefs.operations.contains(&(draft_id, op_id)) {
            return Ok(DraftOperationOutcome::Duplicate(draft));
        }
        if draft.revision != expected_revision {
            return Ok(DraftOperationOutcome::Conflict {
                current_revision: draft.revision,
            });
        }
        let updated = ProjectBriefDraftDAO {
            revision: draft.revision + 1,
            body: body.clone(),
            updated_by: actor_id,
            updated_at: now,
            ..draft
        };
        briefs.drafts.insert(draft_id, updated.clone());
        briefs.operations.insert((draft_id, op_id));
        Ok(DraftOperationOutcome::Applied(updated))
    }

    async fn submit_draft(
        &self,
        project_id: Uuid,
        draft_id: Uuid,
        expected_revision: u64,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<SubmitDraftOutcome, RepoError> {
        let mut briefs = self.project_briefs.write().expect("lock poisoned");
        let Some(draft) = briefs
            .drafts
            .get(&draft_id)
            .filter(|draft| draft.project_id == project_id)
            .cloned()
        else {
            return Err(RepoError::NotFound);
        };
        if draft.revision != expected_revision {
            return Ok(SubmitDraftOutcome::Conflict {
                current_revision: draft.revision,
            });
        }
        let next = briefs
            .current
            .get(&project_id)
            .map(|version| version + 1)
            .unwrap_or(1);
        if let Some(current) = briefs.current.get(&project_id).copied()
            && let Some(versions) = briefs.versions.get_mut(&project_id)
            && let Some(previous) = versions.iter_mut().find(|item| item.version == current)
        {
            previous.status = ProjectBriefStatus::Superseded;
            previous.updated_at = now;
        }
        let version = ProjectBriefVersionDAO {
            project_id,
            version: next,
            status: ProjectBriefStatus::InReview,
            body: draft.body,
            created_by: actor_id,
            approved_by: None,
            created_at: now,
            updated_at: now,
        };
        briefs
            .versions
            .entry(project_id)
            .or_default()
            .push(version.clone());
        briefs.current.insert(project_id, next);
        briefs.drafts.remove(&draft_id);
        briefs
            .operations
            .retain(|(id, _)| *id != draft_id);
        Ok(SubmitDraftOutcome::Submitted(Box::new(version)))
    }

    async fn approve_current(
        &self,
        project_id: Uuid,
        expected_version: u32,
        approver_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<ApproveBriefOutcome, RepoError> {
        let mut briefs = self.project_briefs.write().expect("lock poisoned");
        let Some(current) = briefs.current.get(&project_id).copied() else {
            return Err(RepoError::NotFound);
        };
        if current != expected_version {
            return Ok(ApproveBriefOutcome::Conflict {
                current_version: current,
            });
        }
        let Some(versions) = briefs.versions.get_mut(&project_id) else {
            return Err(RepoError::NotFound);
        };
        let Some(version) = versions.iter_mut().find(|item| item.version == current) else {
            return Err(RepoError::NotFound);
        };
        if version.status != ProjectBriefStatus::InReview {
            return Ok(ApproveBriefOutcome::NotInReview);
        }
        version.status = ProjectBriefStatus::Approved;
        version.approved_by = Some(approver_id);
        version.updated_at = now;
        Ok(ApproveBriefOutcome::Approved(Box::new(version.clone())))
    }

    async fn annotations(
        &self,
        project_id: Uuid,
        include_resolved: bool,
    ) -> Result<Vec<ProjectBriefAnnotation>, RepoError> {
        let mut annotations: Vec<ProjectBriefAnnotation> = self
            .project_briefs
            .read()
            .expect("lock poisoned")
            .annotations
            .iter()
            .filter(|annotation| {
                annotation.project_id == project_id
                    && (include_resolved || annotation.status == AnnotationStatus::Open)
            })
            .cloned()
            .collect();
        annotations.sort_by_key(|annotation| annotation.created_at);
        Ok(annotations)
    }

    async fn annotation(
        &self,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<Option<ProjectBriefAnnotation>, RepoError> {
        Ok(self
            .project_briefs
            .read()
            .expect("lock poisoned")
            .annotations
            .iter()
            .find(|annotation| annotation.project_id == project_id && annotation.id == id)
            .cloned())
    }

    async fn insert_annotation(
        &self,
        annotation: &ProjectBriefAnnotation,
    ) -> Result<(), RepoError> {
        self.project_briefs
            .write()
            .expect("lock poisoned")
            .annotations
            .push(annotation.clone());
        Ok(())
    }

    async fn resolve_annotation(
        &self,
        project_id: Uuid,
        id: Uuid,
        resolver_id: Uuid,
        resolution: &str,
        now: OffsetDateTime,
    ) -> Result<Option<ProjectBriefAnnotation>, RepoError> {
        let mut briefs = self.project_briefs.write().expect("lock poisoned");
        let Some(annotation) = briefs.annotations.iter_mut().find(|annotation| {
            annotation.project_id == project_id
                && annotation.id == id
                && annotation.status == AnnotationStatus::Open
        }) else {
            return Ok(None);
        };
        annotation.status = AnnotationStatus::Resolved;
        annotation.resolution = Some(resolution.to_string());
        annotation.resolved_at = Some(now);
        annotation.resolved_by = Some(resolver_id);
        Ok(Some(annotation.clone()))
    }
}
