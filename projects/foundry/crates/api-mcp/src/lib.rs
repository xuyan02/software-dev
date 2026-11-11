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
    AddAnchorAnnotation, AddTaskLink, AnchorDocumentUseCases, AnchorRef, ApplyAnchorOperations,
    CommentIssue, CreateIssue, DecideAnchorDocument, IssueFilter, IssueUseCases, RemoveTaskLink,
    RequirementsDocumentUseCases, ResolveAnchorAnnotation, TaskLinkUseCases, TransitionIssue,
    WorkflowUseCases,
};
use foundry_domain::{
    Actor, AnchorOp, AnchorOperation, ApprovalDecision, IssueKind, IssueSeverity, IssueSource,
    IssueSourceKind, IssueStatus, TaskLinkKind, TaskLinkRelation, WorkflowId,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

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
    pub issues: &'a dyn IssueUseCases,
    pub workflows: &'a dyn WorkflowUseCases,
    pub anchor_docs: &'a dyn AnchorDocumentUseCases,
    pub requirements_doc: &'a dyn RequirementsDocumentUseCases,
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
            if name == "document" {
                return match document_tool(services, caller, arguments).await {
                    Ok(reply) => {
                        let mut result = json!({
                            "content": [{ "type": "text", "text": reply.text }],
                            "isError": false
                        });
                        if let Some(structured) = reply.structured {
                            result["structuredContent"] = structured;
                        }
                        success(id, result)
                    }
                    Err(message) => success(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": message }],
                            "isError": true
                        }),
                    ),
                };
            }
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

async fn document_tool(
    services: &Services<'_>,
    caller: &Caller<'_>,
    arguments: Value,
) -> Result<DocumentReply, String> {
    let args: DocumentArgs =
        serde_json::from_value(arguments).map_err(|error| error.to_string())?;
    let kind = args.kind.as_str();
    match args.action.as_str() {
        "toc" => {
            let anchor = AnchorRef::parse(args.anchor.as_deref().unwrap_or("document"));
            let view = services
                .anchor_docs
                .get(caller.actor.clone(), caller.project_id, kind, &anchor)
                .await
                .map_err(|error| error.to_string())?;
            Ok(DocumentReply {
                text: foundry_app::render_toc_markdown(&view),
                structured: None,
            })
        }
        "draft_open" => json_reply(
            services
                .anchor_docs
                .open_draft(caller.actor.clone(), caller.project_id, kind)
                .await
                .map_err(|error| error.to_string())?,
        ),
        "draft_create" => {
            require_contribute(caller)?;
            json_reply(
                services
                    .anchor_docs
                    .create_draft(caller.actor.clone(), caller.project_id, kind)
                    .await
                    .map_err(|error| error.to_string())?,
            )
        }
        "content" => {
            let anchor = AnchorRef::parse(args.anchor.as_deref().unwrap_or("document"));
            json_reply(
                services
                    .anchor_docs
                    .content(caller.actor.clone(), caller.project_id, kind, &anchor)
                    .await
                    .map_err(|error| error.to_string())?,
            )
        }
        "add" => {
            require_contribute(caller)?;
            let expected_revision = required_revision(&args)?;
            let op = AnchorOp::Add {
                parent: required(args.parent, "parent")?,
                occurrence_key: args.occurrence_key,
                text: args.text.unwrap_or_default(),
                after: args.after,
            };
            apply_document_op(services, caller, kind, expected_revision, op).await
        }
        "update_text" => {
            require_contribute(caller)?;
            let expected_revision = required_revision(&args)?;
            let op = AnchorOp::UpdateText {
                anchor: required(args.anchor, "anchor")?,
                text: required(args.text, "text")?,
            };
            apply_document_op(services, caller, kind, expected_revision, op).await
        }
        "delete" => {
            require_contribute(caller)?;
            let expected_revision = required_revision(&args)?;
            let op = AnchorOp::Delete {
                anchor: required(args.anchor, "anchor")?,
                cascade: args.cascade,
            };
            apply_document_op(services, caller, kind, expected_revision, op).await
        }
        "move" => {
            require_contribute(caller)?;
            let expected_revision = required_revision(&args)?;
            let op = AnchorOp::Move {
                anchor: required(args.anchor, "anchor")?,
                after: args.after,
            };
            apply_document_op(services, caller, kind, expected_revision, op).await
        }
        "validate" => json_reply(
            services
                .anchor_docs
                .validate(caller.actor.clone(), caller.project_id, kind)
                .await
                .map_err(|error| error.to_string())?,
        ),
        "annotations_list" => json_reply(
            services
                .anchor_docs
                .annotations(
                    caller.actor.clone(),
                    caller.project_id,
                    kind,
                    args.include_resolved,
                )
                .await
                .map_err(|error| error.to_string())?,
        ),
        "annotate" => {
            require_contribute(caller)?;
            let annotation = services
                .anchor_docs
                .annotate(
                    caller.actor.clone(),
                    AddAnchorAnnotation {
                        project_id: caller.project_id,
                        kind: kind.to_string(),
                        anchor: required(args.anchor, "anchor")?,
                        quote: args.quote,
                        body: required(args.body, "body")?,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            json_reply(annotation)
        }
        "resolve_annotation" => {
            require_contribute(caller)?;
            let annotation = services
                .anchor_docs
                .resolve_annotation(
                    caller.actor.clone(),
                    ResolveAnchorAnnotation {
                        project_id: caller.project_id,
                        kind: kind.to_string(),
                        annotation_id: required(args.annotation_id, "annotation_id")?
                            .parse()
                            .map_err(|_| "annotation_id must be a uuid".to_string())?,
                        resolution: required(args.resolution, "resolution")?,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            json_reply(annotation)
        }
        "approvals" => json_reply(
            services
                .anchor_docs
                .approvals(caller.actor.clone(), caller.project_id, kind)
                .await
                .map_err(|error| error.to_string())?,
        ),
        "versions" => json_reply(
            services
                .anchor_docs
                .versions(caller.actor.clone(), caller.project_id, kind)
                .await
                .map_err(|error| error.to_string())?,
        ),
        "decide" => {
            require_dispose(caller)?;
            let expected_revision = required_revision(&args)?;
            let decision = ApprovalDecision::parse(&required(args.decision, "decision")?)
                .ok_or_else(|| "decision must be approved or rejected".to_string())?;
            let result = services
                .anchor_docs
                .decide(
                    caller.actor.clone(),
                    DecideAnchorDocument {
                        project_id: caller.project_id,
                        kind: kind.to_string(),
                        expected_revision,
                        decision,
                        comment: args.comment,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            json_reply(result)
        }
        other => Err(format!("unknown document action: {other}")),
    }
}

struct DocumentReply {
    text: String,
    structured: Option<Value>,
}

fn json_reply(value: impl serde::Serialize) -> Result<DocumentReply, String> {
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    let text = value.to_string();
    Ok(DocumentReply {
        text,
        structured: Some(value),
    })
}

fn required(value: Option<String>, name: &str) -> Result<String, String> {
    value.ok_or_else(|| format!("missing {name}"))
}

fn required_revision(args: &DocumentArgs) -> Result<u64, String> {
    args.expected_revision
        .ok_or_else(|| "missing expected_revision".to_string())
}

async fn apply_document_op(
    services: &Services<'_>,
    caller: &Caller<'_>,
    kind: &str,
    expected_revision: u64,
    op: AnchorOp,
) -> Result<DocumentReply, String> {
    let result = services
        .anchor_docs
        .apply(
            caller.actor.clone(),
            ApplyAnchorOperations {
                project_id: caller.project_id,
                kind: kind.to_string(),
                expected_revision,
                operations: vec![AnchorOperation {
                    op_id: Uuid::now_v7(),
                    op,
                }],
            },
        )
        .await
        .map_err(|error| error.to_string())?;
    json_reply(result)
}

async fn call_tool(
    services: &Services<'_>,
    caller: &Caller<'_>,
    name: &str,
    arguments: Value,
) -> Result<Value, String> {
    let issues = services.issues;
    let workflows = services.workflows;
    let task_links = services.task_links;
    match name {
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
            "name": "document",
            "description": "Read, edit, validate, annotate and query an anchor document. action selects the operation; kind is always required.",
            "inputSchema": {
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
            }
        }
    ])
}

#[derive(Debug, Deserialize)]
struct DocumentArgs {
    action: String,
    kind: String,
    #[serde(default)]
    anchor: Option<String>,
    #[serde(default)]
    expected_revision: Option<u64>,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    occurrence_key: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    after: Option<String>,
    #[serde(default)]
    cascade: bool,
    #[serde(default)]
    include_resolved: bool,
    #[serde(default)]
    quote: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    annotation_id: Option<String>,
    #[serde(default)]
    resolution: Option<String>,
    #[serde(default)]
    decision: Option<String>,
    #[serde(default)]
    comment: Option<String>,
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
