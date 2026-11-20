use super::ast::WorkflowBody;
use super::interpreter::DefinitionWorkflow;
use super::validate::validate;
use crate::RoleKind;
use std::collections::BTreeSet;

const PROJECT: &str = include_str!("../definitions/project.json");
const EMPTY: &str = include_str!("../definitions/empty.json");
const GOAL_CLARIFICATION: &str = include_str!("../definitions/goal_clarification.json");
const REQUIREMENTS_DISCOVERY: &str = include_str!("../definitions/requirements_discovery.json");
const REQUIREMENTS_ANALYSIS: &str = include_str!("../definitions/requirements_analysis.json");
const INTERACTION_DESIGN: &str = include_str!("../definitions/interaction_design.json");
const UI_DESIGN: &str = include_str!("../definitions/ui_design.json");
const ARCHITECTURE_DESIGN: &str = include_str!("../definitions/architecture_design.json");
const DETAILED_DESIGN: &str = include_str!("../definitions/detailed_design.json");
const TEST_PLANNING: &str = include_str!("../definitions/test_planning.json");
const TEST_EXECUTION: &str = include_str!("../definitions/test_execution.json");
const ROADMAP: &str = include_str!("../definitions/roadmap.json");
const ITERATION: &str = include_str!("../definitions/iteration.json");
const RELEASE: &str = include_str!("../definitions/release.json");
const PROJECT_REVIEW: &str = include_str!("../definitions/project_review.json");
const DESIGN_SPACE_MERGE: &str = include_str!("../definitions/design_space_merge.json");
const CODE_SUMMARY_GROUP: &str = include_str!("../definitions/code_summary_group.json");
const DESIGN_BASELINE: &str = include_str!("../definitions/design_baseline.json");
const DESIGN_ITERATION: &str = include_str!("../definitions/design_iteration.json");
const DEVELOPMENT_ITERATION: &str = include_str!("../definitions/development_iteration.json");
const DESIGN_CONSISTENCY_MAINTENANCE: &str =
    include_str!("../definitions/design_consistency_maintenance.json");
const ISSUE_TRIAGE: &str = include_str!("../definitions/issue_triage.json");
const TRIAGE: &str = include_str!("../definitions/triage.json");
const DESIGN_CHANGE: &str = include_str!("../definitions/design_change.json");
const DESIGN_CORRECTION: &str = include_str!("../definitions/design_correction.json");
const DESIGN: &str = include_str!("../definitions/design.json");
const CODE_CHANGE: &str = include_str!("../definitions/code_change.json");
const STYLE_INSPECTION: &str = include_str!("../definitions/style_inspection.json");
const LOGIC_INSPECTION: &str = include_str!("../definitions/logic_inspection.json");
const COHESION_INSPECTION: &str = include_str!("../definitions/cohesion_inspection.json");
const CONSISTENCY_INSPECTION: &str = include_str!("../definitions/consistency_inspection.json");
const DESIGN_INSPECTION: &str = include_str!("../definitions/design_inspection.json");
const DAILY_CHECK: &str = include_str!("../definitions/daily_check.json");
const SOFTWARE_DEVELOPMENT: &str = include_str!("../definitions/software_development.json");

const SOURCES: &[(&str, &str, &str)] = &[
    ("empty", "Empty (no-op)", EMPTY),
    ("project", "Project lifecycle", PROJECT),
    ("goal_clarification", "项目文档撰写", GOAL_CLARIFICATION),
    (
        "requirements_discovery",
        "Requirements discovery",
        REQUIREMENTS_DISCOVERY,
    ),
    (
        "requirements_analysis",
        "Requirements analysis",
        REQUIREMENTS_ANALYSIS,
    ),
    (
        "interaction_design",
        "Interaction design",
        INTERACTION_DESIGN,
    ),
    ("ui_design", "UI design", UI_DESIGN),
    (
        "architecture_design",
        "Architecture design",
        ARCHITECTURE_DESIGN,
    ),
    ("detailed_design", "Detailed design", DETAILED_DESIGN),
    ("test_planning", "Test planning", TEST_PLANNING),
    ("test_execution", "Test execution", TEST_EXECUTION),
    ("roadmap", "Roadmap", ROADMAP),
    ("iteration", "Iteration", ITERATION),
    ("release", "Release", RELEASE),
    ("project_review", "Project review", PROJECT_REVIEW),
    (
        "design_space_merge",
        "Design-space merge",
        DESIGN_SPACE_MERGE,
    ),
    (
        "code_summary_group",
        "Code summary group",
        CODE_SUMMARY_GROUP,
    ),
    (
        "design_consistency_maintenance",
        "Technical design consistency",
        DESIGN_CONSISTENCY_MAINTENANCE,
    ),
    ("design_baseline", "Design baseline", DESIGN_BASELINE),
    ("design_iteration", "Design iteration", DESIGN_ITERATION),
    ("development_iteration", "Development iteration", DEVELOPMENT_ITERATION),
    (
        "software_development",
        "Software development",
        SOFTWARE_DEVELOPMENT,
    ),
    ("daily_check", "Daily check", DAILY_CHECK),
    ("issue_triage", "Issue triage", ISSUE_TRIAGE),
    ("triage", "Issue triage", TRIAGE),
    ("design_change", "Design change", DESIGN_CHANGE),
    ("design_correction", "Design correction", DESIGN_CORRECTION),
    ("design", "Design", DESIGN),
    ("code_change", "Code change", CODE_CHANGE),
    (
        "style_inspection",
        "Code style inspection",
        STYLE_INSPECTION,
    ),
    (
        "logic_inspection",
        "Code logic inspection",
        LOGIC_INSPECTION,
    ),
    (
        "cohesion_inspection",
        "Code cohesion inspection",
        COHESION_INSPECTION,
    ),
    (
        "consistency_inspection",
        "Code-design consistency inspection",
        CONSISTENCY_INSPECTION,
    ),
    ("design_inspection", "Design inspection", DESIGN_INSPECTION),
];

/// Builtin workflows currently offered as project root tasks.
pub const ENABLED_ROOT_WORKFLOWS: &[&str] = &["software_development"];

/// Builtin workflows temporarily disabled. They stay in the catalog so
/// existing tasks can resolve their pinned versions, but they are hidden from
/// workflow listings and reject new task creation.
pub const DISABLED_WORKFLOWS: &[&str] = &[
    "code_summary_group",
    "design_consistency_maintenance",
    "issue_triage",
    "design_change",
    "roadmap",
    "release",
    "project_review",
    "requirements_discovery",
    "iteration",
];

pub fn is_enabled_root_workflow(id: &str) -> bool {
    ENABLED_ROOT_WORKFLOWS.contains(&id)
}

pub fn is_disabled_workflow(id: &str) -> bool {
    DISABLED_WORKFLOWS.contains(&id)
}

/// Every human identity declared by a builtin workflow role.
pub fn human_identities() -> Vec<String> {
    let mut identities = BTreeSet::new();
    for workflow in builtin_workflows() {
        for role in &workflow.body_ref().roles {
            if role.kind == RoleKind::Human {
                identities.insert(role.assignment.clone());
            }
        }
    }
    identities.into_iter().collect()
}

pub fn builtin_title(id: &str) -> Option<&'static str> {
    SOURCES
        .iter()
        .find(|(builtin_id, _, _)| *builtin_id == id)
        .map(|(_, title, _)| *title)
}

/// Parse, validate and construct the platform workflows.
///
/// The sources are embedded JSON owned by this crate; a parse or validation
/// failure is a build-content error and panics at startup rather than silently
/// running a partial catalog.
pub fn builtin_workflows() -> Vec<DefinitionWorkflow> {
    let bodies: Vec<(&str, WorkflowBody)> = SOURCES
        .iter()
        .map(|(id, _, source)| {
            let body = serde_json::from_str(source)
                .unwrap_or_else(|error| panic!("builtin workflow {id} is not valid JSON: {error}"));
            (*id, body)
        })
        .collect();
    let ids: Vec<&str> = bodies.iter().map(|(id, _)| *id).collect();
    let is_known = |id: &str| ids.contains(&id);

    for (id, body) in &bodies {
        let validation = validate(body, &is_known);
        assert!(
            validation.errors.is_empty(),
            "builtin workflow {id} is invalid: {:?}",
            validation.errors
        );
    }

    bodies
        .into_iter()
        .map(|(id, body)| DefinitionWorkflow::new(id, body))
        .collect()
}
