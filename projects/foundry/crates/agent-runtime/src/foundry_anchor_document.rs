//! The single document tool for schema-described anchor documents.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

use crate::{FoundryClient, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

fn required_string(arguments: &Value, key: &str) -> Result<String, ToolError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing {key}")))
}

fn optional_string(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn required_u64(arguments: &Value, key: &str) -> Result<u64, ToolError> {
    arguments
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing {key}")))
}

fn optional_bool(arguments: &Value, key: &str) -> bool {
    arguments.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn pretty(value: Value) -> Result<ToolOutput, ToolError> {
    let content = serde_json::to_string_pretty(&value)
        .map_err(|error| ToolError::Failed(error.to_string()))?;
    Ok(ToolOutput::success(content))
}

pub struct DocumentTool {
    client: Arc<FoundryClient>,
}

impl DocumentTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for DocumentTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "document".to_string(),
            description: "Read, edit, validate, annotate and query an anchor document. \
                          action selects the operation; kind is always required."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["action", "kind"],
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "toc",
                            "draft_open",
                            "draft_create",
                            "content",
                            "add",
                            "update_text",
                            "delete",
                            "move",
                            "validate",
                            "annotations_list",
                            "annotate",
                            "resolve_annotation",
                            "approvals",
                            "versions",
                            "decide"
                        ]
                    },
                    "kind": { "type": "string" },
                    "anchor": { "type": "string" },
                    "expected_revision": { "type": "integer" },
                    "parent": { "type": "string" },
                    "occurrence_key": { "type": "string" },
                    "text": { "type": "string" },
                    "after": { "type": "string" },
                    "cascade": { "type": "boolean", "default": false },
                    "include_resolved": { "type": "boolean", "default": false },
                    "quote": { "type": "string" },
                    "body": { "type": "string" },
                    "annotation_id": { "type": "string" },
                    "resolution": { "type": "string" },
                    "decision": { "type": "string", "enum": ["approved", "rejected"] },
                    "comment": { "type": "string" }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let action = required_string(&arguments, "action")?;
        let kind = required_string(&arguments, "kind")?;
        match action.as_str() {
            "toc" => {
                let anchor = optional_string(&arguments, "anchor");
                let markdown = self
                    .client
                    .get_anchor_document_toc(&kind, anchor.as_deref())
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?;
                Ok(ToolOutput::success(markdown))
            }
            "draft_open" => pretty(
                self.client
                    .open_anchor_document_draft(&kind)
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?,
            ),
            "draft_create" => pretty(
                self.client
                    .create_anchor_document_draft(&kind)
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?,
            ),
            "content" => {
                let anchor = optional_string(&arguments, "anchor");
                pretty(
                    self.client
                        .get_anchor_document_content(&kind, anchor.as_deref())
                        .await
                        .map_err(|error| ToolError::Failed(error.to_string()))?,
                )
            }
            "add" => {
                let expected_revision = required_u64(&arguments, "expected_revision")?;
                let op = json!({
                    "op_id": Uuid::now_v7().to_string(),
                    "type": "add",
                    "parent": required_string(&arguments, "parent")?,
                    "occurrence_key": optional_string(&arguments, "occurrence_key"),
                    "text": optional_string(&arguments, "text").unwrap_or_default(),
                    "after": optional_string(&arguments, "after"),
                });
                self.apply(&kind, expected_revision, op).await
            }
            "update_text" => {
                let expected_revision = required_u64(&arguments, "expected_revision")?;
                let op = json!({
                    "op_id": Uuid::now_v7().to_string(),
                    "type": "update_text",
                    "anchor": required_string(&arguments, "anchor")?,
                    "text": required_string(&arguments, "text")?,
                });
                self.apply(&kind, expected_revision, op).await
            }
            "delete" => {
                let expected_revision = required_u64(&arguments, "expected_revision")?;
                let op = json!({
                    "op_id": Uuid::now_v7().to_string(),
                    "type": "delete",
                    "anchor": required_string(&arguments, "anchor")?,
                    "cascade": optional_bool(&arguments, "cascade"),
                });
                self.apply(&kind, expected_revision, op).await
            }
            "move" => {
                let expected_revision = required_u64(&arguments, "expected_revision")?;
                let op = json!({
                    "op_id": Uuid::now_v7().to_string(),
                    "type": "move",
                    "anchor": required_string(&arguments, "anchor")?,
                    "after": optional_string(&arguments, "after"),
                });
                self.apply(&kind, expected_revision, op).await
            }
            "validate" => pretty(
                self.client
                    .validate_anchor_document(&kind)
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?,
            ),
            "annotations_list" => pretty(
                self.client
                    .list_anchor_annotations(&kind, optional_bool(&arguments, "include_resolved"))
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?,
            ),
            "annotate" => pretty(
                self.client
                    .annotate_anchor_document(
                        &kind,
                        json!({
                            "anchor": required_string(&arguments, "anchor")?,
                            "quote": optional_string(&arguments, "quote"),
                            "body": required_string(&arguments, "body")?,
                        }),
                    )
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?,
            ),
            "resolve_annotation" => pretty(
                self.client
                    .resolve_anchor_annotation(
                        &kind,
                        &required_string(&arguments, "annotation_id")?,
                        &required_string(&arguments, "resolution")?,
                    )
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?,
            ),
            "approvals" => pretty(
                self.client
                    .list_anchor_approvals(&kind)
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?,
            ),
            "versions" => pretty(
                self.client
                    .list_anchor_versions(&kind)
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?,
            ),
            "decide" => {
                let expected_revision = required_u64(&arguments, "expected_revision")?;
                let decision = required_string(&arguments, "decision")?;
                let comment = optional_string(&arguments, "comment");
                pretty(
                    self.client
                        .decide_anchor_document(
                            &kind,
                            expected_revision,
                            &decision,
                            comment.as_deref(),
                        )
                        .await
                        .map_err(|error| ToolError::Failed(error.to_string()))?,
                )
            }
            other => Err(ToolError::InvalidArguments(format!(
                "unknown document action: {other}"
            ))),
        }
    }
}

impl DocumentTool {
    async fn apply(
        &self,
        kind: &str,
        expected_revision: u64,
        op: Value,
    ) -> Result<ToolOutput, ToolError> {
        pretty(
            self.client
                .apply_anchor_document_operations(kind, expected_revision, Value::Array(vec![op]))
                .await
                .map_err(|error| ToolError::Failed(error.to_string()))?,
        )
    }
}
