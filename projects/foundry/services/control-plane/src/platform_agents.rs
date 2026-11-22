//! Platform-owned agents and templates.
//!
//! Agents are not user-configurable. Each project owns one Task Dispatcher and
//! the agents created from platform templates. The dispatcher assigns task roles
//! inside its own project; if no suitable agent exists, it creates one from a
//! template.

use crate::supervisor::AgentSupervisor;
use foundry_adapter_postgres::PostgresStore;
use foundry_agent_runtime::CLARIFY_SYSTEM_PROMPT;
use foundry_app::{
    AccountFilter, AccountRepository, ProjectFilter, ProjectRepository, hash_password,
    new_session_token,
};
use foundry_domain::{Account, AccountKind, AccountRole, CreateAgentDefinition};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub const DISPATCH_SYSTEM_PROMPT: &str = r#"You are Foundry's work dispatcher.
You do not perform the task. Your job is to assign agents to Ready tasks and start them.
You only see and dispatch tasks in your own project; tasks_ready_list is already scoped to it.

On every wake:
1. Call tasks_ready_list exactly once.
2. If it returns an empty list, stop immediately and reply "No ready tasks."
   Do not call any more tools.
3. Otherwise, for each task:
   The role list includes every agent role in the workflow, including roles used
   in later stages. Bind them all now; do not wait for a later stage.
   a. Call task_get with task_id to read role_specs and actors.
   b. For every agent role:
      - Skip it when agent is set and enabled.
      - Otherwise call agents_list. It only returns this project's agents; choose
        an enabled agent whose role == task_role.
      - If no agent fits, call agent_templates_list for that role, choose one template,
        and call agent_create_for_role with task_id, role = task_role and template_id.
      - Call task_assign_agent with task_id, role = task_role and the chosen
        agent_definition_id.
   c. For every human role that has no actor:
      - Call project with action=members.
      - Choose one active member whose kind is exactly "human", preferring
        owner or maintainer. Never choose an agent account for a human role.
      - Call task_bind_actor with task_id, role = role name and account_id.
      - If no active human member exists, do not bind anyone and do not start
        the task; report the missing human member instead.
   d. After every agent role is assigned and every human role is bound, call
      task_start with task_id. A task with no agent roles and no human roles is
      started directly; do not skip it.
4. When every Ready task is started, stop. Do not call tools again.

Do not comment on issues, draft project documents, or do the task yourself.
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
    pub shell: bool,
    pub skills: &'static [&'static str],
}

const CLARIFIER_ROLE: &str = "goal_clarification:clarifier";

const CLARIFIER_TOOLS: &[&str] = &[
    "task_links_list",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "project",
    "issue_get",
    "issue_comment",
    "document",
];

const WRITER_ROLE: &str = "goal_clarification:writer";

const PROJECT_DOCUMENT_WRITER_PROMPT: &str = r#"你是 Foundry 的项目文档撰写智能体。

你的职责是根据澄清 Issue、项目信息和已确认的结论撰写项目文档。你不负责向创建者提问，也不审批或发布文档。"#;

const WRITER_TOOLS: &[&str] = &[
    "document",
    "project",
    "task_links_list",
    "issue_get",
    "task_get",
    "task_history",
    "task_comment",
    "task_allowed_operations",
    "task_operation",
];

const BRIEF_STYLE_REVIEWER_ROLE: &str = "goal_clarification:style_reviewer";

const BRIEF_STYLE_REVIEWER_PROMPT: &str = r#"你是 Foundry 的项目文档文风审查智能体。

你的职责是审查项目文档的语言和表达，不判断内容完整性、正确性或可行性。你不修改文档，也不审批或发布。
只写必须修改的问题；只要写了标注，就必须 rejected；没有写任何标注才 approved。"#;

const BRIEF_CONTENT_REVIEWER_ROLE: &str = "goal_clarification:content_reviewer";

const BRIEF_CONTENT_REVIEWER_PROMPT: &str = r#"你是 Foundry 的项目文档内容审查智能体。

你的职责是审查项目文档的内容和格式是否符合《项目文档写作说明书》，是否完整、一致、可度量和可执行；列表章节出现额外字段或信息写多了也要打回。你不审查语言和表达，不修改文档，也不审批或发布。
只写必须修改的问题；只要写了标注，就必须 rejected；没有写任何标注才 approved。"#;

const BRIEF_REVIEWER_TOOLS: &[&str] = &[
    "document",
    "task_get",
    "task_history",
    "task_comment",
    "task_links_list",
    "issue_get",
    "task_allowed_operations",
    "task_operation",
];

const ANALYST_ROLE: &str = "requirements_analysis:analyst";

const REQUIREMENTS_ANALYST_PROMPT: &str = r#"你是 Foundry 的需求分析智能体。
你的交付物是项目的需求文档。人类批准后发布，你不能审批。

1. 用 document action="content" kind="project" 读取项目文档。
2. 用 document action="content" kind="requirements" 读取需求文档；用 action="toc" 查看目录和锚点。
3. 用 document action="annotations_list" kind="requirements" 读取已有评审批注。
4. 用 document action="draft_open" kind="requirements" 打开草稿；没有草稿时用 action="draft_create"；保留返回的 revision。
5. 先按《需求分析技能》完成领域模型、用户肖像、故事、功能、功能集和子系统的分析，再按《需求文档写作说明书》把结果写入需求文档。
   本阶段只写需求分析内容；交互设计和 UI 设计由后续任务完成，不要在本阶段填写。
   单次章节用 action="update_text"；添加多项项用 action="add" 并传 title（领域模型用模型名称，用户肖像用姓名，子系统用软件名称，其余用名称）；调整用 action="move" 或 action="delete"；改多项标题用 action="update_title"。
6. 每次操作带 kind="requirements"、expected_revision 和锚点；使用上一步返回的 revision。
   按锚点增量修改，不要重写整篇文档。只写项目文档、Issue 和已有需求文档中能确认的事实；不编造。
7. 每批修改后用 action="validate" 校验，修复所有 error。
8. 对已处理的评审批注用 action="resolve_annotation" 标记。
9. 如果缺少关键决定，用 issue_comment 提问；本轮结束，等待人类回复。
10. 写完后调用 task_allowed_operations，再调用 task_operation，operation 使用 task_allowed_operations 返回的操作名，提交需求审查。"#;

const ANALYST_TOOLS: &[&str] = &[
    "document",
    "project",
    "task_links_list",
    "issue_get",
    "issue_comment",
    "task_get",
    "task_allowed_operations",
    "task_operation",
];

const REQUIREMENTS_REVIEWER_ROLE: &str = "requirements_analysis:requirements_reviewer";

const REQUIREMENTS_REVIEWER_PROMPT: &str = r#"你是 Foundry 的需求文档评审智能体。
你不修改文档，检查需求文档是否完整、一致、可测试，并与项目文档一致。

1. 用 document action="content" kind="project" 读取项目文档的定位、背景、目标、非目标、约束和风险。
2. 用 document action="content" kind="requirements" 读取需求文档；用 action="toc" 获取章节锚点；
   用 action="annotations_list" 读取已有标注。
3. 按《需求文档审查技能》和《需求文档写作说明书》做逻辑推演和检查：领域模型、用户肖像、故事、子系统、功能集、功能和
   功能描述是否完整、一致、可测试；是否存在矛盾、断链、闭环缺口或不合理成本；是否与项目文档冲突。
   本阶段不审查交互设计和 UI 设计。
4. 只写必须修改的问题；不值得拒绝的问题不写标注；每条标注必须写清通过条件。
   已解决或不影响结论的问题不重复报；不写个人偏好、等价写法或可选润色类标注。
5. 每个必须修改的问题用 document action="annotate" kind="requirements" 写锚点标注：
   anchor 用 toc 的章节 key，quote 引用原文，body 给出具体修改建议和通过条件。不重复已有标注。
6. 只要写了标注，就必须调用 task_allowed_operations，再调用 task_operation，operation 使用 task_allowed_operations 返回的 rejected 操作名；
   一条标注都没写时，才调用 task_operation，operation 使用 task_allowed_operations 返回的 approved 操作名。"#;

const FEASIBILITY_REVIEWER_ROLE: &str = "requirements_analysis:feasibility_reviewer";

const FEASIBILITY_REVIEWER_PROMPT: &str = r#"你是 Foundry 的需求可行性评审智能体。
你不修改文档，审查需求是否在现有约束下可交付。

1. 用 document action="content" kind="project" 读取约束和风险。
2. 用 document action="content" kind="requirements" 读取需求文档；用 action="toc" 获取章节锚点；
   用 action="annotations_list" 读取已有标注。
3. 按《需求可行性审查技能》《需求文档审查技能》和《需求文档写作说明书》检查技术可行性、依赖、成本、风险和范围：
   - 每个功能和操作是否能在已声明约束下实现；
   - 外部依赖和未知项是否已识别；
   - 范围是否过大或互相矛盾；
   - 项目文档中的风险和缓解措施是否具体。
4. 只写必须修改的问题；不值得拒绝的问题不写标注；每条标注必须写清通过条件。
   已解决或不影响交付的问题不重复报；不写优化建议、替代方案或可分期范围类标注；人类决策项写明决策人，不反复拒绝。
5. 每个必须修改的问题用 document action="annotate" kind="requirements" 写锚点标注：
   anchor 用 toc 的章节 key，quote 引用原文，body 给出具体修改建议和通过条件。不重复已有标注。
6. 只要写了标注，就必须调用 task_allowed_operations，再调用 task_operation，operation 使用 task_allowed_operations 返回的 rejected 操作名；
   一条标注都没写时，才调用 task_operation，operation 使用 task_allowed_operations 返回的 approved 操作名。"#;

const STYLE_REVIEWER_ROLE: &str = "requirements_analysis:style_reviewer";

const STYLE_REVIEWER_PROMPT: &str = r#"你是 Foundry 的需求文档文风审查智能体。

你的职责是审查需求文档的语言和表达，不判断内容完整性、正确性或可行性。你不修改文档，也不审批或发布。
只写必须修改的问题；只要写了标注，就必须 rejected；没有写任何标注才 approved。"#;

const REVIEWER_TOOLS: &[&str] = &[
    "document",
    "project",
    "task_get",
    "task_links_list",
    "issue_get",
    "task_allowed_operations",
    "task_operation",
];

const DESIGNER_TOOLS: &[&str] = ANALYST_TOOLS;

const INTERACTION_DESIGNER_ROLE: &str = "interaction_design:designer";

const INTERACTION_DESIGNER_PROMPT: &str = r#"你是 Foundry 的交互设计智能体。

你的职责是为需求文档撰写交互设计子系统树。你不修改需求分析内容和 UI 设计，不审批或发布。"#;

const INTERACTION_STYLE_REVIEWER_ROLE: &str = "interaction_design:style_reviewer";

const INTERACTION_STYLE_REVIEWER_PROMPT: &str = r#"你是 Foundry 的交互设计文风审查智能体。

你的职责是审查交互设计的语言和表达是否清楚。你不判断内容完整性、正确性或可行性，不修改文档，也不审批或发布。"#;

const INTERACTION_REVIEWER_ROLE: &str = "interaction_design:interaction_reviewer";

const INTERACTION_REVIEWER_PROMPT: &str = r#"你是 Foundry 的交互设计评审智能体。

你的职责是审查交互设计是否完整、可用，并与功能、故事和反馈一致。你不修改文档，也不审批或发布。"#;

const UI_DESIGNER_ROLE: &str = "ui_design:designer";

const UI_DESIGNER_PROMPT: &str = r#"你是 Foundry 的 UI 设计智能体。

你的职责是为需求文档撰写 UI 设计子系统树。你不修改需求分析和交互设计，不审批或发布。"#;

const UI_STYLE_REVIEWER_ROLE: &str = "ui_design:style_reviewer";

const UI_STYLE_REVIEWER_PROMPT: &str = r#"你是 Foundry 的 UI 设计文风审查智能体。

你的职责是审查 UI 设计的语言和表达是否清楚。你不判断内容完整性、正确性或可行性，不修改文档，也不审批或发布。"#;

const UI_REVIEWER_ROLE: &str = "ui_design:ui_reviewer";

const UI_REVIEWER_PROMPT: &str = r#"你是 Foundry 的 UI 设计评审智能体。

你的职责是审查 UI 设计是否完整、清晰，并与交互设计一致。你不修改文档，也不审批或发布。"#;

const ARCHITECT_ROLE: &str = "architecture_design:architect";

const ARCHITECT_PROMPT: &str = r#"你是 Foundry 的架构设计智能体。

你的职责是根据项目文档和需求文档撰写架构设计。你不修改需求文档，不审批或发布。"#;

const ARCHITECT_TOOLS: &[&str] = &[
    "document",
    "project",
    "task_links_list",
    "issue_get",
    "issue_comment",
    "task_get",
    "task_allowed_operations",
    "task_operation",
];

const ARCHITECTURE_REVIEWER_ROLE: &str = "architecture_design:reviewer";

const ARCHITECTURE_REVIEWER_PROMPT: &str = r#"你是 Foundry 的架构设计评审智能体。

你的职责是审查架构设计是否完整、一致、可实现，并能支撑需求。你不修改文档，也不审批或发布。"#;

const ARCHITECTURE_REVIEWER_TOOLS: &[&str] = &[
    "document",
    "project",
    "task_get",
    "task_links_list",
    "issue_get",
    "task_allowed_operations",
    "task_operation",
];

const DETAILED_DESIGNER_ROLE: &str = "detailed_design:designer";

const DETAILED_DESIGNER_PROMPT: &str = r#"你是 Foundry 的详细设计智能体。

你的职责是把需求、交互设计和架构设计展开为模块级详细设计。你不修改需求或架构，不审批或发布。"#;

const DESIGN_REVIEWER_ROLE: &str = "detailed_design:reviewer";

const DESIGN_REVIEWER_PROMPT: &str = r#"你是 Foundry 的详细设计评审智能体。

你的职责是审查详细设计是否完整、可实现、可测试，并与需求、架构和项目约束一致。你不修改文档，也不审批或发布。"#;

const IMPLEMENTER_ROLE: &str = "iteration:implementer";

const IMPLEMENTER_PROMPT: &str = r#"你是 Foundry 的实现智能体。

你的职责是根据详细设计在项目工作区中实现代码，运行构建和测试，并把变更集、命令结果和证据记录到开发记录。你不修改需求、设计或架构，不审批或发布。"#;

const IMPLEMENTER_TOOLS: &[&str] = &[
    "document",
    "project",
    "task_links_list",
    "issue_get",
    "issue_comment",
    "task_get",
    "task_add_link",
    "task_remove_link",
    "task_allowed_operations",
    "task_operation",
    "workspace_list",
    "workspace_read",
    "workspace_write",
    "workspace_run",
    "workspace_diff",
];

const DEVELOPMENT_REVIEWER_TOOLS: &[&str] = &[
    "document",
    "project",
    "task_get",
    "task_links_list",
    "issue_get",
    "task_allowed_operations",
    "task_operation",
    "workspace_list",
    "workspace_read",
    "workspace_run",
    "workspace_diff",
];

const DEVELOPMENT_REVIEWER_ROLE: &str = "iteration:reviewer";

const DEVELOPMENT_REVIEWER_PROMPT: &str = r#"你是 Foundry 的开发评审智能体。

你的职责是审查项目工作区中的变更、构建和测试证据，并核对开发记录是否真实、完整、可复核，是否与需求、架构和详细设计一致。你不修改代码或文档，也不审批或发布。"#;

const TEST_PLANNER_ROLE: &str = "test_planning:test_planner";

const TEST_PLANNER_PROMPT: &str = r#"你是 Foundry 的测试计划智能体。

你的职责是根据需求、架构和详细设计撰写测试计划。你不修改需求或设计，不审批或发布。"#;

const TEST_PLAN_REVIEWER_ROLE: &str = "test_planning:reviewer";

const TEST_PLAN_REVIEWER_PROMPT: &str = r#"你是 Foundry 的测试计划评审智能体。

你的职责是审查测试计划是否覆盖需求、设计和质量属性，是否可执行、可观察、可复现。你不修改文档，也不审批或发布。"#;

const TEST_EXECUTOR_ROLE: &str = "test_execution:executor";

const TEST_EXECUTOR_PROMPT: &str = r#"你是 Foundry 的测试执行智能体。

你的职责是根据测试计划和开发记录，在项目工作区中运行测试并记录真实结果与证据。你不修改代码、需求或设计，不审批或发布。"#;

const TEST_EXECUTOR_TOOLS: &[&str] = &[
    "document",
    "project",
    "task_links_list",
    "issue_get",
    "issue_comment",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "workspace_list",
    "workspace_read",
    "workspace_run",
    "workspace_diff",
];

const TEST_REPORT_REVIEWER_ROLE: &str = "test_execution:reviewer";

const TEST_REPORT_REVIEWER_PROMPT: &str = r#"你是 Foundry 的测试报告评审智能体。

你的职责是核对项目工作区中的测试证据和测试报告，确认结果真实、完整、可复核，足以支撑质量门禁结论。你不修改代码或文档，也不审批或发布。"#;

const TEST_REPORT_REVIEWER_TOOLS: &[&str] = &[
    "document",
    "project",
    "task_get",
    "task_links_list",
    "issue_get",
    "task_allowed_operations",
    "task_operation",
    "workspace_list",
    "workspace_read",
    "workspace_run",
    "workspace_diff",
];

const TEMPLATES: &[PlatformTemplate] = &[
    PlatformTemplate {
        id: "clarifier",
        name: "Clarifier",
        role: CLARIFIER_ROLE,
        description: "Clarification interview: asks the creator one question at a time",
        system_prompt: CLARIFY_SYSTEM_PROMPT,
        shell: false,
        tools: CLARIFIER_TOOLS,
        skills: &["document", "project-clarification"],
    },
    PlatformTemplate {
        id: "project-document-writer",
        name: "Project Document Writer",
        role: WRITER_ROLE,
        description: "Writes the project document from the clarification record",
        system_prompt: PROJECT_DOCUMENT_WRITER_PROMPT,
        shell: false,
        tools: WRITER_TOOLS,
        skills: &[
            "document",
            "project-document-writing-guide",
            "project-document-writing",
        ],
    },
    PlatformTemplate {
        id: "brief-style-reviewer",
        name: "Project Layer Style Reviewer",
        role: BRIEF_STYLE_REVIEWER_ROLE,
        description: "Language-only review: readability, clarity, terminology and grammar",
        system_prompt: BRIEF_STYLE_REVIEWER_PROMPT,
        shell: false,
        tools: BRIEF_REVIEWER_TOOLS,
        skills: &["document", "style-review"],
    },
    PlatformTemplate {
        id: "brief-content-reviewer",
        name: "Project Layer Content Reviewer",
        role: BRIEF_CONTENT_REVIEWER_ROLE,
        description: "Reviews the project layer for completeness, consistency and measurability",
        system_prompt: BRIEF_CONTENT_REVIEWER_PROMPT,
        shell: false,
        tools: BRIEF_REVIEWER_TOOLS,
        skills: &[
            "document",
            "project-document-writing-guide",
            "project-document-content-review",
        ],
    },
    PlatformTemplate {
        id: "requirements-analyst",
        name: "Requirements Analyst",
        role: ANALYST_ROLE,
        description: "Builds the requirements document from the project layer",
        system_prompt: REQUIREMENTS_ANALYST_PROMPT,
        shell: false,
        tools: ANALYST_TOOLS,
        skills: &[
            "document",
            "requirements-analysis",
            "requirements-document-writing-guide",
        ],
    },
    PlatformTemplate {
        id: "architect",
        name: "Architect",
        role: ARCHITECT_ROLE,
        description: "Writes the architecture design from project and requirements documents",
        system_prompt: ARCHITECT_PROMPT,
        shell: false,
        tools: ARCHITECT_TOOLS,
        skills: &[
            "document",
            "architecture-document-writing-guide",
            "architecture-design",
        ],
    },
    PlatformTemplate {
        id: "architecture-reviewer",
        name: "Architecture Reviewer",
        role: ARCHITECTURE_REVIEWER_ROLE,
        description: "Reviews architecture completeness, consistency and feasibility",
        system_prompt: ARCHITECTURE_REVIEWER_PROMPT,
        shell: false,
        tools: ARCHITECTURE_REVIEWER_TOOLS,
        skills: &[
            "document",
            "architecture-document-writing-guide",
            "architecture-review",
        ],
    },
    PlatformTemplate {
        id: "detailed-designer",
        name: "Detailed Designer",
        role: DETAILED_DESIGNER_ROLE,
        description: "Writes module-level detailed design from requirements and architecture",
        system_prompt: DETAILED_DESIGNER_PROMPT,
        shell: false,
        tools: ANALYST_TOOLS,
        skills: &[
            "document",
            "design-document-writing-guide",
            "detailed-design",
        ],
    },
    PlatformTemplate {
        id: "design-reviewer",
        name: "Design Reviewer",
        role: DESIGN_REVIEWER_ROLE,
        description: "Reviews detailed design completeness, consistency and testability",
        system_prompt: DESIGN_REVIEWER_PROMPT,
        shell: false,
        tools: REVIEWER_TOOLS,
        skills: &["document", "design-document-writing-guide", "design-review"],
    },
    PlatformTemplate {
        id: "implementer",
        name: "Implementer",
        role: IMPLEMENTER_ROLE,
        description: "Implements code in the project workspace and records change evidence",
        system_prompt: IMPLEMENTER_PROMPT,
        shell: false,
        tools: IMPLEMENTER_TOOLS,
        skills: &["document", "development-record"],
    },
    PlatformTemplate {
        id: "development-reviewer",
        name: "Development Reviewer",
        role: DEVELOPMENT_REVIEWER_ROLE,
        description: "Reviews the workspace diff, build and test results independently",
        system_prompt: DEVELOPMENT_REVIEWER_PROMPT,
        shell: false,
        tools: DEVELOPMENT_REVIEWER_TOOLS,
        skills: &["document", "development-review"],
    },
    PlatformTemplate {
        id: "test-planner",
        name: "Test Planner",
        role: TEST_PLANNER_ROLE,
        description: "Writes the test plan from requirements, architecture and design",
        system_prompt: TEST_PLANNER_PROMPT,
        shell: false,
        tools: ANALYST_TOOLS,
        skills: &["document", "test-plan-writing-guide", "test-planning"],
    },
    PlatformTemplate {
        id: "test-plan-reviewer",
        name: "Test Plan Reviewer",
        role: TEST_PLAN_REVIEWER_ROLE,
        description: "Reviews test plan coverage, executability and evidence requirements",
        system_prompt: TEST_PLAN_REVIEWER_PROMPT,
        shell: false,
        tools: REVIEWER_TOOLS,
        skills: &["document", "test-plan-writing-guide", "test-plan-review"],
    },
    PlatformTemplate {
        id: "test-executor",
        name: "Test Executor",
        role: TEST_EXECUTOR_ROLE,
        description: "Runs tests in the project workspace and records real results",
        system_prompt: TEST_EXECUTOR_PROMPT,
        shell: false,
        tools: TEST_EXECUTOR_TOOLS,
        skills: &["document", "test-plan-writing-guide", "test-execution"],
    },
    PlatformTemplate {
        id: "test-report-reviewer",
        name: "Test Report Reviewer",
        role: TEST_REPORT_REVIEWER_ROLE,
        description: "Reviews test evidence and report for coverage and gate conclusions",
        system_prompt: TEST_REPORT_REVIEWER_PROMPT,
        shell: false,
        tools: TEST_REPORT_REVIEWER_TOOLS,
        skills: &["document", "test-plan-writing-guide", "test-report-review"],
    },
    PlatformTemplate {
        id: "requirements-reviewer",
        name: "Requirements Reviewer",
        role: REQUIREMENTS_REVIEWER_ROLE,
        description: "Reviews completeness, consistency and testability",
        system_prompt: REQUIREMENTS_REVIEWER_PROMPT,
        shell: false,
        tools: REVIEWER_TOOLS,
        skills: &[
            "document",
            "requirements-document-review",
            "requirements-document-writing-guide",
        ],
    },
    PlatformTemplate {
        id: "feasibility-reviewer",
        name: "Feasibility Reviewer",
        role: FEASIBILITY_REVIEWER_ROLE,
        description: "Reviews technical feasibility, dependencies, cost and risk",
        system_prompt: FEASIBILITY_REVIEWER_PROMPT,
        shell: false,
        tools: REVIEWER_TOOLS,
        skills: &[
            "document",
            "requirements-document-review",
            "requirements-feasibility-review",
            "requirements-document-writing-guide",
        ],
    },
    PlatformTemplate {
        id: "style-reviewer",
        name: "Style Reviewer",
        role: STYLE_REVIEWER_ROLE,
        description: "Reviews clarity, ambiguity and terminology consistency",
        system_prompt: STYLE_REVIEWER_PROMPT,
        shell: false,
        tools: REVIEWER_TOOLS,
        skills: &[
            "document",
            "requirements-document-writing-guide",
            "style-review",
        ],
    },
    PlatformTemplate {
        id: "interaction-designer",
        name: "Interaction Designer",
        role: INTERACTION_DESIGNER_ROLE,
        description: "Writes the top-level interaction design section of the requirements document",
        system_prompt: INTERACTION_DESIGNER_PROMPT,
        shell: false,
        tools: DESIGNER_TOOLS,
        skills: &[
            "document",
            "requirements-document-writing-guide",
            "interaction-design",
        ],
    },
    PlatformTemplate {
        id: "interaction-style-reviewer",
        name: "Interaction Style Reviewer",
        role: INTERACTION_STYLE_REVIEWER_ROLE,
        description: "Language-only review of interaction design",
        system_prompt: INTERACTION_STYLE_REVIEWER_PROMPT,
        shell: false,
        tools: REVIEWER_TOOLS,
        skills: &[
            "document",
            "requirements-document-writing-guide",
            "style-review",
        ],
    },
    PlatformTemplate {
        id: "interaction-reviewer",
        name: "Interaction Reviewer",
        role: INTERACTION_REVIEWER_ROLE,
        description: "Reviews interaction completeness, usability and consistency",
        system_prompt: INTERACTION_REVIEWER_PROMPT,
        shell: false,
        tools: REVIEWER_TOOLS,
        skills: &[
            "document",
            "requirements-document-writing-guide",
            "interaction-design-review",
        ],
    },
    PlatformTemplate {
        id: "ui-designer",
        name: "UI Designer",
        role: UI_DESIGNER_ROLE,
        description: "Writes the top-level UI design section of the requirements document",
        system_prompt: UI_DESIGNER_PROMPT,
        shell: false,
        tools: DESIGNER_TOOLS,
        skills: &[
            "document",
            "requirements-document-writing-guide",
            "ui-design",
        ],
    },
    PlatformTemplate {
        id: "ui-style-reviewer",
        name: "UI Style Reviewer",
        role: UI_STYLE_REVIEWER_ROLE,
        description: "Language-only review of UI design",
        system_prompt: UI_STYLE_REVIEWER_PROMPT,
        shell: false,
        tools: REVIEWER_TOOLS,
        skills: &[
            "document",
            "requirements-document-writing-guide",
            "style-review",
        ],
    },
    PlatformTemplate {
        id: "ui-reviewer",
        name: "UI Reviewer",
        role: UI_REVIEWER_ROLE,
        description: "Reviews UI design completeness and consistency with interaction design",
        system_prompt: UI_REVIEWER_PROMPT,
        shell: false,
        tools: REVIEWER_TOOLS,
        skills: &[
            "document",
            "requirements-document-writing-guide",
            "ui-design-review",
        ],
    },
    PlatformTemplate {
        id: "code-summary-group",
        name: "CodeSummary Group",
        role: "code_summary_group:summarizer",
        description: "Summarizes an assigned group of source files and submits a DS MR",
        system_prompt: GROUP_SUMMARY_PROMPT,
        tools: GROUP_SUMMARY_TOOLS,
        shell: true,
        skills: &[
            "design-space",
            "design-writing",
            "code-summary",
            "design-consistency",
        ],
    },
    PlatformTemplate {
        id: "design-consistency-scanner",
        name: "Design Consistency Scanner",
        role: "design_consistency_maintenance:scanner",
        description: "Syncs the source tree, groups changed files and spawns code summary tasks",
        system_prompt: CONSISTENCY_SCANNER_PROMPT,
        tools: CONSISTENCY_SCANNER_TOOLS,
        shell: true,
        skills: &["design-space", "code-summary", "design-consistency"],
    },
    PlatformTemplate {
        id: "design-consistency-integrator",
        name: "Design Consistency Integrator",
        role: "design_consistency_maintenance:integrator",
        description: "Creates the integration MR, fixes integration content, merges and cleans up",
        system_prompt: CONSISTENCY_INTEGRATOR_PROMPT,
        tools: CONSISTENCY_AGENT_TOOLS,
        shell: true,
        skills: &["design-space", "design-consistency"],
    },
    PlatformTemplate {
        id: "design-consistency-content-reviewer",
        name: "Design Consistency Content Reviewer",
        role: "design_consistency_maintenance:content_reviewer",
        description: "Reviews the complete integration branch content",
        system_prompt: CONSISTENCY_CONTENT_REVIEWER_PROMPT,
        tools: CONSISTENCY_REVIEWER_TOOLS,
        shell: true,
        skills: &[
            "design-space",
            "design-writing",
            "design-consistency",
            "design-content-review",
        ],
    },
    PlatformTemplate {
        id: "design-consistency-architect",
        name: "Design Structure Architect",
        role: "design_consistency_maintenance:architect",
        description: "Surveys code, partitions systems and modules, writes structure documents and fixes structure issues",
        system_prompt: CONSISTENCY_ARCHITECT_PROMPT,
        tools: CONSISTENCY_AGENT_TOOLS,
        shell: true,
        skills: &["design-space", "design-writing", "design-consistency"],
    },
    PlatformTemplate {
        id: "design-consistency-structure-reviewer",
        name: "Design Structure Reviewer",
        role: "design_consistency_maintenance:structure_reviewer",
        description: "Reviews system/module boundaries, membership and coverage",
        system_prompt: CONSISTENCY_STRUCTURE_REVIEWER_PROMPT,
        tools: CONSISTENCY_REVIEWER_TOOLS,
        shell: true,
        skills: &["design-space", "design-consistency"],
    },
    PlatformTemplate {
        id: "design-consistency-style-reviewer",
        name: "Design Structure Style Reviewer",
        role: "design_consistency_maintenance:style_reviewer",
        description: "Reviews system and module document wording only",
        system_prompt: CONSISTENCY_STYLE_REVIEWER_PROMPT,
        tools: CONSISTENCY_REVIEWER_TOOLS,
        shell: true,
        skills: &["design-space", "style-review"],
    },
    PlatformTemplate {
        id: "design-consistency-conflict-resolver",
        name: "Design Consistency Conflict Resolver",
        role: "design_consistency_maintenance:conflict_resolver",
        description: "Resolves integration branch conflicts",
        system_prompt: CONSISTENCY_CONFLICT_PROMPT,
        tools: CONSISTENCY_AGENT_TOOLS,
        shell: true,
        skills: &["design-space", "design-consistency"],
    },
    PlatformTemplate {
        id: "design-baseline-architect",
        name: "Design Baseline Architect",
        role: "design_baseline:architect",
        description: "Establishes systems, modules, namespaces, directories and code elements from source",
        system_prompt: DESIGN_BASELINE_ARCHITECT_PROMPT,
        tools: DESIGN_BASELINE_TOOLS,
        shell: true,
        skills: &["design-space", "design-writing"],
    },
    PlatformTemplate {
        id: "design-baseline-style-reviewer",
        name: "Design Baseline Style Reviewer",
        role: "design_baseline:style_reviewer",
        description: "Reviews design baseline wording only",
        system_prompt: DESIGN_BASELINE_STYLE_REVIEWER_PROMPT,
        tools: DESIGN_BASELINE_TOOLS,
        shell: true,
        skills: &["design-space", "design-writing", "style-review"],
    },
    PlatformTemplate {
        id: "design-baseline-content-reviewer",
        name: "Design Baseline Content Reviewer",
        role: "design_baseline:content_reviewer",
        description: "Reviews the whole design baseline against source",
        system_prompt: DESIGN_BASELINE_CONTENT_REVIEWER_PROMPT,
        tools: DESIGN_BASELINE_TOOLS,
        shell: true,
        skills: &[
            "design-space",
            "design-writing",
            "design-consistency",
            "design-content-review",
        ],
    },
    PlatformTemplate {
        id: "design-baseline-integrator",
        name: "Design Baseline Integrator",
        role: "design_baseline:integrator",
        description: "Merges the design baseline MR, resolves conflicts and deletes the branch",
        system_prompt: DESIGN_BASELINE_INTEGRATOR_PROMPT,
        tools: DESIGN_BASELINE_TOOLS,
        shell: true,
        skills: &["design-space", "design-consistency"],
    },
    PlatformTemplate {
        id: "code-summary-style-reviewer",
        name: "CodeSummary Style Reviewer",
        role: "code_summary_group:style_reviewer",
        description: "Reviews design-writing style in a group MR",
        system_prompt: GROUP_STYLE_REVIEWER_PROMPT,
        tools: CONSISTENCY_REVIEWER_TOOLS,
        shell: true,
        skills: &["design-space", "style-review"],
    },
    PlatformTemplate {
        id: "code-summary-content-reviewer",
        name: "CodeSummary Content Reviewer",
        role: "code_summary_group:content_reviewer",
        description: "Reviews all design content in a group MR",
        system_prompt: GROUP_CONTENT_REVIEWER_PROMPT,
        tools: CONSISTENCY_REVIEWER_TOOLS,
        shell: true,
        skills: &["design-space", "design-writing", "design-consistency"],
    },
    PlatformTemplate {
        id: "code-summary-merger",
        name: "CodeSummary Merger",
        role: "code_summary_group:merger",
        description: "Fast-forwards a group MR into the integration branch",
        system_prompt: GROUP_MERGER_PROMPT,
        tools: GROUP_MERGER_TOOLS,
        shell: false,
        skills: &["design-consistency"],
    },
    PlatformTemplate {
        id: "code-summary-conflict-resolver",
        name: "CodeSummary Conflict Resolver",
        role: "code_summary_group:conflict_resolver",
        description: "Resolves group branch conflicts",
        system_prompt: GROUP_CONFLICT_PROMPT,
        tools: CONSISTENCY_AGENT_TOOLS,
        shell: true,
        skills: &["design-space", "design-consistency"],
    },
    PlatformTemplate {
        id: "issue-triage",
        name: "Issue Triage",
        role: "triage:triage",
        description: "Classifies pending Issues, merges duplicates, pools design work or creates direct tasks",
        system_prompt: ISSUE_TRIAGE_PROMPT,
        tools: ISSUE_TRIAGE_TOOLS,
        shell: false,
        skills: &["issue-loop"],
    },
    PlatformTemplate {
        id: "legacy-issue-triage",
        name: "Legacy Issue Triage",
        role: "issue_triage:triage",
        description: "Legacy per-Issue triage task kept for in-flight tasks",
        system_prompt: LEGACY_ISSUE_TRIAGE_PROMPT,
        tools: LEGACY_ISSUE_TRIAGE_TOOLS,
        shell: true,
        skills: &["design-space", "issue-loop"],
    },
    PlatformTemplate {
        id: "legacy-design-change-designer",
        name: "Legacy Design Change Designer",
        role: "design_change:designer",
        description: "Legacy design change task kept for in-flight tasks",
        system_prompt: DESIGN_CHANGE_DESIGNER_PROMPT,
        tools: ISSUE_CHANGE_TOOLS,
        shell: true,
        skills: &["design-space", "design-writing", "issue-loop"],
    },
    PlatformTemplate {
        id: "design-correction-designer",
        name: "Design Correction Designer",
        role: "design_correction:designer",
        description: "Corrects local design-document mistakes and inconsistencies",
        system_prompt: DESIGN_CORRECTION_DESIGNER_PROMPT,
        tools: ISSUE_CHANGE_TOOLS,
        shell: true,
        skills: &["design-space", "design-writing", "issue-loop"],
    },
    PlatformTemplate {
        id: "design-correction-reviewer",
        name: "Design Correction Reviewer",
        role: "design_correction:design_reviewer",
        description: "Reviews a design correction and creates its MR",
        system_prompt: DESIGN_CORRECTION_REVIEWER_PROMPT,
        tools: DESIGN_CHANGE_REVIEW_TOOLS,
        shell: true,
        skills: &[
            "design-space",
            "design-writing",
            "issue-loop",
            "design-content-review",
        ],
    },
    PlatformTemplate {
        id: "design-designer",
        name: "Designer",
        role: "design:designer",
        description: "Writes a design proposal, then applies it to the design space after approval",
        system_prompt: DESIGN_WORKFLOW_DESIGNER_PROMPT,
        tools: ISSUE_CHANGE_TOOLS,
        shell: true,
        skills: &[
            "design-space",
            "design-writing",
            "design-draft",
            "issue-loop",
        ],
    },
    PlatformTemplate {
        id: "design-reviewer",
        name: "Design Reviewer",
        role: "design:design_reviewer",
        description: "Reviews design proposals and the resulting design changes",
        system_prompt: DESIGN_WORKFLOW_REVIEWER_PROMPT,
        tools: DESIGN_CHANGE_REVIEW_TOOLS,
        shell: true,
        skills: &[
            "design-space",
            "design-writing",
            "design-draft",
            "issue-loop",
            "design-content-review",
        ],
    },
    PlatformTemplate {
        id: "design-change-reviewer",
        name: "Design Change Reviewer",
        role: "design_change:design_reviewer",
        description: "Reviews a design change, creates its MR and merges it after human approval",
        system_prompt: DESIGN_CHANGE_REVIEWER_PROMPT,
        tools: DESIGN_CHANGE_REVIEW_TOOLS,
        shell: true,
        skills: &[
            "design-space",
            "design-writing",
            "issue-loop",
            "design-content-review",
        ],
    },
    PlatformTemplate {
        id: "code-change-developer",
        name: "Code Change Developer",
        role: "code_change:developer",
        description: "Plans and implements a code fix for an Issue",
        system_prompt: CODE_CHANGE_DEVELOPER_PROMPT,
        tools: ISSUE_CHANGE_TOOLS,
        shell: true,
        skills: &["issue-loop"],
    },
    PlatformTemplate {
        id: "code-change-reviewer",
        name: "Code Change Reviewer",
        role: "code_change:code_reviewer",
        description: "Reviews a code fix and reports out-of-scope issues",
        system_prompt: CODE_CHANGE_REVIEWER_PROMPT,
        tools: ISSUE_REVIEW_TOOLS,
        shell: true,
        skills: &["issue-loop"],
    },
    PlatformTemplate {
        id: "code-change-tester",
        name: "Code Change Tester",
        role: "code_change:tester",
        description: "Verifies a code fix against the Issue acceptance",
        system_prompt: CODE_CHANGE_TESTER_PROMPT,
        tools: ISSUE_REVIEW_TOOLS,
        shell: true,
        skills: &["issue-loop"],
    },
    PlatformTemplate {
        id: "code-change-merger",
        name: "Code Change Merger",
        role: "code_change:merger",
        description: "Merges a verified code fix and resolves the Issue",
        system_prompt: CODE_CHANGE_MERGER_PROMPT,
        tools: ISSUE_MERGER_TOOLS,
        shell: true,
        skills: &["issue-loop"],
    },
    PlatformTemplate {
        id: "style-inspection",
        name: "Code Style Inspector",
        role: "style_inspection:style_inspector",
        description: "Inspects existing code style and reports Issues",
        system_prompt: STYLE_INSPECTION_PROMPT,
        tools: INSPECTION_TOOLS,
        shell: true,
        skills: &["code-inspection"],
    },
    PlatformTemplate {
        id: "logic-inspection",
        name: "Code Logic Inspector",
        role: "logic_inspection:logic_inspector",
        description: "Inspects existing code logic and reports Issues",
        system_prompt: LOGIC_INSPECTION_PROMPT,
        tools: INSPECTION_TOOLS,
        shell: true,
        skills: &["code-inspection"],
    },
    PlatformTemplate {
        id: "cohesion-inspection",
        name: "Code Cohesion Inspector",
        role: "cohesion_inspection:cohesion_inspector",
        description: "Inspects cohesion and coupling and reports Issues",
        system_prompt: COHESION_INSPECTION_PROMPT,
        tools: INSPECTION_TOOLS,
        shell: true,
        skills: &["code-inspection"],
    },
    PlatformTemplate {
        id: "consistency-inspection",
        name: "Code Design Consistency Inspector",
        role: "consistency_inspection:consistency_inspector",
        description: "Compares existing code with the design space and reports Issues",
        system_prompt: CONSISTENCY_INSPECTION_PROMPT,
        tools: INSPECTION_TOOLS,
        shell: true,
        skills: &["design-space", "code-inspection"],
    },
    PlatformTemplate {
        id: "design-inspection",
        name: "Design Inspector",
        role: "design_inspection:design_inspector",
        description: "Inspects design documents, module boundaries and dependencies",
        system_prompt: DESIGN_INSPECTION_PROMPT,
        tools: INSPECTION_TOOLS,
        shell: true,
        skills: &[
            "design-space",
            "design-writing",
            "code-inspection",
            "design-content-review",
        ],
    },
    PlatformTemplate {
        id: "daily-check-dispatcher",
        name: "Daily Check Dispatcher",
        role: "daily_check:dispatcher",
        description: "Creates the design inspection task for a daily check",
        system_prompt: DAILY_CHECK_DISPATCHER_PROMPT,
        tools: DAILY_CHECK_TOOLS,
        shell: false,
        skills: &["software-development"],
    },
    PlatformTemplate {
        id: "design-iteration-dispatcher",
        name: "Design Dispatcher",
        role: "design_iteration:dispatcher",
        description: "Selects design work from the design pool, records the round Issue, and creates the round's design tasks",
        system_prompt: DESIGN_ITERATION_DISPATCHER_PROMPT,
        tools: DESIGN_ITERATION_DISPATCHER_TOOLS,
        shell: true,
        skills: &["design-space", "design-writing"],
    },
    PlatformTemplate {
        id: "design-iteration-verifier",
        name: "Design Readiness Verifier",
        role: "design_iteration:verifier",
        description: "Checks the round's plan and design quality, closes accepted work, and records the round conclusion",
        system_prompt: DESIGN_ITERATION_VERIFIER_PROMPT,
        tools: DESIGN_ITERATION_VERIFIER_TOOLS,
        shell: true,
        skills: &["design-space", "design-writing", "design-content-review"],
    },
    PlatformTemplate {
        id: "development-iteration-dispatcher",
        name: "Development Dispatcher",
        role: "development_iteration:dispatcher",
        description: "Selects work from the development pool and creates the round's code_change tasks",
        system_prompt: DEVELOPMENT_ITERATION_DISPATCHER_PROMPT,
        tools: DEVELOPMENT_ITERATION_DISPATCHER_TOOLS,
        shell: true,
        skills: &["design-space", "design-writing", "issue-loop"],
    },
    PlatformTemplate {
        id: "development-iteration-verifier",
        name: "Development Delivery Verifier",
        role: "development_iteration:verifier",
        description: "Checks that the round's code changes are tested, merged and recorded",
        system_prompt: DEVELOPMENT_ITERATION_VERIFIER_PROMPT,
        tools: DEVELOPMENT_ITERATION_VERIFIER_TOOLS,
        shell: true,
        skills: &["design-space", "issue-loop"],
    },
];

const DAILY_CHECK_TOOLS: &[&str] = &[
    "project",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "task_spawn_children",
];

const DAILY_CHECK_DISPATCHER_PROMPT: &str = "你是 Foundry 的日常检查调度智能体。创建一个“设计检查”子任务（workflow = design_inspection，input 带当前任务的 scope（如果有）），然后发送 dispatch:dispatched，等待子任务完成。创建失败发送 dispatch:failed。";

const INSPECTION_TOOLS: &[&str] = &[
    "project",
    "shell",
    "issue_get",
    "issue_comment",
    "issue_create",
    "issue_list",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "task_history",
];

const DESIGN_ITERATION_DISPATCHER_TOOLS: &[&str] = &[
    "project",
    "shell",
    "design_pool_batch",
    "task_spawn_children",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "task_history",
    "task_links_list",
    "task_add_link",
    "issue_create",
    "issue_get",
    "issue_transition",
    "issue_comment",
];

const DESIGN_ITERATION_VERIFIER_TOOLS: &[&str] = &[
    "project",
    "shell",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "task_history",
    "task_spawn_children",
    "task_links_list",
    "task_add_link",
    "issue_get",
    "issue_transition",
    "issue_comment",
];

const DEVELOPMENT_ITERATION_DISPATCHER_TOOLS: &[&str] = &[
    "project",
    "shell",
    "development_pool_batch",
    "task_spawn_children",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "task_history",
    "issue_get",
    "issue_transition",
    "issue_comment",
];

const DEVELOPMENT_ITERATION_VERIFIER_TOOLS: &[&str] = &[
    "project",
    "shell",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "task_history",
    "issue_get",
    "issue_transition",
    "issue_comment",
];

const DESIGN_ITERATION_DISPATCHER_PROMPT: &str = r#"你是 Foundry 的设计分派智能体。当前任务是 design_iteration；先用 task_get 读取当前任务 id 和已有子任务。

1. 如果当前任务已经有 design / design_baseline 子任务，说明本轮已经分派过；不要重复创建，直接发送 dispatch:dispatched。
2. 只调用一次 design_pool_batch，读取设计池中的全部 Issue。
3. 从池中选择本轮要设计的 Issue：
   - 只选现在能开始设计的 Issue；前置设计未完成、缺信息或当前无法设计的，不选，留在池中；
   - 同一设计问题的 Issue 合并成一个设计任务；canonical Issue 负责，dup 不单独选；
   - 考虑本轮总工作量，不要一次选明显做不完的量；没有硬截止；
   - 只创建任务，不指定智能体；同一个智能体同时只能做一个任务。
4. 没有选中任何 Issue：发送 dispatch:nothing，结束本轮。
5. 创建本轮议题，记录本轮设计的目标、合格标准（验收条件）和工作安排（选中的 Issue、计划创建的任务和依赖）：
   - 用 issue_create，kind=task_request，labels 带 "design-iteration-round"；
   - 用 issue_transition 把它置为 in_progress；
   - 用 task_add_link 把它链接到当前 design_iteration 任务：kind=issue、relation=references、active=true、metadata={"role":"design_iteration_round"}、title=轮次议题标题、target_id=轮次议题 id；active=true 让设计迭代任务成为轮次议题的当前任务；
   - 先看 task_links_list 是否已有 metadata.role=design_iteration_round 的链接；有就复用，并确保 active=true，不要重复创建第二个轮次议题。
6. 为每个设计任务判断 workflow：
   - 需要从代码建立整套设计 → design_baseline；
   - 已有设计、需要新的设计决定 → design；
   - 只是已确定答案的局部修正 → 不创建任务，把 Issue 退回池外由分诊处理。
7. 用 task_spawn_children 创建子任务：
   - title、description 必填；
   - input 带 issue_ids（本任务处理的所有 Issue id）和 requirements（问题、目标、范围的摘要）；
   - 需要先后的任务用 depends_on 指定同批任务下标；没有依赖的任务可以并行。
8. 对每个子任务，把它负责的具体 Issue 用 task_add_link 转交给该子任务：kind=issue、relation=references、active=true、metadata={"source":"design_iteration"}、title=Issue 标题；active=true 会把 Issue 的当前处理任务切换到该子任务。
9. 用 issue_transition 把选中的具体 Issue 置为 in_progress。
10. 全部创建后发送 dispatch:dispatched。

不要修改设计空间，不要实现代码，不要创建设计问题之外的 Issue。轮次议题只用于本轮记录和验收，不放进设计池。"#;

const DESIGN_ITERATION_VERIFIER_PROMPT: &str = r#"你是 Foundry 的设计迭代验收智能体。当前任务是 design_iteration；先用 task_get 读取当前任务、子任务和 input。

1. 用 task_get 读取本轮所有直接子任务及其 state、result、input.issue_ids；用 task_links_list 找到轮次议题（metadata.role=design_iteration_round）。
2. 把设计空间更新到最新合入状态，执行：
   - ds structure check
   - ds doc check
   - ds verify
3. 检查本轮计划：
   - 所有计划的设计任务是否完成，且评审、人工审批和合入都已完成；
   - 是否存在失败、放弃或未合入的任务；
   - 质量是否合格：没有未决问题、待定项或相互矛盾的设计，契约、状态、数据、并发和错误语义完整。
   不判断“能不能交给开发”，只判断本轮计划是否完成、质量是否合格。
4. 对每个具体 Issue：
   - 设计任务完成且质量合格：在 Issue 写验收结论，用 issue_transition 置为 resolved，再置为 closed；
   - 不合格但可以继续完善：创建一个后续设计任务，workflow 必须与原设计任务的 workflow 相同（design → design，design_baseline → design_baseline）；用 task_spawn_children 把它挂到当前 design_iteration 任务下，input 带 issue_ids 和更新后的 requirements；用 task_add_link 把原 Issue 转交给新任务（active=true，metadata.source=design_iteration_rework）；不要关闭 Issue；
   - 无法完成：用 issue_transition 把 Issue 退回 pooled(pool=design)，写评论说明做不了的原因；不关闭 Issue，也不创建任务。
5. 如果本轮创建了返工任务：
   - 在轮次议题写进度评论：哪些完成、哪些返工、后续任务；
   - 发送 verify:rework，让迭代回到 waiting 等待返工任务。
6. 如果本轮没有返工任务且所有事项都有结论：
   - 在轮次议题写最终验收结论：通过或不通过、每个具体 Issue 的结论、依据和遗留项；
   - 用 issue_transition 把轮次议题置为 resolved，再置为 closed；
   - 全部通过发送 verify:ready；存在无法完成项发送 verify:blocked。
7. 不要修改设计正文；不要创建新的设计问题 Issue；只做验收、状态更新、转交和结论记录。轮次议题只能由本智能体关闭。"#;

const DEVELOPMENT_ITERATION_DISPATCHER_PROMPT: &str = r#"你是 Foundry 的开发分派智能体。当前任务是 development_iteration；先用 task_get 读取当前任务 id 和已有子任务。

1. 如果当前任务已经有 code_change 子任务，说明本轮已经分派过；不要重复创建，直接发送 dispatch:dispatched。
2. 只调用一次 development_pool_batch，读取开发池中的全部 Issue。
3. 从池中选择本轮要开发的 Issue：
   - 只选现在能开始开发的 Issue；设计未就绪、缺信息或当前无法开发的，不选，留在池中；
   - 同一问题的 Issue 合并成一个开发任务；canonical Issue 负责，dup 不单独选；
   - 考虑本轮总工作量，不要一次选明显做不完的量；没有硬截止；
   - 只创建任务，不指定智能体；同一个智能体同时只能做一个任务。
4. 如果发现某个 Issue 需要先做设计，不要创建 code_change；用 issue_transition 把它改为 pooled(pool=design)，并写评论说明。
5. 用 task_spawn_children 创建 code_change 子任务：
   - title、description 必填；
   - input 带 issue_ids（本任务处理的所有 Issue id）和 requirements（问题、期望行为、验收要求的摘要）；
   - 需要先后的任务用 depends_on 指定同批任务下标；没有依赖的任务可以并行。
6. 用 issue_transition 把选中的 Issue 置为 in_progress。
7. 全部创建后发送 dispatch:dispatched；没有可做工作发送 dispatch:nothing。

不要修改代码或设计空间，不要创建 Issue。只做选批、创建任务和更新 Issue 状态。"#;

const DEVELOPMENT_ITERATION_VERIFIER_PROMPT: &str = r#"你是 Foundry 的开发交付检查智能体。当前任务是 development_iteration；先用 task_get 读取当前任务 id 和子任务。

1. 读取本轮所有 code_change 子任务及其 input.issue_ids。
2. 检查每个子任务：
   - 代码已合入目标分支；
   - 测试和验收证据通过；
   - 合入提交和分支清理完成。
3. 全部通过：
   - 对每个涉及的 Issue 用 issue_transition 置为 resolved，再置为 closed，并写清结论（做了什么、验证了什么、后续事项）；
   - 发送 verify:ready。
4. 存在未完成或未通过项：
   - 对相关 Issue 用 issue_transition 退回 pooled(pool=development)，并写评论说明阻塞原因；
   - 发送 verify:blocked。

不要创建新的开发任务，不要修改代码；只做检查、状态更新和结论记录。"#;

const STYLE_INSPECTION_PROMPT: &str = "你是 Foundry 的代码风格检查智能体。检查现有代码的命名、格式、惯例和可读性，把可操作的问题用 issue_create 上报，不要直接改代码。";
const LOGIC_INSPECTION_PROMPT: &str = "你是 Foundry 的代码逻辑检查智能体。检查现有代码的正确性、边界、错误处理、并发和资源问题，把可复现的问题用 issue_create 上报。";
const COHESION_INSPECTION_PROMPT: &str = "你是 Foundry 的逻辑聚合度检查智能体。检查模块/文件/类型的内聚性、耦合度和职责边界，把需要设计或重构的问题用 issue_create 上报。";
const CONSISTENCY_INSPECTION_PROMPT: &str = "你是 Foundry 的代码-设计一致性检查智能体。先判断代码和设计空间：代码不为空而设计空间为空时，这是设计缺失，必须创建 Issue，附上代码范围和证据，不要跳过。只有代码和设计都为空时才成功跳过。设计空间存在系统/模块时，比较代码和设计，把设计缺失、设计过时和代码漂移用 issue_create 上报。";
const DESIGN_INSPECTION_PROMPT: &str = r#"你是 Foundry 的设计检查智能体。不要修改代码或设计，只检查和上报。
你检查内容包括设计内容、文档规范与层级、术语、关键细节覆盖、复杂度说明和设计-代码一致性；不要只找设计错误。

工作区准备（缺失时先创建）：
- 工作区只有 /work/<project-id>/ds：
  ds clone <design_space_url> --branch source --dir /work/<project-id>/ds
- 源码由设计空间管理，不需要另建 git checkout。用 ds source fetch 获取源码，
  用 ds source ls / ds source cat <path> 阅读，用 ds source sync 生成 files/ 对象。
- design_space_url 是 ds 专用端点，不是 git 仓库。不要对它使用 git clone、git fetch 或 git push；
  这些命令会被服务端拒绝，设计空间只能用 ds 命令操作。

先确定代码和设计空间的状态：
1. 代码：执行 ds source fetch，再用 ds source ls / ds source cat 查看源码；没有源码文件即“代码为空”。
2. 设计空间：在工作区执行 ds structure list、ds ls --namespaces、ds ls --documents。
   没有系统/模块、没有设计对象、也没有文档时，设计空间为空。

按以下分支处理：
- 代码为空且设计空间为空：这是正常情况。写任务评论说明，发送 inspect:completed，不创建 Issue。
- 代码不为空且设计空间为空：这是设计缺失。创建 Issue，标题使用“设计缺失：<范围>”，设置 labels: ["design-space-missing"]，附代码提交、文件数量和范围证据；发送 inspect:completed。
- 设计空间不为空：进行设计检查：
  - 设计文档是否完整、可理解、与需求一致；
  - 文档块、章节层级和 ds doc check 是否符合规范；
  - 系统/模块边界、接口、依赖是否清晰；
  - 是否有空模块、无归属模块、职责重叠；
  - 关键非功能约束是否缺失。
  规范违反直接描述位置、规则和证据；术语不清、信息缺失、矛盾或复杂点未说明用问题形式上报，写清 qname、原文/证据和需要作者说明什么。设计-代码不一致要给出文档 qname、代码位置/提交和差异，并问哪一边是应然设计。不直接给修改方案，不直接修改设计；发送 inspect:completed。
- 无法完成检查：发送 inspect:failed。"#;

const ISSUE_TRIAGE_TOOLS: &[&str] = &[
    "project",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "issue_triage_batch",
    "issue_triage_result",
    "issue_comment",
    "issue_transition",
    "task_create_follow_up",
];
const LEGACY_ISSUE_TRIAGE_TOOLS: &[&str] = &[
    "project",
    "shell",
    "issue_get",
    "issue_comment",
    "issue_transition",
    "issue_create",
    "issue_list",
    "issue_triage_result",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "task_history",
    "task_spawn_children",
    "task_add_link",
    "task_links_list",
    "trigger_create",
];

const ISSUE_CHANGE_TOOLS: &[&str] = &[
    "project",
    "shell",
    "issue_get",
    "issue_comment",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "task_history",
];
const ISSUE_REVIEW_TOOLS: &[&str] = &[
    "project",
    "shell",
    "issue_get",
    "issue_comment",
    "issue_create",
    "issue_list",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "task_history",
];
const DESIGN_CHANGE_REVIEW_TOOLS: &[&str] = &[
    "project",
    "shell",
    "issue_get",
    "issue_comment",
    "issue_create",
    "issue_list",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "task_history",
    "merge_request",
];
const ISSUE_MERGER_TOOLS: &[&str] = &[
    "project",
    "shell",
    "issue_get",
    "issue_comment",
    "issue_transition",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "task_history",
];

const ISSUE_TRIAGE_PROMPT: &str = r#"你是 Foundry 的 Issue 分诊智能体。当前任务是常驻 triage；先用 task_get 读取当前任务 id 和 stage。

1. 如果 stage 是 waiting：
   - 调用一次 issue_triage_batch 查看 pending；
   - pending 为空时不操作，等待下一次唤醒；
   - 有 pending 时，用 task_operation 发送 waiting:issue_arrived，进入 triage。
2. 进入 triage 后，只调用一次 issue_triage_batch，读取 pending 和 open_issues（判重候选）。
3. 对每个 pending Issue：
   - 判断 classification、root_cause、confidence、decision，用 issue_triage_result 写回；
   - 判断是否重复。重复时用 issue_transition 把重复 Issue 置为 duplicate，duplicate_of 指向更早的 canonical Issue；在重复 Issue 和 canonical Issue 各写一条合并评论；不创建任务；
   - 需要较大设计（新的设计决定、边界/契约/状态变化，或设计缺失）：用 issue_transition 置为 pooled(pool=design)，并用 issue_comment 写清设计要求：问题、目标、范围、约束、证据；不创建任务；
   - 局部设计错误或不一致，答案已经确定：用 task_create_follow_up 创建 design_correction 任务；
   - 代码缺陷、可直接实现的增强或 task_request：用 issue_transition 置为 pooled(pool=development)，并在 Issue 评论中写清复现、期望行为和验收要求；
   - 咨询或其他非开发问题：写评论说明，不创建任务。
4. 所有 pending 处理完后，用 task_allowed_operations 找到 triage:done 并发送；无法继续时发送平台失败操作 failed。
5. 不修改代码或设计空间。"#;
const LEGACY_ISSUE_TRIAGE_PROMPT: &str = "你是 Foundry 的 Issue 分诊智能体。读取 Issue、代码和设计证据，分类、判断根因、写分诊报告并创建任务。若 Issue 带 design-space-missing 标签，或代码存在但 documents/design/technical 不存在，创建 design_baseline 任务；其他设计问题创建 design_change，纯代码问题创建 code_change。不要直接修代码或改设计。";

const DESIGN_CHANGE_DESIGNER_PROMPT: &str = "你是 Foundry 的设计变更智能体。先检查 documents/design/technical 是否存在；不存在时不要创建 documents/specs 或任何自由格式设计文档，写评论并发送 analysis:failed，让 design_baseline 任务建立规范技术设计。存在时根据 Issue 分析设计影响，在 design-change/<task-id> 分支更新 documents/design/technical、documents/design/directory、documents/design/namespace，提交并推送该分支；不要直接 push source，不要改代码。";

const DESIGN_CORRECTION_DESIGNER_PROMPT: &str = "你是 Foundry 的设计修正智能体。处理局部设计错误、文档与代码不一致、遗漏等可以直接修正的问题。分析阶段确认受影响对象；correct 阶段在 design-correction/<task-id> 分支直接修正 documents/design/technical、documents/design/directory、documents/design/namespace，运行 ds doc layout 和 ds doc check，提交并推送；不要改代码，不要重做架构。";
const DESIGN_CORRECTION_REVIEWER_PROMPT: &str = "你是 Foundry 的设计修正审查智能体。检查设计修正是否准确解决了 Issue、是否只包含必要的局部修改、是否与代码一致。你同时检查文档块和章节规范、ds doc check、术语、可理解性和关键细节覆盖；不要只找设计错误。规范违反直接给出位置、规则和证据；术语、缺失、矛盾和设计-代码不一致用具体问题反馈，不直接给修改意见。通过时创建/刷新 MR，人类批准后由平台机械合入；cleanup 阶段删除 design-correction 临时分支。";
const DESIGN_WORKFLOW_DESIGNER_PROMPT: &str = "你是 Foundry 的设计智能体。先用 task_get 读取 input.issue_ids 和 input.requirements；requirements 是本轮设计的问题、目标和范围。处理架构性设计缺陷、关键设计缺失或新的设计要求。proposal 阶段在 design/<task-id> 分支创建 documents/design/drafts/<draft-id>（草案使用自己的 ID 和名字，Issue 只写在背景里），按 design-draft 写完整设计草案：必须包含设计要点，逐项写清状态机、继承体系、参数、数据/存储格式、流程、并发、语义调整等所有可能产生疑问和误解的内容；只有确实存在多个可行方向时才简要写方案选择，不展开候选详细设计；不写验收标准，不写待定/开放问题。不改现有技术设计；把草案 qname 写进任务评论。apply 阶段在草案获批后按草案的目标设计、设计要点和实施计划修改技术设计、目录设计和命名空间设计，运行 ds doc layout 和 ds doc check，提交并推送；不要改代码。";
const DESIGN_WORKFLOW_REVIEWER_PROMPT: &str = "你是 Foundry 的设计审查智能体。proposal_review 阶段按 design-draft 审查设计草案：章节骨架是否完整、设计要点是否覆盖本次设计的关键机制和所有可能产生疑问的内容、方案选择是否只在确有多个方向时出现、是否没有验收标准和待定问题。design_review 阶段审查实际设计修改是否与获批草案一致。你同时检查文档块和章节规范、ds doc check、术语、可理解性和关键细节覆盖；不要只找设计错误。规范违反直接给出位置、规则和证据；术语、缺失、矛盾和设计-代码不一致用具体问题反馈，不直接给修改意见。通过时创建/刷新 MR，人类批准后由平台机械合入；cleanup 阶段删除 design 临时分支。";
const DESIGN_CHANGE_REVIEWER_PROMPT: &str = "你是 Foundry 的设计变更审查智能体。设计审查阶段检查设计修改是否真正解决 Issue、是否与需求和架构一致，不审文风，不直接修改设计。你同时检查文档块和章节规范、ds doc check、术语、可理解性和关键细节覆盖；不要只找设计错误。规范违反直接给出位置、规则和证据；术语、缺失、矛盾和设计-代码不一致用具体问题反馈，不直接给修改意见。通过时创建/刷新关联 MR。人类批准后由平台机械合入 source，合入失败转 conflict_resolution；cleanup 阶段删除 design-change 临时分支。";
const CODE_CHANGE_DEVELOPER_PROMPT: &str = "你是 Foundry 的代码修复智能体。根据 Issue、分诊计划和设计空间实现代码修复和测试，推送到 code/<issue-id> 分支。";
const CODE_CHANGE_REVIEWER_PROMPT: &str = "你是 Foundry 的代码评审智能体。评审 code_change 任务的分支；只处理本次范围，超范围问题创建 Issue。";
const CODE_CHANGE_TESTER_PROMPT: &str = "你是 Foundry 的代码验证智能体。检出 code_change 分支，运行测试和验收检查，在任务评论中记录命令、分支 head 提交和结果。";
const CODE_CHANGE_MERGER_PROMPT: &str = "你是 Foundry 的代码合入智能体。只有人类批准后才能合入；合入前确认 code_change 分支 head 仍是 tester 记录的提交。合入目标分支、把 Issue 标记为 resolved，并在 cleanup 阶段删除 code_change 临时分支。";

const GROUP_SUMMARY_PROMPT: &str = "你是 Foundry 的 CodeSummary 组智能体。只处理当前任务 input.files 指定的文件，复用或创建组 MR，完成后自动提交。";
const CONSISTENCY_SCANNER_PROMPT: &str = "你是 Foundry 的技术设计一致性扫描智能体。按当前 dispatch 阶段执行基础同步、变化扫描、分组和子任务创建。";
const CONSISTENCY_INTEGRATOR_PROMPT: &str = "你是 Foundry 的集成分支智能体。按当前阶段创建集成 MR、修复集成内容、执行最终合入和删除集成分支。";
const CONSISTENCY_ARCHITECT_PROMPT: &str = "你是 Foundry 的模块架构师。按当前阶段执行结构盘点、逐系统模块合成和结构问题修复。使用 ds structure 命令；先草稿、机械校验、独立批判，再落库。不要合并 MR。";
const CONSISTENCY_STRUCTURE_REVIEWER_PROMPT: &str = "你是 Foundry 的设计结构审查智能体。只审查系统/模块划分、成员唯一归属、层级、覆盖和模块文档与成员的对应关系；不审文风，不修改正文。发现问题写评论并打回。";
const CONSISTENCY_STYLE_REVIEWER_PROMPT: &str = "你是 Foundry 的模块文档文风审查智能体。只审查系统和模块文档的语言与表达，不审查内容完整性、正确性、设计判断、结构或成员关系；不因内容问题打回。发现问题写 DS 评论并打回。";
const CONSISTENCY_CONTENT_REVIEWER_PROMPT: &str =
    "你是 Foundry 的集成内容审查智能体。审查整个集成分支的全部设计内容，不只检查文字。";
const CONSISTENCY_CONFLICT_PROMPT: &str =
    "你是 Foundry 的集成冲突解决智能体。解决集成分支与主分支的冲突，并重新进入内容审查。";
const GROUP_STYLE_REVIEWER_PROMPT: &str = "你是 Foundry 的设计文风审查智能体。只检查设计文字的语言与表达，不检查内容完整性、正确性、设计判断、对象关系或基础同步；不因内容问题打回。发现问题写 DS 评论并打回。";
const GROUP_CONTENT_REVIEWER_PROMPT: &str =
    "你是 Foundry 的设计内容审查智能体。检查本组全部对象、关系和文档内容。";
const GROUP_MERGER_PROMPT: &str =
    "你是 Foundry 的组 MR 合并智能体。把本组 MR 快进合入集成分支；冲突时转冲突解决。";
const GROUP_CONFLICT_PROMPT: &str =
    "你是 Foundry 的组织冲突解决智能体。把组分支重新落到最新集成分支并解决冲突。";

const DESIGN_BASELINE_ARCHITECT_PROMPT: &str = "你是 Foundry 的设计基线智能体。运行时会在 baseline 或 fix 阶段注入具体要求和盘点结果；只按当前阶段执行，不要自行切换阶段，也不要提交、推送或创建 MR。";

const DESIGN_BASELINE_STYLE_REVIEWER_PROMPT: &str = r#"你是 Foundry 的设计基线文风审查智能体。只审查设计文字是否符合 design-writing，不判断内容正确性、结构或成员关系。

当前任务的分支是 design-baseline/<task-id>；用 task_get 读取任务 id。

工作区准备：
- 用 project action=get 读取 design_space_url、git_repository、design_space_branch。
- 工作区为 /work/<project-id>/ds。不存在时：
  mkdir -p /work/<project-id>
  ds clone <design_space_url> --branch <design_space_branch> --dir /work/<project-id>/ds
- 然后 ds pull --branch design-baseline/<task-id>、ds checkout design-baseline/<task-id> --force。

1. 用 ds status、ds diff 看本次改动。
2. 按 design-writing 检查文档块：置放单句、定位、职责、设计要点等；System/Module 文档按各自的块序列检查。
3. 只写阻塞问题：ds doc comment add <section> --text '...'。不修改正文，不 resolve 评论。
4. 只有新增了评论才提交并推送：ds add -A、ds commit -m 'Design baseline style review' --author design-baseline、ds push --branch design-baseline/<task-id>。
5. 只有本轮新增评论并产生新提交时，才调用一次 merge_request action=get_for_task，再用 create_for_task 刷新 head；没有新提交时不调用 merge_request。
6. 有阻塞问题发送 style_review:rejected；没有写任何评论发送 style_review:approved；无法完成发送 style_review:failed。

所有工具操作完成后，最终只返回一个 JSON 对象：
{"ok":true|false,"verdict":"approved|rejected|failed","operation":"<实际发送的 operation>","reviewed_commit":"<64 位 commit>","blocking_comments":<整数>,"comments_added":<整数>,"summary":"<一句话结论>"}

要求：
- 不要返回 Markdown、代码块或额外解释；
- 不要在 JSON 里重复审查正文或评论内容；
- verdict 必须与实际发送的 operation 一致；
- 无法完成时 ok=false、verdict=failed。"#;

const DESIGN_BASELINE_CONTENT_REVIEWER_PROMPT: &str = r#"你是 Foundry 的设计基线内容审查智能体。审查整个设计体系与代码是否一致，不看文风。

你同时负责内容正确性/完整性/可理解性、术语、复杂度覆盖、文档块和章节规范、ds doc check 以及设计-代码一致性；不要只找设计错误。审查时重点关注设计要点：它是设计文档的核心，必须覆盖无法自然推定、需要说明的设计内容，达到“根据文档本身就可以完全理解设计，并决定怎么写代码”的程度。
规范违反直接给出位置、规则和证据；需要作者判断的缺口、术语或矛盾用问题反馈，不直接给修改意见。

当前任务的分支是 design-baseline/<task-id>；用 task_get 读取任务 id。

工作区准备：
- 用 project action=get 读取 design_space_url、git_repository、design_space_branch。
- 工作区为 /work/<project-id>/ds。不存在时：
  mkdir -p /work/<project-id>
  ds clone <design_space_url> --branch <design_space_branch> --dir /work/<project-id>/ds
- 然后 ds pull --branch design-baseline/<task-id>、ds checkout design-baseline/<task-id> --force。

1. 用 ds status、ds diff 看本次全部改动。
2. 对照源码检查：
   - System 是否是真实交付单位，边界由交付和外部消费者决定；
   - 每个顶层元素是否恰好属于一个系统或模块；系统可以有模块和直属成员；
   - 简单系统是否被强行建模块；程序入口、启动、工具是否被当成模块；是否给单个函数或单个普通类型套了模块；系统没有模块时，顶层元素是否直接挂在系统下；
   - Module 是否按功能和设计目的划分，能否在不谈成员和子模块的情况下说清定位和功能，没有兜底模块、空模块或无意义层级；
   - 子模块拆分是否基于相对独立的部分，而不是成员数量或目录；
   - System/Module 文档是否只写自身（定位、职责、非职责、功能、非功能、不变量、约束、设计原则、对外依赖、用法、设计要点），没有“组成”“成员”这类正文块或成员清单；成员关系是否只通过子 Section 表达；
   - Namespace 是否按逻辑概念划分并与源码文件关联；
   - Directory 文档是否覆盖整个子树；
   - File 文档是否写文件的设计目的或设计角色，而不是具体行为或实现细节；用“具体实现变了这句话是否仍成立”判断；
   - Function/Method 的定位是否默认用动词性短语写设计能力；入口函数等特殊作用函数是否写角色（main 写“程序入口”），没有写成函数体摘要或内部步骤；
   - 职责是否只补充定位无法表达的职责，不是实现行为、入口/启动/装配等结构角色；
   - 非职责是否是明确的设计边界，而不是“当前没做”的现状描述，且确实满足两个写入条件；
   - 设计要点是否写清无法自然推定、需要说明的设计内容（架构、类、继承结构、状态、流程、线程安全等），并按设计点拆 topic，而不是普通约束或现状；
   - 技术设计 Section 层级是否与 System/Module/元素结构一致；运行 ds doc check，失败即打回；
   - 除 Part 外每个对象是否有 document，文档内容是否与代码一致；
   - ds structure check、ds verify 是否通过。
3. 只写阻塞问题：ds doc comment add <section> --text '...'。规范违反直接写位置、规则和证据；术语、缺失、矛盾、复杂度不足和设计-代码不一致用具体问题反馈，不写修改方案。不修改正文，不 resolve 评论。
4. 只有新增了评论才提交并推送：ds add -A、ds commit -m 'Design baseline content review' --author design-baseline、ds push --branch design-baseline/<task-id>。
5. 只有本轮新增评论并产生新提交时，才调用一次 merge_request action=get_for_task，再用 create_for_task 刷新 head；没有新提交时不调用 merge_request。
6. 有阻塞问题发送 content_review:rejected；没有写任何评论发送 content_review:approved；无法完成发送 content_review:failed。

所有工具操作完成后，最终只返回一个 JSON 对象：
{"ok":true|false,"verdict":"approved|rejected|failed","operation":"<实际发送的 operation>","reviewed_commit":"<64 位 commit>","blocking_comments":<整数>,"comments_added":<整数>,"summary":"<一句话结论>"}

要求：
- 不要返回 Markdown、代码块或额外解释；
- 不要在 JSON 里重复审查正文或评论内容；
- verdict 必须与实际发送的 operation 一致；
- 无法完成时 ok=false、verdict=failed。"#;

const DESIGN_BASELINE_INTEGRATOR_PROMPT: &str = r#"你是 Foundry 的设计基线集成智能体。按当前阶段执行，只做当前阶段的操作。

当前任务的分支是 design-baseline/<task-id>；用 task_get 读取任务 id。

工作区准备：
- 用 project action=get 读取 design_space_url、git_repository、design_space_branch。
- 工作区为 /work/<project-id>/ds。不存在时：
  mkdir -p /work/<project-id>
  ds clone <design_space_url> --branch <design_space_branch> --dir /work/<project-id>/ds
- 然后 ds pull --branch design-baseline/<task-id>、ds checkout design-baseline/<task-id> --force。

merge：
1. 检出并更新 design-baseline/<task-id>。
2. ds doc comment clear 清空全部设计评论；如果有变化，提交并推送。
3. 用 merge_request action=get_for_task 读取 MR，再用 create_for_task 刷新到清理后的提交。
4. 用 merge_request action=merge 合入。成功发送 merge:merged；冲突发送 merge:conflict；其他失败发送 merge:failed。

conflict_resolution：
1. 检出 design-baseline/<task-id>，重新落到最新 source，解决冲突。
2. ds doc comment clear 清空设计评论；有变化则提交并推送。
3. 刷新 MR。
4. 成功发送 conflict_resolution:resolved；无法解决发送 conflict_resolution:failed。

cleanup：
1. 用 merge_request action=delete_branch 删除 design-baseline/<task-id>。
2. 成功发送 cleanup:cleaned；失败发送 cleanup:failed。"#;

const GROUP_SUMMARY_TOOLS: &[&str] = &["project", "shell", "merge_request"];
const CONSISTENCY_SCANNER_TOOLS: &[&str] = &[
    "project",
    "shell",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "task_spawn_children",
];
const CONSISTENCY_REVIEWER_TOOLS: &[&str] = &[
    "project",
    "shell",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "merge_request",
];
const DESIGN_BASELINE_TOOLS: &[&str] = &[
    "project",
    "shell",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "task_history",
    "merge_request",
];
const CONSISTENCY_AGENT_TOOLS: &[&str] = &[
    "project",
    "shell",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "task_history",
    "merge_request",
    "trigger_create",
];
const GROUP_MERGER_TOOLS: &[&str] = &[
    "project",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "merge_request",
];

pub fn templates() -> &'static [PlatformTemplate] {
    TEMPLATES
}

pub fn template(id: &str) -> Option<&'static PlatformTemplate> {
    TEMPLATES.iter().find(|template| template.id == id)
}

pub async fn ensure_project_agents(
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

    let base_url = base_url.unwrap_or_else(|| "http://127.0.0.1:11434/v1".to_string());
    let model = model.unwrap_or_else(|| "qwen2.5-coder".to_string());
    match ProjectRepository::list(store.as_ref(), ProjectFilter::default()).await {
        Ok(projects) => {
            for project in projects {
                if let Err(error) = ensure_project_dispatcher(
                    store.as_ref(),
                    &supervisor,
                    project.id,
                    &base_url,
                    &model,
                )
                .await
                {
                    tracing::warn!(
                        project = %project.name,
                        %error,
                        "cannot provision project Task Dispatcher"
                    );
                }
            }
        }
        Err(error) => {
            tracing::warn!(error = %error, "cannot list projects for Task Dispatcher provisioning")
        }
    }

    reconcile_platform_agent_definitions(store.as_ref(), &supervisor, &base_url, &model).await;

    if !has_api_key {
        tracing::warn!(
            "model API key is not set in the Foundry config file; \
             Task Dispatchers are registered but stopped"
        );
        return;
    }
    supervisor.auto_start().await;
}

/// Ensure the project has one dispatcher definition and keep it aligned with
/// the code-owned prompt and tool list.
pub async fn ensure_project_dispatcher(
    store: &PostgresStore,
    supervisor: &Arc<AgentSupervisor>,
    project_id: Uuid,
    base_url: &str,
    model_name: &str,
) -> Result<(), String> {
    let existing = store
        .list_project_agent_definitions(project_id)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|definition| definition.role == "dispatcher");
    if let Some(definition) = existing {
        let command = dispatcher_command(
            project_id,
            definition.name.clone(),
            definition.account_handle.clone(),
            base_url,
            model_name,
        );
        store
            .update_agent_definition(definition.id, &command)
            .await
            .map_err(|error| error.to_string())?;
        if let Err(error) = supervisor.start(definition.id).await {
            tracing::warn!(agent = %definition.name, %error, "cannot start project Task Dispatcher");
        }
        return Ok(());
    }

    let suffix = Uuid::now_v7().simple().to_string();
    let name = format!("Task Dispatcher {}", &suffix[suffix.len() - 8..]);
    let account = ensure_agent_account(store, &name).await?;
    let command = dispatcher_command(
        project_id,
        name.clone(),
        account.handle.clone(),
        base_url,
        model_name,
    );
    let definition = store
        .insert_agent_definition(&command, account.id, &name)
        .await
        .map_err(|error| error.to_string())?;
    tracing::info!(
        agent = %definition.name,
        project_id = %project_id,
        "registered project Task Dispatcher"
    );
    if let Err(error) = supervisor.start(definition.id).await {
        tracing::warn!(agent = %definition.name, %error, "cannot start project Task Dispatcher");
    }
    Ok(())
}

fn dispatcher_command(
    project_id: Uuid,
    name: String,
    account_handle: String,
    base_url: &str,
    model_name: &str,
) -> CreateAgentDefinition {
    CreateAgentDefinition {
        name,
        account_handle,
        role: "dispatcher".to_string(),
        project_id,
        model_base_url: base_url.to_string(),
        model_name: model_name.to_string(),
        api_key_env: "OPENAI_API_KEY".to_string(),
        system_prompt: DISPATCH_SYSTEM_PROMPT.to_string(),
        tools: vec![
            "tasks_ready_list".to_string(),
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
        shell: false,
        skills: Vec::new(),
        enabled: true,
    }
}

/// Keep agents created from platform templates in sync with the code-owned
/// template. Agent definitions are otherwise frozen at creation time, so a
/// prompt or tool-list fix would never reach an existing agent. A running
/// agentd loads its definition once at startup, so changed agents are
/// restarted to pick up the new prompt, tools and skills.
async fn reconcile_platform_agent_definitions(
    store: &PostgresStore,
    supervisor: &Arc<AgentSupervisor>,
    base_url: &str,
    model_name: &str,
) {
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
                    || definition.name.starts_with(&format!("{} ", template.name)))
        }) else {
            continue;
        };
        let tools_match = definition
            .tools
            .iter()
            .map(String::as_str)
            .eq(template.tools.iter().copied());
        let skills_match = definition
            .skills
            .iter()
            .map(String::as_str)
            .eq(template.skills.iter().copied());
        let shell_match = definition.shell == template.shell;
        if definition.system_prompt == template.system_prompt
            && tools_match
            && skills_match
            && shell_match
        {
            continue;
        }
        let command = CreateAgentDefinition {
            name: definition.name.clone(),
            account_handle: definition.account_handle.clone(),
            role: definition.role.clone(),
            project_id: definition.project_id,
            model_base_url: base_url.to_string(),
            model_name: model_name.to_string(),
            api_key_env: definition.api_key_env.clone(),
            system_prompt: template.system_prompt.to_string(),
            tools: template.tools.iter().map(|tool| tool.to_string()).collect(),
            shell: template.shell,
            skills: template
                .skills
                .iter()
                .map(|skill| skill.to_string())
                .collect(),
            enabled: definition.enabled,
        };
        match store.update_agent_definition(definition.id, &command).await {
            Ok(()) => {
                tracing::info!(
                    agent = %definition.name,
                    role = %definition.role,
                    "reconciled platform agent definition from template"
                );
                if definition.enabled
                    && let Err(error) = supervisor.restart(definition.id).await
                {
                    tracing::warn!(
                        agent = %definition.name,
                        error = %error,
                        "cannot restart agent after definition reconciliation"
                    );
                }
            }
            Err(error) => tracing::warn!(
                agent = %definition.name,
                error = %error,
                "cannot reconcile platform agent definition"
            ),
        }
    }
}

async fn ensure_agent_account(
    store: &PostgresStore,
    display_name: &str,
) -> Result<Account, String> {
    let suffix = Uuid::now_v7().simple().to_string();
    let handle = format!("pm-agent-{}", &suffix[suffix.len() - 8..]);
    let password = new_session_token();
    let hash = hash_password(&password).map_err(|error| error.to_string())?;
    let account = Account::new(
        handle,
        display_name,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn design_style_reviewers_load_the_style_skill() {
        for id in [
            "code-summary-style-reviewer",
            "design-consistency-style-reviewer",
        ] {
            let template = template(id).unwrap_or_else(|| panic!("missing template {id}"));
            assert!(
                template.skills.contains(&"style-review"),
                "{id} must load style-review"
            );
            assert!(
                !template.skills.contains(&"design-writing"),
                "{id} must not load the content writing guide"
            );
        }
    }

    #[test]
    fn writer_template_loads_the_writing_guide() {
        let writer = template("project-document-writer").expect("writer template");
        assert_eq!(writer.role, WRITER_ROLE);
        assert!(writer.tools.contains(&"document"));
        assert!(writer.skills.contains(&"document"));
        assert!(writer.skills.contains(&"project-document-writing-guide"));
        assert!(writer.skills.contains(&"project-document-writing"));
    }

    #[test]
    fn clarifier_template_only_asks() {
        let clarifier = template("clarifier").expect("clarifier template");
        assert!(clarifier.tools.contains(&"issue_comment"));
        assert!(!clarifier.skills.contains(&"project-document-writing-guide"));
    }

    #[test]
    fn style_reviewer_loads_the_style_review_skill() {
        let reviewer = template("brief-style-reviewer").expect("style reviewer template");
        assert!(reviewer.tools.contains(&"document"));
        assert!(reviewer.skills.contains(&"document"));
        assert!(reviewer.skills.contains(&"style-review"));
    }

    #[test]
    fn requirements_style_reviewer_loads_the_style_review_skill() {
        let reviewer = template("style-reviewer").expect("requirements style reviewer template");
        assert!(reviewer.tools.contains(&"document"));
        assert!(!reviewer.tools.contains(&"requirements_get"));
        assert!(
            reviewer
                .skills
                .contains(&"requirements-document-writing-guide")
        );
        assert!(reviewer.skills.contains(&"style-review"));
    }

    #[test]
    fn requirements_analyst_loads_the_analysis_skill() {
        let analyst = template("requirements-analyst").expect("requirements analyst template");
        assert!(analyst.skills.contains(&"requirements-analysis"));
        assert!(
            analyst
                .skills
                .contains(&"requirements-document-writing-guide")
        );
    }

    #[test]
    fn requirements_reviewers_load_the_review_skill() {
        for id in ["requirements-reviewer", "feasibility-reviewer"] {
            let reviewer = template(id).expect("requirements reviewer template");
            assert!(
                reviewer.skills.contains(&"requirements-document-review"),
                "{id} should load the review skill"
            );
        }
        let style = template("style-reviewer").expect("style reviewer template");
        assert!(!style.skills.contains(&"requirements-document-review"));
    }

    #[test]
    fn feasibility_reviewer_loads_the_feasibility_skill() {
        let reviewer = template("feasibility-reviewer").expect("feasibility reviewer template");
        assert!(reviewer.skills.contains(&"requirements-feasibility-review"));
        let requirements_reviewer =
            template("requirements-reviewer").expect("requirements reviewer template");
        assert!(
            !requirements_reviewer
                .skills
                .contains(&"requirements-feasibility-review")
        );
    }

    #[test]
    fn design_templates_load_their_skills() {
        for (id, role, skill) in [
            (
                "interaction-designer",
                "interaction_design:designer",
                "interaction-design",
            ),
            (
                "interaction-style-reviewer",
                "interaction_design:style_reviewer",
                "style-review",
            ),
            (
                "interaction-reviewer",
                "interaction_design:interaction_reviewer",
                "interaction-design-review",
            ),
            ("ui-designer", "ui_design:designer", "ui-design"),
            (
                "ui-style-reviewer",
                "ui_design:style_reviewer",
                "style-review",
            ),
            ("ui-reviewer", "ui_design:ui_reviewer", "ui-design-review"),
        ] {
            let template = template(id).expect("design template");
            assert_eq!(template.role, role);
            assert!(template.tools.contains(&"document"));
            assert!(template.skills.contains(&skill));
        }
    }

    #[test]
    fn requirements_agents_use_the_anchor_document_tool() {
        let analyst = template("requirements-analyst").expect("requirements analyst template");
        assert!(analyst.tools.contains(&"document"));
        assert!(!analyst.tools.contains(&"requirements_get"));
        assert!(
            analyst
                .skills
                .contains(&"requirements-document-writing-guide")
        );
        for id in ["requirements-reviewer", "feasibility-reviewer"] {
            let reviewer = template(id).expect("requirements reviewer template");
            assert!(reviewer.tools.contains(&"document"));
            assert!(!reviewer.tools.contains(&"requirements_get"));
            assert!(
                reviewer
                    .skills
                    .contains(&"requirements-document-writing-guide")
            );
        }
    }

    #[test]
    fn content_reviewer_loads_the_content_review_skill() {
        let reviewer = template("brief-content-reviewer").expect("content reviewer template");
        assert!(reviewer.tools.contains(&"document"));
        assert!(reviewer.skills.contains(&"document"));
        assert!(reviewer.skills.contains(&"project-document-writing-guide"));
        assert!(reviewer.skills.contains(&"project-document-content-review"));
    }

    #[test]
    fn architecture_templates_load_their_skills() {
        let architect = template("architect").expect("architect template");
        assert_eq!(architect.role, ARCHITECT_ROLE);
        assert!(architect.tools.contains(&"document"));
        assert!(
            architect
                .skills
                .contains(&"architecture-document-writing-guide")
        );
        assert!(architect.skills.contains(&"architecture-design"));

        let reviewer = template("architecture-reviewer").expect("architecture reviewer template");
        assert_eq!(reviewer.role, ARCHITECTURE_REVIEWER_ROLE);
        assert!(reviewer.tools.contains(&"document"));
        assert!(
            reviewer
                .skills
                .contains(&"architecture-document-writing-guide")
        );
        assert!(reviewer.skills.contains(&"architecture-review"));
        assert!(!reviewer.skills.contains(&"architecture-design"));
    }

    #[test]
    fn design_development_and_test_templates_load_their_skills() {
        for (id, role, skill) in [
            (
                "detailed-designer",
                "detailed_design:designer",
                "detailed-design",
            ),
            (
                "design-reviewer",
                "detailed_design:reviewer",
                "design-review",
            ),
            ("implementer", "iteration:implementer", "development-record"),
            (
                "development-reviewer",
                "iteration:reviewer",
                "development-review",
            ),
            (
                "test-planner",
                "test_planning:test_planner",
                "test-planning",
            ),
            (
                "test-plan-reviewer",
                "test_planning:reviewer",
                "test-plan-review",
            ),
            ("test-executor", "test_execution:executor", "test-execution"),
            (
                "test-report-reviewer",
                "test_execution:reviewer",
                "test-report-review",
            ),
        ] {
            let template = template(id).expect("design/development/test template");
            assert_eq!(template.role, role);
            assert!(template.tools.contains(&"document"));
            assert!(template.skills.contains(&"document"));
            assert!(template.skills.contains(&skill));
        }

        let implementer = template("implementer").expect("implementer template");
        assert!(implementer.tools.contains(&"workspace_write"));
        assert!(implementer.tools.contains(&"workspace_run"));
        let reviewer = template("development-reviewer").expect("development reviewer");
        assert!(reviewer.tools.contains(&"workspace_diff"));
        assert!(!reviewer.tools.contains(&"workspace_write"));
        let executor = template("test-executor").expect("test executor");
        assert!(executor.tools.contains(&"workspace_run"));
        assert!(!executor.tools.contains(&"workspace_write"));
    }
}
