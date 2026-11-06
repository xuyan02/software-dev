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

use foundry_app::{
    CommentIssue, CreateIssue, CreateRequirement, IssueFilter, IssueUseCases, RequirementFilter,
    RequirementUseCases, TransitionIssue, TransitionRequirement, TriageIssue, WorkflowUseCases,
};
use foundry_domain::{
    AcceptanceScenario, Actor, IssueKind, IssueSeverity, IssueSource, IssueSourceKind, IssueStatus,
    Priority, RequirementStatus, RiskClass, Triage, WorkflowId,
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

pub async fn handle(
    requirements: &dyn RequirementUseCases,
    issues: &dyn IssueUseCases,
    workflows: &dyn WorkflowUseCases,
    caller: &Caller<'_>,
    request: Value,
) -> Value {
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
                    account owning the bearer token. Issues are the problem space: create, \
                    discuss and triage them before deciding what becomes a requirement. \
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
            match call_tool(requirements, issues, workflows, caller, name, arguments).await {
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
    requirements: &dyn RequirementUseCases,
    issues: &dyn IssueUseCases,
    workflows: &dyn WorkflowUseCases,
    caller: &Caller<'_>,
    name: &str,
    arguments: Value,
) -> Result<Value, String> {
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
        "triage_issue" => {
            require_contribute(caller)?;
            let args: TriageIssueArgs =
                serde_json::from_value(arguments).map_err(|error| error.to_string())?;
            let issue = issues
                .triage(TriageIssue {
                    project_id: caller.project_id,
                    id: args.id,
                    actor: caller.actor.clone(),
                    triage: Triage {
                        kind: args.kind,
                        severity: args.severity,
                        assignee: args.assignee,
                        area: args.area,
                        labels: args.labels,
                        impact: args.impact,
                    },
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
                            request or incident. Classification can be refined by triage later.",
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
            "name": "triage_issue",
            "description": "Classify an issue: set kind, severity, assignee, area, labels and \
                            impact. Bug issues require a severity.",
            "inputSchema": {
                "type": "object",
                "required": ["id", "kind"],
                "properties": {
                    "id": { "type": "string", "format": "uuid" },
                    "kind": {
                        "type": "string",
                        "enum": ["feedback", "bug", "question", "idea", "task_request", "incident"]
                    },
                    "severity": { "type": "string", "enum": ["s1", "s2", "s3", "s4"] },
                    "assignee": { "type": "object" },
                    "area": { "type": "string" },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "impact": { "type": "string" },
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
                        "enum": ["open", "triaged", "in_progress", "resolved", "closed", "rejected", "duplicate"]
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
        }
    ])
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
struct TriageIssueArgs {
    id: uuid::Uuid,
    kind: IssueKind,
    #[serde(default)]
    severity: Option<IssueSeverity>,
    #[serde(default)]
    assignee: Option<Actor>,
    #[serde(default)]
    area: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    impact: Option<String>,
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
