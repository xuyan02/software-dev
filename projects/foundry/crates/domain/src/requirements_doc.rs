//! Structured requirements document (RFC 0020).
//!
//! One document per project. The body is a typed tree; human-readable output is
//! a projection. Agents mutate it only through structured operations.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Validation, ValidationIssue};

pub const REQUIREMENTS_SCHEMA_VERSION: u16 = 1;
pub const MAX_REQUIREMENTS_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementsDocumentBody {
    pub schema_version: u16,
    #[serde(default)]
    pub users: Vec<UserModel>,
    #[serde(default)]
    pub functions: Vec<Function>,
}

impl Default for RequirementsDocumentBody {
    fn default() -> Self {
        Self {
            schema_version: REQUIREMENTS_SCHEMA_VERSION,
            users: Vec::new(),
            functions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EntityStatus {
    #[default]
    Draft,
    Active,
    Deprecated,
    Retired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StoryPriority {
    Must,
    #[default]
    Should,
    Could,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationTarget {
    Document,
    User,
    Function,
    Story,
    Operation,
}

impl AnnotationTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::User => "user",
            Self::Function => "function",
            Self::Story => "story",
            Self::Operation => "operation",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "document" => Some(Self::Document),
            "user" => Some(Self::User),
            "function" => Some(Self::Function),
            "story" => Some(Self::Story),
            "operation" => Some(Self::Operation),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationStatus {
    #[default]
    Open,
    Resolved,
}

impl AnnotationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Resolved => "resolved",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "resolved" => Self::Resolved,
            _ => Self::Open,
        }
    }
}

/// A review note anchored to a structured requirements object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementsAnnotation {
    pub id: Uuid,
    pub project_id: Uuid,
    pub target_type: AnnotationTarget,
    pub target_id: Option<String>,
    /// Optional field inside the target, such as `goal` or `inputs`.
    pub field: Option<String>,
    pub body: String,
    pub author_id: Uuid,
    pub author_handle: String,
    pub author_name: String,
    pub status: AnnotationStatus,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub resolved_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub resolved_by: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserModel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub age: Option<u8>,
    #[serde(default)]
    pub gender: Option<String>,
    #[serde(default)]
    pub occupation: Option<String>,
    #[serde(default)]
    pub habits: Vec<String>,
    #[serde(default)]
    pub goals: Vec<String>,
    #[serde(default)]
    pub pain_points: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub scenarios: Vec<String>,
    #[serde(default)]
    pub devices: Vec<String>,
    #[serde(default)]
    pub accessibility: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub confidence: Confidence,
    #[serde(default)]
    pub status: EntityStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Function {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub status: EntityStatus,
    #[serde(default)]
    pub operations: Vec<Operation>,
    #[serde(default)]
    pub stories: Vec<Story>,
    #[serde(default)]
    pub interactions: Vec<InteractionSpec>,
    #[serde(default)]
    pub interaction_flows: Vec<InteractionFlow>,
    #[serde(default)]
    pub screens: Vec<Screen>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub actor_user_ids: Vec<String>,
    #[serde(default)]
    pub trigger: Option<String>,
    #[serde(default)]
    pub preconditions: Vec<String>,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub states: Vec<String>,
    #[serde(default)]
    pub exceptions: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub side_effects: Vec<String>,
    #[serde(default)]
    pub audit: Vec<String>,
    #[serde(default)]
    pub interaction_id: Option<String>,
    #[serde(default)]
    pub status: EntityStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Story {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub user_ids: Vec<String>,
    #[serde(default)]
    pub scenario: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub preconditions: Vec<String>,
    #[serde(default)]
    pub postconditions: Vec<String>,
    #[serde(default)]
    pub operation_ids: Vec<String>,
    #[serde(default)]
    pub interaction_flow_id: Option<String>,
    #[serde(default)]
    pub screen_ids: Vec<String>,
    #[serde(default)]
    pub priority: StoryPriority,
    #[serde(default)]
    pub source_issue_ids: Vec<String>,
    #[serde(default)]
    pub acceptance_notes: Vec<String>,
    #[serde(default)]
    pub status: EntityStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InteractionSpec {
    pub id: String,
    pub operation_id: String,
    #[serde(default)]
    pub story_ids: Vec<String>,
    #[serde(default)]
    pub states: Vec<InteractionState>,
    #[serde(default)]
    pub feedback: Feedback,
    #[serde(default)]
    pub accessibility: Vec<String>,
    #[serde(default)]
    pub copy: CopySpec,
    #[serde(default)]
    pub prototype_ref: Option<DesignRef>,
    #[serde(default)]
    pub status: EntityStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InteractionState {
    pub state: String,
    #[serde(default)]
    pub user_sees: String,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub transitions: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feedback {
    #[serde(default)]
    pub success: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub loading: Option<String>,
    #[serde(default)]
    pub empty: Option<String>,
    #[serde(default)]
    pub permission: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopySpec {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub errors: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InteractionFlow {
    pub id: String,
    pub story_id: String,
    #[serde(default)]
    pub steps: Vec<InteractionStep>,
    #[serde(default)]
    pub status: EntityStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InteractionStep {
    pub operation_id: String,
    #[serde(default)]
    pub interaction_state: Option<String>,
    #[serde(default)]
    pub screen_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Screen {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub purpose: Option<String>,
    #[serde(default)]
    pub layout: Option<String>,
    #[serde(default)]
    pub components: Vec<String>,
    #[serde(default)]
    pub states: Vec<String>,
    #[serde(default)]
    pub operation_ids: Vec<String>,
    #[serde(default)]
    pub story_ids: Vec<String>,
    #[serde(default)]
    pub design_system_refs: Vec<String>,
    #[serde(default)]
    pub images: Vec<ImageAsset>,
    #[serde(default)]
    pub accessibility: Vec<String>,
    #[serde(default)]
    pub prototype_ref: Option<DesignRef>,
    #[serde(default)]
    pub status: EntityStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ImageType {
    #[default]
    Mockup,
    Screenshot,
    Wireframe,
    Reference,
    Icon,
    Illustration,
    Annotation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageStorage {
    pub provider: String,
    pub key: String,
    pub hash: String,
    pub mime: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageAsset {
    pub id: String,
    #[serde(default)]
    pub asset_type: ImageType,
    pub storage: ImageStorage,
    #[serde(default)]
    pub alt_text: Option<String>,
    #[serde(default)]
    pub caption: Option<String>,
    #[serde(default)]
    pub annotations: Vec<ImageAnnotation>,
    #[serde(default)]
    pub source: Option<DesignRef>,
    #[serde(default = "default_sensitivity")]
    pub sensitivity: String,
    #[serde(default)]
    pub status: EntityStatus,
}

fn default_sensitivity() -> String {
    "internal".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageAnnotation {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub text: String,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesignRef {
    #[serde(rename = "type")]
    pub kind: String,
    pub reference: String,
}

// ---------------------------------------------------------------------------
// Persistence shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementsDocumentDAO {
    pub id: Uuid,
    pub project_id: Uuid,
    pub current_version: u32,
    pub status: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RequirementsVersionDAO {
    pub document_id: Uuid,
    pub project_id: Uuid,
    pub version: u32,
    pub body: RequirementsDocumentBody,
    pub content_hash: String,
    pub created_by: Uuid,
    pub approved_by: Option<Uuid>,
    pub created_at: OffsetDateTime,
    pub published_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RequirementsDraftDAO {
    pub document_id: Uuid,
    pub project_id: Uuid,
    pub revision: u64,
    pub base_version: u32,
    pub body: RequirementsDocumentBody,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequirementsVersionRow {
    pub version: u32,
    pub created_by: Uuid,
    #[serde(with = "time::serde::rfc3339::option")]
    pub published_at: Option<OffsetDateTime>,
}

// ---------------------------------------------------------------------------
// Structured operations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequirementsOperation {
    pub op_id: Uuid,
    #[serde(flatten)]
    pub op: RequirementOp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequirementOp {
    UserAdd {
        user: UserModel,
    },
    UserUpdate {
        user: UserModel,
    },
    UserRemove {
        user_id: String,
    },
    FunctionAdd {
        function: Function,
    },
    FunctionUpdate {
        function: Function,
    },
    FunctionRemove {
        function_id: String,
    },
    OperationAdd {
        function_id: String,
        operation: Operation,
    },
    OperationUpdate {
        function_id: String,
        operation: Operation,
    },
    OperationRemove {
        function_id: String,
        operation_id: String,
    },
    StoryAdd {
        function_id: String,
        story: Story,
    },
    StoryUpdate {
        function_id: String,
        story: Story,
    },
    StoryRemove {
        function_id: String,
        story_id: String,
    },
    StorySetOperations {
        function_id: String,
        story_id: String,
        operation_ids: Vec<String>,
    },
    InteractionSet {
        function_id: String,
        spec: InteractionSpec,
    },
    InteractionFlowSet {
        function_id: String,
        flow: InteractionFlow,
    },
    ScreenAdd {
        function_id: String,
        screen: Screen,
    },
    ScreenUpdate {
        function_id: String,
        screen: Screen,
    },
    ScreenRemove {
        function_id: String,
        screen_id: String,
    },
    ScreenAddImage {
        function_id: String,
        screen_id: String,
        image: ImageAsset,
    },
    ScreenRemoveImage {
        function_id: String,
        screen_id: String,
        image_id: String,
    },
    ImageAnnotate {
        function_id: String,
        screen_id: String,
        image_id: String,
        annotation: ImageAnnotation,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RequirementOpError {
    #[error("operation {op_id}: {message}")]
    Invalid { op_id: Uuid, message: String },
}

pub fn apply_operations(
    body: &RequirementsDocumentBody,
    operations: &[RequirementsOperation],
) -> Result<RequirementsDocumentBody, RequirementOpError> {
    let mut body = body.clone();
    for operation in operations {
        apply_one(&mut body, &operation.op).map_err(|message| RequirementOpError::Invalid {
            op_id: operation.op_id,
            message,
        })?;
    }
    Ok(body)
}

fn apply_one(body: &mut RequirementsDocumentBody, op: &RequirementOp) -> Result<(), String> {
    match op {
        RequirementOp::UserAdd { user } => {
            if body.users.iter().any(|existing| existing.id == user.id) {
                return Err(format!("user {} already exists", user.id));
            }
            body.users.push(user.clone());
        }
        RequirementOp::UserUpdate { user } => {
            let target = body
                .users
                .iter_mut()
                .find(|existing| existing.id == user.id)
                .ok_or_else(|| format!("user {} not found", user.id))?;
            *target = user.clone();
        }
        RequirementOp::UserRemove { user_id } => {
            let before = body.users.len();
            body.users.retain(|user| user.id != *user_id);
            if body.users.len() == before {
                return Err(format!("user {user_id} not found"));
            }
        }
        RequirementOp::FunctionAdd { function } => {
            if body
                .functions
                .iter()
                .any(|existing| existing.id == function.id)
            {
                return Err(format!("function {} already exists", function.id));
            }
            body.functions.push(function.clone());
        }
        RequirementOp::FunctionUpdate { function } => {
            let target = find_function_mut(body, &function.id)?;
            *target = function.clone();
        }
        RequirementOp::FunctionRemove { function_id } => {
            let before = body.functions.len();
            body.functions
                .retain(|function| function.id != *function_id);
            if body.functions.len() == before {
                return Err(format!("function {function_id} not found"));
            }
        }
        RequirementOp::OperationAdd {
            function_id,
            operation,
        } => {
            let function = find_function_mut(body, function_id)?;
            if function
                .operations
                .iter()
                .any(|existing| existing.id == operation.id)
            {
                return Err(format!("operation {} already exists", operation.id));
            }
            function.operations.push(operation.clone());
        }
        RequirementOp::OperationUpdate {
            function_id,
            operation,
        } => {
            let function = find_function_mut(body, function_id)?;
            let target = function
                .operations
                .iter_mut()
                .find(|existing| existing.id == operation.id)
                .ok_or_else(|| format!("operation {} not found", operation.id))?;
            *target = operation.clone();
        }
        RequirementOp::OperationRemove {
            function_id,
            operation_id,
        } => {
            let function = find_function_mut(body, function_id)?;
            let before = function.operations.len();
            function
                .operations
                .retain(|operation| operation.id != *operation_id);
            if function.operations.len() == before {
                return Err(format!("operation {operation_id} not found"));
            }
        }
        RequirementOp::StoryAdd { function_id, story } => {
            let function = find_function_mut(body, function_id)?;
            if function
                .stories
                .iter()
                .any(|existing| existing.id == story.id)
            {
                return Err(format!("story {} already exists", story.id));
            }
            function.stories.push(story.clone());
        }
        RequirementOp::StoryUpdate { function_id, story } => {
            let function = find_function_mut(body, function_id)?;
            let target = function
                .stories
                .iter_mut()
                .find(|existing| existing.id == story.id)
                .ok_or_else(|| format!("story {} not found", story.id))?;
            *target = story.clone();
        }
        RequirementOp::StoryRemove {
            function_id,
            story_id,
        } => {
            let function = find_function_mut(body, function_id)?;
            let before = function.stories.len();
            function.stories.retain(|story| story.id != *story_id);
            if function.stories.len() == before {
                return Err(format!("story {story_id} not found"));
            }
        }
        RequirementOp::StorySetOperations {
            function_id,
            story_id,
            operation_ids,
        } => {
            let function = find_function_mut(body, function_id)?;
            let story = function
                .stories
                .iter_mut()
                .find(|story| story.id == *story_id)
                .ok_or_else(|| format!("story {story_id} not found"))?;
            story.operation_ids = operation_ids.clone();
        }
        RequirementOp::InteractionSet { function_id, spec } => {
            let function = find_function_mut(body, function_id)?;
            if let Some(existing) = function
                .interactions
                .iter_mut()
                .find(|interaction| interaction.id == spec.id)
            {
                *existing = spec.clone();
            } else {
                function.interactions.push(spec.clone());
            }
        }
        RequirementOp::InteractionFlowSet { function_id, flow } => {
            let function = find_function_mut(body, function_id)?;
            if let Some(existing) = function
                .interaction_flows
                .iter_mut()
                .find(|existing| existing.id == flow.id)
            {
                *existing = flow.clone();
            } else {
                function.interaction_flows.push(flow.clone());
            }
        }
        RequirementOp::ScreenAdd {
            function_id,
            screen,
        } => {
            let function = find_function_mut(body, function_id)?;
            if function
                .screens
                .iter()
                .any(|existing| existing.id == screen.id)
            {
                return Err(format!("screen {} already exists", screen.id));
            }
            function.screens.push(screen.clone());
        }
        RequirementOp::ScreenUpdate {
            function_id,
            screen,
        } => {
            let function = find_function_mut(body, function_id)?;
            let target = function
                .screens
                .iter_mut()
                .find(|existing| existing.id == screen.id)
                .ok_or_else(|| format!("screen {} not found", screen.id))?;
            *target = screen.clone();
        }
        RequirementOp::ScreenRemove {
            function_id,
            screen_id,
        } => {
            let function = find_function_mut(body, function_id)?;
            let before = function.screens.len();
            function.screens.retain(|screen| screen.id != *screen_id);
            if function.screens.len() == before {
                return Err(format!("screen {screen_id} not found"));
            }
        }
        RequirementOp::ScreenAddImage {
            function_id,
            screen_id,
            image,
        } => {
            let screen = find_screen_mut(body, function_id, screen_id)?;
            if screen.images.iter().any(|existing| existing.id == image.id) {
                return Err(format!("image {} already exists", image.id));
            }
            screen.images.push(image.clone());
        }
        RequirementOp::ScreenRemoveImage {
            function_id,
            screen_id,
            image_id,
        } => {
            let screen = find_screen_mut(body, function_id, screen_id)?;
            let before = screen.images.len();
            screen.images.retain(|image| image.id != *image_id);
            if screen.images.len() == before {
                return Err(format!("image {image_id} not found"));
            }
        }
        RequirementOp::ImageAnnotate {
            function_id,
            screen_id,
            image_id,
            annotation,
        } => {
            let screen = find_screen_mut(body, function_id, screen_id)?;
            let image = screen
                .images
                .iter_mut()
                .find(|image| image.id == *image_id)
                .ok_or_else(|| format!("image {image_id} not found"))?;
            if let Some(existing) = image
                .annotations
                .iter_mut()
                .find(|existing| existing.id == annotation.id)
            {
                *existing = annotation.clone();
            } else {
                image.annotations.push(annotation.clone());
            }
        }
    }
    Ok(())
}

fn find_function_mut<'a>(
    body: &'a mut RequirementsDocumentBody,
    function_id: &str,
) -> Result<&'a mut Function, String> {
    body.functions
        .iter_mut()
        .find(|function| function.id == function_id)
        .ok_or_else(|| format!("function {function_id} not found"))
}

fn find_screen_mut<'a>(
    body: &'a mut RequirementsDocumentBody,
    function_id: &str,
    screen_id: &str,
) -> Result<&'a mut Screen, String> {
    let function = find_function_mut(body, function_id)?;
    function
        .screens
        .iter_mut()
        .find(|screen| screen.id == screen_id)
        .ok_or_else(|| format!("screen {screen_id} not found"))
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

pub fn validate_requirements_document(body: &RequirementsDocumentBody) -> Validation {
    let mut validation = Validation::default();
    let mut user_ids = HashSet::new();
    let mut function_ids = HashSet::new();

    if body.schema_version != REQUIREMENTS_SCHEMA_VERSION {
        validation.errors.push(ValidationIssue::error(
            "schema_version",
            "unsupported_schema_version",
            format!(
                "schema_version must be {REQUIREMENTS_SCHEMA_VERSION}, got {}",
                body.schema_version
            ),
        ));
    }
    if body.users.is_empty() {
        validation.errors.push(ValidationIssue::error(
            "users",
            "no_users",
            "the document must declare at least one user model",
        ));
    }
    if body.functions.is_empty() {
        validation.errors.push(ValidationIssue::error(
            "functions",
            "no_functions",
            "the document must declare at least one function",
        ));
    }

    for (index, user) in body.users.iter().enumerate() {
        let path = format!("users[{index}]");
        if user.id.trim().is_empty() {
            validation.errors.push(ValidationIssue::error(
                format!("{path}.id"),
                "empty_id",
                "user id must not be empty",
            ));
        } else if !user_ids.insert(user.id.clone()) {
            validation.errors.push(ValidationIssue::error(
                format!("{path}.id"),
                "duplicate_id",
                format!("duplicate user id {}", user.id),
            ));
        }
        if user.name.trim().is_empty() {
            validation.errors.push(ValidationIssue::error(
                format!("{path}.name"),
                "empty_name",
                "user name must not be empty",
            ));
        }
        if user.goals.is_empty() {
            validation.warnings.push(ValidationIssue::error(
                format!("{path}.goals"),
                "no_goals",
                "user model has no goals",
            ));
        }
        if user.pain_points.is_empty() {
            validation.warnings.push(ValidationIssue::error(
                format!("{path}.pain_points"),
                "no_pain_points",
                "user model has no pain points",
            ));
        }
        if user.evidence.is_empty() {
            validation.warnings.push(ValidationIssue::error(
                format!("{path}.evidence"),
                "no_evidence",
                "user model has no evidence; confidence should be low",
            ));
        }
    }

    for (function_index, function) in body.functions.iter().enumerate() {
        let function_path = format!("functions[{function_index}]");
        if function.id.trim().is_empty() {
            validation.errors.push(ValidationIssue::error(
                format!("{function_path}.id"),
                "empty_id",
                "function id must not be empty",
            ));
        } else if !function_ids.insert(function.id.clone()) {
            validation.errors.push(ValidationIssue::error(
                format!("{function_path}.id"),
                "duplicate_id",
                format!("duplicate function id {}", function.id),
            ));
        }
        if function.name.trim().is_empty() {
            validation.errors.push(ValidationIssue::error(
                format!("{function_path}.name"),
                "empty_name",
                "function name must not be empty",
            ));
        }

        let operation_ids: HashSet<&str> = function
            .operations
            .iter()
            .map(|operation| operation.id.as_str())
            .collect();
        let story_ids: HashSet<&str> = function
            .stories
            .iter()
            .map(|story| story.id.as_str())
            .collect();
        let interaction_ids: HashSet<&str> = function
            .interactions
            .iter()
            .map(|interaction| interaction.id.as_str())
            .collect();
        let screen_ids: HashSet<&str> = function
            .screens
            .iter()
            .map(|screen| screen.id.as_str())
            .collect();

        let mut seen_operations = HashSet::new();
        for (index, operation) in function.operations.iter().enumerate() {
            let path = format!("{function_path}.operations[{index}]");
            if !seen_operations.insert(operation.id.clone()) {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.id"),
                    "duplicate_id",
                    format!("duplicate operation id {}", operation.id),
                ));
            }
            if operation.name.trim().is_empty() {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.name"),
                    "empty_name",
                    "operation name must not be empty",
                ));
            }
            if operation.actor_user_ids.is_empty() {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.actor_user_ids"),
                    "no_actor",
                    "operation must reference at least one user model",
                ));
            }
            for user_id in &operation.actor_user_ids {
                if !user_ids.contains(user_id) {
                    validation.errors.push(ValidationIssue::error(
                        format!("{path}.actor_user_ids"),
                        "unknown_user",
                        format!("unknown user model {user_id}"),
                    ));
                }
            }
            if operation.inputs.is_empty() {
                validation.warnings.push(ValidationIssue::error(
                    format!("{path}.inputs"),
                    "no_inputs",
                    "operation declares no inputs",
                ));
            }
            if operation.states.is_empty() {
                validation.warnings.push(ValidationIssue::error(
                    format!("{path}.states"),
                    "no_states",
                    "operation declares no states",
                ));
            }
            if let Some(interaction_id) = &operation.interaction_id
                && !interaction_ids.contains(interaction_id.as_str())
            {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.interaction_id"),
                    "unknown_interaction",
                    format!("unknown interaction {interaction_id}"),
                ));
            }
        }

        let mut seen_stories = HashSet::new();
        for (index, story) in function.stories.iter().enumerate() {
            let path = format!("{function_path}.stories[{index}]");
            if !seen_stories.insert(story.id.clone()) {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.id"),
                    "duplicate_id",
                    format!("duplicate story id {}", story.id),
                ));
            }
            if story.title.trim().is_empty() {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.title"),
                    "empty_title",
                    "story title must not be empty",
                ));
            }
            if story.scenario.trim().is_empty() {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.scenario"),
                    "empty_scenario",
                    "story scenario must not be empty",
                ));
            }
            if story.goal.trim().is_empty() {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.goal"),
                    "empty_goal",
                    "story goal must not be empty",
                ));
            }
            if story.user_ids.is_empty() {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.user_ids"),
                    "no_users",
                    "story must reference at least one user model",
                ));
            }
            for user_id in &story.user_ids {
                if !user_ids.contains(user_id) {
                    validation.errors.push(ValidationIssue::error(
                        format!("{path}.user_ids"),
                        "unknown_user",
                        format!("unknown user model {user_id}"),
                    ));
                }
            }
            if story.operation_ids.is_empty() {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.operation_ids"),
                    "no_operations",
                    "story must reference at least one operation",
                ));
            }
            for operation_id in &story.operation_ids {
                if !operation_ids.contains(operation_id.as_str()) {
                    validation.errors.push(ValidationIssue::error(
                        format!("{path}.operation_ids"),
                        "unknown_operation",
                        format!("unknown operation {operation_id} in the same function"),
                    ));
                }
            }
            if let Some(flow_id) = &story.interaction_flow_id
                && !function
                    .interaction_flows
                    .iter()
                    .any(|flow| flow.id == *flow_id)
            {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.interaction_flow_id"),
                    "unknown_interaction_flow",
                    format!("unknown interaction flow {flow_id}"),
                ));
            }
            for screen_id in &story.screen_ids {
                if !screen_ids.contains(screen_id.as_str()) {
                    validation.errors.push(ValidationIssue::error(
                        format!("{path}.screen_ids"),
                        "unknown_screen",
                        format!("unknown screen {screen_id}"),
                    ));
                }
            }
        }

        for (index, interaction) in function.interactions.iter().enumerate() {
            let path = format!("{function_path}.interactions[{index}]");
            if interaction.id.trim().is_empty() {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.id"),
                    "empty_id",
                    "interaction id must not be empty",
                ));
            }
            if !operation_ids.contains(interaction.operation_id.as_str()) {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.operation_id"),
                    "unknown_operation",
                    format!("unknown operation {}", interaction.operation_id),
                ));
            }
            if interaction.states.is_empty() {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.states"),
                    "no_states",
                    "interaction must declare at least one state",
                ));
            }
            for state in &interaction.states {
                if state.state.trim().is_empty() {
                    validation.errors.push(ValidationIssue::error(
                        format!("{path}.states"),
                        "empty_state",
                        "interaction state name must not be empty",
                    ));
                }
            }
            for story_id in &interaction.story_ids {
                if !story_ids.contains(story_id.as_str()) {
                    validation.errors.push(ValidationIssue::error(
                        format!("{path}.story_ids"),
                        "unknown_story",
                        format!("unknown story {story_id}"),
                    ));
                }
            }
        }

        for (index, flow) in function.interaction_flows.iter().enumerate() {
            let path = format!("{function_path}.interaction_flows[{index}]");
            if !story_ids.contains(flow.story_id.as_str()) {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.story_id"),
                    "unknown_story",
                    format!("unknown story {}", flow.story_id),
                ));
            }
            for (step_index, step) in flow.steps.iter().enumerate() {
                if !operation_ids.contains(step.operation_id.as_str()) {
                    validation.errors.push(ValidationIssue::error(
                        format!("{path}.steps[{step_index}].operation_id"),
                        "unknown_operation",
                        format!("unknown operation {}", step.operation_id),
                    ));
                }
                if let Some(screen_id) = &step.screen_id
                    && !screen_ids.contains(screen_id.as_str())
                {
                    validation.errors.push(ValidationIssue::error(
                        format!("{path}.steps[{step_index}].screen_id"),
                        "unknown_screen",
                        format!("unknown screen {screen_id}"),
                    ));
                }
            }
        }

        for (index, screen) in function.screens.iter().enumerate() {
            let path = format!("{function_path}.screens[{index}]");
            if screen.name.trim().is_empty() {
                validation.errors.push(ValidationIssue::error(
                    format!("{path}.name"),
                    "empty_name",
                    "screen name must not be empty",
                ));
            }
            if screen.operation_ids.is_empty() && screen.story_ids.is_empty() {
                validation.warnings.push(ValidationIssue::error(
                    path.clone(),
                    "orphan_screen",
                    "screen is not linked to any operation or story",
                ));
            }
            for operation_id in &screen.operation_ids {
                if !operation_ids.contains(operation_id.as_str()) {
                    validation.errors.push(ValidationIssue::error(
                        format!("{path}.operation_ids"),
                        "unknown_operation",
                        format!("unknown operation {operation_id}"),
                    ));
                }
            }
            for story_id in &screen.story_ids {
                if !story_ids.contains(story_id.as_str()) {
                    validation.errors.push(ValidationIssue::error(
                        format!("{path}.story_ids"),
                        "unknown_story",
                        format!("unknown story {story_id}"),
                    ));
                }
            }
            let mut image_ids = HashSet::new();
            for (image_index, image) in screen.images.iter().enumerate() {
                let image_path = format!("{path}.images[{image_index}]");
                if !image_ids.insert(image.id.clone()) {
                    validation.errors.push(ValidationIssue::error(
                        format!("{image_path}.id"),
                        "duplicate_id",
                        format!("duplicate image id {}", image.id),
                    ));
                }
                if image.storage.key.trim().is_empty() || image.storage.hash.trim().is_empty() {
                    validation.errors.push(ValidationIssue::error(
                        format!("{image_path}.storage"),
                        "invalid_storage",
                        "image storage key and hash must not be empty",
                    ));
                }
                if !allowed_image_mime(&image.storage.mime) {
                    validation.errors.push(ValidationIssue::error(
                        format!("{image_path}.storage.mime"),
                        "unsupported_mime",
                        format!("unsupported image mime {}", image.storage.mime),
                    ));
                }
                if image.alt_text.as_deref().unwrap_or("").trim().is_empty() {
                    validation.warnings.push(ValidationIssue::error(
                        format!("{image_path}.alt_text"),
                        "missing_alt_text",
                        "meaningful images should have alt text",
                    ));
                }
                for (annotation_index, annotation) in image.annotations.iter().enumerate() {
                    if !(0.0..=1.0).contains(&annotation.x) || !(0.0..=1.0).contains(&annotation.y)
                    {
                        validation.errors.push(ValidationIssue::error(
                            format!("{image_path}.annotations[{annotation_index}]"),
                            "invalid_annotation",
                            "annotation coordinates must be between 0 and 1",
                        ));
                    }
                    if let Some(operation_id) = &annotation.operation_id
                        && !operation_ids.contains(operation_id.as_str())
                    {
                        validation.errors.push(ValidationIssue::error(
                            format!("{image_path}.annotations[{annotation_index}].operation_id"),
                            "unknown_operation",
                            format!("unknown operation {operation_id}"),
                        ));
                    }
                }
            }
        }

        // Orphan operations are a warning: an operation may be a supporting
        // primitive not yet used by a story.
        let used_operations: HashSet<&str> = function
            .stories
            .iter()
            .flat_map(|story| story.operation_ids.iter().map(String::as_str))
            .collect();
        for (index, operation) in function.operations.iter().enumerate() {
            if !used_operations.contains(operation.id.as_str()) {
                validation.warnings.push(ValidationIssue::error(
                    format!("{function_path}.operations[{index}]"),
                    "orphan_operation",
                    format!("operation {} is not referenced by any story", operation.id),
                ));
            }
        }
    }

    validation
}

fn allowed_image_mime(mime: &str) -> bool {
    matches!(
        mime,
        "image/png" | "image/jpeg" | "image/webp" | "image/svg+xml"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str) -> UserModel {
        UserModel {
            id: id.to_string(),
            name: "Test User".to_string(),
            age: Some(30),
            gender: None,
            occupation: None,
            habits: vec![],
            goals: vec!["do the thing".to_string()],
            pain_points: vec!["the current way is slow".to_string()],
            permissions: vec![],
            scenarios: vec![],
            devices: vec![],
            accessibility: vec![],
            evidence: vec!["interview-1".to_string()],
            confidence: Confidence::Medium,
            status: EntityStatus::Draft,
        }
    }

    fn operation(id: &str, user_id: &str) -> Operation {
        Operation {
            id: id.to_string(),
            name: "Search".to_string(),
            description: None,
            actor_user_ids: vec![user_id.to_string()],
            trigger: Some("user searches".to_string()),
            preconditions: vec![],
            inputs: vec!["keyword".to_string()],
            outputs: vec!["results".to_string()],
            states: vec!["idle".to_string()],
            exceptions: vec![],
            permissions: vec![],
            side_effects: vec![],
            audit: vec![],
            interaction_id: None,
            status: EntityStatus::Draft,
        }
    }

    fn story(id: &str, user_id: &str, operation_id: &str) -> Story {
        Story {
            id: id.to_string(),
            title: "Search for a product".to_string(),
            user_ids: vec![user_id.to_string()],
            scenario: "A customer wants a product".to_string(),
            goal: "Find and open the product".to_string(),
            preconditions: vec![],
            postconditions: vec![],
            operation_ids: vec![operation_id.to_string()],
            interaction_flow_id: None,
            screen_ids: vec![],
            priority: StoryPriority::Must,
            source_issue_ids: vec![],
            acceptance_notes: vec![],
            status: EntityStatus::Draft,
        }
    }

    fn function() -> Function {
        Function {
            id: "func-shopping".to_string(),
            name: "Shopping".to_string(),
            description: None,
            owner: None,
            status: EntityStatus::Draft,
            operations: vec![],
            stories: vec![],
            interactions: vec![],
            interaction_flows: vec![],
            screens: vec![],
        }
    }

    #[test]
    fn add_story_operation_and_validate() {
        let operations = vec![
            RequirementsOperation {
                op_id: Uuid::now_v7(),
                op: RequirementOp::UserAdd { user: user("u1") },
            },
            RequirementsOperation {
                op_id: Uuid::now_v7(),
                op: RequirementOp::FunctionAdd {
                    function: function(),
                },
            },
            RequirementsOperation {
                op_id: Uuid::now_v7(),
                op: RequirementOp::OperationAdd {
                    function_id: "func-shopping".to_string(),
                    operation: operation("op-search", "u1"),
                },
            },
            RequirementsOperation {
                op_id: Uuid::now_v7(),
                op: RequirementOp::StoryAdd {
                    function_id: "func-shopping".to_string(),
                    story: story("story-search", "u1", "op-search"),
                },
            },
        ];
        let body = apply_operations(&RequirementsDocumentBody::default(), &operations).unwrap();
        assert_eq!(body.users.len(), 1);
        assert_eq!(body.functions.len(), 1);
        assert_eq!(body.functions[0].stories.len(), 1);
        assert_eq!(body.functions[0].operations.len(), 1);
        let validation = validate_requirements_document(&body);
        assert!(validation.is_valid(), "{:?}", validation.errors);
    }

    #[test]
    fn rejects_unknown_operation_reference() {
        let body = RequirementsDocumentBody {
            schema_version: REQUIREMENTS_SCHEMA_VERSION,
            users: vec![user("u1")],
            functions: vec![Function {
                operations: vec![operation("op-search", "u1")],
                stories: vec![story("story-search", "u1", "missing")],
                ..function()
            }],
        };
        let validation = validate_requirements_document(&body);
        assert!(!validation.is_valid());
        assert!(
            validation
                .errors
                .iter()
                .any(|issue| issue.code == "unknown_operation")
        );
    }

    #[test]
    fn image_requires_storage_and_alt_text_warning() {
        let body = RequirementsDocumentBody {
            schema_version: REQUIREMENTS_SCHEMA_VERSION,
            users: vec![user("u1")],
            functions: vec![Function {
                operations: vec![operation("op-search", "u1")],
                stories: vec![story("story-search", "u1", "op-search")],
                screens: vec![Screen {
                    id: "screen-search".to_string(),
                    name: "Search".to_string(),
                    purpose: None,
                    layout: None,
                    components: vec![],
                    states: vec![],
                    operation_ids: vec!["op-search".to_string()],
                    story_ids: vec!["story-search".to_string()],
                    design_system_refs: vec![],
                    images: vec![ImageAsset {
                        id: "img-1".to_string(),
                        asset_type: ImageType::Mockup,
                        storage: ImageStorage {
                            provider: "s3".to_string(),
                            key: "key".to_string(),
                            hash: "sha256:abc".to_string(),
                            mime: "image/png".to_string(),
                            width: Some(100),
                            height: Some(100),
                            size_bytes: Some(1000),
                        },
                        alt_text: None,
                        caption: None,
                        annotations: vec![],
                        source: None,
                        sensitivity: "internal".to_string(),
                        status: EntityStatus::Draft,
                    }],
                    accessibility: vec![],
                    prototype_ref: None,
                    status: EntityStatus::Draft,
                }],
                ..function()
            }],
        };
        let validation = validate_requirements_document(&body);
        assert!(validation.is_valid());
        assert!(
            validation
                .warnings
                .iter()
                .any(|issue| issue.code == "missing_alt_text")
        );
    }
}
