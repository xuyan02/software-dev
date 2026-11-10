//! MCP (Model Context Protocol) endpoint.
//!
//! Implements the JSON-RPC subset needed for tool use over streamable HTTP:
//! `initialize`, `tools/list`, `tools/call` and `ping`. Authentication is the
//! transport layer's job: the HTTP adapter resolves the bearer token to an
//! account and a project, then hands the derived actor and role capabilities to
//! this module. Tool arguments never carry an actor.
//!
//! Tools deliberately mirror the human API. An agent can do exactly what a
//! human with the same role can do — no more.

#![recursion_limit = "512"]

use foundry_app::{
    AddTaskLink, AnnotateProjectBrief, ApplyProjectBriefDraftOperation,
    ApplyRequirementsOperations, ApproveProjectBrief, CommentIssue, CreateIssue,
    CreateProjectBriefDraft, CreateRequirement, IssueFilter, IssueUseCases, ProjectBriefUseCases,
    PublishRequirementsDocument, RemoveTaskLink, RequirementFilter, RequirementUseCases,
    RequirementsDocumentUseCases, ResolveProjectBriefAnnotation, SubmitProjectBriefDraft,
    TaskLinkUseCases, TransitionIssue, TransitionRequirement, WorkflowUseCases,
};
use foundry_domain::{
    AcceptanceScenario, Actor, IssueKind, IssueSeverity, IssueSource, IssueSourceKind, IssueStatus,
    Priority, ProjectBriefAnnotationTarget, ProjectBriefDraftOperation, RequirementStatus,
    RequirementsOperation, RiskClass, TaskLinkKind, TaskLinkRelation, WorkflowId,
};
use serde::Deserialize;
use serde_json::{Value, json};

pub const PROTOCOL_VERSION: &str = "2025-06-18";

pub struct Caller<'a> {
    pub actor: &'a Actor,
    pub project_id: uuid::Uuid,
    pub can_contribute: bool,
    pub can_dispose: bool,
}

/// The use cases exposed over MCP. Passing them as one value keeps the tool
/// dispatcher readable as more domains are added.
pub struct Services<'a> {
    pub requirements: &'a dyn RequirementUseCases,
    pub issues: &'a dyn IssueUseCases,
    pub workflows: &'a dyn WorkflowUseCases,
    pub requirements_doc: &'a dyn RequirementsDocumentUseCases,
    pub project_brief: &'a dyn ProjectBriefUseCases,
    pub task_links: &'a dyn TaskLinkUseCases,
}

pub async fn handle(services: &Services<'_>, caller: &Caller<'_>, request: Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();

    // Notifications have no id and expect no response body.
    if id.is_null() && method.starts_with("notifications/") {
        return Value::Null;
    }

    match method {
        "initialize" => success(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": {
                    "name": "foundry",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "instructions": "Foundry is a software engineering control plane. \
                    This connection is scoped to one project and authenticated as the \
                    account owning the bearer token. Issues are the problem space: create and \
                    discuss them before deciding what becomes a requirement. \
                    Requirements are executable contracts: describe the problem, users and \
                    acceptance scenarios before moving them forward. Agents may not approve \
                    requirements or close issues; ask a human to decide."
            }),
        ),
        "ping" => success(id, json!({})),
        "tools/list" => success(id, json!({ "tools": tool_definitions() })),
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or(Value::Null);
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match call_tool(services, caller, name, arguments).await {
                Ok(value) => success(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": value.to_string() }],
                        "structuredContent": value,
                        "isError": false
                    }),
                ),
                Err(message) => success(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": message }],
                        "isError": true
                    }),
                ),
            }
        }
        _ => error(id, -32601, format!("method not found: {method}")),
    }
}

async fn call_tool(
    services: &Services<'_>,
    caller: &Caller<'_>,
    name: &str,
    arguments: Value,
) -> Result<Value, String> {
    let requirements = services.requirements;
    let issues = services.issues;
    let workflows = services.workflows;
    let requirements_doc = services.requirements_doc;
    let project_brief = services.project_brief;
    let task_links = services.task_links;
    match name {
        "create_requirement" => {
            require_contribute(caller)?;
            let args: CreateArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let requirement = requirements
                .create(
                    caller.actor.clone(),
                    CreateRequirement {
                        project_id: caller.project_id,
                        title: args.title,
                        problem: args.problem,
                        users: args.users,
                        acceptance: args.acceptance,
                        non_goals: args.non_goals,
                        constraints: args.constraints,
                        priority: args.priority,
                        risk: args.risk,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(requirement).map_err(|error| error.to_string())
        }
        "get_requirement" => {
            let args: GetArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let requirement = requirements
                .get(caller.project_id, args.id)
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(requirement).map_err(|error| error.to_string())
        }
        "list_requirements" => {
            let args: ListArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let items = requirements
                .list(
                    caller.project_id,
                    RequirementFilter {
                        status: args.status,
                        owner: args.owner,
                        text: args.text,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(items).map_err(|error| error.to_string())
        }
        "request_transition" => {
            require_dispose(caller)?;
            let args: TransitionArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let requirement = requirements
                .transition(TransitionRequirement {
                    project_id: caller.project_id,
                    id: args.id,
                    to: args.to,
                    actor: caller.actor.clone(),
                    expected_version: args.expected_version,
                    reason: args.reason,
                })
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(requirement).map_err(|error| error.to_string())
        }
        "create_issue" => {
            require_contribute(caller)?;
            let args: CreateIssueArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let issue = issues
                .create(
                    caller.actor.clone(),
                    CreateIssue {
                        project_id: caller.project_id,
                        title: args.title,
                        description: args.description,
                        kind: args.kind,
                        source: args.source,
                        severity: args.severity,
                        area: args.area,
                        labels: args.labels,
                        environment: args.environment,
                        affected_versions: args.affected_versions,
                        reproduction: args.reproduction,
                        expected: args.expected,
                        actual: args.actual,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(issue).map_err(|error| error.to_string())
        }
        "get_issue" => {
            let args: GetArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let issue = issues
                .get(caller.project_id, args.id)
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(issue).map_err(|error| error.to_string())
        }
        "list_issues" => {
            let args: ListIssuesArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let items = issues
                .list(
                    caller.project_id,
                    IssueFilter {
                        status: args.status,
                        kind: args.kind,
                        source: args.source,
                        assignee: args.assignee,
                        text: args.text,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(items).map_err(|error| error.to_string())
        }
        "comment_issue" => {
            require_contribute(caller)?;
            let args: CommentIssueArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let issue = issues
                .comment(CommentIssue {
                    project_id: caller.project_id,
                    id: args.id,
                    actor: caller.actor.clone(),
                    body: args.body,
                    internal: args.internal,
                    expected_version: args.expected_version,
                })
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(issue).map_err(|error| error.to_string())
        }
        "transition_issue" => {
            require_dispose(caller)?;
            let args: IssueTransitionArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let issue = issues
                .transition(TransitionIssue {
                    project_id: caller.project_id,
                    id: args.id,
                    actor: caller.actor.clone(),
                    to: args.to,
                    expected_version: args.expected_version,
                    resolution: args.resolution,
                    duplicate_of: args.duplicate_of,
                })
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(issue).map_err(|error| error.to_string())
        }
        "list_task_links" => {
            let args: TaskIdArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let links = task_links
                .list(caller.actor.clone(), args.task_id)
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(links).map_err(|error| error.to_string())
        }
        "add_task_link" => {
            require_contribute(caller)?;
            let args: AddTaskLinkArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let kind = TaskLinkKind::parse(&args.kind)
                .ok_or_else(|| format!("unknown task link kind {}", args.kind))?;
            let relation = TaskLinkRelation::parse(&args.relation)
                .ok_or_else(|| format!("unknown task link relation {}", args.relation))?;
            let link = task_links
                .add(
                    caller.actor.clone(),
                    AddTaskLink {
                        task_id: args.task_id,
                        kind,
                        target_id: args.target_id,
                        url: args.url,
                        title: args.title,
                        relation,
                        metadata: args.metadata,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(link).map_err(|error| error.to_string())
        }
        "remove_task_link" => {
            require_contribute(caller)?;
            let args: RemoveTaskLinkArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            task_links
                .remove(
                    caller.actor.clone(),
                    RemoveTaskLink {
                        task_id: args.task_id,
                        link_id: args.link_id,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({ "removed": true }))
        }
        "get_project_brief" => {
            let brief = project_brief
                .current(caller.actor.clone(), caller.project_id)
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(brief).map_err(|error| error.to_string())
        }
        "list_project_brief_versions" => {
            let versions = project_brief
                .versions(caller.actor.clone(), caller.project_id)
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(versions).map_err(|error| error.to_string())
        }
        "get_project_brief_version" => {
            let args: BriefVersionArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let version = project_brief
                .version(caller.actor.clone(), caller.project_id, args.version)
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(version).map_err(|error| error.to_string())
        }
        "create_project_brief_draft" => {
            require_contribute(caller)?;
            let draft = project_brief
                .create_draft(
                    caller.actor.clone(),
                    CreateProjectBriefDraft {
                        project_id: caller.project_id,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(draft).map_err(|error| error.to_string())
        }
        "list_project_brief_drafts" => {
            let drafts = project_brief
                .drafts(caller.actor.clone(), caller.project_id)
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(drafts).map_err(|error| error.to_string())
        }
        "get_project_brief_draft" => {
            let args: BriefDraftIdArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let draft = project_brief
                .draft(
                    caller.actor.clone(),
                    caller.project_id,
                    args.draft_id,
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(draft).map_err(|error| error.to_string())
        }
        "apply_project_brief_draft_operation" => {
            require_contribute(caller)?;
            let args: ApplyProjectBriefDraftOperationArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let draft = project_brief
                .apply_operation(
                    caller.actor.clone(),
                    ApplyProjectBriefDraftOperation {
                        project_id: caller.project_id,
                        draft_id: args.draft_id,
                        op_id: args.op_id,
                        base_revision: args.base_revision,
                        operation: args.operation,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(draft).map_err(|error| error.to_string())
        }
        "submit_project_brief_draft" => {
            require_contribute(caller)?;
            let args: SubmitProjectBriefDraftArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let version = project_brief
                .submit_draft(
                    caller.actor.clone(),
                    SubmitProjectBriefDraft {
                        project_id: caller.project_id,
                        draft_id: args.draft_id,
                        expected_revision: args.expected_revision,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(version).map_err(|error| error.to_string())
        }
        "approve_project_brief" => {
            require_dispose(caller)?;
            let args: ApproveBriefArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let version = project_brief
                .approve(
                    caller.actor.clone(),
                    ApproveProjectBrief {
                        project_id: caller.project_id,
                        expected_version: args.expected_version,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(version).map_err(|error| error.to_string())
        }
        "list_project_brief_annotations" => {
            let args: ProjectBriefAnnotationsArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let annotations = project_brief
                .annotations(
                    caller.actor.clone(),
                    caller.project_id,
                    args.include_resolved,
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(annotations).map_err(|error| error.to_string())
        }
        "annotate_project_brief" => {
            require_contribute(caller)?;
            let args: AnnotateProjectBriefArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let annotation = project_brief
                .annotate(
                    caller.actor.clone(),
                    AnnotateProjectBrief {
                        project_id: caller.project_id,
                        target_type: args.target_type,
                        target_id: args.target_id,
                        field: args.field,
                        body: args.body,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(annotation).map_err(|error| error.to_string())
        }
        "resolve_project_brief_annotation" => {
            require_contribute(caller)?;
            let args: ResolveProjectBriefAnnotationArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let annotation = project_brief
                .resolve_annotation(
                    caller.actor.clone(),
                    ResolveProjectBriefAnnotation {
                        project_id: caller.project_id,
                        annotation_id: args.annotation_id,
                        resolution: args.resolution,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(annotation).map_err(|error| error.to_string())
        }
        "get_requirements_document" => {
            let document = requirements_doc
                .get(caller.actor.clone(), caller.project_id)
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(document).map_err(|error| error.to_string())
        }
        "get_requirements_draft" => {
            let draft = requirements_doc
                .draft(caller.actor.clone(), caller.project_id)
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(draft).map_err(|error| error.to_string())
        }
        "apply_requirements_operations" => {
            require_contribute(caller)?;
            let args: ApplyRequirementOpsArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let draft = requirements_doc
                .apply(
                    caller.actor.clone(),
                    ApplyRequirementsOperations {
                        project_id: caller.project_id,
                        expected_revision: args.expected_revision,
                        operations: args.operations,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(draft).map_err(|error| error.to_string())
        }
        "validate_requirements_document" => {
            let validation = requirements_doc
                .validate(caller.actor.clone(), caller.project_id)
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(validation).map_err(|error| error.to_string())
        }
        "publish_requirements_document" => {
            require_dispose(caller)?;
            let args: PublishRequirementDocArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let published = requirements_doc
                .publish(
                    caller.actor.clone(),
                    PublishRequirementsDocument {
                        project_id: caller.project_id,
                        expected_revision: args.expected_revision,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(published).map_err(|error| error.to_string())
        }
        "list_workflows" => {
            let items = workflows
                .list(caller.actor.clone())
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(items).map_err(|error| error.to_string())
        }
        "get_workflow" => {
            let args: GetWorkflowArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let id = WorkflowId::new(args.id);
            let value = match args.version {
                Some(version) => serde_json::to_value(
                    workflows
                        .version(caller.actor.clone(), id, version)
                        .await
                        .map_err(|error| error.to_string())?,
                ),
                None => serde_json::to_value(
                    workflows
                        .get(caller.actor.clone(), id)
                        .await
                        .map_err(|error| error.to_string())?,
                ),
            }
            .map_err(|error| error.to_string())?;
            Ok(value)
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

fn require_contribute(caller: &Caller<'_>) -> Result<(), String> {
    if caller.can_contribute {
        Ok(())
    } else {
        Err("project role does not allow contributions".to_string())
    }
}

fn require_dispose(caller: &Caller<'_>) -> Result<(), String> {
    if caller.can_dispose {
        Ok(())
    } else {
        Err("project role does not allow dispositions".to_string())
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "create_requirement",
            "description": "Create a requirement in draft state. Acceptance scenarios are mandatory. \
                            Users and non-goals are needed before approval.",
            "inputSchema": {
                "type": "object",
                "required": ["title", "problem", "acceptance"],
                "properties": {
                    "title": { "type": "string" },
                    "problem": { "type": "string" },
                    "users": { "type": "array", "items": { "type": "string" } },
                    "acceptance": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["id", "title", "then"],
                            "properties": {
                                "id": { "type": "string" },
                                "title": { "type": "string" },
                                "given": { "type": "array", "items": { "type": "string" } },
                                "when": { "type": "array", "items": { "type": "string" } },
                                "then": { "type": "array", "items": { "type": "string" } }
                            }
                        }
                    },
                    "non_goals": { "type": "array", "items": { "type": "string" } },
                    "constraints": { "type": "array", "items": { "type": "string" } },
                    "priority": { "type": "string", "enum": ["p0", "p1", "p2", "p3"] },
                    "risk": { "type": "string", "enum": ["low", "medium", "high", "critical"] }
                }
            }
        },
        {
            "name": "get_requirement",
            "description": "Fetch one requirement by id in the current project.",
            "inputSchema": {
                "type": "object",
                "required": ["id"],
                "properties": { "id": { "type": "string", "format": "uuid" } }
            }
        },
        {
            "name": "list_requirements",
            "description": "List requirements in the current project, optionally filtered by status, owner or free text.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": { "type": "string" },
                    "owner": { "type": "string" },
                    "text": { "type": "string" }
                }
            }
        },
        {
            "name": "request_transition",
            "description": "Request a requirement state transition. The policy engine decides \
                            whether this actor may perform it; the domain state machine decides \
                            whether the transition exists.",
            "inputSchema": {
                "type": "object",
                "required": ["id", "to"],
                "properties": {
                    "id": { "type": "string", "format": "uuid" },
                    "to": { "type": "string" },
                    "expected_version": { "type": "integer" },
                    "reason": { "type": "string" }
                }
            }
        },
        {
            "name": "create_issue",
            "description": "Record a problem-space issue: feedback, bug, question, idea, task \
                            request or incident. Classify it at creation; bug issues require \
                            a severity.",
            "inputSchema": {
                "type": "object",
                "required": ["title", "description"],
                "properties": {
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "kind": {
                        "type": "string",
                        "enum": ["feedback", "bug", "question", "idea", "task_request", "incident"]
                    },
                    "source": {
                        "type": "object",
                        "properties": {
                            "kind": {
                                "type": "string",
                                "enum": ["internal", "customer", "support", "monitoring", "analytics", "incident", "other"]
                            },
                            "reference": { "type": "string" }
                        }
                    },
                    "severity": { "type": "string", "enum": ["s1", "s2", "s3", "s4"] },
                    "area": { "type": "string" },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "environment": { "type": "string" },
                    "affected_versions": { "type": "array", "items": { "type": "string" } },
                    "reproduction": { "type": "string" },
                    "expected": { "type": "string" },
                    "actual": { "type": "string" }
                }
            }
        },
        {
            "name": "get_issue",
            "description": "Fetch one issue in the current project, including its discussion and status history.",
            "inputSchema": {
                "type": "object",
                "required": ["id"],
                "properties": { "id": { "type": "string", "format": "uuid" } }
            }
        },
        {
            "name": "list_issues",
            "description": "List issues in the current project, optionally filtered by status, kind, source, assignee or free text.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": { "type": "string" },
                    "kind": { "type": "string" },
                    "source": { "type": "string" },
                    "assignee": { "type": "string" },
                    "text": { "type": "string" }
                }
            }
        },
        {
            "name": "comment_issue",
            "description": "Add a comment to an issue. Use internal=true for notes that should \
                            not be shown to external reporters.",
            "inputSchema": {
                "type": "object",
                "required": ["id", "body"],
                "properties": {
                    "id": { "type": "string", "format": "uuid" },
                    "body": { "type": "string" },
                    "internal": { "type": "boolean" },
                    "expected_version": { "type": "integer" }
                }
            }
        },
        {
            "name": "transition_issue",
            "description": "Request an issue state transition. Resolved/Rejected need a \
                            resolution; Duplicate needs duplicate_of. Agents cannot close or \
                            reject issues.",
            "inputSchema": {
                "type": "object",
                "required": ["id", "to"],
                "properties": {
                    "id": { "type": "string", "format": "uuid" },
                    "to": {
                        "type": "string",
                        "enum": ["open", "in_progress", "resolved", "closed", "rejected", "duplicate"]
                    },
                    "resolution": { "type": "string" },
                    "duplicate_of": { "type": "string", "format": "uuid" },
                    "expected_version": { "type": "integer" }
                }
            }
        },
        {
            "name": "list_workflows",
            "description": "List workflow definitions visible to the authenticated account: \
                            platform workflows and the account's own private workflows.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "get_workflow",
            "description": "Fetch one workflow definition, or one immutable published version \
                            when version is given.",
            "inputSchema": {
                "type": "object",
                "required": ["id"],
                "properties": {
                    "id": { "type": "string" },
                    "version": { "type": "integer" }
                }
            }
        },
        {
            "name": "list_task_links",
            "description": "List links attached to a Task: Issues, documents, artifacts or URLs.",
            "inputSchema": {
                "type": "object",
                "required": ["task_id"],
                "properties": { "task_id": { "type": "string", "format": "uuid" } }
            }
        },
        {
            "name": "add_task_link",
            "description": "Attach a link to a Task. Use kind=url with url for external links, \
                            or kind=issue/requirement/design/adr/contract/decision/task/artifact \
                            with target_id for internal links.",
            "inputSchema": {
                "type": "object",
                "required": ["task_id", "kind", "title", "relation"],
                "properties": {
                    "task_id": { "type": "string", "format": "uuid" },
                    "kind": { "type": "string" },
                    "target_id": { "type": "string", "format": "uuid" },
                    "url": { "type": "string" },
                    "title": { "type": "string" },
                    "relation": { "type": "string" },
                    "metadata": { "type": "object" }
                }
            }
        },
        {
            "name": "remove_task_link",
            "description": "Remove a link from a Task.",
            "inputSchema": {
                "type": "object",
                "required": ["task_id", "link_id"],
                "properties": {
                    "task_id": { "type": "string", "format": "uuid" },
                    "link_id": { "type": "string", "format": "uuid" }
                }
            }
        },
        {
            "name": "get_project_brief",
            "description": "Read the current project brief version: background, goals, scope, \
                            constraints, risks, terms, budget and open questions.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "list_project_brief_versions",
            "description": "List all submitted project brief versions, newest first.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "get_project_brief_version",
            "description": "Read one submitted project brief version.",
            "inputSchema": {
                "type": "object",
                "required": ["version"],
                "properties": { "version": { "type": "integer" } }
            }
        },
        {
            "name": "create_project_brief_draft",
            "description": "Create a new server-persisted draft. Each call creates a new draft, \
                            initialized from the current brief version. Drafts are private to \
                            their creator.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "list_project_brief_drafts",
            "description": "List drafts created by the current account, newest first.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "get_project_brief_draft",
            "description": "Read one of the current account's drafts.",
            "inputSchema": {
                "type": "object",
                "required": ["draft_id"],
                "properties": { "draft_id": { "type": "string", "format": "uuid" } }
            }
        },
        {
            "name": "apply_project_brief_draft_operation",
            "description": "Apply exactly one structured operation to a draft. Operation types: \
                            set_problem, set_goals, set_scope, \
                            set_constraints, set_risks, set_terms, set_budget. Use the \
                            revision returned by \
                            the previous call.",
            "inputSchema": {
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
            }
        },
        {
            "name": "submit_project_brief_draft",
            "description": "Submit a draft for review: creates a new brief version, moves the \
                            current pointer and deletes the draft. This does NOT advance the \
                            task; after a successful submit, call task_operation with operation \
                            \"completed\".",
            "inputSchema": {
                "type": "object",
                "required": ["draft_id", "expected_revision"],
                "properties": {
                    "draft_id": { "type": "string", "format": "uuid" },
                    "expected_revision": { "type": "integer" }
                }
            }
        },
        {
            "name": "approve_project_brief",
            "description": "Approve the current project brief version. Only a human \
                            Maintainer/Owner may approve.",
            "inputSchema": {
                "type": "object",
                "required": ["expected_version"],
                "properties": { "expected_version": { "type": "integer" } }
            }
        },
        {
            "name": "list_project_brief_annotations",
            "description": "List review annotations on the project brief. Open annotations are \
                            feedback the clarifier must address or resolve.",
            "inputSchema": {
                "type": "object",
                "properties": { "include_resolved": { "type": "boolean", "default": false } }
            }
        },
        {
            "name": "annotate_project_brief",
            "description": "Add a review annotation to a project brief section, metric, risk or \
                            stakeholder.",
            "inputSchema": {
                "type": "object",
                "required": ["target_type", "body"],
                "properties": {
                    "target_type": {
                        "type": "string",
                        "enum": [
                            "document", "problem", "goals", "scope",
                            "constraints", "risks", "terms", "budget"
                        ]
                    },
                    "target_id": { "type": "string" },
                    "field": { "type": "string" },
                    "body": { "type": "string" }
                }
            }
        },
        {
            "name": "resolve_project_brief_annotation",
            "description": "Mark a project brief annotation resolved after applying the requested \
                            change. Include a short resolution note.",
            "inputSchema": {
                "type": "object",
                "required": ["annotation_id", "resolution"],
                "properties": {
                    "annotation_id": { "type": "string", "format": "uuid" },
                    "resolution": { "type": "string" }
                }
            }
        },
        {
            "name": "get_requirements_document",
            "description": "Read the project's requirements document: users, functions, stories, \
                            operations, interactions, screens and images.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "get_requirements_draft",
            "description": "Open the shared requirements draft and return its revision. \
                            Creates an empty draft when none exists; use the returned revision \
                            for apply_requirements_operations.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "apply_requirements_operations",
            "description": "Apply structured operations to the shared requirements draft. \
                            Operations are idempotent; revision conflicts are rejected.",
            "inputSchema": {
                "type": "object",
                "required": ["expected_revision", "operations"],
                "properties": {
                    "expected_revision": { "type": "integer" },
                    "operations": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["op_id", "type"],
                            "properties": {
                                "op_id": { "type": "string", "format": "uuid" },
                                "type": { "type": "string" }
                            },
                            "additionalProperties": true
                        }
                    }
                }
            }
        },
        {
            "name": "validate_requirements_document",
            "description": "Validate the current requirements draft and return errors and warnings.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "publish_requirements_document",
            "description": "Publish the current requirements draft as an immutable version. \
                            Agents cannot publish; a human Maintainer/Owner must approve.",
            "inputSchema": {
                "type": "object",
                "required": ["expected_revision"],
                "properties": {
                    "expected_revision": { "type": "integer" }
                }
            }
        }
    ])
}

#[derive(Debug, Deserialize)]
struct TaskIdArgs {
    task_id: uuid::Uuid,
}

#[derive(Debug, Deserialize)]
struct AddTaskLinkArgs {
    task_id: uuid::Uuid,
    kind: String,
    #[serde(default)]
    target_id: Option<uuid::Uuid>,
    #[serde(default)]
    url: Option<String>,
    title: String,
    relation: String,
    #[serde(default)]
    metadata: Value,
}

#[derive(Debug, Deserialize)]
struct RemoveTaskLinkArgs {
    task_id: uuid::Uuid,
    link_id: uuid::Uuid,
}

#[derive(Debug, Deserialize)]
struct BriefVersionArgs {
    version: u32,
}

#[derive(Debug, Deserialize)]
struct BriefDraftIdArgs {
    draft_id: uuid::Uuid,
}

#[derive(Debug, Deserialize)]
struct ApplyProjectBriefDraftOperationArgs {
    draft_id: uuid::Uuid,
    op_id: uuid::Uuid,
    base_revision: u64,
    operation: ProjectBriefDraftOperation,
}

#[derive(Debug, Deserialize)]
struct SubmitProjectBriefDraftArgs {
    draft_id: uuid::Uuid,
    expected_revision: u64,
}

#[derive(Debug, Deserialize)]
struct ApproveBriefArgs {
    expected_version: u32,
}

#[derive(Debug, Deserialize)]
struct ProjectBriefAnnotationsArgs {
    #[serde(default)]
    include_resolved: bool,
}

#[derive(Debug, Deserialize)]
struct AnnotateProjectBriefArgs {
    target_type: ProjectBriefAnnotationTarget,
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default)]
    field: Option<String>,
    body: String,
}

#[derive(Debug, Deserialize)]
struct ResolveProjectBriefAnnotationArgs {
    annotation_id: uuid::Uuid,
    resolution: String,
}

#[derive(Debug, Deserialize)]
struct ApplyRequirementOpsArgs {
    expected_revision: u64,
    #[serde(default)]
    operations: Vec<RequirementsOperation>,
}

#[derive(Debug, Deserialize)]
struct PublishRequirementDocArgs {
    expected_revision: u64,
}

#[derive(Debug, Deserialize)]
struct CreateArgs {
    title: String,
    problem: String,
    #[serde(default)]
    users: Vec<String>,
    #[serde(default)]
    acceptance: Vec<AcceptanceScenario>,
    #[serde(default)]
    non_goals: Vec<String>,
    #[serde(default)]
    constraints: Vec<String>,
    #[serde(default)]
    priority: Priority,
    #[serde(default)]
    risk: RiskClass,
}

#[derive(Debug, Deserialize)]
struct GetArgs {
    id: uuid::Uuid,
}

#[derive(Debug, Deserialize)]
struct GetWorkflowArgs {
    id: String,
    #[serde(default)]
    version: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ListArgs {
    #[serde(default)]
    status: Option<RequirementStatus>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TransitionArgs {
    id: uuid::Uuid,
    to: RequirementStatus,
    #[serde(default)]
    expected_version: Option<u32>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateIssueArgs {
    title: String,
    description: String,
    #[serde(default)]
    kind: IssueKind,
    #[serde(default)]
    source: IssueSource,
    #[serde(default)]
    severity: Option<IssueSeverity>,
    #[serde(default)]
    area: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    affected_versions: Vec<String>,
    #[serde(default)]
    reproduction: Option<String>,
    #[serde(default)]
    expected: Option<String>,
    #[serde(default)]
    actual: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListIssuesArgs {
    #[serde(default)]
    status: Option<IssueStatus>,
    #[serde(default)]
    kind: Option<IssueKind>,
    #[serde(default)]
    source: Option<IssueSourceKind>,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CommentIssueArgs {
    id: uuid::Uuid,
    body: String,
    #[serde(default)]
    internal: bool,
    #[serde(default)]
    expected_version: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct IssueTransitionArgs {
    id: uuid::Uuid,
    to: IssueStatus,
    #[serde(default)]
    resolution: Option<String>,
    #[serde(default)]
    duplicate_of: Option<uuid::Uuid>,
    #[serde(default)]
    expected_version: Option<u32>,
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}
