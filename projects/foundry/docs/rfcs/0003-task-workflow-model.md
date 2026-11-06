# RFC 0003：Task Workflow 模型与项目运行设计

- 状态：Draft，设计待评审
- 日期：2026-11-03
- 关联：[ADR 0002 控制平面形态](../adr/0002-control-plane-form.md)、[ADR 0003 Git 存内容，Postgres 存状态](../adr/0003-git-content-postgres-state.md)、[ADR 0004 Issue 是问题空间，Requirement 是承诺空间](../adr/0004-issue-vs-requirement.md)、[ADR 0006 项目隔离](../adr/0006-project-isolation.md)、[RFC 0001 Foundry MVP](0001-foundry-mvp.md)、[RFC 0004 持久事件总线](0004-durable-event-bus.md)
- 取代：本 RFC 取代 [RFC 0002](0002-project-issue-workflow-and-policy.md) 中关于 Issue/Task 状态、stage、项目流程对象和 Issue 权限的部分；RFC 0002 中关于策略、审批、证据、发布和外部执行面的总体方向仍可参考。

## 0. 结论

Foundry 的工作单位统一为 **Task**。原 Issue 改名为 Task；现有代码中的旧 Task 聚合（Todo / InProgress / Blocked / Review / Done）退出新模型。

核心模型：

1. **每个 Task 必须挂一个流程**。流程是程序化脚本，不是给智能体的建议。
2. **state 与 stage 正交**：
   - `state` 是所有 Task 共享的执行/完成状态；
   - `stage` 由流程定义，可以为空或不展示。
3. **流程脚本由事件驱动**，持续监听 Task 创建、输入信号、actor 分配变化、子 Task 终止等事件；脚本只能修改自己 Task 的 `state`、`stage`、当前 active actors 及其 `work`。
4. **角色、actor 与 active assignment 分离**：
   - 流程声明需要哪些角色；
   - PM 智能体把账户绑定到角色；
   - 流程脚本决定当前哪些 actor 活跃，可以同时激活多个；每个 active actor 可以有独立的工作说明；
   - 不再有单一 `assignee`；当前 assignee 是 `task_actors` 中 `active = true` 的 actor 集合。
5. **输入是纯信号**：`started`、`completed`、`approved`、`rejected` 等，不携带业务内容；内容放在附件、Requirement 或评论里。
6. **只有 Task 参与流程**。需求、文档、迭代计划、设计文档等可以被人类和智能体看见，但不能直接驱动 `state`/`stage`。
7. 父 Task 的流程脚本只能创建子 Task（指定 workflow id、标题和依赖）和观察子 Task 的终止事件，不能修改子 Task 的 `state`/`stage` 或 active actors；父 Task 何时完成由父流程定义，系统不会因“所有子 Task 完成”而自动完成父 Task，但 `Complete` 要求所有直接子 Task 已 `Completed`。
8. **简单验收由 Task 自身覆盖**：进入验收 stage，把验收角色下的 actor 设为 active，`approved` 完成，`rejected` 退回原 stage；不创建验收子 Task。复杂验收才创建子 Task。
9. **需求池装 Requirement，不装 Task**。这里的 Requirement 是粗略能力描述，不是需求分析结果；它不参与流程。
10. **PM 智能体只绑定角色，不设置 active actor、`work`，也不修改 `state`/`stage`**。
11. 项目默认流程由 Root Task 驱动：目标澄清 → 需求发现 → 需求分析 → 架构设计 → Roadmap → 迭代（逐个创建）。Root 没有 active actor，也不接受外部输入，只根据子 Task 的终止事件推进；发布和项目复盘暂不接入默认流程，等有明确的触发需求再定义。
12. **首期不支持取消**：没有 `cancel` 输入、`CancelRequested` 事件或 `Cancelled` 状态；Task 只能通过 `Complete` 结束，失败/废弃用 `result` 标记。
13. **Task 视图带操作记录**：`Task.operations` 按写入顺序倒序展示该 Task 的操作历史；它来自不可变 `events`，不新增 Task 列或操作表。
14. **Task 视图带评论列表**：人类/智能体可以追加评论进行交流或补充信息；评论不驱动流程，也不进入操作记录。
15. **Task 可以声明创建时固定的依赖**：依赖是同一项目内已存在的 Task；全部依赖 `Completed` 前，Task 不能开始。依赖满足只解除启动限制，是否启动仍由该 Task 自己的 workflow 决定。
16. **调度状态独立于 `TaskState`**：`Task.dispatch_state` 由 `state` 和依赖派生为 `Blocked` / `Ready` / `Running` / `Closed`；`Blocked` 表示依赖未完成，`Ready` 表示依赖已完成但尚未开始，供系统查询待调度任务。它不是持久化字段，也不由 workflow 设置。
17. **跨 Task 推进通过平台事件总线**：Task workflow 是 `task-workflow` consumer；子 Task 终止、调度唤醒都写成目标 Task 事件流里的触发事件，由 dispatcher 在目标 Task 的行锁内处理。写入方不修改目标状态，也不锁目标 Task。详见 [RFC 0004](0004-durable-event-bus.md)。

## 1. 目标与非目标

### 1.1 目标

- 用一个统一的 Task 模型承载项目生命周期、迭代、验收和日常工程工作。
- 让流程可编程、可测试、可审计，而不是依赖智能体“自觉遵守”。
- 让人类和智能体操作同一套对象；区别只在权限、入口和当前角色。
- 支持人类在任意验收点介入，也支持把验收完全交给智能体；由项目配置决定。
- 支持一个项目多次迭代；发布 workflow 保留为模板，首期不接入 Root 默认流程。
- 跨 Task 的推进复用平台持久事件总线（[RFC 0004](0004-durable-event-bus.md)），Task 模块不另造通知、轮询或投递机制。
- 保留现有 Rust workspace、Postgres、HTTP/MCP/CLI/Web、事件架构。

### 1.2 非目标

- 本设计不写代码，不决定最终授权规则的粒度。
- 不在首期实现可视化流程编辑器或用户自定义脚本；流程定义先是受信任的 Rust 代码。
- 不重新实现 Git、CI、沙箱、部署或模型推理。
- 不规定所有项目必须使用同一套阶段；默认流程是模板，可替换。
- 不把“人类批准”设成硬编码前提；人类验收点由项目配置决定。
- 不在首期实现 per-actor 完成状态或会签/阈值聚合；多个 active actor 的完成语义由 workflow 自己决定。
- 不把事件总线做成 Task 专属设施；`task-workflow` 只是它的一个 consumer，订阅、游标和 dispatcher 属于平台。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| Task | 流程的唯一参与单位。原 Issue 改名而来。 |
| state | 所有 Task 共享的执行状态：`Pending` / `InProgress` / `Completed`。 |
| stage | 流程定义的阶段，如 `discovery`、`awaiting_acceptance`、`implementation`；可以为空。 |
| workflow | Task 的 `workflow` 字段（workflow id）指向的程序化流程定义，包含角色、stage、输入、效果。 |
| role | 流程声明的职责位，如 `discoverer`、`acceptor`、`planner`。 |
| actor | 通过登录会话认证的具体账户；也是事件里的操作者身份。 |
| actor binding | PM 把某个账户分配到某个 role 的成员记录；存于 `task_actors`，初始 `active = false`。 |
| active actor / assignee | 当前由 workflow 选中、正在承担工作的账户；存储上就是 `task_actors` 中 `active = true` 的成员行。active actor 集合就是 assignees，不是独立表或字段。 |
| work | 某个 assignee 在该 Task 上的独立工作说明；只对该 actor 展示。 |
| input signal | 发给 Task 流程的纯信号，如 `started`、`completed`、`approved`。 |
| result | Task 进入终止状态时由流程定义的标签，如 `approved`、`rejected`、`failed`、`obsolete`；父流程据此分支。 |
| Requirement | 需求池中的粗略能力描述；不是 Task，不参与流程。 |
| attachment | 文档/证据/计划等附件；可见，但不作为流程输入。 |
| dependency | 当前 Task 的前置 Task；全部为 `Completed` 前，当前 Task 不能开始。 |
| dispatch state | 由 `state` 和依赖派生的只读调度状态：`Blocked` / `Ready` / `Running` / `Closed`；不是 `TaskState`。 |
| PM agent | 项目级管理智能体；只添加角色成员和做升级协调，不改 Task 状态。 |
| event bus | 平台级持久事件设施：`events` 日志 + 订阅 + 消费游标 + dispatcher；见 RFC 0004。 |
| trigger event | 会触发某个 consumer 处理的事件类型；Task workflow 只订阅触发事件。 |
| consumer cursor | `(consumer_id, partition_key) -> last_seq`，表示 consumer 处理到哪个事件。 |
| dispatcher | 按订阅/分区拉取未处理事件并交给 consumer 的平台后台组件。 |

不变量：

1. 每个项目恰好有一个 Root Task；Root Task 覆盖项目全生命周期。
2. 每个 Task 恰好绑定一个 workflow id；创建时固定。
3. 只有该 Task 自己的 workflow 脚本能修改它的 `state`、`stage`、assignee 和 `work`；没有例外。
4. 任何 workflow 脚本都不能修改另一个 Task 的 `state`、`stage`、assignee 或 `work`，包括父子关系。父流程只能创建子 Task、观察子 Task 终止事件。
5. 父流程只能创建子 Task、观察子 Task 终止事件；父 Task 的完成时机由父流程定义，系统不自动聚合子 Task 结果。父 Task 的 `Complete` 在存在未完成直接子 Task 时被拒绝，没有终止级联。
6. 只有 Task 是流程参与者。Requirement、附件、评论、迭代计划、设计文档都不能直接驱动流程。
7. 输入信号是纯信号，不携带业务内容；内容写入附件或 Requirement。
8. 同一 Task 可以同时有多个 assignee；当前 assignee 集合是 `active = true` 的成员行，不存在单一 `assignee` 字段。每个 assignee 可以有独立的 `work`。
9. PM 只绑定 role → account 成员，不设置 `active`/`work`，不修改 `state`/`stage`。
10. 人类验收点由流程定义、项目配置决定是否必须由人类执行；不存在隐藏的硬编码人类审批。
11. 所有状态、阶段、assignee、`work`、角色成员、输入和配置变更都进入不可变审计日志。
12. 身份只来自登录会话；项目隔离优先于一切。
13. 依赖只能引用同一项目、创建时已经存在的 Task，不能是自身的祖先；依赖在创建时固定，之后不可变。
14. 依赖未全部为 `Completed` 时，Task 只能保持 `Pending`；不能进入 `InProgress` 或 `Completed`。依赖满足不会自动改变 `state`，是否启动由该 Task 的 workflow 决定。
15. `dispatch_state` 是派生调度状态，不是 `TaskState`：`Pending` + 任一依赖未 `Completed` = `Blocked`；`Pending` + 所有依赖 `Completed` = `Ready`；`InProgress` = `Running`；`Completed` = `Closed`。系统不通过修改 `state` 来标记调度状态。
16. 跨 Task 只能追加目标 Task 事件流里的触发事件（`TaskCreated`、`ChildTerminal`、`DispatchRequested`），不能修改目标 Task 的 `state`、`stage`、assignee 或 `work`；没有例外。

### 2.1 命名说明

现有术语里 `actor` 已经表示“执行动作的账户”。本设计新增：

- `role`：流程职责位；
- `actor binding`：账户到 role 的成员分配；
- `active actor`（assignee）：当前被 workflow 选中、可以行动的 actor；`active` 只是存储谓词，assignee 是它的语义名称；
- `work`：只属于某个 assignee 的工作说明。

实现时必须区分“事件 actor（谁发的信号）”“role 成员（谁可以担任该职责）”和“assignee（当前谁被安排工作）”，不要继续复用同一个字段名。

## 3. 核心模型

### 3.1 Task

```text
Task
  id
  project_id
  parent_id?           父 Task；Root Task 为空
  workflow             workflow id
  title
  state                Pending | InProgress | Completed
  stage?               流程定义；可为空
  result?              终止时由流程设置，表示结果标签
  dependencies         创建时固定的前置 Task 列表（关系表，不是 tasks 列）
  created_at / updated_at / completed_at
```

Task 不保存 `description`；需要上下文时通过 `title`、引用或附件传递。任务执行过程中产生的内容写附件或 Requirement。

持久化模型对应 `TaskDAO`（不含 `dependencies`；依赖在 `TaskDependencyDAO`）；视图 `Task` 还包含 `dispatch_state`、`dependencies`、`role_specs`、`actors`、`allowed_inputs`、`comments`、`operations`，见 11.2。

### 3.2 state 与 stage

`state` 是共享的粗粒度生命周期。首期只有三个状态，`Completed` 是唯一终态：

| From | To | 说明 |
|---|---|---|
| Pending | InProgress | 工作开始，通常是 `started` 信号或流程初始化；仅当所有依赖 `Completed` |
| Pending | Completed | 极少数无工作 Task；流程可直接完成；仅当所有依赖 `Completed` |
| InProgress | Completed | 流程定义的完成条件满足 |
| Completed | — | 终止后不可变；重试创建新 Task |

进入 `Completed` 只能通过 `Complete` 效果；它同时设置 `result`、冻结 `stage` 并清空 assignee。`SetState` 只用于 `Pending` / `InProgress`。`Completed` 只表示“已终止”，不表示成功；成功、失败、废弃等结果由 `result` 标签表达。父 Task 只有在所有直接子 Task 都已 `Completed` 时才能 `Complete`；没有强制终止级联，因为首期没有 `Cancelled` 状态。

`stage` 由流程定义，只在非终止状态变化；终止时 stage 冻结。流程可以定义空 stage，此时 UI 不展示阶段。

依赖门禁优先于共享 state 转换：依赖未全部 `Completed` 时，`Pending → InProgress` 和 `Pending → Completed` 都被拒绝，Task 只能保持 `Pending`。门禁只检查依赖的 `state == Completed`，不检查依赖的 `result`；依赖的 `result` 由依赖方 workflow 在被调度器唤醒后读取 `DependencyState` 自行判断。依赖全部完成后门禁解除，但系统不会自动把 Task 置为 `InProgress`；是否启动、何时启动仍由该 Task 的 workflow 决定。

#### 调度状态

`Pending` 同时表示“依赖未满足”和“依赖已满足但尚未开始”，仅看 `state` 无法知道哪些 Task 需要调度。为此在视图上增加 `TaskDispatchState`；它由 `state` 和依赖派生，是只读调度状态，不是 `TaskState`，不是数据库列，也不由 workflow 设置。

不把 `Blocked` 加进 `TaskState`：`TaskState` 由 workflow 控制；依赖完成时要由系统把 `Blocked` 改成 `Pending` 就会破坏“只有 workflow 能改 state”，让 workflow 改又会使系统无法可靠知道哪些 Task 已 ready。派生状态让系统能查询调度池，同时不引入系统写 `state` 的旁路。

| dispatch_state | 条件 |
|---|---|
| `Blocked` | `state = Pending` 且至少一个依赖不是 `Completed` |
| `Ready` | `state = Pending` 且所有依赖 `Completed`（包括没有依赖） |
| `Running` | `state = InProgress` |
| `Closed` | `state = Completed` |

规则：

1. `Blocked → Ready` 是依赖完成后的派生结果，不需要系统修改 `state`，也不需要专门的状态变更事件；每次读取 `dispatch_state` 时按当前依赖状态重新计算。调度器发现 `Ready` 后另行追加 `DispatchRequested` 唤醒 workflow。
2. `Ready` 是调度候选，不表示 workflow 一定会立即启动。workflow 仍按自己的启动条件把 `state` 改为 `InProgress`，此时调度状态变为 `Running`。
3. `Blocked` 不能进入 `InProgress`/`Completed`；`Ready` 可以。
4. 依赖不可变，且依赖到达终止时只会是 `Completed`，因此不存在 `Ready → Blocked` 回退。
5. 不新增 `Scheduled` 状态。系统调度器查询 `Ready`，向选中的 Task 的事件流追加 `DispatchRequested` 唤醒 workflow，并负责选择 Task 或补齐执行资源；真正开始仍由 workflow 决定。`Ready` 只表示依赖门禁已解除且 `state` 仍为 `Pending`，它不判断是否已有 active actor，也不提供认领/租约语义；以后如果需要“已认领但未开始”的抢占/租约，应另建 claim/lease，不要把调度细节塞进 `TaskState` 或 `dispatch_state`。

`StageKey` 是 workflow 内唯一的阶段标识，不是全局枚举，也不是 `TaskState`。它建议用 newtype 包一层字符串（如 `pub struct StageKey(String)`），持久化为 `tasks.stage` 的 text 列；合法值由 workflow 的 `stages()` 声明，运行时校验。

`result` 是流程定义的终止标签，不是共享 state 的一部分。例如：

- 普通完成：`completed`
- 验收通过：`approved`
- 验收拒绝：`rejected`
- 失败或不可行：`failed`、`infeasible`
- 废弃：`obsolete`
- 流程自定义：`needs_more_discovery`、`blocked` 等

父流程通过子 Task 的 `result` 分支，而不是读取子 Task 的附件内容。

### 3.3 role、actor 与 assignee

流程为每个 workflow 声明角色：

```text
RoleSpec
  name          如 discoverer / acceptor / planner
  kind          human | agent | any
  min_actors    1 = 至少一个；0 = 可选角色
  max_actors    None = 不限；1 = 单 actor
```

Task 用 `task_actors` 表记录角色成员和当前 assignee：

```text
TaskActorDAO
  task_id
  role_key
  account_id
  active        是否是当前 assignee（由 workflow 选择）
  work?         只对该 assignee 展示的工作说明
```

规则：

1. 一行表示一个账户在某个 role 上的成员关系；同一 Task 同一 role 可以有多行。
2. PM 只添加角色成员，插入时 `active = false`；PM 不删除成员，也不设置 `active` / `work`。首期没有移除角色成员的路径。
3. 流程脚本通过 `SetActiveActors` 设置当前 assignee 集合。它选择的是具体账户，不是 role：每一项指定某个已绑定到某 role 的 account，并可为该 assignee 设置不同的 `work`。
4. `active = true` 的行就是 assignee 集合，不是另一张表或另一个字段；Task 没有单一 `assignee` 字段。
5. assignee 的 role 必须是 workflow 声明的 role，且该账户已由 PM 绑定；否则拒绝执行。
6. 每个 assignee 只能看到自己的 `work`；其他 actor 只能看到 assignee 和角色，不能看到别人的 `work`（PM/Owner 例外）。
7. 流程脚本在 stage 变化或相关事件时可以重新计算 assignee 集合；允许清空。
8. 角色成员添加产生 `ActorBound`；assignee 变化产生 `ActorActivated` / `ActorDeactivated` 事件。
9. 角色成员和 assignee 变化都不是输入信号，不直接改变 `state`/`stage`；流程脚本可以观察它们，但推进仍由脚本决定。
10. 同一账户可以持有多个 role，除非项目策略禁止；available actions 取并集后按策略过滤。
11. 多个 assignee 可以并发提交 action；服务端按 Task 串行处理。是否等待全部 actor 完成，由 workflow 决定；首期不增加 per-actor 完成状态字段。

### 3.4 输入信号

输入信号是纯信号：

```text
TaskInput
  task_id
  signal          started / completed / approved / rejected / ...
  actor_id        从会话派生
  idempotency_key
```

规则：

- 信号不携带业务内容；原因、证据、文档、计划都放在附件或评论里。
- 流程定义每个 stage 允许哪些信号，以及哪个 role 可以发送。
- 项目配置/策略进一步决定：是否需要人类、角色成员必须满足的账户/项目角色条件、是否需要额外验收。
- 未授权的输入会被拒绝并审计，不产生状态变化。
- 输入处理必须幂等；同一 `idempotency_key` 不能重复产生效果。
- 输入不携带 `expected_version`；并发安全由服务端按 Task 串行化保证。
- 输入只能通过流程脚本产生效果；actor 不能直接修改 `state`/`stage`/active actors/`work`。

### 3.5 子 Task

父流程创建子 Task 时可以指定：

- `workflow`（workflow id）
- `title`
- 可选的上下文引用（例如父 Task id、目标 id）
- `dependencies`：同一项目内已存在的 Task id 列表

不能指定：

- 子 Task 的初始 `state` / `stage`
- 子 Task 的 active actors / `work`
- 子 Task 的 role 成员

依赖规则：

1. 依赖必须属于同一项目、创建时已经存在；不能是自身的祖先 Task。祖先依赖会和“父流程等待子 Task 终止”形成死锁，因此创建时拒绝。
2. 依赖只在创建时写入，创建后不可变。没有修改依赖的 API 或 workflow 效果；需要改变依赖时，创建新 Task；首期没有取消，旧 Task 会保留在系统中。
3. 因为新 Task 只能依赖创建时已经存在的 Task，已有 Task 不可能反向依赖这个新 Task，依赖图不会成环，不需要运行时环检测。
4. `CreateChild` 由运行时分配并返回新的 `TaskId`；同一批子 Task 按拓扑顺序创建，后创建的子 Task 可以把先创建的子 Task id 放进 `dependencies`。这个 id 只作为不透明引用，不参与流程分支。
5. 依赖完成不会向依赖方发送跨 Task 事件，也不会自动启动依赖方。依赖方的 `dispatch_state` 在下一次读取时由 `state` 和依赖状态重新计算；变为 `Ready` 后由调度器发现并唤醒其 workflow。依赖方 workflow 读取当前 `DependencyState` 后自行决定是否启动、完成或升级。依赖永远不会完成时，依赖方保持 `Blocked`，由 PM 的等待池处理；首期没有取消，无法强制结束这个依赖方。

采用关系表而不是 `tasks.dependencies` JSON 列，是因为门禁校验和 `dispatch_state` 查询需要按 `task_id` 做 `NOT EXISTS` 连接，关系表能用主键和唯一约束保证一致性；采用创建时固定而不是可变，是为了在 Task 启动前就确定门禁，避免运行中修改依赖绕过门禁，也让新 Task 只能指向已存在的 Task，从结构上排除环。依赖满足不自动启动，是为了不绕过 workflow 对启动条件的控制。

子 Task 创建后由自己的流程初始化，PM 再加入角色成员。父流程只收到子 Task 的终止事件：

```text
ChildTerminal
  child_id
  child_workflow
  result?          approved / rejected / completed / failed / obsolete / ...
```

父流程不能读子 Task 的中间状态，也不能修改子 Task。父 Task 何时完成由父流程根据这些终止事件决定；系统不会因所有子 Task 完成而自动完成父 Task，但运行时要求 `Complete` 时所有直接子 Task 已 `Completed`。

子 Task 终止时，子 Task 的事务在写入自身终止事件的同时，向父 Task 的事件流追加一条 `ChildTerminal`（`aggregate_id = parent_id`）；它不锁父 Task、不修改父 Task 状态。父 Task 的 `task-workflow` consumer 由平台 dispatcher 调度，在父 Task 行锁内处理该事件。子 Task 创建时同理：父事务插入子 Task 并向子 Task 事件流追加 `TaskCreated`；子 workflow 由子 Task 的 consumer 执行，不在父事务内递归运行。

子 Task 创建与父流程事件在同一个事务中写入；子 Task 从 `Pending` 开始。

### 3.6 附件、Requirement 与流程

- **附件**：迭代计划、设计文档、报告、证据等。可见、可评论、可版本化，但不是流程输入。流程脚本不读取附件内容。
- **Requirement**：需求池中的粗略能力描述。它不是 Task，没有 `state`/`stage`/active actor，也不参与流程。人和智能体可以读取、创建、细化；流程脚本不读取 Requirement 内容。
- 流程只根据 Task 自身的事件和纯信号推进。任何需要“看内容才能决定”的逻辑，必须由智能体/人类执行，再通过信号通知流程。

## 4. 流程运行时

### 4.1 流程定义

首期流程定义是受信任的 Rust 代码，按 workflow id 注册。Task 创建时固定 `workflow`；升级使用新的 workflow id，不影响已存在的 Task。

```rust
pub trait TaskWorkflow: Send + Sync {
    fn id(&self) -> WorkflowId;
    fn roles(&self) -> &'static [RoleSpec];
    fn stages(&self) -> &'static [StageSpec];
    fn initial_stage(&self) -> StageKey;
    fn inputs(&self, stage: &StageKey) -> &'static [InputSpec];
    fn on_event(
        &self,
        ctx: &mut WorkflowContext<'_>,
        event: TaskEvent,
    ) -> Result<WorkflowOutcome, WorkflowError>;
}
```

`WorkflowContext` 只暴露：

- 当前 Task 的快照；
- 当前依赖及其 `state`/`result`（只读）；
- 当前角色成员和 active actors；
- 项目配置（只读）；
- 子 Task 创建接口（分配并返回新 `TaskId`，可指定依赖）；
- 效果缓冲区。

它不提供修改其他 Task 的接口。

`WorkflowContext` 暴露的 Task 快照是持久化模型（`TaskDAO` + `TaskActorDAO`），不是面向 viewer 的 `Task` 视图。依赖快照只包含流程判断需要的字段，不包含标题等展示数据。

```rust
pub struct DependencyState {
    pub task_id: TaskId,
    pub state: TaskState,
    pub result: Option<TaskResult>,
}

pub enum Effect {
    SetState(TaskState),          // 只允许 Pending / InProgress
    SetStage(StageKey),
    SetActiveActors(Vec<ActiveActor>),
    Complete {                    // 必须是最后一个修改性效果
        result: Option<TaskResult>,
    },
    CreateChild {
        id: TaskId,
        workflow: WorkflowId,
        title: String,
        dependencies: Vec<TaskId>,
    },
    EmitEvent(DomainEvent),
}

pub struct ActiveActor {
    pub role: RoleKey,
    pub account_id: AccountId,
    pub work: Option<String>,
}
```

`SetActiveActors` 是当前 assignee 集合的全量替换：列表中的行设为 `active = true` 并写入 `work`，列表外的行设为 `active = false`。选择单位是账户；`role` 是该账户被选中时占用的角色，运行时必须校验该账户已经绑定到对应 role。

`CreateChild` 的 `id` 由 `WorkflowContext` 在调用创建接口时分配；接口返回该 id，使同一处理中的后续 `CreateChild` 可以用它作为依赖。id 是运行时分配的不透明值，脚本不能用它做业务分支。`CreateChild` 效果按列表顺序应用；`dependencies` 只能引用已经存在的 Task，或同一列表中更早创建的子 Task，不能引用同一列表中尚未创建的子 Task。

`SetState` 只接受 `Pending` / `InProgress`；从 `Pending` 进入 `InProgress` 时，运行时必须重新校验所有依赖是否 `Completed`。`Complete` 是唯一的终止效果，可选携带 `result`；它是效果列表中最后一个修改 Task 的效果，之后只允许 `EmitEvent`。运行时应用 `Complete` 时原子完成：校验依赖门禁（依赖必须全部 `Completed`）、校验所有直接子 Task 都已 `Completed`、设置 `state = Completed` 和 `result`、写入 `completed_at`、冻结 `stage`、清空 assignee，并写入终止事件和父 Task 的 `ChildTerminal`。`Complete` 之后出现 `SetState`、`SetStage`、`SetActiveActors`、`CreateChild` 或 `EmitEvent` 以外的效果一律拒绝，不能依赖脚本自觉排序。

`Complete` 不强制终止任何子 Task。父 Task 只有在所有直接子 Task 都已 `Completed` 时才能完成；每个子 Task 完成时也满足同样的条件，因此完成父 Task 时整棵子树都已终止。如果某个子 Task 永远不完成，父 Task 就会一直等待；首期没有取消，PM 只能升级。没有任何跨 Task 的终止级联。

首期不做用户自定义 DSL；如果后续需要，可在同一运行时上增加声明式解释器，但不得绕过上面的效果边界。

### 4.2 事件

Task workflow 是平台事件总线的 `task-workflow` consumer，按 `task_id` 分区、事务型投递（见 [RFC 0004](0004-durable-event-bus.md)）。它处理的事件：

| 事件 | 类别 | 来源 | 用途 |
|---|---|---|---|
| `TaskCreated` | 触发 | 项目创建/父流程 | 初始化 stage、创建首批子 Task |
| `ActorBound` | 触发 | PM | 添加 role 成员 |
| `InputReceived` | 触发 | 人类/智能体 | 处理纯信号 |
| `ChildTerminal` | 触发 | 子 Task 事务 | 父流程分支 |
| `DispatchRequested` | 触发 | 调度器 | 唤醒 `Ready` Task 的 workflow；payload 不含业务内容，workflow 读取当前依赖快照 |
| `ActorActivated` / `ActorDeactivated` | 审计 | 流程脚本 | 更新 active actors 和 `work`，不再触发 workflow |
| `ConfigChanged` | 审计 | 项目配置 | 只影响后续验收点，不回溯已进入的验收；首期不唤醒 workflow |

所有已接受的输入和效果（包括 `state`/`stage`/`result` 变化）都写入 `events`；`Task.operations` 是这些事件的只读视图。触发事件和审计事件都进入该视图；consumer 游标不进入。

依赖完成不会产生跨 Task 事件。依赖方是否可调度完全由 `state` 和 `task_dependencies` 派生：依赖方下一次被读取时，`dispatch_state` 自动从 `Blocked` 变为 `Ready`。依赖的 `result` 不需要通过事件传递；依赖方 workflow 被唤醒后直接读取当前 `DependencyState` 快照。

调度器周期性查询 `dispatch_state = Ready` 的 Task，向它们的事件流追加 `DispatchRequested`。这个事件只唤醒 workflow，不携带依赖结果；workflow 在锁内读取当前依赖快照，决定是否启动、完成或升级。调度器不修改 Task 的 `state`，也不设置 active actors；事件由平台 dispatcher 投递。

`DispatchRequested` 可以重复提交：workflow 必须按当前快照幂等处理；如果 Task 已经 `InProgress` 或终止，事件不产生状态效果。调度器按 `task_id` 记录本进程已唤醒的 Task，避免每个轮询周期重复提交；重启后重复唤醒一次是允许的。若 workflow 选择继续等待，后续需要 PM 或其他事件再次唤醒；首期不引入 timer。

依赖永远不会完成时，依赖方永远不是 `Ready`，调度器不会选中它。PM 的等待池展示 `Blocked` Task 及其依赖状态，负责升级；首期没有取消，PM 不能强制结束依赖方，只能人工介入或创建替代 Task。

`TaskCreated` 不是调度触发点；创建时依赖已满足的 Task 在第一次读取时就是 `Ready`。`dispatch_state` 是读模型，不写字段，也不新增 `TaskReady` 事件。

同 Task 的命令（输入、角色绑定）调用 `process_partition(task-workflow, task_id)`，把校验授权、检查幂等和追加输入事件放进 command 闭包，在游标锁内同步执行；跨 Task 触发（`TaskCreated`、`ChildTerminal`、`DispatchRequested`）只追加到目标 Task 的事件流，由目标 partition 的 dispatcher 异步处理。无论哪种方式，效果、事件和 consumer 游标都在同一个事务中落库。

### 4.3 原子性、幂等和并发

Task 的并发安全由服务端的命令处理保证，不依赖 Task 上的版本字段。

1. Task workflow 是 `task-workflow` consumer，按 `task_id` 分区、事务型投递；同一 partition 内串行处理，跨 partition 可以并行。
2. 命令路径不自己开事务，也不先锁 Task：它把“校验授权、检查幂等、追加输入事件”作为 command 传给 `process_partition(task-workflow, task_id)`。设施先锁 consumer 游标，再在 command 内锁 Task 行；命令路径不得在调用前锁 Task，否则会和 dispatcher 反向加锁。
3. 在 Task 行锁内加载 Task 快照、依赖快照、角色成员、active actors 和配置，验证信号、授权和当前 state/stage。
4. 调用流程脚本，得到效果列表。
5. 在同一个数据库事务中：
   - 更新 Task 的 `state`/`stage`/`result`；
   - 更新 `task_actors` 的 `active`/`work`；
   - 创建子 Task 和 `task_dependencies`（如有）；依赖必须与子 Task 在同一事务写入；
   - 写 `task_inputs` 记录；
   - 写领域事件；已接受输入、角色成员添加和每个效果都生成事件，作为 `Task.operations` 的来源；
   - 推进 `task-workflow` consumer 游标。
6. 同一个 `idempotency_key` 重复提交返回第一次的结果，不重复产生效果；由 `(task_id, idempotency_key)` 唯一约束兜底。
7. 流程脚本必须确定性：相同 Task 快照、依赖快照、角色成员、active actors、配置版本和事件，产生相同效果；脚本不能直接调用外部系统。
8. 不使用 `expected_version`。如果未来需要防止陈旧 UI 提交，使用语义化的 `expected_stage` 或显式的 revision/ETag，而不是在 Task 上保留通用版本字段。
9. 跨 Task 的约束（子 Task 创建、根 Task、角色成员）由唯一约束和事务保证；外部副作用不在本流程内同步执行。
10. 依赖写入只发生在 Task 创建事务中：校验同一项目、已存在、不是自身祖先，然后插入 `task_dependencies`。创建后没有修改依赖的路径。
11. 依赖门禁在应用效果时校验，不依赖脚本自觉：`SetState(Pending → InProgress)` 和 `Complete` 前重新读取依赖状态；未全部 `Completed` 则拒绝该效果。
12. 子 Task 终止时，子 Task 事务向父 Task 的事件流追加 `ChildTerminal`（`aggregate_id = parent_id`），不锁父 Task、不修改父状态；父 workflow 由父 partition 的 dispatcher 在父 Task 行锁内执行。
13. 父 Task 不能取消子 Task；首期没有取消请求，也没有跨 Task 的强制终止。
14. 依赖完成不跨 Task 写事件；依赖方的 `dispatch_state` 由 `state` 和依赖派生，读取时重新计算。
15. 调度器查询 `Ready` 后向依赖方的事件流追加 `DispatchRequested`；该事件只唤醒 workflow，workflow 在锁内读取依赖快照并决定是否启动。调度器不修改 `state`/`stage`/active actors。
16. `DispatchRequested` 可以重复提交；workflow 必须幂等，已启动或已终止的 Task 忽略该事件。调度器可以在内存中去抖，重启后重复唤醒是允许的。
17. 依赖永远不会完成时，依赖方保持 `Blocked`，不会变成 `Ready`；由 PM 的等待池发现并升级，首期没有取消或强制结束路径。
18. 创建 Task 时仍校验依赖存在、同项目、非祖先；依赖不可变。因为不发送依赖终止通知，创建与依赖终止并发不需要额外的跨 Task 锁；依赖状态在门禁校验和 `dispatch_state` 读取时重新读取。
19. consumer 游标只推进到本次事务实际读取的触发事件，不能取 partition 的 `max(seq)`；审计事件不进入订阅，不影响游标。
20. 效果按列表顺序应用；`Complete` 必须是最后一个修改性效果。运行时在 `Complete` 处原子完成 state/result 设置、stage 冻结、assignee 清空和终止事件写入；其后的修改性效果一律拒绝。
21. 父 Task 的 `Complete` 在应用前校验所有直接子 Task 都已 `Completed`；没有终止级联，子 Task 未完成时父 Task 只能继续等待。

### 4.4 assignee 切换

- stage 声明 action 和每个 action 需要的 role；流程脚本在 stage 变化或相关事件时调用 `SetActiveActors`，为 role 选择具体的 assignee。
- 脚本可以同时选择多个 assignee，并给每个 assignee 设置不同的 `work`。
- 若某个 role 没有已绑定账户，脚本无法为该 role 选择 assignee；Task 等待，PM 负责补充角色成员或升级。
- `ActorBound` 变化时，脚本可以重新计算 assignee 集合，但最终由脚本决定。
- 终止时 assignee 集合清空；历史保留最后一次 assignee 和 `work`。
- assignee 被 workflow 停用后，后续输入按当前 assignee 集合拒绝；已接受的历史输入不撤销。
- `work` 只对该 assignee 展示；其他 actor 只看到 assignee 和角色。

### 4.5 授权

流程定义声明：

- 每个 stage 允许的 signal；
- 允许发送该 signal 的 role；
- 该 role 的 `kind` 约束（human / agent / any）。

项目配置/策略再决定：

- 某个验收点是否必须由人类执行；
- 哪些账户或项目角色可以填充某个 role；
- 是否要求 worker 与 acceptor 不同；
- 是否需要额外证据或审批。

建议的授权判断维度（最终粒度待定）：

```text
project_id
workflow
task_id
stage
signal
actor_id
actor.kind
actor.project_role
active_actor
role_key
project_config_version
```

流程脚本只处理已经通过授权的输入；未通过授权的输入不得进入脚本。

### 4.6 不允许跨 Task 变更

`WorkflowContext` 不提供 `update_task(other_id, ...)`。父流程只能：

- `CreateChild`：在子 Task 事件流追加 `TaskCreated`，不运行子 workflow；
- 观察 `ChildTerminal`（父 Task 事件流里的触发事件）。

父流程不能取消子 Task，也不能直接修改子 Task 的 `state`/`stage`。写入方只追加事件，不锁目标 Task、不修改目标状态；目标 workflow 由目标 partition 的 dispatcher 在目标 Task 行锁内执行。

父流程的 `Complete` 不会终止任何子 Task：运行时在应用 `Complete` 前校验所有直接子 Task 都已 `Completed`，否则拒绝。没有终止级联，也没有任何跨 Task 的例外。

依赖 Task 终止不会产生跨 Task 事件，也不会修改依赖方。依赖方通过 `dispatch_state = Ready` 被调度器发现，调度器向依赖方事件流追加 `DispatchRequested` 唤醒其 workflow；workflow 读取依赖快照后只能修改自己的 `state`/`stage`/active actors/`work`，不能反向修改依赖 Task。依赖永远不会完成时，依赖方保持 `Blocked`，由 PM 的等待池处理。

## 5. 验收

### 5.1 简单验收：Task 自身覆盖

适用于需求发现、需求分析、架构设计、Roadmap、迭代、发布等“一次 approve/reject”的验收。

流程：

```text
work stage
  ├─ worker 发 completed
  ▼
awaiting_acceptance (active actors = selected acceptor role members)
  ├─ acceptor 发 approved → state=Completed, result=approved
  └─ acceptor 发 rejected → stage=work stage, active actors=worker actor(s), state=InProgress
```

特点：

- 不创建验收子 Task。
- 验收者是当前 active acceptor actor，通过纯信号操作。
- 拒绝不终止父 Task，只退回原 stage；同一 worker 修改后再次 `completed`，再次进入验收。
- 验收原因、修改说明、证据放在附件/评论里，不进入信号。
- Task 的 `state` 在整个验收期间保持 `InProgress`；`stage` 表达“待验收”。

### 5.2 复杂验收：子 Task

只有在验收本身需要多步、多个验收者、独立证据链或独立工作队列时，才由父流程创建验收子 Task。

- 父流程创建子 Task（workflow id 如 `acceptance`）。
- 子 Task 有自己的 role、stage、active actors 和流程。
- 父流程只观察子 Task 的 `ChildTerminal`，根据 `result` 决定完成、退回或创建新 Task。
- 父流程不能修改子 Task 的状态。

是否使用复杂验收由流程定义决定，不是系统自动判断。

### 5.3 人类验收配置

- 流程定义验收点（例如 `requirements_discovery.awaiting_acceptance`）。
- 项目配置决定该验收点是否必须由人类执行：
  - `human_required = true`：`acceptor` 角色成员必须是满足条件的人类账户；
  - `human_required = false`：角色成员可以是人类或智能体；
  - 如果项目完全不想要验收，选择不带验收 stage 的 workflow。
- 人类验收点可配置，不是硬编码。
- 配置版本记录在验收决策中；已进入的验收按进入时的配置版本执行，后续配置变更不回溯。

### 5.4 终止结果与父流程

子 Task 的终止事件携带 `result`。父流程可以据此分支：

- `approved` → 进入下一阶段/创建下一个子 Task；
- `rejected` → 退回、重试或创建修订 Task；
- `failed` / `infeasible` / `obsolete` → 进入等待决策 stage、升级或创建替代 Task；
- 自定义结果（如 `needs_more_discovery`）→ 父流程定义。

系统不会因所有子 Task 完成就自动完成父 Task；但父流程返回 `Complete` 时，所有直接子 Task 必须已 `Completed`。

## 6. Requirement 与需求池

### 6.1 Requirement 的定义

Requirement 是**粗略能力描述**，回答“系统需要提供什么能力”，不是需求分析结果，也不是 Task。

字段：

| 字段 | 说明 |
|---|---|
| `id` | UUID |
| `project_id` | 项目隔离 |
| `title` | 能力名称 |
| `capability` | 能力描述，例如“客服可以按金额部分退款” |
| `beneficiary` | 受益者/使用者 |
| `source` | 来源：项目目标、人类干系人、反馈、监控、法规等 |
| `source_ref` | 来源引用 |
| `non_goals` | 明确不包含什么 |
| `open_questions` | 尚未确认的问题 |
| `maturity` | `raw` / `analyzed` / `superseded`；用于池的展示，不驱动流程 |
| `version` | 版本 |
| `created_by` / `created_at` / `updated_at` | 审计 |

Requirement 没有 `state`、`stage`、active actor，不绑定流程。它可以被 Task 引用，也可以链接附件；但它不是流程参与者。

### 6.2 需求发现

需求发现是一个独立 Task（workflow id `requirements_discovery`），由 Root Task 在目标澄清通过后创建。

角色：

- `discoverer`：产品/需求智能体；
- `acceptor`：人类或智能体，由项目配置决定。

做法：

1. **目标分解**：从项目目标、成功指标、非目标、约束出发，拆出系统必须提供的能力。
2. **干系人获取**：向 owner、最终用户、支持、合规等干系人提问；记录原始诉求、来源和冲突。智能体可以提问，但不能替人编造需求。
3. **证据补全**：从现有反馈、事故、监控、竞品、法规中补充；每条需求必须有来源，或明确标记为假设。
4. **汇总入池**：把候选能力写成 `Requirement`，录入需求池；只写能力、受益者、来源、边界、待确认问题，不写实现方案和验收标准。
5. **验收**：`discoverer` 发 `completed`；`acceptor` 查看需求池后发 `approved` 或 `rejected`。拒绝则退回发现 stage，由原智能体修改。
6. **完成**：Task 完成时，Root 只收到 `completed` 信号，不读取需求池内容。

需求发现不回答“怎么做”和“做到什么程度”；这些留给需求分析和设计。

### 6.3 需求分析

需求分析是另一个 Task（workflow id `requirements_analysis`），由 Root 在需求发现通过后创建。

- 角色：`analyst`、`acceptor`。
- 输入：需求池中的 raw Requirements（由执行智能体自己读取）。
- 输出：补全后的 `Requirement` 版本（`maturity=analyzed`），包括问题、用户、验收场景、非目标、约束、优先级、风险；必要时创建新的 Requirement 或拆分。
- 验收：同样由 Task 自身覆盖；拒绝回到分析 stage。
- 流程脚本不读取 Requirement 内容；Root 只根据 Task 终止事件推进。

### 6.4 需求池规则

- 需求池按项目隔离。
- 人类可以随时添加 Requirement；智能体也可以，但必须记录来源。
- Requirement 可以被 Task/附件引用，但不是流程输入。
- `maturity` 只是展示/过滤字段，不驱动任何 Task 状态。
- 需求发现和需求分析可以修改/新增 Requirement；流程层面只看到 Task 的完成信号。

## 7. 默认项目流程

### 7.1 Root Task

创建项目时，在同一个事务中创建 Root Task：

- `workflow = project`
- `state = InProgress`
- `stage = goal`
- `parent_id = null`
- 无 active actor

Root Task 没有角色、没有 active actor，也不接受外部输入。它不做具体工作，只创建阶段子 Task，并根据子 Task 的终止事件推进。由于没有输入，Root 不引入额外的授权规则；其他 Task 的输入仍由各自的 workflow 角色和 active actor 授权。

### 7.2 默认阶段

| 阶段 | Workflow | 角色 | 产物 | 验收 |
|---|---|---|---|---|
| 目标澄清 | `goal_clarification` | `clarifier`, `acceptor` | ProjectBrief 附件 | 自身覆盖 |
| 需求发现 | `requirements_discovery` | `discoverer`, `acceptor` | 需求池 raw Requirements | 自身覆盖 |
| 需求分析 | `requirements_analysis` | `analyst`, `acceptor` | analyzed Requirements | 自身覆盖 |
| 架构设计 | `architecture_design` | `architect`, `reviewer`, `acceptor` | 设计文档/ADR 附件 | 自身覆盖 |
| Roadmap | `roadmap` | `planner`, `acceptor` | Roadmap 附件 | 自身覆盖 |
| 迭代 | `iteration` | `planner`, `implementer`, `reviewer`, `acceptor` | 迭代计划附件、实现结果 | 自身覆盖 |
| 发布 | `release` | `release_manager`, `verifier`, `approver`, `deployer`, `observer` | Release/证据 | 自身覆盖 |
| 项目复盘 | `project_review` | `reviewer`, `acceptor` | 复盘附件 | 自身覆盖 |

默认流程是模板，不是系统硬编码；项目可以选择不同 workflow。发布和项目复盘保留为 workflow 模板，首期没有触发入口，不进入 Root 默认流程。

### 7.3 Root 流程伪代码

```text
on TaskCreated:
  set_state(InProgress)
  set_stage("goal")
  create_child(goal_clarification, "澄清项目目标")

on ChildTerminal(child_workflow=goal_clarification, result=approved):
  set_stage("requirements_discovery")
  create_child(requirements_discovery, "发现初始需求")

on ChildTerminal(child_workflow=requirements_discovery, result=approved):
  set_stage("requirements_analysis")
  create_child(requirements_analysis, "分析需求池中的需求")

on ChildTerminal(child_workflow=requirements_analysis, result=approved):
  set_stage("architecture")
  create_child(architecture_design, "形成架构方案和关键 ADR")

on ChildTerminal(child_workflow=architecture_design, result=approved):
  set_stage("roadmap")
  create_child(roadmap, "根据需求和架构形成 Roadmap")

on ChildTerminal(child_workflow=roadmap, result=approved):
  set_stage("iteration")
  create_iteration(next_iteration_number())

on ChildTerminal(child_workflow=iteration, result=approved):
  create_iteration(next_iteration_number())

on ChildTerminal(child_workflow=*, result=failed):
  # 父流程自行决定：创建替代 Task 或进入 attention_required
```

失败/拒绝分支由流程定义，例如：

- `needs_more_discovery`：Root 创建新的需求发现 Task；
- 关键子 Task `failed`：Root 进入 `attention_required` 并升级；首期没有 `retry` 或 `cancel` 输入，不自动恢复；
- 非关键子 Task 失败：记录并继续，由流程判断。

Root 不读取附件、Requirement、迭代计划或设计文档的内容。

### 7.4 迭代

默认迭代是**叶子 Task**，不创建阶段子 Task；通过 stage 和角色切换覆盖迭代周期：

- `planning`：`planner` 写迭代计划附件，发 `plan_ready`；
- `implementation`：`implementer` 实现，发 `implementation_complete`；
- `review`：`reviewer` 审查，发 `review_approved` 或 `changes_requested`；
- `acceptance`：`acceptor` 验收，发 `approved` 或 `rejected`；
- 完成：state=Completed，result=approved。

迭代计划是 Iteration Task 的附件，从 Task 可见；它不是流程输入，脚本不读取其内容。迭代做什么由计划描述，流程只规定阶段顺序。

Root 只在上一个迭代终止后创建下一个迭代：

```text
iteration N terminal
  ├─ result=approved → create iteration N+1
  ├─ result=failed → 流程决定重试/停止
  └─ 其他结果 → 流程定义
```

如果项目需要更细的工作项拆分，可以换用“迭代父 Task + 工作项子 Task”的 workflow；默认不这样做。

### 7.5 发布

首期不把发布接入 Root 默认流程，也没有独立触发入口。`release` workflow 模板保留，等出现明确的发布触发需求（例如某个 workflow 在特定事件后创建 Release Task）后再接入。

Release 的默认形态仍是叶子 Task，通过 stage 覆盖候选、验证、批准、部署、观察、回滚；人类批准要求由项目配置决定。以上只是模板，首期没有创建入口，也不占用 Root 的输入。

Release 的输入示例：`started`、`completed`、`approved`、`rejected`、`deployed`、`healthy`、`unhealthy`、`rollback`。

### 7.6 项目完成

首期没有“主动结束项目”的输入，Root 不创建 `project_review`，也不会自动进入 `Completed`；项目在迭代循环中持续运行。`project_review` workflow 模板保留，等出现明确的完成触发需求后再接入。

首期没有项目级暂停/关闭操作；如果以后需要暂停，应作为明确的“暂停”操作单独设计，不使用“归档”或其他含糊名称。

### 7.7 需求发现推演

| 步骤 | 操作/事件 | 脚本动作 | state | stage | active actors |
|---|---|---|---|---|---|
| 0 | Root 创建 D，声明角色 `discoverer`/`acceptor` | 等待 PM 加入角色成员 | Pending | discovery | — |
| 1 | PM 把 A 加入 `discoverer`、H 加入 `acceptor` | 设 A 为 active | Pending | discovery | A |
| 2 | A 发 `started` | — | InProgress | discovery | A |
| 3 | A 写需求到池，发 `completed` | 切验收，H 设为 active | InProgress | awaiting_acceptance | H |
| 4a | H 发 `approved` | 完成，清空 active | Completed | accepted | —（result=approved） |
| 4b | H 发 `rejected` | 退回，A 设为 active | InProgress | discovery | A |
| 5b | A 修改后再次 `completed` | 再切验收 | InProgress | awaiting_acceptance | H |

这个推演验证了：

- Task 始终是叶子，不创建验收子 Task；
- PM 只添加角色成员，active actors 由脚本设置；
- 拒绝不创建新 Task，只退回 stage；
- 验收者通过纯信号操作；
- 人类验收要求可由配置改变。

### 7.8 迭代推演

| 步骤 | 操作/事件 | 脚本动作 | state | stage | active actors |
|---|---|---|---|---|---|
| 0 | Root 创建 Iteration N，角色 `planner`/`implementer`/`reviewer`/`acceptor` | 等待 PM 加入角色成员 | Pending | planning | — |
| 1 | PM 把 P/I/R/H 加入对应角色 | 设 P 为 active | Pending | planning | P |
| 2 | P 发 `started` | — | InProgress | planning | P |
| 3 | P 写迭代计划附件，发 `plan_ready` | 切实现，I 设为 active | InProgress | implementation | I |
| 4 | I 发 `implementation_complete` | 切评审，R 设为 active | InProgress | review | R |
| 5a | R 发 `review_approved` | 切验收，H 设为 active | InProgress | acceptance | H |
| 5b | R 发 `changes_requested` | 退回，I 设为 active | InProgress | implementation | I |
| 6 | H 发 `approved` | 完成，清空 active | Completed | done | —（result=approved） |
| 6b | H 发 `rejected` | 退回，I 设为 active | InProgress | implementation | I |

这套流程证明 active actor 切换、循环退回、验收和完成条件都在同一个叶子 Task 的流程内完成。

review stage 也可以同时激活多个 reviewer，例如：

```text
SetActiveActors([
  { role: reviewer, account: alice, work: "审查安全性" },
  { role: reviewer, account: bob,   work: "审查性能" },
])
```

每个 reviewer 只看到自己的 `work`；是否等待所有 reviewer 完成，由该 workflow 的 review 逻辑决定。首期不增加 per-actor 完成状态字段。

## 8. PM 智能体

PM 是项目级智能体账户，不是 Task 角色，也不是流程参与者。它通过普通 API/MCP 操作，受项目策略约束。

职责：

1. **添加角色成员**：观察缺少成员的 Task，按 role 的 `kind`、项目成员、可用性、技能和职责分离要求，把账户加入 role；不移除成员，不激活 actor。
2. **补充成员**：账户停用、离开项目、长期无响应时，PM 只能加入替代者，不能移除原成员；由 workflow 决定是否切换 active。必须审计。
3. **池管理**：维护 `dispatch_state = Ready` 的待调度池和 `Blocked` 的等待池，按优先级和依赖协调；不设置 active actor 或 `work`。`dispatch_state` 是派生只读状态，PM 不能修改。等待池中依赖永远不会完成的 Task，PM 负责升级，不直接改 `state`。
4. **验收协调**：确保验收 role 有合法角色成员；人类验收点必须包含人类。PM 不代替人类验收，除非项目明确授权它作为智能体验收者。
5. **升级**：无可用 actor、Task 长期停留、子 Task 失败导致停滞时，通知 owner 或指定队列。
6. **不做什么**：
   - 不修改 `state`/`stage`/active actors/`work`；
   - 不移除角色成员；
   - 不创建/修改流程定义；
   - 不创建子 Task（子 Task 由父流程创建）；
   - 不设置或修改依赖（依赖由创建该 Task 的父流程在创建时指定）；
   - 不修改 `dispatch_state`（它由 `state` 和依赖派生）；
   - 不读取/改写附件内容来决定流程；
   - 不绕过人类验收配置。

PM 的选择算法首期可以简单：按 role kind、项目成员、最近负载、显式配置优先级；后续再加入技能、成本、并发限制。Agent 供给策略不在本设计范围，假定可以按需创建。

## 9. 终止与不可逆性

Task 只有一个终止路径：workflow 返回 `Complete { result }`。

1. `Complete` 只由该 Task 自己的 workflow 产生；没有外部结束操作，也没有 `cancel` 输入。
2. 运行时在应用 `Complete` 前校验：所有依赖都已 `Completed`；所有直接子 Task 都已 `Completed`。
3. `Complete` 原子地设置 `state = Completed`、写入 `result`、记录 `completed_at`、冻结 `stage`、清空 assignee，并向父 Task 的事件流追加 `ChildTerminal`。
4. `Completed` 不可重开，也不可修改。需要重试、替换或放弃时，创建新的 Task；旧 Task 保留在系统中。
5. `result` 只是流程定义的标签，系统不解释其含义。`failed`、`infeasible`、`obsolete` 都通过 `Complete` 进入 `Completed`，由父流程读取 `result` 后决定下一步。
6. 首期没有取消，因此没有停止一个卡住或失控 Task 的手段。PM 只能升级、协调，或创建替代 Task；旧 Task 仍然存在。若将来确实需要停止，必须显式引入取消，而不是复用 `Complete`。
7. Root Task 没有 active actor，也没有输入；首期没有项目级暂停/关闭操作，项目生命周期不在 Task 模型中定义。

## 10. 项目配置与授权

项目配置是版本化文档，至少包含：

```toml
[project]
project_manager_account = "..."

[acceptance.goal_clarification]
human_required = true

[acceptance.requirements_discovery]
human_required = true

[acceptance.requirements_analysis]
human_required = true

[acceptance.iteration]
human_required = true

[role_policy.requirements_discovery.discoverer]
kinds = ["agent"]

[role_policy.requirements_discovery.acceptor]
kinds = ["human"]   # 或 ["human", "agent"]
```

规则：

- 流程定义验收点和角色；项目配置决定人类要求和账户约束。
- 配置有版本；Task 进入验收点时记录配置版本。
- 已进入的验收按进入时版本执行；后续配置变更不回溯。
- 不存在隐藏的“人类必须批准”硬编码。
- PM 的角色成员分配受配置约束；人类验收 role 不能包含智能体。
- 授权判断维度见 4.5；最终粒度待定，但必须覆盖 `workflow`、`stage`、`signal`、`actor`、active actor/role 和配置版本。

## 11. 数据模型与对象模型

### 11.1 数据库模型

| 表 | 用途 |
|---|---|
| `tasks` | 新 Task 聚合快照 |
| `task_dependencies` | Task 创建时固定的依赖关系 |
| `task_actors` | Task 的 role 成员、active actor 和每个 actor 的 `work` |
| `task_comments` | Task 评论，人类/智能体交流与附加信息 |
| `task_inputs` | 输入信号、幂等键、授权结果 |
| `requirements` | 需求池当前版本 |
| `requirement_revisions` | Requirement 版本历史 |
| `documents` / `document_links` | 附件元数据及关联 |
| `project_workflow_configs` | 项目流程/验收配置版本 |
| `events` | 现有事件表，增加 Task/流程关联字段；`Task.operations` 的数据源 |
| `projects` | 增加 `root_task_id`、`active_config_version`、`project_manager_account_id` |

平台事件总线另建 `event_consumer_cursors(consumer_id, partition_key, last_seq)`（[RFC 0004](0004-durable-event-bus.md)），不属于 Task 数据模型；Task 不建自己的游标表或通知表。

`tasks` 关键字段：

```sql
id uuid primary key
project_id uuid not null
parent_id uuid null
workflow text not null
title text not null
state text not null
stage text null
result text null
created_at timestamptz not null
updated_at timestamptz not null
completed_at timestamptz null
```

`task_dependencies`：

```sql
task_id uuid not null references tasks(id)
depends_on_task_id uuid not null references tasks(id)
primary key (task_id, depends_on_task_id)
check (task_id <> depends_on_task_id)
```

`task_actors`：

```sql
task_id uuid not null
role_key text not null
account_id uuid not null
active boolean not null default false
work text null
primary key (task_id, role_key, account_id)
```

`task_comments`：

```sql
id uuid primary key
task_id uuid not null
author_account_id uuid not null
body text not null
created_at timestamptz not null
```

约束：

- `task_actors` 对 `(task_id, role_key, account_id)` 唯一。
- `task_actors` 建索引 `(task_id, active)` 用于读取当前 active actors，建索引 `(account_id, task_id)` 用于“我的 Task”查询。
- `task_dependencies` 对 `(task_id, depends_on_task_id)` 唯一；主键覆盖按 `task_id` 读取依赖和 `dispatch_state` 计算。
- 依赖只能引用同一项目、创建时已存在的 Task，不能是自身祖先；由应用层在创建事务中校验。`task_id <> depends_on_task_id` 由数据库约束兜底。
- 依赖只在 Task 创建时写入，没有更新或删除依赖的 API/effect；因此不需要环检测表或递归查询。要改变依赖，创建新 Task，旧 Task 保留。

调度状态不建列、不建表，由 `tasks.state` 和 `task_dependencies` 派生。待调度查询：

```sql
select t.*
from tasks t
where t.state = 'pending'
  and not exists (
    select 1
    from task_dependencies d
    join tasks dep on dep.id = d.depends_on_task_id
    where d.task_id = t.id
      and dep.state <> 'completed'
  );
```

- 为调度查询建 `tasks(state)` 部分索引（`where state = 'pending'`）；`task_dependencies` 主键覆盖按 `task_id` 查依赖。
- `dispatch_state` 不写入 `tasks`，不进入 `events`，也不新建调度队列表。调度器查询 `Ready`；需要认领/租约时另建 claim/lease，不复用 `state`。

- `task_comments` 建索引 `(task_id, created_at, id)`，用于按时间读取评论。
- 评论是追加式内容；首期不更新、不删除。`body` 非空由应用层校验，author 从会话派生。
- `task_inputs` 对 `(task_id, idempotency_key)` 唯一。
- Task 操作历史不新建表；从 `events` 按 `aggregate_id = task_id` 读取，按 `seq` 倒序。已接受的输入和每个流程效果必须与状态变更、consumer 游标推进在同一事务写入。
- 依赖完成不写跨 Task 事件；依赖方是否可调度由 `tasks.state` 和 `task_dependencies` 派生，读取时计算。
- 调度器查询 `dispatch_state = ready` 后，向 Task 的事件流追加 `DispatchRequested`；该事件只唤醒 workflow，不修改 `state`。`DispatchRequested` 可以重复提交，workflow 必须幂等。
- 依赖永远不会完成时依赖方保持 `Blocked`；PM 的等待池查询 `Blocked` Task 及其依赖状态，发现长期不满足的依赖时升级。
- 依赖门禁由命令处理器在应用 `SetState(Pending → InProgress)` 和 `Complete` 时校验，不依赖 workflow 自觉；未全部 `Completed` 时 Task 只能保持 `Pending`。
- 所有 Task 状态、角色成员和 active actor 修改都走 `task-workflow` consumer；`process_partition` 先锁 consumer 游标，再锁 Task 行。不使用版本字段；评论是独立追加，不在此列。
- Task 终止后 `state`/`stage`/`result` 不可变；终止时 `task_actors.active` 全部置为 `false`。
- 父 Task 的 `Complete` 在同一个 Task 行锁内校验所有直接子 Task 都为 `Completed`；没有终止级联，也没有跨 Task 的批量锁。
- 附件内容按 ADR 0003 存 Git/对象存储，Postgres 存 URI、hash、类型、版本。
- 流程定义首期在代码注册表中，不建数据库表；Task 只存 `workflow`（workflow id）。

### 11.2 对象模型

持久化对象统一使用 `DAO` 后缀，只映射数据库列；视图对象使用领域名称，可以组合 DAO 数据、workflow 定义和 actor 相关派生数据，但不写回数据库。仓库的读写接口只使用 DAO；应用层对外返回视图对象。

持久化对象：

```rust
pub struct TaskDAO {
    pub id: TaskId,
    pub project_id: ProjectId,
    pub parent_id: Option<TaskId>,
    pub workflow: WorkflowId,
    pub title: String,
    pub state: TaskState,
    pub stage: Option<StageKey>,
    pub result: Option<TaskResult>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub completed_at: Option<Timestamp>,
}

pub struct TaskDependencyDAO {
    pub task_id: TaskId,
    pub depends_on_task_id: TaskId,
}

pub struct TaskActorDAO {
    pub task_id: TaskId,
    pub role_key: RoleKey,
    pub account_id: AccountId,
    pub active: bool,
    pub work: Option<String>,
}

pub struct TaskCommentDAO {
    pub id: CommentId,
    pub task_id: TaskId,
    pub author_account_id: AccountId,
    pub body: String,
    pub created_at: Timestamp,
}
```

视图对象：

```rust
pub enum TaskDispatchState {
    Blocked,
    Ready,
    Running,
    Closed,
}

pub struct Task {
    pub id: TaskId,
    pub project_id: ProjectId,
    pub parent_id: Option<TaskId>,
    pub workflow: WorkflowId,
    pub title: String,
    pub state: TaskState,
    pub dispatch_state: TaskDispatchState,
    pub stage: Option<StageKey>,
    pub result: Option<TaskResult>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub completed_at: Option<Timestamp>,
    pub dependencies: Vec<TaskDependency>,
    pub role_specs: Vec<RoleSpec>,
    pub actors: Vec<TaskActor>,
    pub allowed_inputs: Vec<SignalKey>,
    pub comments: Vec<TaskComment>,
    pub operations: Vec<TaskOperation>,
}

pub struct TaskDependency {
    pub task_id: TaskId,
    pub title: String,
    pub state: TaskState,
    pub result: Option<TaskResult>,
}

pub struct TaskActor {
    pub role_key: RoleKey,
    pub account_id: AccountId,
    pub display_name: String,
    pub active: bool,
    pub work: Option<String>,
}

pub struct TaskComment {
    pub id: CommentId,
    pub author_account_id: AccountId,
    pub author_display_name: String,
    pub author_kind: AccountKind,
    pub body: String,
    pub created_at: Timestamp,
}

pub struct TaskOperation {
    pub id: EventId,
    pub kind: DomainEventKind,
    pub actor: Actor,
    pub occurred_at: Timestamp,
    pub payload: serde_json::Value,
}
```

规则：

1. `TaskDAO`、`TaskDependencyDAO`、`TaskActorDAO` 和 `TaskCommentDAO` 是持久化对象；只有它们能被仓库写入。
2. `Task`、`TaskDependency`、`TaskActor`、`TaskComment` 和 `TaskOperation` 是视图对象，由每次读取时的 DAO/事件数据和视图数据构造，不是数据库行的别名。`Task.role_specs` 来自 workflow 注册表，`Task.allowed_inputs` 来自 workflow 的 stage 声明并经授权过滤，二者都不是数据库列。`SignalKey` 是 workflow 注册的信号标识（如 `completed`、`approved`），也不是数据库列。
3. `Task.actors` 是 `task_actors` 的视图投影，额外带上 `display_name`。可见范围按 viewer 裁剪：PM/Owner 看到全部角色成员；普通 viewer 看到所有 `active = true` 的行以及自己的行；其他 inactive 行不进入视图。`display_name` 只用于展示，不参与流程判断。
4. `work` 是隔离字段：只有 viewer 自己的 active 行，或 PM/Owner 查看时，`TaskActor.work` 才为 `Some`；其他 actor 的行一律为 `None`。构造 `Task` 时必须执行该裁剪，不能依赖调用方自觉。
5. API 返回的 `my_assignments` 和 `active_actors` 是 `Task.actors` 的投影，不是独立字段：前者按 viewer 过滤，后者按 `active = true` 过滤。
6. 视图字段不参与 workflow 决策。workflow 只读取 `TaskDAO`、`TaskActorDAO`、依赖状态和项目配置，不读取 `display_name`、`TaskDependency.title` 等展示数据。
7. 写入路径只接受 DAO；不能把 `Task` / `TaskDependency` / `TaskActor` / `TaskComment` 直接写回数据库。
8. `Task` 是快照；命令执行后重新读取，不跨 viewer 缓存。
9. `Task.operations` 是当前 Task 的只读操作历史，按 `events.seq` 倒序排列（写入顺序；展示时间用 `occurred_at`）；元素来自 `events` 中 `aggregate_id = task.id` 的不可变事件，不写入 `tasks` 表，也不新建操作表。`DomainEventKind` 增加 Task 事件类型，至少覆盖 4.2 的事件和 `state`/`stage`/`result` 变化。
10. 每条已接受的输入、角色成员添加和每个流程效果都必须在同一事务中写一条事件；因此 `Task.operations` 覆盖 Task 创建、输入、`state`/`stage`/`result` 变化、角色成员添加、active actor/`work` 变化、子 Task 创建/终止和调度唤醒。未授权输入只进入安全审计，不进入 `Task.operations`。
11. `TaskOperation.payload` 是事件详情的只读快照，按 `kind` 解释；视图层可以格式化，但不能回写，也不能用于 workflow 决策。
12. `Task.operations` 是分页读取的视图；`GET /tasks/{id}` 只返回最近一页，完整历史用 `/history`；分页游标基于 `events.seq`。
13. `Task.operations` 同样按 viewer 裁剪；事件 payload 中的 `work` 在构造 `TaskOperation` 时移除，只有对应 actor 或 PM/Owner 可见；其他 viewer 只能看到操作类型、actor、时间和其余非敏感字段。历史列表不能成为绕过 `work` 隔离的通道。
14. 列表查询不填充 `Task.dependencies` 完整摘要、`Task.comments` 和 `Task.operations`；但会计算 `Task.dispatch_state`（一个 `NOT EXISTS` 判断），因为调度查询需要。只有 Task 详情和对应子资源接口才读取完整依赖、评论和事件日志。
15. 评论、附件和 Requirement 是独立内容对象，不进入 `Task.operations`；评论通过 `Task.comments` 展示，附件和 Requirement 仍按各自对象管理。操作记录只覆盖 Task 生命周期和流程操作。
16. `Task.comments` 是 `task_comments` 的视图投影，按 `created_at` 正序（最早在前），同一时间按 `id`；`author_display_name` 和 `author_kind` 来自 accounts，只用于展示。
17. 评论是追加式内容；首期不支持编辑、删除或回复线程。创建评论由有权访问该 Task 且具备评论权限的项目成员发起，author 从会话派生，请求体不能指定 author。评论写入是独立追加操作，不进入 workflow 命令路径，也不需要 Task 行锁。Task 终止后仍可追加评论，评论不影响终止状态的不可变性。
18. 评论不驱动流程：不改变 `state`/`stage`/`result`/active actors/`work`，也不进入 `Task.operations`；需要流程响应的内容必须另发输入信号。
19. 评论对所有有权查看该 Task 的项目成员可见；首期不设 internal 标记，也不做 `work` 式隔离。
20. `GET /tasks/{id}` 返回最近一页评论（页内按 `created_at, id` 正序）；完整列表通过 `GET /tasks/{id}/comments` 分页读取。
21. `Task.dependencies` 是 `task_dependencies` 的视图投影，按 `depends_on_task_id` 稳定排序；`TaskDependency` 的 `title`、`state`、`result` 来自依赖 Task 的当前快照，只用于展示和判断依赖是否完成。不单独存储 `satisfied`，它等价于 `state == Completed`。
22. 依赖只能通过 `TaskDependencyDAO` 在 Task 创建事务中写入；`tasks` 表没有 `dependencies` 列，视图不能写回。创建后没有修改依赖的 API 或 workflow effect。
23. `TaskCreated` 事件 payload 包含创建时的依赖列表；依赖创建后不可变，因此 `Task.operations` 不需要依赖变更事件。依赖完成不写入依赖方的事件；依赖方被调度器唤醒时写入 `DispatchRequested`，actor 为 `System`，payload 不含依赖结果，workflow 直接读取当前 `DependencyState`。
24. workflow 收到的是 `DependencyState`（`task_id`、`state`、`result`），不是 `TaskDependency` 视图；脚本不能用依赖的标题或展示字段做流程判断。
25. 依赖摘要与依赖 Task 的可见性一致，按项目隔离；首期同一项目成员都可以看到。依赖不引入新的 `work` 式隔离。
26. `Task.dispatch_state` 是只读派生字段，不是 `TaskDAO` 列，不建表；由 `state` 和依赖状态按 3.2 的规则计算。
27. `Blocked`/`Ready` 只用于 `state = Pending`；`Running` 对应 `InProgress`；`Closed` 对应 `Completed`。无依赖或依赖全部 `Completed` 的 Pending Task 是 `Ready`；任一依赖不是 `Completed` 是 `Blocked`。
28. `dispatch_state` 每次读取时计算，不写回、不进入 `Task.operations`，也不作为 workflow 决策输入；workflow 使用 `TaskState` 和 `DependencyState`。
29. 列表查询可以计算 `Task.dispatch_state`（一个 `NOT EXISTS` 判断），但不填充完整 `Task.dependencies` 摘要；详情同时返回两者。
30. 不新增 `TaskDispatchState` 表/列，也不新增 `TaskReady` 事件；调度器直接查询 `GET /tasks?dispatch_state=ready`，发现后向事件流追加 `DispatchRequested`。

## 12. API、MCP、CLI、Web

### 12.1 REST

| 方法 | 路径 | 说明 |
|---|---|---|
| `POST` | `/api/v1/projects` | 创建项目，同时创建 Root Task |
| `GET` | `/api/v1/tasks` | 按项目、`state`、`dispatch_state`、`stage`、role、active actor、parent 过滤 |
| `GET` | `/api/v1/tasks/{id}` | Task、`dispatch_state`、依赖摘要、角色成员、active actors、当前 actor 的 `work`、允许的输入、最近一页评论、最近一页操作记录、子 Task 摘要 |
| `POST` | `/api/v1/tasks/{id}/inputs` | 发送纯信号 |
| `GET` | `/api/v1/tasks/{id}/allowed-inputs` | 当前 actor 可发送的信号 |
| `GET` | `/api/v1/tasks/{id}/comments` | 分页读取评论 |
| `POST` | `/api/v1/tasks/{id}/comments` | 添加评论 |
| `PUT` | `/api/v1/tasks/{id}/roles/{role}/actors/{account_id}` | PM 加入角色成员（仅 PM） |
| `GET` | `/api/v1/tasks/{id}/history` | 分页操作历史（`TaskOperation`） |
| `POST` | `/api/v1/tasks/{id}/attachments` | 登记附件元数据 |
| `GET` | `/api/v1/projects/{id}/requirements` | 需求池 |
| `POST` | `/api/v1/projects/{id}/requirements` | 创建粗略需求 |
| `PATCH` | `/api/v1/requirements/{id}` | 需求分析更新/新版本 |
| `GET` | `/api/v1/projects/{id}/workflow-config` | 当前配置 |
| `PUT` | `/api/v1/projects/{id}/workflow-config` | 创建新配置版本（按策略审批） |

`GET /tasks?dispatch_state=ready` 是调度器和 PM 的待调度查询；`dispatch_state=blocked` 用于查看等待依赖的 Task。该过滤由 `state` 和 `task_dependencies` 派生，不新增数据库列。

输入请求：

```json
{
  "signal": "completed",
  "idempotency_key": "task-...:completed:1"
}
```

评论请求：

```json
{
  "body": "已复现，建议先检查..."
}
```

Task 详情返回当前 actor 自己的 assignment；其他 active actor 只显示角色和身份，不暴露 `work`。`my_assignments` 和 `active_actors` 都是 `Task.actors` 的投影，不是独立存储的数据：

```json
{
  "id": "t1",
  "state": "InProgress",
  "stage": "review",
  "dispatch_state": "running",
  "dependencies": [
    {
      "task_id": "...",
      "title": "完成数据模型",
      "state": "Completed",
      "result": "approved"
    },
    {
      "task_id": "...",
      "title": "实现 API",
      "state": "Completed",
      "result": "approved"
    }
  ],
  "my_assignments": [
    { "role": "reviewer", "work": "审查安全性", "active": true }
  ],
  "active_actors": [
    { "role": "reviewer", "account_id": "...", "display_name": "Alice" },
    { "role": "reviewer", "account_id": "...", "display_name": "Bob" }
  ],
  "allowed_inputs": ["approve", "reject"],
  "comments": [
    {
      "id": "...",
      "author_account_id": "...",
      "author_display_name": "Alice",
      "author_kind": "human",
      "body": "已复现，建议先检查...",
      "created_at": "2026-11-04T..."
    }
  ]
}
```

不提供外部直接创建任意 Task 的接口。Root Task 由项目创建生成；子 Task 由流程脚本创建；人类/智能体通过 Requirements 和输入信号参与。

不提供外部修改依赖的接口。依赖只由创建 Task 的父流程在创建时指定；需要改变依赖时，创建新 Task，旧 Task 保留。

### 12.2 MCP 工具

- `list_tasks` / `get_task` / `list_task_children`
- `send_task_input`
- `list_task_comments` / `add_task_comment`
- `list_task_roles` / `add_task_role_actor` / `remove_task_role_actor`
- `list_allowed_inputs`
- `get_task_history`
- `list_requirements` / `create_requirement` / `update_requirement`
- `attach_document`
- `get_workflow_config` / `update_workflow_config`

`get_task` 返回 `dispatch_state`、依赖摘要、最近一页评论和操作记录；`list_tasks` 支持 `dispatch_state` 过滤（如 `ready`、`blocked`）；`list_task_comments` 分页读取评论；`get_task_history` 分页读取完整历史。没有新增或修改依赖的 MCP 工具。

工具只操作当前项目；权限由会话、项目上下文、角色成员、active actors 和配置共同决定。

### 12.3 CLI

```bash
foundry task list
foundry task list --dispatch ready
foundry task get <ID>
foundry task input <ID> --signal completed
foundry task comments <ID>
foundry task comment add <ID> --body "已复现，建议先检查..."
foundry task roles <ID>
foundry task role add <ID> --role acceptor --account <ACCOUNT>
foundry task role remove <ID> --role acceptor --account <ACCOUNT>
foundry task history <ID>
foundry requirement list
foundry requirement create fixtures/requirements/partial-refund.json
foundry requirement update <ID> fixtures/requirements/partial-refund-analyzed.json
foundry project config get
```

`task get` 返回 `dispatch_state` 和依赖摘要；`task list --dispatch ready` 查询待调度 Task。没有新增或修改依赖的命令，依赖由父流程在创建 Task 时指定。

### 12.4 Web

- 项目页：Root Task、当前 stage、子 Task 树、需求池。
- 我的 Task：我是 active actor 的 Task，按 stage 分组。
- 待调度：`dispatch_state = Ready` 的 Task；`Blocked` 的 Task 单独展示阻塞依赖。
- Task 详情：`state`/`dispatch_state`/stage、依赖 Task 列表与状态、角色成员、active actors、我的 `work`、允许的输入按钮、最近一页操作记录、评论列表与评论输入框、附件、子 Task。
- PM 视图：缺少角色成员的 role、`Ready` 待调度池、`Blocked` 等待池、停滞 Task、升级队列。
- 需求池：raw/analyzed 过滤、添加需求、查看来源和开放问题。
- 配置页：验收点的人类要求、角色约束。

按钮只展示当前 actor 允许的输入；服务端仍独立校验。

### 12.5 错误码

| HTTP | code | 含义 |
|---|---|---|
| 400 | `invalid_signal` | 信号不在当前 stage 允许列表 |
| 400 | `invalid_comment` | 评论内容为空或不可用 |
| 403 | `not_active_actor` | 不是当前 active actor |
| 403 | `policy_denied` | 项目配置/策略拒绝 |
| 403 | `human_acceptance_required` | 该验收点要求人类 |
| 404 | `not_found` | 对象不存在或不属于当前项目 |
| 409 | `invalid_state` | 当前 state/stage 不接受该信号 |
| 409 | `dependencies_unmet` | 依赖未全部 `Completed`，不能开始或完成 |
| 409 | `role_unbound` | 角色没有成员，无法激活 |
| 409 | `role_capacity_exceeded` | 角色成员数超过 `max_actors` |
| 500 | `workflow_error` | 流程脚本执行失败 |
| 503 | `workflow_unavailable` | 流程运行时不可用 |

## 13. 与现有实现的关系

### 13.1 命名冲突

新模型的 Task 取代原 Issue。当前代码中的旧 Task 聚合（Todo / InProgress / Blocked / Review / Done）不进入新模型：

- 新聚合使用 `Task` 名称；
- 旧 Task 重命名/标记为 `LegacyTask` 或删除（实现前检查引用）；
- 原 Issue 的 API 和数据迁移到 Task；
- 新 API 使用 `/tasks`，旧 `/issues` 作为兼容别名或只读过渡。

### 13.2 Issue 迁移

- 为每个已有项目创建 Root Task（`workflow=legacy_project_v1`）。
- 每个 Issue 迁移为 Root 下的子 Task（`workflow=legacy_issue_v1`）。
- 旧状态映射：

| 旧 IssueStatus | 新 state | stage | result |
|---|---|---|---|
| Open | Pending | open | — |
| Triaged | Pending | triaged | — |
| InProgress | InProgress | in_progress | — |
| Resolved | InProgress | resolved | — |
| Closed | Completed | closed | closed |
| Rejected | Completed | rejected | rejected |
| Duplicate | Completed | duplicate | duplicate |

- `legacy_issue_v1` 保留旧转换规则和旧字段；旧 `/issues` 端点映射到 Task 输入。
- 新项目和新任务使用新流程；旧项目可以逐步迁移，不强制一次性切换。

### 13.3 与 RFC 0002 的关系

本 RFC 取代 RFC 0002 中：

- Issue 的 `status`/`stage` 双层模型；
- 旧 Task 聚合；
- 项目流程对象的状态机表达；
- Issue 级权限覆盖中与 Task 流程相关的部分。

RFC 0002 中关于策略版本、审批、证据、发布、外部执行面、安全的原则仍适用，但需要按新 Task 模型重新映射。

## 14. 实施计划

### 阶段 0：确认设计

- 评审本 RFC。
- 确认旧 Task 聚合的处置和 Issue 迁移策略。
- 确认流程定义首期使用 Rust 代码而非 DSL。
- 确认授权粒度的首期范围。

### 阶段 1：平台事件总线

- 按 [RFC 0004](0004-durable-event-bus.md) 实现 `EventLog`、`Subscription`、`EventConsumer`、`UnitOfWork`、consumer 游标和 dispatcher。
- 内存实现覆盖顺序、分区隔离、游标推进和失败回滚。
- Postgres 实现 `event_consumer_cursors`（迁移 `0006_event_consumer_cursors.sql`）、`events(kind, seq)` 索引、`process_partition` 和多实例 `SKIP LOCKED`。
- control-plane 启动 dispatcher；此时还没有 `task-workflow` consumer。

### 阶段 2：领域与内存运行时

- `TaskDAO`、`TaskDependencyDAO`、`TaskActorDAO`、`TaskCommentDAO`、`Task`、`TaskDependency`、`TaskActor`、`TaskComment`、`TaskOperation`、`TaskState`、`TaskDispatchState`、`StageKey`、`TaskResult`、`WorkflowId`、`RoleSpec`、`Requirement`。
- `TaskWorkflow` trait、`WorkflowContext`、`DependencyState`、`Effect`。
- 内存仓库和确定性测试。
- 覆盖需求发现、迭代推演、父子终止事件、拒绝回退、依赖门禁和调度唤醒。

### 阶段 3：持久化与 Task consumer

- `tasks`、`task_dependencies`、`task_actors`、`task_comments`、`task_inputs`、`requirements`、`requirement_revisions`、`documents`。
- 实现 `task-workflow` consumer：按 `task_id` 分区、事务型投递、订阅触发事件。
- 同 Task 命令经 `process_partition` 串行化；幂等输入、效果事件和游标同事务。
- 依赖门禁、调度器查询 `Ready` 并追加 `DispatchRequested`、workflow 幂等处理重复唤醒。
- `dispatch_state` 派生查询和 `tasks(state)` 部分索引；不新增调度列/表。
- `Task.operations` 从 `events` 分页投影，不新建操作表。
- 迁移脚本和旧 Issue 兼容层。

### 阶段 4：PM、配置与授权

- PM 账户/项目管理者能力。
- PM 的 `Ready` 待调度池和 `Blocked` 等待池视图。
- 角色成员 API/MCP/CLI。
- 项目配置版本、人类验收点、角色约束。
- 授权引擎接入（先实现维度，规则粒度可迭代）。

### 阶段 5：默认流程

- `project`、`goal_clarification`、`requirements_discovery`、`requirements_analysis`、`architecture_design`、`roadmap`、`iteration`、`release`、`project_review`。
- Root 创建、子 Task 创建、迭代 just-in-time；发布和项目复盘模板暂不接入 Root。
- Web/MCP/CLI 操作面。

### 阶段 6：迁移与端到端

- 完成路径、失败 `result`、父 Task 子 Task 门禁。
- Issue 数据迁移和旧端点兼容。
- 端到端仿真：项目创建 → 目标澄清 → 需求发现/分析 → 架构 → Roadmap → 迭代 1/2。
- 依赖场景：创建带依赖的子 Task、依赖满足前拒绝启动、依赖完成后由调度器唤醒、依赖永远不完成时的升级路径。
- 调度状态场景：`Blocked` 与 `Ready` 的派生、`dispatch_state=ready` 过滤、`Ready` 不自动启动。
- 事件总线场景：跨 Task 触发只追加事件、目标 workflow 由 dispatcher 执行、重试不产生部分效果、并发命令与 dispatcher 不死锁。
- 审计、指标、异常升级。

## 15. 验收标准

1. 创建项目后自动创建 Root Task，并由 Root 流程创建目标澄清子 Task。
2. 只有 PM 能添加角色成员，不能移除；PM 不能设置 active actor 或 `work`，active actors 由 workflow 设置；项目 Owner 和 Admin 不能代替 PM 绑定角色成员。
3. 需求发现 Task 支持 `discoverer`/`acceptor` 角色；`approved` 完成，`rejected` 退回同一 Task，不创建新 Task。
4. 需求池包含 Requirement，不包含 Task；流程脚本不读取 Requirement 内容。
5. 迭代 Task 支持 planner → implementer → reviewer → acceptor 的 stage/active actor 切换；`changes_requested` 和 `rejected` 能退回；上一迭代终止后才创建下一迭代。
6. 首期 Root 默认流程不包含独立发布和主动项目完成；Root 没有 `release_requested`、`project_complete_requested`、`retry`、`cancel` 输入，只按子 Task 终止事件推进。
7. 父 Task 只根据子 Task 的 `result` 分支；没有“所有子 Task 完成就自动完成父 Task”的规则，但父 Task 的 `Complete` 要求所有直接子 Task 已 `Completed`。
8. 任何流程脚本都不能修改其他 Task 的 `state`/`stage`/active actors/`work`；跨 Task 只能向目标事件流追加触发事件。
9. 输入信号是纯信号；附件和 Requirement 内容不进入信号。
10. 人类验收点由项目配置决定；配置变化不回溯已进入的验收。
11. 首期没有取消：没有 `cancel` 输入、`CancelRequested` 事件、`cancelling` stage 或 `Cancelled` 状态；Task 只能通过 `Complete` 结束。
12. 所有状态、stage、active actor、work、角色成员、输入、配置变更可审计。
13. 旧 Issue 数据迁移后仍可按 legacy workflow 读取/处理。
14. Web、REST、MCP、CLI 对同一 Task 和同一输入给出一致结果。
15. 同一 Task 可以同时有多个 active actor；每个 actor 只看到自己的 `work`，其他 actor 只看到活跃参与者和角色。
16. `GET /tasks/{id}` 返回最近一页 `Task.operations`；`/history` 可分页读取完整操作历史；两者都来自不可变 `events`，不新增 Task 列或操作表。
17. 人类/智能体可以给 Task 追加评论；评论按时间展示，author 来自会话；评论不改变 `state`/`stage`，也不进入 `Task.operations`。
18. 有依赖的 Task 在所有依赖 `Completed` 前不能进入 `InProgress` 或 `Completed`；依赖完成后依赖方变为 `Ready`，由调度器发现并唤醒 workflow，系统不自动启动，是否开始由依赖方 workflow 决定。
19. 依赖只能引用同项目、创建时已存在的 Task，不能是自身祖先；依赖创建后不可变，因此不需要环检测。
20. `Pending` 且依赖未全部 `Completed` 的 Task 显示 `dispatch_state = Blocked`；`Pending` 且依赖全部 `Completed` 的 Task 显示 `Ready`；系统可按 `dispatch_state = ready` 查询待调度 Task。
21. `dispatch_state` 不写入 `tasks` 表，不进入 `events`；Task 从 `Blocked` 变为 `Ready` 不需要系统修改 `state`，由依赖派生。
22. 调度器可以重复向同一个 `Ready` Task 的事件流追加 `DispatchRequested`；workflow 幂等处理，已启动或已终止的 Task 不会重复产生效果。
23. 依赖永远不会完成时，依赖方保持 `Blocked`，不能进入 `InProgress`；PM 的等待池能发现并升级，但 PM 不能强制结束依赖方。
24. 创建依赖边与依赖终止并发时，不需要跨 Task 通知；依赖方的 `dispatch_state` 和门禁在读取时重新计算，不会出现依赖已终止但依赖方状态无法发现的情况。
25. 同一 Task 的全部输入、角色成员添加和触发事件由 `task-workflow` consumer 在同一 partition 内按 `seq` 串行处理；不同 partition 可以并行。
26. 事务型 consumer 的效果、效果事件和游标推进在同一事务；提交前崩溃、重试后不产生部分效果。
27. 命令路径先锁 consumer 游标、再锁 Task 行；与 dispatcher 并发时不死锁，同一触发事件不会被处理两次。
28. 子 Task 终止只向父 Task 的事件流追加 `ChildTerminal`，不锁父 Task；父 workflow 由父 partition 的 dispatcher 在父 Task 行锁内执行。
29. 没有跨 Task 的终止操作；父 Task 不能取消子 Task，也不存在终止级联。
30. 游标只推进到本次实际处理的触发事件，不跳过处理期间并发追加的事件；游标不出现在 `events` 或 `Task.operations` 中。
31. 新增一个非 Task 的 consumer 不需要修改事件生产者。
32. `Complete` 必须是效果列表中最后一个修改性效果；终止时原子设置 `state`/`result`、记录 `completed_at`、冻结 `stage`、清空 assignee，之后的修改性效果一律拒绝。
33. 父 Task 只由自己的 workflow 完成；存在未完成直接子 Task 时 `Complete` 被拒绝，没有强制终止或级联。

## 16. 风险与待决问题

1. **授权粒度**：`(workflow, stage, signal, actor, role)` 的具体规则格式未定；首期先实现维度和默认配置。
2. **无法停止卡住或失控的 Task**：首期没有取消，Task 只能由自己的 workflow `Complete`。如果 workflow 卡住、agent 失控或子 Task 永远不完成，父 Task 也无法完成；PM 只能升级或创建替代 Task，旧 Task 永久保留。若这成为现实问题，必须显式引入取消机制。
3. **流程定义表达**：首期 Rust 代码；是否需要 DSL、可视化编辑、版本回滚，后续决定。
4. **旧 Task 聚合**：当前代码里的 Task 是否被使用、如何迁移/删除，需要在实现前确认。
5. **PM 选择算法**：首期简单分配；容量、技能、成本、职责分离和故障转移未定。
6. **Requirement 生命周期**：目前只有 raw/analyzed/superseded 展示字段；需求分析是更新同一对象还是创建新对象，需确认。
7. **多 active actor 的完成语义**：任一 actor、全部 actor 或阈值达成，尚未定义；首期不设 per-actor 完成状态，复杂会签使用子 Task 或后续引入投票对象。
8. **成员移除**：首期 PM 不能移除角色成员，账户停用/离职时旧 actor 可能仍被 workflow 视为 active。PM 只能加入替代者，由 workflow 决定是否切换；移除语义、旧 `work` 和访问回收需要后续设计。
9. **空需求池**：需求发现产出为空时，需求分析 Task 如何退回/升级；可用自定义 `result` 或 attention_required stage。
10. **项目完成条件**：首期没有主动完成入口，项目在迭代循环中持续运行；未来接入 `project_review` 时再定义是否等待活跃迭代/发布、是否允许带尾完成。
11. **发布/项目复盘触发**：首期没有接入 Root；未来由哪个 workflow、在什么事件后创建 Release/ProjectReview Task，需要明确需求后再定义。
12. **附件存储与权限**：Git/对象存储的访问控制、Requirement 内容是否也走 Git，需按 ADR 0003 细化。
13. **外部创建 Task**：本设计不允许人类/智能体直接创建任意 Task；如果反馈需要直接进入 Task，需增加受控的反馈入口或由 Root 流程创建。
14. **workflow 升级**：Task 固定 workflow id；旧 workflow 保留策略、升级迁移和回放测试需明确。
15. **定时器/SLA**：首期无 timer；超时、提醒、自动升级需要后续事件类型。
16. **评论编辑/删除/线程**：首期只追加、扁平展示；如果需要编辑、删除或回复线程，需要补版本、软删除和审计规则。
17. **依赖可变性**：首期依赖只在创建时固定、之后不可变；如果后续需要运行中增加/替换依赖，必须补环检测、对已开始 Task 的影响、权限和审计规则。
18. **依赖永远不完成**：依赖方会一直 `Blocked`，没有取消或强制结束路径；PM 的等待池只能升级。默认 workflow 应如何升级、何时创建替代 Task，需要明确。依赖图本身不会自动改写。
19. **调度器触发与认领**：首期调度器轮询 `Ready` 并向事件流追加 `DispatchRequested`；是否引入专门的调度器、任务租约、抢占和优先级排序，后续决定。租约/认领应独立于 `TaskState` 和 `dispatch_state`。
20. **事件总线运维**：lag、毒事件、游标重置、订阅变更和新增 consumer 的起始位置见 [RFC 0004](0004-durable-event-bus.md) §15。首期没有 dead-letter；毒 `ChildTerminal` 或 `DispatchRequested` 会阻塞对应 Task 的 partition，需要人工处理。
21. **同步命令的事务长度**：命令路径在同一事务内处理该 partition 的待办触发事件；积压很大时事务会变长。批大小和超时策略需要与 RFC 0004 一起确定。
22. **父 Task 被子 Task 永久阻塞**：父 Task 的 `Complete` 要求所有直接子 Task 已 `Completed`；某个子 Task 永远不完成时，父 Task 也不能完成。首期没有取消或强制结束，只能人工介入或创建替代 Task；替代后原父子树仍然保留。
