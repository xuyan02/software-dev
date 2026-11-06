# RFC 0007：Workflow 脚本语法

- 状态：Draft，设计待评审
- 日期：2026-11-05
- 关联：[RFC 0006 Workflow 变量、Operation、Event 与转移](0006-workflow-variables.md)
- 取代：RFC 0006 第 13 节中关于脚本语法、类型、校验、限制和编辑器支持的待设计项。

## 0. 结论

workflow 只有一套脚本语言，有两种使用形式：

- **action**：语句脚本，用于 operation、event 和 transition；可以读写 task 变量和当前 stage 变量。
- **`on`**：表达式，必须返回 boolean，不能赋值、不能有副作用。

示例：

```json
"action": "add event.actor to approved"
```

```json
"on": "approved ⊇ active_actors('reviewer')"
```

完整片段（一个 stage）：

```json
{
  "name": "review",
  "active_roles": ["reviewer"],
  "vars": {
    "approved": { "type": "actor_set", "display_name": "已批准" }
  },
  "operations": [
    {
      "name": "review_approved",
      "display_name": "批准",
      "description": "确认当前实现可以进入验收。所有 reviewer 都批准后，任务进入验收。",
      "roles": ["reviewer"],
      "action": "add event.actor to approved"
    }
  ],
  "transitions": [
    {
      "on": "approved ⊇ active_actors('reviewer')",
      "to": "acceptance"
    }
  ]
}
```

设计原则：

1. **小且可完全静态检查**。语法、变量、类型、role 和 workflow 引用都在发布时检查；运行期不应出现解析或类型错误。
2. **确定性**。不能读时钟、随机数，不能做 I/O；相同输入必须得到相同结果。
3. **没有循环、递归和用户定义函数**。执行时间与脚本长度成线性关系，不需要步数上限。
4. **唯一的可变状态是 task 变量、当前 stage 变量和内置 `result`**。不能改变 stage、创建子任务或完成任务；这些结构效果属于 transition 和 stage 进入。
5. **两种形式共用一套词法和表达式语法**。`on` 只允许表达式；action 只允许语句。
6. **没有隐式类型转换**。所有赋值和操作数类型必须精确匹配。
7. **文本友好**。ASCII 标识符、单引号字符串、`//` 行注释；只有集合包含运算符 `⊇` 使用非 ASCII 字符。

## 1. 目标与非目标

### 1.1 目标

- 让 operation/event/transition 的 action 和 transition 的 `on` 使用同一套可读、可校验的文本语言。
- 让“全员确认”这类逻辑可以写成一句 `add event.actor to approved` 和一条 `on` 表达式。
- 让编辑器能做高亮、补全、类型检查和行列错误定位。
- 让解释器在执行前完成全部静态检查，运行期只做确定性的求值和赋值。
- 保持与 RFC 0006 的变量、operation、event、transition 模型一致。
- 不引入通用编程语言的复杂度：没有 I/O、循环、用户定义函数、模块、异常和动态类型。

### 1.2 非目标

- 通用编程语言。
- 循环、递归、用户定义函数、闭包。
- I/O、网络、文件、时钟、随机数。
- 用户定义类型、对象、数组、map。
- `actor_set` 之外的集合类型；集合字面量。
- 字符串插值、多行字符串、模板。
- 异常处理、`try`/`catch`、`return`。
- 模块、导入、宏。
- 运行时动态求值或反射。

## 2. 词法

### 2.1 源文本

- UTF-8；忽略 UTF-8 BOM。
- 空白字符：空格、水平制表符、回车、换行。空白只分隔 token，不决定语句边界。
- 注释：`//` 到行尾。注释可以出现在任何空白可以出现的位置。
- 字符串中的 `//` 不是注释。

### 2.2 标识符

```text
identifier = [a-z] [a-z0-9_]*
```

- 变量名和路径段使用同一规则。
- 标识符区分大小写；只允许小写 ASCII。
- 以下名字是保留字，不能作为变量名：

```text
if  else  add  to  and  or  not
true  false  null  task  event
active_actors  count_children  result
```

保留字不能用作变量名；变量名与之冲突时发布校验报 `script_reserved_name`。`result` 是内置的 task 变量，本身可直接引用，但不能再被声明。

### 2.3 字面量

| 字面量 | 类型 | 语法 |
|---|---|---|
| 布尔 | `bool` | `true`、`false` |
| 整数 | `int` | `0` 或 `[1-9][0-9]*`；负数用一元 `-` |
| 字符串 | `string` | 单引号字符串，见下 |
| null | null | `null` |

整数字面量必须在 `i64` 范围内；超出范围报语法错误。`-9223372036854775808` 不作为字面量接受，需要写成 `-9223372036854775807 - 1`。

字符串：

- 以 `'` 开始和结束。
- 不允许直接包含换行；需要换行时用 `\n`。
- 转义：`\\`、`\'`、`\n`、`\r`、`\t`。
- 其他反斜杠组合是语法错误。
- 字符串内容按 UTF-8 原样保存，不做 Unicode 规范化。
- 不支持字符串插值。

### 2.4 运算符与标点

```text
(   )   {   }   ,   .
=   +   -   ==   !=   <   <=   >   >=   ⊇
```

- `⊇` 是 U+2287，表示集合包含（superset of）。
- 没有 `;`、`!`、`&&`、`||`、`*`、`/`、`%`。
- `/` 只出现在 `//` 注释中。

## 3. 语法

### 3.1 两种模式

- **program 模式**：operation/event/transition 的 action 使用。解析一个语句序列。
- **expression 模式**：`on` 使用。解析单个表达式，之后只允许空白和注释。

同一段文本不能同时作为两种模式使用；`on` 中出现语句是语法错误，action 中出现裸表达式也是语法错误。

### 3.2 program 模式

```ebnf
program        = { statement } EOF ;

statement      = assignment
               | add_statement
               | if_statement
               | block ;

assignment     = IDENT "=" expression ;

add_statement  = "add" expression "to" IDENT ;

if_statement   = "if" expression block
                 [ "else" ( if_statement | block ) ] ;

block          = "{" { statement } "}" ;
```

- 空 program 合法，表示空 action。
- 语句之间没有分隔符；空白不决定语句边界，解析器根据语法判断语句何时结束。
- 建议一行一个语句；换行没有任何语义。
- `{}` 合法，表示空块。
- 没有表达式语句：`approved` 单独出现不是合法 action。
- 没有变量声明：变量来自 task 的 `vars`、当前 stage 的 `vars`，以及内置 `result`。
- 没有 `;`；出现 `;` 是语法错误。

### 3.3 expression 模式

```ebnf
expression     = or_expression ;

or_expression  = and_expression { "or" and_expression } ;

and_expression = not_expression { "and" not_expression } ;

not_expression = "not" not_expression | comparison ;

comparison     = additive
                 [ ( "==" | "!=" | "<" | "<=" | ">" | ">=" | "⊇" ) additive ] ;

additive       = unary { ( "+" | "-" ) unary } ;

unary          = "-" unary | primary ;

primary        = literal
               | IDENT
               | context_path
               | call
               | "(" expression ")" ;

literal        = "true" | "false" | "null" | INTEGER | STRING ;

context_path   = ( "task" | "event" ) "." IDENT { "." IDENT } ;

call           = ( "active_actors" | "count_children" )
                 "(" STRING ")" ;
```

- 比较运算符是非结合的：`a == b == c` 是语法错误，必须写 `(a == b) == c` 或拆成两个条件。
- `not` 的优先级低于比较：`not a == b` 等价于 `not (a == b)`。
- 一元 `-` 的优先级高于二元 `+`/`-`：`-a + b` 等价于 `(-a) + b`。
- `active_actors` 和 `count_children` 的实参必须是字符串字面量，不能是变量或表达式。

### 3.4 优先级

从低到高：

| 优先级 | 运算符 | 结合性 |
|---|---|---|
| 1 | `or` | 左结合 |
| 2 | `and` | 左结合 |
| 3 | `not` | 前缀 |
| 4 | `==` `!=` `<` `<=` `>` `>=` `⊇` | 非结合 |
| 5 | `+` `-` | 左结合 |
| 6 | 一元 `-` | 前缀 |
| 7 | `()`、字面量、变量、路径、函数调用 | — |

用括号可以覆盖优先级。

## 4. 类型与求值上下文

### 4.1 类型

| 类型 | 值 | 可用的操作 |
|---|---|---|
| `bool` | `true`、`false` | `not`、`and`、`or`、`==`、`!=` |
| `int` | 64 位有符号整数 | `+`、`-`、一元 `-`、比较、`==`、`!=` |
| `string` | UTF-8 字符串 | `==`、`!=` |
| `string?` | 字符串或 null | 与 `string`/`string?`/`null` 做 `==`、`!=` |
| `actor` | 一个账户 id | 只能作为 `add ... to` 的右操作数 |
| `actor_set` | 账户 id 的集合 | `⊇`、赋值、作为 `add` 的目标 |

- 变量类型只能是 `bool`、`string`、`int`、`actor_set`，由 task 的 `vars` 或当前 stage 的 `vars` 声明；内置 `result` 始终是 `string`。
- 同一作用域内变量名唯一；stage 变量不能与 task 变量同名，因此脚本中的变量名没有歧义。
- `string?` 只用于 `event.child.result`。
- `actor` 只用于 `event.actor`。
- 没有隐式转换：`int` 和 `string` 不能比较，`string?` 不能赋给 `string`，`actor` 不能赋给变量。

### 4.2 上下文值

所有 action 和 `on` 都可以读：

| 值 | 类型 | 说明 |
|---|---|---|
| 变量名 | 变量声明的类型 | task 变量、当前 stage 变量，以及内置 `result`（`string`）。 |
| `task.state` | `string` | `pending`、`in_progress`、`completed`。 |
| `task.title` | `string` | 任务标题。 |
| `active_actors('role')` | `actor_set` | role 必须已声明，且在当前 stage 的 `active_roles` 中。 |
| `count_children('workflow')` | `int` | 当前 task 的直接子任务中，workflow id 匹配的数量。 |

不同触发器的 action 还可以读：

| 触发器 | 可读的 event 字段 |
|---|---|
| operation 的 action | `event.actor`：`actor` |
| `task.created` 的 action | 无 |
| `task.state_changed` 的 action | `event.state`：`string` |
| `child.created` 的 action | `event.child.id`：`string`；`event.child.workflow`：`string`；`event.child.title`：`string` |
| `child.state_changed` 的 action | `event.child.id`：`string`；`event.child.workflow`：`string`；`event.child.state`：`string`；`event.child.result`：`string?` |
| transition 的 action | 无 |
| 任意 `on` | 无 |

`event.child.result` 在子任务进入 `completed` 且该任务有 result 时是字符串，否则是 `null`。

不在上表中的 `event.*` 路径是编译错误。`on` 中的任何 `event.*` 都是编译错误。

### 4.3 运算符类型规则

| 表达式 | 要求 | 结果类型 |
|---|---|---|
| `not a` | `a: bool` | `bool` |
| `a and b` / `a or b` | 两边都是 `bool` | `bool`；短路求值 |
| `a == b` / `a != b` | 两边同为 `bool`、`int`、`string`；或操作数涉及 `string?`/`null`，且另一边是 `string`/`string?`/`null` | `bool` |
| `a < b` 等 | 两边都是 `int` | `bool` |
| `a + b` / `a - b` | 两边都是 `int` | `int` |
| `-a` | `a: int` | `int` |
| `a ⊇ b` | 两边都是 `actor_set` | `bool` |
| `active_actors('r')` | `'r'` 是字符串字面量 | `actor_set` |
| `count_children('w')` | `'w'` 是字符串字面量 | `int` |

- `==`/`!=` 不支持 `actor` 和 `actor_set`。
- `actor_set` 没有相等比较、`size`、`contains`、`union`；只有 `⊇`。
- `string` 没有排序比较；`<` 等只用于 `int`。
- `null` 只出现在与 `string?` 相关的比较中；`null == null` 为 true，`null == 'x'` 为 false。
- `a ⊇ b` 为真当且仅当 `b` 的每个元素都在 `a` 中；空集是任何集合的子集。

### 4.4 赋值与 `add`

- `x = expr`：`expr` 的类型必须与 `x` 的声明类型完全一致。`actor_set` 变量可以整体赋值，例如 `approved = active_actors('reviewer')`。
- `add expr to x`：`expr` 必须是 `actor`，`x` 必须是 `actor_set` 变量。`event.actor` 是目前唯一的 `actor` 表达式。重复加入同一账户是幂等的。
- 没有 `remove`、`clear` 或集合字面量。
- 不能给 `task.*`、`event.*` 或函数结果赋值。

### 4.5 role 与 workflow 参数

`active_actors('role')` 和 `count_children('workflow')` 的参数必须是字符串字面量，目的是让发布校验能静态解析引用；写成变量或表达式是语法错误。

- role 必须出现在 workflow 声明的 roles 中，并且在当前 stage 的 `active_roles` 中。
- workflow 必须是合法的 workflow id；发布时检查它能否被解析为子 workflow。
- 参数不能是变量或运行时计算值。

## 5. 语句语义

### 5.1 执行顺序

- program 从上到下执行每个语句。
- `if` 只执行命中的分支。
- 块没有独立作用域；变量仍属于其声明作用域（task 或当前 stage）。
- action 执行完后，变量作为 transition `on` 的输入。
- transition 的 action 在 `on` 匹配后执行；它读到的是触发器 action 执行完后的变量，写入的变量不会触发新一轮 transition 求值。
- 空 action 不改变量；`on` 仍然会被求值。

### 5.2 语句

```text
attempt = attempt + 1
completed = true
decision = 'approved'
approved = active_actors('reviewer')
add event.actor to approved
if event.child.result == 'approved' {
    outcome = 'approved'
} else if event.child.result == 'failed' {
    outcome = 'failed'
} else {
    next = true
}
```

- 赋值立即生效，后续语句读到新值。
- `add` 立即生效；重复加入不改变集合。
- `if` 的条件必须是 `bool`；没有 truthiness。
- `else` 可以省略；`else if` 等价于 `else { if ... }`。
- 没有循环、`return`、用户定义函数调用或异常。

### 5.3 原子性

- 发布校验失败时定义不能发布，脚本不会执行。
- action 在运行期遇到错误（目前只有整数溢出）时，本次触发器整体失败：变量、stage、actors、children 都不改变。
- 触发器失败的错误传播沿用现有 `TaskError::Workflow` 和事件消费机制。
- 因为语言是确定性的，运行期错误会在同一个触发器上重复出现；这类错误应视为定义缺陷，而不是可重试的瞬时故障。

## 6. 表达式求值

- `and`、`or` 短路：`false and x` 不计算 `x`，`true or x` 不计算 `x`。
- `not` 只接受 `bool`。
- 整数使用 `i64`；`+`、`-` 和一元 `-` 做溢出检查，溢出是运行期错误。
- 字符串比较按字节的 UTF-8 编码做精确比较，不做规范化。
- `null` 比较规则见 4.3。
- `active_actors(role)` 返回求值时刻该 role 的 active 账户集合。
- `count_children(workflow)` 返回求值时刻当前 task 的直接子任务数量；包含所有 state 的子任务。
- 变量、`task.*` 和 children 的读取是只读的。
- 表达式没有副作用，不能赋值，不能调用有副作用的函数。

## 7. `on` 表达式

- `on` 使用 expression 模式，不允许语句。
- 顶层表达式必须是 `bool`。
- `on` 不能读 `event.*`、operation 名称或触发类型。
- `on` 可以读变量、`task.state`、`task.title`、`active_actors`、`count_children` 和字面量。
- `on` 是纯函数式的：相同变量和 Task 快照必须得到相同结果。
- 空字符串、只有注释的字符串不是合法 `on`。
- 示例：

```text
changes_requested
decision == 'approved'
approved ⊇ active_actors('reviewer')
count_children('iteration') > 0
task.state == 'in_progress'
```

## 8. 校验

### 8.1 阶段

发布时按顺序执行：

1. 词法分析。
2. 语法分析；区分 program 模式和 expression 模式。
3. 变量解析：所有标识符必须是 task 变量、当前 stage 变量或内置 `result`。
4. 类型检查：赋值、运算符、条件、`on` 的顶层类型。
5. 上下文检查：`event.*` 只能出现在对应的 action 中；`event.actor` 只能出现在 operation 的 action 中。
6. 引用检查：`active_actors` 的 role、`count_children` 的 workflow。
7. 限制检查：源文本大小、嵌套深度、语句数量。
8. 静态分析结果：每个 action 的读/写变量集合，每个 `on` 的读变量集合。

### 8.2 错误码

| 错误码 | 条件 |
|---|---|
| `script_syntax_error` | 词法或语法错误；消息包含行、列。 |
| `script_unknown_variable` | 引用了 task 和当前 stage 都未声明的变量（`result` 始终可用）。 |
| `script_reserved_name` | 变量名是保留字。 |
| `script_type_mismatch` | 赋值、运算或比较的操作数类型不匹配。 |
| `script_condition_not_boolean` | `if` 条件或 `on` 顶层不是 `bool`。 |
| `script_unknown_field` | `task.*` 或 `event.*` 路径不存在。 |
| `script_event_field_not_available` | `event.*` 出现在 `on` 中，或出现在不提供该字段的 action 中。 |
| `script_unknown_role` | `active_actors` 引用了未声明的 role。 |
| `script_role_not_active` | `active_actors` 引用了当前 stage 未激活的 role。 |
| `script_unknown_workflow` | `count_children` 引用的 workflow 不存在或不可见。 |
| `script_statement_not_allowed` | `on` 中出现语句，或 action 中出现裸表达式。 |
| `script_source_too_large` | 源文本超过限制。 |
| `script_too_deep` | 表达式或块嵌套超过限制。 |
| `script_too_many_statements` | 语句数量超过限制。 |
| `script_integer_overflow` | 运行期整数运算溢出。 |

- 错误信息包含 1-based 行号和列号。
- 多个错误可以一起返回；编辑器展示全部可恢复的错误。
- 运行期错误只可能是 `script_integer_overflow`；其他错误都应在发布时被拒绝。

### 8.3 静态分析

- action 的写集合：所有赋值目标和 `add` 目标。
- action 的读集合：所有被读取的变量。
- `on` 的读集合。
- 集合按声明作用域归属：stage 变量的读写只在该 stage 内统计；task 变量和 `result` 的读写在整个 workflow 内统计。
- 这些信息用于：
  - `unused_variable`：变量从未被任何 action 或 `on` 读取；
  - `transition_never_triggered`：`on` 读取了变量，但这些变量都没有被任何 action 写；
  - 编辑器补全和“未使用”提示。

## 9. 限制

| 限制 | 值 |
|---|---|
| 源文本大小 | 16 KiB（JSON 解码后的脚本文本，UTF-8 bytes） |
| 表达式/块嵌套深度 | 32 |
| 单个 action 的语句数 | 1024 |
| 整数 | `i64`，溢出报运行期错误 |
| 字符串长度 | 不单独限制；受源文本和事件数据约束 |

- 没有循环，执行时间与脚本长度成线性关系；不需要执行步数或超时限制。
- 没有 I/O、网络、文件、时钟、随机数。
- 没有动态求值。
- 所有函数都是纯函数；`active_actors` 和 `count_children` 只读当前 Task 快照。

## 10. 完整示例

### 10.1 全员确认

operation：

```json
{
  "name": "review_approved",
  "display_name": "批准",
  "description": "确认当前实现可以进入验收。所有 reviewer 都批准后，任务进入验收。",
  "roles": ["reviewer"],
  "action": "add event.actor to approved"
}
```

transition：

```json
{
  "on": "approved ⊇ active_actors('reviewer')",
  "to": "acceptance"
}
```

要求修改：

```json
{
  "name": "changes_requested",
  "display_name": "要求修改",
  "description": "当前实现需要修改，任务退回实现阶段。",
  "roles": ["reviewer"],
  "action": "changes_requested = true"
}
```

```json
{
  "on": "changes_requested",
  "to": "implementation"
}
```

### 10.2 goal 的 child 事件 action

```text
if event.child.workflow == 'goal_clarification' and event.child.state == 'completed' {
    if event.child.result == 'approved' {
        outcome = 'approved'
    }
    if event.child.result == 'failed'
        or event.child.result == 'infeasible'
        or event.child.result == 'obsolete' {
        outcome = 'failed'
    }
}
```

作为 JSON 字符串时可以写成一行，或用 `\n` 保留换行。换行不影响语义。

### 10.3 iteration 的 child 事件 action

```text
if event.child.workflow == 'iteration' and event.child.state == 'completed' {
    if event.child.result == 'approved' {
        outcome = 'approved'
    } else if event.child.result == 'failed'
        or event.child.result == 'infeasible'
        or event.child.result == 'obsolete' {
        outcome = 'failed'
    } else {
        next = true
    }
}
```

### 10.4 `on` 表达式

```text
changes_requested
decision == 'approved'
outcome == 'failed'
next
count_children('iteration') > 0
task.state == 'in_progress'
approved ⊇ active_actors('reviewer')
```

### 10.5 计数器

```text
attempt = attempt + 1
if attempt > 3 {
    outcome = 'failed'
}
```

## 11. 编辑器

- `action`（operation、event、transition）使用 program 模式编辑器；`on` 使用 expression 模式编辑器。
- 语法高亮：关键字、字符串、整数、布尔/null、变量、`task.*`/`event.*`、内置函数、运算符。
- 补全：
  - 提示 task 变量、当前 stage 变量和内置 `result`；按类型过滤；赋值右侧只提示与左侧类型相同的变量和表达式；
  - `event.*` 只提示当前触发器可用的字段；
  - `on` 不提示任何 `event.*`；
  - `active_actors` 提示当前 stage 的 active role；
  - `count_children` 提示可见的 workflow id。
- 诊断：发布校验的 `script_*` 错误直接显示在文本编辑器的对应行列；`on` 的顶层非 bool 在表达式末尾提示。
- 代码片段：`add ... to ...`、`if ... { ... } else { ... }`、`x = ...`。
- 不提供在编辑器中执行脚本的功能。

## 12. 版本化

- 脚本语言当前没有独立的版本字段；它与 `schema_version: 1` 一起演进。
- 开发完成前，语法变更直接改写，不保留兼容解析器。
- 引入破坏性语法变更时，如果需要兼容，再单独设计语言版本；本 RFC 不做预留。

## 13. 后续扩展

只有出现真实需求时再考虑：

- `remove ... from ...`、`clear`（撤回确认）；
- `∈` / `contains`、集合相等；
- 字符串排序比较、字符串拼接；
- `*`、`/`、`%`；
- 循环和用户定义函数（会引入执行上限）；
- 多行字符串、字符串插值；
- 块注释；
- actor 相等比较；
- `null` 合并/默认值操作符。

这些都不属于当前语言，避免把“全员确认”一个需求扩展成通用脚本语言。
