# RFC 0006：Workflow 变量、Operation、Event 与转移

- 状态：Draft，设计待评审
- 日期：2026-11-05
- 关联：[RFC 0003 Task Workflow 模型](0003-task-workflow-model.md)、[RFC 0005 Workflow 定义、版本与可视化编辑器](0005-workflow-definitions.md)、[RFC 0007 Workflow 脚本语法](0007-workflow-script-syntax.md)
- 取代：RFC 0005 中“input signal 直接携带 guard 和 action”的机制（其中 `action` 指旧的转移动作）由本 RFC 取代。RFC 0005 的角色、版本、发布、编辑器与权限边界保持不变。
- 不取代：仍然没有定时器、per-actor `work` 或内建会签/阈值聚合；script/表达式语言的语法和能力边界由 [RFC 0007](0007-workflow-script-syntax.md) 定义。
- 脚本：operation 与 event 的 action、transition 的 `on` 和可选 `action` 都是同一种语言的文本；语法见 [RFC 0007](0007-workflow-script-syntax.md)。

## 0. 结论

新的机制只有一句话：

> **触发器执行 action；action 之后按声明顺序求值 transition 的 `on` 表达式，第一条为 true 的 transition 进入 `to` 指定的 stage。**

触发器有两类：

- **operation**：人类或智能体发起的一次命名操作。
- **event**：系统内发生的事情，例如 task 创建、task state 变化、子任务创建、子任务 state 变化。

两者都执行 action。action 可以修改 task 变量和当前 stage 的变量；action 执行完后，按声明顺序求值该 stage 的 transitions。不再有 changed 集合：`on` 表达式为 true 就转移，不要求本次 action 修改了变量。transition 可以带一段可选 action，在 `on` 匹配后、进入 `to` 之前执行；transition 不能创建子任务，也不能直接完成任务。

处理管线：

```text
trigger (operation | event)
  → 执行 action
  → 按声明顺序求值 transitions
       first `on` 为 true → 执行该 transition 的可选 action → 进入 `to`
  → 至多执行一条 transition
  → 目标 stage：重置 stage 变量（task 变量保留）、激活 role、执行 created effects
  → 目标为虚拟 stage `ended`：任务完成，结果为保留变量 result 的值
```

“全员确认”因此只需要一个 operation 和一条 transition。下面是一个 task workflow 的 `review` stage 定义（不是整个 task，也不是整个 workflow）：

```json
{
  "name": "review",
  "active_roles": ["reviewer"],
  "vars": {
    "approved": { "type": "actor_set", "display_name": "已批准" },
    "changes_requested": { "type": "bool", "display_name": "要求修改" }
  },
  "operations": [
    {
      "name": "review_approved",
      "display_name": "批准",
      "description": "确认当前实现可以进入验收。所有 reviewer 都批准后，任务进入验收。",
      "roles": ["reviewer"],
      "action": "add event.actor to approved"
    },
    {
      "name": "changes_requested",
      "display_name": "要求修改",
      "description": "当前实现需要修改，任务退回实现阶段。",
      "roles": ["reviewer"],
      "action": "changes_requested = true"
    }
  ],
  "events": [],
  "transitions": [
    {
      "on": "changes_requested",
      "to": "implementation"
    },
    {
      "on": "approved ⊇ active_actors('reviewer')",
      "to": "acceptance"
    }
  ]
}
```

`on` 和 `action` 的语法见 [RFC 0007](0007-workflow-script-syntax.md)。两个 action 分别是：

- `review_approved`：把本次 operation 的发送者（`event.actor`）加入 `approved`。
- `changes_requested`：把 `changes_requested` 置为 `true`。

转移在 action 执行完之后求值，因此：

- 不需要 `union` 把发送者补进判断；`on` 读到的已经是更新后的集合。
- 不需要两条监听同一个 operation 的转移，也不依赖 first-match 的顺序技巧。
- 重复发送同一个 operation 时，只要 `on` 仍为 false，就不会转移。

核心决定：

1. **变量有两个作用域**。task 变量在 workflow 上声明，跨 stage 保留；stage 变量在 stage 上声明，进入 stage 时按声明（含 `initial`）重置，离开时丢弃。
2. **变量类型为 `bool`、`string`、`int`、`actor_set`**。没有集合字面量、对象或数组。
3. **触发器执行 action，action 是修改变量的唯一方式**。operation 和 event 都携带 `action`；不再有独立的 `updates` 结构。
4. **action 与 `on` 使用同一套脚本语言**，但 `on` 只能是表达式（无副作用），action 是语句脚本。transition 的可选 `action` 也是语句脚本。语法见 [RFC 0007](0007-workflow-script-syntax.md)。
5. **operation 是给人或智能体看的操作**。每个 operation 有 `name`、`display_name`、`description` 和 `roles`；后两者会展示给人类或智能体，用于判断是否发送该 operation。
6. **operation 声明允许的 role**。`roles` 必填且非空，必须是当前 stage `active_roles` 的子集；发送者必须是持有其中一个 active role 的 actor。
7. **event 是系统内发生的事情**。stage 通过 `events` 声明监听哪些事件，每个事件对应一个 action；事件没有 role，也没有条件。
8. **transition 是 `on`、可选 `action` 和 `to`**。`on` 是文本 boolean 表达式，触发器 action 执行完后按声明顺序求值；第一条为 true 的 transition 先执行它的 `action`（若有），再进入 `to`。
9. **没有 changed 集合**。转移的触发条件就是 `on` 表达式本身；action 没有改变量时也会求值。
10. **transition 的 action 可以修改变量，也可以执行受控副作用（如 `create_issue`）**。它没有 effects 列表，不能改变 stage、不能 `create_child` 或完成任务；完成只由 `to = "ended"` 触发。
11. **创建子任务属于 stage 进入效果**。目标 stage 的 `created` effects 在进入时执行；`create_child` 只出现在这里。
12. **完成用虚拟 stage `ended` 表示**。`to` 为 `"ended"` 时任务完成，任务不会停在一个名为 `ended` 的 stage 上；完成结果来自保留的 task 变量 `result`。
13. **`result` 是内置的 task 级 string 变量**。不需要声明，可与 task 变量一起读写；`to = "ended"` 时如果它非空，就写入 `task.result`，否则 `task.result` 为 null。
14. **`on` 是纯状态查询**。可以读 task 变量、当前 stage 变量、`active_actors`、`count_children`、`task.state`、`task.title`；不能读 operation 名称或 event 数据。
15. **进入 stage 重置 stage 变量并执行 created effects**。task 变量保留。`to` 可以指向当前 stage（自转移），每次进入都按新进入处理。
16. **每个触发器至多执行一条 transition**。执行后不再求值，避免隐式循环。transition 的 action 也不再次触发 transition 求值。
17. **变量持久化在 `tasks.stage_variables` 和 `tasks.task_variables`**，不进入 Task API；审计仍由事件日志提供。
18. **直接改写现有 AST，不切换 schema 版本**。`schema_version` 保持 1；旧形状的定义全部重写。
19. **不内建会签**。“全员确认”由定义表达，编辑器以模板呈现。

## 1. 目标与非目标

### 1.1 目标

- 把人类操作和智能体操作统一为 operation，并让 operation 只负责执行 action，不把“收到什么”和“因此去哪里”耦合在一起。
- 让 operation 携带展示信息（`display_name`、`description`），人类和智能体能据此选择操作。
- 让 operation 只对指定的 role 开放，而不是对 stage 的全部 active role 开放。
- 让定义可以响应系统事件：task 创建、task state 变化、直接子任务创建、直接子任务 state 变化。
- 让“全部 reviewer 通过”“某个事实已经发生”这类跨事件条件可以由定义表达。
- 保持定义是纯数据、解释器确定性、可静态校验。
- 让编辑器把 operation、event、变量、stage 进入效果和转移分开编辑，并把“全员确认”作为模板。
- 保持现有角色、发布和版本固定模型；授权从 stage 级细化到 operation 的 `roles`。

### 1.2 非目标

- 本 RFC 不定义 action 和 `on` 的语法；它们由 [RFC 0007](0007-workflow-script-syntax.md) 定义。本 RFC 只定义它们在 workflow 中的位置、执行上下文和作用。
- 内建会签、阈值、法定人数、加权投票。
- `remove`、`clear`、`increment`、集合字面量。
- 由 transition 引起的新一轮转移。
- `on` 表达式读取 operation 名称、发送者或 event 数据。
- 在 Task API 中暴露变量值。
- operation 参数。operation 只有名称；需要的数据由 action 从变量和 event 中读取。
- event 过滤条件或 per-event 的 role；事件由系统触发，action 自己判断是否处理。
- per-actor `work`、超时、SLA、取消、自动升级。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| operation | 人类或智能体对 task 发起的一次命名操作。stage 声明它，它作为触发器执行 action。 |
| event | 系统内发生的事情。stage 声明监听哪些事件，每个事件对应一个 action。 |
| action | operation、event 或 transition 执行的脚本。语法见 RFC 0007。 |
| `on` | transition 的文本 boolean 表达式。与 action 使用同一套语言，但没有副作用。语法见 RFC 0007。 |
| trigger | 一次 operation，或一次 event。 |
| transition | `{ on, action, to }`：`on` 为 true 时执行 `action`，再进入 `to` 指定的 stage 或 `ended`。 |
| 进入效果 | stage 的 `created` 列表，在进入该 stage 时执行，只能 `set_state` 和 `create_child`。 |
| task 变量 | 在 workflow 上声明、跨 stage 保留的变量。 |
| stage 变量 | 在 stage 上声明、进入时重置、离开时丢弃的变量。 |
| `ended` | 虚拟 stage，不是声明的 stage；`to` 指向它时任务完成。 |
| `result` | 内置的 task 级 string 变量；`to = "ended"` 时作为完成结果。 |
| `roles` | operation 允许的 role 集合；必须是当前 stage `active_roles` 的非空子集。 |
| `actor_set` | 账户 id 的集合，元素为 `account_id`，自动去重。 |
| event actor | 发送 operation 的账户；来自登录会话，不是请求体字段。 |
| active actors | 当前 stage 中 `active = true` 的角色成员。 |

不变量：

1. 触发器只通过 action 修改 task 变量和当前 stage 变量，或执行受控副作用（如 `create_issue`）；不直接改变 stage、不完成任务、不创建子任务。
2. transition 只执行自己的 action 并进入 `to`；action 可以修改变量或执行受控副作用，但不能改变 stage、创建子任务或完成任务。`to = "ended"` 是唯一的完成方式。
3. 每个触发器执行完 action 后，按声明顺序求值当前 stage 的 transitions；第一条 `on` 为 true 的 transition 执行其 action 并进入 `to`，至多一条。
4. `on` 必须是 boolean 表达式，且没有副作用。
5. stage 变量只属于当前 stage，其他 stage 不能读写；task 变量在整个 task 生命周期内可读写。
6. 进入 stage 时 stage 变量按声明重置（包括 `initial`）；离开时全部丢弃。task 变量保留；自转移也按进入处理。
7. `on` 读取 action 执行完之后的变量状态和 Task 快照。
8. 定义不能指定账户 id；operation 的 action 可得到 `event.actor`，event 的 action 只能得到该事件公开的字段。
9. 解释器确定性：相同定义、Task 快照、变量、active actors 和触发器产生相同效果；action 和 `on` 也必须满足这一点。
10. operation 的发送者必须持有其 `roles` 中列出的一个 active role；stage 只决定哪些 role 处于 active。
11. event 按事件流的顺序逐个处理；一个事件处理时产生的后续事件，作为后续触发器处理，不在当前触发器内嵌套。
12. `ended` 是虚拟 stage：`to = "ended"` 时任务完成，`task.stage` 保持来源 stage；不能声明名为 `ended` 的 stage，也不能把 `ended` 设为 `initial_stage`。

## 3. 定义语言

定义属于 workflow：workflow 声明 roles、task 变量和 stages；每个 stage 声明自己的 `vars`、`operations`、`events`、`created` 和 `transitions`。task 是按某个已发布 workflow 创建的实例，运行期保存当前 stage、task 变量（`tasks.task_variables`）和当前 stage 的变量（`tasks.stage_variables`）。本节以及后面的 JSON 片段，除非特别说明，都是一个 stage 的定义。

### 3.1 Task 变量与 Stage 变量

`WorkflowBody` 增加 task 级 `vars`，`WorkflowStage` 也有 `vars`：

```json
{
  "schema_version": 1,
  "roles": ["reviewer"],
  "vars": {
    "attempts": { "type": "int", "display_name": "尝试次数", "initial": 0 }
  },
  "initial_stage": "review",
  "stages": [
    {
      "name": "review",
      "active_roles": ["reviewer"],
      "vars": {
        "approved": { "type": "actor_set", "display_name": "已批准" },
        "changes_requested": { "type": "bool", "display_name": "要求修改", "initial": false },
        "decision": { "type": "string", "display_name": "结论", "initial": "" }
      },
      "operations": [],
      "events": [],
      "created": [],
      "transitions": []
    }
  ]
}
```

- 两处 `vars` 省略时都是空对象。
- task 变量在 task 创建时初始化，跨 stage 保留；stage 变量在每次进入 stage 时初始化，离开时丢弃。
- 变量名使用与 role/stage 相同的命名规则：`[a-z][a-z0-9_]*`，在同一作用域内唯一。
- 不同 stage 可以有同名变量，互不影响；stage 变量不能与 task 变量同名，否则脚本无法区分作用域，发布校验拒绝。
- map 的 key 是机器名，只用于 `on` 和 action 中的引用；`display_name` 是必填的展示名，非空、trim 后非空、最多 100 个字符。
- `display_name` 展示给人类或智能体，运行时忽略。编辑器、变量选择器和只读定义视图以 `display_name` 为主标签，机器名作为次要提示。
- `initial` 可选；省略时使用类型默认值：
  - `bool`：`false`
  - `string`：`""`
  - `int`：`0`
  - `actor_set`：空集合
- `initial` 的值必须与声明类型一致；`actor_set` 不支持 `initial`。
- `result` 是内置的 task 级 `string` 变量，不需要也不允许在 `vars` 中声明；脚本可直接读写它，初始值为 `""`。
- 所有 action 和 `on` 看到的是 task 变量与当前 stage 变量合并后的作用域；写入按变量声明归属回到对应作用域。

### 3.2 Operations

`WorkflowStage` 增加 `operations`，取代原来的 `inputs`：

```json
{
  "operations": [
    {
      "name": "review_approved",
      "display_name": "批准",
      "description": "确认当前实现可以进入验收。所有 reviewer 都批准后，任务进入验收。",
      "roles": ["reviewer"],
      "action": "add event.actor to approved"
    },
    {
      "name": "changes_requested",
      "display_name": "要求修改",
      "description": "当前实现需要修改，任务退回实现阶段。",
      "roles": ["reviewer"],
      "action": "changes_requested = true"
    },
    {
      "name": "started",
      "display_name": "开始",
      "description": "认领并开始当前阶段的工作。",
      "roles": ["reviewer"],
      "action": ""
    }
  ]
}
```

- `name` 使用 `[a-z][a-z0-9_]*`，在同一个 stage 内必须唯一。
- `display_name` 必填，非空、trim 后非空、最多 100 个字符。
- `description` 必填，非空、trim 后非空、最多 1000 个字符。它说明这个 operation 做什么、什么时候该发送，展示给人类或智能体。
- `roles` 必填、非空；每个 role 必须出现在当前 stage 的 `active_roles` 中。它限定哪些 role 能发送这个 operation。
- `action` 必填，是 operation 执行的脚本文本。空字符串合法，表示这个 operation 不改变量（例如只需要记录“已开始”并让任务从 `pending` 进入 `in_progress`）。
- 只有本 stage 声明的 operation 才能发送；未声明的 operation 在授权阶段拒绝。
- 发送者必须是当前 stage 的 active actor，并且其 active role 在 operation 的 `roles` 中；否则拒绝。
- 一个账户可以同时绑定多个 role；只要其中任一 active role 在 `roles` 中即可发送。
- `event.actor` 只在这里的 action 中可用。
- operation 没有参数；请求体只有 operation 名称。action 需要的其他数据来自变量和 `event.actor`。
- operation 的 action 不能改变 stage、完成任务或创建子任务；改变 stage 只属于 transition，创建子任务只属于 stage 进入，完成只属于 `to = "ended"`。

### 3.3 Events

`WorkflowStage` 增加 `events`，用来监听系统事件：

```json
{
  "vars": {
    "child_done": { "type": "bool", "display_name": "子任务完成" }
  },
  "events": [
    {
      "event": "child.state_changed",
      "action": "if event.child.state == 'completed' { child_done = true }"
    }
  ]
}
```

事件表：

| `event` | 触发时机 | action 可读的字段 |
|---|---|---|
| `task.created` | 当前 task 创建完成、初始 stage 和 `created` effects 生效后。 | 无额外字段；action 读取 task 和变量上下文。 |
| `task.state_changed` | 当前 task 的 state 发生变化；新 state 为 `completed` 时不处理。 | `event.state`：新 state。 |
| `child.created` | 直接子任务创建后。 | `event.child.id`、`event.child.workflow`、`event.child.title`。 |
| `child.state_changed` | 直接子任务的 state 发生变化，包括进入 `completed`。 | `event.child.id`、`event.child.workflow`、`event.child.state`、`event.child.result`。 |

- `event` 必须来自事件表；发布校验拒绝未知事件名。
- 同一个 stage 内同一个 `event` 只能声明一次；需要多个处理逻辑时，合并到一个 action 里。
- event 没有条件表达式，也没有 `roles`。事件到达时，如果是否处理取决于事件内容，由 action 自己判断；不处理时不改变量。
- event 的 action 与 operation 的 action 有相同限制：可以读写 task 变量和当前 stage 的变量，也可以执行受控副作用（如 `create_issue`），但不能改变 stage、完成任务或创建子任务。
- 事件按 task 的事件流顺序逐个处理；处理一个事件时产生的后续事件作为后续触发器处理，不在当前触发器内嵌套。
- `task.created` 只可能由初始 stage 处理；其他 stage 不会再收到这个事件。
- `child.created` 和 `child.state_changed` 只针对直接子任务；孙任务的事件不会冒泡到当前 task。
- `event.child.result` 只在子任务进入 `completed` 且该任务有 result 时有值；其他 state 或没有 result 时为 null。
- 平台目前没有删除 task 的操作，因此事件表没有 `deleted` 事件。引入删除能力时再增加 `task.deleted` / `child.deleted`。

### 3.4 action 与 `on`

`action` 和 `on` 使用同一套脚本语言，但形式不同：

- `action` 是语句脚本：可以赋值、读写变量和使用条件分支，但没有循环。
- `on` 是表达式：只能求值，必须返回 boolean，不能赋值、不能有副作用。

本节固定它们与 workflow 其余部分的关系；完整语法、类型和校验见 [RFC 0007](0007-workflow-script-syntax.md)。

已确定的部分：

- 执行时机：
  - operation 通过授权后执行它的 action；
  - event 被投递且当前 stage 监听了该事件时执行它的 action；
  - transition 的 `on` 为 true 后、进入 `to` 之前执行它的 `action`（若有）；
  - 触发器 action 执行完后，按 3.5 求值 `on`。
- action 的执行上下文：
  - task 变量与当前 stage 变量（读写），以及内置 `result`；
  - `active_actors(role)`、`count_children(workflow)`、`task.state`、`task.title`（只读）；
  - operation 的 action 可以读 `event.actor`；
  - event 的 action 可以读该事件公开的字段，见 3.3 的事件表；不能读 `event.actor`。
- `on` 及 transition 的 action 的求值上下文：
  - task 变量与当前 stage 变量（`on` 只读；transition action 可写），以及内置 `result`；
  - `active_actors(role)`；
  - `count_children(workflow)`；
  - `task.state`、`task.title`；
  - 字面量；
  - 不能读任何 `event.*`。
- 执行效果：action 可以读写变量，也可以执行受控副作用（目前是 `create_issue`）。stage 转移、创建子任务、完成任务仍然只能由 transition 和 stage 进入效果完成。
- 空 action：合法，不改变量。action 执行完后仍然求值 `on`；如果 `on` 已经为 true，转移仍会发生。transition 的空 `action` 同样是合法的。
- 确定性：action 和 `on` 不得读时钟、随机数或做 I/O；相同输入必须得到相同结果。
- 发布校验：按 RFC 0007 执行语法、标识符、类型、上下文引用和限制检查；`on` 不允许空字符串。

### 3.5 Transitions

`WorkflowStage` 增加 `transitions`：

```json
{
  "transitions": [
    { "on": "changes_requested", "action": "", "to": "implementation" },
    { "on": "approved ⊇ active_actors('reviewer')", "action": "", "to": "acceptance" },
    { "on": "decision == 'approved'", "action": "result = 'approved'", "to": "ended" }
  ]
}
```

- 每个 transition 有 `on`、可选 `action` 和 `to`。
- `on` 是必填的非空文本 boolean 表达式，使用 3.4 的表达式语言。触发器 action 执行完后按声明顺序求值。
- `action` 可选；省略或空字符串表示不执行脚本。它在 `on` 匹配后、进入 `to` 之前执行，可以读写变量，但不能读 `event.*`。
- `to` 是必填的 stage 名或虚拟 stage `"ended"`；目标可以是任意已声明的 stage、当前 stage 自己或 `ended`。
- 第一条 `on` 为 true 的 transition 执行其 `action` 并进入 `to`，后面的不再求值。
- 没有任何 transition 的 `on` 为 true 时，stage 不变；变量保留。
- transition 的 action 可以修改变量或执行受控副作用。没有 effects 列表，不改变 stage、不创建子任务，也不直接完成任务。
- 进入已声明的目标 stage 时：
  - stage 变量按目标 stage 的声明重置，task 变量保留；
  - active roles 按目标 stage 的 `active_roles` 重新激活；
  - 执行目标 stage 的 `created` effects。
- `to` 为 `"ended"` 时任务完成：
  - task 状态设置为 `completed`；
  - `task.stage` 保持来源 stage，不会变成 `ended`；
  - `result` 变量非空时写入 `task.result`，否则 `task.result` 为 null；
  - 不进入任何 stage，也不再执行 `created`。
- 进入 stage 本身不再求值 transition；一次触发器只执行一条 transition。下一个触发器才会再次求值。
- `to` 指向当前 stage 是合法的自转移；自转移按一次新的进入处理，stage 变量重置、`created` 重新执行，task 变量保留。它用于“在同一个 stage 内反复创建子任务”这类循环。
- 同一个 stage 内 transitions 的顺序有意义：把更具体的条件放在前面。

### 3.6 Stage 进入与完成

**进入效果**：

每个 stage 都可以声明 `created`：

```json
{
  "created": [
    { "set_state": "in_progress" },
    { "create_child": { "workflow": "goal_clarification", "title": "Clarify project goals" } }
  ]
}
```

- `created` 在每次进入该 stage 时执行，包括初始进入、`to` 进入和自转移。
- `created` 的 effects 只能是：
  - `{ "set_state": "pending" | "in_progress" }`；
  - `{ "create_child": { "workflow": "<workflow id>", "title": "<title template>" } }`。
- `set_state` 不能设置为 `completed`；完成只能通过 `to = "ended"`。
- `create_child` 的 title 模板只能使用 `task.title` 和 `count_children`；不再有 `trigger.child.*`。
- 进入效果按声明顺序执行；`create_child` 在本次进入中创建子任务。
- 进入效果不能修改变量；变量只能由 action 修改。

**完成与 `ended`**：

完成不是一种 stage，而是 transition 的目标 `"ended"`：

```json
{
  "on": "decision == 'approved'",
  "action": "result = 'approved'",
  "to": "ended"
}
```

- `ended` 是保留的虚拟 stage 名，不能声明同名的 stage，也不能用作 `initial_stage`。
- `to = "ended"` 时任务完成；完成结果来自内置 task 变量 `result`（见 3.1）。`result` 为空字符串时 `task.result` 为 null。
- 完成时 `task.stage` 保持来源 stage；之后不再处理 operation、event 或 transition。
- 不同结果由 transition 的 `action` 写入 `result` 区分，可以指向同一个 `"ended"`。
- `ended` 可以出现在任意数量的 transition 中。

### 3.7 表达式语言

`on` 与 `action` 使用同一套语言，但 `on` 只能是表达式。完整语法、类型规则、错误码和限制见 [RFC 0007](0007-workflow-script-syntax.md)；本节只固定它必须能表达的内容和语义边界。

必须支持：

- 读取 task 变量与当前 stage 变量，变量类型见 3.1。
- 读取 `active_actors(role)`，类型为 `actor_set`；role 必须在当前 stage 的 `active_roles` 中。
- 读取 `count_children(workflow)`，类型为 `int`。
- 读取 `task.state` 和 `task.title`，类型为 `string`。
- 字面量：boolean、整数、字符串、null。
- 布尔运算：`and`、`or`、`not`。
- 比较：`==`、`!=`，以及 `actor_set` 的包含关系 `⊇`（用于全员确认）。
- 括号。

必须保持的限制：

- `on` 必须求值为 boolean；否则发布校验失败。
- `on` 没有副作用，不能赋值，不能调用有副作用的函数。
- `on` 不能读取 `event.actor`、event 字段、operation 名称或触发类型。事件数据只存在于产生它的那次触发的 action 中；action 通过变量把需要的事实留给 `on`。
- 没有集合字面量；`actor_set` 只能来自变量和 `active_actors(role)`。
- 没有集合相等、`size`、`contains`、`union`；这些能力需要时再设计。
- `on` 必须是确定性的，不能读时钟、随机数或做 I/O。

示例：

| 语义 | 写法 |
|---|---|
| 读取 bool 变量 | `changes_requested` |
| 字符串比较 | `decision == 'approved'` |
| 子任务计数 | `count_children('iteration')` |
| 活跃 role 集合 | `active_actors('reviewer')` |
| 集合包含 | `approved ⊇ active_actors('reviewer')` |

**多 role stage**：`active_actors(role)` 只统计该 role 的 active 成员。operation 的 `roles` 决定哪个 active role 能发送它，因此多 role stage 不需要为了限制发送者而拆分 stage。仍需注意目标 role 集合为空的情况：此时 `approved ⊇ 空集` 恒真；如果定义让其他 role 也能发送写入 `approved` 的 operation，`on` 就可能被提前满足。审批 operation 的 `roles` 应只包含需要确认的 role；编辑器模板会按这个规则生成并提示。

### 3.8 全员确认的完整执行过程

使用 0 节的定义，active actors 为 A、B、C。各 operation 的 action 语义：`review_approved` 把发送者加入 `approved`；`changes_requested` 把 `changes_requested` 置为 true。`on` 表达式按 RFC 0007 的语法理解。

1. A 发送 `review_approved`：action 把 A 加入 `approved`；两条 transition 按顺序求值：`changes_requested` 为 false；超集条件为 false；不转移。
2. B 发送：`approved = {A,B}`；仍不满足超集；不转移。
3. C 发送：`approved = {A,B,C}`；第一条 transition 的 `on` 为 false；第二条为 true；进入 `acceptance`。C 不需要被写入集合，因为 stage 即将离开；`OperationReceived` 事件已经记录 C。
4. A 再次发送：`review_approved` 没有在 `acceptance` 声明，按未声明 operation 拒绝；不会影响旧集合。
5. 任意 reviewer 在转移前发送 `changes_requested`：`changes_requested = true`；第一条 transition 为 true；立即进入 `implementation`；`approved` 随 stage 离开被丢弃。
6. 重新进入 `review`：两个变量分别重置为空集合和 `false`，上一轮的票不会带过来。
7. 中途绑定新的 reviewer D：`ActorBound` 只刷新 active actors；不执行 action，也不求值 transition。D 进入 `active_actors('reviewer')`，因此 A、B、C 都通过后仍然不满足超集，直到 D 也发送 `review_approved`。
8. A 重复发送：action 没有改变量；两条 transition 的 `on` 仍然为 false；不转移。

边界：

- 没有超时：如果某个 reviewer 永远不发送，任务停在 `review`。
- 审批总是针对当前版本；不需要记录“批准的是哪一版”。
- 最后一个 reviewer 的 `review_approved` 记录在 `OperationReceived` 事件里，即使没有写入 `approved`，审计仍然完整。
- active actors 的动态变化只影响后续 trigger 的求值；绑定本身不触发转移。

### 3.9 全员确认的补充说明

“全员确认”只是本定义模式，不是新的 AST 节点。它依赖以下事实：operation 的 action 先写入 `approved`，action 执行完后才求值 `on`，表达式读到的已经是更新后的集合。编辑器把它呈现为模板，解释器仍然按普通的 operation/action/transition 执行。

## 4. 运行时语义

### 4.1 Operation 处理

1. 校验 operation 在当前 stage 声明；否则 `InvalidOperation`。
2. 校验发送者是当前 stage 的 active actor，且其 active role 在 operation 的 `roles` 中；否则 `Unauthorized`。
3. 如果 `task.state == pending`，设置为 `in_progress`。这是任务状态，不是 stage 转移，保持现有行为。
4. 执行该 operation 的 action。
5. 按 4.3 求值当前 stage 的 transitions；至多执行一条。
6. 持久化 Task（含变量）、actors、children 和事件。

action 执行失败时的错误分类与事务行为按 RFC 0007 和现有 `TaskError::Workflow` 处理；本 RFC 要求一次触发要么全部生效，要么全部不生效。

### 4.2 Event 处理

1. 找到当前 stage。
2. 按事件名查找该 stage 的 event 处理器；没有处理器则结束。
3. 以事件公开的字段为上下文执行该 action。
4. 按 4.3 求值 transitions。
5. 持久化。

- 事件按 task 的事件流顺序逐个处理。operation 引起的 `in_progress` state 变化、transition 引起的子任务创建或 state 变化，都作为后续事件处理，不会在当前触发器的 action 中间嵌套执行。
- 事件被处理时使用的是当时的 current stage。如果前一个事件已经切换了 stage，后一个事件由新 stage 的 `events` 决定是否处理。
- `task.state_changed` 的新 state 为 `completed` 时不投递处理器：任务已经终止，变量不再变化。
- `task.created` 在初始 stage 进入并执行完 `created` effects 之后处理；处理完后按 4.3 求值一次 transitions。

### 4.3 Transition 求值

按声明顺序遍历当前 stage 的 transitions：

1. 求值 `on` 表达式。上下文是触发器 action 执行完之后的变量、当前 active actors、children、`task.state` 和 `task.title`。
2. 第一条 `on` 为 true 的 transition 执行：
   - 执行 transition 的 `action`（若有）；它可以读写 task 变量和 stage 变量，不能读 `event.*`。
   - 如果 `to` 是 `"ended"`：任务完成；`result` 非空时写入 `task.result`，`task.stage` 保持当前 stage。
   - 否则：设置 stage，重置 stage 变量（task 变量保留），按目标 stage 的 `active_roles` 重新激活 active actors，执行目标 stage 的 `created` effects。
   - 停止遍历。
3. 没有任何 `on` 为 true：stage 不变，停止。

- 一次触发器最多执行一条 transition。
- 执行 transition 后不再重新求值，即使进入的新 stage 中有 `on` 为 true 的 transition。
- `on` 为 true 不要求本次 action 改变过变量。
- `ActorBound` 不是这里定义的 trigger：它只刷新 active actors，不执行 action，也不求值 transition。
- `DispatchRequested` 无操作。

### 4.4 Stage 生命周期

- **创建**：task 变量按声明初始化，进入初始 stage（stage 变量按声明初始化），激活 active roles，执行初始 stage 的 `created` effects；然后处理初始 stage 的 `task.created` 事件（如果有）；最后按 4.3 求值一次 transitions。
- **`to` 进入**：在 4.3 中设置新 stage，stage 变量重置，task 变量保留，激活 active roles，执行目标 stage 的 `created` effects。不再次求值 transitions。
- **自转移**：`to` 等于当前 stage；按一次新的进入处理，stage 变量重置，task 变量保留，`created` 重新执行。
- **完成**：`to = "ended"` 时设置任务完成并写入 `result`；`task.stage` 保持来源 stage；没有后续触发器。
- **`ActorBound`**：只刷新当前 stage 的 active actors；不改变量，不求值 transition。
- **`DispatchRequested`**：无操作。
- **`task.state_changed`**：是新 stage 中可能监听的普通事件；`pending → in_progress` 会触发一次 event 处理和 transition 求值。

### 4.5 变量与事件

- 变量变化不产生新的 DomainEvent。触发器本身的事件（`OperationReceived`、`TaskCreated`、`TaskStateChanged`、`ChildCreated`、`ChildStateChanged`）是审计记录。
- `TaskStageChanged` 仍然记录 stage 变化。
- `to = "ended"` 时记录 `TaskCompleted`，payload 包含 `result`；如果 task 有父任务，同时向父任务写入 `ChildStateChanged`，`state = completed`、`result` 为该结果。
- 变量是 Task 聚合的一部分，和 stage、active actors 一起构成 `on` 的输入快照。
- 变量可以通过事件和定义版本重放重建；持久化列是投影。

### 4.6 错误

- 变量、类型和表达式错误都在发布校验阶段拒绝，运行期不应出现未知变量或类型错误。
- `event.actor` 出现在非 operation 的 action 中、event 字段出现在非对应事件的 action 中，正常路径由校验阻止；运行期返回 `TaskError::Workflow`。
- `active_actors` 引用不存在的 role 时返回 `TaskError::Workflow`；正常路径由校验阻止。
- `to` 引用不存在的 stage 时返回 `TaskError::UnknownStage`；正常路径由校验阻止。
- 表达式/脚本自身的错误由 [RFC 0007](0007-workflow-script-syntax.md) 定义。

## 5. 持久化

### 5.1 数据库

`tasks` 表有两列变量：

```sql
alter table tasks rename column variables to stage_variables;

alter table tasks
    add column if not exists task_variables jsonb not null default '{}'::jsonb;
```

- `stage_variables` 保存当前 stage 的变量；`task_variables` 保存 task 变量（含内置 `result`）。
- 两列的 JSON 形状相同：

```json
{
  "approved": ["<uuid>", "<uuid>"],
  "changes_requested": true,
  "attempt": 2,
  "decision": "approved"
}
```

- 类型由定义中的 `vars` 决定；序列化按值本身的 JSON 类型写入。
- `actor_set` 序列化为 UUID 字符串数组，写入前排序，保证确定性。
- 不建索引：不在 SQL 里按变量查询。
- 变量与 Task 行在同一个事务里更新；stage 变化时 `stage_variables` 写入目标 stage 的初始值，`task_variables` 原样保留。
- 已有行由默认值覆盖；`variables` 直接重命名为 `stage_variables`，不做形状转换。
- 完成结果继续写现有的 `tasks.result` 和 `tasks.completed_at`；`result` 变量是它的唯一来源，不新增结果列。

### 5.2 内存适配器

`TaskDAO` 直接携带变量；clone 即可。聚合加载和保存路径与 Postgres 保持一致。

### 5.3 视图

- 公开的 `Task` 视图不增加变量字段。
- 审批的审计信息来自 Task 视图的 `operations`（操作记录）中的 `OperationReceived`，其中已有 `actor_id` 和 operation 名称。
- `allowed_operations` 是 operation 名称、`display_name` 和 `description` 的列表，按查看者可发送的 operation 过滤，不含变量。
- 如果以后需要“2/3 已确认”的进度展示，再单独设计投影/API，不把内部变量直接暴露出去。

### 5.4 事件

变量变化不产生新的 DomainEvent。`OperationReceived`、`TaskStateChanged`、`ChildCreated`、`ChildStateChanged` 是持久审计记录。变量是 workflow 内部状态，不是新的领域事件。

子任务的 state 变化需要写入父任务的事件流：

- 子任务进入新的 state 时，除自身的 `TaskStateChanged` 外，还向父任务追加 `ChildStateChanged`，payload 包含 `child_id`、`child_workflow`、`child_state` 和 `result`。
- 子任务创建时已经有 `ChildCreated` 写入父任务。
- 原来的 `ChildTerminal` 由 `ChildStateChanged` 取代；`to = completed` 时携带 result。

## 6. 校验

### 6.1 变量

| 错误码 | 条件 |
|---|---|
| `invalid_variable_name` | 变量名不匹配 `[a-z][a-z0-9_]*`。 |
| `duplicate_variable` | 同一作用域（task 或某个 stage）内变量名重复。 |
| `variable_shadows_task_variable` | stage 变量与 task 变量同名。 |
| `invalid_display_name` | `display_name` 缺失、trim 后为空，或超过 100 个字符。 |
| `invalid_variable_type` | 类型不是 `bool`、`string`、`int`、`actor_set`。 |
| `invalid_initial_value` | `initial` 与声明类型不匹配，或 `actor_set` 声明了 `initial`。 |
| `script_reserved_name` | 变量名是 RFC 0007 的保留字（包括内置 `result`）。 |

### 6.2 Operations

| 错误码 | 条件 |
|---|---|
| `invalid_operation` | operation 名不匹配 `[a-z][a-z0-9_]*`。 |
| `duplicate_operation` | 同一 stage 内 operation 名重复。 |
| `invalid_display_name` | 与变量共用：缺失、trim 后为空或超长。 |
| `invalid_description` | `description` 缺失、trim 后为空，或超过 1000 个字符。 |
| `empty_operation_roles` | `roles` 缺失或为空。 |
| `operation_role_not_active` | `roles` 中的 role 不在当前 stage 的 `active_roles` 中。 |
| `invalid_action` | `action` 缺失或不是字符串。空字符串合法。 |

### 6.3 Events

| 错误码 | 条件 |
|---|---|
| `invalid_event` | `event` 缺失，或不在事件表中。 |
| `duplicate_event` | 同一 stage 内重复声明同一个 `event`。 |
| `invalid_action` | `action` 缺失或不是字符串。空字符串合法。 |

### 6.4 Transitions

| 错误码 | 条件 |
|---|---|
| `invalid_transition_on` | `on` 缺失、不是字符串，或 trim 后为空。 |
| `unknown_stage` | `to` 缺失，或引用的 stage 不存在且不是 `ended`。 |
| `invalid_transition_action` | `action` 不是字符串，或脚本校验失败。 |

`on` 和 action 内部的语法、标识符、类型和上下文引用校验由 [RFC 0007](0007-workflow-script-syntax.md) 定义，诊断使用其 `script_*` 错误码。本节仍只定义 transition 自身的结构校验。

### 6.5 Stage 进入与完成

| 错误码 | 条件 |
|---|---|
| `invalid_created_effect` | `created` 中出现 `set_state`、`create_child` 之外的 effect。 |
| `set_state_completed` | `created` 的 `set_state` 试图设置 `completed`。 |
| `unknown_child_workflow` | `create_child` 引用的 workflow 不存在或不可见。 |
| `reserved_stage_name` | stage 名是保留的 `ended`。 |

### 6.6 警告

| 警告 | 条件 |
|---|---|
| `unreachable_stage` | 不是初始 stage，也没有任何 transition 指向它。 |
| `dead_end_stage` | stage 没有任何 transition，也没有指向 `ended` 的完成路径。 |
| `operations_without_actors` | stage 声明了 operation，但没有 active role 能发送。 |
| `optional_role_on_operation_stage` | stage 的 active role 不是必选绑定，可能导致无人能发送 operation。 |
| `unused_variable` | 变量从未被任何 action 或 `on` 读取。 |
| `transition_never_triggered` | 某条 `on` 读取了变量，但这些变量都没有被任何 action 写；task 变量在其他 stage 的写入也算。 |

`transition_never_triggered` 和 `unused_variable` 依赖脚本的读/写变量集合；RFC 0007 要求发布校验提供该静态信息，因此这两个警告可以按 RFC 0007 第 8.3 节的规则实现。

## 7. 编辑器

### 7.1 类型与 API

- `WorkflowBody` 增加 task 级 `vars`；`WorkflowStage` 增加 `vars`、`operations`、`events`、`transitions`；移除 `inputs`、`child_terminal`、`terminal` 和 `result`。
- `VarSpec { type, display_name, initial? }`。
- `OperationSpec { name, display_name, description, roles, action }`。
- `EventSpec { event, action }`。
- `TransitionSpec { on: String, action: String, to: String }`；`to` 是 stage 名或 `ended`；没有 `effects`，没有结构化的 `Guard` 对象。
- `created` 的 effect 只保留 `set_state` 和 `create_child`；删除 `to` 和 `complete` 的 effect 节点。
- 删除现有 `Guard`/`Operand` 的 JSON 结构；`on` 是文本，解析与类型检查属于 [RFC 0007](0007-workflow-script-syntax.md)。
- 删除 `Update` 及 `set`/`add` 的 AST 节点。
- `emptyBody()` 保持 `schema_version: 1`。

### 7.2 Stage 面板

编辑器分两个层面：workflow 级的 **task 变量** 面板，和 stage 检查器的五个区。

Task 变量面板：

- 名称、显示名、类型（bool/string/int/actor_set）、初始值；添加、重命名、删除。与 stage 变量共用同一个编辑组件。
- 初始值按类型编辑：`bool` 用开关，`int`/`string` 用输入框，`actor_set` 不提供初始值。修改类型时清空初始值。
- `result` 是内置变量，不在面板中列出。

stage 检查器分成五个区：

1. **stage 变量**：名称、显示名、类型、初始值，添加、重命名、删除。名称不能与 task 变量重名。新建时显示名默认填入变量名，可修改。删除被引用的变量时提示受影响的 `on` 表达式和 action。
2. **Operations**：operation 列表；每条编辑 `name`、`display_name`、`description`、`roles`（当前 stage 的 `active_roles` 多选，至少一个）和 `action`。`action` 使用文本编辑器，按 RFC 0007 提供高亮、补全和校验。不再有 update 编辑器。
3. **Events**：事件列表；每条从事件表选择 `event`，并编辑 `action`。同一个事件只能出现一次；事件表之外的名字不能保存。不再有 child terminal 面板。
4. **进入效果**：`created` 列表，只能添加 `set_state` 和 `create_child`。所有 stage 都可编辑。`create_child` 选择 workflow 并填写 title 模板。
5. **Transitions**：有序列表，每条包含 `on` 文本表达式、可选 `action` 文本脚本和 `to`（stage 下拉，包含当前 stage 和 `ended`）。不再有效果列表，也不再使用结构化 guard 编辑器。

### 7.3 完成与 `ended`

- 从 stage 拖到画布右侧的“结束”节点，创建一条 `{ on: "false", action: "", to: "ended" }` 的 transition；用户再填写条件和结果写入。
- 所有 stage 都可以声明指向 `ended` 的 transition；`ended` 不是可编辑的 stage，也不显示为普通节点。
- 完成结果不在 stage 上编辑，而是由 transition 的 action 写入 `result`。
- 画布上把 `to = "ended"` 的边画到“结束”节点。

### 7.4 `on` 表达式编辑

- `on` 是单行或多行文本编辑器；语法与 action 相同，因此可以共用高亮、补全和错误展示。
- 按 RFC 0007 校验 `on` 非空、是表达式且顶层为 boolean。
- 补全变量（按类型）、`active_actors`、`count_children`、`task.state`、`task.title` 和字面量；禁止 event 字段和 operation 名称。
- 不再提供 `trigger.child.*`；event 字段只出现在 event action 的编辑器中。
- 校验面板展示 `on` 的行列错误。

### 7.5 全员确认模板

- 入口：stage 面板的“添加全员确认”。
- 参数：role（当前 stage 的 active role 之一）、approval operation 名称、目标 stage。
- 生成：
  1. 变量 `approved: actor_set`，`display_name` 为“已批准”（名称冲突时两者一起加后缀）；
  2. operation `review_approved`，`display_name` 为“批准”，`description` 由模板填写，`roles` 为 `[role]`，`action` 为 `add event.actor to approved`；
  3. transition：`on` 为 `approved ⊇ active_actors('<role>')`，`to` 为所选目标 stage。
- 模板直接生成 RFC 0007 语法的 action 和 `on`。
- 提示：stage 有其他 active role 时，模板只把目标 role 放进 `roles`，其他 role 不能发送此 operation。如果确实需要多个 role 参与确认，需要手动扩展 `roles` 和 `on`；模板只生成单 role 的全员确认。
- 展示：作为一条“所有 {role} 确认后 → {target}”的语义行呈现；底层仍是一个 operation 和一条 transition，高级用户可以在文本表达式模式查看。
- `changes_requested` 不自动生成；如果需要，另行添加 operation/变量/transition。
- 目标可以是已声明的 stage 或 `ended`；模板不做特殊处理。

### 7.6 画布与只读视图

- 画布边来自 transitions；边标签显示 `on` 的摘要和目标。operations 和 events 不属于边，它们是 stage 内的触发器。
- `to = "ended"` 的边指向固定的“结束”节点。
- `VersionView` 和只读面板渲染变量、operations、events、进入效果和 transitions。
- 识别出标准全员确认模式时显示语义标签；识别不出时按普通表达式展示。
- 不显示运行期变量值；只读视图展示的是定义。

## 8. API

- 不新增 HTTP 端点。
- `POST /api/v1/tasks/{id}/operations` 的发送者继续来自会话；请求体只有 `operation` 和 `idempotency_key`。原来的 `/inputs` 路径不再保留。
- `allowed_operations` 来自当前 stage 的 `operations`，元素包含 `name`、`display_name`、`description`，供人类或智能体选择操作。
- `allowed_operations` 只返回查看者作为 active actor 且其 active role 在 operation 的 `roles` 中的 operation。
- event 是系统内部触发器，没有发送端点。
- workflow 草稿保存和发布接口接受新的 AST；发布时执行第 6 节校验。
- Task 视图不变，不返回 `variables`。
- 错误码按现有 workflow 校验错误结构返回；新增错误码只出现在校验结果里。

## 9. 内置定义改写

内置定义直接改写为新的 AST。典型变化：

- 原来的 signal 转移：把 signal 改成一个 operation，其 action 完成变量写入，再写一条 `on` 该变量的 transition。
- 原来的 child terminal：改成 `events` 中监听 `child.state_changed`（或 `child.created`）的处理器，由 action 判断结果并写变量，再由 `on` 变量的 transition 转移。
- 原来的转移内 `create_child`：移到目标 stage 的 `created`；进入目标 stage 时创建子任务。
- 原来的转移内 `complete`：改成 `to: "ended"`，并在 transition 的 `action` 中写 `result`。
- `trigger.child.*`：改成 event action 可读的 `event.child.*` 字段；`on` 表达式不再使用。

示例中的 action 使用 RFC 0007 语法；`on` 也使用同一语法。

`goal_clarification` 的 `work` stage：

```json
{
  "name": "work",
  "active_roles": ["clarifier"],
  "vars": { "completed": { "type": "bool", "display_name": "已完成" } },
  "operations": [
    {
      "name": "started",
      "display_name": "开始",
      "description": "认领并开始澄清目标。",
      "roles": ["clarifier"],
      "action": ""
    },
    {
      "name": "completed",
      "display_name": "完成澄清",
      "description": "目标已经澄清完毕，提交进入验收。",
      "roles": ["clarifier"],
      "action": "completed = true"
    }
  ],
  "transitions": [
    {
      "on": "completed",
      "to": "awaiting_acceptance"
    }
  ]
}
```

`completed` 的 action 把 `completed` 置为 true。

`goal_clarification` 的 `awaiting_acceptance` stage：

```json
{
  "name": "awaiting_acceptance",
  "active_roles": ["acceptor"],
  "vars": { "decision": { "type": "string", "display_name": "结论" } },
  "operations": [
    {
      "name": "approved",
      "display_name": "通过",
      "description": "接受澄清结果，任务完成。",
      "roles": ["acceptor"],
      "action": "decision = 'approved'"
    },
    {
      "name": "rejected",
      "display_name": "驳回",
      "description": "驳回澄清结果，返回继续澄清。",
      "roles": ["acceptor"],
      "action": "decision = 'rejected'"
    }
  ],
  "transitions": [
    {
      "on": "decision == 'approved'",
      "action": "result = 'approved'",
      "to": "ended"
    },
    {
      "on": "decision == 'rejected'",
      "to": "work"
    }
  ]
}
```

两个 operation 的 action 分别把 `decision` 置为 `"approved"` 和 `"rejected"`。通过分支的 transition 把 `result` 置为 `"approved"` 并指向虚拟 stage `ended`，任务完成。

`project` 的 `goal` stage：

```json
{
  "name": "goal",
  "vars": { "outcome": { "type": "string", "display_name": "结果" } },
  "created": [
    { "set_state": "in_progress" },
    { "create_child": { "workflow": "goal_clarification", "title": "Clarify project goals" } }
  ],
  "events": [
    {
      "event": "child.state_changed",
      "action": "if event.child.workflow == 'goal_clarification' and event.child.state == 'completed' { if event.child.result == 'approved' { outcome = 'approved' } if event.child.result == 'failed' or event.child.result == 'infeasible' or event.child.result == 'obsolete' { outcome = 'failed' } }"
    }
  ],
  "transitions": [
    {
      "on": "outcome == 'approved'",
      "to": "requirements_discovery"
    },
    {
      "on": "outcome == 'failed'",
      "action": "result = 'failed'",
      "to": "ended"
    }
  ]
}
```

event action 检查 `event.child.workflow`、`event.child.state` 和 `event.child.result`：`goal_clarification` 完成且结果为 `approved` 时把 `outcome` 置为 `"approved"`；结果为 `failed`、`infeasible` 或 `obsolete` 时置为 `"failed"`。项目目标失败时任务以 `result = "failed"` 完成。

`project` 的 `iteration` stage 需要在同一个 stage 内反复创建子任务。由于转移只有 `to`，用自转移进入同一 stage 来重新执行 `created`：

```json
{
  "name": "iteration",
  "vars": {
    "outcome": { "type": "string", "display_name": "结果" },
    "next": { "type": "bool", "display_name": "下一轮" }
  },
  "created": [
    { "create_child": { "workflow": "iteration", "title": "Iteration {count_children('iteration') + 1}" } }
  ],
  "events": [
    {
      "event": "child.state_changed",
      "action": "if event.child.workflow == 'iteration' and event.child.state == 'completed' { if event.child.result == 'approved' { outcome = 'approved' } else if event.child.result == 'failed' or event.child.result == 'infeasible' or event.child.result == 'obsolete' { outcome = 'failed' } else { next = true } }"
    }
  ],
  "transitions": [
    {
      "on": "outcome == 'approved'",
      "action": "result = 'approved'",
      "to": "ended"
    },
    {
      "on": "outcome == 'failed'",
      "action": "result = 'failed'",
      "to": "ended"
    },
    {
      "on": "next",
      "to": "iteration"
    }
  ]
}
```

event action 在 `child.state_changed` 且 `event.child.workflow == "iteration"` 时处理：进入 `completed` 且结果为 `approved` 时把 `outcome` 置为 `"approved"`；结果为 `failed`、`infeasible` 或 `obsolete` 时把 `outcome` 置为 `"failed"`；其他完成情况把 `next` 置为 `true`。自转移会重置变量并再次执行 `created`，创建下一个 iteration 子任务。title 模板只使用 `task.title` 和 `count_children`；`trigger.child.*` 不再出现在 title 模板中。

## 10. 直接改写

- `SCHEMA_VERSION` 保持 1；不引入版本分支，不保留旧解释器。
- 新 AST 直接在现有代码里替换：`inputs` 换成 `operations`；operation 从 `{ signal, when, actions }` 改为 `{ name, display_name, description, roles, action }`；移除 `child_terminal`，新增 `events: [{ event, action }]`；workflow 增加 task 级 `vars`，stage 增加 `vars`（含 `initial`）；transition 改为 `{ on: String, action: String, to: String }`；删除 transition 的 `actions`/`effects`、`Complete` 和 `CreateChild` 节点；stage 不再有 `terminal` 和 `result`。
- `created` 从“只有初始 stage 可用”改为所有 stage 都可声明，并在每次进入时执行；其中的 effect 只保留 `set_state` 和 `create_child`。
- 删除现有 JSON guard AST 和解释器；`on` 和 action 由 [RFC 0007](0007-workflow-script-syntax.md) 定义的脚本语言解析/执行。实现前，解释器不能运行含 `on` 的定义；实现顺序由后续工作确定。
- 解释器：删除 changed 快照/比较逻辑；触发器执行 action 后直接按顺序求值 `on`；命中后先执行 transition 的 `action`，再判断 `to`：为 `ended` 时读 `result` 变量并调用 `complete`，否则设置 stage、重置 stage 变量、激活 role、执行目标 stage 的 `created`。
- 删除 `Update` 以及 `set`/`add` 的 AST 和解释器分支。
- 术语同步：`SignalKey` → `OperationName`；`TaskEvent::Input` → `TaskEvent::Operation`；`has_input` → `has_operation`；`input_signals` → `operation_names`；`DomainEventKind::InputReceived` → `OperationReceived`；`allowed_inputs` → `allowed_operations`；`POST /tasks/{id}/inputs` → `POST /tasks/{id}/operations`。RFC 0003/0004/0005 中对应的术语另行同步。
- 删除 `ChildTerminal` 事件和 `TaskEvent::ChildTerminal`，改用 `ChildStateChanged`；`task-workflow` 消费者增加 `TaskStateChanged`、`ChildCreated`、`ChildStateChanged` 三个 trigger kind。
- 子任务 state 变化时向父任务写入 `ChildStateChanged`；`to = completed` 时 payload 带 `result`。
- 所有内置定义直接改写；数据库里已有的定义数据在开发环境重新 seed/重置，不做兼容读取。
- 编辑器不保留旧节点类型。
- 数据库迁移把 `tasks.variables` 重命名为 `stage_variables`，并增加 `tasks.task_variables`，默认 `'{}'`。
- `TaskDAO` 反序列化使用 `#[serde(default)]`，旧数据按空变量处理。
- 内存和 Postgres 适配器同步加载/保存变量。
- 没有数据回填。

## 11. 测试计划

### 11.1 领域单元测试

- operation 的 action 只改变量，不直接改变 stage。
- 空 action 后仍求值 `on`；`on` 为 true 时转移。
- `on` 为 true 不要求变量变化。
- 多条 transition 同时为 true 时按声明顺序取第一条。
- 一条 transition 执行后不再求值下一条。
- stage 变量在 stage 内累加；离开 stage 后清空；重新进入回到 `initial`（或类型默认值）。
- task 变量跨 stage 保留；transition 的 action 也能读写它。
- 自转移重置 stage 变量、保留 task 变量，并重新执行 `created`。
- 进入目标 stage 执行 `created`，包括 `create_child`。
- `to = "ended"` 完成任务，`task.stage` 保持来源 stage，`task.result` 来自 `result` 变量；`result` 为空时 `task.result` 为 null。
- `ActorBound` 只刷新 active actors，不求值 transition。
- `event.actor` 只在 operation 的 action 中可用；event 字段只在对应事件的 action 中可用。
- `active_actors` 返回正确 role 的 active 成员；绑定新成员后包含。
- 全员确认：两个/三个 reviewer 的场景、重复发送、`changes_requested`、重新进入、中途绑定新 reviewer。
- 只有 operation 的 `roles` 中的 active actor 能发送；同一账户的其他 active role 不在 `roles` 中时仍被拒绝。
- 同一账户同时持有允许和不允许的 active role 时，允许的 role 之一即可发送。
- `task.created` 在初始 stage 的 `created` effects 之后执行，并能触发 transition。
- `task.state_changed` 处理 pending → in_progress；completed 不处理。
- `child.created` 和 `child.state_changed` 只投递给直接父任务；`event.child.result` 只在 completed 且 result 存在时有值。
- 没有 event 处理器时事件不改变量，也不转移。
- `iteration` 的自转移反复创建子任务场景。
- schema 结构与类型校验，含 operation 的 `display_name`、`description`、`action` 和 event 的 `event`、`action`。

- 表达式/脚本语言按 RFC 0007 补充单元测试：词法、语法、类型、上下文、限制、整数溢出、读/写集合。

### 11.2 服务与发布校验

- 发布带 task/stage 变量、operations、events、transitions、created 的定义成功。
- 引用未知 stage 时失败。
- 缺少 operation 元数据或元数据为空时失败。
- `roles` 为空，或引用不是当前 stage active role 的 role 时失败。
- 未知 event 名、同一 stage 重复 event 时失败。
- `on` 缺失、不是字符串或为空时失败。
- stage 名为 `ended`、stage 变量与 task 变量同名、`initial` 类型不匹配时失败。
- `created` 中的 `set_state: completed` 失败。
- 现有 stage 可达性警告。
- 表达式/脚本的语法和类型校验按 RFC 0007 补充。

### 11.3 HTTP 端到端

- 创建带“全员确认”的账户 workflow，发布。
- 创建项目并绑定两个 reviewer。
- `allowed_operations` 返回两个 operation 的 `name`、`display_name` 和 `description`，且按查看者的 active role 过滤。
- 第一个人发送 `review_approved`：stage 不变，变量改变。
- 第二个人发送：进入 `acceptance`。
- 重复发送不改变结果。
- 任何一个人发送 `changes_requested`：立即退回。
- 重新进入 `review` 后，上一轮的票不影响新一轮。
- 中途绑定第三个 reviewer：必须等他确认。
- 子任务创建/完成时，父任务的 event action 被执行并驱动父任务转移。
- 完成时 `task.result` 来自 `result` 变量。

### 11.4 编辑器

- 类型检查、变量/operation/event/进入效果/transition 渲染。
- operation 元数据和 action 文本框的编辑。
- event 名称从事件表选择，同一事件不能重复。
- `on` 与 `action` 文本编辑、`to` 下拉包含自转移和 `ended`。
- task 变量面板与 stage 变量面板的初始值编辑；变量重名被拒绝。
- 模板生成的 operation/transition 正确。
- 多 active role 时显示提示。
- 只读视图和 Diff 能渲染新节点。
- 画布边来自 transition，标签显示 `on` 摘要。

## 12. 替代方案

| 方案 | 结论 |
|---|---|
| operation 直接携带 guard 和 effects | 违反“operation 不直接转移”的界定；无法干净表达“全部 reviewer 通过”。 |
| 保留 `updates` 并另加 `action` | 两套修改变量的机制，编辑器和校验都要维护两条路径；本设计只保留 `action`。 |
| 保留 `child_terminal` 特例 | 只能处理子任务完成，无法处理 task 和子任务的其他系统事件；本设计用统一的 `events`。 |
| `on` 是变量名列表 + `when` 表达式 | 两个概念描述同一个条件；本设计把条件直接写成 `on` 表达式。 |
| `on` 继续用结构化 JSON guard | 和 action 形成两套表达式机制，编辑器和校验各维护一套；本设计让 `on` 与 action 共用文本语言。 |
| 保留 changed 门槛 | `on` 已经是显式条件；再要求“变量变化”会让空 action 和重复发送的行为不直观。本设计直接求值 `on`。 |
| transition 带 effects（`create_child`/`complete`） | 结构效果分散在条件和转移里；本设计把创建子任务放到目标 stage 的进入效果，把完成放到 `to = "ended"`。 |
| 用 `complete` effect 结束任务 | 每个结果都要在转移里重复写 result；本设计用虚拟 stage `ended` 和保留变量 `result`。 |
| 禁止自转移 | 无法在同一个 stage 内反复创建子任务；本设计允许自转移，并按新进入重置变量。 |
| 内建会签原语 | 更小，但表达力局限于“全员/阈值”；本 RFC 按需求选择通用集合变量。 |
| 只有 task 作用域变量 + `clear` | 需要作者在每次进入 stage 时手动清空，容易漏；本设计同时提供 task 变量和自动重置的 stage 变量。 |
| 暴露 actor 历史和输入事件 | 让作者用没有循环/聚合的表达式语言手写集合逻辑，方向和 RFC 0005 的“有限表达”冲突。 |
| 只按 stage 授权 | 多 role stage 无法限制“谁能确认”；本设计用 operation 的 `roles` 限制发送者。 |
| 在 workflow 定义里放通用 JSON 更新 DSL | 无法表达后续的真实 action 需求；本设计改用 RFC 0007 定义的脚本语言。 |

## 13. 脚本语言

脚本语言的语法、类型、上下文、校验、限制、编辑器和版本化由 [RFC 0007](0007-workflow-script-syntax.md) 定义。本节不再保留待设计项。

RFC 0006 只依赖 RFC 0007 的以下保证：

- `on` 是纯 boolean 表达式，没有副作用；action 可以读写当前 stage 的变量，并执行受控副作用（如 `create_issue`）。
- 脚本不能改变 stage、创建子任务或完成任务；这些结构效果只属于 transition 和 stage 进入。
- 发布时完成全部词法、语法、类型、上下文引用和限制检查；运行期只有确定性的求值。
- 发布校验提供 action 的读/写变量集合和 `on` 的读变量集合，用于 `unused_variable` 和 `transition_never_triggered`。

## 14. 后续扩展

只有出现真实需求时再考虑：

- 变量上的 `remove` / `clear`（撤回确认）；
- `size` / `contains` / 集合字面量；
- 更多系统事件：删除 task/子任务（`task.deleted`、`child.deleted`）、stage 变化、actor 绑定；
- event 的条件表达式或 per-event role；
- Task 视图中的审批进度投影；
- 超时、SLA、取消。

这些都不属于本设计，避免把“全员确认”一个需求扩展成完整的语言能力。
