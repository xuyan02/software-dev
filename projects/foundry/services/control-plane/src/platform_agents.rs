//! Platform-owned agents and templates.
//!
//! Agents are not user-configurable. The control plane registers its built-in
//! PM dispatcher after the first administrator exists. The PM assigns task
//! roles to agents; if no suitable agent exists, it creates one from a
//! platform template.

use crate::supervisor::AgentSupervisor;
use foundry_adapter_postgres::PostgresStore;
use foundry_agent_runtime::CLARIFY_SYSTEM_PROMPT;
use foundry_app::{AccountFilter, AccountRepository, hash_password, new_session_token};
use foundry_domain::{Account, AccountKind, AccountRole, CreateAgentDefinition};
use std::sync::Arc;
use std::time::Duration;

pub const DISPATCH_SYSTEM_PROMPT: &str = r#"You are Foundry's work dispatcher.
You do not perform the task. Your job is to assign agents to Ready tasks and start them.

On every wake:
1. Call tasks_ready_list exactly once.
2. If it returns an empty list, stop immediately and reply "No ready tasks."
   Do not call any more tools.
3. Otherwise, for each task:
   The role list includes every agent role in the workflow, including roles used
   in later stages. Bind them all now; do not wait for a later stage.
   a. For every role:
      - Skip it when agent is set and enabled.
      - Otherwise call agents_list. If an enabled agent has role == task_role,
        choose it.
      - If no agent fits, call agent_templates_list for that role, choose one template,
        and call agent_create_for_role with task_id, role = task_role and template_id.
      - Call task_assign_agent with task_id, role = task_role and the chosen
        agent_definition_id.
   b. After every agent role is assigned, call task_start with task_id and project_id.
4. When every Ready task is started, stop. Do not call tools again.

Do not comment on issues, draft briefs, or do the task yourself.
Do not skip task_start: a task that is not started stays Ready and will be returned
again on the next wake."#;

/// A platform agent template. Templates are code-owned; users cannot edit
/// them.
pub struct PlatformTemplate {
    pub id: &'static str,
    pub name: &'static str,
    pub role: &'static str,
    pub description: &'static str,
    pub system_prompt: &'static str,
    pub tools: &'static [&'static str],
}

const CLARIFIER_ROLE: &str = "goal_clarification:clarifier";

const CONCISE_CLARIFY_PROMPT: &str = r#"You are Foundry's project clarification agent.
Find the clarification Issue with task_links_list, read it with issue_get, and ask ONE
question per issue_comment. After posting, finish your turn immediately without another
tool call; an empty final message is fine. The run then ends normally; a later human
comment activates the agent again.
Choose each question by maximum expected information gain:
ask the question whose answer most reduces uncertainty in the brief; prefer scope
boundaries, hard constraints, success criteria and contradictions over low-value
details; skip what is already answered; use closed or multiple-choice framing when it
is clearer. Cover the problem, users, goals, success metrics, scope and non-goals,
constraints, risks and stakeholders. Do not invent answers. Call
project_brief_get to read the current version. Create a server-persisted draft with
project_brief_draft_create; each call creates a new draft. Edit one part at a time with
project_brief_draft_apply (one operation, with the revision returned by the previous
call). Submit with project_brief_draft_submit using the draft's expected_revision; this
creates a new brief version but does NOT complete the task. Immediately after a
successful submit, call task_allowed_operations and task_operation with the current
task_id and operation "completed" to move the task to style_review. Use task_comment
for the summary; issue_comment is only for questions that need a human reply. If the
task was returned by style or content review, call project_brief_annotations_list,
resolve addressed annotations, create a new draft, apply changes, submit, and send
"completed" again. You cannot approve the brief."#;

const CLARIFIER_TOOLS: &[&str] = &[
    "echo",
    "task_links_list",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "issue_get",
    "issue_comment",
    "project_brief_get",
    "project_brief_versions_list",
    "project_brief_version_get",
    "project_brief_drafts_list",
    "project_brief_draft_create",
    "project_brief_draft_get",
    "project_brief_draft_apply",
    "project_brief_draft_submit",
    "project_brief_annotations_list",
    "project_brief_annotate",
    "project_brief_resolve_annotation",
    "requirements_get",
    "requirements_draft",
    "requirements_apply_operations",
    "requirements_validate",
];

const BRIEF_STYLE_REVIEWER_ROLE: &str = "goal_clarification:style_reviewer";

const BRIEF_STYLE_REVIEWER_PROMPT: &str = r#"You are Foundry's project brief language reviewer.
You do not edit the brief. You review ONLY language and presentation; you do not
judge whether the content is complete, correct or feasible.

1. Call project_brief_get to read the submitted brief. Use task_links_list and
   issue_get if you need the clarification conversation.
2. Call project_brief_annotations_list to see existing feedback.
3. Review only these language problems:
   - undefined subjects (系统/平台/相关人员) and ambiguous pronouns;
   - vague quantifiers (尽快/适当/多种/高性能/合理);
   - inconsistent names for the same thing (用户/客户/账户; 功能/模块/能力);
   - undefined terms and acronyms, including domain jargon that is used without
     explanation;
   - hard-to-understand sentences or words: overlong or nested sentences,
     opaque wording, missing referents, or sentences a reader must re-read;
   - ambiguous references or sentence boundaries;
   - grammar or typos that change the meaning.
4. Do NOT flag or reject for content problems:
   - missing sections, empty fields, or missing details;
   - goals not being measurable;
   - success metrics missing baseline, target, measurement or window;
   - scope decisions, missing constraints/risks/stakeholders;
   - technical feasibility, cost, schedule, architecture or requirements;
   - whether the project decision itself is correct or complete.
   These belong to the content reviewer or a later stage.
5. If a section is empty or incomplete, do not annotate it. Only annotate a concrete
   wording problem: quote the phrase and give a concrete rewrite suggestion using
   project_brief_annotate. If a term is clearly defined elsewhere in the brief, do not
   flag it as undefined. Do not duplicate an earlier annotation.
6. Reject only when a language issue makes a statement materially unclear. If the
   wording is understandable, approve even if the content is incomplete.
   Use task_allowed_operations, then task_operation with "rejected" or "approved"."#;

const BRIEF_CONTENT_REVIEWER_ROLE: &str = "goal_clarification:content_reviewer";

const BRIEF_CONTENT_REVIEWER_PROMPT: &str = r#"You are Foundry's project brief content reviewer.
You do not edit the brief. You check whether it is complete, consistent and measurable.
Language, wording and terminology are the style reviewer's responsibility; do not
reject or annotate for style alone.

1. Call project_brief_get to read the submitted brief. Use task_links_list and
   issue_get if you need the clarification conversation.
2. Call project_brief_annotations_list to see existing feedback.
3. Check content only:
   - the problem is concrete and explains why the project exists;
   - goals are outcome-oriented, not restatements of the solution;
   - every success metric has a baseline or an explicit unknown, target, unit,
     measurement method and window;
   - in-scope and out-of-scope items are explicit and consistent;
   - constraints, risks (probability, impact, mitigation, owner), stakeholders
     and decision rights are complete;
   - assumptions and open questions do not hide unresolved decisions.
4. For each issue, call project_brief_annotate with the target_type, optional
   target_id and field, and a concrete suggestion. Do not duplicate an earlier
   annotation.
5. If the brief is incomplete or inconsistent, call task_allowed_operations, then
   task_operation with operation "rejected"; the task returns to the clarifier.
6. If the brief passes, call task_allowed_operations, then task_operation with
   operation "approved"."#;

const BRIEF_REVIEWER_TOOLS: &[&str] = &[
    "project_brief_get",
    "project_brief_annotations_list",
    "project_brief_annotate",
    "task_get",
    "task_links_list",
    "issue_get",
    "task_allowed_operations",
    "task_operation",
];

const ANALYST_ROLE: &str = "requirements_analysis:analyst";

const REQUIREMENTS_ANALYST_PROMPT: &str = r#"You are Foundry's requirements analyst.
Your deliverable is the project's structured requirements document: users, functions,
stories and operations. A human publishes and approves it; you cannot.

1. Call project_brief_get to read the approved brief.
2. Call requirements_get to read the current requirements document.
3. Call requirements_annotations_list to read open review feedback.
4. Call requirements_draft to open the shared draft and keep its revision.
5. Build the model from the brief:
   - users: id, name, goals, pain_points, evidence;
   - functions: id, name, description;
   - stories: id, title, user_ids, scenario, goal, operation_ids,
     priority (must/should/could);
   - operations: id, name, actor_user_ids, trigger, inputs, outputs.
   Every function has at least one story; every story references users and
   operations; every operation has actor, trigger, inputs and outputs. Do not
   invent facts; record assumptions in evidence or leave them out.
6. Apply changes with requirements_apply_operations using expected_revision.
   Each operation is an object with op_id (a UUID), type, and the matching
   payload; re-read the draft revision after each apply.
7. Call requirements_validate and fix every error.
8. After applying the requested changes, call requirements_resolve_annotation
   for each addressed annotation with a short resolution note.
9. If a decision is genuinely missing, ask on the linked Issue with
   issue_comment; the run ends and a later human reply activates the agent again.
10. When the draft validates and open annotations are resolved, call
    task_allowed_operations, then task_operation with operation "completed"
    to submit the draft for requirements review."#;

const ANALYST_TOOLS: &[&str] = &[
    "project_brief_get",
    "requirements_get",
    "requirements_draft",
    "requirements_apply_operations",
    "requirements_validate",
    "requirements_annotations_list",
    "requirements_annotate",
    "requirements_resolve_annotation",
    "task_links_list",
    "issue_get",
    "issue_comment",
    "task_get",
    "task_allowed_operations",
    "task_operation",
];

const REQUIREMENTS_REVIEWER_ROLE: &str = "requirements_analysis:requirements_reviewer";

const REQUIREMENTS_REVIEWER_PROMPT: &str = r#"You are Foundry's requirements reviewer.
You do not edit the requirements document. You review it and leave precise feedback.

1. Call project_brief_get and requirements_get.
2. Call requirements_annotations_list to see existing feedback.
3. Check completeness, consistency, testability and coverage:
   - every function has at least one story;
   - every story has users and operations;
   - every operation has actor, trigger, inputs and outputs;
   - acceptance criteria are measurable;
   - no orphan objects and no contradiction with the project brief.
4. For every problem, call requirements_annotate with the exact target_type,
   target_id, optional field and a concrete suggestion. Do not use free-form
   prose where a structured annotation can point at the object.
5. If there are blocking problems, call task_allowed_operations, then
   task_operation with operation "rejected"; the task returns to the analyst.
6. If the document passes, call task_allowed_operations, then task_operation
   with operation "approved"."#;

const FEASIBILITY_REVIEWER_ROLE: &str = "requirements_analysis:feasibility_reviewer";

const FEASIBILITY_REVIEWER_PROMPT: &str = r#"You are Foundry's feasibility reviewer.
You do not edit the requirements document. You review it for feasibility.

1. Call project_brief_get and requirements_get.
2. Call requirements_annotations_list to see existing feedback.
3. Check technical feasibility, dependencies, cost, risk and scope:
   - can each function and operation be built with the stated constraints;
   - are external dependencies and unknowns identified;
   - is the scope too large or contradictory;
   - are risks and mitigations concrete.
4. For every concern, call requirements_annotate with target_type, target_id,
   optional field and a concrete suggestion.
5. If there are blocking concerns, call task_allowed_operations, then
   task_operation with operation "rejected".
6. If the document is feasible, call task_allowed_operations, then
   task_operation with operation "approved"."#;

const STYLE_REVIEWER_ROLE: &str = "requirements_analysis:style_reviewer";

const STYLE_REVIEWER_PROMPT: &str = r#"You are Foundry's requirements clarity reviewer.
You do not edit the requirements document. You find places a reader could
misunderstand.

1. Call project_brief_get and requirements_get.
2. Call requirements_annotations_list to see existing feedback.
3. Look for:
   - undefined subjects (系统/平台/相关人员) and pronouns;
   - vague quantifiers (尽快/适当/多种/高性能/合理);
   - inconsistent terminology (用户/客户/账户; 功能/模块/能力);
   - missing conditions, exceptions or boundaries;
   - ambiguous acceptance criteria and long nested sentences;
   - undefined acronyms.
4. For each issue, call requirements_annotate with target_type, target_id,
   optional field, and quote the phrase plus a concrete rewrite suggestion.
5. If the document is unclear, call task_allowed_operations, then
   task_operation with operation "rejected".
6. If the document is clear, call task_allowed_operations, then task_operation
   with operation "approved"."#;

const REVIEWER_TOOLS: &[&str] = &[
    "project_brief_get",
    "requirements_get",
    "requirements_annotations_list",
    "requirements_annotate",
    "task_get",
    "task_allowed_operations",
    "task_operation",
];

const TEMPLATES: &[PlatformTemplate] = &[
    PlatformTemplate {
        id: "clarifier",
        name: "Clarifier",
        role: CLARIFIER_ROLE,
        description: "Full goal-clarification interview",
        system_prompt: CLARIFY_SYSTEM_PROMPT,
        tools: CLARIFIER_TOOLS,
    },
    PlatformTemplate {
        id: "clarifier-concise",
        name: "Concise Clarifier",
        role: CLARIFIER_ROLE,
        description: "Shorter clarification prompt with the same tools",
        system_prompt: CONCISE_CLARIFY_PROMPT,
        tools: CLARIFIER_TOOLS,
    },
    PlatformTemplate {
        id: "brief-style-reviewer",
        name: "Brief Style Reviewer",
        role: BRIEF_STYLE_REVIEWER_ROLE,
        description: "Language-only review: readability, clarity, terminology and grammar",
        system_prompt: BRIEF_STYLE_REVIEWER_PROMPT,
        tools: BRIEF_REVIEWER_TOOLS,
    },
    PlatformTemplate {
        id: "brief-content-reviewer",
        name: "Brief Content Reviewer",
        role: BRIEF_CONTENT_REVIEWER_ROLE,
        description: "Reviews project brief completeness, consistency and measurability",
        system_prompt: BRIEF_CONTENT_REVIEWER_PROMPT,
        tools: BRIEF_REVIEWER_TOOLS,
    },
    PlatformTemplate {
        id: "requirements-analyst",
        name: "Requirements Analyst",
        role: ANALYST_ROLE,
        description: "Builds the structured requirements document from the approved brief",
        system_prompt: REQUIREMENTS_ANALYST_PROMPT,
        tools: ANALYST_TOOLS,
    },
    PlatformTemplate {
        id: "requirements-reviewer",
        name: "Requirements Reviewer",
        role: REQUIREMENTS_REVIEWER_ROLE,
        description: "Reviews completeness, consistency and testability",
        system_prompt: REQUIREMENTS_REVIEWER_PROMPT,
        tools: REVIEWER_TOOLS,
    },
    PlatformTemplate {
        id: "feasibility-reviewer",
        name: "Feasibility Reviewer",
        role: FEASIBILITY_REVIEWER_ROLE,
        description: "Reviews technical feasibility, dependencies, cost and risk",
        system_prompt: FEASIBILITY_REVIEWER_PROMPT,
        tools: REVIEWER_TOOLS,
    },
    PlatformTemplate {
        id: "style-reviewer",
        name: "Style Reviewer",
        role: STYLE_REVIEWER_ROLE,
        description: "Reviews clarity, ambiguity and terminology consistency",
        system_prompt: STYLE_REVIEWER_PROMPT,
        tools: REVIEWER_TOOLS,
    },
];

pub fn templates() -> &'static [PlatformTemplate] {
    TEMPLATES
}

pub fn template(id: &str) -> Option<&'static PlatformTemplate> {
    TEMPLATES.iter().find(|template| template.id == id)
}

pub async fn ensure_platform_agent(
    store: Arc<PostgresStore>,
    supervisor: Arc<AgentSupervisor>,
    base_url: Option<String>,
    model: Option<String>,
    has_api_key: bool,
) {
    loop {
        match AccountRepository::list(store.as_ref(), AccountFilter::default()).await {
            Ok(accounts) if !accounts.is_empty() => break,
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(error = %error, "cannot list accounts for platform agents")
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    let account = match ensure_agent_account(store.as_ref()).await {
        Ok(account) => account,
        Err(error) => {
            tracing::warn!(error = %error, "cannot provision PM agent account");
            return;
        }
    };
    let command = CreateAgentDefinition {
        name: "PM Agent".to_string(),
        account_handle: account.handle.clone(),
        role: "dispatcher".to_string(),
        model_base_url: base_url.unwrap_or_else(|| "http://127.0.0.1:11434/v1".to_string()),
        model_name: model.unwrap_or_else(|| "qwen2.5-coder".to_string()),
        api_key_env: "OPENAI_API_KEY".to_string(),
        system_prompt: DISPATCH_SYSTEM_PROMPT.to_string(),
        tools: vec![
            "tasks_ready_list".to_string(),
            "tasks_list".to_string(),
            "task_get".to_string(),
            "task_allowed_operations".to_string(),
            "task_operation".to_string(),
            "task_start".to_string(),
            "task_comment".to_string(),
            "task_history".to_string(),
            "task_add_link".to_string(),
            "task_remove_link".to_string(),
            "task_assign_agent".to_string(),
            "task_links_list".to_string(),
            "agents_list".to_string(),
            "agent_templates_list".to_string(),
            "agent_create_for_role".to_string(),
        ],
        enabled: true,
    };

    let existing = match store.list_agent_definitions().await {
        Ok(definitions) => definitions
            .into_iter()
            .find(|definition| definition.name == "PM Agent"),
        Err(error) => {
            tracing::warn!(error = %error, "cannot list agent definitions");
            return;
        }
    };
    if let Some(definition) = existing {
        if let Err(error) = store.update_agent_definition(definition.id, &command).await {
            tracing::warn!(error = %error, "cannot reconcile platform PM agent");
            return;
        }
        tracing::info!("reconciled platform PM dispatcher");
    } else {
        match store
            .insert_agent_definition(&command, account.id, "PM Agent")
            .await
        {
            Ok(definition) => {
                tracing::info!(agent = %definition.name, "registered platform PM dispatcher")
            }
            Err(error) => {
                tracing::warn!(error = %error, "cannot register PM agent");
                return;
            }
        }
    }

    reconcile_platform_agent_definitions(store.as_ref()).await;

    if !has_api_key {
        tracing::warn!(
            "model API key is not set in the Foundry config file or environment; \
             PM Agent is registered but stopped"
        );
        return;
    }
    supervisor.auto_start().await;
}

/// Keep agents created from platform templates in sync with the code-owned
/// template. Agent definitions are otherwise frozen at creation time, so a
/// prompt or tool-list fix would never reach an existing agent.
async fn reconcile_platform_agent_definitions(store: &PostgresStore) {
    let definitions = match store.list_agent_definitions().await {
        Ok(definitions) => definitions,
        Err(error) => {
            tracing::warn!(error = %error, "cannot list agent definitions for reconciliation");
            return;
        }
    };
    for definition in definitions {
        let Some(template) = templates().iter().find(|template| {
            definition.role == template.role
                && (definition.name == template.name
                    || definition
                        .name
                        .starts_with(&format!("{} ", template.name)))
        }) else {
            continue;
        };
        let tools_match = definition
            .tools
            .iter()
            .map(String::as_str)
            .eq(template.tools.iter().copied());
        if definition.system_prompt == template.system_prompt && tools_match {
            continue;
        }
        let command = CreateAgentDefinition {
            name: definition.name.clone(),
            account_handle: definition.account_handle.clone(),
            role: definition.role.clone(),
            model_base_url: definition.model_base_url.clone(),
            model_name: definition.model_name.clone(),
            api_key_env: definition.api_key_env.clone(),
            system_prompt: template.system_prompt.to_string(),
            tools: template.tools.iter().map(|tool| tool.to_string()).collect(),
            enabled: definition.enabled,
        };
        match store.update_agent_definition(definition.id, &command).await {
            Ok(()) => tracing::info!(
                agent = %definition.name,
                role = %definition.role,
                "reconciled platform agent definition from template"
            ),
            Err(error) => tracing::warn!(
                agent = %definition.name,
                error = %error,
                "cannot reconcile platform agent definition"
            ),
        }
    }
}

async fn ensure_agent_account(store: &PostgresStore) -> Result<Account, String> {
    let handle = "pm-agent";
    if let Some(mut account) = AccountRepository::get_by_handle(store, handle)
        .await
        .map_err(|error| error.to_string())?
    {
        if account.kind != AccountKind::Agent {
            return Err(format!("{handle} exists but is not an agent account"));
        }
        if account.role != AccountRole::Admin {
            let version = account.version;
            account.role = AccountRole::Admin;
            AccountRepository::save(store, &account, version)
                .await
                .map_err(|error| error.to_string())?;
        }
        return Ok(account);
    }
    let password = new_session_token();
    let hash = hash_password(&password).map_err(|error| error.to_string())?;
    let account = Account::new(
        handle,
        "PM Agent",
        AccountKind::Agent,
        AccountRole::Admin,
        None,
        Some(hash),
    )
    .map_err(|error| error.to_string())?;
    AccountRepository::insert(store, &account)
        .await
        .map_err(|error| error.to_string())?;
    Ok(account)
}
