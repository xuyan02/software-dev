//! In-memory storage for structured requirements documents.

use async_trait::async_trait;
use foundry_app::{
    RepoError, RequirementsApplyOutcome, RequirementsApprovalOutcome,
    RequirementsDocumentRepository,
};
use foundry_domain::{
    AnnotationStatus, ApprovalDecision, RequirementsAnnotation, RequirementsApprovalDAO,
    RequirementsDocumentBody, RequirementsDocumentDAO, RequirementsDraftDAO, RequirementsOperation,
    RequirementsVersionDAO, RequirementsVersionRow,
};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::InMemoryStore;

#[derive(Default)]
pub(crate) struct MemoryRequirementDocs {
    documents: HashMap<Uuid, RequirementsDocumentDAO>,
    drafts: HashMap<Uuid, RequirementsDraftDAO>,
    versions: HashMap<(Uuid, u32), RequirementsVersionDAO>,
    approvals: Vec<RequirementsApprovalDAO>,
    applied_ops: HashSet<(Uuid, Uuid)>,
    annotations: Vec<RequirementsAnnotation>,
}

impl MemoryRequirementDocs {
    pub(crate) fn delete_project(&mut self, project_id: Uuid) {
        self.documents.remove(&project_id);
        self.drafts.remove(&project_id);
        self.versions.retain(|(id, _), _| *id != project_id);
        self.approvals
            .retain(|approval| approval.project_id != project_id);
        self.applied_ops.retain(|(id, _)| *id != project_id);
        self.annotations
            .retain(|annotation| annotation.project_id != project_id);
    }
}

#[async_trait]
impl RequirementsDocumentRepository for InMemoryStore {
    async fn document(
        &self,
        project_id: Uuid,
    ) -> Result<Option<RequirementsDocumentDAO>, RepoError> {
        Ok(self
            .requirements_docs
            .read()
            .expect("lock poisoned")
            .documents
            .get(&project_id)
            .cloned())
    }

    async fn ensure_document(
        &self,
        project_id: Uuid,
        _actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<(RequirementsDocumentDAO, RequirementsDraftDAO), RepoError> {
        let mut docs = self.requirements_docs.write().expect("lock poisoned");
        let document = docs
            .documents
            .entry(project_id)
            .or_insert_with(|| RequirementsDocumentDAO {
                id: Uuid::now_v7(),
                project_id,
                current_version: 0,
                status: "draft".to_string(),
                created_at: now,
                updated_at: now,
            })
            .clone();
        let draft = docs
            .drafts
            .entry(project_id)
            .or_insert_with(|| RequirementsDraftDAO {
                document_id: document.id,
                project_id,
                revision: 1,
                base_version: 0,
                body: RequirementsDocumentBody::default(),
                updated_at: now,
            })
            .clone();
        Ok((document, draft))
    }

    async fn draft(&self, project_id: Uuid) -> Result<Option<RequirementsDraftDAO>, RepoError> {
        Ok(self
            .requirements_docs
            .read()
            .expect("lock poisoned")
            .drafts
            .get(&project_id)
            .cloned())
    }

    async fn apply_operations(
        &self,
        project_id: Uuid,
        expected_revision: u64,
        operations: &[RequirementsOperation],
        _actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<RequirementsApplyOutcome, RepoError> {
        let mut docs = self.requirements_docs.write().expect("lock poisoned");
        let Some(current) = docs.drafts.get(&project_id).cloned() else {
            return Ok(RequirementsApplyOutcome::NoDraft);
        };
        let new_operations: Vec<RequirementsOperation> = operations
            .iter()
            .filter(|operation| !docs.applied_ops.contains(&(project_id, operation.op_id)))
            .cloned()
            .collect();
        if new_operations.is_empty() {
            return Ok(RequirementsApplyOutcome::Applied {
                revision: current.revision,
                updated_at: current.updated_at,
            });
        }
        if current.revision != expected_revision {
            return Ok(RequirementsApplyOutcome::Conflict {
                current_revision: current.revision,
                current_updated_at: current.updated_at,
            });
        }
        let body = foundry_domain::apply_operations(&current.body, &new_operations)
            .map_err(|error| RepoError::Invalid(error.to_string()))?;
        let bytes = serde_json::to_vec(&body).map_err(memory_backend)?;
        if bytes.len() > foundry_domain::MAX_REQUIREMENTS_BYTES {
            return Ok(RequirementsApplyOutcome::TooLarge);
        }
        let revision = current.revision + 1;
        docs.drafts.insert(
            project_id,
            RequirementsDraftDAO {
                document_id: current.document_id,
                project_id,
                revision,
                base_version: current.base_version,
                body,
                updated_at: now,
            },
        );
        for operation in new_operations {
            docs.applied_ops.insert((project_id, operation.op_id));
        }
        Ok(RequirementsApplyOutcome::Applied {
            revision,
            updated_at: now,
        })
    }

    async fn approve(
        &self,
        project_id: Uuid,
        expected_revision: u64,
        actor_id: Uuid,
        comment: Option<&str>,
        now: OffsetDateTime,
    ) -> Result<RequirementsApprovalOutcome, RepoError> {
        let mut docs = self.requirements_docs.write().expect("lock poisoned");
        let Some(document) = docs.documents.get(&project_id).cloned() else {
            return Ok(RequirementsApprovalOutcome::NoDraft);
        };
        let Some(draft) = docs.drafts.get(&project_id).cloned() else {
            return Ok(RequirementsApprovalOutcome::NoDraft);
        };
        if let Some(existing) = docs
            .versions
            .values()
            .find(|version| {
                version.project_id == project_id && version.source_revision == expected_revision
            })
            .cloned()
        {
            return Ok(RequirementsApprovalOutcome::AlreadyPublished {
                version: existing.version,
            });
        }
        if draft.revision != expected_revision {
            return Ok(RequirementsApprovalOutcome::Conflict {
                current_revision: draft.revision,
                current_updated_at: draft.updated_at,
            });
        }
        let validation = foundry_domain::validate_requirements_document(&draft.body);
        if !validation.is_valid() {
            return Err(RepoError::Invalid(format!(
                "requirements document is invalid: {}",
                validation
                    .errors
                    .iter()
                    .map(|issue| format!("{}: {}", issue.path, issue.message))
                    .collect::<Vec<_>>()
                    .join("; ")
            )));
        }
        let bytes = serde_json::to_vec(&draft.body).map_err(memory_backend)?;
        if bytes.len() > foundry_domain::MAX_REQUIREMENTS_BYTES {
            return Ok(RequirementsApprovalOutcome::TooLarge);
        }
        let version = document.current_version + 1;
        let approval_id = Uuid::now_v7();
        let hash = content_hash(&bytes);
        docs.approvals.push(RequirementsApprovalDAO {
            id: approval_id,
            project_id,
            revision: draft.revision,
            decision: ApprovalDecision::Approved,
            actor_id,
            comment: comment.map(str::to_string),
            content_hash: hash.clone(),
            created_at: now,
        });
        docs.versions.insert(
            (project_id, version),
            RequirementsVersionDAO {
                document_id: document.id,
                project_id,
                version,
                body: draft.body.clone(),
                content_hash: hash,
                source_revision: draft.revision,
                created_by: actor_id,
                approved_by: Some(actor_id),
                approval_comment: comment.map(str::to_string),
                created_at: now,
                published_at: Some(now),
            },
        );
        docs.documents.insert(
            project_id,
            RequirementsDocumentDAO {
                current_version: version,
                status: "published".to_string(),
                updated_at: now,
                ..document
            },
        );
        docs.drafts.insert(
            project_id,
            RequirementsDraftDAO {
                base_version: version,
                updated_at: now,
                ..draft.clone()
            },
        );
        Ok(RequirementsApprovalOutcome::Approved {
            version,
            revision: draft.revision,
            approval_id,
        })
    }

    async fn reject(
        &self,
        project_id: Uuid,
        expected_revision: u64,
        actor_id: Uuid,
        comment: &str,
        now: OffsetDateTime,
    ) -> Result<RequirementsApprovalOutcome, RepoError> {
        let mut docs = self.requirements_docs.write().expect("lock poisoned");
        let Some(draft) = docs.drafts.get(&project_id).cloned() else {
            return Ok(RequirementsApprovalOutcome::NoDraft);
        };
        if let Some(existing) = docs
            .versions
            .values()
            .find(|version| {
                version.project_id == project_id && version.source_revision == expected_revision
            })
            .cloned()
        {
            return Ok(RequirementsApprovalOutcome::AlreadyPublished {
                version: existing.version,
            });
        }
        if draft.revision != expected_revision {
            return Ok(RequirementsApprovalOutcome::Conflict {
                current_revision: draft.revision,
                current_updated_at: draft.updated_at,
            });
        }
        let bytes = serde_json::to_vec(&draft.body).map_err(memory_backend)?;
        if let Some(existing) = docs.approvals.iter().find(|approval| {
            approval.project_id == project_id
                && approval.revision == expected_revision
                && approval.decision == ApprovalDecision::Rejected
        }) {
            return Ok(RequirementsApprovalOutcome::Rejected {
                approval_id: existing.id,
            });
        }
        let approval_id = Uuid::now_v7();
        docs.approvals.push(RequirementsApprovalDAO {
            id: approval_id,
            project_id,
            revision: draft.revision,
            decision: ApprovalDecision::Rejected,
            actor_id,
            comment: Some(comment.to_string()),
            content_hash: content_hash(&bytes),
            created_at: now,
        });
        Ok(RequirementsApprovalOutcome::Rejected { approval_id })
    }

    async fn approvals(&self, project_id: Uuid) -> Result<Vec<RequirementsApprovalDAO>, RepoError> {
        let docs = self.requirements_docs.read().expect("lock poisoned");
        let mut approvals: Vec<RequirementsApprovalDAO> = docs
            .approvals
            .iter()
            .filter(|approval| approval.project_id == project_id)
            .cloned()
            .collect();
        approvals.sort_by_key(|approval| approval.created_at);
        Ok(approvals)
    }

    async fn versions(&self, project_id: Uuid) -> Result<Vec<RequirementsVersionRow>, RepoError> {
        let docs = self.requirements_docs.read().expect("lock poisoned");
        let mut versions: Vec<RequirementsVersionRow> = docs
            .versions
            .values()
            .filter(|version| version.project_id == project_id)
            .map(|version| RequirementsVersionRow {
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
        version: u32,
    ) -> Result<Option<RequirementsVersionDAO>, RepoError> {
        Ok(self
            .requirements_docs
            .read()
            .expect("lock poisoned")
            .versions
            .get(&(project_id, version))
            .cloned())
    }

    async fn latest_version(
        &self,
        project_id: Uuid,
    ) -> Result<Option<RequirementsVersionDAO>, RepoError> {
        let docs = self.requirements_docs.read().expect("lock poisoned");
        Ok(docs
            .versions
            .values()
            .filter(|version| version.project_id == project_id)
            .max_by_key(|version| version.version)
            .cloned())
    }

    async fn replace_draft(
        &self,
        project_id: Uuid,
        expected_revision: u64,
        body: &RequirementsDocumentBody,
        _actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<RequirementsApplyOutcome, RepoError> {
        let mut docs = self.requirements_docs.write().expect("lock poisoned");
        let Some(current) = docs.drafts.get(&project_id).cloned() else {
            return Ok(RequirementsApplyOutcome::NoDraft);
        };
        if current.revision != expected_revision {
            return Ok(RequirementsApplyOutcome::Conflict {
                current_revision: current.revision,
                current_updated_at: current.updated_at,
            });
        }
        let bytes = serde_json::to_vec(body).map_err(memory_backend)?;
        if bytes.len() > foundry_domain::MAX_REQUIREMENTS_BYTES {
            return Ok(RequirementsApplyOutcome::TooLarge);
        }
        let revision = current.revision + 1;
        docs.drafts.insert(
            project_id,
            RequirementsDraftDAO {
                revision,
                body: body.clone(),
                updated_at: now,
                ..current
            },
        );
        Ok(RequirementsApplyOutcome::Applied {
            revision,
            updated_at: now,
        })
    }

    async fn annotations(
        &self,
        project_id: Uuid,
        include_resolved: bool,
    ) -> Result<Vec<RequirementsAnnotation>, RepoError> {
        Ok(self
            .requirements_docs
            .read()
            .expect("lock poisoned")
            .annotations
            .iter()
            .filter(|annotation| {
                annotation.project_id == project_id
                    && (include_resolved || annotation.status == AnnotationStatus::Open)
            })
            .cloned()
            .collect())
    }

    async fn annotation(
        &self,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<Option<RequirementsAnnotation>, RepoError> {
        Ok(self
            .requirements_docs
            .read()
            .expect("lock poisoned")
            .annotations
            .iter()
            .find(|annotation| annotation.project_id == project_id && annotation.id == id)
            .cloned())
    }

    async fn insert_annotation(
        &self,
        annotation: &RequirementsAnnotation,
    ) -> Result<(), RepoError> {
        self.requirements_docs
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
    ) -> Result<Option<RequirementsAnnotation>, RepoError> {
        let mut docs = self.requirements_docs.write().expect("lock poisoned");
        let Some(annotation) = docs.annotations.iter_mut().find(|annotation| {
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

fn memory_backend(error: impl std::fmt::Display) -> RepoError {
    RepoError::Backend(error.to_string())
}

fn content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(7 + digest.len() * 2);
    hex.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}
