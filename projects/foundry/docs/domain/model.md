# 领域模型

Foundry 的核心命题：**工作项不是便签，而是可执行契约。**

一张工单要能进入下一状态，必须满足该状态要求的证据；谁有权推动它，由策略决定；推动了什么、依据是什么，全部进入不可变事件日志。AI 和人在同一个模型里操作，区别只在权限和入口。

## 对象

| 对象 | 回答的问题 | 生命周期 |
|---|---|---|
| Account | 谁在操作 | 活跃 → 停用 |
| Session | 谁在什么时间内代表账户 | 活跃 → 过期 / 撤销 |
| Project | 这是哪个隔离边界 | 活跃 → 归档 |
| Initiative | 为什么现在做这件事 | 提案 → 立项 → 完成 / 取消 |
| Issue | 世界上发生了什么、谁想要什么、哪里需要讨论 | 待处理 → 处理中 → 待确认 → 已关闭 / 不处理 / 重复 |
| ProjectDocument（项目文档） | 为什么做、边界是什么 | 草案 → 发布版本（不可变） |
| RequirementsDocument（需求文档） | 为谁做、做什么 | 草案 → 发布版本（不可变） |
| DesignDoc | 打算怎么实现，权衡是什么 | 草稿 → 评审 → 接受 / 被取代 |
| ADR | 为什么做出某个长期架构决定 | 提议 → 接受 / 拒绝 → 被取代 / 废弃 |
| Contract | 系统之间约定什么接口 | 草稿 → 生效 → 废弃 |
| Task | 谁在什么时候做什么 | 待办 → 进行中 → 评审 → 完成 |
| Decision | 哪个选项被选中，谁负责 | 开放 → 已决定 → 重开 / 被取代 |
| Review | 谁在什么标准下检查过 | 待评审 → 通过 / 要求修改 |
| Release | 哪些变更一起上线 | 计划 → 构建 → 验证 → 发布 → 回滚 |
| Event | 发生过什么 | 只追加，不修改 |

## 账户、会话与项目

### Account

账户是唯一身份来源。人类和智能体都是账户，只是 `kind` 不同：

- `handle`：稳定、唯一、小写；进入历史记录。
- `display_name` / `email`：展示与联系。
- `kind`：`human` / `agent`。
- `role`：全局 `admin` / `member`。全局 admin 可以管理账户和所有项目。
- `status`：`active` / `suspended`。停用后现有会话立即失效。
- `password_hash`：Argon2。智能体账户也通过会话 token 操作，不信任请求体身份。

系统必须始终保留至少一个活跃 admin；降级或停用最后一个 admin 会被拒绝。

### Session

登录成功后生成一个随机 token，只在响应中出现一次；数据库只存 SHA-256 哈希。会话记录 `account_id`、`expires_at`、`revoked_at`。

- Web 使用 `HttpOnly`、`SameSite=Lax` 的 `foundry_session` Cookie。
- REST / MCP 使用 `Authorization: Bearer <token>`。
- 登出撤销当前会话，而不是只删客户端 Cookie。

actor 由会话账户派生，请求体中的 `actor` 不再是可信输入。

### Project

项目是隔离边界。每个 Issue、项目文档、需求文档和事件都属于恰好一个项目。

- `id`：项目标识。机器和 URL 用它定位项目。
- `name` / `description`：展示信息。`name` 可修改，不要求唯一。
- `status`：`active` / `archived`。归档项目只读。
- 删除：Owner 或全局 admin 可以物理删除项目；项目范围内的 Task、项目文档、需求文档、议题、成员和事件一并删除，不可恢复。
- `members`：账户及其项目角色 `viewer < member < maintainer < owner`。
- 创建者自动成为 Owner；项目必须始终保留至少一个 Owner。

角色决定操作权限，策略再叠加一层业务限制（例如智能体不能关闭议题）。跨项目读取一律按不存在处理。

没有默认项目或配置种子项目。首个管理员创建完成后，由管理员显式创建第一个项目；此后项目通过控制台或 API 创建。

## Issue

Issue 是问题空间的对象：客户反馈、缺陷、问题、想法、任务请求和事故都先进入这里。它便宜、量多、允许模糊，目标是**先收集和讨论，再决定**。

核心字段：

- `kind`：`feedback` / `bug` / `question` / `idea` / `task_request` / `incident`。创建时确定。
- `source`：来源类型（客户、客服、监控、数据分析、内部等）和自由引用（工单号、告警链接、客户名）。
- `reporter`：谁报告的。人类和智能体都可以。
- `description`：发生了什么。不要求写成需求。
- `severity` / `impact` / `area` / `labels` / `assignee`：创建时填写。
- `comments`：讨论。评论可以标记为 `internal`，不对外部报告人展示。
- `history`：状态变更及 actor、时间、备注。
- `resolution` / `duplicate_of`：处置结论。
- `links`：可以关联任务、设计、决策等。

### 分类与处置

分类（`kind`、`severity`、`impact`、`area`、`labels`、`assignee`）在创建议题时确定，没有独立的分诊步骤。处置通过状态转换和链接表达：

| 处置 | 结果 |
|---|---|
| 关闭（`Closed`） | 已解决并确认，或问题已消失 |
| 不处理（`Rejected`） | 明确记录为什么不做 |
| 重复（`Duplicate`） | 指向已有 Issue |
| 需求分析 | 后续进入项目文档和需求文档，并保留指向 Issue 的证据 |
| 工程任务 | 创建 Task，Issue 在验证通过后关闭 |
| 缺陷修复 | Bug 仍是 Issue，修复工作用 Task 表达 |

### Bug 为什么不再单独建模

Bug 有额外字段（环境、影响版本、复现步骤、预期、实际），但它和反馈、问题共享同一套收集、讨论、处置流程。把 Bug 做成 Issue 的一种 `kind`，可以避免两套入口、两套权限和两份重复讨论。修复工作本身是 Task，验证证据回到 Issue 上，最后才关闭。

事故同理，是 `IssueKind::Incident`；响应过程用评论和状态历史记录，复盘结论用 Decision 或文档承载，不再维护第二套事故状态机。

## 项目文档（Project Document）

每个项目维护一份唯一的项目文档，它回答“为什么做、边界是什么”。文档包含定位（是什么、不是什么）、背景（现状、问题）、目标、非目标、分期计划、约束、风险和术语表；人类看到渲染结果，Agent 通过锚点操作编辑草案。已发布版本不可变。

### 为什么批准要求定位、背景、目标和非目标

草稿应该便宜，批准应该昂贵。一个没有说清“是什么”“现状和痛点”“要达成什么”和“明确不做什么”的项目，进入开发后一定会膨胀。定位、背景的现状与至少一个问题、至少一个目标和至少一个非目标在批准时必填；约束和风险为空只产生警告。

## 需求文档（Requirements Document）

每个项目维护一份唯一的需求文档，它回答“为谁做、做什么”。文档包含用户、子系统、功能集、故事、功能；功能描述和交互设计是功能正文中的 `字段：值` 行，UI 设计是功能下的图片列表。项目级内容属于项目文档，不在这里重复。需求文档使用与项目文档相同的锚点文档机制，人类看到渲染结果，Agent 通过锚点操作编辑草案。已发布版本不可变。

早期以 Requirement 工作项聚合为中心的实现已删除。完整结构见 [RFC 0020](../rfcs/0020-requirements-subsystem.md)。

## 文档

- 需求、设计文档、ADR 以 Markdown + YAML front matter 存放在项目 Git 仓库。
- Postgres 只存元数据（路径、Git revision、状态、版本），用于检索和一致性检查。
- 文档的评审结论落为 `Decision` 或 `Review` 对象，而不是散落在评论里。

## Decision

重要决策是一等对象。智能体提交决策时必须带：

1. 问题和上下文；
2. 至少两个选项及各自代价；
3. 判断标准；
4. 证据（测试、基准、事故记录、原型）；
5. 推荐选项。

人类（或获授权的智能体）做出选择后，决策进入 `Decided`，并成为后续变更的引用依据。可逆性为 `Irreversible` 的决策必须由人类做出。

## 不变量

1. 任何状态变更都必须有 actor、时间和前序状态；actor 只能来自登录会话。
2. Issue 不是承诺：原始反馈不得直接成为承诺；只有明确的需求分析才写入项目文档和需求文档。
3. 智能体不能批准项目文档或需求文档、不能关闭/拒绝议题、不能发布版本，除非策略显式授权。
4. 已批准的需求不能被静默编辑；范围变化必须走新的变更或取代流程。
5. 事件只追加；修正通过新事件表达。
6. 跨项目默认不可见；所有查询都带 `project_id`。
7. 项目至少保留一个 Owner；系统至少保留一个活跃 admin。
8. 归档项目只读；会话撤销或账户停用后不能继续操作。

## 生命周期对象（设计目标）

本节是 RFC 0008–0019 定义的生命周期对象索引，不是当前实现。对象按“聚合 / 记录 / 投影”分类，新增对象必须满足“独立状态机、独立权限、独立证据或独立版本”中的至少一个条件。

| 组 | 对象 | 类型 | RFC |
|---|---|---|---|
| 贯穿 | Artifact / ArtifactVersion / LineageEdge | 记录 | 0009 |
| 贯穿 | TaskLink（Task 链接列表） | 记录 | 0008/0009 |
| 贯穿 | Baseline / BaselineItem / ChangeRequest | 聚合 | 0009 |
| 贯穿 | AuxiliarySystem / ToolInvocation / Evidence | 记录 | 0010 |
| 需求 | Requirement / RequirementBaseline | 聚合 | 0011 |
| 需求 | AcceptanceScenario / Holdout | 记录 | 0011 |
| 架构 | ArchitectureBaseline / ADR / DesignDoc / Contract | 聚合 | 0012 |
| 架构 | QualityAttribute / Constraint / ComponentModel / FitnessFunction | 记录/聚合 | 0012 |
| 架构 | TechDebtItem | 聚合 | 0012 |
| 实现 | ChangeSet / Review / ReviewFinding | 记录/聚合 | 0013 |
| 测试 | TestPlan / TestCase | 聚合 | 0013 |
| 测试 | TestRun / TestResult / CoverageReport | 记录 | 0013 |
| 质量 | QualityGate / ScanResult / SBOM / Waiver | 记录 | 0013 |
| 发布 | Release / ReleaseCandidate | 聚合 | 0014 |
| 发布 | Environment / Deployment / ConfigVersion / HealthSignal | 记录 | 0014 |
| 运维 | Incident（IssueKind::Incident） / Postmortem | 聚合/记录 | 0014 |
| 反馈 | Feedback / ImprovementAction | 记录 | 0014 |
| 退役 | RetirementPlan | 聚合 | 0014 |
| Agent | AgentDefinition / AgentVersion | 聚合 | 0015 |
| Agent | ToolDefinition / ModelConfig / PromptVersion / AgentBinding | 记录 | 0015 |
| Agent | EvaluationSuite / EvaluationRun / AgentTrajectory | 聚合/记录 | 0015 |
| 度量 | MetricDefinition / MetricSnapshot / CostRecord | 记录 | 0016 |
| 产品与组合 | Vision / Strategy / Opportunity | 聚合/记录 | 0017 |
| 立项 | PortfolioDecision / SuccessMetric | 聚合/记录 | 0017 |
| 知识 | KnowledgeItem / Document / KnowledgeLink | 记录 | 0018 |
| 上下文 | ContextBundle / ContextItem / MemoryEntry | 记录 | 0018 |
| 治理 | Policy / PolicyDecision / Capability | 记录 | 0019 |
| 风险 | RiskRegister / RiskAcceptance / Waiver | 聚合/记录 | 0019 |
| 审计 | AuditRecord / ComplianceEvidence | 记录 | 0019 |
| 数据 | DataClassification | 记录 | 0019 |

核心原则：

- 内容在 Git 或对象存储，元数据和状态在 Postgres；不产生第二真相。
- 外部事实（commit、PR、CI run、部署、监控）只存引用和哈希。
- 图、看板、血缘视图、度量是投影，不是编辑入口。
- 已批准产物和基线不可变；变更通过 ChangeRequest 和新版本表达。

详见 [RFC 0008](../rfcs/0008-software-lifecycle-model.md) 和各专项 RFC。
