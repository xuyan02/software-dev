//! Postgres storage for structured requirements documents.

use async_trait::async_trait;
use foundry_app::{
    RepoError, RequirementsApplyOutcome, RequirementsApprovalOutcome,
    RequirementsDocumentRepository,
};
use foundry_domain::{
    AnnotationStatus, AnnotationTarget, ApprovalDecision, RequirementsAnnotation,
    RequirementsApprovalDAO, RequirementsDocumentBody, RequirementsDocumentDAO,
    RequirementsDraftDAO, RequirementsOperation, RequirementsVersionDAO, RequirementsVersionRow,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::PostgresStore;

#[async_trait]
impl RequirementsDocumentRepository for PostgresStore {
    async fn document(
        &self,
        project_id: Uuid,
    ) -> Result<Option<RequirementsDocumentDAO>, RepoError> {
        let row: Option<(Uuid, Uuid, i32, String, OffsetDateTime, OffsetDateTime)> =
            sqlx::query_as(
                "select id, project_id, current_version, status, created_at, updated_at \
                 from requirements_documents where project_id = $1",
            )
            .bind(project_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        Ok(row.map(document_from_row))
    }

    async fn ensure_document(
        &self,
        project_id: Uuid,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<(RequirementsDocumentDAO, RequirementsDraftDAO), RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let existing: Option<(Uuid, Uuid, i32, String, OffsetDateTime, OffsetDateTime)> =
            sqlx::query_as(
                "select id, project_id, current_version, status, created_at, updated_at \
                 from requirements_documents where project_id = $1 for update",
            )
            .bind(project_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;

        let document = if let Some(row) = existing {
            document_from_row(row)
        } else {
            let id = Uuid::now_v7();
            sqlx::query(
                "insert into requirements_documents \
                 (id, project_id, current_version, status, created_at, updated_at) \
                 values ($1, $2, 0, 'draft', $3, $3)",
            )
            .bind(id)
            .bind(project_id)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
            RequirementsDocumentDAO {
                id,
                project_id,
                current_version: 0,
                status: "draft".to_string(),
                created_at: now,
                updated_at: now,
            }
        };

        let draft_row: Option<(Uuid, i64, i32, Value, OffsetDateTime)> = sqlx::query_as(
            "select id, revision, base_version, body, updated_at \
             from requirements_document_drafts where document_id = $1 for update",
        )
        .bind(document.id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;

        let draft = if let Some((_draft_id, revision, base_version, body, updated_at)) = draft_row {
            RequirementsDraftDAO {
                document_id: document.id,
                project_id,
                revision: revision as u64,
                base_version: base_version as u32,
                body: serde_json::from_value(body).map_err(backend)?,
                updated_at,
            }
        } else {
            let body =
                serde_json::to_value(RequirementsDocumentBody::default()).map_err(backend)?;
            let draft_id = Uuid::now_v7();
            sqlx::query(
                "insert into requirements_document_drafts \
                 (id, document_id, revision, base_version, body, updated_by_account_id, updated_at) \
                 values ($1, $2, 1, 0, $3, $4, $5)",
            )
            .bind(draft_id)
            .bind(document.id)
            .bind(body)
            .bind(actor_id)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
            RequirementsDraftDAO {
                document_id: document.id,
                project_id,
                revision: 1,
                base_version: 0,
                body: RequirementsDocumentBody::default(),
                updated_at: now,
            }
        };

        tx.commit().await.map_err(backend)?;
        Ok((document, draft))
    }

    async fn draft(&self, project_id: Uuid) -> Result<Option<RequirementsDraftDAO>, RepoError> {
        let row: Option<(Uuid, Uuid, i64, i32, Value, OffsetDateTime)> = sqlx::query_as(
            "select d.id, d.project_id, dr.revision, dr.base_version, dr.body, dr.updated_at \
             from requirements_document_drafts dr \
             join requirements_documents d on d.id = dr.document_id \
             where d.project_id = $1",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        Ok(row.map(
            |(document_id, project_id, revision, base_version, body, updated_at)| {
                RequirementsDraftDAO {
                    document_id,
                    project_id,
                    revision: revision as u64,
                    base_version: base_version as u32,
                    body: serde_json::from_value(body).unwrap_or_default(),
                    updated_at,
                }
            },
        ))
    }

    async fn apply_operations(
        &self,
        project_id: Uuid,
        expected_revision: u64,
        operations: &[RequirementsOperation],
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<RequirementsApplyOutcome, RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let document_id: Option<Uuid> = sqlx::query_scalar(
            "select id from requirements_documents where project_id = $1 for update",
        )
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(document_id) = document_id else {
            return Ok(RequirementsApplyOutcome::NoDraft);
        };
        let draft: Option<(Uuid, i64, i32, Value, OffsetDateTime)> = sqlx::query_as(
            "select id, revision, base_version, body, updated_at \
             from requirements_document_drafts where document_id = $1 for update",
        )
        .bind(document_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some((draft_id, revision, _base_version, body, updated_at)) = draft else {
            return Ok(RequirementsApplyOutcome::NoDraft);
        };
        let mut new_operations = Vec::new();
        for operation in operations {
            let exists: Option<Uuid> = sqlx::query_scalar(
                "select op_id from requirements_operations where draft_id = $1 and op_id = $2",
            )
            .bind(draft_id)
            .bind(operation.op_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
            if exists.is_none() {
                new_operations.push(operation.clone());
            }
        }
        if new_operations.is_empty() {
            tx.commit().await.map_err(backend)?;
            return Ok(RequirementsApplyOutcome::Applied {
                revision: revision as u64,
                updated_at,
            });
        }
        if revision as u64 != expected_revision {
            return Ok(RequirementsApplyOutcome::Conflict {
                current_revision: revision as u64,
                current_updated_at: updated_at,
            });
        }

        let body: RequirementsDocumentBody = serde_json::from_value(body).map_err(backend)?;
        let new_body = foundry_domain::apply_operations(&body, &new_operations)
            .map_err(|error| RepoError::Invalid(error.to_string()))?;
        let new_value = serde_json::to_value(&new_body).map_err(backend)?;
        let bytes = serde_json::to_vec(&new_body).map_err(backend)?;
        if bytes.len() > foundry_domain::MAX_REQUIREMENTS_BYTES {
            return Ok(RequirementsApplyOutcome::TooLarge);
        }
        let new_revision = revision + 1;
        sqlx::query(
            "update requirements_document_drafts \
             set body = $1, revision = $2, updated_by_account_id = $3, updated_at = $4 \
             where id = $5",
        )
        .bind(&new_value)
        .bind(new_revision)
        .bind(actor_id)
        .bind(now)
        .bind(draft_id)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;

        for operation in &new_operations {
            let payload = serde_json::to_value(&operation.op).map_err(backend)?;
            let op_type = payload
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            sqlx::query(
                "insert into requirements_operations \
                 (draft_id, op_id, base_revision, type, target, payload, actor_id, result) \
                 values ($1, $2, $3, $4, '{}'::jsonb, $5, $6, 'applied')",
            )
            .bind(draft_id)
            .bind(operation.op_id)
            .bind(revision)
            .bind(op_type)
            .bind(payload)
            .bind(actor_id)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        }

        tx.commit().await.map_err(backend)?;
        Ok(RequirementsApplyOutcome::Applied {
            revision: new_revision as u64,
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
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let document: Option<(Uuid, i32)> = sqlx::query_as(
            "select id, current_version from requirements_documents where project_id = $1 for update",
        )
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some((document_id, current_version)) = document else {
            return Ok(RequirementsApprovalOutcome::NoDraft);
        };
        let draft: Option<(Uuid, i64, Value, OffsetDateTime)> = sqlx::query_as(
            "select id, revision, body, updated_at \
             from requirements_document_drafts where document_id = $1 for update",
        )
        .bind(document_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some((draft_id, revision, body, updated_at)) = draft else {
            return Ok(RequirementsApprovalOutcome::NoDraft);
        };
        let existing: Option<i32> = sqlx::query_scalar(
            "select version from requirements_document_versions \
             where document_id = $1 and source_revision = $2",
        )
        .bind(document_id)
        .bind(expected_revision as i64)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        if let Some(version) = existing {
            return Ok(RequirementsApprovalOutcome::AlreadyPublished {
                version: version as u32,
            });
        }
        if revision as u64 != expected_revision {
            return Ok(RequirementsApprovalOutcome::Conflict {
                current_revision: revision as u64,
                current_updated_at: updated_at,
            });
        }
        let body: RequirementsDocumentBody = serde_json::from_value(body).map_err(backend)?;
        let validation = foundry_domain::validate_requirements_document(&body);
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
        let bytes = serde_json::to_vec(&body).map_err(backend)?;
        if bytes.len() > foundry_domain::MAX_REQUIREMENTS_BYTES {
            return Ok(RequirementsApprovalOutcome::TooLarge);
        }
        let version = (current_version + 1) as u32;
        let value = serde_json::to_value(&body).map_err(backend)?;
        let hash = content_hash(&bytes);
        let approval_id = Uuid::now_v7();
        sqlx::query(
            "insert into requirements_document_approvals \
             (id, project_id, revision, decision, actor_id, comment, content_hash, created_at) \
             values ($1, $2, $3, 'approved', $4, $5, $6, $7)",
        )
        .bind(approval_id)
        .bind(project_id)
        .bind(expected_revision as i64)
        .bind(actor_id)
        .bind(comment)
        .bind(&hash)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        sqlx::query(
            "insert into requirements_document_versions \
             (id, document_id, version, body, content_hash, source_revision, created_by, \
              approved_by, approval_comment, created_at, published_at) \
             values ($1, $2, $3, $4, $5, $6, $7, $7, $8, $9, $9)",
        )
        .bind(Uuid::now_v7())
        .bind(document_id)
        .bind(version as i32)
        .bind(&value)
        .bind(hash)
        .bind(expected_revision as i64)
        .bind(actor_id)
        .bind(comment)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        sqlx::query(
            "update requirements_documents \
             set current_version = $1, status = 'published', updated_at = $2 where id = $3",
        )
        .bind(version as i32)
        .bind(now)
        .bind(document_id)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        sqlx::query(
            "update requirements_document_drafts \
             set base_version = $1, updated_at = $2 where id = $3",
        )
        .bind(version as i32)
        .bind(now)
        .bind(draft_id)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(RequirementsApprovalOutcome::Approved {
            version,
            revision: expected_revision,
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
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let document_id: Option<Uuid> = sqlx::query_scalar(
            "select id from requirements_documents where project_id = $1 for update",
        )
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(document_id) = document_id else {
            return Ok(RequirementsApprovalOutcome::NoDraft);
        };
        let draft: Option<(i64, Value, OffsetDateTime)> = sqlx::query_as(
            "select revision, body, updated_at from requirements_document_drafts \
             where document_id = $1 for update",
        )
        .bind(document_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some((revision, body, updated_at)) = draft else {
            return Ok(RequirementsApprovalOutcome::NoDraft);
        };
        let existing: Option<i32> = sqlx::query_scalar(
            "select version from requirements_document_versions \
             where document_id = $1 and source_revision = $2",
        )
        .bind(document_id)
        .bind(expected_revision as i64)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        if let Some(version) = existing {
            return Ok(RequirementsApprovalOutcome::AlreadyPublished {
                version: version as u32,
            });
        }
        if revision as u64 != expected_revision {
            return Ok(RequirementsApprovalOutcome::Conflict {
                current_revision: revision as u64,
                current_updated_at: updated_at,
            });
        }
        let body: RequirementsDocumentBody = serde_json::from_value(body).map_err(backend)?;
        let bytes = serde_json::to_vec(&body).map_err(backend)?;
        let existing: Option<Uuid> = sqlx::query_scalar(
            "select id from requirements_document_approvals \
             where project_id = $1 and revision = $2 and decision = 'rejected'",
        )
        .bind(project_id)
        .bind(expected_revision as i64)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        if let Some(approval_id) = existing {
            return Ok(RequirementsApprovalOutcome::Rejected { approval_id });
        }
        let approval_id = Uuid::now_v7();
        sqlx::query(
            "insert into requirements_document_approvals \
             (id, project_id, revision, decision, actor_id, comment, content_hash, created_at) \
             values ($1, $2, $3, 'rejected', $4, $5, $6, $7)",
        )
        .bind(approval_id)
        .bind(project_id)
        .bind(expected_revision as i64)
        .bind(actor_id)
        .bind(comment)
        .bind(content_hash(&bytes))
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(RequirementsApprovalOutcome::Rejected { approval_id })
    }

    async fn approvals(&self, project_id: Uuid) -> Result<Vec<RequirementsApprovalDAO>, RepoError> {
        let rows: Vec<(Uuid, Uuid, i64, String, Uuid, Option<String>, String, OffsetDateTime)> =
            sqlx::query_as(
                "select id, project_id, revision, decision, actor_id, comment, content_hash, created_at \
                 from requirements_document_approvals where project_id = $1 order by created_at, id",
            )
            .bind(project_id)
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;
        rows.into_iter()
            .map(
                |(
                    id,
                    project_id,
                    revision,
                    decision,
                    actor_id,
                    comment,
                    content_hash,
                    created_at,
                )| {
                    let decision = ApprovalDecision::parse(&decision).ok_or_else(|| {
                        RepoError::Backend(format!("unknown approval decision {decision}"))
                    })?;
                    Ok(RequirementsApprovalDAO {
                        id,
                        project_id,
                        revision: revision as u64,
                        decision,
                        actor_id,
                        comment,
                        content_hash,
                        created_at,
                    })
                },
            )
            .collect()
    }

    async fn versions(&self, project_id: Uuid) -> Result<Vec<RequirementsVersionRow>, RepoError> {
        let rows: Vec<(i32, Uuid, Option<Uuid>, Option<i64>, Option<OffsetDateTime>)> =
            sqlx::query_as(
                "select v.version, v.created_by, v.approved_by, v.source_revision, v.published_at \
             from requirements_document_versions v \
             join requirements_documents d on d.id = v.document_id \
             where d.project_id = $1 order by v.version desc",
            )
            .bind(project_id)
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;
        Ok(rows
            .into_iter()
            .map(
                |(version, created_by, approved_by, source_revision, published_at)| {
                    RequirementsVersionRow {
                        version: version as u32,
                        created_by,
                        approved_by,
                        source_revision: source_revision.map(|value| value as u64),
                        published_at,
                    }
                },
            )
            .collect())
    }

    async fn version(
        &self,
        project_id: Uuid,
        version: u32,
    ) -> Result<Option<RequirementsVersionDAO>, RepoError> {
        let row: Option<VersionRow> = sqlx::query_as(
            "select v.document_id, d.project_id, v.version, v.body, v.content_hash, \
                    v.source_revision, v.created_by, v.approved_by, v.approval_comment, \
                    v.created_at, v.published_at \
             from requirements_document_versions v \
             join requirements_documents d on d.id = v.document_id \
             where d.project_id = $1 and v.version = $2",
        )
        .bind(project_id)
        .bind(version as i32)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(version_from_row).transpose()
    }

    async fn latest_version(
        &self,
        project_id: Uuid,
    ) -> Result<Option<RequirementsVersionDAO>, RepoError> {
        let row: Option<VersionRow> = sqlx::query_as(
            "select v.document_id, d.project_id, v.version, v.body, v.content_hash, \
                    v.source_revision, v.created_by, v.approved_by, v.approval_comment, \
                    v.created_at, v.published_at \
             from requirements_document_versions v \
             join requirements_documents d on d.id = v.document_id \
             where d.project_id = $1 order by v.version desc limit 1",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(version_from_row).transpose()
    }

    async fn replace_draft(
        &self,
        project_id: Uuid,
        expected_revision: u64,
        body: &RequirementsDocumentBody,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<RequirementsApplyOutcome, RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let document_id: Option<Uuid> = sqlx::query_scalar(
            "select id from requirements_documents where project_id = $1 for update",
        )
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(document_id) = document_id else {
            return Ok(RequirementsApplyOutcome::NoDraft);
        };
        let draft: Option<(Uuid, i64, OffsetDateTime)> = sqlx::query_as(
            "select id, revision, updated_at from requirements_document_drafts \
             where document_id = $1 for update",
        )
        .bind(document_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some((draft_id, revision, updated_at)) = draft else {
            return Ok(RequirementsApplyOutcome::NoDraft);
        };
        if revision as u64 != expected_revision {
            return Ok(RequirementsApplyOutcome::Conflict {
                current_revision: revision as u64,
                current_updated_at: updated_at,
            });
        }
        let bytes = serde_json::to_vec(body).map_err(backend)?;
        if bytes.len() > foundry_domain::MAX_REQUIREMENTS_BYTES {
            return Ok(RequirementsApplyOutcome::TooLarge);
        }
        let value = serde_json::to_value(body).map_err(backend)?;
        let new_revision = revision + 1;
        sqlx::query(
            "update requirements_document_drafts \
             set body = $1, revision = $2, updated_by_account_id = $3, updated_at = $4 \
             where id = $5",
        )
        .bind(value)
        .bind(new_revision)
        .bind(actor_id)
        .bind(now)
        .bind(draft_id)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(RequirementsApplyOutcome::Applied {
            revision: new_revision as u64,
            updated_at: now,
        })
    }

    async fn annotations(
        &self,
        project_id: Uuid,
        include_resolved: bool,
    ) -> Result<Vec<RequirementsAnnotation>, RepoError> {
        let rows: Vec<AnnotationRow> = sqlx::query_as(
            "select a.id, a.project_id, a.target_type, a.target_id, a.field, a.body, \
                    a.author_id, au.handle, au.display_name, a.status, a.resolution, \
                    a.created_at, a.resolved_at, a.resolved_by \
             from requirements_draft_annotations a \
             join accounts au on au.id = a.author_id \
             where a.project_id = $1 and ($2::bool or a.status = 'open') \
             order by a.created_at, a.id",
        )
        .bind(project_id)
        .bind(include_resolved)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter().map(annotation_from_row).collect()
    }

    async fn annotation(
        &self,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<Option<RequirementsAnnotation>, RepoError> {
        let row: Option<AnnotationRow> = sqlx::query_as(
            "select a.id, a.project_id, a.target_type, a.target_id, a.field, a.body, \
                    a.author_id, au.handle, au.display_name, a.status, a.resolution, \
                    a.created_at, a.resolved_at, a.resolved_by \
             from requirements_draft_annotations a \
             join accounts au on au.id = a.author_id \
             where a.project_id = $1 and a.id = $2",
        )
        .bind(project_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(annotation_from_row).transpose()
    }

    async fn insert_annotation(
        &self,
        annotation: &RequirementsAnnotation,
    ) -> Result<(), RepoError> {
        sqlx::query(
            "insert into requirements_draft_annotations \
             (id, project_id, target_type, target_id, field, body, author_id, status, created_at) \
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(annotation.id)
        .bind(annotation.project_id)
        .bind(annotation.target_type.as_str())
        .bind(&annotation.target_id)
        .bind(&annotation.field)
        .bind(&annotation.body)
        .bind(annotation.author_id)
        .bind(annotation.status.as_str())
        .bind(annotation.created_at)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
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
        let affected = sqlx::query(
            "update requirements_draft_annotations \
             set status = 'resolved', resolution = $3, resolved_at = $4, resolved_by = $5 \
             where project_id = $1 and id = $2 and status = 'open'",
        )
        .bind(project_id)
        .bind(id)
        .bind(resolution)
        .bind(now)
        .bind(resolver_id)
        .execute(&self.pool)
        .await
        .map_err(backend)?
        .rows_affected();
        if affected == 0 {
            return Ok(None);
        }
        self.annotation(project_id, id).await
    }
}

type AnnotationRow = (
    Uuid,
    Uuid,
    String,
    Option<String>,
    Option<String>,
    String,
    Uuid,
    String,
    String,
    String,
    Option<String>,
    OffsetDateTime,
    Option<OffsetDateTime>,
    Option<Uuid>,
);

fn annotation_from_row(row: AnnotationRow) -> Result<RequirementsAnnotation, RepoError> {
    let target_type = AnnotationTarget::parse(&row.2)
        .ok_or_else(|| RepoError::Backend(format!("unknown annotation target {}", row.2)))?;
    Ok(RequirementsAnnotation {
        id: row.0,
        project_id: row.1,
        target_type,
        target_id: row.3,
        field: row.4,
        body: row.5,
        author_id: row.6,
        author_handle: row.7,
        author_name: row.8,
        status: AnnotationStatus::parse(&row.9),
        resolution: row.10,
        created_at: row.11,
        resolved_at: row.12,
        resolved_by: row.13,
    })
}

fn document_from_row(
    row: (Uuid, Uuid, i32, String, OffsetDateTime, OffsetDateTime),
) -> RequirementsDocumentDAO {
    RequirementsDocumentDAO {
        id: row.0,
        project_id: row.1,
        current_version: row.2 as u32,
        status: row.3,
        created_at: row.4,
        updated_at: row.5,
    }
}

type VersionRow = (
    Uuid,
    Uuid,
    i32,
    Value,
    String,
    Option<i64>,
    Uuid,
    Option<Uuid>,
    Option<String>,
    OffsetDateTime,
    Option<OffsetDateTime>,
);

fn version_from_row(row: VersionRow) -> Result<RequirementsVersionDAO, RepoError> {
    Ok(RequirementsVersionDAO {
        document_id: row.0,
        project_id: row.1,
        version: row.2 as u32,
        body: serde_json::from_value(row.3).map_err(backend)?,
        content_hash: row.4,
        source_revision: row.5.unwrap_or(0) as u64,
        created_by: row.6,
        approved_by: row.7,
        approval_comment: row.8,
        created_at: row.9,
        published_at: row.10,
    })
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

fn backend(error: impl std::fmt::Display) -> RepoError {
    RepoError::Backend(error.to_string())
}
