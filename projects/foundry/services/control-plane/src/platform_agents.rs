//! Platform-owned agents and templates.
//!
//! Agents are not user-configurable. Each project owns one PM dispatcher and
//! the agents created from platform templates. The PM assigns task roles
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
      task_start with task_id.
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
        skills: &["design-space", "design-writing", "design-consistency"],
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
        skills: &["design-space", "design-writing", "design-consistency"],
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
        id: "code-summary-style-reviewer",
        name: "CodeSummary Style Reviewer",
        role: "code_summary_group:style_reviewer",
        description: "Reviews design-writing style in a group MR",
        system_prompt: GROUP_STYLE_REVIEWER_PROMPT,
        tools: CONSISTENCY_REVIEWER_TOOLS,
        shell: true,
        skills: &["design-space", "design-writing", "design-consistency"],
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
        role: "issue_triage:triage",
        description: "Classifies Issues, determines root cause and creates design/development tasks",
        system_prompt: ISSUE_TRIAGE_PROMPT,
        tools: ISSUE_TRIAGE_TOOLS,
        shell: true,
        skills: &["design-space", "issue-loop"],
    },
    PlatformTemplate {
        id: "design-change-designer",
        name: "Design Change Designer",
        role: "design_change:designer",
        description: "Analyzes an Issue and updates the design space",
        system_prompt: DESIGN_CHANGE_DESIGNER_PROMPT,
        tools: ISSUE_CHANGE_TOOLS,
        shell: true,
        skills: &["design-space", "design-writing", "issue-loop"],
    },
    PlatformTemplate {
        id: "design-change-reviewer",
        name: "Design Change Reviewer",
        role: "design_change:design_reviewer",
        description: "Reviews a design change",
        system_prompt: DESIGN_CHANGE_REVIEWER_PROMPT,
        tools: ISSUE_REVIEW_TOOLS,
        shell: true,
        skills: &["design-space", "design-writing", "issue-loop"],
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
        skills: &["design-space", "code-inspection"],
    },
    PlatformTemplate {
        id: "software-development-setup",
        name: "Software Development Setup",
        role: "software_development:project_setup",
        description: "Creates the default daily-check trigger and enters the running stage",
        system_prompt: SOFTWARE_DEVELOPMENT_SETUP_PROMPT,
        tools: SOFTWARE_DEVELOPMENT_SETUP_TOOLS,
        shell: false,
        skills: &["software-development"],
    },
    PlatformTemplate {
        id: "daily-check-dispatcher",
        name: "Daily Check Dispatcher",
        role: "daily_check:dispatcher",
        description: "Creates the consistency and design inspection tasks for a daily check",
        system_prompt: DAILY_CHECK_DISPATCHER_PROMPT,
        tools: DAILY_CHECK_TOOLS,
        shell: false,
        skills: &["software-development"],
    },
];

const SOFTWARE_DEVELOPMENT_SETUP_TOOLS: &[&str] = &[
    "project",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "trigger_create",
    "trigger_list",
];
const DAILY_CHECK_TOOLS: &[&str] = &[
    "project",
    "task_get",
    "task_allowed_operations",
    "task_operation",
    "task_comment",
    "task_spawn_children",
];

const SOFTWARE_DEVELOPMENT_SETUP_PROMPT: &str = "你是 Foundry 的软件项目初始化智能体。为项目创建一个“日常检查”触发器，计划为 daily@09:00，目标流程 daily_check；如果同名触发器已存在则复用，不要重复创建。完成后发送 setup:ready。";
const DAILY_CHECK_DISPATCHER_PROMPT: &str = "你是 Foundry 的日常检查调度智能体。按顺序创建“代码-设计一致性检查”和“设计检查”两个子任务，然后发送 dispatch:dispatched，等待子任务完成。";

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

const STYLE_INSPECTION_PROMPT: &str = "你是 Foundry 的代码风格检查智能体。检查现有代码的命名、格式、惯例和可读性，把可操作的问题用 issue_create 上报，不要直接改代码。";
const LOGIC_INSPECTION_PROMPT: &str = "你是 Foundry 的代码逻辑检查智能体。检查现有代码的正确性、边界、错误处理、并发和资源问题，把可复现的问题用 issue_create 上报。";
const COHESION_INSPECTION_PROMPT: &str = "你是 Foundry 的逻辑聚合度检查智能体。检查模块/文件/类型的内聚性、耦合度和职责边界，把需要设计或重构的问题用 issue_create 上报。";
const CONSISTENCY_INSPECTION_PROMPT: &str = "你是 Foundry 的代码-设计一致性检查智能体。先检查设计空间是否为空：没有 design 结构根、没有系统/模块时，写评论说明并发送 inspect:completed，不创建 Issue。只有存在设计结构时才比较代码与设计，把设计过时和代码漂移用 issue_create 上报。";
const DESIGN_INSPECTION_PROMPT: &str = "你是 Foundry 的设计检查智能体。先检查设计空间是否为空：没有 design 结构根、没有系统/模块、也没有文档时，这是正常情况，写评论说明并发送 inspect:completed，不创建 Issue。只有存在设计对象或文档时才检查边界、接口和依赖，把问题用 issue_create 上报，不直接修改设计。";

const ISSUE_TRIAGE_TOOLS: &[&str] = &[
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

const ISSUE_TRIAGE_PROMPT: &str = "你是 Foundry 的 Issue 分诊智能体。读取 Issue、代码和设计证据，分类、判断根因、写分诊报告，并创建 design_change/code_change 子任务。不要直接修代码或改设计。";
const DESIGN_CHANGE_DESIGNER_PROMPT: &str = "你是 Foundry 的设计变更智能体。根据 Issue 分析设计影响，更新设计空间，提交并推送 source 分支；不要改代码。";
const DESIGN_CHANGE_REVIEWER_PROMPT: &str = "你是 Foundry 的设计变更审查智能体。检查设计修改是否真正解决 Issue、是否与需求和架构一致；不审文风，不直接修改设计。";
const CODE_CHANGE_DEVELOPER_PROMPT: &str = "你是 Foundry 的代码修复智能体。根据 Issue、分诊计划和设计空间实现代码修复和测试，推送到 code/<issue-id> 分支。";
const CODE_CHANGE_REVIEWER_PROMPT: &str = "你是 Foundry 的代码评审智能体。评审 code_change 任务的分支；只处理本次范围，超范围问题创建 Issue。";
const CODE_CHANGE_TESTER_PROMPT: &str = "你是 Foundry 的代码验证智能体。检出 code_change 分支，运行测试和验收检查，记录证据。";
const CODE_CHANGE_MERGER_PROMPT: &str = "你是 Foundry 的代码合入智能体。把已验证的 code_change 分支合入目标分支，并将 Issue 标记为 resolved。";

const GROUP_SUMMARY_PROMPT: &str = "你是 Foundry 的 CodeSummary 组智能体。只处理当前任务 input.files 指定的文件，复用或创建组 MR，完成后自动提交。";
const CONSISTENCY_SCANNER_PROMPT: &str = "你是 Foundry 的技术设计一致性扫描智能体。按当前 dispatch 阶段执行基础同步、变化扫描、分组和子任务创建。";
const CONSISTENCY_INTEGRATOR_PROMPT: &str = "你是 Foundry 的集成分支智能体。按当前阶段创建集成 MR、修复集成内容、执行最终合入和删除集成分支。";
const CONSISTENCY_ARCHITECT_PROMPT: &str = "你是 Foundry 的模块架构师。按当前阶段执行结构盘点、逐系统模块合成和结构问题修复。使用 ds structure 命令；先草稿、机械校验、独立批判，再落库。不要合并 MR。";
const CONSISTENCY_STRUCTURE_REVIEWER_PROMPT: &str = "你是 Foundry 的设计结构审查智能体。只审查系统/模块划分、成员唯一归属、层级、覆盖和模块文档与成员的对应关系；不审文风，不修改正文。发现问题写评论并打回。";
const CONSISTENCY_STYLE_REVIEWER_PROMPT: &str = "你是 Foundry 的模块文档文风审查智能体。只审查系统和模块文档的文字，不审查结构或成员关系；发现问题写评论并打回。";
const CONSISTENCY_CONTENT_REVIEWER_PROMPT: &str =
    "你是 Foundry 的集成内容审查智能体。审查整个集成分支的全部设计内容，不只检查文字。";
const CONSISTENCY_CONFLICT_PROMPT: &str =
    "你是 Foundry 的集成冲突解决智能体。解决集成分支与主分支的冲突，并重新进入内容审查。";
const GROUP_STYLE_REVIEWER_PROMPT: &str =
    "你是 Foundry 的设计文风审查智能体。只检查设计文字，发现问题写 review comment 并打回。";
const GROUP_CONTENT_REVIEWER_PROMPT: &str =
    "你是 Foundry 的设计内容审查智能体。检查本组全部对象、关系和文档内容。";
const GROUP_MERGER_PROMPT: &str =
    "你是 Foundry 的组 MR 合并智能体。把本组 MR 快进合入集成分支；冲突时转冲突解决。";
const GROUP_CONFLICT_PROMPT: &str =
    "你是 Foundry 的组织冲突解决智能体。把组分支重新落到最新集成分支并解决冲突。";

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
                if let Err(error) =
                    ensure_project_pm(store.as_ref(), &supervisor, project.id, &base_url, &model)
                        .await
                {
                    tracing::warn!(
                        project = %project.name,
                        %error,
                        "cannot provision project PM agent"
                    );
                }
            }
        }
        Err(error) => tracing::warn!(error = %error, "cannot list projects for PM provisioning"),
    }

    reconcile_platform_agent_definitions(store.as_ref(), &supervisor).await;

    if !has_api_key {
        tracing::warn!(
            "model API key is not set in the Foundry config file; \
             PM agents are registered but stopped"
        );
        return;
    }
    supervisor.auto_start().await;
}

/// Ensure the project has one dispatcher definition and keep it aligned with
/// the code-owned prompt and tool list.
pub async fn ensure_project_pm(
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
        let command = pm_command(
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
            tracing::warn!(agent = %definition.name, %error, "cannot start project PM agent");
        }
        return Ok(());
    }

    let suffix = Uuid::now_v7().simple().to_string();
    let name = format!("PM Agent {}", &suffix[suffix.len() - 8..]);
    let account = ensure_agent_account(store, &name).await?;
    let command = pm_command(
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
        "registered project PM dispatcher"
    );
    if let Err(error) = supervisor.start(definition.id).await {
        tracing::warn!(agent = %definition.name, %error, "cannot start project PM agent");
    }
    Ok(())
}

fn pm_command(
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
            model_base_url: definition.model_base_url.clone(),
            model_name: definition.model_name.clone(),
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
