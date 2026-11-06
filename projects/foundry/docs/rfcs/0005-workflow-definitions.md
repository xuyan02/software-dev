# RFC 0005：Workflow 定义、版本与可视化编辑器

- 状态：Draft，设计待评审
- 日期：2026-11-04
- 关联：[ADR 0001 Rust workspace 与六边形架构](../adr/0001-rust-workspace-and-hexagonal-architecture.md)、[ADR 0002 控制平面形态](../adr/0002-control-plane-form.md)、[ADR 0003 Git 存内容，Postgres 存状态](../adr/0003-git-content-postgres-state.md)、[ADR 0006 项目隔离](../adr/0006-project-isolation.md)、[RFC 0003 Task Workflow 模型](0003-task-workflow-model.md)、[RFC 0004 持久事件总线](0004-durable-event-bus.md)
- 取代：本 RFC 取代 [RFC 0003](0003-task-workflow-model.md) 中关于“workflow 定义首期只能是受信任的 Rust 代码”、首期不支持声明式 DSL/可视化编辑、workflow 只按 id 注册、Task 只存 workflow id 的部分；Task 的 `state`/`stage`/role/actor/input/effect 边界、事件总线、调度器、取消策略保持不变。

## 0. 结论

Foundry 的 workflow 从“启动时注册的 Rust 代码”升级为**可版本化、可可视化编辑的声明式状态机定义**。流程图是定义的一种展示和编辑界面，不是另一份真相。

核心决定：

1. **AST 是唯一真相**。workflow 定义是一份有类型的 JSON 文档，由受信任的解释器执行；流程图由 AST 派生，编辑流程图最终修改的是 AST。没有“流程图导出脚本”或“脚本同步到流程图”的双向同步。
2. **以 stage 为基本单元**。转移、子 Task 终止分支、创建时的初始化动作都定义在 stage 内部；`active_roles` 是 stage 的属性。一个 Task 同时只有一个当前 stage。
3. **定义归属只有两级：平台和账户**。不存在项目级 workflow。平台流程是全局目录；账户流程属于账户、默认私有，其他账户不可见、不可用。
4. **workflow id 全局唯一且不可变**。标题允许重复；id 不允许覆盖、遮蔽或复用。平台内置流程保留现有 9 个 id。
5. **版本是定义上的独立字段**。每次发布产生一个新版本；已发布版本不可变；定义不删除，只增加版本。Task 在创建时固定 `(workflow id, workflow version)`。
6. **项目创建必须选择根流程**。候选是“有已发布版本的平台流程”加“创建者账户下有已发布版本的账户流程”；控制台默认选中平台 `project`。项目与 Root Task 在同一事务创建，没有 Provisioning 状态。
7. **Root Task 一旦创建就固定根流程版本**；之后再发布新版本不影响它。子 Task 在创建时固定“当时对定义所有者可见的最新已发布版本”，因此子流程可以逐步升级，而 Root 保持稳定。
8. **引用作用域**：平台定义只能引用平台 id；账户定义可以引用平台 id 和同账户的 id；跨账户引用在发布时拒绝。运行时的子流程解析使用“父定义所有者”的作用域。
9. **账户流程私有**：只有所有者账户和平台管理员可见；管理员只读，不修改他人的账户流程。平台流程对所有已认证账户可见，普通用户只读，平台管理员可编辑和发布。
10. **只允许人类账户编写定义**。Agent 账户不能创建、修改、发布或复制 workflow 定义；RFC 0003 中“PM 智能体不能创建/修改流程定义”的约束继续成立。
11. **从平台复制是创建账户流程的一种方式**：生成新的账户 workflow id，把某个平台已发布版本和布局复制为初始草稿；复制后与原流程完全独立，不跟随平台更新。
12. **定义语言只暴露必要能力**：触发只有 `created`、`input`、`child_terminal`；效果只有 `set_state`、`to`、`create_child`、`complete`。`actor_bound` 和 `dispatch_requested` 不作为可编写触发：前者由 `active_roles` 隐式激活，后者只是平台调度唤醒。没有定时器、外部事件、任意函数、I/O 或循环。
13. **本期不支持 per-actor `work` 指令**。定义只能通过 stage 的 `active_roles` 批量激活角色成员，生成的 active actor 一律 `work = null`；没有定义语言、API 或编辑器入口写入 `work`。
14. **草稿是每个定义共享的一份**，带 revision 和幂等保存 id；自动保存、显式发布、冲突不自动合并。已发布版本不可变，布局是独立的展示元数据，采用 last-write-wins。
15. **可视化编辑器是独立的 SPA**，使用 React + TypeScript + React Flow + dagre，构建为静态资源挂在 `/workflows/editor/`；控制台其余部分继续服务端渲染、不引入 JavaScript。

## 1. 目标与非目标

### 1.1 目标

- 让 workflow 可编程、可测试、可审计，同时让人类可以用流程图观察和编辑它。
- 把流程定义从“发版才能改的代码”变成“平台目录 + 账户私有定义 + 不可变版本”。
- 保留 RFC 0003 的运行时边界：workflow 只能修改自己 Task 的 `state`/`stage`/active actors，只能创建子 Task，不能修改其他 Task。
- 让项目创建一步完成，并且项目根流程的选择是显式的、可审计的。
- 让已发布定义的行为可重现：Task 固定版本，旧版本永远可读；不存在“同一个 workflow id 的行为悄悄变化”。
- 让定义编辑足够安全：AST 是纯数据，解释器是纯函数，不允许任意代码、I/O、网络或随机数。
- 在不把控制台改成 SPA 的前提下，提供流畅的图形编辑体验。

### 1.2 非目标

- 不实现通用脚本语言（JavaScript、Lua、WASM、表达式内嵌代码等）；AST 不是图灵完备语言。
- 不实现项目级 workflow、项目覆盖、同 id 覆盖或流程继承。
- 不实现账户间共享、公开市场、流程转让或跨账户引用。
- 不实现 per-actor `work` 指令、会签/阈值聚合；这些仍按 RFC 0003 的边界处理。
- 不实现实时多人协作、自动合并、分支草稿；同一时刻只有一份共享草稿。
- 不实现定时器、SLA、超时升级、取消；这些需要独立设计，不能塞进 AST。
- 不实现定义删除、版本删除、版本回滚 API。回滚只能通过“把旧内容放进草稿并发布为新版本”完成。
- 不实现在线仿真/回放；本期用解释器单元测试和端到端测试保证行为。
- 不把控制台改造成 SPA；只有流程编辑器是 SPA。
- 不用可视化编辑器替代 Git 中的代码编辑；ADR 0002 的“不做代码编辑器”仍然成立，本编辑器只编辑结构化流程定义。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| workflow definition | 一份有 id、所有者、标题和版本序列的流程定义。 |
| workflow version | 定义的一次不可变发布快照，包含完整的 AST。 |
| workflow draft | 每个定义共享的一份可编辑工作副本；有 revision，没有分支。 |
| publish | 把当前草稿固化为下一个版本；发布前必须通过完整校验。 |
| platform workflow | 所有账户可见的流程；9 个内置流程加平台管理员创建的全局流程。 |
| account workflow | 属于某个账户的私有流程；只有所有者账户和平台管理员可见。 |
| root workflow | 项目创建时选择的流程，用于创建 Root Task。 |
| workflow version pin | Task 创建时固定的 `(workflow id, workflow version)`；之后不可变。 |
| AST | 定义语言的数据结构：roles、initial stage、stages、transitions、guards、actions。 |
| stage | 定义中的一个阶段；包含 `active_roles`、输入转移、子 Task 终止分支和（仅初始 stage）创建动作。 |
| active_roles | stage 声明的“进入该 stage 后应激活哪些角色”的列表；不是具体账户。 |
| transition | stage 内由某个触发匹配后执行的一组动作。 |
| guard | 纯表达式，决定一个 transition 是否匹配。 |
| action / effect | 定义中的动作（`to`、`create_child`、`complete`、`set_state`）及其对应的运行时效果。 |
| title template | 支持字面量和有限插值的子 Task 标题模板。 |
| layout | 流程图节点的位置等展示元数据；不是定义语义，不进入发布版本。 |
| editor SPA | `/workflows/editor/` 下的 React 应用；只负责查看和编辑定义。 |

不变量：

1. 每个 Task 在创建时固定 `(workflow id, workflow version)`；此后该 Task 的解释、视图和审计都使用这个版本。Task 的 workflow id 不可变。
2. 每个 workflow id 全局唯一且不可变；标题可重复。不存在覆盖、别名或同 id 的第二个定义。
3. 已发布版本不可变。定义不删除，版本不删除；变化只能产生新版本。
4. 版本号在定义内独立递增，从 1 开始；不跨定义共享。
5. 平台定义只能引用平台 workflow id；账户定义只能引用平台 id 和同一账户的 id；跨账户引用在发布时拒绝。
6. 账户流程只对所有者账户和平台管理员可见；其他账户既不能读取，也不能把它选为项目根流程或子流程。
7. 只有人类账户能创建、修改、发布、复制定义；Agent 账户只能读取可见的已发布定义。
8. AST 是唯一真相；流程图、阶段卡片和边都是 AST 的投影，不能独立存在。
9. 不存在项目级 workflow；`workflow_definitions` 没有 `project_id`；项目只通过 Root Task 选择并固定一个根流程版本。
10. 项目创建必须显式选择根流程，并且项目行、Root Task 行、Root Task 的 `TaskCreated` 事件在同一事务中写入；没有 Provisioning 状态。
11. Root Task 固定创建时的根流程版本；后续发布不改变它。子 Task 固定创建时对“定义所有者作用域”可见的最新已发布版本。
12. 解释器是确定性的纯函数：相同定义、Task 快照、角色成员、active actors、依赖、子 Task 和触发事件产生相同效果；解释器不读时钟、不做 I/O、不使用随机数。
13. active actor 由“当前 stage 的 `active_roles` + 已绑定角色成员”派生；定义不能直接指定账户，也不能指定 per-actor `work`。
14. 已发布定义不能执行任意代码；guard、title template 和 action 都是有界、静态类型检查的受限语言。
15. 每个定义最多一份草稿；草稿保存和发布使用 revision 做乐观并发控制；冲突不自动合并。
16. 布局是展示元数据，独立于草稿内容和发布版本，采用 last-write-wins；布局变化不产生新版本。

## 3. 定义归属、可见性与版本

### 3.1 平台流程与账户流程

| | 平台流程 | 账户流程 |
|---|---|---|
| 所有者 | 平台（`owner_account_id = null`） | 一个账户（`owner_account_id = account.id`） |
| 来源 | 9 个内置流程 + 平台管理员创建 | 账户所有者创建，或从平台复制 |
| 可见性 | 已发布版本对所有已认证账户可见；未发布定义仅管理员可见 | 所有者账户和平台管理员可见；其他账户不可见 |
| 普通用户权限 | 只读 | 所有者可读、编辑、发布 |
| 平台管理员权限 | 可读、可编辑、可发布 | 可读；不可编辑、不可发布 |
| 项目根流程候选 | 有已发布版本的平台流程 | 创建者账户下有已发布版本的流程 |
| 子流程引用 | 只能引用平台流程 | 可以引用平台流程和同账户流程 |
| 删除 | 不删除 | 不删除 |

“平台管理员”指 `AccountRole::Admin` 的人类账户。“所有者账户”就是 `owner_account_id` 指向的账户；不存在额外的 workflow 成员表或共享表。

账户流程是**私有**的：其他账户看不到它的标题、版本、草稿或布局，也无法在项目创建、API 或运行时解析中使用它。平台流程是唯一的共享目录。

私有性约束的是定义目录、版本、草稿和布局，不是运行中的 Task 视图。账户流程被用作某个项目的根流程或在项目内创建子 Task 时，项目成员可以通过 Task 视图看到该流程的运行时投影（workflow id、stage key、角色声明、允许输入）；看不到定义 body、版本历史和草稿。这是运行共享项目所必需的最小暴露；如果要求连运行时投影也不可见，就不能把账户流程用于共享项目，而本期明确不引入项目级流程。

### 3.2 没有项目级 workflow，也没有覆盖

- `workflow_definitions` 没有 `project_id`；workflow 不隶属于项目。
- 项目创建只“选择”一个根流程并固定版本，不复制、不覆盖、不派生项目级流程。
- 不允许账户流程使用与平台流程相同的 id；不允许任何定义遮蔽另一个定义。id 由服务端生成或由内置目录保留。
- 需要项目专属流程时，创建一个账户流程，再在项目创建时选择它；它不是“项目流程”，而是“该项目使用的账户流程”。

### 3.3 id、标题和版本

- **id**：
  - 9 个内置流程保留现有 id：`project`、`goal_clarification`、`requirements_discovery`、`requirements_analysis`、`architecture_design`、`roadmap`、`iteration`、`release`、`project_review`。
  - 新建的平台或账户流程使用服务端生成的 `wf_<uuidv7>` 形式，id 不透明、不回收、不允许用户修改。
  - id 是全局主键；标题不是。
- **title**：
  - 用户可读名称，允许重复，最长 200 字符。
  - 标题是定义元数据，不属于版本语义；重命名不产生新版本，不影响已固定版本的 Task。
  - 重命名是显式操作并写入审计事件；并发重命名采用 last-write-wins。
- **version**：
  - 整数，定义内从 1 开始递增；每次发布 `max(version) + 1`。
  - 每个版本是完整 AST 快照；已发布版本不可变。
  - 版本之间没有继承、补丁或引用；旧版本永远可读。
  - 回滚不是 API：把旧版本内容放进草稿并发布，会产生一个内容相同或相近的新版本。
- **copy from platform**：
  - 生成新的账户 workflow id；新定义与源定义完全独立。
  - 初始草稿和布局复制自指定的平台已发布版本。
  - 之后平台流程发布新版本，不会传播到副本；副本发布也不影响平台流程。
  - 复制是逐字复制，**不重写引用**。如果源定义引用自身（`create_child.workflow`、`count_children` 或 guard 中的字符串），副本仍引用源定义；编辑器会把该引用显示为普通引用，作者需要手动改成新 id。不对 guard/title 中的字符串做语义重写，是为了避免复制过程意外改变条件文本。

### 3.4 可见性与权限矩阵

| 操作者 | 平台已发布 | 平台未发布 | 自己的账户流程 | 其他账户的流程 |
|---|---|---|---|---|
| 人类普通账户 | 读 | 不可见 | 读、编辑、发布、复制 | 不可见 |
| 人类平台管理员 | 读、编辑、发布 | 读、编辑、发布 | 读、编辑、发布 | 读；不可编辑/发布 |
| Agent 账户 | 读 | 不可见 | 不适用（Agent 不能拥有定义） | 不可见 |
| 项目创建 | 可选为根流程 | 不可选 | 可选为根流程 | 不可选 |

“读”包括读取定义元数据、版本列表、版本 AST、布局和草稿（草稿仅所有者和管理员）。API 对不可见对象统一返回 `404`，避免泄露账户流程是否存在。

### 3.5 账户停用与数据保留

- workflow 定义不随账户停用而删除；已发布版本永远保留。
- 账户停用后：该账户不能登录，不能继续编辑或发布；已有项目和 Task 继续按固定版本运行；其他账户不能使用它的流程；平台管理员仍可读取。
- 账户流程被用作 Root 流程的项目不受影响；运行时按定义所有者作用域解析其子流程，不依赖所有者登录。
- 如果未来需要硬删除账户，必须保留其 workflow 定义和版本的归属墓碑；本 RFC 不引入硬删除。

## 4. Workflow 定义语言

### 4.1 设计原则

1. **声明式**：定义是数据，不是脚本；解释器执行受限 AST。
2. **stage 中心**：一个 Task 只有一个当前 stage；转移、子 Task 分支和初始化动作都写在 stage 内。
3. **有限表达**：只暴露实现 workflow 所需的最小集合；没有任意函数、循环、I/O、变量赋值或模块。
4. **静态可检查**：类型、引用、顺序和效果约束在发布前校验；运行时仍保留防御性校验。
5. **效果边界不变**：AST 只能生成 RFC 0003 已有的效果，不能绕过 Task 运行时。
6. **可投影**：每个 stage、转移和完成动作都能稳定映射为流程图的节点和边；布局不影响语义。

### 4.2 顶层结构

```json
{
  "schema_version": 1,
  "roles": [],
  "initial_stage": "work",
  "stages": []
}
```

| 字段 | 必填 | 说明 |
|---|---|---|
| `schema_version` | 是 | 本期固定为 `1`；未知版本拒绝。 |
| `roles` | 否，默认 `[]` | 工作流声明的角色。 |
| `initial_stage` | 草稿可空，发布必填 | 初始 stage 的 key；必须是 `stages` 中的一项。 |
| `stages` | 是 | stage 列表；发布时至少一个。 |

限制：AST 序列化后不超过 1 MiB；最多 200 个 stage、50 个 role、500 条转移；guard 嵌套深度不超过 20；标题模板不超过 500 字符。超限在保存或发布时拒绝。

### 4.3 角色

```json
{
  "key": "reviewer",
  "kind": "human",
  "min_actors": 0,
  "max_actors": null
}
```

| 字段 | 说明 |
|---|---|
| `key` | role 标识，匹配 `[a-z][a-z0-9_]*`，定义内唯一。 |
| `kind` | `human` / `agent` / `any`；绑定角色成员时校验。 |
| `min_actors` | 期望的最少成员数；供 PM/界面提示，不阻断运行时激活。 |
| `max_actors` | 最多成员数；`null` 表示不限；绑定和激活时校验。 |

角色 key 在定义内唯一；stage 的 `active_roles` 和 guard/title 中的引用都使用 key。

### 4.4 Stage

```json
{
  "key": "work",
  "active_roles": ["clarifier"],
  "created": [],
  "inputs": [],
  "child_terminal": []
}
```

| 字段 | 必填 | 说明 |
|---|---|---|
| `key` | 是 | stage 标识，匹配 `[a-z][a-z0-9_]*`，定义内唯一。 |
| `active_roles` | 否，默认 `[]` | 进入该 stage 后激活的角色；必须是已声明 role，且不重复。 |
| `created` | 否，默认 `[]` | Task 创建时的初始化动作；只允许出现在初始 stage。 |
| `inputs` | 否，默认 `[]` | 输入转移列表；同一个 signal 可以有多条，按顺序匹配。 |
| `child_terminal` | 否，默认 `[]` | 子 Task 终止分支列表；按顺序匹配。 |

进入 stage（`created` 或 `to`）时，运行时把该 stage 的 `active_roles` 作为当前激活角色集合；所有绑定到这些角色的账户成为 active actors。`active_roles = []` 会清空 active actors。

`created` 在非初始 stage 出现且非空是校验错误；编辑器中只在初始 stage 展示创建动作。

### 4.5 触发与可编写性

| 触发 | 来源 | 是否可编写 | 语义 |
|---|---|---|---|
| `created` | `TaskCreated` | 仅初始 stage | 进入初始 stage 后执行初始化动作。 |
| `input` | `InputReceived` | 是 | 当前 stage 的输入转移。 |
| `child_terminal` | `ChildTerminal` | 是 | 当前 stage 的子 Task 终止分支。 |
| `actor_bound` | `ActorBound` | 否 | 如果绑定的 role 在当前 stage 的 `active_roles` 中，重新计算 active actors。 |
| `dispatch_requested` | 调度器 | 否 | 平台唤醒；本期定义没有对应动作，事件仍被消费和记录。 |

`actor_bound` 不开放给定义，是因为“stage 进入时激活哪些角色”已经由 `active_roles` 完整表达；如果允许脚本单独处理绑定事件，就会重新引入 per-actor 激活逻辑和 `work` 语义，而本期不需要。`dispatch_requested` 是平台调度设施，不是业务触发；workflow 的开始仍由授权输入或 `created` 动作决定。

没有定时器、webhook、外部消息、失败重试事件或取消事件；需要这些时必须先扩展平台事件模型，不能伪装成 input。

### 4.6 输入转移

```json
{
  "signal": "completed",
  "when": {
    "==": [{"field": "task.state"}, "in_progress"]
  },
  "actions": [
    {"to": "awaiting_acceptance"}
  ]
}
```

| 字段 | 必填 | 说明 |
|---|---|---|
| `signal` | 是 | 纯信号名，匹配 `[a-z][a-z0-9_]*`；不带业务内容。 |
| `when` | 否 | guard；省略表示无条件匹配。 |
| `actions` | 否，默认 `[]` | 匹配后执行的动作；空列表表示“只接受信号，不改变 stage”。 |

规则：

1. 一个 stage 的 `inputs` 中出现的所有 signal 构成“该 stage 声明的信号集合”。发送未声明的 signal 返回 `InvalidSignal`。
2. 发送者必须是当前 active actor；授权只要求“该 actor 在当前 stage 的 `active_roles` 中”，不区分具体 signal。v1 不支持 per-signal 角色。
3. 已声明且已授权的输入被接受后，按事件到达前的 Task 快照顺序匹配 `inputs`；匹配确定后、执行 actions 前，如果 Task 是 `Pending`，产生 `SetState(InProgress)`。
4. 第一个 `when` 为真（或没有 `when`）的转移生效；同一 signal 的后续转移不再评估。
5. 如果 signal 已声明、已授权，但所有 guard 都为假，则输入被接受但无进一步效果：输入事件保留在历史中，`state`/`stage` 不变（`Pending` 的启动效果除外）。
6. 同 signal 的无条件转移必须排在所有带 guard 的同 signal 转移之后，否则后面的转移不可达，属于校验错误。
7. `when` 只能使用 `task.state` 和 `count_children(...)`；不能使用 `trigger.child.*`。

### 4.7 子 Task 终止转移

```json
{
  "when": {
    "all": [
      {"==": [{"field": "trigger.child.workflow"}, "goal_clarification"]},
      {"==": [{"field": "trigger.child.result"}, "approved"]}
    ]
  },
  "actions": [
    {"to": "requirements_discovery"},
    {"create_child": {"workflow": "requirements_discovery", "title": "Discover initial requirements"}}
  ]
}
```

| 字段 | 必填 | 说明 |
|---|---|---|
| `when` | 否 | guard；省略表示匹配任何子 Task 终止。 |
| `actions` | 否，默认 `[]` | 匹配后执行的动作。 |

规则：

1. 只评估当前 stage 的 `child_terminal` 列表；第一个匹配的条目生效。
2. 没有匹配条目时，事件仍被记录，但不产生效果。
3. 无条件的条目必须排在所有带 guard 的条目之后。
4. `when` 可以使用 `task.state`、`trigger.child.workflow`、`trigger.child.result`、`count_children(...)`。
5. 子 Task 的版本不参与匹配；父流程只看到 child workflow id 和结果标签。

### 4.8 动作与效果

| 动作 | 允许的触发 | 运行时效果 | 约束 |
|---|---|---|---|
| `{"set_state": "in_progress"}` | 仅 `created` | `SetState(InProgress)` | 至多一次；只允许 `in_progress`；用于 Root 等需要显式启动的流程。 |
| `{"to": "stage_key"}` | `input`、`child_terminal` | `SetStage(target)` + `SetActiveActors(target.active_roles)` | 至多一次；target 必须存在且不等于当前 stage；不能与 `complete` 同时出现；不能在 `created` 中出现。 |
| `{"create_child": {"workflow": "id", "title": "template"}}` | 任意 | `CreateChild { workflow, title, dependencies: [] }` | 可多次；目标必须可解析；title 是合法模板；不能与 `complete` 同时出现。 |
| `{"complete": {"result": "approved"}}` | 任意 | `Complete { result }` | 至多一次；必须是动作列表最后一项；`result` 可省略。 |

规则：

1. **`Complete` 最后**：`complete` 之后不能再有修改性动作；运行时仍会强制校验。
2. **`to` 取代 `set_stage`**：stage 的进入和 active actor 激活是一个原子语义；定义不能单独设置 stage 或 active actors。
3. **`active_roles` 取代 `set_active_actors`**：定义不能指定具体账户，也不能指定 `work`。运行时按当前 stage 的角色成员派生 active actors。
4. **`set_state` 只用于启动**：`Pending → InProgress` 也可以由被接受的 input 隐式触发；`set_state` 只用于没有输入、需要在 `created` 时启动的流程（例如 `project`）。不允许设置 `Pending` 或 `Completed`；`Completed` 只能由 `complete` 产生。
5. **`create_child` 不带依赖**：本期 AST 不提供子 Task 依赖表达式。运行时效果仍支持 `dependencies`，但定义语言固定传空列表。没有内置流程需要由定义创建带依赖的子 Task；如果将来需要，必须增加“同一事件中先前创建的子 Task 引用”语法，不能凭空使用运行时 Task id。
6. **没有 `emit_event`**：定义不能写任意事件；运行时按效果生成审计事件。
7. **没有 `work`**：定义没有 per-actor 工作说明字段。
8. **没有变量和循环**：同一动作列表内不能引用前面动作的结果；`count_children` 读取的是触发事件发生前的子 Task 快照。

### 4.9 Guard 表达式

Guard 是纯表达式，JSON 形式如下：

```json
{
  "all": [
    {"==": [{"field": "task.state"}, "in_progress"]},
    {"not": {"==": [{"field": "trigger.child.result"}, "failed"]}}
  ]
}
```

语法：

| 形式 | 含义 |
|---|---|
| `{"all": [expr, ...]}` | 全部为真；空数组为真。 |
| `{"any": [expr, ...]}` | 任一为真；空数组为假。 |
| `{"not": expr}` | 取反。 |
| `{"==": [left, right]}` | 相等。 |
| `{"!=": [left, right]}` | 不等。 |
| `{"in": [left, [literal, ...]]}` | left 属于字面量列表。 |
| `{"not_in": [left, [literal, ...]]}` | left 不属于字面量列表。 |
| `{"field": "task.state"}` | 读取字段。 |
| `{"count_children": "workflow_id"}` | 当前 Task 的直接子 Task 中，workflow id 等于参数的数量；返回整数。 |
| `"literal"` / `123` / `true` / `false` / `null` | 字面量。 |

字段：

| 字段 | 类型 | 可用触发 |
|---|---|---|
| `task.state` | `pending` / `in_progress` / `completed` | 全部 |
| `trigger.child.workflow` | string | `child_terminal` |
| `trigger.child.result` | string 或 null | `child_terminal` |

类型规则：

- `==` / `!=` 两侧类型必须相同，或其中一侧是 `null`。
- `in` / `not_in` 的右侧必须是同类型字面量数组；`null` 与任何列表成员都不相等，`null in [...]` 为假，`null not_in [...]` 为真。
- `all` / `any` 的元素必须是 bool；`not` 的操作数必须是 bool。
- Guard 读取触发事件到达前的 Task 快照；同一事件内的 `SetState`/`SetStage` 效果不会改变本次 guard 的输入。
- `count_children` 返回 int，只能与 int 比较或作为整数表达式使用；它的参数与 `create_child.workflow` 使用同一套作用域/已发布校验，自身引用除外。
- 未知字段、未知操作符、类型不匹配、在错误触发中使用 `trigger.child.*` 都是发布校验错误。

限制：

- 没有算术比较（`<`、`>`）、字符串函数、正则、成员访问、变量、用户函数。
- 没有 I/O、时间、随机数、环境读取。
- `all`/`any` 可以短路；表达式必须无副作用。
- `count_children` 统计的是直接子 Task，不递归；参数是 workflow id 字符串，按 §6.2 校验作用域和已发布版本（自身引用除外）。

### 4.10 Title 模板

```json
"Iteration {count_children('iteration') + 1}"
```

语法：

- 字面量文本 + `{expr}` 插值。
- `{{` / `}}` 表示字面量花括号。
- `expr` 支持：
  - 字符串字段：`task.title`、`trigger.child.workflow`、`trigger.child.result`；
  - 整数表达式：`count_children('workflow_id')`，可加整数字面量，例如 `count_children('iteration') + 1`。
- 字符串字段为 `null` 时插值为空字符串。
- 字符串字段的可用触发与 guard 相同：`trigger.child.*` 只在 `child_terminal` 动作中可用，`task.title` 在所有触发中可用。
- 模板必须能静态确定类型；未知字段、未知函数或对字符串做加法是发布校验错误。
- 生成后的标题必须非空且不超过 200 字符；运行时生成失败视为 workflow 错误（发布校验应提前排除）。

模板只用于 `create_child` 的标题，不参与 guard，不读取依赖或子 Task 标题。

### 4.11 完整示例

#### goal_clarification

```json
{
  "schema_version": 1,
  "roles": [
    {"key": "clarifier", "kind": "any", "min_actors": 1, "max_actors": null},
    {"key": "acceptor", "kind": "any", "min_actors": 1, "max_actors": null}
  ],
  "initial_stage": "work",
  "stages": [
    {
      "key": "work",
      "active_roles": ["clarifier"],
      "inputs": [
        {"signal": "started", "actions": []},
        {"signal": "completed", "actions": [{"to": "awaiting_acceptance"}]}
      ]
    },
    {
      "key": "awaiting_acceptance",
      "active_roles": ["acceptor"],
      "inputs": [
        {"signal": "approved", "actions": [{"complete": {"result": "approved"}}]},
        {"signal": "rejected", "actions": [{"to": "work"}]}
      ]
    }
  ]
}
```

#### iteration

```json
{
  "schema_version": 1,
  "roles": [
    {"key": "planner", "kind": "any", "min_actors": 1, "max_actors": null},
    {"key": "implementer", "kind": "any", "min_actors": 1, "max_actors": null},
    {"key": "reviewer", "kind": "any", "min_actors": 0, "max_actors": null},
    {"key": "acceptor", "kind": "any", "min_actors": 1, "max_actors": null}
  ],
  "initial_stage": "planning",
  "stages": [
    {
      "key": "planning",
      "active_roles": ["planner"],
      "inputs": [
        {"signal": "started", "actions": []},
        {"signal": "plan_ready", "actions": [{"to": "implementation"}]}
      ]
    },
    {
      "key": "implementation",
      "active_roles": ["implementer"],
      "inputs": [
        {"signal": "started", "actions": []},
        {"signal": "implementation_complete", "actions": [{"to": "review"}]}
      ]
    },
    {
      "key": "review",
      "active_roles": ["reviewer"],
      "inputs": [
        {"signal": "review_approved", "actions": [{"to": "acceptance"}]},
        {"signal": "changes_requested", "actions": [{"to": "implementation"}]}
      ]
    },
    {
      "key": "acceptance",
      "active_roles": ["acceptor"],
      "inputs": [
        {"signal": "approved", "actions": [{"complete": {"result": "approved"}}]},
        {"signal": "rejected", "actions": [{"to": "implementation"}]}
      ]
    }
  ]
}
```

#### project（节选）

`project` 的每个 stage 都没有 `active_roles`，也不接受输入；它只创建生命周期子 Task 并根据子 Task 终止事件推进。下面是 `goal`、`iteration` 和 `attention_required` 三个 stage，其余中间 stage 结构相同。

```json
{
  "schema_version": 1,
  "roles": [],
  "initial_stage": "goal",
  "stages": [
    {
      "key": "goal",
      "active_roles": [],
      "created": [
        {"set_state": "in_progress"},
        {"create_child": {"workflow": "goal_clarification", "title": "Clarify project goals"}}
      ],
      "child_terminal": [
        {
          "when": {
            "all": [
              {"==": [{"field": "trigger.child.workflow"}, "goal_clarification"]},
              {"==": [{"field": "trigger.child.result"}, "approved"]}
            ]
          },
          "actions": [
            {"to": "requirements_discovery"},
            {"create_child": {"workflow": "requirements_discovery", "title": "Discover initial requirements"}}
          ]
        },
        {
          "when": {"in": [{"field": "trigger.child.result"}, ["failed", "infeasible", "obsolete"]]},
          "actions": [{"to": "attention_required"}]
        }
      ]
    },
    {
      "key": "iteration",
      "active_roles": [],
      "child_terminal": [
        {
          "when": {
            "all": [
              {"==": [{"field": "trigger.child.workflow"}, "iteration"]},
              {"==": [{"field": "trigger.child.result"}, "approved"]}
            ]
          },
          "actions": [
            {"create_child": {"workflow": "iteration", "title": "Iteration {count_children('iteration') + 1}"}}
          ]
        },
        {
          "when": {"in": [{"field": "trigger.child.result"}, ["failed", "infeasible", "obsolete"]]},
          "actions": [{"to": "attention_required"}]
        }
      ]
    },
    {
      "key": "attention_required",
      "active_roles": []
    }
  ]
}
```

`project` 的完整 stage 序列是：`goal` → `requirements_discovery` → `requirements_analysis` → `architecture` → `roadmap` → `iteration`；每个中间 stage 匹配对应子流程的 `approved` 并创建下一个子 Task，同时在最后放一条失败分支转入 `attention_required`。`iteration` stage 在每次迭代 `approved` 后只创建下一个迭代子 Task，不改变 stage；`attention_required` 是刻意保留的等待升级 stage，没有出口。

## 5. 运行时语义

### 5.1 加载固定版本

`tasks` 保存 `workflow`（定义 id）和 `workflow_version`（版本号）。dispatcher 在处理该 Task 的触发事件时按 `(workflow, workflow_version)` 加载 AST，并使用该版本解释；不会读取最新版本。

已发布版本不可变，因此进程内可以按 `(workflow, version)` 缓存 AST，且不需要失效。定义元数据、草稿和最新版本指针可以变化，但固定版本的执行不受影响。

Task 视图中的 `stage`、`role_specs`、`allowed_inputs` 都来自固定版本；如果旧版本的 stage 已从最新版本删除，旧 Task 仍能正确显示和运行。

### 5.2 `created`

处理 `TaskCreated` 时：

1. 如果 Task 已终止，直接 no-op（沿用现有运行时）。
2. 设置 `stage = initial_stage`，并按该 stage 的 `active_roles` 计算 active actors。
3. 按顺序执行初始 stage 的 `created` 动作。
4. `created` 动作可以包含 `set_state`、`create_child`、`complete`；不能包含 `to`。

`set_state` 用于 Root 等“没有外部输入也要开始”的流程。普通工作流程通常在初始 stage 声明 `started`，由 input 隐式启动；也可以完全省略 `set_state`，让 Task 保持 `Pending` 直到有合法输入。

`created` 中的 `create_child` 与 `complete` 不能同时出现：运行时 `Complete` 要求所有直接子 Task 已 `Completed`，同一事件刚创建的子 Task 不可能满足。该组合在校验阶段就拒绝。

### 5.3 `actor_bound`

`ActorBound` 不作为可编写触发，但运行时仍处理：

1. 校验 role 存在（沿用现有运行时）。
2. 如果该 role 在当前 stage 的 `active_roles` 中，重新计算当前 stage 的 active actors：所有绑定到 `active_roles` 中任意角色的账户都成为 active actor。
3. 如果该 role 不在当前 stage 的 `active_roles` 中，不产生 active actor 变化；等进入包含该 role 的 stage 时再激活。
4. 计算结果是全量替换，不是增量追加；同一 role 的多个成员都会激活（受 `max_actors` 约束）。

这复刻了现有 `TableWorkflow` 的 `ActorBound` 行为，同时把“哪些角色在当前 stage 活跃”变成 stage 的声明属性。

### 5.4 `input`

1. 校验发送者是当前 active actor。没有 active actor 时，任何输入都未授权。
2. 在固定版本的当前 stage 中查找 `signal` 对应的 `inputs`。没有声明则返回 `InvalidSignal`（400）。
3. 已声明且已授权的输入被接受：
   - 按事件到达前的 Task 快照，按顺序评估同 signal 的转移；
   - 确定第一个匹配的转移（可能没有）；
   - 如果 Task 是 `Pending`，先产生 `SetState(InProgress)`；
   - 执行匹配转移的 actions；
   - 没有匹配则只保留输入事件和启动效果，不改变 `stage`/active actors。
4. 输入处理保持 RFC 0003 的幂等、串行化和审计规则；本 RFC 不改变 `task_inputs` 或 consumer 游标。

授权粒度是 stage 级，不是 signal 级：只要 actor 在当前 stage 的 `active_roles` 中，就可以发送该 stage 声明的任意 signal。9 个内置流程的 stage 角色与 signal 角色本来就一致，因此行为不变。需要更细粒度授权时，应拆成不同 stage，或在后续版本显式增加 per-signal 角色；本期不引入。

### 5.5 `child_terminal`

1. 只评估当前 stage 的 `child_terminal` 列表；第一个匹配的条目执行 actions。
2. 没有匹配条目时，事件仍进入 Task 操作历史，但不产生效果。
3. 父流程不能读取子 Task 的中间状态或内容；匹配字段只有子 workflow id 和终止 `result`。
4. 运行时 `Complete` 仍要求所有直接子 Task 已 `Completed`；本 RFC 不引入终止级联。

stage-local 匹配是有意的：子 Task 终止分支只应该在“创建它或等待它”的 stage 生效，避免一个迟到的旧子 Task 事件把流程拉回其他阶段。

### 5.6 `dispatch_requested`

调度器仍会向 `Ready` Task 追加 `DispatchRequested`，dispatcher 仍会消费并记录该事件。但本期的定义语言没有可编写的 `dispatch_requested` 动作；它对固定版本的 workflow 解释是 no-op。

workflow 的启动路径是：

- Root/无输入流程：`created` 中的 `set_state`；
- 普通流程：当前 stage 声明的、由 active actor 发送的 input（通常是 `started`）。

`dispatch_requested` 不携带业务内容，不能替代 input，也不能用来实现定时器。

### 5.7 效果应用

解释器输出 RFC 0003 的效果：

| 定义动作 | 效果 |
|---|---|
| `set_state` | `SetState(InProgress)` |
| `to` | `SetStage(target)`，随后 `SetActiveActors(按 target.active_roles 计算)` |
| `create_child` | `CreateChild { workflow, title, dependencies: [] }` |
| `complete` | `Complete { result }` |

运行时继续执行现有校验：

- `SetState` 只允许 `Pending → InProgress`；依赖未全部 `Completed` 时拒绝。
- `SetStage` 校验 stage 存在；同 stage 不产生变化。
- `SetActiveActors` 校验 role、账户绑定、重复和 `max_actors`；每个 active actor 的 `work` 为 `None`。
- `CreateChild` 在同一事务中创建子 Task 和 `TaskCreated` 事件；子 Task 的版本在插入时解析并固定。
- `Complete` 必须是最后一个修改性效果；校验依赖和直接子 Task；原子设置 `state`/`result`/`completed_at`、冻结 stage、清空 active actors，并向父 Task 追加 `ChildTerminal`。

解释器不直接写数据库；它只填充 `WorkflowContext` 的效果缓冲区，由现有 dispatcher 在 Task 行锁和事务中应用。

### 5.8 确定性与错误

- 解释器只读取固定版本 AST、Task 快照、角色成员、依赖、子 Task 和当前触发事件；不读时间、环境、网络或随机数。
- `count_children` 和 title template 使用触发事件发生前的子 Task 快照；同一事件内更早的 `create_child` 效果不会改变本次快照。
- 校验期错误（未知引用、类型错误、非法动作组合）阻止发布；运行期错误（例如数据损坏导致子流程无法解析）使该触发事件处理失败，事件保留并按事件总线规则重试。发布校验应确保正常运行不会出现这类错误。
- 运行时仍保留防御性校验；不能因为发布校验通过就跳过 `Complete`、依赖和 active actor 的检查。

## 6. 作用域、引用与版本解析

### 6.1 引用作用域

`create_child.workflow` 使用 workflow id，不使用版本号。引用是否合法由“当前定义的所有者”决定：

| 当前定义 | 可引用 |
|---|---|
| 平台定义 | 平台定义 |
| 账户定义 | 平台定义 + 同一账户的定义 |

规则：

1. 跨账户引用一律拒绝，即使引用者能读取该定义（例如平台管理员）。
2. 定义可以引用自己；这用于 `iteration` 创建下一个 `iteration` 子 Task。自引用在首次发布时也允许，因为正在发布的版本将成为可解析的已发布版本。
3. 不允许引用只有草稿、没有已发布版本的定义。
4. 引用 target 时只校验 id 和可见性；版本在运行时解析。
5. 定义之间的相互引用（A → B，B → A）是允许的；Task 树是运行时按事件逐步展开的有限树，不需要静态环检测。定义作者对可能产生的深层递归负责；平台可以后续增加配额，而不是在本期引入图灵完备的递归限制。

### 6.2 发布时引用校验

发布一个版本时，对每个 `create_child.workflow` 以及 guard/title 中的 `count_children` 参数校验：

1. target 定义存在且对当前定义所有者可见。
2. 如果 target 不是当前定义自身，target 必须有至少一个已发布版本。
3. 平台定义不能引用账户定义。
4. 账户定义不能引用其他账户的定义。
5. target 的 id 匹配 `[a-z0-9_]+` 或服务端生成的 `wf_...` 形式。

当前定义自身总是允许：发布成功后它就会拥有正在发布的版本；首次发布的自引用不会因为“还没有已发布版本”被拒绝。也正因为非自身的 target 必须先有已发布版本，第一次发布不能形成互相引用的闭环：至少有一个定义要先发布一个不含该引用的版本；自引用是例外。

### 6.3 子 Task 版本解析

`create_child` 在运行时解析版本：

1. 找到父定义（当前 Task 固定的版本所属定义）及其所有者作用域。
2. 在该作用域内查找 target 的最新已发布版本。
3. 在创建子 Task 的同一事务中插入子 Task，固定 `(target id, resolved version)`。
4. 无法解析时该效果失败；发布校验应保证这不会发生。

父 Task 自身的固定版本不影响子 Task 版本。例子：

- `project` 固定 v1；`iteration` 平台定义已发布 v2。
- Root 在 v1 的 `iteration` stage 创建子 Task 时，按平台作用域解析 `iteration` 的最新已发布版本 v2；子 Iteration 固定 v2。
- 下一次迭代再创建时，如果 `iteration` 已发布 v3，则新子 Task 固定 v3；之前的 Iteration 仍是 v2。
- Root 仍然是 v1，不受影响。

这个规则让长期运行的 Root 保持稳定，同时允许叶子流程独立演进。

### 6.4 Root 版本固定

项目创建时解析根流程的最新已发布版本，并在同一事务中写入 Root Task。之后发布任何新版本都不会改变该项目的 Root Task 及其行为。已存在的项目不迁移版本；它们的 Root Task 继续使用创建时固定的版本。

## 7. 持久化模型

### 7.1 表结构

```sql
create table workflow_definitions (
    id                text primary key,
    owner_account_id  uuid references accounts (id),
    title             text not null,
    created_at        timestamptz not null default now(),
    updated_at        timestamptz not null default now()
);

create index workflow_definitions_owner_idx
    on workflow_definitions (owner_account_id, updated_at desc);
```

- `owner_account_id is null` 表示平台流程；非空表示账户流程。
- 没有 `project_id`、`deleted_at`、`status`；定义不删除，可见性由 owner 和版本决定。
- 标题允许重复；id 主键保证全局唯一。

```sql
create table workflow_versions (
    workflow_id              text not null references workflow_definitions (id),
    version                  integer not null check (version >= 1),
    body                     jsonb not null,
    published_by_account_id  uuid references accounts (id),
    published_at             timestamptz not null default now(),
    primary key (workflow_id, version)
);
```

- 已发布版本不提供 UPDATE/DELETE 路径；应用层只插入。
- 版本号连续递增；不要求数据库序列，使用 `max(version) + 1` 在草稿行锁内计算。
- `body` 是完整 AST；不存单独的 stage/role 表。
- `published_by_account_id` 为 null 仅表示“系统引导发布的平台内置 v1”；普通发布必须写入发布者账户，API 视图把这个值显示为“系统”。

```sql
create table workflow_drafts (
    workflow_id              text primary key references workflow_definitions (id),
    revision                 bigint not null default 1 check (revision >= 1),
    body                     jsonb not null,
    last_client_save_id      uuid,
    updated_by_account_id    uuid not null references accounts (id),
    updated_at               timestamptz not null default now()
);
```

- 每个定义最多一行草稿；草稿是共享的，不分分支、不按编辑器实例隔离。
- `revision` 是乐观并发版本；每次成功保存和发布都递增。
- `last_client_save_id` 用于自动保存重试的幂等判断；只保留最近一次成功的保存 id。

```sql
create table workflow_layouts (
    workflow_id              text primary key references workflow_definitions (id),
    layout                   jsonb not null default '{}'::jsonb,
    updated_by_account_id    uuid not null references accounts (id),
    updated_at               timestamptz not null default now()
);
```

- 每个定义一行布局，作用于该定义的所有版本；布局不是版本快照的一部分。
- 布局采用 last-write-wins，不参与草稿 revision，不产生审计事件。
- 未知 stage key 的坐标被忽略；缺失 stage 由编辑器自动布局。

Task 增加版本固定：

```sql
alter table tasks
    add column workflow_version integer not null default 1;

alter table tasks
    add constraint tasks_workflow_version_fk
    foreign key (workflow, workflow_version)
    references workflow_versions (workflow_id, version)
    not valid;

create index tasks_workflow_version_idx
    on tasks (workflow, workflow_version);
```

- 迁移时 `workflow_version` 默认 1；随后启动引导先播种 9 个平台 v1，再 `validate constraint`。任何 Task 的 workflow id 不在平台 v1 中时，校验失败并拒绝启动。
- `not valid` 允许迁移在播种前添加约束；新写入仍立即受 FK 检查，只有既有行延迟到 `validate constraint`。
- `tasks.workflow` 继续保存定义 id；与 `workflow_version` 一起构成固定版本。不新增 UUID 版本列。

项目增加 Root Task 引用：

```sql
alter table projects
    add column root_task_id uuid;

alter table projects
    add constraint projects_root_task_fk
    foreign key (root_task_id) references tasks (id)
    deferrable initially deferred;

create unique index projects_root_task_idx
    on projects (root_task_id)
    where root_task_id is not null;
```

- 项目创建事务中先写项目（带预生成的 `root_task_id`），再写 Root Task；延迟 FK 在提交时校验，避免项目 ↔ Task 的循环引用问题。
- 迁移从 `tasks` 中 `parent_id is null` 的唯一 Root Task 回填 `root_task_id`；没有 Root Task 的项目保持 null，并列入迁移报告，不能静默伪造 Root。

### 7.2 对象模型

持久化对象（只映射数据库列）：

```rust
pub struct WorkflowDefinitionDAO {
    pub id: WorkflowId,
    pub owner_account_id: Option<Uuid>,
    pub title: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

pub struct WorkflowVersionDAO {
    pub workflow_id: WorkflowId,
    pub version: u32,
    pub body: WorkflowBody,
    pub published_by_account_id: Option<Uuid>,
    pub published_at: Timestamp,
}

pub struct WorkflowDraftDAO {
    pub workflow_id: WorkflowId,
    pub revision: u64,
    pub body: WorkflowBody,
    pub last_client_save_id: Option<Uuid>,
    pub updated_by_account_id: Uuid,
    pub updated_at: Timestamp,
}

pub struct WorkflowLayoutDAO {
    pub workflow_id: WorkflowId,
    pub layout: WorkflowLayout,
    pub updated_by_account_id: Uuid,
    pub updated_at: Timestamp,
}
```

视图对象（组合 DAO 和派生数据，不写回）：

```rust
pub struct WorkflowDefinition {
    pub id: WorkflowId,
    pub title: String,
    pub owner: WorkflowOwner,
    pub latest_version: Option<u32>,
    pub draft_revision: Option<u64>,
    pub editable: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

pub enum WorkflowOwner {
    Platform,
    Account { account_id: Uuid, display_name: String },
}

pub struct WorkflowVersion {
    pub workflow_id: WorkflowId,
    pub version: u32,
    pub body: WorkflowBody,
    pub published_by_account_id: Option<Uuid>,
    pub published_by_display_name: String,
    pub published_at: Timestamp,
}

pub struct WorkflowVersionRef {
    pub workflow_id: WorkflowId,
    pub version: u32,
}

pub struct PinnedWorkflow {
    pub workflow_id: WorkflowId,
    pub version: u32,
    pub owner: WorkflowOwner,
    pub body: WorkflowBody,
}

pub struct WorkflowDraftView {
    pub workflow_id: WorkflowId,
    pub revision: u64,
    pub body: WorkflowBody,
    pub updated_by_account_id: Uuid,
    pub updated_at: Timestamp,
}

pub struct WorkflowLayout {
    pub positions: BTreeMap<StageKey, StagePosition>,
}

pub struct StagePosition {
    pub x: f64,
    pub y: f64,
}
```

AST 类型：

```rust
pub struct WorkflowBody {
    pub schema_version: u32,
    pub roles: Vec<RoleSpec>,
    pub initial_stage: Option<StageKey>,
    pub stages: Vec<WorkflowStage>,
}

pub struct WorkflowStage {
    pub key: StageKey,
    pub active_roles: Vec<RoleKey>,
    pub created: Vec<Action>,
    pub inputs: Vec<InputTransition>,
    pub child_terminal: Vec<ChildTerminalTransition>,
}

pub struct InputTransition {
    pub signal: SignalKey,
    pub when: Option<Guard>,
    pub actions: Vec<Action>,
}

pub struct ChildTerminalTransition {
    pub when: Option<Guard>,
    pub actions: Vec<Action>,
}

pub enum Action {
    SetState(TaskState),
    To(StageKey),
    CreateChild { workflow: WorkflowId, title: TitleTemplate },
    Complete { result: Option<TaskResult> },
}
```

- `RoleSpec` 中现有的 `&'static str` 改为拥有所有权的字符串或 `Arc<str>`；现有 `StageDef` 被 `WorkflowStage` 取代（包含 `active_roles`、`created`、`inputs`、`child_terminal`）。`InputSpec` 不再作为定义结构（输入没有 per-signal roles），`allowed_inputs` 从 stage 的输入转移派生。
- `TaskDAO` 增加 `workflow_version: u32`；`Task` 视图增加 `workflow_version`。`Task.repository.insert` 写入 `(workflow, workflow_version)`。
- `WorkflowContext` 继续只暴露 Task 快照、角色成员、依赖、子 Task 和效果缓冲区；解释器通过固定版本的 AST 读取 stage/role 声明。

### 7.3 内置流程的播种与迁移

9 个内置流程以 JSON 资源随二进制发布（例如 `crates/domain/workflows/*.json`，用 `include_str!` 编译进去）。启动时执行一次系统目录引导：

1. 对每个内置 id：如果 `workflow_definitions` 不存在，插入平台定义和 `version = 1` 的完整 AST；`published_by_account_id` 为 null，表示系统引导发布。null 只允许出现在平台内置 v1，普通发布必须写入发布者账户；API 视图显示为“系统”。这样不引入虚假账户，也不要求首次启动时已存在管理员。
2. 如果定义存在但 v1 不存在，插入 v1；如果 v1 已存在但 AST 与内置 JSON 不一致，启动失败。已发布版本不可变，代码更新不能静默改写 v1。
3. 如果内置 id 被非平台定义占用，启动失败。
4. 引导完成后执行 `alter table tasks validate constraint tasks_workflow_version_fk`；未知 workflow id 会导致启动失败。
5. 迁移回填 `projects.root_task_id`，并把没有 Root 的项目列入报告。

这个引导只处理**平台系统目录**，不创建用户项目、账户或业务数据；与“启动不播种业务数据”的原则不冲突。9 个内置流程的 v1 是系统目录的一部分，必须可重现地存在。

修改内置流程的正确方式：

- 通过平台管理员在流程管理界面发布新版本（推荐）；或
- 在发版时增加一个数据迁移，插入显式的新版本，绝不修改 v1。

本地开发可以重建测试数据库；生产环境不能通过改 JSON 文件让既有 v1 变化。

### 7.4 生命周期与事务

- **创建账户流程**：同一事务插入 `workflow_definitions` 和 `workflow_drafts`（revision = 1，body 为空或复制内容）；可选插入 `workflow_layouts`（复制来源布局）。
- **创建平台流程**（管理员）：同上，但 `owner_account_id = null`；未发布前只对管理员可见。
- **打开草稿**：如果草稿不存在，从最新已发布版本复制 body（没有版本则用空 AST）并插入 revision = 1；并发打开用 `on conflict do nothing` 后读取。
- **保存草稿**：`update workflow_drafts set body = $body, revision = revision + 1, ... where workflow_id = $id and revision = $expected`；影响行数为 0 即冲突。`last_client_save_id` 与请求相同则直接返回当前 revision，不重复应用。
- **发布**：`select ... for update` 锁定草稿行；校验 revision；完整校验 AST；插入 `version = max(version) + 1`；把草稿 revision 加 1、清空 `last_client_save_id`；写审计事件；提交。
- **布局保存**：`insert ... on conflict (workflow_id) do update set layout = excluded.layout, ...`；不加 revision 条件。
- **重命名**：更新 `workflow_definitions.title` 和 `updated_at`，写审计事件；不改变版本或草稿 revision。

### 7.5 审计事件

新增审计事件（不进入 `task-workflow` 订阅）：

- `WorkflowDefinitionCreated`
- `WorkflowTitleChanged`
- `WorkflowVersionPublished`

草稿自动保存和布局保存不逐条写入 `events`：前者频率高、属于工作副本；后者是展示元数据。发布版本本身是持久、不可变的审计事实；定义创建和重命名也进入审计。需要更细的草稿审计时再单独设计，不能把每 600ms 的自动保存写进审计日志。

## 8. 项目创建与根流程

### 8.1 请求

```http
POST /api/v1/projects
Content-Type: application/json

{
  "name": "Payments",
  "description": "部分退款",
  "root_workflow_id": "project"
}
```

- `root_workflow_id` 必填。控制台在表单中默认选中 `project`，但用户必须看见并提交这个选择；服务端不提供隐藏默认值。
- 服务端按会话账户校验候选：平台已发布流程，或该账户自己的已发布流程。不可见或未发布返回 `404 workflow_not_found` / `409 workflow_unpublished`，不泄露其他账户的定义。
- 解析根流程的最新已发布版本；在事务中固定。

### 8.2 原子创建

同一事务中：

1. 插入 `projects` 行，预生成 `root_task_id` 并写入；延迟 FK 暂不检查。
2. 插入 `ProjectCreated` 事件；payload 与现有项目创建路径保持一致。
3. 插入 Root Task：
   - `project_id` = 新项目；
   - `parent_id` = null；
   - `workflow` = `root_workflow_id`；
   - `workflow_version` = 解析出的最新已发布版本；
   - `title` = 项目名称；
   - `state` = `Pending`；
   - `stage` = null；
   - 无依赖、无角色成员。
4. 插入 Root Task 的 `TaskCreated` 事件，payload 包含 workflow id、version、title、`parent_id: null`、空依赖。
5. 提交。延迟 FK 在提交时校验项目 ↔ Root Task。

没有 Provisioning 状态：项目创建成功即 Active，Root Task 已存在。Root 的初始 `state`/`stage` 由根流程的 `created` 动作决定，不再在 `create_root` 里硬编码 `project`/`goal`/`InProgress`。

### 8.3 创建后的初始化

提交后，命令路径同步调用一次 `dispatcher.process(root_task_id, None)`，让响应中的项目/根 Task 反映初始化后的 stage。如果这次处理失败：

- 不回滚项目；`TaskCreated` 已在事件日志中，dispatcher 后续会重试；
- 返回 201，响应包含 `root_task_id`；控制台展示 Root Task 时能看到 `Pending` 或后续重试结果；
- 记录告警，不能把“项目已创建但 Root 尚未初始化”伪装成创建失败。

### 8.4 现有项目

已存在的项目不重新选择根流程、不迁移版本；它们的 Root Task 保留当前 `workflow`/`workflow_version`（迁移统一回填为 v1）。后续发布新版本不影响它们。

## 9. Workflow 管理 API

### 9.1 REST

所有路径以 `/api/v1` 开头；身份来自登录会话，不使用 `account_id` 路径参数。读接口对 Agent 账户开放可见范围；写接口只允许人类账户。

| 方法 | 路径 | 说明 |
|---|---|---|
| `GET` | `/workflows` | 列出可见定义：平台已发布 + 自己的账户流程；管理员可见全部。支持分页。 |
| `POST` | `/workflows` | 创建账户流程（默认）；管理员可传 `scope=platform` 创建平台流程。可带 `copy_from`。 |
| `GET` | `/workflows/{id}` | 定义元数据、所有者、最新版本、草稿 revision（有权限时）、可编辑标记。 |
| `PATCH` | `/workflows/{id}` | 重命名标题。仅所有者（平台流程为管理员）。 |
| `GET` | `/workflows/{id}/versions` | 版本列表（版本号、发布者、发布时间）；不返回完整 body。 |
| `GET` | `/workflows/{id}/versions/{version}` | 读取某个不可变版本的完整 AST。 |
| `POST` | `/workflows/{id}/draft` | 打开/创建共享草稿；存在则返回，不存在则从最新版本复制。 |
| `GET` | `/workflows/{id}/draft` | 读取草稿；没有草稿返回 404。 |
| `PUT` | `/workflows/{id}/draft` | 保存草稿；要求 `If-Match` 和 `client_save_id`。 |
| `POST` | `/workflows/{id}/validate` | 对提供的 body 运行完整校验，不写入；供编辑器校验面板使用。 |
| `POST` | `/workflows/{id}/publish` | 发布当前草稿；要求 `If-Match`；返回新版本。 |
| `GET` | `/workflows/{id}/layout` | 读取定义布局。 |
| `PUT` | `/workflows/{id}/layout` | 保存布局；last-write-wins。 |
| `POST` | `/projects` | 创建项目；新增必填 `root_workflow_id`。 |

不提供：`DELETE /workflows/{id}`、`DELETE /versions`、`POST /rollback`、跨账户共享、草稿分支。定义永不删除；回滚通过发布旧内容实现。

#### 创建

```json
{
  "title": "我的迭代流程",
  "scope": "account",
  "copy_from": {"workflow_id": "iteration", "version": 1}
}
```

- `scope` 默认 `account`；`platform` 仅管理员。
- `copy_from` 必须是平台已发布版本；逐字复制 body 和布局到新草稿，不重写引用。
- 返回新定义和新草稿（revision = 1）。
- 标题必填、非空、最长 200 字符；允许重复。

#### 草稿保存

```http
PUT /api/v1/workflows/wf_019.../draft
If-Match: "7"
Content-Type: application/json

{
  "body": { "schema_version": 1, "roles": [], "initial_stage": null, "stages": [] },
  "client_save_id": "019..."
}
```

```json
{
  "revision": 8,
  "updated_at": "2026-11-04T10:00:00Z"
}
```

- `If-Match` 必填；缺失返回 `428 precondition_required`，格式错误返回 `400 invalid_if_match`。
- `client_save_id` 必填；如果它等于草稿的 `last_client_save_id`，直接返回当前 revision，不重复写入；否则按正常 `If-Match` 校验并保存。
- 保存只做结构解析和大小限制；语义校验不阻断保存，允许中间态。AST 字段类型错误返回 `422 workflow_invalid`。
- 冲突返回 `409 draft_conflict`，带 `current_revision` 和 `current_updated_at`。

#### 发布

```http
POST /api/v1/workflows/wf_019.../publish
If-Match: "8"
```

```json
{
  "version": 3,
  "revision": 9,
  "warnings": []
}
```

- 先按 `If-Match` 校验草稿 revision；不匹配返回 409。
- 对草稿 body 运行完整校验；有 error 返回 `422 workflow_invalid`，不插入版本、不改草稿 revision。
- 成功插入 `version = 3`，草稿 revision 递增；发布快照是草稿 body 的副本。
- 发布不自动清理草稿；草稿继续作为下一版本的工作副本。

#### 校验

```http
POST /api/v1/workflows/wf_019.../validate
Content-Type: application/json

{ "body": { ... } }
```

```json
{
  "errors": [
    {"path": "stages[1].inputs[0].actions[0].to", "code": "unknown_stage", "message": "stage 'revieww' does not exist"}
  ],
  "warnings": [
    {"path": "stages[2]", "code": "unreachable_stage", "message": "stage is not reachable from the initial stage"}
  ]
}
```

校验是无副作用的，使用与发布完全相同的校验器；错误路径用于编辑器定位。

### 9.2 错误码

| HTTP | code | 含义 |
|---|---|---|
| 400 | `invalid_if_match` | `If-Match` 格式错误。 |
| 400 | `invalid_request` | JSON、字段或分页参数非法。 |
| 403 | `workflow_forbidden` | Agent 账户写定义；非所有者写；管理员修改他人账户流程。 |
| 404 | `workflow_not_found` | 定义不存在或对当前账户不可见。 |
| 409 | `draft_conflict` | 草稿 revision 不匹配。 |
| 409 | `draft_not_found` | 发布时没有草稿。 |
| 409 | `workflow_unpublished` | 项目根流程没有已发布版本。 |
| 413 | `workflow_too_large` | AST 超过大小限制。 |
| 422 | `workflow_invalid` | AST 结构或语义校验失败；响应含 `errors`/`warnings`。 |
| 428 | `precondition_required` | 保存或发布缺少 `If-Match`。 |

工作流定义的读取权限错误使用 404，不使用 403，以避免泄露账户流程的存在；对**可见但不可写**的对象（普通用户写平台流程、管理员写他人账户流程）使用 403；对**完全不可见**的对象（其他账户的私有流程）读写都使用 404。

### 9.3 MCP 与 CLI

MCP 只增加只读工具：

- `list_workflows`
- `get_workflow`（可指定版本；不返回草稿）

不增加创建、修改、发布、复制工具；Agent 账户不能写定义，平台流程由人类管理员维护。这保持 RFC 0003 的约束。

CLI：

```bash
foundry workflow list
foundry workflow show <ID> [--version N]
foundry workflow publish <ID>
foundry project create --name "Payments" --root-workflow project
```

CLI 不提供 AST 直接编辑；结构化编辑在 Web 编辑器中完成，CLI 只做查看、发布和项目创建。`workflow publish` 使用当前草稿，需要草稿已通过 Web/API 编辑并满足校验。

### 9.4 Web

- 控制台导航新增“流程”；`/workflows` 是服务端渲染的列表页：
  - “平台流程”：已发布的平台流程；普通用户显示“查看”和“复制到我的账户”；管理员显示“编辑”。
  - “我的流程”：当前账户的账户流程，显示最新版本、草稿状态、编辑/发布入口。
  - 管理员额外显示“新建平台流程”和未发布的平台草稿。
  - “新建流程”：空白创建或从平台流程复制。
- 编辑器 SPA：`/workflows/editor/{id}`；只读版本：`/workflows/editor/{id}/versions/{version}`。
- 项目创建表单增加必填的“根流程”选择器，分组展示“平台流程”和“我的流程”，默认选中 `project`。服务端再次校验，不能只信表单。
- 控制台其余页面继续保持服务端渲染、无 JavaScript；流程编辑器是唯一的 SPA 区域。

## 10. 草稿、自动保存与发布

### 10.1 草稿模型

- 每个定义一份共享草稿；没有分支、没有“我的草稿”。
- `revision` 是单调递增整数；创建时为 1，每次成功保存或发布加 1。
- 草稿 body 可以暂时语义无效（缺少 stage、引用不存在等），但必须能解析为 AST 结构；编辑器允许中间态，发布才强制完整校验。
- 打开草稿时，如果不存在，从最新已发布版本复制 body；没有版本时使用空 AST。并发打开不能产生两份草稿（主键 + `on conflict do nothing`）。
- 草稿不随发布删除；发布后它继续作为下一版本的工作副本。
- 放弃修改不是独立 API：编辑器载入最新发布版本并保存即可；这会正常产生一个新 revision。

### 10.2 自动保存

编辑器在本地维护 AST，按以下策略保存：

1. 用户编辑后 600ms 防抖保存；从第一次未保存编辑起最多 5 秒必须尝试一次（最大等待），避免持续输入导致永不保存。
2. `Ctrl/Cmd+S` 立即保存。
3. 同一时刻只允许一个保存请求在途；保存期间的编辑标记为 dirty，完成后立即安排下一次保存。
4. 每次保存生成 `client_save_id`（UUID），同一个请求体的重试复用同一个 id；有新编辑时生成新 id。
5. 保存请求带 `If-Match: "<当前 revision>"`；成功后将 revision 更新为响应值。
6. 网络错误或 5xx 使用指数退避重试（1s、2s、4s、8s，上限 30s），状态栏显示“保存失败，重试中”。4xx 不自动重试。
7. 页面隐藏（`visibilitychange` → hidden）或 `pagehide` 时，如果有未保存内容，发送一次 `fetch(..., { keepalive: true })` 的最后保存；body 过大导致 keepalive 失败时，依赖 localStorage 备份。
8. 有未保存内容时注册 `beforeunload` 提示。

保存状态在工具栏显示：已保存、保存中、未保存、保存失败、冲突。

### 10.3 冲突

- 409 `draft_conflict` 表示另一个标签页、另一个编辑器或发布操作已经推进了 revision。
- 编辑器立即停止自动保存，显示冲突横幅，不自动合并。选项：
  - **载入服务端版本**：丢弃本地未保存修改，加载服务端草稿；
  - **用我的版本覆盖**：二次确认后，用服务端返回的当前 revision 作为 `If-Match`，把本地 body 保存为新 revision。覆盖是显式的、破坏性的，不做字段级合并。
- 多标签页编辑同一草稿时，后保存者会收到冲突；没有实时协作或锁。
- 发布也会递增 revision；编辑器发布成功后更新本地 revision，避免下一次保存误冲突。

### 10.4 localStorage 崩溃备份

- 每次本地编辑后（防抖前）尝试写入 `foundry.workflow-draft.{workflow_id}`：
  ```json
  {
    "revision": 7,
    "body": { ... },
    "client_save_id": "019...",
    "savedAt": "2026-11-04T10:00:00Z"
  }
  ```
- 读取和写入都包在 try/catch 中；隐私模式、配额不足或存储被禁用时静默降级，不能影响编辑。
- 打开编辑器时，如果本地备份的 revision 与服务端草稿相同但 body 不同，或本地 revision 更新（崩溃前未保存），提示“恢复未保存的修改”/“丢弃本地备份”。
- 成功保存、成功发布或用户明确丢弃后清除对应 key。
- localStorage 只做本机崩溃恢复，不作为共享状态；服务端草稿才是权威。

### 10.5 发布流程

1. 编辑器先 flush 未保存修改，等待保存完成；如果保存失败或冲突，不能发布。
2. 调用 `POST /validate` 或直接发布；发布服务端会重新运行完整校验，编辑器不能绕过。
3. 发布请求带当前 `If-Match` revision。
4. 校验错误：停在编辑器，展示错误路径；不产生版本，草稿不变。
5. 发布成功：显示新版本号和时间；版本列表刷新；草稿 revision 更新；继续编辑。
6. 发布是显式动作；自动保存永远不会触发发布。

### 10.6 布局

- 布局独立保存，不参与草稿 revision，不阻塞保存/发布。
- 客户端在节点拖动后 1s 防抖 `PUT /layout`；last-write-wins。失败静默重试或提示，但不影响 AST 保存。
- “自动布局”用 dagre 重新计算位置并保存；手动位置覆盖自动结果。
- 布局按定义保存，作用于所有版本；旧版本中不存在的 stage 坐标被忽略，新 stage 自动布局。
- 布局不写入 `workflow_versions`；发布不会快照布局。

## 11. 可视化编辑器

### 11.1 技术栈与部署

- 源码目录：`web/workflow-editor/`。
- 技术栈：Vite + React + TypeScript + `@xyflow/react`（React Flow）+ `@dagrejs/dagre`。
- React Flow 提供节点/边渲染、拖拽、缩放、连接手柄和自定义节点；dagre 提供自动布局。编辑器不自己实现图形交互基础设施。
- 构建：`npm ci && npm run build` 输出 `web/workflow-editor/dist/`。
- 服务：control-plane 从配置的静态目录（默认 `web/workflow-editor/dist/`）提供 `/workflows/editor/`；SPA 路由回退到 `index.html`。静态目录缺失时，`/workflows/editor/*` 返回 503 并提示构建前端；控制台列表页和 API 不受影响。
- 开发：Vite dev server 以 `/workflows/editor/` 为 base，`/api` 代理到 control-plane。
- Rust workspace 的 `cargo build/test` 不依赖 Node；前端构建在 CI/部署阶段单独执行。控制台的其余部分继续内嵌 HTML，不增加 JavaScript 构建步骤。

### 11.2 AST 与画布的映射

| 画布元素 | AST |
|---|---|
| Start 节点 | `initial_stage` |
| Stage 卡片节点 | `stages[i]`；标题显示 `key`，副标题显示 `active_roles` |
| 卡片内的输入行 | `stages[i].inputs[j]`；显示 signal、guard 摘要、actions |
| 卡片内的子 Task 终止行 | `stages[i].child_terminal[j]`；显示 guard 摘要、actions |
| 卡片内的创建行动作 | 仅初始 stage 的 `created` 行 |
| 边 Stage → Stage | `inputs[j].actions` 中的 `to`，或 `child_terminal[j].actions` 中的 `to`；标签显示 signal 或 “child terminal” + guard 摘要 |
| 边 Stage → End | `actions` 中的 `complete`；标签显示 result |
| 无边的行 | 没有 `to`/`complete` 的转移（例如 `started`、创建下一个迭代）；卡片内显示“停留”图标 |
| 角色面板 | 顶层 `roles` |
| 校验面板 | 服务端校验结果，按 AST 路径定位 |

规则：

- 边完全由 `to`/`complete` 派生；没有独立的边数据，也没有“布局边”或“手工边”。
- 同一对 stage 之间可能有多条边（不同 signal/guard）；React Flow 渲染平行边，边选中后定位到对应行。
- 自 stage 的“停留”转移不画自环；在卡片行内显示，避免画布噪音。
- End 节点是共享的终止标记，不是 stage；`complete` 动作指向它。
- 节点位置只来自布局，不写入 AST。

### 11.3 编辑交互

- **Stage**：工具栏“添加 stage”生成 `stage_1` 等 key，选中后可改 key、编辑 `active_roles`。
- **Role**：角色面板添加/删除 role；删除被 stage 引用的 role 时阻止并定位引用。
- **Input 转移**：卡片内“添加输入”选择或输入 signal，创建带空 actions 的行；可在行内添加 `to`、`create_child`、`complete`，或拖拽阶段手柄创建默认 `signal_1` 的输入转移并打开检查器。
- **Child terminal 转移**：卡片内“添加子 Task 分支”，用结构化 guard 构建器选择 child workflow、result；可添加 `to`/`create_child`/`complete`。
- **Created 动作**：只在初始 stage 显示，可添加 `set_state`、`create_child`、`complete`；不能添加 `to`。
- **检查器**：右侧面板编辑选中 stage/role/transition 的字段；guard 使用结构化构建器（all/any/not + `==`/`!=`/`in`/`not_in`，字段和 `count_children` 选择），child title 使用模板编辑器（字面量 + 字段/整数插值），不提供自由文本表达式编辑器。
- **删除**：删除 stage 前检查引用（`to`、`initial_stage`），有引用时要求先改目标或取消；删除转移行即删除对应 AST 条目；删除边删除其对应的单条转移，不删除同目标的其他转移。
- **排序**：输入转移和 child terminal 分支可上下移动；顺序影响匹配优先级，必须在 UI 中明确提示。
- **Undo/Redo**：本地内存栈（最近 100 次修改），不跨刷新、不服务端持久化；自动保存不产生独立 undo 项。
- **Auto-layout**：dagre 从左到右布局；用户拖动后保存布局；提供“重新自动布局”按钮。
- **只读模式**：平台流程对普通用户、所有已发布版本、以及管理员查看他人账户流程时只读；隐藏编辑控件，保留缩放、版本切换、复制到我的账户（仅平台已发布版本）。

### 11.4 校验面板与发布

- 保存成功后调用校验接口；结果按 error/warning 分组，点击条目定位到 stage/行。
- 发布按钮在存在 error、保存失败、冲突或未保存修改时禁用；禁用原因在按钮旁说明。
- 发布成功后显示版本号；版本下拉可切换到只读历史版本；历史版本不提供编辑，只提供复制（平台）或返回草稿（自己的流程）。
- 校验延迟不影响输入；用递增的本地 token 丢弃过期响应。

### 11.5 版本查看与复制

- 版本列表显示版本号、发布者、发布时间；选择版本加载只读 AST + 定义布局。
- 平台流程的“复制到我的账户”调用 `POST /workflows` 的 `copy_from`；复制成功后跳转到新账户流程的草稿。
- 自己的账户流程在历史版本页只读；返回草稿继续编辑。不能从旧版本开新草稿；要回退，复制旧 body 到当前草稿并发布。
- 版本 diff 是后续增强：前端可获取两个版本 AST 做结构化对比，不需要新 API；本期不做也可以，先保证版本可读和可复制。

## 12. 校验规则

### 12.1 Error（阻止发布）

1. `schema_version != 1`。
2. `stages` 为空；`initial_stage` 为空或不在 `stages` 中。
3. stage key 重复、为空或不符合 `[a-z][a-z0-9_]*`。
4. role key 重复、为空或格式非法；`kind` 未知；`max_actors` 存在时小于 1 或小于 `min_actors`。
5. `active_roles` 引用未声明 role，或重复。
6. 非初始 stage 的 `created` 非空。
7. `set_state` 出现在 `created` 之外；`set_state` 不是 `in_progress`；同一动作列表出现多次。
8. `to` 出现在 `created`；target 不存在；target 等于当前 stage；同一列表出现多次；与 `complete` 同时出现。
9. `complete` 出现多次；不是列表最后一项；与 `create_child` 同时出现。
10. `create_child.workflow` 或 `count_children` 参数为空、格式非法、不在作用域、目标无已发布版本（自身除外）。
11. `create_child.title` 模板语法错误、引用了不可用字段、类型错误。
12. `signal` 为空或格式非法。
13. guard 使用了未知字段/操作符/函数；类型不匹配；在错误的触发上使用 `trigger.child.*`；嵌套超过 20。
14. 同一 signal 的无条件转移后面还有带 guard 的同 signal 转移；child terminal 的无条件条目后面还有条目。
15. AST 超过大小或数量限制。
16. 未知 JSON 字段（防止拼写错误被静默忽略）；结构无法反序列化。

### 12.2 Warning（允许发布）

1. stage 从初始 stage 不可达。
2. stage 声明了 input 但 `active_roles` 为空（合法但无人能发）。
3. 非终止 stage 没有 outgoing `to`/`complete`，也没有 child terminal 分支（可能是刻意等待升级，例如 `attention_required`）。
4. role 声明了但从未在 `active_roles` 中使用（可能是预留）。
5. `min_actors = 0` 且 role 只在输入 stage 使用（提示可能无人可动）。

Warning 不阻止发布，但编辑器和发布响应都应返回；发布方可以确认接受。Error 和 Warning 使用同一套路径格式，便于定位。

### 12.3 运行期检查

即使发布校验通过，运行时仍保留：

- 依赖未全部 `Completed` 时拒绝 `InProgress`/`Complete`；
- 子 Task 未全部 `Completed` 时拒绝 `Complete`；
- `Complete` 是最后一个修改性效果；
- active actor 必须绑定且角色合法；
- `to` 的 stage 必须存在于固定版本；
- 子流程版本解析失败时使事件处理失败，不能创建部分状态的子 Task。

## 13. 与现有实现的关系

### 13.1 对 RFC 0003 的修订

本 RFC 明确取代或修订以下内容：

1. RFC 0003 §1.2、§4.1、§16：workflow 不再只能是受信任的 Rust 代码；本期实现声明式 AST 和可视化编辑器。解释器仍是受信任代码，AST 不能执行任意代码。
2. RFC 0003 §2 不变量 2、§3.1：Task 不只固定 workflow id，而是固定 `(workflow id, workflow version)`。
3. RFC 0003 §4.1：`TaskWorkflow` 的 `'static` 注册表被固定版本的 `DefinitionWorkflow` 取代；`RoleSpec` 改为拥有所有权，`StageDef` 被 `WorkflowStage` 取代，输入不再有 per-signal roles。
4. RFC 0003 §4.2：`ActorBound` 不再作为可编写触发，而是由 stage `active_roles` 隐式激活；`DispatchRequested` 仍是平台唤醒，不是定义触发。
5. RFC 0003 §3.2 / §4.1：stage 的 action/role 声明被 `active_roles` 和 stage-local transitions 取代；没有 per-transition `activate` 列表。
6. RFC 0003 §11.1：新增 workflow 定义/版本/草稿/布局表；Task 增加 `workflow_version`；项目增加 `root_task_id`。
7. RFC 0003 §12.1：`POST /projects` 增加必填 `root_workflow_id`；新增 workflow 管理 API。
8. RFC 0003 §7.1：Root Task 不再硬编码 `workflow = project`、`state = InProgress`、`stage = goal`；项目创建必须选择根流程，Root 的初始状态由根流程的 `created` 动作决定。
9. RFC 0003 §16 风险 14：workflow 升级由版本固定和发布流程解决，不再需要“升级使用新 workflow id”。
10. RFC 0003 §13.2 的旧 Issue 迁移仍适用；如果旧项目/Task 使用 legacy workflow id，迁移必须为这些 id 提供已发布 v1，否则启动失败并报告，不能静默改写。

不改变的部分：

- Task 的 `state`/`stage`/`result`/依赖/调度状态；
- 事件总线、consumer 游标、调度器和事务边界；
- `Complete` 的唯一终止地位、依赖门禁、子 Task 完成门禁；
- 取消策略（仍然没有取消）；
- PM 只能绑定角色成员、不能修改定义；
- `work` 的既有存储字段和可见性规则（但本期没有定义语言/API 写入路径）。

### 13.2 9 个内置流程

9 个内置流程成为平台定义的 v1：

| id | 标题 | 结构 |
|---|---|---|
| `project` | Project lifecycle | Root；创建生命周期子 Task；迭代循环；失败转 `attention_required`。 |
| `goal_clarification` | Goal clarification | `work` → `awaiting_acceptance`；approved 完成，rejected 退回。 |
| `requirements_discovery` | Requirements discovery | 同上，worker = `discoverer`。 |
| `requirements_analysis` | Requirements analysis | 同上，worker = `analyst`。 |
| `architecture_design` | Architecture design | `work` → `review` → `awaiting_acceptance`；review 拒绝退回 work。 |
| `roadmap` | Roadmap | `work` → `awaiting_acceptance`；worker = `planner`。 |
| `iteration` | Iteration | `planning` → `implementation` → `review` → `acceptance`；changes/reject 退回 implementation。 |
| `release` | Release | `candidate` → `verification` → `approval` → `deployment` → `observation`；失败退回 candidate；完成 result = `released`。 |
| `project_review` | Project review | `work` → `awaiting_acceptance`；保留模板，暂未接入 Root。 |

- 内置流程的 Rust `TableWorkflow`/`ProjectWorkflow` 被删除或仅保留为测试夹具；生产路径只使用 JSON v1。
- 在删除 Rust 实现前，用现有实现的输出生成 golden fixtures，覆盖 `created`、`ActorBound`、各 input、child terminal、complete、iteration 编号和失败分支；新解释器必须逐项一致。
- `project` 的 stage-local child terminal 分支按 stage 复制失败兜底；内置 Root 每个 stage 只创建一个对应子 Task，行为与旧全局匹配一致。
- `project` 的初始 stage 从 `created` 的 `set_state` 启动；旧实现硬编码的 `InProgress`/`goal` 不再存在于 `create_root`。
- `iteration` 的标题模板 `Iteration {count_children('iteration') + 1}` 复刻旧的 `children.filter(workflow == iteration).count() + 1`。

### 13.3 代码影响

- 新增 domain 类型：`WorkflowBody`、`WorkflowStage`、`InputTransition`、`ChildTerminalTransition`、`Action`、`Guard`、`TitleTemplate`、`WorkflowVersionRef`。
- 新增 `DefinitionWorkflow` 解释器、AST 校验器和 title template/guard 求值器。
- `WorkflowRegistry` 改为 `WorkflowCatalog`：
  - `pinned(id, version) -> Arc<PinnedWorkflow>`，其中包含 body 和定义所有者；
  - `latest_published(id) -> Option<WorkflowVersionRef>`；
  - `resolve_child(owner_scope, id) -> WorkflowVersionRef`。
  固定版本可永久缓存；最新版本解析走数据库。
- `TaskDAO`/`Task` 增加 `workflow_version`；`TaskRepository::insert` 写入该列。
- `TaskService` 的视图改为从固定版本读取 `role_specs`，并把 `allowed_inputs` 定义为“当前 stage 声明的 signal 集合，且查看者是 active actor”；不再按 per-signal roles 过滤。
- `runtime::authorize_input` 改为校验“发送者是 active actor，且 signal 出现在当前 stage 的输入转移中”；不再使用 per-signal roles。
- `create_root` 改为“由项目创建用例传入根流程 id 和版本”，不再硬编码 `project`/`goal`/`InProgress`。
- `PostgresTaskDispatcher` 在应用 `CreateChild` 时解析并写入子 Task 版本；解析和插入在同一事务。
- `ProjectService::create` 与 `TaskService::create_root` 合并为一个原子用例，或由新的项目创建服务协调；仓库层提供 `create_project_with_root` 事务方法。
- 新增 `WorkflowService`（定义、草稿、版本、发布、布局、校验）和对应的 repository ports。
- API 路由新增 workflow 管理；`create_project` 请求/表单增加根流程。
- 控制台新增流程列表和项目根流程选择器；SPA 静态服务新增。

### 13.4 现有数据迁移

1. 应用 `0007_workflow_definitions.sql`：创建定义/版本/草稿/布局表；给 `tasks` 加 `workflow_version default 1` 和 `not valid` FK；给 `projects` 加 `root_task_id`、延迟 FK 和唯一索引。
2. 启动引导播种 9 个平台 v1；校验已有 v1 与内置 JSON 一致。
3. `validate constraint tasks_workflow_version_fk`；未知 workflow id 使启动失败并输出清单。
4. 回填 `projects.root_task_id`；没有 Root Task 的项目进入迁移报告。
5. 现有 Task 不改状态、stage、事件或 consumer 游标；它们只是从“按 id 查 Rust 注册表”变为“按 `(id, 1)` 查平台 v1”。
6. 现有项目不重新初始化 Root，不重新发送 `TaskCreated`。

## 14. 实施计划

### 阶段 0：评审与冻结

- 评审本 RFC，确认 AST schema、作用域、版本固定和编辑器范围。
- 确认 `workflow_version` 的持久化形式、内置 v1 播种方式和现有数据迁移报告。
- 冻结 AST v1；后续变更必须通过新 schema 版本，不能静默改字段含义。

### 阶段 1：解释器与内置定义

- 实现 AST 类型、guard/title template 求值、校验器。
- 实现 `DefinitionWorkflow`，保持 `WorkflowContext`/`Effect` 边界。
- 把 9 个内置流程写成 JSON v1。
- 用旧 Rust 实现生成 golden fixtures，做 parity 测试；删除生产路径的 `TableWorkflow`/`ProjectWorkflow`。
- 覆盖：created 初始化、ActorBound 激活、input 授权/未声明/guard 不匹配、child terminal 分支、complete、iteration 编号、release 结果。

### 阶段 2：持久化与版本固定

- 迁移 `0007`；启动播种与 v1 一致性校验；FK `validate`。
- `WorkflowCatalog`、固定版本加载、子流程版本解析。
- `TaskDAO`/`Task` 增加 `workflow_version`。
- 项目创建原子用例：根流程选择、最新版本解析、项目 + Root Task + `TaskCreated`。
- 迁移现有 Task/项目；更新 Postgres/内存 adapter 和测试。
- 验收：现有 Task 按 v1 运行；新项目根流程固定；子流程版本按定义所有者作用域解析。

### 阶段 3：管理 API 与控制台

- `WorkflowService`：列表、读取、创建、复制、重命名、草稿、校验、发布、布局。
- 权限：人类写、Agent 读、账户私有、管理员平台管理。
- API 错误码和 `If-Match`/`client_save_id` 语义。
- 控制台“流程”列表、“新建/复制”表单、项目根流程选择器。
- MCP 只读工具；CLI 查看/发布/项目创建。
- 测试：权限矩阵、跨账户引用拒绝、根流程候选、未发布错误、原子创建。

### 阶段 4：SPA 编辑器

- 搭建 `web/workflow-editor/`：Vite + React + TypeScript + React Flow + dagre。
- 静态服务 `/workflows/editor/` 和 SPA fallback。
- Stage 卡片、Start/End、派生边、角色面板、检查器、结构化 guard 编辑器、title template 编辑器。
- 自动保存：防抖、最大等待、单请求、重试、幂等 id、keepalive、localStorage、beforeunload。
- 冲突横幅（载入/覆盖）、发布流程、校验面板、布局保存、自动布局。
- 只读版本页、复制到我的账户。
- 前端测试（组件/交互）和端到端测试（创建 → 编辑 → 保存 → 冲突 → 发布 → 项目创建）。

### 阶段 5：完善与运维

- 版本 diff、undo/redo 打磨、快捷键、可访问性。
- 管理员平台流程创建/发布界面。
- 定义配额、metrics（草稿保存延迟、发布失败、固定版本缓存命中）。
- 文档：AST 参考、编辑器使用说明、迁移 runbook、内置流程说明。
- 端到端：9 个内置流程的项目生命周期、迭代 1/2、失败分支、账户私有流程、平台复制、管理员发布新版本后 Root 不受影响、子 Iteration 使用新版本。

质量门禁沿用仓库标准：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`；Postgres 集成测试在无 `TEST_DATABASE_URL` 时跳过。前端增加 `npm run lint`、`npm run test`、`npm run build`（在 CI/前端阶段执行，不进入 cargo 测试）。

## 15. 验收标准

1. workflow 定义以 AST 为唯一真相；流程图完全由 AST 派生，编辑流程图产生 AST 修改，不存在独立的图数据或双向同步。
2. 9 个内置流程以平台 v1 存在，行为与旧 Rust 实现逐项一致；旧实现生成 golden fixtures 并通过 parity 测试。
3. 项目创建必须提交 `root_workflow_id`；控制台默认选中 `project`，服务端不提供隐藏默认。
4. 项目行、Root Task 行和 `TaskCreated` 事件在同一事务创建；不存在 Provisioning 状态。
5. Root Task 固定创建时的根流程版本；之后发布新版本不改变 Root 的行为。
6. 子 Task 固定创建时对定义所有者作用域可见的最新已发布版本；`iteration` 平台 v2 发布后，新的 Iteration 子 Task 固定 v2，旧 Iteration 和 Root 不受影响。
7. 平台定义只能引用平台定义；账户定义只能引用平台和同账户定义；跨账户引用在发布时返回 422。
8. 账户流程只对所有者账户和平台管理员可见；其他账户的列表、详情、项目候选和运行时解析均不可见。
9. 平台管理员可以读取账户流程，但不能编辑或发布它；账户所有者可以编辑/发布自己的流程。
10. 平台流程对所有已认证账户可见；普通用户只读，管理员可编辑/发布；管理员可以创建新的平台流程。
11. workflow id 全局唯一且不可变；标题可重复；不存在覆盖、同 id 或项目级定义。
12. 已发布版本不可变；定义和版本不删除；每次发布版本号递增。
13. 从平台复制生成新的账户 id 和独立草稿；源平台流程更新不传播，副本发布不影响平台。
14. 只有人类账户可以创建、编辑、发布、复制定义；Agent 账户写操作返回 403，MCP 没有写工具。
15. 定义语言只有 `created`、`input`、`child_terminal` 触发；`actor_bound` 由 `active_roles` 隐式激活；`dispatch_requested` 不可编写。
16. `active_roles` 进入 stage 时激活所有绑定成员；绑定新成员时如果其 role 在当前 `active_roles` 中则重新计算；定义不能指定具体账户或 `work`。
17. 输入授权是 stage 级：active actor 可以发送当前 stage 声明的任意 signal；未声明 signal 返回 `InvalidSignal`；所有 guard 不匹配时输入被接受但无效果。
18. `to` 原子地设置 stage 和激活目标 stage 的 `active_roles`；`complete` 原子设置终止状态、result、completed_at、冻结 stage、清空 active actors。
19. `complete` 必须是动作列表最后一项；与 `create_child` 同列表、或有未完成子 Task 时被拒绝。
20. `create_child` 的标题模板支持 `count_children('iteration') + 1` 并生成正确的迭代编号；生成标题为空或超长时发布校验拒绝。
21. 草稿每个定义一份，带 revision；保存要求 `If-Match` 和 `client_save_id`；重复的保存 id 不重复应用。
22. 自动保存在 600ms 防抖、5s 最大等待、单请求、指数退避下工作；页面隐藏时尝试 keepalive；崩溃后从 localStorage 恢复。
23. 冲突返回 409；编辑器显示载入/覆盖选项，不自动合并；发布也推进 revision。
24. 发布前完整校验；error 阻止发布并返回路径；warning 允许发布并返回；校验接口无副作用。
25. 布局独立保存、last-write-wins、不进入版本；发布不需要保存布局。
26. 编辑器是独立 SPA，静态资源挂载在 `/workflows/editor/`；控制台其余页面保持服务端渲染、无 JavaScript。
27. Stage 卡片显示 `active_roles` 和输入/子 Task 终止行；边由 `to` 派生；`complete` 指向 End；“停留”转移不画自环。
28. 校验面板可定位错误到 stage/转移行；发布按钮在 error、冲突或未保存时禁用并说明原因。
29. 现有 Task 迁移后固定 v1，状态/stage/事件/游标不变；现有项目不重新初始化 Root，不重新发送 `TaskCreated`。
30. 项目创建、Task 运行时、事件总线和调度器的既有测试全部通过；Postgres 集成测试在无 `TEST_DATABASE_URL` 时干净跳过。
31. 平台流程未发布版本不可作为项目根流程；账户流程未发布版本不可作为根流程或子流程引用。
32. 账户停用后，已有项目和 Task 继续按固定版本运行；其他账户不能使用其流程；管理员仍可读取。

## 16. 风险与待决问题

1. **AST 表达力与 per-signal 授权**：v1 的输入授权是 stage 级，不能在一个 stage 内为不同 signal 指定不同角色。内置流程不受影响；如果真实流程需要更细授权，必须拆 stage 或引入新 schema 版本，不能偷偷在 guard 里模拟。
2. **子 Task 依赖缺失**：AST v1 的 `create_child` 不暴露 dependencies。需要定义表达式来引用同一事件中先创建的子 Task id；在出现明确流程需求前不引入。
3. **没有实时协作**：共享草稿 + revision 只能检测冲突，不能合并。多标签页/多人编辑需要显式载入或覆盖；如果协作成为高频需求，再设计分支/合并或实时协同，不能在本期用自动合并掩盖。
4. **草稿和版本存储增长**：定义不删除、草稿每次保存只保留最新 body，版本永久保留。大量账户流程和频繁发布会增长；后续可增加账户配额和归档策略，但不能删除已发布版本。
5. **内置 v1 不可变**：代码发布不能静默修改已播种的 v1；必须发布新版本或写数据迁移。这会让修复内置流程多一步，但保证旧 Task 可重现。
6. **定义质量**：AST 允许自引用和相互引用，可能产生很深的 Task 树；本期靠人类编写和发布校验，未设递归深度/子 Task 配额。平台后续应增加账户级配额和运行时保护。
7. **编辑器性能**：React Flow 在数百节点内表现良好；v1 限制 200 stage。超大定义的卡片内行数和边数量仍可能影响体验；需要折叠、过滤和分组。
8. **布局跨版本共享**：stage 重命名后旧坐标可能失效，缺失 stage 自动布局，未知坐标忽略。这是可接受的展示降级；如果要求“每个版本固定布局”，需要把布局纳入版本快照，但这会破坏“布局不产生版本”的简化。
9. **`work` 字段遗留**：Task 模型仍有 `TaskActorDAO.work`，但本期没有定义/API 写入路径；旧数据可能保留。是否移除该列、如何迁移历史 `work`，需要单独决定；本期只保证 workflow 激活不写 `work`。
10. **平台管理员读取账户流程**：管理员可读所有账户流程，这是运维/审核需要，但也意味着账户流程不是对平台完全不可见。若需要更强的租户隔离，应改为管理员仅在明确授权下读取；本期按“所有者 + 管理员”设计。
11. **账户所有权转移**：本期不支持 workflow 转让或账户合并。账户停用后流程保留但不可编辑；如果业务需要转移，必须设计显式转让操作和审计，不能直接改 `owner_account_id`。
12. **旧 legacy workflow**：迁移要求所有现有 Task 的 workflow id 都有平台 v1；如果部署中存在 RFC 0003 的 `legacy_project_v1`/`legacy_issue_v1` 等 id，必须一并提供 v1 或先完成旧数据迁移，否则启动会拒绝。迁移报告必须列出未知 id，不能静默丢弃 Task。
13. **发布并发**：发布通过草稿行锁串行化；同一账户多人同时发布时，后到者拿到 revision 冲突，需要重新载入。没有自动重试或合并，这是有意的安全选择。
14. **编辑器静态资源部署**：SPA 构建产物不是 cargo 构建的一部分；部署必须包含 `dist/`，否则 `/workflows/editor/` 503。CI 需要同时构建 Rust 和前端；本地开发者可以用 Vite dev server。若希望单二进制自包含，后续可改为嵌入构建产物，但会增加构建耦合。
15. **title 重命名与版本**：标题不属于版本，重命名不会让旧版本显示旧标题。若审计需要“发布时的标题快照”，需要在版本快照中增加 title 字段；本期不做。
