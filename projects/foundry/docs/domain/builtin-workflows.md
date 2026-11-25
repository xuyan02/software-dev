# 内置流程

当前实现 44 个平台流程，随二进制发布，启动时播种为平台定义的 v1。它们是系统目录的一部分，不是业务数据：没有项目、账户或 Task 会被自动创建。

RFC 0008 的完整生命周期还包括需求基线、变更请求、事故响应和项目退役；下表后半部分是尚未实现的计划项，落地时必须按“新增流程”更新 `definitions/`、`builtin.rs` 的 `SOURCES` 和本文档。

内置流程由平台随应用发布：启动时如果代码中的内置定义与数据库 v1 不一致，会以代码定义更新 v1。用户发布的流程版本仍然不可变；旧 Task 固定原版本继续运行。

| id | 标题 | 用途 | 状态 |
|---|---|---|---|
| `empty` | Empty (no-op) | 不声明阶段和角色；用于测试和空 Task | 已实现 |
| `project` | Project lifecycle | 旧项目生命周期模板（不再是根流程） | 已实现 |
| `software_development` | Software development | 迭代式根流程：创建项目澄清、需求/设计/开发/测试四个长期迭代和分诊；阶段验收通过时自动创建下一阶段池内 Issue，测试通过后创建 release 子任务 | 已实现 |
| `feedback_cycle` | Continuous feedback cycle | 并行审计需求、技术设计、代码、测试和产品试用，去重后转成后续 Issue | 已实现 |
| `requirements_iteration` | Requirements iteration | 从需求池选批，创建 `requirements_analysis` 任务，验收后关闭 Issue | 已实现 |
| `test_iteration` | Test iteration | 从测试池选批，创建 `test_execution` 任务，验收后关闭 Issue | 已实现 |
| `requirements_audit` | Requirements audit | 需求反馈审计叶子流程 | 已实现 |
| `design_audit` | Design audit | 技术设计反馈审计叶子流程 | 已实现 |
| `code_audit` | Code audit | 代码反馈审计叶子流程 | 已实现 |
| `test_audit` | Test audit | 测试反馈审计叶子流程 | 已实现 |
| `product_audit` | Product audit | 产品试用反馈审计叶子流程 | 已实现 |
| `maintenance_development` | Maintenance development | 长期运行的维护根流程；创建分诊子任务，适合已经完成生命周期、只需要持续维护的项目 | 已实现 |
| `autonomous_software_development` | Autonomous software development | 早期完整生命周期根流程：目标澄清 → 需求（含交互/UI）→ 架构 → 详细设计 → 测试计划 → 迭代开发 → 测试执行 → 设计基线 → 发布 → 长期运行 | 已实现 |
| `daily_check` | Daily check | 并行创建需求设计检查、技术设计检查、代码检查、日常测试，汇总后进入四个 Issue 池 | 已实现 |
| `goal_clarification` | 项目文档撰写 | 澄清并撰写项目文档；Clarifier → Writer → 文风 Reviewer → 内容 Reviewer → 人类合并请求审批 | 已实现 |
| `requirements_discovery` | Requirements discovery | 发现初始需求池（独立流程；项目主流程已跳过） | 已实现 |
| `requirements_analysis` | Requirements analysis | 在设计空间 requirement 空间中工作；Analyst → Style/Requirements/Feasibility Reviewer → 人类合并请求审批 | 已实现 |
| `interaction_design` | Interaction design | 手动/legacy；交互设计已并入需求分析 | 已停用 |
| `ui_design` | UI design | 手动/legacy；UI 设计已并入需求分析 | 已停用 |
| `architecture_design` | Architecture design | 旧锚点文档流程 | 已停用 |
| `detailed_design` | Detailed design | 旧锚点文档流程 | 已停用 |
| `test_planning` | Test planning | 旧锚点文档流程 | 已停用 |
| `test_execution` | Test execution | 测试证据写入任务评论；Executor → Reviewer → 人类评审 | 已实现 |
| `roadmap` | Roadmap | 制定路线图 | 已实现 |
| `iteration` | Iteration | 规划、实现、评审和验收一个迭代；开发证据写入任务评论 | 已实现 |
| `release` | Release | 候选、验证、批准、部署和观察 | 已实现 |
| `project_review` | Project review | 项目复盘 | 已实现 |
| `design_space_merge` | Design-space merge | 设计空间合并请求的人类审核与快进合并 | 已实现 |
| `code_summary_group` | Code summary group | 按文件组总结设计，经过文风和内容审查后自动合入集成分支 | 已实现 |
| `design_consistency_maintenance` | Technical design consistency | 扫描代码变化、分组、驱动组任务、集成审查、人类审批和最终合入 | 已实现 |
| `issue_triage` | Issue triage | 分诊 Issue、判断根因、创建设计/开发任务 | 已实现 |
| `design_change` | Design change | 分析设计影响、修改设计空间、设计审查 | 已实现 |
| `code_change` | Code change | 修复计划、实现、代码评审、测试、合入 | 已实现 |
| `style_inspection` | Code style inspection | 检查现有代码风格并上报 Issue | 已实现 |
| `logic_inspection` | Code logic inspection | 检查现有代码逻辑并上报 Issue | 已实现 |
| `cohesion_inspection` | Code cohesion inspection | 检查内聚/耦合并上报 Issue | 已实现 |
| `consistency_inspection` | Code-design consistency inspection | 检查代码与设计空间一致性并上报 Issue | 已实现 |
| `design_inspection` | Design inspection | 检查设计文档、边界和接口并上报 Issue | 已实现 |
| `requirements_baseline` | Requirements baseline | 固定一批已批准需求并生成基线 | 计划 |
| `change_request` | Change request | 影响分析、验证计划、批准、关闭 | 计划 |
| `incident_response` | Incident response | 分级、止损、修复、复盘 | 计划 |
| `project_retirement` | Project retirement | 依赖、数据、迁移、归档 | 计划 |


## 自主生产线与人工角色

- 项目只能由人类账户创建；智能体不能创建项目。
- `software_development` 是推荐根流程：项目创建后直接创建项目澄清、需求/设计/开发/测试四个长期迭代和分诊。澄清、需求、设计、开发、测试智能体通过 Arche 读取项目说明书和源码；阶段验收通过时用池内 Issue 交接，测试通过后创建 `release` 子任务。
- `autonomous_software_development` 保留为早期完整生命周期实现；`maintenance_development` 用于只需长期维护的项目。
- 人类只承担三类工作：创建项目、回答澄清、审批。其他撰写、实现、评审、测试、合入、发布均由平台 Agent 执行。
- 项目设置中的“跳过所有人工审批”开启后，所有人类审批阶段自动通过，包括设计空间合并请求：
  - 设计空间合并请求由平台自动审批并执行快进合入；缺少 MR 时先自动创建；
  - 目标澄清、项目文档、需求分析、设计、代码变更、路线图、需求发现、项目复盘等人工审批都由平台自动完成。
- 发布约定：Agent 在项目工作区执行 `scripts/release.sh`、`scripts/verify.sh`、`scripts/deploy.sh`、`scripts/observe.sh`（存在时）；缺失时使用项目构建/测试，部署和观察必须由项目提供脚本，避免伪造成功。

## 技术设计一致性维护

`software_development` 是推荐的项目根流程；`design_consistency_maintenance` 作为可被触发器或子任务调用的流程保留。`dispatch` 阶段由 Scanner 智能体完成基础同步、变化扫描和分组，并创建 `code_summary_group` 子任务。子任务串行执行，依次经过 summary、文风审查、内容审查、合入集成分支和删除组分支。

所有组完成后，模块架构师依次执行 `structure_survey`（盘点系统和元素）和 `structure_synthesis`（逐系统划分模块、绑定唯一成员、写模块文档），然后由独立的结构审查和文风审查两个阶段把关。通过后创建集成分支到 DS 主分支的最终 MR，进行集成内容审查和人类审批；合入前清理设计评论，合入后删除集成分支。

组任务失败进入 `failed` 阶段，可由项目维护者重试或放弃；重试复用同一 Task 和同一 MR。

## Code summary group

`code_summary_group` 处理一个约 50 个文件的组：

| Stage | 角色 | 说明 |
|---|---|---|
| `summary` | summarizer（agent） | 只处理任务 input 中的文件，创建或复用具 MR |
| `style_review` | style_reviewer（agent） | 只检查设计文字 |
| `content_review` | content_reviewer（agent） | 检查本组全部设计内容和对象关系 |
| `merge` | merger（agent） | 快进合入集成分支；冲突转冲突解决 |
| `cleanup` | merger（agent） | 删除组分支 |
| `conflict_resolution` | conflict_resolver（agent） | 重置组分支并重新生成，复用同一 MR |
| `failed` | 无 | 人类重试或放弃 |

组 MR 合入集成分支不需要人类审批。

## Design-space merge

`design_space_merge` 是合并请求专用的审核流程。合并请求本身只保存源/目标分支、创建时的两个 commit 和合并结果；审核人、审批、评论和操作历史全部来自该 Task。

| Stage | active roles | operation |
|---|---|---|
| `review` | `reviewer`（human） | `review_approved` 写 `decision = 'approved'` → `merge`；`review_rejected` 写 `decision = 'rejected'`，完成并设置 `result = 'rejected'` |
| `merge` | `reviewer`（human） | `merge` 写 `merged = true`，完成并设置 `result = 'merged'`；`close` 写 `result = 'closed'`，完成 |

创建合并请求时，Foundry 同时创建这个 Task，并把项目里的一位真人成员绑定到 `reviewer` 角色；通常优先选择不是创建者的成员。合并请求没有独立的审批状态或审批人表，页面和 API 都从 Task 读取 `allowed_operations`。快进合并由 Foundry 执行，成功后自动删除源分支（受保护分支除外），再发送 `merge` operation 完成任务。清理阶段再次删除同一分支是幂等操作。

## Project lifecycle

根 Task 没有角色，也不接收 operation；它只通过 `child.state_changed` 事件推进。创建时：

1. 进入 `goal`；
2. `set_state in_progress`；
3. 创建 `goal_clarification` 子 Task。

每个 stage 声明 `outcome` 变量。子 Task 完成时，event action 根据其 `result` 写入 `outcome`；对应的 transition 再推进 stage 或完成任务：

| Stage | 创建的子流程 | 子 Task 完成后的转移 |
|---|---|---|
| `goal` | `goal_clarification` | `approved` → `requirements_analysis`；失败结果 → 完成任务，`result = 'failed'` |
| `requirements_analysis` | `requirements_analysis` | `approved` → `architecture`；交互设计和 UI 设计已在需求阶段完成；失败结果 → 完成任务，`result = 'failed'` |
| `architecture` | `architecture_design` | `approved` → `detailed_design`；失败结果 → 完成任务，`result = 'failed'` |
| `detailed_design` | `detailed_design` | `approved` → `test_planning`；失败结果 → 完成任务，`result = 'failed'` |
| `test_planning` | `test_planning` | `approved` → `iteration`；失败结果 → 完成任务，`result = 'failed'` |
| `iteration` | `iteration` | `approved` → `test_execution`；失败结果 → 完成任务，`result = 'failed'`；其他结果 → 自转移并创建下一个迭代 |
| `test_execution` | `test_execution` | `approved` → 完成任务，`result = 'approved'`；失败结果 → 完成任务，`result = 'failed'` |

失败结果指 `failed`、`infeasible` 或 `obsolete`。`iteration` 的自转移会重新执行 `created`，标题为 `Iteration {count_children('iteration') + 1}`，所以编号按已创建的子 Task 递增。

根 Task 不会自动重试失败；它以 `failed` 完成后停在固定版本，等待人工处理。

## 项目文档撰写

| Stage | active roles | operation |
|---|---|---|
| `clarification` | `clarifier` | `started`；`completed` 写 `completed = true` → `writing` |
| `writing` | `writer` | `started`；`completed` 写 `completed = true` → `style_review` |
| `style_review` | `style_reviewer` | `approved` 写 `decision = 'approved'` → `content_review`；`rejected` 写 `decision = 'rejected'` → `writing` |
| `content_review` | `content_reviewer` | `approved` 写 `decision = 'approved'` → `human_review`；`rejected` 写 `decision = 'rejected'` → `writing` |
| `human_review` | `acceptor` | `merge` 写 `decision = 'approved'`，合并 MR 后完成并设置 `result = 'approved'`，同时创建需求池 Issue `Requirements analysis: <项目名>`；`rejected` 写 `decision = 'rejected'` → `writing` |

`clarifier`、`writer`、`style_reviewer` 和 `content_reviewer` 都是 agent 角色。Writer 在 `project-document-<task-id>` 分支撰写并推送 `project/document`，两个 Reviewer 检出同一分支审查；Reviewer 打回时任务回到 `writing`。内容审查通过后由内容审查智能体创建到 `source` 的合并请求；人类在 MR 页面审批，平台快进合并 `source` 后发送 `human_review:merge` 完成任务。任务完成时创建 `kind = task_request` 的 Issue 到需求池，描述要求按 `project/document` 进行需求分析；`requirements_iteration` 的 Dispatcher 会把它分派为 `requirements_analysis` 子任务。

## 阶段交接

每个阶段验收通过时创建下一阶段的池内 Issue，保证主链不会停在空池上：

| 当前阶段完成 | 下一跳 |
|---|---|
| 项目说明书 MR 合入 | `pool = requirements` 的 `Requirements analysis: <项目名>` Issue |
| `requirements_analysis` 完成 | 为每个需求 Issue 创建 `pool = design` 的 `Design: <需求标题>` Issue |
| `design` / `design_baseline` 完成 | 为每个设计 Issue 创建 `pool = development` 的 `Development: <设计标题>` Issue |
| `code_change` 完成 | 为每个开发 Issue 创建 `pool = tests` 的 `Test: <开发标题>` Issue |
| `test_execution` 完成 | 在测试迭代下创建 `release` 子任务 |

交接 Issue 使用 `handoff:<next_pool>:<source_issue_id>` 标签去重；同一来源 Issue 已有未关闭的交接时不会重复创建。`issue_create` 的 `pool` 参数让 Issue 直接以 `Pooled` 状态创建，不再经过分诊。

所有设计空间写入都必须有合并请求：`goal_clarification`、`design`、`design_change`、`design_correction`、`design_baseline` 进入 `human_review` 时，control-plane 会检查任务是否已有 MR，没有就按标准分支名补建；任务 API 和页面在没有 MR 时拒绝 `merge`/`approved`，只能从 MR 页面审批。标准分支为 `project-document-<task-id>`、`design/<task-id>`、`design-change/<task-id>`、`design-correction/<task-id>`、`design-baseline/<task-id>`。

## Iteration

| Stage | active roles | operation |
|---|---|---|
| `implementation` | `implementer` | `started`；`implementation_complete` 写 `completed = true` → `review` |
| `review` | `reviewer` | `review_approved` 写 `decision = 'approved'` → `acceptance`；`changes_requested` 写 `decision = 'changes_requested'` → `implementation` |
| `acceptance` | `acceptor` | `approved` 写 `decision = 'approved'`，完成并设置 `result = 'approved'`；`rejected` 写 `decision = 'rejected'` → `implementation` |

`acceptor` 是 `any`（人类可以承担），`implementer` 和 `reviewer` 是 `agent`。每个 stage 的 `started` 没有 action，也不触发 transition；它只用于把 `pending` 推进到 `in_progress`。`completed` 和 `decision` 是 stage 变量，进入 stage 时重置。

开发阶段的代码在项目工作区中由 Implementer 编写。Implementer 运行构建和测试，把变更集、代码引用、命令结果和证据写入任务评论；Development Reviewer 独立检查工作区 diff 并重跑构建/测试；人类在验收阶段审批。

## 测试执行

`test_execution` 使用 `work → review → human_review` 结构：`executor`（agent）启动真实产物、像用户一样操作、观察日志和截图，把证据写入任务评论；`reviewer`（agent）核对证据；人类在 `human_review` 审批。

## Release

| Stage | active roles | operation |
|---|---|---|
| `candidate` | `release_manager` | `started`；`completed` 写 `completed = true` → `verification` |
| `verification` | `verifier` | `verified` 写 `outcome = 'verified'` → `approval`；`verification_failed` 写 `outcome = 'failed'` → `candidate` |
| `approval` | `approver` | `approved` 写 `decision = 'approved'` → `deployment`；`rejected` 写 `decision = 'rejected'` → `candidate` |
| `deployment` | `deployer` | `deployed` 写 `outcome = 'deployed'` → `observation`；`deploy_failed` 写 `outcome = 'failed'` → `candidate` |
| `observation` | `observer` | `healthy` 写 `outcome = 'healthy'`，完成并设置 `result = 'released'`；`unhealthy` 写 `outcome = 'unhealthy'` → `candidate` |

失败分支都回到 `candidate`，由 release manager 重新准备候选。每个 stage 同样声明无 action 的 `started`。

## 单个评审流程

`requirements_discovery`、`roadmap`、`project_review` 结构相同：

| Stage | active roles | operation |
|---|---|---|
| `work` | 第一位角色（`discoverer` / `planner` / `reviewer`） | `started`；`completed` 写 `completed = true` → `human_review` |
| `human_review` | `acceptor` | `approved` 写 `decision = 'approved'`，完成并设置 `result = 'approved'`；`rejected` 写 `decision = 'rejected'` → `work` |

`requirements_analysis` 的 `analyst` 是 agent 角色，由 Requirements Analyst 模板承接；需求写入设计空间 Arche requirement 空间，位于 `requirements/<task-id>` 分支。评审通过后由 Feasibility Reviewer 创建到 `design_space_branch` 的合并请求，人类审批合并后任务完成。它的完整链路是：

```text
work (analyst)
  → style_review (style_reviewer agent)
  → requirements_review (requirements_reviewer agent)
  → feasibility_review (feasibility_reviewer agent)
  → human_review (acceptor human)
  → ended (result = approved)
```

每个 review stage 的 `rejected` 回到 `work`；Reviewer 用 `arche comment add` 写评审意见，Analyst 读取并解决后再提交。`human_review` 只通过合并请求审批，没有 MR 时批准会被拒绝。

## 执行方式：确定性代码优先

平台按“能用代码完成就不用 LLM”执行任务：

- **平台代码直接完成**：Task 状态机、阶段转移、子任务创建/等待、触发器防重叠、跳过人工审批、设计空间 MR 自动快进合入、Task Dispatcher 分配、Agent 生命周期管理。
- **确定性运行时（不调用 LLM）**：`software_development:assessor`、`code_change:tester`、`code_change:merger`、`requirements_iteration:dispatcher`、`requirements_iteration:verifier`、`design_iteration:dispatcher`、`design_iteration:verifier`、`development_iteration:dispatcher`、`development_iteration:verifier`、`test_iteration:dispatcher`、`test_iteration:verifier`、`code_summary_group:merger`、`design_consistency_maintenance:scanner`、`release:release_manager`、`release:verifier`、`release:deployer`、`release:observer`。这些运行时代码直接读取项目、执行 git/shell/scripts、操作 Issue/任务/MR、写任务评论并发送阶段操作。
- **混合运行时**：`design_consistency_maintenance:integrator` 的集成 MR 创建、最终合入和 clean up，`design_baseline:integrator` 的 cleanup，以及 `design` / `design_change` / `design_correction` 审查者的 cleanup 阶段由代码完成；同一角色的设计内容审查、冲突解决等判断阶段仍走 LLM。
- **已有确定性控制流程**：`code_summary_group:summarizer`、`design_baseline:architect` 使用专用图运行时，按固定步骤执行；内容判断仍可能调用模型。
- **需要 LLM 的环节**：目标澄清、需求/交互/UI/架构/详细设计写作与评审、测试计划与测试报告、代码实现与代码评审、分诊、代码/设计检查、反馈审计与汇总、发布风险审批，以及各类 merge conflict 的语义解决。这些环节需要理解、创作、审查或权衡，不能仅靠固定命令完成。
- **人类负责审批**：所有 `kind = human` 的角色只做批准/拒绝；跳过审批开启后由平台代码自动通过，不调用 LLM。

新增或修改 workflow 时，机械步骤应优先实现为确定性运行时或平台操作；只有确实需要判断的步骤才绑定 LLM Agent。

## 修改内置流程

- **发布新版本**：平台管理员在「流程」页创建平台流程或编辑已有平台流程，发布后项目创建和新的子 Task 使用新版本；已有 Task 不受影响。
- **数据迁移**：在发版时插入显式的新版本，保持 v1 原样。启动时会校验 v1 与嵌入 JSON 一致，不一致直接拒绝启动。
- **不要**直接改 `crates/domain/src/task/definitions/*.json` 来“修复”线上 v1；那只会让下一次启动失败。
- 新增内置流程需要同时更新 `definitions/`、`builtin.rs` 的 `SOURCES` 和文档；启动会校验引用完整性。

## Issue 分诊与变更闭环

Issue 是问题的唯一记录；Issue 创建后自动生成 `issue_triage` 任务。分诊智能体读取 Issue、代码和设计证据，判断是代码缺陷、设计缺陷、需求缺陷、测试缺陷、重复、不是问题或需要补充信息。

- 纯代码问题：创建 `code_change` 任务，input 包含 `issue_id`、`code_branch` 和 `fix_plan`；
- 设计问题：按顺序创建 `design_change` 和依赖它的 `code_change`；
- 不需要修改：写分诊报告，建议拒绝或补充信息，由人类处置。

`design_change` 经过 `analysis → design → design_review`，设计修改直接提交到设计空间 `source` 分支。`code_change` 经过 `plan → implement → code_review → test → merge`，代码在 `code/<issue-id>` 分支开发，评审和测试通过后合入目标分支，并把 Issue 推进到 `resolved`。

## 代码检查任务

项目页的“代码检查”表单会创建独立的检查任务。每类检查使用自己的角色和智能体：

| 流程 | 角色 | 检查重点 |
|---|---|---|
| `style_inspection` | `style_inspector` | 命名、格式、惯例、可读性 |
| `logic_inspection` | `logic_inspector` | 正确性、边界、错误处理、并发、资源 |
| `cohesion_inspection` | `cohesion_inspector` | 内聚性、耦合度、职责边界 |
| `consistency_inspection` | `consistency_inspector` | 代码与设计空间的一致性 |
| `design_inspection` | `design_inspector` | 设计文档、系统/模块边界、接口、依赖 |

检查智能体不修改代码或设计；发现问题通过 `issue_create` 上报。Issue 创建后自动进入 `issue_triage`，由分诊智能体决定生成设计或开发任务。

## 软件开发根流程

`software_development`、`maintenance_development`、`autonomous_software_development` 都可以作为项目根流程，其中 `software_development` 是推荐的统一流程：

```text
setup（project_setup 智能体）
  → running
```

`setup` 阶段创建一个名为“日常检查”的项目触发器：

```json
{
  "name": "日常检查",
  "workflow_id": "daily_check",
  "schedule": "daily@09:00",
  "input": {}
}
```

如果同名触发器已存在则复用。创建成功后根任务进入 `running` 并长期保持运行；用户可以在项目页手动运行日常检查。

`daily_check` 由触发器创建，`audits` 阶段一次性并行创建四个子任务：

1. `requirements_audit`：需求设计检查；
2. `design_audit`：技术设计检查；
3. `code_audit`：代码检查；
4. `test_audit`：日常测试。

四个子任务全部结束后进入 `synthesis` 阶段，由汇总智能体读取所有子任务评论，去重、归并并把后续工作分流到需求池/设计池/开发池/测试池。任一审计失败时本轮检查结果为 `failed`。`feedback_cycle` 仍是包含产品试用审计的五维反馈流程。

触发器在创建任务前检查自己上一轮的任务：只要该任务还不是 `Completed`（包括 `Pending`、`InProgress` 和待重试的 `Failed`），本轮就跳过，避免同一触发器重叠运行。

四个迭代不使用触发器。控制平面在 Issue 进入对应池时确保该池只有一个活动迭代任务：不存在就创建，处于 `waiting` 就发送 `waiting:work_arrived` 唤醒；池空时任务保持 InProgress 等待。控制平面监听 `foundry_issue_changes`，并提供 30 秒对账作为通知丢失的兜底。
