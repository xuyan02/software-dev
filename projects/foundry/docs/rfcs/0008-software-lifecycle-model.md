# RFC 0008：软件生命周期模型

- 状态：Draft，设计待评审
- 日期：2026-11-09
- 关联：[ADR 0001 Rust workspace 与六边形架构](../adr/0001-rust-workspace-and-hexagonal-architecture.md)、[ADR 0002 控制平面形态](../adr/0002-control-plane-form.md)、[ADR 0003 Git 存内容，Postgres 存状态](../adr/0003-git-content-postgres-state.md)、[ADR 0004 Issue 是问题空间，Requirement 是承诺空间](../adr/0004-issue-vs-requirement.md)、[RFC 0003 Task Workflow 模型](0003-task-workflow-model.md)、[RFC 0005 Workflow 定义、版本与可视化编辑器](0005-workflow-definitions.md)、[RFC 0006 Workflow 变量、Operation、Event 与转移](0006-workflow-variables.md)、[RFC 0009 产物血缘、基线与变更管理](0009-artifact-lineage-baselines-and-change.md)、[RFC 0010 辅助系统注册与产物契约](0010-auxiliary-systems-and-evidence.md)、[RFC 0017 产品、组合与立项管理](0017-product-portfolio-and-initiation.md)、[RFC 0018 知识、文档与上下文管理](0018-knowledge-documentation-and-context.md)、[RFC 0019 治理、安全、合规与数据](0019-governance-security-compliance-and-data.md)
- 关系：本 RFC 是生命周期总纲；需求、架构、实现与验证、发布与运营、Agent 架构、度量、产品组合、知识、治理分别由 RFC 0011–0019 展开。本文定义它们共同遵守的阶段模型、对象分类、门禁和证据语义。

## 0. 结论

Foundry 管理的不是“需求到发布”的一段流水线，而是**从产品判断到退役的完整软件生命周期**。生命周期由三类东西驱动：

1. **阶段（stage）**：项目在生命周期中的位置。阶段不是新对象，也不是第二套状态机；它是根 Task 的 workflow stage。
2. **工作项（work item）**：阶段内被推进的对象，例如 Issue、Requirement、Task、Release、Incident。每个工作项有自己的状态机。
3. **产物（artifact）**：阶段之间传递的可版本化结果，例如需求文档、架构决策、契约、补丁、测试报告、部署记录。产物有血缘、基线和变更。

核心决定：

1. **生命周期阶段是根流程的 stage。** 不引入 `LifecycleStage` 聚合，不维护第二套项目状态。项目根 Task 的当前 stage 就是项目生命周期阶段；阶段之间的转移由根 workflow 声明，并受门禁约束。
2. **每个阶段必须声明入口条件、出口条件、所需产物、所需证据和允许的角色。** 没有声明的阶段不可执行；没有出口条件的阶段不可进入。
3. **阶段不可静默跳过。** 跳过必须产生显式 waiver，记录批准人、理由、有效期和补偿措施。
4. **产物先于状态。** 状态转换的合法性由“所需产物是否齐全、证据是否达到信任等级”决定，而不是由“谁点了按钮”决定。
5. **管理主干与辅助系统分离。** Foundry 管理对象、流程、权限、证据、审计和血缘；需求分析、架构推理、测试生成、发布编排等由可替换的辅助系统提供，输出是产物或证据，不是批准（见 RFC 0010）。
6. **证据分级。** 自述、工具验证、独立验证、人类批准、签证实证是不同信任等级；关键门禁必须声明最低等级。
7. **产物血缘、基线和变更管理是贯穿机制。** 它们不是某个阶段的附属功能，而是所有阶段共享的连接件（见 RFC 0009）。
8. **对象按需新增。** 只有需要独立身份、独立版本或长期状态的对象才成为聚合；一次性结果用 Record 表达，查询视图用 Projection 表达，外部事实（commit、CI run、部署）通过引用接入，不复制。
9. **不重造通用执行面。** Git、CI、沙箱、监控、密钥管理、模型推理通过适配器接入；Foundry 重造的是 AI 原生的管理、约束、证据和 Agent 架构层。
10. **评测和独立证据前置。** Agent 能力评测、holdout 验收、独立验证从第一阶段就参与，不放到最后。
11. **安全、成本和可复现性是一等约束。** 每个阶段都要记录配置、模型、提示词、工具和数据版本；不可复现的产物不能作为关键证据。
12. **生命周期目标是收敛，不是产出。** 度量必须包含返工率、缺陷逃逸率、变更失败率、人工接管率和成本，而不只是完成的任务数。

## 1. 目标与非目标

### 1.1 目标

- 覆盖完整软件生命周期：产品与组合、立项、需求、架构与设计、实现、评审、测试、质量与安全、发布、部署与环境、运维与事故、反馈与复盘、退役与归档。
- 让每个阶段都有明确的输入、输出、角色、证据和门禁，而不是只有阶段名。
- 让 AI Agent 和人类在同一套对象、流程和证据上工作，区别只在权限、入口和节奏。
- 让产物可追溯、可复现、可审计：从目标到需求、架构、任务、提交、测试、发布、事故、反馈形成闭环血缘。
- 让辅助系统可插拔、可替换、可评测，避免把某一家工具焊死进领域模型。
- 让生命周期可以分级验收：L1 单任务、L2 单迭代、L3 单项目、L4 自举、L5 组织。
- 让“是否真的支持整个软件开发”成为可测量的问题，而不是口号。

### 1.2 非目标

- 不实现完全无人的软件公司；关键不可逆决策仍由人类或明确授权的策略做出。
- 不自动决定“做什么产品”。产品方向、市场判断和价值取舍是人类或明确授权的产品策略的职责。
- 不重写 Git、CI、容器运行时、监控、密钥管理或模型推理；这些是执行面，通过适配器接入。
- 不把流程图、架构图或看板当作第二真相；图是模型或 AST 的投影。
- 不要求第一阶段就实现所有阶段；阶段按依赖顺序落地，但设计必须一次完整。
- 不实现通用工作流引擎、通用 BPMN 或图灵完备脚本；阶段与工作项仍受 RFC 0005/0006/0007 的受限语言约束。
- 不实现多租户组织模型；数据模型预留隔离键，但不提前实现组织管理。
- 不把辅助系统输出直接当成批准；辅助系统没有审批权。
- 不实现跨项目的实时协同、CRDT 或自动合并；冲突显式处理。
- 不承诺“做完就能替代整个工程组织”；是否支持取决于 Agent 能力、评测成熟度和生产反馈闭环。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| Lifecycle（生命周期） | 项目从产品判断到退役的完整阶段序列。 |
| Stage（阶段） | 根 Task 的 workflow stage；项目在生命周期中的位置。 |
| Work item（工作项） | 阶段内被推进的对象：Issue、Requirement、Task、Release、Incident 等。 |
| Artifact（产物） | 阶段之间传递的、可版本化的结果；有 id、类型、版本、内容和血缘。 |
| Artifact version（产物版本） | 产物的一次不可变快照；内容寻址。 |
| Evidence（证据） | 支撑状态转换的可验证产物或记录；有信任等级。 |
| Gate（门禁） | 进入下一状态或下一阶段前必须满足的条件集合：产物、证据、策略、批准。 |
| Baseline（基线） | 某一时刻被批准的一组产物版本；用于变更比较和发布追溯。 |
| Change request（变更请求） | 对基线或已批准产物的修改提案；带影响分析、验证计划和批准。 |
| Auxiliary system（辅助系统） | 帮助人或 Agent 完成分析、设计、生成、验证、编排的可替换系统；输出是产物或证据。 |
| Execution plane（执行面） | 实际执行的外部系统：Git、CI、沙箱、部署、监控、模型、工具。 |
| Trust level（信任等级） | 证据的可信程度：自述、工具验证、独立验证、人类批准、签证实证。 |
| Closure level（闭环层级） | L1 单任务、L2 单迭代、L3 单项目、L4 自举、L5 组织。 |
| Waiver（豁免） | 有期限、有批准、有补偿措施的跳过或降低门禁的显式记录。 |

不变量：

1. 项目的生命周期阶段由根 Task 的当前 stage 唯一确定；不存在独立的项目阶段字段。
2. 每个阶段必须有入口条件、出口条件、所需产物、所需证据和允许角色；缺一不可。
3. 每次阶段转移必须评估门禁；门禁结果、评估时间、评估者和证据快照写入事件日志。
4. 每个产物有唯一 id 和至少一个不可变版本；内容寻址，元数据与内容分离。
5. 已批准或已进入基线的产物不可原地修改；变更必须产生新的产物版本和 ChangeRequest。
6. 证据必须有来源、时间、信任等级和内容引用；自述证据不能单独满足关键门禁。
7. 辅助系统的输出是草稿、分析或证据，不是批准；批准只能由策略或授权的人类/Agent 做出。
8. 任何跳过阶段、降低门禁或接受风险的动作都必须产生 Waiver，并记录补偿措施和有效期。
9. 外部执行面的事实（commit、CI run、部署、监控）通过引用接入；Foundry 不复制其内容作为第二真相。
10. 所有关键产物必须记录生成或验证时使用的工具、模型、提示词、配置和数据版本，保证可复现。
11. 不可逆操作（发布、删除、覆盖、外部写）必须有人类或显式授权的策略批准。
12. 任何阶段的度量都必须包含质量、成本和收敛指标，不能只有吞吐。
13. 生命周期数据按项目隔离；跨项目读取按不存在处理。
14. Agent 不能批准自己的产物，不能关闭自己造成的 Incident，不能发布自己未经验证的变更；除非策略显式授权并留痕。

## 3. 生命周期全景

### 3.1 分组

| 组 | 阶段 | 回答的问题 |
|---|---|---|
| A 产品与立项 | 0 战略与组合、1 立项与目标 | 为什么做、做到什么程度算成功 |
| B 需求 | 2 需求发现、3 需求分析、4 需求基线 | 为谁解决什么问题、怎样算解决 |
| C 架构与设计 | 5 架构设计、6 详细设计 | 打算怎么实现、权衡是什么、约束是什么 |
| D 实现与验证 | 7 实现、8 评审、9 测试、10 质量与安全 | 做出来了吗、做对了吗、可接受吗 |
| E 发布与运营 | 11 发布、12 部署与环境、13 运维与事故、14 反馈与复盘 | 怎么安全上线、线上怎么样、学到了什么 |
| F 退役 | 15 退役与归档 | 怎么安全停止、数据和依赖怎么处理 |
| G 贯穿 | 变更、配置、文档、知识、度量、安全、治理 | 如何保持一致、可追溯、可改进 |

阶段编号只是文档顺序，不是状态机编号；实际项目可以按根流程声明的顺序执行，也可以显式并行或裁剪，但必须通过门禁和 Waiver。

### 3.2 阶段表

| # | 阶段 | 目的 | 主要输入 | 主要输出 | 主要对象 | 主要证据 | 默认流程 |
|---|---|---|---|---|---|---|---|
| 0 | 战略与组合 | 决定做哪些项目、优先级和预算 | 市场、用户、组合、财务 | 组合决策、预算、路线图 | PortfolioDecision、Budget | 批准的组合决策 | RFC 0017 组合管理 |
| 1 | 立项与目标 | 澄清目标、成功指标、约束和风险 | 组合决策、干系人 | ProjectBrief、成功指标、风险 | Project、ProjectBrief | 批准的简报 | `goal_clarification` |
| 2 | 需求发现 | 收集和聚类候选需求 | 用户反馈、Issue、监控 | 需求池、分类、优先级候选 | Issue、RequirementCandidate | 分诊记录、来源链接 | `requirements_discovery` |
| 3 | 需求分析 | 把候选变成可验证的承诺 | 需求池、领域知识 | Requirement、验收场景、holdout | Requirement | 批准的需求、验收场景 | `requirements_analysis` |
| 4 | 需求基线 | 固定一批已批准需求 | Requirement | RequirementBaseline | Baseline | 基线批准 | `requirements_baseline`（新增） |
| 5 | 架构设计 | 做长期技术决策和约束 | Requirement、质量属性 | 架构基线、ADR、契约、约束 | ArchitectureBaseline、ADR、Contract | 架构评审、契约校验、fitness function | `architecture_design` |
| 6 | 详细设计 | 把架构落到模块、接口、数据和任务 | 架构基线、需求 | DesignDoc、任务分解、变更计划 | DesignDoc、Task | 设计评审、任务计划 | `detailed_design`（新增） |
| 7 | 实现 | 产生可评审的代码变更 | Task、设计、仓库 | 分支、提交、PR、构建 | Task、ChangeSet | diff、构建、单测、静态分析 | `iteration` |
| 8 | 评审 | 独立检查变更的正确性和风险 | PR、设计、证据 | Review、发现项、批准 | Review | 评审记录、批准 | `iteration` / `review` |
| 9 | 测试 | 验证行为和验收场景 | 需求、设计、PR | TestPlan、TestRun、覆盖率、缺陷 | TestPlan、TestRun、Issue | 独立测试报告、覆盖率 | `test_planning`、`test_execution`（新增） |
| 10 | 质量与安全 | 检查安全、合规、供应链和质量门禁 | PR、依赖、构建 | 扫描报告、SBOM、豁免 | QualityGate、ScanResult、Waiver | 扫描报告、SBOM、豁免批准 | 平台门禁 |
| 11 | 发布 | 决定哪些变更一起上线 | 基线、证据、风险 | Release、发布说明、回滚计划 | Release、ReleaseCandidate | 发布批准、部署计划 | `release` |
| 12 | 部署与环境 | 安全地把版本放到环境 | Release、环境、配置 | Deployment、配置版本、迁移记录 | Environment、Deployment | 部署记录、迁移日志 | `release` / `deployment` |
| 13 | 运维与事故 | 保持服务健康并响应事故 | 监控、告警、部署 | 事故、时间线、修复、复盘 | Incident、Postmortem | 健康指标、事故时间线 | `incident_response`（新增） |
| 14 | 反馈与复盘 | 把线上结果变成下一轮输入 | 用户反馈、指标、事故 | Feedback、改进任务、实验结论 | Feedback、ImprovementAction | 复盘决策、改进任务 | `project_review` |
| 15 | 退役与归档 | 安全停止系统并处理数据依赖 | 依赖、数据、合同 | 退役计划、迁移记录、归档 | RetirementPlan | 退役批准、迁移验证 | `project_retirement`（新增） |
| G | 贯穿 | 保持一致、可追溯、可改进 | 所有阶段 | 基线、变更、文档、度量 | ChangeRequest、Baseline、Metric | 变更批准、审计记录 | 平台机制 |

### 3.3 阶段的入口与出口

每个阶段在根 workflow 中声明为 stage，并至少包含：

- `entry_criteria`：进入前必须存在的产物、证据、批准。
- `exit_criteria`：离开前必须产生的产物、证据、批准。
- `active_roles`：本阶段允许操作的角色（人类/Agent/不限）。
- `operations`：本阶段允许的工作项操作。
- `required_artifacts`：必须存在的产物类型和最低版本。
- `required_evidence`：必须存在的证据类型和最低信任等级。
- `waivable`：哪些条件允许豁免、谁可以批准、最长有效期。
- `auxiliary_capabilities`：本阶段可调用的辅助能力（不是具体实现）。

阶段声明的具体语法可以复用 RFC 0005/0006 的 stage 结构，通过扩展字段或平台级配置表达；本 RFC 不引入第二套定义语言。

## 4. 阶段细化

### 4.1 产品与立项

**目的**：回答“为什么做、做到什么程度算成功、值不值得做”。

- 管理对象：Project、ProjectBrief、成功指标、约束、干系人、风险、预算。
- 辅助系统：市场/用户研究、竞品分析、机会评估、ROI 模型、可行性分析。
- 门禁：目标可度量、成功指标有基线、约束和 non-goals 明确、干系人确认、预算批准。
- 证据：批准的 ProjectBrief、目标澄清记录、风险清单。
- Agent 边界：Agent 可以收集、分析、起草；目标取舍和预算批准由人类或明确授权的产品策略完成。

### 4.2 需求

**目的**：把问题空间转成可验证的承诺空间。

- 管理对象：Issue、Requirement、RequirementBaseline、ChangeRequest。
- 辅助系统：反馈接入、聚类、去重、歧义/完整性/一致性检查、验收场景生成、holdout 候选、影响分析、可追溯性建议。
- 门禁：需求有 problem/users/acceptance/non_goals/constraints；验收场景可执行；holdout 由独立方持有；变更走 ChangeRequest。
- 证据：批准的需求版本、验收场景、holdout 清单、追溯链接。
- Agent 边界：Agent 可以起草、分析和建议；需求批准由人类或策略授权；实现 Agent 不能看到 holdout 场景。
- 详见 RFC 0011。

### 4.3 架构与设计

**目的**：做长期决策，并把决策变成可执行约束。

- 管理对象：ArchitectureBaseline、ADR、DesignDoc、Contract、QualityAttribute、Constraint、ComponentModel、TechDebtItem、FitnessFunction。
- 辅助系统：方案生成、权衡分析、建模、依赖/影响分析、契约校验、符合性检查、仿真/原型、基准测试。
- 门禁：架构基线和质量属性场景已批准；关键契约可校验；约束可执行；技术债有记录；偏离有 ADR 或豁免。
- 证据：批准的架构基线、ADR、契约校验报告、fitness function 结果、基准数据。
- Agent 边界：Agent 可以提出方案和证据；不可逆或高影响决策由人类批准；后续实现必须引用架构基线。
- 详见 RFC 0012。

### 4.4 实现与验证

**目的**：产生可评审、可验证、可发布的变更。

- 管理对象：Task、ChangeSet、Review、TestPlan、TestRun、QualityGate、ScanResult、Waiver。
- 辅助系统：代码 Agent、仓库索引、构建、lint、静态分析、自动评审、测试生成、变异/模糊测试、覆盖率、安全扫描。
- 门禁：构建通过；单测/集成测试通过；验收场景和 holdout 通过；静态分析和安全扫描达标；评审批准；无未处理的高危发现。
- 证据：diff、提交、构建日志、独立测试报告、覆盖率、扫描报告、评审记录。
- Agent 边界：Agent 可以写代码、跑测试、提交 PR；不能自我批准；不能豁免自己的失败；关闭 Issue 需要人类或策略授权。
- 详见 RFC 0013。

### 4.5 发布与运营

**目的**：安全上线、观察、响应、学习和退役。

- 管理对象：Release、ReleaseCandidate、Environment、Deployment、ConfigVersion、Rollout、HealthSignal、Incident、Postmortem、Feedback、ImprovementAction、RetirementPlan。
- 辅助系统：变更聚合、风险评估、发布说明、部署编排、监控、日志、追踪、异常检测、自动修复、实验平台、复盘助手。
- 门禁：发布证据齐全、批准完成、回滚计划可执行、环境配置版本固定、观察期通过；事故有复盘和改进任务；退役有依赖和数据计划。
- 证据：发布批准、部署记录、健康指标、事故时间线、复盘决策、退役记录。
- Agent 边界：Agent 可以准备、执行、观察和起草复盘；发布批准、不可逆部署、事故关闭由人类或策略授权。
- 详见 RFC 0014。

## 5. 对象模型

### 5.1 对象分类

| 类型 | 定义 | 例子 | 存储 |
|---|---|---|---|
| Aggregate | 有独立身份、状态机和不变量的领域对象 | Project、Issue、Requirement、Task、Release、AgentDefinition | Postgres + 事件 |
| Record | 一次性或追加式记录，不承担独立状态机 | Review、TestRun、Deployment、Evidence、ScanResult | Postgres + 对象存储 |
| Projection | 从事件或对象派生的只读视图 | Artifact 索引、血缘图、看板、度量 | Postgres 物化视图或查询 |
| External | 外部执行面的事实，按引用接入 | Commit、PR、CI run、镜像、监控指标 | 外部系统 + 引用 |
| Auxiliary output | 辅助系统产生的草稿或分析 | 需求分析报告、架构方案、测试用例候选 | Artifact + 辅助系统 |

### 5.2 对象清单

| 对象 | 类型 | 归属 RFC | 现有状态 |
|---|---|---|---|
| Project | Aggregate | 现有 | 已实现 |
| Issue | Aggregate | 现有 | 已实现 |
| Requirement | Aggregate | RFC 0011 | 早期实现，需重设计 |
| RequirementBaseline | Aggregate | RFC 0011 | 新增 |
| ChangeRequest | Aggregate | RFC 0009 | 新增 |
| ArchitectureBaseline | Aggregate | RFC 0012 | 新增 |
| ADR / DesignDoc / Contract | Aggregate 或 Record | RFC 0012 | 领域模型存在，无持久化 |
| QualityAttribute / Constraint | Record | RFC 0012 | 新增 |
| ComponentModel | Aggregate | RFC 0012 | 新增 |
| TechDebtItem | Aggregate | RFC 0012 | 新增 |
| FitnessFunction | Record | RFC 0012 | 新增 |
| Task | Aggregate | 现有 | 已实现 |
| ChangeSet | Record | RFC 0013 | 新增 |
| Review | Aggregate | RFC 0013 | 文档存在，代码无 |
| TestPlan / TestCase | Aggregate | RFC 0013 | 新增 |
| TestRun / TestResult / Coverage | Record | RFC 0013 | 新增 |
| QualityGate / ScanResult / Waiver | Record | RFC 0013 | 新增 |
| Release / ReleaseCandidate | Aggregate | RFC 0014 | 新增 |
| Environment / Deployment / ConfigVersion | Record | RFC 0014 | 新增 |
| Incident | Aggregate（IssueKind::Incident） | RFC 0014 | Issue 支持类型 |
| Postmortem / Feedback / ImprovementAction | Record | RFC 0014 | 新增 |
| RetirementPlan | Aggregate | RFC 0014 | 新增 |
| AgentDefinition / AgentVersion | Aggregate | RFC 0015 | 新增 |
| ToolDefinition / ModelConfig / PromptVersion | Record | RFC 0015 | 新增 |
| EvaluationSuite / EvaluationRun | Aggregate / Record | RFC 0015 | 新增 |
| Artifact / ArtifactVersion | Record | RFC 0009 | 新增 |
| Baseline / BaselineItem | Aggregate | RFC 0009 | 新增 |
| Evidence / EvidenceVerification | Record | RFC 0010 | 新增 |
| AuxiliarySystem / ToolInvocation | Record | RFC 0010 | 新增 |
| MetricSnapshot | Record | RFC 0016 | 新增 |
| Vision / Strategy / Opportunity | Aggregate/Record | RFC 0017 | 新增 |
| ProjectBrief / SuccessMetric / PortfolioDecision | Aggregate | RFC 0017 | 新增 |
| KnowledgeItem / Document | Record/Aggregate | RFC 0018 | 新增 |
| ContextBundle / MemoryEntry | Record | RFC 0018 | 新增 |
| Policy / PolicyDecision / Capability | Record | RFC 0019 | 新增 |
| RiskRegister / RiskAcceptance | Aggregate/Record | RFC 0019 | 新增 |
| AuditRecord / ComplianceEvidence / DataClassification | Record | RFC 0019 | 新增 |

### 5.3 对象建模规则

1. **先问是否需要独立生命周期。** 如果对象只是某个 Task 的结果，用 Record + Evidence 表达，不建聚合。
2. **先问是否需要独立版本。** 需要被基线引用、被发布固定、被变更比较的对象，才建版本化聚合。
3. **先问是否已有承载对象。** Release 可以先由 Release Task + ReleaseRecord + Evidence 表达；只有当发布需要独立状态机、独立权限和长期历史时，才升级为聚合。
4. **外部事实不复制。** Commit、PR、CI run、镜像 digest、监控指标只存引用和哈希；内容留在外部系统。
5. **投影不写回。** 血缘图、看板、度量是查询视图，不能成为修改入口。
6. **每个新对象必须有：独立状态机、独立权限、独立证据或独立版本需求中的至少一个。** 否则不建。

## 6. 阶段与流程的映射

### 6.1 根流程

项目创建时选择一个根流程（RFC 0005）。根流程的 stage 就是项目生命周期阶段。根流程通过子 Task 协调各阶段：

```text
project
  goal
  requirements_discovery
  requirements_analysis
  requirements_baseline
  architecture
  detailed_design
  iteration
  review
  test
  quality
  release
  deployment
  operations
  feedback
  retirement
```

阶段内的工作由子 Task 或工作项推进；根 Task 只负责阶段转移、收集结果和评估门禁，不直接执行具体工作。

### 6.2 新增内置流程

| 流程 id | 用途 | 说明 |
|---|---|---|
| `detailed_design` | 详细设计 | 模块、接口、数据、错误、任务分解 |
| `requirements_baseline` | 需求基线 | 固定一批已批准需求，生成基线 |
| `test_planning` | 测试计划 | 测试策略、用例、holdout、环境 |
| `test_execution` | 测试执行 | 运行、覆盖率、缺陷、证据 |
| `change_request` | 变更请求 | 影响分析、验证计划、批准 |
| `incident_response` | 事故响应 | 分级、止损、修复、复盘 |
| `project_retirement` | 退役 | 依赖、数据、迁移、归档 |

内置流程的修改遵循“v1 不可变、新版本显式迁移”的规则（见 `docs/domain/builtin-workflows.md`）。

### 6.3 阶段与工作项状态的关系

- 阶段回答“项目走到哪”，工作项状态回答“这个对象走到哪”。
- 阶段转移不直接修改工作项状态；阶段出口门禁要求工作项达到指定状态或产生指定产物。
- 一个阶段可以包含多个工作项；一个工作项可以跨阶段存在（例如一个 Release 从发布阶段持续到运维观察期）。
- 不允许用阶段代替工作项状态机，也不允许用工作项状态代替阶段。

## 7. 门禁与证据

### 7.1 门禁模型

每个门禁声明：

```json
{
  "id": "release.ready",
  "stage": "release",
  "requires": {
    "artifacts": ["release_candidate", "release_notes", "rollback_plan"],
    "evidence": [
      { "type": "test_report", "trust": "independent" },
      { "type": "security_scan", "trust": "tool_verified" },
      { "type": "approval", "trust": "human_approved" }
    ],
    "policy": "release.approver != author && release.approver.role == 'release_manager'"
  },
  "waivable": true,
  "waiver_authority": "release_manager",
  "max_waiver_hours": 72
}
```

门禁评估结果只有三种：`pass`、`blocked`、`waived`。每次评估写入事件日志，包含证据快照哈希和评估者。

### 7.2 证据信任等级

| 等级 | 含义 | 能单独满足的门禁 |
|---|---|---|
| `self_reported` | 执行者自己声明 | 只能满足低风险、可逆的检查 |
| `tool_verified` | 独立工具产生并可复现 | 大多数构建、静态分析、测试门禁 |
| `independently_verified` | 独立执行者或 holdout 验证 | 关键验收、安全、发布门禁 |
| `human_approved` | 授权人类明确批准 | 不可逆、高风险、合规门禁 |
| `attested` | 执行面签名 + 内容寻址 + 证据链完整 | 最高等级，可用于审计和发布 |

规则：

- 关键门禁不能只接受 `self_reported`。
- 同一 Agent 不能同时是执行者和唯一验证者。
- holdout 验收由独立验证方持有，执行者不可见。
- 证据失效（代码变更、依赖更新、环境变化）后必须重新验证。
- Waiver 不能替代 `attested` 证据用于不可逆发布，除非策略显式允许。

### 7.3 门禁与阶段转移

```text
请求进入下一阶段
  → 收集 required_artifacts 和 required_evidence
  → 评估 evidence trust
  → 评估 policy
  → 评估审批和职责分离
  → 结果：pass / blocked / waived
  → 写入 gate_evaluated 事件
  → pass 或有效 waiver 才允许转移
```

阻塞原因必须显示在界面上，并定位到缺失的产物或证据；不能只返回 403。

## 8. 贯穿机制

| 机制 | 内容 | 归属 RFC |
|---|---|---|
| 产品与组合 | Vision、Strategy、Opportunity、PortfolioDecision、ProjectBrief、SuccessMetric | RFC 0017 |
| 产物血缘 | Artifact、ArtifactVersion、LineageEdge、影响分析 | RFC 0009 |
| 基线与变更 | Baseline、BaselineItem、ChangeRequest、影响分析与批准 | RFC 0009 |
| 配置与版本 | 代码、配置、环境、数据 schema、模型、提示词、工具的版本引用 | RFC 0009 |
| 辅助系统 | 注册、能力、输入输出契约、调用记录、信任等级 | RFC 0010 |
| 需求管理 | Requirement、需求池、分析、验收、holdout | RFC 0011 |
| 架构与设计 | 架构基线、ADR、契约、约束、符合性 | RFC 0012 |
| 实现与验证 | 实现、评审、测试、质量与安全门禁 | RFC 0013 |
| 发布与运营 | 发布、部署、环境、事故、反馈、退役 | RFC 0014 |
| Agent 架构 | AgentDefinition、工具、模型/提示词、评测、观测 | RFC 0015 |
| 度量与自举 | 指标、成本、闭环层级、自举 | RFC 0016 |
| 知识与上下文 | 知识条目、文档、代码索引、上下文组装、记忆 | RFC 0018 |
| 治理与安全 | 角色、策略、审批、审计、合规、风险、数据 | RFC 0019 |

## 9. 数据模型

### 9.1 新增表（按 RFC 分组）

| RFC | 表 |
|---|---|
| 0009 | `artifacts`、`artifact_versions`、`artifact_edges`、`baselines`、`baseline_items`、`change_requests`、`change_impacts`、`config_refs` |
| 0010 | `auxiliary_systems`、`auxiliary_capabilities`、`tool_invocations`、`evidence`、`evidence_verifications` |
| 0011 | `requirements`、`requirement_scenarios`、`requirement_holdouts`、`requirement_baselines`、`requirement_baseline_items` |
| 0012 | `architecture_baselines`、`architecture_decisions`、`design_docs`、`contracts`、`quality_attributes`、`constraints`、`component_models`、`tech_debt_items`、`fitness_functions` |
| 0013 | `changesets`、`reviews`、`review_findings`、`test_plans`、`test_cases`、`test_runs`、`test_results`、`coverage_reports`、`quality_gates`、`scan_results`、`waivers` |
| 0014 | `releases`、`release_candidates`、`environments`、`deployments`、`config_versions`、`health_signals`、`postmortems`、`feedback`、`improvement_actions`、`retirement_plans` |
| 0015 | `agent_definitions`、`agent_versions`、`tool_definitions`、`model_configs`、`prompt_versions`、`evaluation_suites`、`evaluation_runs`、`agent_bindings`、`agent_trajectories` |
| 0016 | `metric_snapshots`、`cost_records` |

### 9.2 存储约定

- 长文本和代码内容在 Git；对象存储存大证据（报告、截图、trace、覆盖率包、SBOM）；Postgres 存元数据、状态、索引和引用。
- 所有内容引用使用 SHA-256 或 Git commit + path，保证内容寻址。
- 事件继续只追加；对象状态变更产生事件，事件携带产物/证据引用。
- 新增表必须带 `project_id`（适用时）和 `created_at`；跨项目查询默认不可见。
- 不使用通用 EAV 表承载领域字段；类型化对象用类型化列或 JSONB 快照 + 索引列。
- 迁移按 RFC 顺序拆分，避免一次大迁移阻塞。

## 10. API、MCP、CLI、Web

### 10.1 REST

| 领域 | 端点示例 |
|---|---|
| 生命周期 | `GET /api/v1/projects/{id}/lifecycle`、`POST /api/v1/projects/{id}/lifecycle/advance` |
| 阶段门禁 | `GET /api/v1/projects/{id}/stages/{stage}/gate`、`POST .../evaluate` |
| 产物 | `GET/POST /api/v1/projects/{id}/artifacts`、`GET .../artifacts/{id}/versions` |
| 血缘 | `GET /api/v1/artifacts/{id}/lineage?direction=upstream\|downstream` |
| 基线 | `GET/POST /api/v1/projects/{id}/baselines`、`POST .../baselines/{id}/diff` |
| 变更 | `GET/POST /api/v1/projects/{id}/change-requests`、`POST .../{id}/approve` |
| 证据 | `GET/POST /api/v1/projects/{id}/evidence`、`POST .../{id}/verify` |
| 辅助系统 | `GET /api/v1/auxiliary-systems`、`GET .../{id}/capabilities` |

### 10.2 MCP

Agent 通过 MCP 使用生命周期能力，至少包括：

- `lifecycle.get`：读取项目阶段、门禁和阻塞原因。
- `lifecycle.advance`：请求进入下一阶段；返回 pass/blocked/waived。
- `artifact.submit`：提交产物版本。
- `artifact.lineage`：查询上下游。
- `baseline.get` / `baseline.diff`：读取基线和差异。
- `change.request`：发起变更请求。
- `evidence.submit` / `evidence.query`：提交和查询证据。
- `auxiliary.invoke`：调用已注册的辅助能力（受策略约束）。

MCP 工具必须按项目隔离、按角色授权、记录调用审计；不能暴露跨项目数据。

### 10.3 CLI

```bash
foundry lifecycle show <project>
foundry lifecycle advance <project> --to requirements
foundry artifact list <project> --type requirement
foundry artifact lineage <artifact>
foundry baseline create <project> --name v1.0
foundry baseline diff <baseline> <other>
foundry change request <artifact> --reason "..."
foundry gate evaluate <project> --stage release
```

### 10.4 Web

- 项目生命周期看板：阶段、门禁状态、阻塞原因、下一步。
- 需求池与需求详情：承诺、验收、holdout、变更、追溯。
- 架构页：基线、ADR、契约、约束、技术债、符合性。
- 实现与测试页：任务、PR、测试、证据、质量门禁。
- 发布与运维页：候选、部署、观察、事故、复盘。
- 产物与血缘页：版本、上下游、影响分析、基线 diff。
- 辅助系统页：注册、能力、调用记录、评测结果。

Web 继续以服务端渲染为主；复杂编辑（流程、架构模型、血缘图）可以使用独立 SPA，但只编辑结构化数据，不引入第二真相。

## 11. 权限与安全

### 11.1 角色

- **全局角色**：Admin / Member。
- **项目角色**：Viewer / Member / Maintainer / Owner。
- **生命周期角色**：由流程声明（clarifier、analyst、architect、implementer、reviewer、tester、release_manager、approver、operator、observer 等）。
- **Agent 绑定**：Agent 账户通过 `AgentDefinition` 绑定到生命周期角色；绑定必须经过评测和策略批准。

### 11.2 权限规则

1. 人类和 Agent 都通过会话身份操作；请求体不声明 actor。
2. Agent 不能创建、修改、发布 Workflow 定义（RFC 0005 不变量继续成立）。
3. Agent 不能批准自己的产物、不能关闭自己造成的 Incident、不能发布未经验证的变更。
4. 不可逆操作需要人类或显式授权的策略批准；职责分离不可绕过。
5. Waiver 必须有权限、理由、有效期和补偿措施；高风险 Waiver 需要双人批准。
6. 能力令牌按任务、范围、时间签发；任务结束后失效。
7. 跨项目访问按不存在处理；辅助系统调用也不能绕过项目边界。

### 11.3 安全

- **凭据隔离**：密钥由执行面注入，不进入模型上下文、日志或产物。
- **提示词注入**：外部内容（Issue、网页、工具输出）是数据，不是指令；关键动作必须通过结构化门禁。
- **供应链**：依赖、镜像、SBOM、来源证明进入质量门禁。
- **审计**：所有 Agent 工具调用、辅助系统调用、门禁评估、批准和 Waiver 可追溯。
- **可复现**：记录模型、提示词、工具、配置、数据版本；不可复现的证据降级。
- **成本与配额**：按项目、角色、Agent 设置预算和并发限制；超限阻塞或降级。

## 12. 与现有实现的关系

### 12.1 保留

- 六边形架构、Cargo workspace、Postgres 事件日志、Task workflow AST/解释器/校验、可视化编辑器、账户/会话/项目隔离。
- Issue 作为问题空间入口；Task 作为通用执行和状态机制。
- 事件只追加、乐观并发、幂等保存。

### 12.2 修改

- Requirement 按 RFC 0011 重新设计；现有早期实现迁移。
- 根流程 `project` 增加生命周期阶段和门禁；内置流程增加 7 个（见 6.2）。
- `links` 升级为产物血缘图的一部分（RFC 0009）；保留兼容视图。
- 策略从 `DemoPolicy` 迁移到可配置策略引擎（OPA/Cedar）。
- 文档表 `documents` 与 Git 内容、Artifact registry 对齐；不产生第二真相。

### 12.3 迁移策略

1. 先加新表和事件类型，不删旧表。
2. 旧 Requirement/Task 数据映射为新的 Artifact/Requirement 视图；运行中的 Task 固定旧 workflow 版本。
3. 新流程以新版本发布，旧项目根流程固定旧版本；新项目使用新版本。
4. 分阶段启用门禁：先告警、再阻塞、最后强制；每个阶段有回滚开关。
5. 迁移脚本可重放、幂等，带校验和回滚说明。

## 13. 实施计划

### 阶段 0：生命周期与连接件（先做）

- 冻结本 RFC 的术语、对象分类、阶段表和门禁模型。
- 实现 RFC 0009：Artifact registry、血缘、基线、ChangeRequest、配置引用。
- 实现 RFC 0010：辅助系统注册、能力契约、调用记录、证据与信任等级。
- 定义根流程 v2 的阶段和门禁；先以告警模式运行。
- 出口标准：任意一个对象能产生 Artifact、血缘、基线和证据；门禁可评估但不阻塞。

### 阶段 1：执行闭环

- Task 租约、Worker、沙箱、Agent runtime、模型/工具网关、Git/CI 适配器。
- 独立验证、holdout、评测前置。
- 一条真实链路：Issue → Task → Agent → PR → 测试 → 证据 → 状态推进。
- 出口标准：L1 单任务闭环，证据可独立验证，人工接管率可测。

### 阶段 2：需求与架构

- Requirement 重设计；需求池、分析、验收、holdout、基线、变更。
- 架构基线、ADR/DesignDoc/Contract、质量属性、约束、fitness function。
- 需求与架构辅助系统接入；追溯图打通。
- 出口标准：L2 单迭代闭环；需求变更能影响分析到任务和测试。

### 阶段 3：实现、评审、测试、质量

- ChangeSet、Review、TestPlan/TestRun、覆盖率、质量与安全门禁。
- 独立测试与 holdout 验证；Waiver 流程。
- 出口标准：L2 稳定；证据链完整；缺陷逃逸率可测。

### 阶段 4：发布、部署、运维、反馈、退役

- Release/ReleaseCandidate、Environment/Deployment、健康信号、事故、复盘、反馈、退役。
- 生产反馈回流的自动化；发布门禁与回滚。
- 出口标准：L3 单项目闭环；发布观察期通过才算完成。

### 阶段 5：Agent 架构、度量、自举、规模

- AgentDefinition/版本、工具注册、评测、观测、绑定。
- 指标、成本、闭环层级仪表盘。
- 自举：Foundry 管 Foundry 自己的需求、架构、任务和发布。
- 出口标准：L4 自举；L5 组织能力按需评估。

## 14. 验收标准

### 14.1 闭环层级

| 层级 | 验收标准 | 关键指标 |
|---|---|---|
| L1 单任务 | Agent 从 Issue 到 PR 合并，证据可独立验证 | 成功率、人工接管率、成本 |
| L2 单迭代 | 规划 → 实现 → 评审 → 测试 → 验收完整跑通 | 返工率、周期时间、证据覆盖率 |
| L3 单项目 | 需求 → 架构 → 多迭代 → 发布 → 观察 → 反馈 | 缺陷逃逸率、回滚率、需求漂移 |
| L4 自举 | Foundry 的需求和迭代由 Foundry 管理 | 自举任务占比、人工干预率 |
| L5 组织 | 多项目组合、资源分配、优先级、成本 | 吞吐、可预测性、单位成本 |

### 14.2 场景验收

1. 新项目创建后，生命周期看板显示当前阶段、入口条件、出口条件和阻塞原因。
2. 需求从 Issue 进入需求池，经过分析、批准、基线，产生可追溯的验收场景和 holdout。
3. 架构设计产生基线、ADR 和可执行契约；实现任务引用基线；偏离被 fitness function 发现。
4. Agent 在沙箱中实现任务，提交 PR；独立测试和评审产生证据；门禁评估通过后才进入下一阶段。
5. 发布需要独立的测试证据、安全证据、批准和回滚计划；观察期通过后才算完成。
6. 线上事故自动生成 Incident，止损、修复、复盘、改进任务形成闭环。
7. 任一产物都能查询上下游血缘、基线版本和变更历史。
8. 跳过阶段或降低门禁必须产生 Waiver，且可审计。
9. 同一任务重复执行不产生重复副作用；事件可重放。
10. 关闭模型/工具/配置版本记录后，任何关键证据都不可复现，门禁降级并可见。

## 15. 风险与待决问题

| 风险 / 问题 | 说明 | 建议 |
|---|---|---|
| 范围膨胀 | 15 个阶段 + 辅助系统 + Agent 架构，容易一次做全 | 先做连接件和执行闭环；阶段按依赖顺序启用 |
| 证据不可信 | Agent 自证、测试同源、holdout 泄漏 | 独立验证、签名证据、评测前置 |
| 辅助系统锁定 | 把某家分析工具写进领域模型 | 注册 + 契约 + 可替换；输出只是产物 |
| 对象膨胀 | 每个阶段都建聚合 | 按 5.3 规则；先用 Record/Task/Artifact 表达 |
| 产品侧缺失 | 系统能执行但不能决定做什么 | 明确产品判断在系统边界外；人类/产品策略负责 |
| 成本失控 | Agent 和辅助系统成本不可见 | 预算、配额、成本记录、ROI 指标 |
| 安全与供应链 | Agent 写代码放大风险 | 密钥隔离、SBOM、扫描、来源证明、权限最小化 |
| 自举难度 | 系统管不好自己就无法证明价值 | L4 作为正式验收；先选一个真实项目自举 |
| 多租户 | 组织隔离、配额、合规 | 数据模型预留 `org_id`，不提前实现 |
| 策略引擎选型 | OPA/Cedar/自研 | 先 OPA；策略与流程分离，可替换 |
| 发布对象建模 | Release 是聚合还是 Task 结果 | 先 Record + Evidence；需要独立状态机时再升格 |
| 退役阶段 | 容易被忽略 | 纳入根流程；数据保留、依赖、合同、归档都要门禁 |

**待决问题**：

1. 第一阶段先闭环哪条链路：Bug 修复、单迭代、还是自举？
2. 辅助系统首批自研还是集成：需求分析、架构推理、测试生成分别选什么？
3. holdout 场景由谁持有、如何防泄漏？
4. 策略引擎选 OPA 还是 Cedar？
5. 发布对象是否一开始就建聚合？
6. 多项目/多组织何时进入？
7. 生命周期阶段是全部默认启用，还是按项目类型裁剪？

---

本 RFC 是总纲。后续 RFC 0009–0019 分别定义连接件和阶段细节；任何与本文冲突的地方，以本文的阶段模型、对象分类和门禁语义为准，并回写本文件。
