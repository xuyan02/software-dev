//! In-memory storage for anchor documents.

use async_trait::async_trait;
use foundry_app::{
    AnchorApplyOutcome, AnchorApprovalOutcome, AnchorDocumentApprovalDAO, AnchorDocumentDAO,
    AnchorDocumentDraftDAO, AnchorDocumentRepository, AnchorDocumentVersionDAO,
    AnchorDocumentVersionRow, RepoError,
};
use foundry_domain::{
    AnchorAnnotation, AnchorAnnotationStatus, AnchorOperation, ApprovalDecision, DocumentSchema,
    DocumentSnapshot, MAX_ANCHOR_DOCUMENT_BYTES,
};
use std::collections::{HashMap, HashSet};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::InMemoryStore;

#[derive(Default)]
pub(crate) struct MemoryAnchorDocuments {
    documents: HashMap<(Uuid, String), AnchorDocumentDAO>,
    drafts: HashMap<(Uuid, String), AnchorDocumentDraftDAO>,
    versions: HashMap<(Uuid, String, u32), AnchorDocumentVersionDAO>,
    approvals: Vec<AnchorDocumentApprovalDAO>,
    annotations: Vec<AnchorAnnotation>,
    applied_ops: HashSet<(Uuid, String, Uuid)>,
}

impl MemoryAnchorDocuments {
    pub(crate) fn delete_project(&mut self, project_id: Uuid) {
        self.documents.retain(|(id, _), _| *id != project_id);
        self.drafts.retain(|(id, _), _| *id != project_id);
        self.versions.retain(|(id, _, _), _| *id != project_id);
        self.approvals
            .retain(|approval| approval.project_id != project_id);
        self.annotations
            .retain(|annotation| annotation.project_id != project_id);
        self.applied_ops.retain(|(id, _, _)| *id != project_id);
    }
}

fn too_large(snapshot: &DocumentSnapshot) -> Result<bool, RepoError> {
    Ok(serde_json::to_vec(snapshot)
        .map_err(|error| RepoError::Backend(error.to_string()))?
        .len()
        > MAX_ANCHOR_DOCUMENT_BYTES)
}

#[async_trait]
impl AnchorDocumentRepository for InMemoryStore {
    async fn document(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Option<AnchorDocumentDAO>, RepoError> {
        Ok(self
            .anchor_documents
            .read()
            .expect("lock poisoned")
            .documents
            .get(&(project_id, kind.to_string()))
            .cloned())
    }

    async fn ensure_document(
        &self,
        project_id: Uuid,
        kind: &str,
        schema: &DocumentSchema,
        _actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<(AnchorDocumentDAO, AnchorDocumentDraftDAO), RepoError> {
        let mut docs = self.anchor_documents.write().expect("lock poisoned");
        let document = docs
            .documents
            .entry((project_id, kind.to_string()))
            .or_insert_with(|| AnchorDocumentDAO {
                id: Uuid::now_v7(),
                project_id,
                kind: kind.to_string(),
                schema_version: schema.schema_version,
                current_version: 0,
                status: "draft".to_string(),
                created_at: now,
                updated_at: now,
            })
            .clone();
        let draft = docs
            .drafts
            .entry((project_id, kind.to_string()))
            .or_insert_with(|| AnchorDocumentDraftDAO {
                document_id: document.id,
                project_id,
                kind: kind.to_string(),
                revision: 1,
                base_version: 0,
                schema_version: schema.schema_version,
                snapshot: DocumentSnapshot::empty(schema),
                updated_at: now,
            })
            .clone();
        Ok((document, draft))
    }

    async fn draft(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Option<AnchorDocumentDraftDAO>, RepoError> {
        Ok(self
            .anchor_documents
            .read()
            .expect("lock poisoned")
            .drafts
            .get(&(project_id, kind.to_string()))
            .cloned())
    }

    async fn apply_operations(
        &self,
        project_id: Uuid,
        kind: &str,
        expected_revision: u64,
        operations: &[AnchorOperation],
        _actor_id: Uuid,
        now: OffsetDateTime,
        schema: &DocumentSchema,
    ) -> Result<AnchorApplyOutcome, RepoError> {
        let mut docs = self.anchor_documents.write().expect("lock poisoned");
        let key = (project_id, kind.to_string());
        let Some(current) = docs.drafts.get(&key).cloned() else {
            return Ok(AnchorApplyOutcome::NoDraft);
        };
        let new_operations: Vec<AnchorOperation> = operations
            .iter()
            .filter(|operation| {
                !docs
                    .applied_ops
                    .contains(&(project_id, kind.to_string(), operation.op_id))
            })
            .cloned()
            .collect();
        if new_operations.is_empty() {
            return Ok(AnchorApplyOutcome::Applied {
                revision: current.revision,
                root_id: current.snapshot.root_id().to_string(),
                created_key: None,
                updated_at: current.updated_at,
            });
        }
        if current.revision != expected_revision {
            return Ok(AnchorApplyOutcome::Conflict {
                current_revision: current.revision,
                current_updated_at: current.updated_at,
            });
        }
        let mut snapshot = current.snapshot.clone();
        let mut created_key = None;
        for operation in &new_operations {
            match snapshot.apply(schema, &operation.op) {
                Ok((next, key)) => {
                    snapshot = next;
                    if key.is_some() {
                        created_key = key;
                    }
                }
                Err(error) => return Ok(AnchorApplyOutcome::Invalid(error.to_string())),
            }
        }
        if too_large(&snapshot)? {
            return Ok(AnchorApplyOutcome::TooLarge);
        }
        let revision = current.revision + 1;
        docs.drafts.insert(
            key.clone(),
            AnchorDocumentDraftDAO {
                document_id: current.document_id,
                project_id,
                kind: kind.to_string(),
                revision,
                base_version: current.base_version,
                schema_version: current.schema_version,
                snapshot,
                updated_at: now,
            },
        );
        for operation in new_operations {
            docs.applied_ops
                .insert((project_id, kind.to_string(), operation.op_id));
        }
        Ok(AnchorApplyOutcome::Applied {
            revision,
            root_id: docs.drafts[&key].snapshot.root_id().to_string(),
            created_key,
            updated_at: now,
        })
    }

    async fn approve(
        &self,
        project_id: Uuid,
        kind: &str,
        expected_revision: u64,
        actor_id: Uuid,
        comment: Option<&str>,
        now: OffsetDateTime,
        schema: &DocumentSchema,
    ) -> Result<AnchorApprovalOutcome, RepoError> {
        let mut docs = self.anchor_documents.write().expect("lock poisoned");
        let key = (project_id, kind.to_string());
        let Some(document) = docs.documents.get(&key).cloned() else {
            return Ok(AnchorApprovalOutcome::NoDraft);
        };
        let Some(draft) = docs.drafts.get(&key).cloned() else {
            return Ok(AnchorApprovalOutcome::NoDraft);
        };
        if let Some(existing) = docs
            .versions
            .iter()
            .find(|((project, document_kind, _), version)| {
                *project == project_id
                    && document_kind == kind
                    && version.source_revision == expected_revision
            })
            .map(|(_, version)| version.clone())
        {
            return Ok(AnchorApprovalOutcome::AlreadyPublished {
                version: existing.version,
            });
        }
        if draft.revision != expected_revision {
            return Ok(AnchorApprovalOutcome::Conflict {
                current_revision: draft.revision,
                current_updated_at: draft.updated_at,
            });
        }
        let validation = draft.snapshot.validate(schema);
        if !validation.is_valid() {
            return Ok(AnchorApprovalOutcome::Invalid(validation));
        }
        if too_large(&draft.snapshot)? {
            return Ok(AnchorApprovalOutcome::TooLarge);
        }
        let version = document.current_version + 1;
        let approval_id = Uuid::now_v7();
        let content_hash = draft.snapshot.root_id().to_string();
        docs.approvals.push(AnchorDocumentApprovalDAO {
            id: approval_id,
            project_id,
            kind: kind.to_string(),
            revision: draft.revision,
            decision: ApprovalDecision::Approved,
            actor_id,
            comment: comment.map(str::to_string),
            content_hash: content_hash.clone(),
            created_at: now,
        });
        docs.versions.insert(
            (project_id, kind.to_string(), version),
            AnchorDocumentVersionDAO {
                document_id: document.id,
                project_id,
                kind: kind.to_string(),
                version,
                snapshot: draft.snapshot.clone(),
                content_hash,
                source_revision: draft.revision,
                created_by: actor_id,
                approved_by: Some(actor_id),
                approval_comment: comment.map(str::to_string),
                created_at: now,
                published_at: Some(now),
            },
        );
        docs.documents.insert(
            key.clone(),
            AnchorDocumentDAO {
                current_version: version,
                status: "published".to_string(),
                updated_at: now,
                ..document
            },
        );
        docs.drafts.insert(
            key,
            AnchorDocumentDraftDAO {
                base_version: version,
                updated_at: now,
                ..draft
            },
        );
        Ok(AnchorApprovalOutcome::Approved {
            version,
            revision: expected_revision,
            approval_id,
        })
    }

    async fn reject(
        &self,
        project_id: Uuid,
        kind: &str,
        expected_revision: u64,
        actor_id: Uuid,
        comment: &str,
        now: OffsetDateTime,
    ) -> Result<AnchorApprovalOutcome, RepoError> {
        let mut docs = self.anchor_documents.write().expect("lock poisoned");
        let key = (project_id, kind.to_string());
        let Some(draft) = docs.drafts.get(&key).cloned() else {
            return Ok(AnchorApprovalOutcome::NoDraft);
        };
        if let Some(existing) = docs
            .versions
            .iter()
            .find(|((project, document_kind, _), version)| {
                *project == project_id
                    && document_kind == kind
                    && version.source_revision == expected_revision
            })
            .map(|(_, version)| version.clone())
        {
            return Ok(AnchorApprovalOutcome::AlreadyPublished {
                version: existing.version,
            });
        }
        if draft.revision != expected_revision {
            return Ok(AnchorApprovalOutcome::Conflict {
                current_revision: draft.revision,
                current_updated_at: draft.updated_at,
            });
        }
        if let Some(existing) = docs.approvals.iter().find(|approval| {
            approval.project_id == project_id
                && approval.kind == kind
                && approval.revision == expected_revision
                && approval.decision == ApprovalDecision::Rejected
        }) {
            return Ok(AnchorApprovalOutcome::Rejected {
                approval_id: existing.id,
            });
        }
        let approval_id = Uuid::now_v7();
        docs.approvals.push(AnchorDocumentApprovalDAO {
            id: approval_id,
            project_id,
            kind: kind.to_string(),
            revision: expected_revision,
            decision: ApprovalDecision::Rejected,
            actor_id,
            comment: Some(comment.to_string()),
            content_hash: draft.snapshot.root_id().to_string(),
            created_at: now,
        });
        Ok(AnchorApprovalOutcome::Rejected { approval_id })
    }

    async fn approvals(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Vec<AnchorDocumentApprovalDAO>, RepoError> {
        Ok(self
            .anchor_documents
            .read()
            .expect("lock poisoned")
            .approvals
            .iter()
            .filter(|approval| approval.project_id == project_id && approval.kind == kind)
            .cloned()
            .collect())
    }

    async fn versions(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Vec<AnchorDocumentVersionRow>, RepoError> {
        let mut versions: Vec<AnchorDocumentVersionRow> = self
            .anchor_documents
            .read()
            .expect("lock poisoned")
            .versions
            .iter()
            .filter(|((project, document_kind, _), _)| {
                *project == project_id && document_kind == kind
            })
            .map(|(_, version)| AnchorDocumentVersionRow {
                version: version.version,
                created_by: version.created_by,
                approved_by: version.approved_by,
                source_revision: Some(version.source_revision),
                published_at: version.published_at,
            })
            .collect();
        versions.sort_by(|left, right| right.version.cmp(&left.version));
        Ok(versions)
    }

    async fn version(
        &self,
        project_id: Uuid,
        kind: &str,
        version: u32,
    ) -> Result<Option<AnchorDocumentVersionDAO>, RepoError> {
        Ok(self
            .anchor_documents
            .read()
            .expect("lock poisoned")
            .versions
            .get(&(project_id, kind.to_string(), version))
            .cloned())
    }

    async fn latest_version(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Option<AnchorDocumentVersionDAO>, RepoError> {
        Ok(self
            .anchor_documents
            .read()
            .expect("lock poisoned")
            .versions
            .iter()
            .filter(|((project, document_kind, _), _)| {
                *project == project_id && document_kind == kind
            })
            .max_by_key(|((_, _, version), _)| *version)
            .map(|(_, version)| version.clone()))
    }

    async fn annotations(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Vec<AnchorAnnotation>, RepoError> {
        Ok(self
            .anchor_documents
            .read()
            .expect("lock poisoned")
            .annotations
            .iter()
            .filter(|annotation| annotation.project_id == project_id && annotation.kind == kind)
            .cloned()
            .collect())
    }

    async fn insert_annotation(
        &self,
        annotation: AnchorAnnotation,
    ) -> Result<AnchorAnnotation, RepoError> {
        self.anchor_documents
            .write()
            .expect("lock poisoned")
            .annotations
            .push(annotation.clone());
        Ok(annotation)
    }

    async fn resolve_annotation(
        &self,
        project_id: Uuid,
        kind: &str,
        annotation_id: Uuid,
        resolution: &str,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<Option<AnchorAnnotation>, RepoError> {
        let mut docs = self.anchor_documents.write().expect("lock poisoned");
        let Some(annotation) = docs.annotations.iter_mut().find(|annotation| {
            annotation.id == annotation_id
                && annotation.project_id == project_id
                && annotation.kind == kind
        }) else {
            return Ok(None);
        };
        annotation.status = AnchorAnnotationStatus::Resolved;
        annotation.resolution = Some(resolution.to_string());
        annotation.resolved_at = Some(now);
        annotation.resolved_by = Some(actor_id);
        Ok(Some(annotation.clone()))
    }
}
