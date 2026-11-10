# 内置流程

当前实现 9 个平台流程，随二进制发布，启动时播种为平台定义的 v1。它们是系统目录的一部分，不是业务数据：没有项目、账户或 Task 会被自动创建。

RFC 0008 设计目标为 16 个流程；下表后半部分是尚未实现的计划项，落地时必须按“新增流程”更新 `definitions/`、`builtin.rs` 的 `SOURCES` 和本文档。

内置流程由平台随应用发布：启动时如果代码中的内置定义与数据库 v1 不一致，会以代码定义更新 v1。用户发布的流程版本仍然不可变；旧 Task 固定原版本继续运行。

| id | 标题 | 用途 | 状态 |
|---|---|---|---|
| `project` | Project lifecycle | 项目根 Task，串起从目标澄清到迭代的完整生命周期 | 已实现 |
| `project_brief_writing` | Project brief writing | 先澄清目标，再撰写并评审项目简报；`task.created` 的 action 创建统一澄清 Issue，Clarifier → Brief Writer → 文风 Reviewer → 内容 Reviewer → 验收 | 已实现 |
| `requirements_discovery` | Requirements discovery | 发现初始需求池（独立流程；项目主流程已跳过） | 已实现 |
| `requirements_analysis` | Requirements analysis | 结构化需求文档；Analyst → Style/Requirements/Feasibility Reviewer → 人类评审 | 已实现 |
| `architecture_design` | Architecture design | 架构设计与评审 | 已实现 |
| `roadmap` | Roadmap | 制定路线图 | 已实现 |
| `iteration` | Iteration | 规划、实现、评审和验收一个迭代 | 已实现 |
| `release` | Release | 候选、验证、批准、部署和观察 | 已实现 |
| `project_review` | Project review | 项目复盘 | 已实现 |
| `requirements_baseline` | Requirements baseline | 固定一批已批准需求并生成基线 | 计划 |
| `detailed_design` | Detailed design | 模块、接口、数据、错误、任务分解 | 计划 |
| `test_planning` | Test planning | 测试策略、用例、holdout、环境 | 计划 |
| `test_execution` | Test execution | 运行、覆盖率、缺陷、证据 | 计划 |
| `change_request` | Change request | 影响分析、验证计划、批准、关闭 | 计划 |
| `incident_response` | Incident response | 分级、止损、修复、复盘 | 计划 |
| `project_retirement` | Project retirement | 依赖、数据、迁移、归档 | 计划 |

## Project lifecycle

根 Task 没有角色，也不接收 operation；它只通过 `child.state_changed` 事件推进。创建时：

1. 进入 `goal`；
2. `set_state in_progress`；
3. 创建 `project_brief_writing` 子 Task。

每个 stage 声明 `outcome` 变量。子 Task 完成时，event action 根据其 `result` 写入 `outcome`；对应的 transition 再推进 stage 或完成任务：

| Stage | 创建的子流程 | 子 Task 完成后的转移 |
|---|---|---|
| `goal` | `project_brief_writing` | `approved` → `requirements_analysis`；失败结果 → 完成任务，`result = 'failed'` |
| `requirements_analysis` | `requirements_analysis` | `approved` → `architecture`；失败结果 → 完成任务，`result = 'failed'` |
| `architecture` | `architecture_design` | `approved` → `roadmap`；失败结果 → 完成任务，`result = 'failed'` |
| `roadmap` | `roadmap` | `approved` → `iteration`；失败结果 → 完成任务，`result = 'failed'` |
| `iteration` | `iteration` | `approved` → 完成任务，`result = 'approved'`；失败结果 → 完成任务，`result = 'failed'`；其他结果 → 自转移并创建下一个迭代 |

失败结果指 `failed`、`infeasible` 或 `obsolete`。`iteration` 的自转移会重新执行 `created`，标题为 `Iteration {count_children('iteration') + 1}`，所以编号按已创建的子 Task 递增。

根 Task 不会自动重试失败；它以 `failed` 完成后停在固定版本，等待人工处理。

## Project brief writing

分为澄清和撰写项目简报两个阶段：

| Stage | active roles | operations |
|---|---|---|
| `clarify` | `clarifier` | `started`；`completed` 写 `completed = true` → `brief` |
| `brief` | `brief_writer` | `started`；`completed` 写 `outcome = 'completed'` → `style_review`；`continue_clarification` 写 `outcome = 'continue'` → `clarify` |
| `style_review` | `style_reviewer` | `approved` 写 `decision = 'approved'` → `content_review`；`rejected` 写 `decision = 'rejected'` → `brief` |
| `content_review` | `content_reviewer` | `approved` 写 `decision = 'approved'` → `awaiting_acceptance`；`rejected` 写 `decision = 'rejected'` → `brief` |
| `acceptance` | `acceptor` | `approved` 写 `decision = 'approved'`，完成并设置 `result = 'approved'`；`rejected` 写 `decision = 'rejected'` → `brief` |

各智能体的输入、任务和操作：

- **Clarifier**（`clarify`）：输入是澄清 Issue（`task_links_list` 找到 `clarifies` 链接，`issue_get` 读取标题、描述和全部评论）。任务是一次只问一个问题：每轮最多发一条 `issue_comment`，评论里只包含一个问题；发完结束本轮，等下一轮再问下一个。没有未回答问题后，先在 Issue 里发一条交接评论，说明澄清结束、接下来将撰写项目简报，然后调用 `completed`。它不写项目简报，也不调用项目简报工具。
- **Brief Writer**（`brief`）：输入是 Issue 评论、当前项目简报和其批注（`issue_get`、`project_brief_get`、`project_brief_annotations_list`、草稿工具）。任务是在服务端草稿上填写并提交简报。它必须处理每一条批注：接受的修改写进简报，不接受的也要 resolve 并写明理由，不能留下未解决批注。信息不足时先在 Issue 里发 `issue_comment` 列出具体问题，再调用 `continue_clarification` 回到澄清阶段。只有当**没有未解决批注**且 **Issue 中的重点内容已被简报覆盖**时，才能提交草稿并调用 `completed` 进入文风评审。
- **Style / Content Reviewer**：输入是 `project_brief_get` 和已有批注；任务是用 `project_brief_annotate` 写具体意见。只要添加了任何批注（或仍有上一轮未解决的批注）就调用 `rejected`；只有完全没有批注时才调用 `approved`。打回时任务回到 `brief`，由 Brief Writer 修改。
- **Acceptor**：人类或其他账户；批准后任务完成并设置 `result = 'approved'`，拒绝则回到 `brief`。

Clarifier、Brief Writer、Style Reviewer 和 Content Reviewer 都是 agent 角色，由 PM 在任务启动前统一分配。它们都通过通用的 `load_skill` 工具加载名为 `project_brief_writing` 的 [项目简报撰写技能](../../skills/project-brief-writing.md)，该技能定义简报应该写什么、不应该写什么，以及每个字段的质量门槛。人类批准简报后，control-plane 只在任务处于 `acceptance` 时发送保留操作 `accept`，不会跳过评审。

## Iteration

| Stage | active roles | operation |
|---|---|---|
| `planning` | `planner` | `started`；`plan_ready` 写 `completed = true` → `implementation` |
| `implementation` | `implementer` | `started`；`implementation_complete` 写 `completed = true` → `review` |
| `review` | `reviewer` | `review_approved` 写 `decision = 'approved'` → `acceptance`；`changes_requested` 写 `decision = 'changes_requested'` → `implementation` |
| `acceptance` | `acceptor` | `approved` 写 `decision = 'approved'`，完成并设置 `result = 'approved'`；`rejected` 写 `decision = 'rejected'` → `implementation` |

每个 stage 的 `started` 没有 action，也不触发 transition；它只用于把 `pending` 推进到 `in_progress`。`completed` 和 `decision` 是 stage 变量，进入 stage 时重置。`reviewer` 的 `min_actors` 为 0，评审可以留空。

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
| `work` | 第一位角色（`discoverer` / `planner` / `reviewer`） | `started`；`completed` 写 `completed = true` → `awaiting_acceptance` |
| `awaiting_acceptance` | `acceptor` | `approved` 写 `decision = 'approved'`，完成并设置 `result = 'approved'`；`rejected` 写 `decision = 'rejected'` → `work` |

`requirements_analysis` 的 `analyst` 是 agent 角色，由 Requirements Analyst 模板承接；人类发布需求文档后，平台发送保留操作 `accept` 直接完成任务。它的完整链路是：

```text
work (analyst)
  → style_review (style_reviewer agent)
  → requirements_review (requirements_reviewer agent)
  → feasibility_review (feasibility_reviewer agent)
  → human_review (acceptor human)
  → ended (result = approved)
```

每个 review stage 的 `rejected` 回到 `work`；Reviewer 用 `requirements_annotate` 写字段级意见，Analyst 读取并解决后再提交。`human_review` 也支持打回，打回原因作为文档注释保存。

`architecture_design` 多一个评审环节：

| Stage | active roles | operation |
|---|---|---|
| `work` | `architect` | `started`；`completed` 写 `completed = true` → `review` |
| `review` | `reviewer` | `review_approved` 写 `decision = 'approved'` → `awaiting_acceptance`；`review_rejected` 写 `decision = 'rejected'` → `work` |
| `awaiting_acceptance` | `acceptor` | `approved` 写 `decision = 'approved'`，完成并设置 `result = 'approved'`；`rejected` 写 `decision = 'rejected'` → `work` |

所有角色的 `kind` 都是 `any`，人类和 Agent 都可以绑定。`work` 和 `review` 的 `completed` / `decision` 是 stage 变量，进入 stage 时重置。项目生命周期通过子 Task 的 `approved`、`failed`、`infeasible`、`obsolete` 结果协调这些流程。

## 修改内置流程

- **发布新版本**：平台管理员在「流程」页创建平台流程或编辑已有平台流程，发布后项目创建和新的子 Task 使用新版本；已有 Task 不受影响。
- **数据迁移**：在发版时插入显式的新版本，保持 v1 原样。启动时会校验 v1 与嵌入 JSON 一致，不一致直接拒绝启动。
- **不要**直接改 `crates/domain/src/task/definitions/*.json` 来“修复”线上 v1；那只会让下一次启动失败。
- 新增内置流程需要同时更新 `definitions/`、`builtin.rs` 的 `SOURCES` 和文档；启动会校验引用完整性。
