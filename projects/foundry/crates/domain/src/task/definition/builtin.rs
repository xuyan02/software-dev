use super::ast::WorkflowBody;
use super::interpreter::DefinitionWorkflow;
use super::validate::validate;

const PROJECT: &str = include_str!("../definitions/project.json");
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

const SOURCES: &[(&str, &str, &str)] = &[
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
];

pub fn builtin_title(id: &str) -> Option<&'static str> {
    SOURCES
        .iter()
        .find(|(builtin_id, _, _)| *builtin_id == id)
        .map(|(_, title, _)| *title)
}

/// Parse, validate and construct the nine platform workflows.
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
