//! Postgres storage for schema-described anchor documents.

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
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{PostgresStore, backend};

type VersionRow = (
    Uuid,
    Uuid,
    String,
    i32,
    Value,
    String,
    i64,
    Uuid,
    Option<Uuid>,
    Option<String>,
    OffsetDateTime,
    Option<OffsetDateTime>,
);

type ApprovalRow = (
    Uuid,
    Uuid,
    String,
    i64,
    String,
    Uuid,
    Option<String>,
    String,
    OffsetDateTime,
);

type AnnotationRow = (
    Uuid,
    Uuid,
    String,
    String,
    String,
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

fn document_from_row(
    row: (
        Uuid,
        Uuid,
        String,
        i32,
        i32,
        String,
        OffsetDateTime,
        OffsetDateTime,
    ),
) -> AnchorDocumentDAO {
    AnchorDocumentDAO {
        id: row.0,
        project_id: row.1,
        kind: row.2,
        schema_version: row.3 as u32,
        current_version: row.4 as u32,
        status: row.5,
        created_at: row.6,
        updated_at: row.7,
    }
}

fn draft_from_row(
    row: (Uuid, Uuid, String, i64, i32, i32, Value, OffsetDateTime),
) -> Result<AnchorDocumentDraftDAO, RepoError> {
    Ok(AnchorDocumentDraftDAO {
        document_id: row.0,
        project_id: row.1,
        kind: row.2,
        revision: row.3 as u64,
        base_version: row.4 as u32,
        schema_version: row.5 as u32,
        snapshot: serde_json::from_value(row.6).map_err(json_backend)?,
        updated_at: row.7,
    })
}

fn version_from_row(row: VersionRow) -> Result<AnchorDocumentVersionDAO, RepoError> {
    Ok(AnchorDocumentVersionDAO {
        document_id: row.0,
        project_id: row.1,
        kind: row.2,
        version: row.3 as u32,
        snapshot: serde_json::from_value(row.4).map_err(json_backend)?,
        content_hash: row.5,
        source_revision: row.6 as u64,
        created_by: row.7,
        approved_by: row.8,
        approval_comment: row.9,
        created_at: row.10,
        published_at: row.11,
    })
}

fn approval_from_row(row: ApprovalRow) -> Result<AnchorDocumentApprovalDAO, RepoError> {
    Ok(AnchorDocumentApprovalDAO {
        id: row.0,
        project_id: row.1,
        kind: row.2,
        revision: row.3 as u64,
        decision: ApprovalDecision::parse(&row.4)
            .ok_or_else(|| RepoError::Backend(format!("unknown approval decision {}", row.4)))?,
        actor_id: row.5,
        comment: row.6,
        content_hash: row.7,
        created_at: row.8,
    })
}

fn annotation_from_row(row: AnnotationRow) -> AnchorAnnotation {
    AnchorAnnotation {
        id: row.0,
        project_id: row.1,
        kind: row.2,
        anchor_key: row.3,
        anchor_id: row.4,
        quote: row.5,
        body: row.6,
        author_id: row.7,
        author_handle: row.8,
        author_name: row.9,
        status: if row.10 == "resolved" {
            AnchorAnnotationStatus::Resolved
        } else {
            AnchorAnnotationStatus::Open
        },
        resolution: row.11,
        created_at: row.12,
        resolved_at: row.13,
        resolved_by: row.14,
    }
}

fn json_backend(error: serde_json::Error) -> RepoError {
    RepoError::Backend(error.to_string())
}

fn check_size(snapshot: &DocumentSnapshot) -> Result<bool, RepoError> {
    Ok(serde_json::to_vec(snapshot).map_err(json_backend)?.len() > MAX_ANCHOR_DOCUMENT_BYTES)
}

#[async_trait]
impl AnchorDocumentRepository for PostgresStore {
    async fn document(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Option<AnchorDocumentDAO>, RepoError> {
        let row: Option<(
            Uuid,
            Uuid,
            String,
            i32,
            i32,
            String,
            OffsetDateTime,
            OffsetDateTime,
        )> = sqlx::query_as(
            "select id, project_id, kind, schema_version, current_version, status, \
                        created_at, updated_at \
                 from anchor_documents where project_id = $1 and kind = $2",
        )
        .bind(project_id)
        .bind(kind)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        Ok(row.map(document_from_row))
    }

    async fn ensure_document(
        &self,
        project_id: Uuid,
        kind: &str,
        schema: &DocumentSchema,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<(AnchorDocumentDAO, AnchorDocumentDraftDAO), RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let existing: Option<(
            Uuid,
            Uuid,
            String,
            i32,
            i32,
            String,
            OffsetDateTime,
            OffsetDateTime,
        )> = sqlx::query_as(
            "select id, project_id, kind, schema_version, current_version, status, \
                        created_at, updated_at \
                 from anchor_documents where project_id = $1 and kind = $2 for update",
        )
        .bind(project_id)
        .bind(kind)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let document = if let Some(row) = existing {
            document_from_row(row)
        } else {
            let id = Uuid::now_v7();
            sqlx::query(
                "insert into anchor_documents \
                 (id, project_id, kind, schema_version, current_version, status, created_at, updated_at) \
                 values ($1, $2, $3, $4, 0, 'draft', $5, $5)",
            )
            .bind(id)
            .bind(project_id)
            .bind(kind)
            .bind(schema.schema_version as i32)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
            AnchorDocumentDAO {
                id,
                project_id,
                kind: kind.to_string(),
                schema_version: schema.schema_version,
                current_version: 0,
                status: "draft".to_string(),
                created_at: now,
                updated_at: now,
            }
        };
        let draft_row: Option<(Uuid, Uuid, String, i64, i32, i32, Value, OffsetDateTime)> =
            sqlx::query_as(
                "select d.document_id, doc.project_id, doc.kind, d.revision, d.base_version, \
                        d.schema_version, d.snapshot, d.updated_at \
                 from anchor_document_drafts d \
                 join anchor_documents doc on doc.id = d.document_id \
                 where d.document_id = $1 for update",
            )
            .bind(document.id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
        let draft = if let Some(row) = draft_row {
            draft_from_row(row)?
        } else {
            let snapshot = DocumentSnapshot::empty(schema);
            let value = serde_json::to_value(&snapshot).map_err(json_backend)?;
            let draft_id = Uuid::now_v7();
            sqlx::query(
                "insert into anchor_document_drafts \
                 (id, document_id, revision, base_version, schema_version, root_anchor_id, \
                  snapshot, updated_by_account_id, updated_at) \
                 values ($1, $2, 1, 0, $3, $4, $5, $6, $7)",
            )
            .bind(draft_id)
            .bind(document.id)
            .bind(schema.schema_version as i32)
            .bind(snapshot.root_id())
            .bind(value)
            .bind(actor_id)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
            AnchorDocumentDraftDAO {
                document_id: document.id,
                project_id,
                kind: kind.to_string(),
                revision: 1,
                base_version: 0,
                schema_version: schema.schema_version,
                snapshot,
                updated_at: now,
            }
        };
        tx.commit().await.map_err(backend)?;
        Ok((document, draft))
    }

    async fn draft(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Option<AnchorDocumentDraftDAO>, RepoError> {
        let row: Option<(Uuid, Uuid, String, i64, i32, i32, Value, OffsetDateTime)> =
            sqlx::query_as(
                "select d.document_id, doc.project_id, doc.kind, d.revision, d.base_version, \
                        d.schema_version, d.snapshot, d.updated_at \
                 from anchor_document_drafts d \
                 join anchor_documents doc on doc.id = d.document_id \
                 where doc.project_id = $1 and doc.kind = $2",
            )
            .bind(project_id)
            .bind(kind)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        row.map(draft_from_row).transpose()
    }

    async fn apply_operations(
        &self,
        project_id: Uuid,
        kind: &str,
        expected_revision: u64,
        operations: &[AnchorOperation],
        actor_id: Uuid,
        now: OffsetDateTime,
        schema: &DocumentSchema,
    ) -> Result<AnchorApplyOutcome, RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let document_id: Option<Uuid> = sqlx::query_scalar(
            "select id from anchor_documents where project_id = $1 and kind = $2 for update",
        )
        .bind(project_id)
        .bind(kind)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(document_id) = document_id else {
            return Ok(AnchorApplyOutcome::NoDraft);
        };
        let draft: Option<(Uuid, i64, i32, i32, Value, OffsetDateTime)> = sqlx::query_as(
            "select id, revision, base_version, schema_version, snapshot, updated_at \
             from anchor_document_drafts where document_id = $1 for update",
        )
        .bind(document_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some((draft_id, revision, base_version, schema_version, snapshot_value, updated_at)) =
            draft
        else {
            return Ok(AnchorApplyOutcome::NoDraft);
        };
        let mut new_operations = Vec::new();
        for operation in operations {
            let exists: Option<Uuid> = sqlx::query_scalar(
                "select op_id from anchor_document_operations where draft_id = $1 and op_id = $2",
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
            let snapshot: DocumentSnapshot =
                serde_json::from_value(snapshot_value).map_err(json_backend)?;
            return Ok(AnchorApplyOutcome::Applied {
                revision: revision as u64,
                root_id: snapshot.root_id().to_string(),
                created_key: None,
                updated_at,
            });
        }
        if revision as u64 != expected_revision {
            return Ok(AnchorApplyOutcome::Conflict {
                current_revision: revision as u64,
                current_updated_at: updated_at,
            });
        }
        let mut snapshot: DocumentSnapshot =
            serde_json::from_value(snapshot_value).map_err(json_backend)?;
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
        if check_size(&snapshot)? {
            return Ok(AnchorApplyOutcome::TooLarge);
        }
        let new_revision = revision + 1;
        let snapshot_value = serde_json::to_value(&snapshot).map_err(json_backend)?;
        sqlx::query(
            "update anchor_document_drafts \
             set snapshot = $1, root_anchor_id = $2, revision = $3, \
                 updated_by_account_id = $4, updated_at = $5 \
             where id = $6",
        )
        .bind(&snapshot_value)
        .bind(snapshot.root_id())
        .bind(new_revision)
        .bind(actor_id)
        .bind(now)
        .bind(draft_id)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        for operation in &new_operations {
            let payload = serde_json::to_value(&operation.op).map_err(json_backend)?;
            let op_type = payload
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            sqlx::query(
                "insert into anchor_document_operations \
                 (draft_id, op_id, base_revision, type, payload, actor_id, applied_at) \
                 values ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(draft_id)
            .bind(operation.op_id)
            .bind(revision)
            .bind(op_type)
            .bind(payload)
            .bind(actor_id)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        }
        tx.commit().await.map_err(backend)?;
        let _ = (base_version, schema_version);
        Ok(AnchorApplyOutcome::Applied {
            revision: new_revision as u64,
            root_id: snapshot.root_id().to_string(),
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
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let document: Option<(Uuid, i32)> = sqlx::query_as(
            "select id, current_version from anchor_documents \
             where project_id = $1 and kind = $2 for update",
        )
        .bind(project_id)
        .bind(kind)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some((document_id, current_version)) = document else {
            return Ok(AnchorApprovalOutcome::NoDraft);
        };
        let draft: Option<(Uuid, i64, Value, OffsetDateTime)> = sqlx::query_as(
            "select id, revision, snapshot, updated_at \
             from anchor_document_drafts where document_id = $1 for update",
        )
        .bind(document_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some((draft_id, revision, snapshot_value, updated_at)) = draft else {
            return Ok(AnchorApprovalOutcome::NoDraft);
        };
        let existing: Option<i32> = sqlx::query_scalar(
            "select version from anchor_document_versions \
             where document_id = $1 and source_revision = $2",
        )
        .bind(document_id)
        .bind(expected_revision as i64)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        if let Some(version) = existing {
            return Ok(AnchorApprovalOutcome::AlreadyPublished {
                version: version as u32,
            });
        }
        if revision as u64 != expected_revision {
            return Ok(AnchorApprovalOutcome::Conflict {
                current_revision: revision as u64,
                current_updated_at: updated_at,
            });
        }
        let snapshot: DocumentSnapshot =
            serde_json::from_value(snapshot_value).map_err(json_backend)?;
        let validation = snapshot.validate(schema);
        if !validation.is_valid() {
            return Ok(AnchorApprovalOutcome::Invalid(validation));
        }
        if check_size(&snapshot)? {
            return Ok(AnchorApprovalOutcome::TooLarge);
        }
        let version = (current_version + 1) as u32;
        let snapshot_value = serde_json::to_value(&snapshot).map_err(json_backend)?;
        let content_hash = snapshot.root_id().to_string();
        let approval_id = Uuid::now_v7();
        sqlx::query(
            "insert into anchor_document_approvals \
             (id, document_id, project_id, kind, revision, decision, actor_id, comment, \
              content_hash, created_at) \
             values ($1, $2, $3, $4, $5, 'approved', $6, $7, $8, $9)",
        )
        .bind(approval_id)
        .bind(document_id)
        .bind(project_id)
        .bind(kind)
        .bind(expected_revision as i64)
        .bind(actor_id)
        .bind(comment)
        .bind(&content_hash)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        sqlx::query(
            "insert into anchor_document_versions \
             (id, document_id, version, snapshot, root_anchor_id, content_hash, \
              source_revision, created_by, approved_by, approval_comment, created_at, published_at) \
             values ($1, $2, $3, $4, $5, $6, $7, $8, $8, $9, $10, $10)",
        )
        .bind(Uuid::now_v7())
        .bind(document_id)
        .bind(version as i32)
        .bind(&snapshot_value)
        .bind(snapshot.root_id())
        .bind(&content_hash)
        .bind(expected_revision as i64)
        .bind(actor_id)
        .bind(comment)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        sqlx::query(
            "update anchor_documents \
             set current_version = $1, status = 'published', updated_at = $2 where id = $3",
        )
        .bind(version as i32)
        .bind(now)
        .bind(document_id)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        sqlx::query(
            "update anchor_document_drafts set base_version = $1, updated_at = $2 where id = $3",
        )
        .bind(version as i32)
        .bind(now)
        .bind(draft_id)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
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
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let document_id: Option<Uuid> = sqlx::query_scalar(
            "select id from anchor_documents where project_id = $1 and kind = $2 for update",
        )
        .bind(project_id)
        .bind(kind)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(document_id) = document_id else {
            return Ok(AnchorApprovalOutcome::NoDraft);
        };
        let draft: Option<(i64, Value, OffsetDateTime)> = sqlx::query_as(
            "select revision, snapshot, updated_at \
             from anchor_document_drafts where document_id = $1 for update",
        )
        .bind(document_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some((revision, snapshot_value, updated_at)) = draft else {
            return Ok(AnchorApprovalOutcome::NoDraft);
        };
        let published: Option<i32> = sqlx::query_scalar(
            "select version from anchor_document_versions \
             where document_id = $1 and source_revision = $2",
        )
        .bind(document_id)
        .bind(expected_revision as i64)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        if let Some(version) = published {
            return Ok(AnchorApprovalOutcome::AlreadyPublished {
                version: version as u32,
            });
        }
        if revision as u64 != expected_revision {
            return Ok(AnchorApprovalOutcome::Conflict {
                current_revision: revision as u64,
                current_updated_at: updated_at,
            });
        }
        let snapshot: DocumentSnapshot =
            serde_json::from_value(snapshot_value).map_err(json_backend)?;
        let existing: Option<Uuid> = sqlx::query_scalar(
            "select id from anchor_document_approvals \
             where project_id = $1 and kind = $2 and revision = $3 and decision = 'rejected'",
        )
        .bind(project_id)
        .bind(kind)
        .bind(expected_revision as i64)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        if let Some(approval_id) = existing {
            return Ok(AnchorApprovalOutcome::Rejected { approval_id });
        }
        let approval_id = Uuid::now_v7();
        sqlx::query(
            "insert into anchor_document_approvals \
             (id, document_id, project_id, kind, revision, decision, actor_id, comment, \
              content_hash, created_at) \
             values ($1, $2, $3, $4, $5, 'rejected', $6, $7, $8, $9)",
        )
        .bind(approval_id)
        .bind(document_id)
        .bind(project_id)
        .bind(kind)
        .bind(expected_revision as i64)
        .bind(actor_id)
        .bind(comment)
        .bind(snapshot.root_id())
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(AnchorApprovalOutcome::Rejected { approval_id })
    }

    async fn approvals(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Vec<AnchorDocumentApprovalDAO>, RepoError> {
        let rows: Vec<ApprovalRow> = sqlx::query_as(
            "select id, project_id, kind, revision, decision, actor_id, comment, \
                    content_hash, created_at \
             from anchor_document_approvals \
             where project_id = $1 and kind = $2 order by created_at, id",
        )
        .bind(project_id)
        .bind(kind)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter().map(approval_from_row).collect()
    }

    async fn versions(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Vec<AnchorDocumentVersionRow>, RepoError> {
        let rows: Vec<(i32, Uuid, Option<Uuid>, i64, Option<OffsetDateTime>)> = sqlx::query_as(
            "select v.version, v.created_by, v.approved_by, v.source_revision, v.published_at \
             from anchor_document_versions v \
             join anchor_documents d on d.id = v.document_id \
             where d.project_id = $1 and d.kind = $2 order by v.version desc",
        )
        .bind(project_id)
        .bind(kind)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        Ok(rows
            .into_iter()
            .map(
                |(version, created_by, approved_by, source_revision, published_at)| {
                    AnchorDocumentVersionRow {
                        version: version as u32,
                        created_by,
                        approved_by,
                        source_revision: (source_revision > 0).then_some(source_revision as u64),
                        published_at,
                    }
                },
            )
            .collect())
    }

    async fn version(
        &self,
        project_id: Uuid,
        kind: &str,
        version: u32,
    ) -> Result<Option<AnchorDocumentVersionDAO>, RepoError> {
        let row: Option<VersionRow> = sqlx::query_as(
            "select v.document_id, d.project_id, d.kind, v.version, v.snapshot, \
                    v.content_hash, v.source_revision, v.created_by, v.approved_by, \
                    v.approval_comment, v.created_at, v.published_at \
             from anchor_document_versions v \
             join anchor_documents d on d.id = v.document_id \
             where d.project_id = $1 and d.kind = $2 and v.version = $3",
        )
        .bind(project_id)
        .bind(kind)
        .bind(version as i32)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(version_from_row).transpose()
    }

    async fn latest_version(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Option<AnchorDocumentVersionDAO>, RepoError> {
        let row: Option<VersionRow> = sqlx::query_as(
            "select v.document_id, d.project_id, d.kind, v.version, v.snapshot, \
                    v.content_hash, v.source_revision, v.created_by, v.approved_by, \
                    v.approval_comment, v.created_at, v.published_at \
             from anchor_document_versions v \
             join anchor_documents d on d.id = v.document_id \
             where d.project_id = $1 and d.kind = $2 order by v.version desc limit 1",
        )
        .bind(project_id)
        .bind(kind)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(version_from_row).transpose()
    }

    async fn annotations(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Vec<AnchorAnnotation>, RepoError> {
        let rows: Vec<AnnotationRow> = sqlx::query_as(
            "select id, project_id, kind, anchor_key, anchor_id, quote, body, \
                    author_id, author_handle, author_name, status, resolution, \
                    created_at, resolved_at, resolved_by \
             from anchor_document_annotations \
             where project_id = $1 and kind = $2 order by created_at, id",
        )
        .bind(project_id)
        .bind(kind)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        Ok(rows.into_iter().map(annotation_from_row).collect())
    }

    async fn insert_annotation(
        &self,
        annotation: AnchorAnnotation,
    ) -> Result<AnchorAnnotation, RepoError> {
        let row: AnnotationRow = sqlx::query_as(
            "insert into anchor_document_annotations \
             (id, project_id, kind, anchor_key, anchor_id, quote, body, \
              author_id, author_handle, author_name, status, resolution, \
              created_at, resolved_at, resolved_by) \
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15) \
             returning id, project_id, kind, anchor_key, anchor_id, quote, body, \
                       author_id, author_handle, author_name, status, resolution, \
                       created_at, resolved_at, resolved_by",
        )
        .bind(annotation.id)
        .bind(annotation.project_id)
        .bind(&annotation.kind)
        .bind(&annotation.anchor_key)
        .bind(&annotation.anchor_id)
        .bind(&annotation.quote)
        .bind(&annotation.body)
        .bind(annotation.author_id)
        .bind(&annotation.author_handle)
        .bind(&annotation.author_name)
        .bind(match annotation.status {
            AnchorAnnotationStatus::Resolved => "resolved",
            _ => "open",
        })
        .bind(&annotation.resolution)
        .bind(annotation.created_at)
        .bind(annotation.resolved_at)
        .bind(annotation.resolved_by)
        .fetch_one(&self.pool)
        .await
        .map_err(backend)?;
        Ok(annotation_from_row(row))
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
        let row: Option<AnnotationRow> = sqlx::query_as(
            "update anchor_document_annotations \
             set status = 'resolved', resolution = $1, resolved_at = $2, resolved_by = $3 \
             where id = $4 and project_id = $5 and kind = $6 \
             returning id, project_id, kind, anchor_key, anchor_id, quote, body, \
                       author_id, author_handle, author_name, status, resolution, \
                       created_at, resolved_at, resolved_by",
        )
        .bind(resolution)
        .bind(now)
        .bind(actor_id)
        .bind(annotation_id)
        .bind(project_id)
        .bind(kind)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        Ok(row.map(annotation_from_row))
    }
}
