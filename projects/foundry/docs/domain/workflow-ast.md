# 流程定义 AST v1

流程定义是一个 JSON AST，描述 Task 的 stage、变量、operation、event、转移和进入效果。AST 是唯一真相：流程图、版本 diff 和编辑器都是它的投影，不存在独立的图数据。解释器只能执行受控的脚本和效果，不能做任意 I/O。

完整设计见 [RFC 0006：Workflow 变量、Operation、Event 与转移](../rfcs/0006-workflow-variables.md)，脚本语法见 [RFC 0007：Workflow 脚本语法](../rfcs/0007-workflow-script-syntax.md)。本文是实现者查阅结构和语义的参考。

## 顶层结构

```json
{
  "schema_version": 1,
  "roles": [],
  "vars": {},
  "initial_stage": "work",
  "stages": []
}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `schema_version` | `integer` | 当前只接受 `1` |
| `roles` | `RoleSpec[]` | 流程声明的角色；可以为空 |
| `vars` | `map<string, VarSpec>` | task 变量；跨 stage 保留 |
| `initial_stage` | `string \| null` | Task 创建时进入的 stage；必须是已声明的 stage |
| `stages` | `WorkflowStage[]` | stage 列表，`name` 在流程内唯一；`ended` 是保留名 |

未知字段会被拒绝（`deny_unknown_fields`），拼写错误不能静默生效。

## 角色

```json
{
  "name": "reviewer",
  "kind": "any",
  "min_actors": 1,
  "max_actors": null
}
```

| 字段 | 说明 |
|---|---|
| `name` | 小写字母开头，只能包含小写字母、数字和下划线 |
| `kind` | `human` / `agent` / `any`；`any` 同时接受人类和智能体 |
| `min_actors` | 最少绑定的 actor 数；`0` 表示可留空 |
| `max_actors` | 最多绑定的 actor 数；`null` 表示不限 |

角色只声明可绑定的种类和数量。具体账户由 Task 的 actor 绑定决定，定义不能指定具体账户。

## Stage

```json
{
  "name": "review",
  "active_roles": ["reviewer"],
  "vars": {
    "approved": { "type": "actor_set", "display_name": "已批准" }
  },
  "operations": [],
  "events": [],
  "created": [],
  "transitions": []
}
```

| 字段 | 说明 |
|---|---|
| `name` | stage 标识；小写字母开头，只能包含小写字母、数字和下划线。stage 没有显示名。 |
| `active_roles` | 进入 stage 时激活这些角色已绑定的 actor |
| `vars` | 本 stage 的变量；进入时按声明初始化，离开或重新进入时丢弃 |
| `operations` | 人类或智能体可以发起的操作 |
| `events` | 本 stage 监听哪些系统事件 |
| `created` | 每次进入本 stage 时执行的进入效果 |
| `transitions` | 触发器处理后按声明顺序求值的转移 |

进入 stage 时，`active_roles` 中的所有角色会激活其当前已绑定的 actor。之后绑定新 actor 时，如果该 actor 的角色在当前 stage 的 `active_roles` 中，也会被激活；`ActorBound` 本身只刷新 active actors，不执行 action，也不求值 transition。

### 变量

```json
"vars": {
  "changes_requested": { "type": "bool", "display_name": "要求修改" },
  "attempts": { "type": "int", "display_name": "尝试次数", "initial": 0 }
}
```

变量有两个作用域：

- workflow 顶层的 `vars` 是 task 变量；Task 创建时初始化，跨 stage 保留。
- stage 的 `vars` 是 stage 变量；每次进入 stage（包括自转移）按声明重新初始化，离开后丢弃。

| 字段 | 说明 |
|---|---|
| `type` | `bool` / `string` / `int` / `actor_set` |
| `display_name` | 展示名；必填 |
| `initial` | 可选初始值；类型必须与 `type` 一致。`actor_set` 不能声明初始值 |

| 类型 | 未声明 `initial` 时的值 |
|---|---|
| `bool` | `false` |
| `string` | `""` |
| `int` | `0` |
| `actor_set` | 空集合 |

- 任意 stage 的 action 都可以读写 task 变量；stage 变量只属于当前 stage。
- stage 变量不能与 task 变量同名（`variable_shadows_task_variable`）。
- task 变量和 stage 变量都只能由 action（operation、event、transition）修改；进入效果不能改变量。
- `result` 是内置的 task 级 `string` 变量，无需也不能声明。任务完成时，它为非空则写入 `task.result`。
- 变量持久化在 `tasks.task_variables` 和 `tasks.stage_variables`，不进入公开的 Task 视图。
- 变量名遵守标识符规则；保留字不能作为变量名。

### Operation

```json
{
  "name": "review_approved",
  "display_name": "批准",
  "description": "确认当前实现可以进入验收。所有 reviewer 都批准后，任务进入验收。",
  "roles": ["reviewer"],
  "action": "add event.actor to approved"
}
```

| 字段 | 说明 |
|---|---|
| `name` | operation 标识；同一 stage 内唯一 |
| `display_name` | 展示给人类或智能体的名称 |
| `description` | 展示给发送者，帮助判断是否执行 |
| `roles` | 允许发送该 operation 的 role；必填且非空，必须是本 stage `active_roles` 的子集 |
| `action` | 脚本；operation 被发送时执行 |

发送者必须是当前 stage 的 active actor，且其 active role 在 `roles` 中。operation 的 action 可以读取 `event.actor`（发送者）。

### Event

```json
{
  "event": "child.state_changed",
  "action": "if event.child.result == 'approved' { outcome = 'approved' }"
}
```

事件是系统内发生的事情。event 没有 role，也没有过滤条件；action 自己判断是否处理。

| `event` | 可读取的字段 |
|---|---|
| `task.created` | 无 |
| `task.state_changed` | `event.state` |
| `child.created` | `event.child.id`、`event.child.workflow`、`event.child.title` |
| `child.state_changed` | `event.child.id`、`event.child.workflow`、`event.child.state`、`event.child.result` |

- 同一 stage 内每个事件最多声明一次。
- 只监听直接子任务。
- 事件按 task 的事件流顺序逐个处理；处理时使用当时的 current stage。
- `task.state_changed` 的新 state 为 `completed` 时不投递。
- `task.created` 在初始 stage 进入并执行完 `created` 效果之后处理。

### 进入效果

```json
"created": [
  { "set_state": "in_progress" },
  {
    "create_child": {
      "workflow": "goal_clarification",
      "title": "Clarify project goals"
    }
  }
]
```

| 效果 | 说明 |
|---|---|
| `{ "set_state": "pending" \| "in_progress" }` | 修改 Task state；不能设为 `completed` |
| `{ "create_child": { "workflow": "...", "title": "..." } }` | 创建子 Task；workflow 必须能在定义所有者作用域内解析 |

- 每次进入 stage 都执行，包括初始进入、`to` 进入和自转移。
- 按声明顺序执行。
- 不能修改变量，不能直接完成任务。
- `create_child` 的 title 模板只能使用 `task.title` 和 `count_children`。

### 转移

```json
"transitions": [
  { "on": "changes_requested", "to": "implementation" },
  {
    "on": "approved ⊇ active_actors('reviewer')",
    "action": "result = 'approved'",
    "to": "ended"
  }
]
```

| 字段 | 说明 |
|---|---|
| `on` | 必填的非空文本 boolean 表达式；触发器的 action 执行完后按声明顺序求值 |
| `action` | 可选的语句脚本；`on` 匹配后、进入 `to` 前执行 |
| `to` | 目标 stage name、`ended` 或当前 stage 自己 |

- 第一条 `on` 为 true 的 transition 执行，后面的不再求值。
- transition 的 action 读不到 `event.*`；读写的变量与触发器 action 使用同一个合并作用域。
- 一次触发器至多执行一条 transition；进入新 stage 后不会再次求值。
- 自转移按一次新的进入处理：stage 变量重置为初始值、task 变量保留、`active_roles` 重新激活、`created` 重新执行。
- `to` 为 `ended` 时任务完成：若内置 `result` 非空则写入 `task.result`，写入 `completed_at`，清空 active actors；`task.stage` 保持为转移前的 stage。
- 若 Task 仍有未完成依赖或未终止的直接子任务，完成被拒绝。
- transition 不能创建子任务或设置 `task.state`。
- 没有任何 `on` 为 true 时，stage 不变，变量保留。

`ended` 是虚拟的完成目标，不是 stage：它不能出现在 `stages` 中，也不能作为 `initial_stage`。

## 脚本

operation、event 和 transition 的 action，以及 transition 的 `on`，使用同一套语言：action 是语句脚本，`on` 是必须返回 boolean 的表达式。完整语法见 [RFC 0007](../rfcs/0007-workflow-script-syntax.md)。

脚本看到的是合并作用域：task 变量、当前 stage 变量和内置 `result`。stage 变量不能与 task 变量同名，因此变量名没有歧义。

```text
approved = active_actors('reviewer')
add event.actor to approved
attempt = attempt + 1
completed = true
create_issue(
    kind = 'question',
    title = 'Project clarification: {task.title}',
    description = 'Discuss the project.',
    relation = 'clarifies'
)
if event.child.result == 'approved' {
    outcome = 'approved'
} else {
    next = true
}
```

```text
approved ⊇ active_actors('reviewer')
done and not rejected
count_children('iteration') >= 2
```

- 语句：赋值 `x = expr`、`add expr to x`、`create_issue(...)`、`if`/`else`、块；空 action 合法。
- 表达式：字面量、变量、`task.state`、`task.title`、`active_actors('role')`、`count_children('workflow')`、布尔/比较/整数运算、括号。
- 类型只有 `bool`、`string`、`int`、`actor_set`，以及只在 event 上下文出现的 `actor` 和 `string?`。
- action 可以执行受控副作用（目前是 `create_issue`），但不能改变 task state/stage、不能完成任务；这些仍由 transition 和进入效果负责。
- `on` 必须是纯 boolean 表达式，没有副作用。
- 没有隐式转换、循环、函数定义、任意 I/O 或时钟。
- 发布时完成语法、类型、变量、role、workflow 引用和事件字段检查；运行期只可能发生整数溢出。
- 源文本上限 16 KiB，嵌套深度 32，语句数 1024。

### 创建 Issue

`create_issue` 只能出现在 action 中，参数全部使用命名参数和字符串字面量：

| 参数 | 必填 | 说明 |
|---|---|---|
| `kind` | 是 | `feedback` / `bug` / `question` / `idea` / `task_request` / `incident` |
| `title` | 是 | Title 模板，语法与 `create_child.title` 相同 |
| `description` | 否 | Issue 描述，默认空 |
| `relation` | 否 | 到当前 Task 的链接关系，默认 `clarifies` |

执行时由任务引擎创建 Issue 和 TaskLink，并在处理该 trigger 的同一个事务里落库。每次执行都会创建一条新 Issue；需要幂等时，由脚本只在只触发一次的事件或 operation 中调用。

## Title 模板

`create_child.title` 和 `create_issue.title` 是模板字符串：

| 模板 | 含义 |
|---|---|
| 普通文本 | 原样输出 |
| `{task.title}` | 父 Task 的标题 |
| `{count_children('iteration')}` | 当前 Task 下 workflow 为 `iteration` 的直接子 Task 数 |
| `{count_children('iteration') + 1}` | 计数加偏移 |
| `{{` / `}}` | 输出字面量 `{` / `}` |

限制：模板源码最多 500 字符；生成后的标题最多 200 字符且不能为空；`count_children` 引用的 workflow id 必须可解析。

## 运行时管线

```text
trigger (operation | event)
  → 执行 action
  → 按声明顺序求值 transitions
       first `on` 为 true
         → 执行 transition 的可选 action
         → `to` 为 ended：任务完成，非空的 `result` 写入 task.result
         → 否则进入 `to` 指定的 stage
  → 至多执行一条 transition
  → 目标 stage：重置 stage 变量、保留 task 变量、激活 role、执行 created effects
```

各类触发器：

| 触发器 | 行为 |
|---|---|
| operation | 校验 operation 已声明且发送者有权限；`pending` → `in_progress`；执行 action；求值 transitions |
| `task.created` | 初始 stage 的 `created` 效果执行完后处理 event action；然后求值一次 transitions |
| `task.state_changed` | 当前 stage 声明的 handler 执行 action；然后求值 transitions |
| `child.created` / `child.state_changed` | 当前 stage 声明的 handler 执行 action；然后求值 transitions |
| `ActorBound` | 只刷新 active actors |
| `DispatchRequested` | 无操作 |

Task 创建时初始化 task 变量（含 `result = ""`）、进入初始 stage（初始化 stage 变量、激活 role、执行进入效果），然后处理 `task.created` 并求值一次 transitions。

## 作用域与版本

- workflow id 全局唯一、不可变；定义不删除，已发布版本不可变。
- 平台定义只能引用平台定义；账户定义可以引用平台定义和同一账户的定义。跨账户引用在发布时拒绝。
- Task 固定 `(workflow, workflow_version)`。Root Task 固定项目创建时的版本；子 Task 固定创建时按定义所有者作用域解析到的最新版本。
- 运行时按固定版本执行，后来的发布不会改变已有 Task 的行为。

## 发布校验

校验入口是 `POST /api/v1/workflows/{id}/validate`；发布前执行同一套规则。

Error（阻止发布）包括：

- 未知 `schema_version`；没有 stage；超过 200 个 stage、50 个角色、500 条 transition 或 1 MiB 序列化大小。
- 非法或重复的 stage/role name；stage 名为保留的 `ended`；`initial_stage` 缺失或未知。
- `active_roles` 引用未声明角色或重复；`max_actors` 非法。
- 变量名非法或为保留字；`display_name` 缺失或超长；stage 变量与 task 变量同名；`initial` 与声明类型不匹配。
- operation 名称非法、重复；`display_name` / `description` 超长；`roles` 为空或不是 `active_roles` 的子集。
- 同一 stage 重复声明同一 event。
- `set_state` 设为 `completed`；`create_child` 的 workflow 无法解析；title 模板非法。
- transition 的 `on` 为空或脚本非法；`to` 不是已声明的 stage，也不是 `ended`。
- operation/event/transition 的 action 和 `on` 的语法、类型、变量、role、workflow 和事件字段错误（`script_*` 错误码）。

Warning（允许发布）包括：

- 不是 `initial_stage`、也没有任何 transition 指向的 stage（`unreachable_stage`）。
- 没有声明任何 transition 的 dead-end stage（自转移也算 transition）。
- 声明了 operation 但没有 `active_roles` 的 stage。
- 角色带 `min_actors = 0`，却被用在声明 operation 的 stage 上。
- 声明但从未激活的角色；声明但从未被读取的变量。
- `on` 读取的变量都没有被任何 action 写入，可能永远不会触发（task 变量按整个 workflow 统计）。

错误和警告都带 AST 路径（如 `stages[2].transitions[0].on`），编辑器用它定位到具体行。
