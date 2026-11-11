use crate::{DocumentReviewLinkAction, RoleSpec, TaskState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::title::TitleTemplate;
use crate::script::{EventFieldSet, VarType, VariableValue};

/// The only AST schema version this interpreter understands.
pub const SCHEMA_VERSION: u32 = 1;

/// The reserved target that completes a task. It is a virtual stage: it is
/// never declared in `stages` and never becomes the task's current stage.
pub const ENDED_STAGE: &str = "ended";

/// The reserved task variable that carries the completion result. It is
/// readable and writable from every script, and is copied to `task.result`
/// when a transition targets [`ENDED_STAGE`].
pub const RESULT_VARIABLE: &str = "result";

/// The declarative body of one published workflow version.
///
/// The AST is the source of truth: a flowchart is a projection of it, never a
/// second representation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowBody {
    pub schema_version: u32,
    #[serde(default)]
    pub roles: Vec<RoleSpec>,
    /// Task variables. They are initialized when the task is created and
    /// survive stage changes.
    #[serde(default)]
    pub vars: BTreeMap<String, VarSpec>,
    #[serde(default)]
    pub initial_stage: Option<String>,
    #[serde(default)]
    pub stages: Vec<WorkflowStage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStage {
    pub name: String,
    #[serde(default)]
    pub active_roles: Vec<String>,
    #[serde(default)]
    pub vars: BTreeMap<String, VarSpec>,
    #[serde(default)]
    pub operations: Vec<OperationSpec>,
    #[serde(default)]
    pub events: Vec<EventSpec>,
    /// Effects executed on every entry into this stage.
    #[serde(default)]
    pub created: Vec<CreatedEffect>,
    #[serde(default)]
    pub transitions: Vec<TransitionSpec>,
    /// Stage to enter when a platform-level human rejection is sent from this
    /// stage. Falls back to the workflow's initial stage when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reject_to: Option<String>,
}

impl WorkflowStage {
    pub fn var_types(&self) -> BTreeMap<String, VarType> {
        self.vars
            .iter()
            .map(|(name, spec)| (name.clone(), spec.var_type))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VarSpec {
    #[serde(rename = "type")]
    pub var_type: VarType,
    pub display_name: String,
    /// The value a variable starts with. Absent means the type's default.
    /// `actor_set` variables always start empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial: Option<InitialValue>,
}

impl VarSpec {
    pub fn initial_value(&self) -> VariableValue {
        match (&self.initial, self.var_type) {
            (Some(InitialValue::Bool(value)), VarType::Bool) => VariableValue::Bool(*value),
            (Some(InitialValue::Int(value)), VarType::Int) => VariableValue::Int(*value),
            (Some(InitialValue::String(value)), VarType::String) => {
                VariableValue::String(value.clone())
            }
            _ => self.var_type.default_value(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InitialValue {
    Bool(bool),
    Int(i64),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationSpec {
    pub name: String,
    pub display_name: String,
    pub description: String,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventSpec {
    pub event: EventKind,
    #[serde(default)]
    pub action: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventKind {
    #[serde(rename = "task.created")]
    TaskCreated,
    #[serde(rename = "task.state_changed")]
    TaskStateChanged,
    #[serde(rename = "child.created")]
    ChildCreated,
    #[serde(rename = "child.state_changed")]
    ChildStateChanged,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TaskCreated => "task.created",
            Self::TaskStateChanged => "task.state_changed",
            Self::ChildCreated => "child.created",
            Self::ChildStateChanged => "child.state_changed",
        }
    }

    pub fn field_set(self) -> EventFieldSet {
        match self {
            Self::TaskCreated => EventFieldSet::None,
            Self::TaskStateChanged => EventFieldSet::TaskStateChanged,
            Self::ChildCreated => EventFieldSet::ChildCreated,
            Self::ChildStateChanged => EventFieldSet::ChildStateChanged,
        }
    }
}

/// An effect executed when a stage is entered. Externally tagged:
/// `{"set_state": "in_progress"}`, `{"create_child": {...}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreatedEffect {
    SetState(TaskState),
    CreateChild(CreateChild),
    DocumentReviewLink(DocumentReviewLinkEffect),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentReviewLinkEffect {
    pub kind: String,
    pub action: DocumentReviewLinkAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateChild {
    pub workflow: String,
    pub title: TitleTemplate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionSpec {
    pub on: String,
    /// Executed after `on` matched and before entering `to`.
    #[serde(default)]
    pub action: String,
    /// A declared stage name, or [`ENDED_STAGE`].
    pub to: String,
}
