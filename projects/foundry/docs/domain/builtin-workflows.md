# 内置流程

当前实现 42 个平台流程，随二进制发布，启动时播种为平台定义的 v1。它们是系统目录的一部分，不是业务数据：没有项目、账户或 Task 会被自动创建。

RFC 0008 的完整生命周期还包括需求基线、变更请求、事故响应和项目退役；下表后半部分是尚未实现的计划项，落地时必须按“新增流程”更新 `definitions/`、`builtin.rs` 的 `SOURCES` 和本文档。

内置流程由平台随应用发布：启动时如果代码中的内置定义与数据库 v1 不一致，会以代码定义更新 v1。用户发布的流程版本仍然不可变；旧 Task 固定原版本继续运行。

| id | 标题 | 用途 | 状态 |
|---|---|---|---|
| `empty` | Empty (no-op) | 不声明阶段和角色；用于测试和空 Task | 已实现 |
| `project` | Project lifecycle | 旧项目生命周期模板（不再是根流程） | 已实现 |
| `software_development` | Software development | 统一的全生命周期根流程：评估代码状态 → 澄清/确认（有代码时先通过 git 读代码）→ 需求发现/分析 → 交互/UI → 架构 → 详细设计 → 测试计划 → 迭代开发 → 测试执行 → 发布 → 持续反馈 → 长期运行 | 已实现 |
| `feedback_cycle` | Continuous feedback cycle | 并行审计需求、技术设计、代码、测试和产品试用，去重后转成后续 Issue | 已实现 |
| `requirements_audit` | Requirements audit | 需求反馈审计叶子流程 | 已实现 |
| `design_audit` | Design audit | 技术设计反馈审计叶子流程 | 已实现 |
| `code_audit` | Code audit | 代码反馈审计叶子流程 | 已实现 |
| `test_audit` | Test audit | 测试反馈审计叶子流程 | 已实现 |
| `product_audit` | Product audit | 产品试用反馈审计叶子流程 | 已实现 |
| `maintenance_development` | Maintenance development | 长期运行的维护根流程；创建分诊子任务，适合已经完成生命周期、只需要持续维护的项目 | 已实现 |
| `autonomous_software_development` | Autonomous software development | 早期完整生命周期根流程：目标澄清 → 需求 → 交互/UI → 架构 → 详细设计 → 测试计划 → 迭代开发 → 测试执行 → 设计基线 → 发布 → 长期运行 | 已实现 |
| `daily_check` | Daily check | 并行创建五个审计子任务，汇总后发现进入设计/开发闭环 | 已实现 |
| `goal_clarification` | 项目文档撰写 | 澄清并撰写项目文档；`task.created` 的 action 创建统一澄清 Issue，Clarifier → Writer → 文风 Reviewer → 内容 Reviewer → 人类审批 | 已实现 |
| `requirements_discovery` | Requirements discovery | 发现初始需求池（独立流程；项目主流程已跳过） | 已实现 |
| `requirements_analysis` | Requirements analysis | 需求锚点文档；Analyst → Style/Requirements/Feasibility Reviewer → 人类评审 | 已实现 |
| `interaction_design` | Interaction design | 维护需求文档的交互设计子系统树；Designer → Style/Interaction Reviewer → 人类评审 | 已实现 |
| `ui_design` | UI design | 维护需求文档的 UI设计子系统树；Designer → Style/UI Reviewer → 人类评审 | 已实现 |
| `architecture_design` | Architecture design | 架构锚点文档；Architect → Reviewer → 人类评审 | 已实现 |
| `detailed_design` | Detailed design | 详细设计锚点文档；Designer → Reviewer → 人类评审 | 已实现 |
| `test_planning` | Test planning | 测试计划锚点文档；Test Planner → Reviewer → 人类评审 | 已实现 |
| `test_execution` | Test execution | 测试报告锚点文档；Executor → Reviewer → 人类评审 | 已实现 |
| `roadmap` | Roadmap | 制定路线图 | 已实现 |
| `iteration` | Iteration | 规划、实现、评审和验收一个迭代；开发证据写入开发记录 | 已实现 |
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
- `software_development` 是推荐的新项目根流程：先评估代码状态；有代码时澄清、需求、交互/UI、架构、详细设计及其评审智能体通过 `git clone/fetch/checkout` 在 `/work/<project-id>/source` 读取代码，再把结论交给人类确认；无代码时正常澄清。流程不再先跑 `design_baseline_initial`；Arche 设计基线由后续设计一致性维护/反馈审计按需建立。
- `autonomous_software_development` 保留为早期完整生命周期实现；`maintenance_development` 用于只需长期维护的项目。
- 人类只承担三类工作：创建项目、回答澄清、审批。其他撰写、实现、评审、测试、合入、发布均由平台 Agent 执行。
- 项目设置中的“跳过所有人工审批”开启后，除目标澄清外，所有人类审批阶段自动通过：
  - 锚点文档由项目 Owner 自动批准，随后进入下一阶段；
  - 设计空间合并请求由平台自动审批并执行快进合入；
  - 代码变更、路线图、需求发现、项目复盘等人工审批由任务引擎自动完成。
- 目标澄清始终保留人类参与；即使开启跳过审批，项目文档仍需要人类批准。
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

创建合并请求时，Foundry 同时创建这个 Task，并把项目里的一位真人成员绑定到 `reviewer` 角色；通常优先选择不是创建者的成员。合并请求没有独立的审批状态或审批人表，页面和 API 都从 Task 读取 `allowed_operations`。快进合并由 Foundry 执行，成功后发送 `merge` operation 完成任务。

## Project lifecycle

根 Task 没有角色，也不接收 operation；它只通过 `child.state_changed` 事件推进。创建时：

1. 进入 `goal`；
2. `set_state in_progress`；
3. 创建 `goal_clarification` 子 Task。

每个 stage 声明 `outcome` 变量。子 Task 完成时，event action 根据其 `result` 写入 `outcome`；对应的 transition 再推进 stage 或完成任务：

| Stage | 创建的子流程 | 子 Task 完成后的转移 |
|---|---|---|
| `goal` | `goal_clarification` | `approved` → `requirements_analysis`；失败结果 → 完成任务，`result = 'failed'` |
| `requirements_analysis` | `requirements_analysis` | `approved` → `interaction_design`；失败结果 → 完成任务，`result = 'failed'` |
| `interaction_design` | `interaction_design` | `approved` → `ui_design`；失败结果 → 完成任务，`result = 'failed'` |
| `ui_design` | `ui_design` | `approved` → `architecture`；失败结果 → 完成任务，`result = 'failed'` |
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
| `human_review` | `acceptor` | `approved` 写 `decision = 'approved'`，完成并设置 `result = 'approved'`；`rejected` 写 `decision = 'rejected'` → `writing` |

`clarifier`、`writer`、`style_reviewer` 和 `content_reviewer` 都是 agent 角色。Clarifier 只负责在澄清 Issue 中提问，不写文档；Writer 读取澄清记录和项目信息，用 `document` 工具维护项目文档；两个 Reviewer 不修改文档，用 `document` 工具 `action = "content"` 阅读，用 `action = "annotate"` 写锚点标注；Reviewer 打回时任务回到 `writing`。人类批准当前草案后，control-plane 只在任务处于 `human_review` 时发送保留操作 `accept`，不会跳过评审。

## Iteration

| Stage | active roles | operation |
|---|---|---|
| `implementation` | `implementer` | `started`；`implementation_complete` 写 `completed = true` → `review` |
| `review` | `reviewer` | `review_approved` 写 `decision = 'approved'` → `acceptance`；`changes_requested` 写 `decision = 'changes_requested'` → `implementation` |
| `acceptance` | `acceptor` | `approved` 写 `decision = 'approved'`，完成并设置 `result = 'approved'`；`rejected` 写 `decision = 'rejected'` → `implementation` |

`acceptor` 是 `any`（人类可以承担），`implementer` 和 `reviewer` 是 `agent`。每个 stage 的 `started` 没有 action，也不触发 transition；它只用于把 `pending` 推进到 `in_progress`。`completed` 和 `decision` 是 stage 变量，进入 stage 时重置。`acceptance` 进入时绑定 `kind = development` 的文档审阅关联，`implementation` 进入时释放它。

开发阶段的代码在项目工作区中由 Implementer 编写。Implementer 运行构建和测试，把变更集、代码引用、命令结果和证据写入开发记录；Development Reviewer 独立检查工作区 diff 并重跑构建/测试；人类在验收阶段批准当前开发记录。control-plane 发现开发记录已批准且任务处于 `acceptance` 时发送保留操作 `accept`，完成任务。

## 详细设计、测试计划与测试执行

`detailed_design`、`test_planning`、`test_execution` 使用与 `architecture_design` 相同的 `work → review → human_review` 结构：

| 流程 | work 角色 | review 角色 | 文档 kind |
|---|---|---|---|
| `detailed_design` | `designer`（agent） | `reviewer`（agent） | `design` |
| `test_planning` | `test_planner`（agent） | `reviewer`（agent） | `test_plan` |
| `test_execution` | `executor`（agent） | `reviewer`（agent） | `test_report` |

设计、测试计划、测试报告和开发记录都使用锚点文档；进入 `human_review`（开发记录为 `acceptance`）时绑定文档审阅关联，回到 `work`（开发记录为 `implementation`）时释放。人类批准当前草案后，control-plane 发送保留操作 `accept` 完成任务。

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

`requirements_analysis` 的 `analyst` 是 agent 角色，由 Requirements Analyst 模板承接；需求文档使用 `kind = requirements` 的锚点文档，人类批准当前草案后，平台发送保留操作 `accept` 直接完成任务。它的完整链路是：

```text
work (analyst)
  → style_review (style_reviewer agent)
  → requirements_review (requirements_reviewer agent)
  → feasibility_review (feasibility_reviewer agent)
  → human_review (acceptor human)
  → ended (result = approved)
```

每个 review stage 的 `rejected` 回到 `work`；Reviewer 用 `document` 工具 `action = "annotate"` 写锚点标注，Analyst 读取并解决后再提交。`human_review` 也支持打回，打回原因作为文档标注保存。

## 交互设计和 UI 设计

`interaction_design` 和 `ui_design` 使用与需求分析相同的评审结构，但只修改需求文档中各自的内容：

```text
work (designer)
  → style_review (style_reviewer agent)
  → interaction_review / ui_review (reviewer agent)
  → human_review (acceptor human)
  → ended (result = approved)
```

- `interaction_design` 维护需求文档的「交互设计」子系统树：与需求子系统同构，按子系统 → 功能集 → 功能组织，并在子系统层增加信息架构和页面清单；不写视觉样式或技术实现。
- `ui_design` 维护需求文档的「UI设计」子系统树：与交互设计使用同一套子系统和页面清单，每个页面写布局、信息层级、组件、状态和设计稿图片。
- 两个流程各自有 Designer、Style Reviewer、Reviewer 和人类验收；review 打回回到 `work`，人类打回也回到 `work`。
- 项目主流程在需求分析批准后依次创建这两个子 Task，全部批准后才进入 `architecture`。

## 文档审阅关联

进入人类审批阶段时，任务通过 `document_review_link` created effect 关联当前文档草稿：

```json
"created": [
  {"document_review_link": {"kind": "project", "action": "bind"}}
]
```

- `kind` 是 `project`、`requirements`、`architecture`、`design`、`development`、`test_plan` 或 `test_report`。
- `bind` 写入 `anchor_document_review_links`，同一文档同时最多一个未释放关联。
- 进入 `writing` / `work` 时用 `action = "release"` 释放关联；任务完成或异常退出时由 control-plane 扫描兜底释放。
- 文档页根据未释放关联和任务当前阶段决定是否显示“批准 / 拒绝”按钮；按钮携带 `task_id`，提交时同时做文档决策、任务操作和释放关联。
- control-plane 仍会扫描 `human_review` 任务并补绑、清理残留关联，作为 effect 失败或旧数据的兜底。

`architecture_design` 维护 `kind = "architecture"` 的锚点文档。Architect 读取项目文档和需求文档，加载 `architecture-document-writing-guide` 与 `architecture-design` 技能；Reviewer 加载 `architecture-document-writing-guide` 与 `architecture-review` 技能，用锚点标注写评审意见。人类在架构设计页批准草案后，control-plane 发送保留操作 `accept` 完成任务。

流程多一个评审环节：

| Stage | active roles | operation |
|---|---|---|
| `work` | `architect` | `started`；`completed` 写 `completed = true` → `review` |
| `review` | `reviewer` | `review_approved` 写 `decision = 'approved'` → `human_review`；`review_rejected` 写 `decision = 'rejected'` → `work` |
| `human_review` | `acceptor` | `approved` 写 `decision = 'approved'`，完成并设置 `result = 'approved'`；`rejected` 写 `decision = 'rejected'` → `work` |

`architect` 和 `reviewer` 的 `kind` 是 `agent`，`assignment` 指向对应的 workflow；`acceptor` 的 `kind` 是 `human`，`assignment` 是 `acceptor` 身份。`work` 和 `review` 的 `completed` / `decision` 是 stage 变量，进入 stage 时重置。项目生命周期通过子 Task 的 `approved`、`failed`、`infeasible`、`obsolete` 结果协调这些流程。

## 执行方式：确定性代码优先

平台按“能用代码完成就不用 LLM”执行任务：

- **平台代码直接完成**：Task 状态机、阶段转移、子任务创建/等待、触发器防重叠、跳过人工审批、锚点文档自动批准、设计空间 MR 自动快进合入、Task Dispatcher 分配、Agent 生命周期管理。
- **确定性运行时（不调用 LLM）**：`software_development:assessor`、`code_change:tester`、`code_change:merger`、`design_iteration:dispatcher`、`design_iteration:verifier`、`development_iteration:dispatcher`、`development_iteration:verifier`、`code_summary_group:merger`、`design_consistency_maintenance:scanner`、`release:release_manager`、`release:verifier`、`release:deployer`、`release:observer`。这些运行时代码直接读取项目、执行 git/shell/scripts、操作 Issue/任务/MR、写任务评论并发送阶段操作。
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

如果同名触发器已存在则复用。创建成功后根任务进入 `running` 并长期保持运行；用户可以在项目页手动运行触发器。

`daily_check` 由触发器创建，`audits` 阶段一次性并行创建五个子任务：

1. `requirements_audit`：需求检查；
2. `design_audit`：技术设计检查；
3. `code_audit`：代码检查；
4. `test_audit`：测试检查；
5. `product_audit`：产品试用检查。

五个子任务全部结束后进入 `synthesis` 阶段，由汇总智能体读取所有子任务评论，去重、归并并把后续工作分流到设计池/开发池。任一审计失败时本轮检查结果为 `failed`。`feedback_cycle` 使用相同的并行审计和汇总结构。

触发器在创建任务前检查自己上一轮的任务：只要该任务还不是 `Completed`（包括 `Pending`、`InProgress` 和待重试的 `Failed`），本轮就跳过，避免同一触发器重叠运行。`design_iteration` 和 `development_iteration` 默认使用 `continuous` 调度：一轮验收通过后立即回到分派阶段继续处理池中任务，直到池为空才结束；空闲时由调度器按退避间隔重新检查。
