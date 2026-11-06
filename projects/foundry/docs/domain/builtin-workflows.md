# 内置流程

9 个平台流程随二进制发布，启动时播种为平台定义的 v1。它们是系统目录的一部分，不是业务数据：没有项目、账户或 Task 会被自动创建。

v1 不可变。修改内置流程只能由平台管理员发布新版本，或在发版时增加数据迁移插入显式的新版本；不能改 JSON 让已播种的 v1 变化。旧 Task 固定旧版本继续运行。

| id | 标题 | 用途 |
|---|---|---|
| `project` | Project lifecycle | 项目根 Task，串起从目标澄清到迭代的完整生命周期 |
| `goal_clarification` | Goal clarification | 澄清项目目标并获得验收 |
| `requirements_discovery` | Requirements discovery | 发现初始需求池 |
| `requirements_analysis` | Requirements analysis | 分析需求池 |
| `architecture_design` | Architecture design | 架构设计与评审 |
| `roadmap` | Roadmap | 制定路线图 |
| `iteration` | Iteration | 规划、实现、评审和验收一个迭代 |
| `release` | Release | 候选、验证、批准、部署和观察 |
| `project_review` | Project review | 项目复盘 |

## Project lifecycle

根 Task 没有角色，也不接收 operation；它只通过 `child.state_changed` 事件推进。创建时：

1. 进入 `goal`；
2. `set_state in_progress`；
3. 创建 `goal_clarification` 子 Task。

每个 stage 声明 `outcome` 变量。子 Task 完成时，event action 根据其 `result` 写入 `outcome`；对应的 transition 再推进 stage 或完成任务：

| Stage | 创建的子流程 | 子 Task 完成后的转移 |
|---|---|---|
| `goal` | `goal_clarification` | `approved` → `requirements_discovery`；失败结果 → 完成任务，`result = 'failed'` |
| `requirements_discovery` | `requirements_discovery` | `approved` → `requirements_analysis`；失败结果 → 完成任务，`result = 'failed'` |
| `requirements_analysis` | `requirements_analysis` | `approved` → `architecture`；失败结果 → 完成任务，`result = 'failed'` |
| `architecture` | `architecture_design` | `approved` → `roadmap`；失败结果 → 完成任务，`result = 'failed'` |
| `roadmap` | `roadmap` | `approved` → `iteration`；失败结果 → 完成任务，`result = 'failed'` |
| `iteration` | `iteration` | `approved` → 完成任务，`result = 'approved'`；失败结果 → 完成任务，`result = 'failed'`；其他结果 → 自转移并创建下一个迭代 |

失败结果指 `failed`、`infeasible` 或 `obsolete`。`iteration` 的自转移会重新执行 `created`，标题为 `Iteration {count_children('iteration') + 1}`，所以编号按已创建的子 Task 递增。

根 Task 不会自动重试失败；它以 `failed` 完成后停在固定版本，等待人工处理。

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

`goal_clarification`、`requirements_discovery`、`requirements_analysis`、`roadmap`、`project_review` 结构相同：

| Stage | active roles | operation |
|---|---|---|
| `work` | 第一位角色（`clarifier` / `discoverer` / `analyst` / `planner` / `reviewer`） | `started`；`completed` 写 `completed = true` → `awaiting_acceptance` |
| `awaiting_acceptance` | `acceptor` | `approved` 写 `decision = 'approved'`，完成并设置 `result = 'approved'`；`rejected` 写 `decision = 'rejected'` → `work` |

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
