//! Project brief tools backed by Foundry's HTTP API.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

use crate::{FoundryClient, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

fn input_uuid(arguments: &Value, key: &str) -> Result<Uuid, ToolError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing {key}")))?
        .parse()
        .map_err(|_| ToolError::InvalidArguments(format!("invalid {key}")))
}

fn input_u64(arguments: &Value, key: &str) -> Result<u64, ToolError> {
    arguments
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing {key}")))
}

pub struct ProjectBriefGetTool {
    client: Arc<FoundryClient>,
}

impl ProjectBriefGetTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ProjectBriefGetTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "project_brief_get".to_string(),
            description: "Read the current project brief version: background, goals, \
                          non-goals, constraints, risks and terms."
                .to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let brief = self
            .client
            .get_project_brief()
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(brief)
    }
}

pub struct ProjectBriefVersionsListTool {
    client: Arc<FoundryClient>,
}

impl ProjectBriefVersionsListTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ProjectBriefVersionsListTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "project_brief_versions_list".to_string(),
            description: "List submitted project brief versions, newest first.".to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let versions = self
            .client
            .list_project_brief_versions()
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(versions)
    }
}

pub struct ProjectBriefVersionGetTool {
    client: Arc<FoundryClient>,
}

impl ProjectBriefVersionGetTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ProjectBriefVersionGetTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "project_brief_version_get".to_string(),
            description: "Read one submitted project brief version.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["version"],
                "properties": { "version": { "type": "integer" } }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let version = input_u64(&arguments, "version")? as u32;
        let brief = self
            .client
            .get_project_brief_version(version)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(brief)
    }
}

pub struct ProjectBriefDraftsListTool {
    client: Arc<FoundryClient>,
}

impl ProjectBriefDraftsListTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ProjectBriefDraftsListTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "project_brief_drafts_list".to_string(),
            description: "List drafts created by the current account.".to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let drafts = self
            .client
            .list_project_brief_drafts()
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(drafts)
    }
}

pub struct ProjectBriefDraftCreateTool {
    client: Arc<FoundryClient>,
}

impl ProjectBriefDraftCreateTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ProjectBriefDraftCreateTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "project_brief_draft_create".to_string(),
            description: "Create a new server-persisted draft, initialized from the current \
                          brief version. Each call creates a new draft."
                .to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let draft = self
            .client
            .create_project_brief_draft()
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(draft)
    }
}

pub struct ProjectBriefDraftGetTool {
    client: Arc<FoundryClient>,
}

impl ProjectBriefDraftGetTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ProjectBriefDraftGetTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "project_brief_draft_get".to_string(),
            description: "Read one of the current account's drafts.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["draft_id"],
                "properties": { "draft_id": { "type": "string", "format": "uuid" } }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let draft_id = input_uuid(&arguments, "draft_id")?;
        let draft = self
            .client
            .get_project_brief_draft(draft_id)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(draft)
    }
}

pub struct ProjectBriefDraftApplyTool {
    client: Arc<FoundryClient>,
}

impl ProjectBriefDraftApplyTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ProjectBriefDraftApplyTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "project_brief_draft_apply".to_string(),
            description: "Apply exactly one structured operation to a draft. Operation types: \
                          set_problem, set_goals, set_non_goals, \
                          set_constraints, set_risks, set_terms. Use the \
                          revision returned by the previous call."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["draft_id", "op_id", "base_revision", "operation"],
                "properties": {
                    "draft_id": { "type": "string", "format": "uuid" },
                    "op_id": { "type": "string", "format": "uuid" },
                    "base_revision": { "type": "integer" },
                    "operation": {
                        "type": "object",
                        "required": ["type"],
                        "properties": { "type": { "type": "string" } },
                        "additionalProperties": true
                    }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let draft_id = input_uuid(&arguments, "draft_id")?;
        let op_id = input_uuid(&arguments, "op_id")?;
        let base_revision = input_u64(&arguments, "base_revision")?;
        let operation = arguments
            .get("operation")
            .cloned()
            .ok_or_else(|| ToolError::InvalidArguments("missing operation".to_string()))?;
        let draft = self
            .client
            .apply_project_brief_draft_operation(draft_id, op_id, base_revision, operation)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(draft)
    }
}

pub struct ProjectBriefDraftSubmitTool {
    client: Arc<FoundryClient>,
}

impl ProjectBriefDraftSubmitTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ProjectBriefDraftSubmitTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "project_brief_draft_submit".to_string(),
            description: "Submit a draft: creates a new brief version, moves the current pointer \
                          and deletes the draft. This does NOT advance the task; immediately \
                          after a successful submit, call task_allowed_operations and \
                          task_operation with operation \"completed\"."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["draft_id", "expected_revision"],
                "properties": {
                    "draft_id": { "type": "string", "format": "uuid" },
                    "expected_revision": { "type": "integer" }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let draft_id = input_uuid(&arguments, "draft_id")?;
        let expected_revision = input_u64(&arguments, "expected_revision")?;
        let version = self
            .client
            .submit_project_brief_draft(draft_id, expected_revision)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(version)
    }
}

pub struct ProjectBriefAnnotationsListTool {
    client: Arc<FoundryClient>,
}

impl ProjectBriefAnnotationsListTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ProjectBriefAnnotationsListTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "project_brief_annotations_list".to_string(),
            description: "List annotations on the project brief. Open annotations are review \
                          feedback that must be addressed or resolved."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "include_resolved": { "type": "boolean", "default": false }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let include_resolved = arguments
            .get("include_resolved")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let annotations = self
            .client
            .list_brief_annotations(include_resolved)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(annotations)
    }
}

pub struct ProjectBriefAnnotateTool {
    client: Arc<FoundryClient>,
}

impl ProjectBriefAnnotateTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ProjectBriefAnnotateTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "project_brief_annotate".to_string(),
            description: "Add a review annotation to a project brief section or to one item \
                          inside it."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["target_type", "body"],
                "properties": {
                    "target_type": {
                        "type": "string",
                        "enum": [
                            "document", "problem", "goals", "non_goals",
                            "constraints", "risks", "terms"
                        ]
                    },
                    "target_id": { "type": "string", "description": "Item to annotate: goal/non-goal/constraint text, risk id or term name. Omit to target the whole section." },
                    "field": { "type": "string" },
                    "body": { "type": "string" }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let target_type = arguments
            .get("target_type")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing target_type".to_string()))?;
        let body = arguments
            .get("body")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing body".to_string()))?;
        let target_id = arguments.get("target_id").and_then(Value::as_str);
        let field = arguments.get("field").and_then(Value::as_str);
        let annotation = self
            .client
            .annotate_brief(target_type, target_id, field, body)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(annotation)
    }
}

pub struct ProjectBriefResolveAnnotationTool {
    client: Arc<FoundryClient>,
}

impl ProjectBriefResolveAnnotationTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ProjectBriefResolveAnnotationTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "project_brief_resolve_annotation".to_string(),
            description: "Mark a project brief annotation resolved after applying the requested \
                          change. Include a short resolution note."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["annotation_id", "resolution"],
                "properties": {
                    "annotation_id": { "type": "string", "format": "uuid" },
                    "resolution": { "type": "string" }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let annotation_id = input_uuid(&arguments, "annotation_id")?;
        let resolution = arguments
            .get("resolution")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing resolution".to_string()))?;
        let annotation = self
            .client
            .resolve_brief_annotation(annotation_id, resolution)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(annotation)
    }
}

fn pretty(value: Value) -> Result<ToolOutput, ToolError> {
    let content = serde_json::to_string_pretty(&value)
        .map_err(|error| ToolError::Failed(error.to_string()))?;
    Ok(ToolOutput::success(content))
}
