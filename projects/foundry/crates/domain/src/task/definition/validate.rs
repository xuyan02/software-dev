use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::Serialize;

use super::ast::{
    CreatedEffect, ENDED_STAGE, InitialValue, OperationSpec, RESULT_VARIABLE, SCHEMA_VERSION,
    VarSpec, WorkflowBody, WorkflowStage,
};
use crate::script::{
    EventAccess, ScriptIssue, ScriptScope, VarType, compile_action, compile_condition, is_reserved,
};
use crate::{ACCEPT_OPERATION, REJECT_OPERATION, START_OPERATION, TaskState};

pub const MAX_STAGES: usize = 200;
pub const MAX_ROLES: usize = 50;
pub const MAX_TRANSITIONS: usize = 500;
pub const MAX_AST_BYTES: usize = 1024 * 1024;
pub const MAX_DISPLAY_NAME_LEN: usize = 100;
pub const MAX_DESCRIPTION_LEN: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationIssue {
    pub path: String,
    pub code: String,
    pub message: String,
}

impl ValidationIssue {
    pub fn error(path: impl Into<String>, code: &str, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            code: code.to_string(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Validation {
    pub errors: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationIssue>,
}

impl Validation {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    fn error(&mut self, path: impl Into<String>, code: &str, message: impl Into<String>) {
        self.errors
            .push(ValidationIssue::error(path, code, message));
    }

    fn warning(&mut self, path: impl Into<String>, code: &str, message: impl Into<String>) {
        self.warnings
            .push(ValidationIssue::error(path, code, message));
    }
}

/// Validate one definition body.
///
/// `is_known` answers whether a referenced workflow id is visible and has a
/// published version in the definition owner's scope. A self-reference is the
/// caller's responsibility to allow when the workflow has a published version.
pub fn validate(body: &WorkflowBody, is_known: &dyn Fn(&str) -> bool) -> Validation {
    let mut validation = Validation::default();

    if body.schema_version != SCHEMA_VERSION {
        validation.error(
            "schema_version",
            "unsupported_schema_version",
            format!(
                "schema_version must be {SCHEMA_VERSION}, got {}",
                body.schema_version
            ),
        );
    }
    if body.stages.len() > MAX_STAGES {
        validation.error(
            "stages",
            "too_many_stages",
            format!("a workflow may declare at most {MAX_STAGES} stages"),
        );
    }
    if body.roles.len() > MAX_ROLES {
        validation.error(
            "roles",
            "too_many_roles",
            format!("a workflow may declare at most {MAX_ROLES} roles"),
        );
    }
    if body.stages.is_empty() {
        validation.error("stages", "no_stages", "a workflow must declare a stage");
    }
    if let Ok(bytes) = serde_json::to_vec(body)
        && bytes.len() > MAX_AST_BYTES
    {
        validation.error(
            "",
            "workflow_too_large",
            format!("serialized definition exceeds {MAX_AST_BYTES} bytes"),
        );
    }

    let mut stage_names = HashSet::new();
    for (index, stage) in body.stages.iter().enumerate() {
        let path = format!("stages[{index}].name");
        if stage.name == ENDED_STAGE {
            validation.error(
                &path,
                "reserved_stage_name",
                format!("stage name {ENDED_STAGE:?} is reserved for task completion"),
            );
        }
        if !is_valid_key(&stage.name) {
            validation.error(
                &path,
                "invalid_stage_name",
                format!("stage name {:?} must match [a-z][a-z0-9_]*", stage.name),
            );
        }
        if !stage_names.insert(stage.name.as_str()) {
            validation.error(
                &path,
                "duplicate_stage_name",
                format!("stage {:?} is declared more than once", stage.name),
            );
        }
    }

    let initial = body.initial_stage.as_deref();
    match initial {
        None => validation.error(
            "initial_stage",
            "missing_initial_stage",
            "a published workflow must declare initial_stage",
        ),
        Some(name) if !stage_names.contains(name) => validation.error(
            "initial_stage",
            "unknown_initial_stage",
            format!("initial_stage {name:?} is not a declared stage"),
        ),
        Some(_) => {}
    }

    let mut role_names = HashSet::new();
    for (index, role) in body.roles.iter().enumerate() {
        let path = format!("roles[{index}]");
        if !is_valid_key(&role.name) {
            validation.error(
                format!("{path}.name"),
                "invalid_role_name",
                format!("role name {:?} must match [a-z][a-z0-9_]*", role.name),
            );
        }
        if !role_names.insert(role.name.as_str()) {
            validation.error(
                format!("{path}.name"),
                "duplicate_role_name",
                format!("role {:?} is declared more than once", role.name),
            );
        }
        if let Some(max) = role.max_actors
            && (max == 0 || max < role.min_actors)
        {
            validation.error(
                format!("{path}.max_actors"),
                "invalid_max_actors",
                format!(
                    "max_actors must be at least 1 and at least min_actors ({})",
                    role.min_actors
                ),
            );
        }
    }

    let mut task_var_types = BTreeMap::new();
    for (name, spec) in &body.vars {
        validate_var(name, spec, &format!("vars.{name}"), &mut validation);
        task_var_types.insert(name.clone(), spec.var_type);
    }

    let declared_roles: Vec<String> = body.roles.iter().map(|role| role.name.clone()).collect();
    let known_stage_names: HashSet<&str> = stage_names.iter().copied().collect();
    let mut transition_count = 0;
    let mut global_reads = BTreeSet::new();
    let mut global_writes = BTreeSet::new();
    let mut stage_scripts = Vec::new();

    for (index, stage) in body.stages.iter().enumerate() {
        let stage_path = format!("stages[{index}]");
        if let Some(target) = &stage.reject_to
            && !known_stage_names.contains(target.as_str())
        {
            validation.error(
                format!("{stage_path}.reject_to"),
                "unknown_reject_stage",
                format!("reject_to {target:?} is not a declared stage"),
            );
        }
        let scripts = validate_stage(
            stage,
            &stage_path,
            &declared_roles,
            &known_stage_names,
            &task_var_types,
            is_known,
            &mut validation,
        );
        global_reads.extend(scripts.reads.iter().cloned());
        global_writes.extend(scripts.writes.iter().cloned());
        transition_count += stage.transitions.len();
        stage_scripts.push(scripts);
    }

    for name in body.vars.keys() {
        if !global_reads.contains(name) {
            validation.warning(
                format!("vars.{name}"),
                "unused_variable",
                format!("variable {name:?} is never read by any action or on"),
            );
        }
    }

    // `transition_never_triggered`: `on` reads variables that no action writes.
    // A task variable may be written by an action in another stage, so the
    // whole workflow's writes count for those.
    for scripts in &stage_scripts {
        for (path, reads, stage_writes) in &scripts.never_triggered {
            let triggered = reads.iter().any(|name| {
                stage_writes.contains(name)
                    || (task_var_types.contains_key(name) && global_writes.contains(name))
            });
            if !triggered {
                validation.warning(
                    path.clone(),
                    "transition_never_triggered",
                    "this on reads variables that no action writes",
                );
            }
        }
    }

    if transition_count > MAX_TRANSITIONS {
        validation.error(
            "stages",
            "too_many_transitions",
            format!("a workflow may declare at most {MAX_TRANSITIONS} transitions"),
        );
    }

    add_warnings(body, initial, &mut validation);
    validation
}

fn validate_stage(
    stage: &WorkflowStage,
    path: &str,
    declared_roles: &[String],
    stage_names: &HashSet<&str>,
    task_var_types: &BTreeMap<String, VarType>,
    is_known: &dyn Fn(&str) -> bool,
    validation: &mut Validation,
) -> StageScripts {
    let mut active_roles = HashSet::new();
    for (role_index, role) in stage.active_roles.iter().enumerate() {
        let role_path = format!("{path}.active_roles[{role_index}]");
        if !declared_roles.iter().any(|declared| declared == role) {
            validation.error(
                &role_path,
                "unknown_role",
                format!("active role {role:?} is not declared"),
            );
        }
        if !active_roles.insert(role.as_str()) {
            validation.error(
                &role_path,
                "duplicate_active_role",
                format!("active role {role:?} is listed more than once"),
            );
        }
    }

    for (name, spec) in &stage.vars {
        validate_var(name, spec, &format!("{path}.vars.{name}"), validation);
        if task_var_types.contains_key(name) {
            validation.error(
                format!("{path}.vars.{name}"),
                "variable_shadows_task_variable",
                format!("stage variable {name:?} collides with a task variable"),
            );
        }
    }
    let variables = script_variables(task_var_types, stage);

    let mut scripts = StageScripts::default();
    let mut operation_names = HashSet::new();

    for (operation_index, operation) in stage.operations.iter().enumerate() {
        let operation_path = format!("{path}.operations[{operation_index}]");
        validate_operation(operation, &operation_path, &active_roles, validation);
        if [START_OPERATION, ACCEPT_OPERATION, REJECT_OPERATION].contains(&operation.name.as_str())
        {
            validation.error(
                format!("{operation_path}.name"),
                "reserved_operation",
                format!(
                    "operation name {:?} is reserved for a platform command",
                    operation.name
                ),
            );
        }
        if !operation_names.insert(operation.name.as_str()) {
            validation.error(
                format!("{operation_path}.name"),
                "duplicate_operation",
                format!("operation {:?} is declared more than once", operation.name),
            );
        }
        if let Some(compiled) = compile_stage_action(
            &operation.action,
            stage,
            &variables,
            declared_roles,
            is_known,
            EventAccess::Operation,
            format!("{operation_path}.action"),
            validation,
        ) {
            scripts.reads.extend(compiled.0);
            scripts.writes.extend(compiled.1);
        }
    }

    let mut event_names = HashSet::new();
    for (event_index, event) in stage.events.iter().enumerate() {
        let event_path = format!("{path}.events[{event_index}]");
        if !event_names.insert(event.event) {
            validation.error(
                &event_path,
                "duplicate_event",
                format!(
                    "event {:?} is declared more than once",
                    event.event.as_str()
                ),
            );
        }
        if let Some(compiled) = compile_stage_action(
            &event.action,
            stage,
            &variables,
            declared_roles,
            is_known,
            EventAccess::Event(event.event.field_set()),
            format!("{event_path}.action"),
            validation,
        ) {
            scripts.reads.extend(compiled.0);
            scripts.writes.extend(compiled.1);
        }
    }

    for (effect_index, effect) in stage.created.iter().enumerate() {
        let effect_path = format!("{path}.created[{effect_index}]");
        match effect {
            CreatedEffect::SetState(TaskState::Completed) => validation.error(
                format!("{effect_path}.set_state"),
                "set_state_completed",
                "set_state cannot set completed; use a transition to ended",
            ),
            CreatedEffect::SetState(_) => {}
            CreatedEffect::CreateChild(child) => {
                let child_path = format!("{effect_path}.create_child");
                if !is_valid_key(&child.workflow) {
                    validation.error(
                        format!("{child_path}.workflow"),
                        "invalid_workflow_id",
                        format!(
                            "workflow id {:?} must match [a-z][a-z0-9_]*",
                            child.workflow
                        ),
                    );
                } else if !is_known(&child.workflow) {
                    validation.error(
                        format!("{child_path}.workflow"),
                        "unknown_child_workflow",
                        format!(
                            "create_child references unknown workflow {:?}",
                            child.workflow
                        ),
                    );
                }
                child.title.validate(
                    is_known,
                    &format!("{child_path}.title"),
                    &mut validation.errors,
                );
            }
            CreatedEffect::DocumentReviewLink(link) => {
                if !matches!(
                    link.kind.as_str(),
                    "project"
                        | "requirements"
                        | "architecture"
                        | "design"
                        | "development"
                        | "test_plan"
                        | "test_report"
                ) {
                    validation.error(
                        format!("{effect_path}.document_review_link.kind"),
                        "unknown_document_kind",
                        format!(
                            "document_review_link kind {:?} must be project, requirements, architecture, design, development, test_plan or test_report",
                            link.kind
                        ),
                    );
                }
            }
        }
    }

    let mut transition_reads = Vec::new();
    for (transition_index, transition) in stage.transitions.iter().enumerate() {
        let transition_path = format!("{path}.transitions[{transition_index}]");
        if transition.on.trim().is_empty() {
            validation.error(
                format!("{transition_path}.on"),
                "invalid_transition_on",
                "on must be a non-empty boolean expression",
            );
        } else if let Some(reads) = compile_stage_condition(
            &transition.on,
            stage,
            &variables,
            declared_roles,
            is_known,
            format!("{transition_path}.on"),
            validation,
        ) {
            transition_reads.push((transition_index, reads.clone()));
            scripts.reads.extend(reads);
        }
        if !transition.action.trim().is_empty()
            && let Some(compiled) = compile_stage_action(
                &transition.action,
                stage,
                &variables,
                declared_roles,
                is_known,
                EventAccess::Condition,
                format!("{transition_path}.action"),
                validation,
            )
        {
            scripts.reads.extend(compiled.0);
            scripts.writes.extend(compiled.1);
        }
        if transition.to != ENDED_STAGE && !stage_names.contains(transition.to.as_str()) {
            validation.error(
                format!("{transition_path}.to"),
                "unknown_stage",
                format!(
                    "transition target {:?} is not a declared stage or {ENDED_STAGE:?}",
                    transition.to
                ),
            );
        }
    }

    // `unused_variable`: never read by any action or `on` in this stage.
    for name in stage.vars.keys() {
        if !scripts.reads.contains(name) {
            validation.warning(
                format!("{path}.vars.{name}"),
                "unused_variable",
                format!("variable {name:?} is never read by any action or on"),
            );
        }
    }

    let stage_writes = scripts.writes.clone();
    for (transition_index, reads) in &transition_reads {
        if reads.is_empty() || reads.iter().any(|name| stage_writes.contains(name)) {
            continue;
        }
        scripts.never_triggered.push((
            format!("{path}.transitions[{transition_index}].on"),
            reads.clone(),
            stage_writes.clone(),
        ));
    }

    scripts
}

#[derive(Default)]
struct StageScripts {
    reads: BTreeSet<String>,
    writes: BTreeSet<String>,
    never_triggered: Vec<(String, BTreeSet<String>, BTreeSet<String>)>,
}

fn validate_var(name: &str, spec: &VarSpec, path: &str, validation: &mut Validation) {
    if !is_valid_key(name) {
        validation.error(
            path,
            "invalid_variable_name",
            format!("variable name {name:?} must match [a-z][a-z0-9_]*"),
        );
    }
    if is_reserved(name) {
        validation.error(
            path,
            "script_reserved_name",
            format!("variable name {name:?} is a reserved word"),
        );
    }
    if !is_valid_display_name(&spec.display_name) {
        validation.error(
            format!("{path}.display_name"),
            "invalid_display_name",
            format!("display_name must be non-empty and at most {MAX_DISPLAY_NAME_LEN} characters"),
        );
    }
    if let Some(initial) = &spec.initial
        && !initial_matches(spec.var_type, initial)
    {
        validation.error(
            format!("{path}.initial"),
            "initial_variable_type",
            format!(
                "initial value does not match variable type {}",
                var_type_name(spec.var_type)
            ),
        );
    }
}

fn initial_matches(var_type: VarType, initial: &InitialValue) -> bool {
    matches!(
        (var_type, initial),
        (VarType::Bool, InitialValue::Bool(_))
            | (VarType::Int, InitialValue::Int(_))
            | (VarType::String, InitialValue::String(_))
    )
}

fn var_type_name(var_type: VarType) -> &'static str {
    match var_type {
        VarType::Bool => "bool",
        VarType::String => "string",
        VarType::Int => "int",
        VarType::ActorSet => "actor_set",
    }
}

/// Variable types visible to a script: task variables, the reserved `result`
/// variable and the stage's variables.
fn script_variables(
    task_var_types: &BTreeMap<String, VarType>,
    stage: &WorkflowStage,
) -> BTreeMap<String, VarType> {
    let mut variables = task_var_types.clone();
    variables.insert(RESULT_VARIABLE.to_string(), VarType::String);
    for (name, spec) in &stage.vars {
        variables.insert(name.clone(), spec.var_type);
    }
    variables
}

fn validate_operation(
    operation: &OperationSpec,
    path: &str,
    active_roles: &HashSet<&str>,
    validation: &mut Validation,
) {
    if !is_valid_key(&operation.name) {
        validation.error(
            format!("{path}.name"),
            "invalid_operation",
            format!(
                "operation name {:?} must match [a-z][a-z0-9_]*",
                operation.name
            ),
        );
    }
    if !is_valid_display_name(&operation.display_name) {
        validation.error(
            format!("{path}.display_name"),
            "invalid_display_name",
            format!("display_name must be non-empty and at most {MAX_DISPLAY_NAME_LEN} characters"),
        );
    }
    if !is_valid_description(&operation.description) {
        validation.error(
            format!("{path}.description"),
            "invalid_description",
            format!("description must be non-empty and at most {MAX_DESCRIPTION_LEN} characters"),
        );
    }
    if operation.roles.is_empty() {
        validation.error(
            format!("{path}.roles"),
            "empty_operation_roles",
            "an operation must declare at least one role",
        );
    }
    for (role_index, role) in operation.roles.iter().enumerate() {
        if !active_roles.contains(role.as_str()) {
            validation.error(
                format!("{path}.roles[{role_index}]"),
                "operation_role_not_active",
                format!("operation role {role:?} is not an active role of this stage"),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compile_stage_action(
    source: &str,
    stage: &WorkflowStage,
    variables: &BTreeMap<String, VarType>,
    roles: &[String],
    is_known: &dyn Fn(&str) -> bool,
    event: EventAccess,
    path: String,
    validation: &mut Validation,
) -> Option<(BTreeSet<String>, BTreeSet<String>)> {
    let known = |workflow: &str| is_known(workflow);
    let scope = ScriptScope {
        variables,
        roles,
        active_roles: &stage.active_roles,
        known_workflow: &known,
        event,
    };
    match compile_action(source, &scope) {
        Ok(compiled) => Some((compiled.reads, compiled.writes)),
        Err(issues) => {
            report_script_issues(issues, path, validation);
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compile_stage_condition(
    source: &str,
    stage: &WorkflowStage,
    variables: &BTreeMap<String, VarType>,
    roles: &[String],
    is_known: &dyn Fn(&str) -> bool,
    path: String,
    validation: &mut Validation,
) -> Option<BTreeSet<String>> {
    let known = |workflow: &str| is_known(workflow);
    let scope = ScriptScope {
        variables,
        roles,
        active_roles: &stage.active_roles,
        known_workflow: &known,
        event: EventAccess::Condition,
    };
    match compile_condition(source, &scope) {
        Ok(compiled) => Some(compiled.reads),
        Err(issues) => {
            report_script_issues(issues, path, validation);
            None
        }
    }
}

fn report_script_issues(issues: Vec<ScriptIssue>, path: String, validation: &mut Validation) {
    for issue in issues {
        validation.error(path.clone(), issue.code, issue.message);
    }
}

fn add_warnings(body: &WorkflowBody, initial: Option<&str>, validation: &mut Validation) {
    let mut incoming: HashSet<&str> = HashSet::new();
    for stage in &body.stages {
        for transition in &stage.transitions {
            incoming.insert(transition.to.as_str());
        }
    }

    let mut roles_in_play: HashSet<&str> = HashSet::new();
    for (index, stage) in body.stages.iter().enumerate() {
        for role in &stage.active_roles {
            roles_in_play.insert(role.as_str());
        }
        if initial.is_some()
            && initial != Some(stage.name.as_str())
            && !incoming.contains(stage.name.as_str())
        {
            validation.warning(
                format!("stages[{index}]"),
                "unreachable_stage",
                format!("stage {:?} is not reachable from initial_stage", stage.name),
            );
        }
        if stage.transitions.is_empty() {
            validation.warning(
                format!("stages[{index}]"),
                "dead_end_stage",
                format!("stage {:?} has no transition", stage.name),
            );
        }
        if !stage.operations.is_empty() && stage.active_roles.is_empty() {
            validation.warning(
                format!("stages[{index}]"),
                "operations_without_actors",
                "stage declares operations but no active_roles, so nobody can send them",
            );
        }
        if !stage.operations.is_empty() {
            for role in &stage.active_roles {
                if let Some(spec) = body.roles.iter().find(|spec| &spec.name == role)
                    && spec.min_actors == 0
                {
                    validation.warning(
                        format!("stages[{index}].active_roles"),
                        "optional_role_on_operation_stage",
                        format!(
                            "role {:?} is optional (min_actors = 0) on a stage with operations",
                            spec.name
                        ),
                    );
                }
            }
        }
    }

    for (index, role) in body.roles.iter().enumerate() {
        if !roles_in_play.contains(role.name.as_str()) {
            validation.warning(
                format!("roles[{index}]"),
                "unused_role",
                format!("role {:?} is declared but never active", role.name),
            );
        }
    }
}

fn is_valid_display_name(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= MAX_DISPLAY_NAME_LEN
}

fn is_valid_description(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= MAX_DESCRIPTION_LEN
}

pub(crate) fn is_valid_key(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}
