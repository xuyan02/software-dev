# HTTP / MCP API（MVP）

> 本文描述当前已实现的 MVP API。完整生命周期的目标 API 表面见 [RFC 0008–0019](../rfcs/0008-software-lifecycle-model.md)：产物血缘、基线/变更、辅助系统、需求、架构、实现与验证、发布与运营、Agent 架构、度量、产品组合、知识、治理。实现后逐步回写本文。

Base URL：`http://127.0.0.1:8080`

身份来自登录会话，**不再从请求体读取 `actor`**。REST 与 MCP 使用：

```http
Authorization: Bearer <session-token>
```

Web 控制台使用 `HttpOnly` 的 `foundry_session` Cookie。会话 token 只在登录响应中出现一次。

当前项目由以下顺序决定：

1. `X-Foundry-Project: <uuid>` 请求头；
2. `foundry_project` Cookie；
3. 账户可见的第一个活跃项目。

Issue、Requirement 和 MCP 请求如果没有任何可见项目，返回 `400 project_required`。项目边界优先于一切：跨项目读取按 `404 not_found` 处理，不泄露对象是否存在。

> 仍然只支持本机/受信网络：没有 TLS、CSRF token、速率限制。不要把服务暴露到不可信网络。

## 认证

### 登录

`POST /api/v1/auth/login` → `200 OK`

```json
{ "handle": "admin", "password": "<首次启动时设置的管理员密码>" }
```

```json
{
  "token": "<shown once>",
  "expires_at": "2026-11-09T13:59:14Z",
  "account": {
    "id": "…",
    "handle": "admin",
    "display_name": "Administrator",
    "kind": "human",
    "role": "admin",
    "status": "active",
    "email": null,
    "version": 1,
    "created_at": "…",
    "updated_at": "…"
  }
}
```

密码错误、账户被停用或 handle 不存在统一返回 `401 unauthorized`。token 的 SHA-256 哈希写入 `sessions`；原始 token 不落库。

### 登出

`POST /api/v1/auth/logout` → `204 No Content`。撤销当前 token。

### 当前账户

`GET /api/v1/auth/me` → 当前 `Account`。不要求项目上下文。

## 账户

全部账户管理接口只允许全局 `admin`。`GET /api/v1/accounts/{id}` 允许本人或 admin。

| 方法 | 路径 | 说明 |
|---|---|---|
| `GET` | `/api/v1/accounts` | 列出账户 |
| `POST` | `/api/v1/accounts` | 创建账户 |
| `GET` | `/api/v1/accounts/{id}` | 查看账户 |
| `PATCH` | `/api/v1/accounts/{id}` | 修改显示名、邮箱、角色、状态、密码 |

创建：

```json
{
  "handle": "fixer",
  "display_name": "Fixer",
  "kind": "agent",
  "role": "member",
  "email": null,
  "password": "fixer-password"
}
```

`kind`：`human` / `agent`（默认 `human`）。
`role`：`admin` / `member`（默认 `member`）。
`status`：`active` / `suspended`。

最后一个活跃 admin 不能被降级或停用。账户被停用后，其现有会话立即失效。

## 项目

项目是隔离边界。任何活跃账户都可以创建项目，创建者自动成为 Owner。项目用 UUID 定位；名称可修改且不要求唯一。系统不会自动创建项目或默认项目；首个管理员创建完成后，需要显式创建第一个项目。

| 方法 | 路径 | 权限 |
|---|---|---|
| `GET` | `/api/v1/projects` | 列出当前账户可见的项目 |
| `POST` | `/api/v1/projects` | 任何活跃账户 |
| `GET` | `/api/v1/projects/{id}` | 项目成员或全局 admin，返回项目详情和成员 |
| `PATCH` | `/api/v1/projects/{id}` | Owner 或全局 admin |
| `DELETE` | `/api/v1/projects/{id}` | Owner 或全局 admin；永久删除项目和项目范围内全部数据 |
| `GET` | `/api/v1/projects/{id}/brief` | 读取当前简报版本 |
| `GET` | `/api/v1/projects/{id}/brief/versions` | 列出所有简报版本 |
| `POST` | `/api/v1/projects/{id}/brief/drafts` | Member 或以上；创建自己的新草稿 |
| `GET` | `/api/v1/projects/{id}/brief/drafts` | 列出自己创建的草稿 |
| `GET` | `/api/v1/projects/{id}/brief/drafts/{draft_id}` | 读取自己的草稿 |
| `POST` | `/api/v1/projects/{id}/brief/drafts/{draft_id}/operations` | Member 或以上；提交一个结构化编辑 |
| `POST` | `/api/v1/projects/{id}/brief/drafts/{draft_id}/submit` | Member 或以上；生成新版本并删除草稿 |
| `GET` | `/api/v1/projects/{id}/brief/annotations` | 项目成员或全局 admin；读取简报批注 |
| `POST` | `/api/v1/projects/{id}/brief/annotations` | Member 或以上；添加简报批注 |
| `POST` | `/api/v1/projects/{id}/brief/annotations/{annotation_id}/resolve` | Member 或以上；解决简报批注 |
| `POST` | `/api/v1/projects/{id}/members` | Owner 或全局 admin |
| `PATCH` | `/api/v1/projects/{id}/members/{account_id}` | Owner 或全局 admin |
| `DELETE` | `/api/v1/projects/{id}/members/{account_id}` | Owner 或全局 admin |

创建：

```json
{ "name": "Payments", "description": "支付与退款" }
```

`name` 必填；`description` 可选。创建者成为 Owner。

添加成员：

```json
{ "handle": "fixer", "role": "member" }
```

`role`：`viewer` / `member` / `maintainer` / `owner`。项目必须至少保留一个 Owner。

修改项目：

```json
{ "name": "Payments v2", "description": "…", "status": "archived" }
```

归档项目变为只读：不能新增成员、改角色、处理议题。可以重新设为 `active`。

## Actor

请求体不再接受 `actor`。actor 由会话账户派生：

```json
{ "kind": "human", "id": "<account-id>", "name": "<display-name>" }
{ "kind": "agent", "id": "<account-id>", "role": "<display-name>" }
```

`id` 进入状态历史和事件日志。全局角色和项目角色共同决定权限。

## Issue

Issue 是问题空间：反馈、缺陷、问题、想法、任务请求和事故。它和 Requirement 是两层对象，见 [ADR 0004](../adr/0004-issue-vs-requirement.md)。

### 创建

`POST /api/v1/issues` → `201 Created`

需要项目角色至少 `member`。

```json
{
  "title": "部分退款返回 500",
  "description": "申请 30 元退款时接口报错，用户无法完成退款。",
  "kind": "bug",
  "source": { "kind": "support", "reference": "TICKET-42" },
  "severity": "s2",
  "area": "payments",
  "labels": ["refund"],
  "environment": "production",
  "affected_versions": ["1.4.0"],
  "reproduction": "1. 创建 100 元订单\n2. 申请退款 30 元\n3. 接口返回 500",
  "expected": "退款成功，订单余额为 70 元",
  "actual": "HTTP 500"
}
```

必填：`title`、`description`。

`kind`：`feedback` / `bug` / `question` / `idea` / `task_request` / `incident`（默认 `feedback`）。
`source.kind`：`internal` / `customer` / `support` / `monitoring` / `analytics` / `incident` / `other`（默认 `internal`）。
`severity`：`s1` / `s2` / `s3` / `s4`。Bug 必填。

### 查询

```http
GET /api/v1/issues?status=open&kind=bug&assignee=fixer&text=退款
GET /api/v1/issues/{id}
```

过滤参数：`status`、`kind`、`source`、`assignee`、`text`（标题和描述的子串）。结果按创建时间倒序，最新在前。所有查询都限定在当前项目。

### 评论

`POST /api/v1/issues/{id}/comments`，需要 `member` 及以上。

```json
{
  "body": "已复现，退款失败率约 5%。",
  "internal": false,
  "expected_version": 1
}
```

`internal: true` 表示内部备注。评论会让 Issue 版本递增。

### 状态转换

`POST /api/v1/issues/{id}/transitions`，需要 `maintainer` 及以上。

```json
{ "to": "closed", "expected_version": 5 }
```

| 目标状态 | 额外要求 |
|---|---|
| `resolved` / `rejected` | `resolution` 非空 |
| `duplicate` | `duplicate_of` 指向同一项目内的 Issue UUID |
| `closed` | 只能从 `resolved` 进入，且已有 resolution |

项目角色之外，DemoPolicy 仍然限制智能体：不能关闭、拒绝、判重或重开议题，也不能批准需求。最终处置由人类完成。

## Requirement

> Requirement 当前仍是早期实现，后续会按“承诺空间”重新设计。原始反馈应先进入 Issue。

创建需要 `member` 及以上；编辑和状态转换需要 `maintainer` 及以上。

### 创建需求

`POST /api/v1/requirements` → `201 Created`

```json
{
  "title": "支持部分退款",
  "problem": "用户只能全额退款，客服需要手工处理。",
  "users": ["买家", "客服"],
  "acceptance": [
    {
      "id": "S-1",
      "title": "部分退款成功",
      "given": ["订单已支付 100 元"],
      "when": ["用户申请退款 30 元"],
      "then": ["退款成功且订单余额为 70 元"]
    }
  ],
  "non_goals": ["不支持跨币种退款"],
  "constraints": ["退款接口 P99 < 800ms"],
  "priority": "p1",
  "risk": "medium"
}
```

必填：`title`、`problem`、`acceptance`（至少一条，且每条有 `id`、`title`、`then`）。

`priority`：`p0` / `p1` / `p2` / `p3`（默认 `p3`）。
`risk`：`low` / `medium` / `high` / `critical`（默认 `medium`）。

### 查询

```http
GET /api/v1/requirements?status=in_review&owner=<account-id>&text=退款
GET /api/v1/requirements/{id}
```

`status` 取值：`draft`、`in_review`、`approved`、`in_design`、`in_development`、`in_verification`、`done`、`rejected`、`deferred`、`superseded`。

### 编辑

`PATCH /api/v1/requirements/{id}`

只允许编辑 `draft` / `in_review`。已批准的需求发生范围变化时，必须走新的评审或取代流程。支持字段：`title`、`problem`、`users`、`acceptance`、`non_goals`、`constraints`、`priority`、`risk`，以及可选的 `expected_version`。

### 状态转换

`POST /api/v1/requirements/{id}/transitions`

```json
{
  "to": "in_review",
  "expected_version": 1,
  "reason": "需求已补充验收场景"
}
```

返回转换后的完整需求。

## Workflow 定义

流程定义不属于项目。平台定义对所有账户可见；账户定义只对所有者和管理员可见。只有人类账户可以写入；Agent 的写操作返回 `403 workflow_forbidden`。完整语法见 [AST 参考](../domain/workflow-ast.md)。

| 方法 | 路径 | 权限 |
|---|---|---|
| `GET` | `/api/v1/workflows` | 当前账户可见的定义 |
| `POST` | `/api/v1/workflows` | 人类账户 |
| `GET` | `/api/v1/workflows/{id}` | 可见 |
| `PATCH` | `/api/v1/workflows/{id}` | 所有者或平台管理员 |
| `GET` | `/api/v1/workflows/{id}/versions` | 可见 |
| `GET` | `/api/v1/workflows/{id}/versions/{version}` | 可见 |
| `GET` | `/api/v1/workflows/{id}/draft` | 所有者或平台管理员 |
| `POST` | `/api/v1/workflows/{id}/draft` | 所有者或平台管理员；打开或创建草稿 |
| `PUT` | `/api/v1/workflows/{id}/draft` | 所有者或平台管理员 |
| `POST` | `/api/v1/workflows/{id}/validate` | 所有者或平台管理员；无副作用 |
| `POST` | `/api/v1/workflows/{id}/publish` | 所有者或平台管理员 |
| `GET` | `/api/v1/workflows/{id}/layout` | 可见 |
| `PUT` | `/api/v1/workflows/{id}/layout` | 所有者或平台管理员 |

创建：

```json
{
  "title": "Payments delivery",
  "scope": "account",
  "copy_from": { "workflow_id": "project", "version": 1 }
}
```

`scope`：`account`（默认）或 `platform`（仅平台管理员）。`copy_from` 可选，引用一个已发布的平台版本；新定义得到新的 id 和独立草稿。响应 `201`：

```json
{ "definition": { "id": "…", "title": "…", "owner": { "kind": "account", "account_id": "…", "display_name": "…" }, "latest_version": null, "draft_revision": 1, "editable": true, "created_at": "…", "updated_at": "…" }, "draft": { "workflow_id": "…", "revision": 1, "body": { … }, "updated_by_account_id": "…", "updated_at": "…" } }
```

保存草稿：

```http
PUT /api/v1/workflows/{id}/draft
If-Match: "<revision>"
Content-Type: application/json
```

```json
{ "body": { "schema_version": 1, "roles": [], "initial_stage": null, "stages": [] }, "client_save_id": "<uuid>" }
```

`If-Match` 必填；缺失返回 `428 precondition_required`，不是单个带引号整数返回 `400 invalid_if_match`。相同 `client_save_id` 重复提交不会重复应用，直接返回当前 revision。响应 `{ "revision": 2, "updated_at": "…" }`。

发布：

```http
POST /api/v1/workflows/{id}/publish
If-Match: "<revision>"
```

发布在同一事务中校验 AST、插入 `version = max(version) + 1`、推进草稿 revision 并写审计事件。响应 `{ "version": 2, "revision": 4, "warnings": [] }`。

校验：

```json
{ "body": { … } }
```

响应 `{ "errors": [{ "path": "stages[0].transitions[0].on", "code": "unknown_stage", "message": "…" }], "warnings": [] }`。

布局：

```json
{ "positions": { "work": { "x": 0, "y": 0 } } }
```

布局不影响版本，last-write-wins；删除 stage 后遗留的坐标会被忽略。

流程错误码：

| HTTP | code | 含义 |
|---|---|---|
| 403 | `workflow_forbidden` | Agent 写操作，或无权编辑/发布该定义 |
| 404 | `workflow_not_found` | 定义不存在或对当前账户不可见 |
| 409 | `workflow_unpublished` | 引用的流程没有已发布版本 |
| 409 | `draft_conflict` | `If-Match` 与当前 revision 不一致；详情含 `current_revision`、`current_updated_at` |
| 409 | `draft_not_found` | 保存/发布时草稿不存在，需要先打开草稿 |
| 413 | `workflow_too_large` | 序列化 AST 超过 1 MiB |
| 422 | `workflow_invalid` | 发布校验失败；详情含 `errors` 和 `warnings` |
| 428 | `precondition_required` | 缺少 `If-Match` |
| 400 | `invalid_if_match` | `If-Match` 不是带引号的 revision |

### Workflow MCP 工具

| 工具 | 说明 |
|---|---|
| `list_workflows` | 列出当前账户可见的定义 |
| `get_workflow` | `{ "id": "...", "version"?: 1 }`；带版本时读取不可变版本，否则读取定义 |

MCP 没有 workflow 写工具；定义只能由人类通过 REST 或编辑器修改。

## 错误

| HTTP | code | 含义 |
|---|---|---|
| 400 | `project_required` | 账户没有任何可见项目 |
| 400 | — | 请求体或路径不合法 |
| 401 | `unauthorized` | 未登录、token 无效/过期/已撤销，或账户被停用 |
| 403 | `forbidden` | 全局角色、项目角色或策略拒绝该动作 |
| 404 | `not_found` | 对象不存在，或不属于当前项目 |
| 409 | `duplicate` | handle 已存在 |
| 409 | `invalid_transition` | 状态机不允许该转换，或缺少处置证据 |
| 409 | `version_conflict` | `expected_version` 与当前版本不一致 |
| 422 | `validation_failed` | 领域校验失败（如 Bug 创建时缺少 severity） |
| 500 | `internal_error` | 存储或事件后端失败 |

## MCP

`POST /mcp`，JSON-RPC 2.0。必须携带 Bearer token；项目和权限由该会话决定。支持：

- `initialize`
- `ping`
- `tools/list`
- `tools/call`

工具：

| 工具 | 说明 |
|---|---|
| `create_issue` | 创建议题，参数同 REST（无 `actor`） |
| `get_issue` | `{ "id": "..." }` |
| `list_issues` | `{ "status"?, "kind"?, "source"?, "assignee"?, "text"? }` |
| `comment_issue` | `{ "id", "body", "internal"?, "expected_version"? }` |
| `transition_issue` | `{ "id", "to", "resolution"?, "duplicate_of"?, "expected_version"? }` |
| `create_requirement` | 创建需求，参数同 REST（无 `actor`） |
| `get_requirement` | `{ "id": "..." }` |
| `list_requirements` | `{ "status"?, "owner"?, "text"? }` |
| `request_transition` | `{ "id", "to", "expected_version"?, "reason"? }` |

工具只操作当前项目。账号的项目角色不足时，`tools/call` 返回 `isError: true`，错误文本在 `content[0].text`，不产生 JSON-RPC 协议错误。

示例：

```bash
curl -s http://127.0.0.1:8080/mcp \
  -H "Authorization: Bearer $TOKEN" \
  -H "X-Foundry-Project: $PROJECT" \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

```bash
curl -s http://127.0.0.1:8080/mcp \
  -H "Authorization: Bearer $TOKEN" \
  -H "X-Foundry-Project: $PROJECT" \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
        "name":"get_issue",
        "arguments":{"id":"<uuid>"}
      }}'
```

## 人类控制台

- `GET /login`、`POST /login`：登录；成功后写入会话 Cookie 并跳转到 `next`（仅允许站内路径）。
- `POST /logout`：撤销会话并清除 Cookie。
- `GET /`：工作台，汇总当前项目的议题和需求，并提供项目切换与侧边导航。
- `GET /issues`：议题收件箱，带「新建议题」入口。
- `GET /issues/new`：新建议题表单。
- `POST /issues`：表单提交（`application/x-www-form-urlencoded`）。成功返回 `303 See Other` 并跳转到新议题详情；校验失败返回 `422` 并回填表单。
- `GET /issues/{id}`：议题详情、讨论和状态转换表单。
- `POST /issues/{id}/comments`：添加评论。
- `POST /issues/{id}/transitions`：执行状态转换。
- `GET /requirements`：需求列表（早期实现）。
- `GET /requirements/new`、`POST /requirements`、`GET /requirements/{id}`：需求创建和查看。
- `GET /projects`、`POST /projects`：项目列表和创建。
- `POST /projects/select`：切换当前项目 Cookie。
- `GET /projects/{id}`：项目设置与成员管理。
- `POST /projects/{id}/edit`、`POST /projects/{id}/members`、成员角色/移除表单。
- `POST /projects/{id}/delete`：删除项目及其关联数据；需要 Owner 或全局管理员。
- `GET /projects/{id}/brief`、`POST /projects/{id}/brief/drafts`、`POST /projects/{id}/brief`：项目简报、创建草稿、保存/提交草稿；表单内部拆成结构化 operations。
- `GET /accounts`、`GET /accounts/{id}`、账户创建和修改（Admin）。
- `GET /workflows`：流程列表；人类账户可新建或从平台版本复制。
- `POST /workflows`、`POST /workflows/{id}/rename`、`POST /workflows/{id}/publish`：表单创建、重命名和发布。
- `GET /workflows/editor/{id}`：可视化编辑器；历史版本在 `/workflows/editor/{id}/versions/{n}`。编辑器是独立 SPA，需要部署 `web/workflow-editor/dist`。

控制台是服务端渲染，不需要 JavaScript 构建。页面按钮按项目角色隐藏或禁用；服务端仍会独立校验，不能靠隐藏按钮绕过权限。

## 需求文档（RFC 0020 第一切片）

每个项目一份结构化需求文档。人类文档是投影；Agent 通过结构化操作修改。

```text
GET  /api/v1/projects/{project}/requirements-document
GET  /api/v1/projects/{project}/requirements-document/draft
POST /api/v1/projects/{project}/requirements-document/draft/operations
POST /api/v1/projects/{project}/requirements-document/draft/validate
POST /api/v1/projects/{project}/requirements-document/draft/publish
GET  /api/v1/projects/{project}/requirements-document/versions
GET  /api/v1/projects/{project}/requirements-document/versions/{version}
```

操作和发布使用 `If-Match: "<revision>"`；冲突返回 `409 draft_conflict`。操作请求体：

```json
{
  "operations": [
    {
      "op_id": "uuid",
      "type": "user_add",
      "user": { "id": "persona-1", "name": "张先生", "goals": ["快速找到商品"] }
    },
    {
      "op_id": "uuid",
      "type": "function_add",
      "function": { "id": "func-shopping", "name": "购物" }
    },
    {
      "op_id": "uuid",
      "type": "operation_add",
      "function_id": "func-shopping",
      "operation": { "id": "op-search", "name": "搜索", "actor_user_ids": ["persona-1"] }
    },
    {
      "op_id": "uuid",
      "type": "story_add",
      "function_id": "func-shopping",
      "story": {
        "id": "story-search",
        "title": "搜索商品",
        "user_ids": ["persona-1"],
        "scenario": "客户想买某件商品",
        "goal": "找到商品",
        "operation_ids": ["op-search"]
      }
    }
  ]
}
```

同一 `op_id` 重放是幂等的。发布前校验结构、引用、覆盖和外部依赖；错误阻止发布。

MCP 工具：`get_requirements_document`、`get_requirements_draft`、`apply_requirements_operations`、`validate_requirements_document`、`publish_requirements_document`。

## 项目简报（ProjectBrief）

项目澄清的产物，按版本保存。编辑在服务端草稿上进行；每次创建草稿都会生成一个新的 `draft_id`，草稿只对创建者可见。提交草稿会生成新版本并删除草稿，但**不会推进 Task**。

```text
GET  /api/v1/projects/{project}/brief
GET  /api/v1/projects/{project}/brief/versions
GET  /api/v1/projects/{project}/brief/versions/{version}

POST /api/v1/projects/{project}/brief/drafts
GET  /api/v1/projects/{project}/brief/drafts
GET  /api/v1/projects/{project}/brief/drafts/{draft_id}
POST /api/v1/projects/{project}/brief/drafts/{draft_id}/operations
POST /api/v1/projects/{project}/brief/drafts/{draft_id}/submit

POST /api/v1/projects/{project}/brief/approve
GET  /api/v1/projects/{project}/brief/annotations?include_resolved=false
POST /api/v1/projects/{project}/brief/annotations
POST /api/v1/projects/{project}/brief/annotations/{annotation_id}/resolve
```

草稿编辑每次只接受一个 operation：

```text
set_problem            { value }                 // 背景：现状、需求、痛点、价值
set_goals              { values: string[] }       // 描述性目标，按重要程度排序，第一条为核心目标；不写量化标准
set_scope              { in_scope: string[], out_of_scope: string[] }  // 非目标紧跟目标；一次提交
set_constraints        { values: string[] }
set_risks              { values: Risk[] }
set_terms              { values: Term[] }        // Term = { term, definition }
set_budget             { value: Budget | null }
```

请求体：

```json
{
  "op_id": "uuid",
  "base_revision": 3,
  "operation": { "type": "set_goals", "values": ["支持部分退款"] }
}
```

`op_id` 幂等；`base_revision` 必须等于草稿当前 revision。提交草稿和批准使用 `If-Match`：

- `POST .../drafts/{draft_id}/submit`，`If-Match: "<draft revision>"`
- `POST .../approve`，`If-Match: "<version>"`

提交后生成新版本，旧当前版本变为 `superseded`，草稿被删除。**提交成功后必须发送 `task_operation(operation = "completed")`，否则 project_brief_writing 任务仍停在 brief。**

批注用于文风/内容评审：`target_type` 可取值 `document`、`problem`、`goals`、`scope`、`constraints`、`risks`、`terms`、`budget`；`target_id` 和 `field` 可选，用于定位具体风险、术语或字段。批注正文非空，解决时必须填写 `resolution`。

状态：版本状态为 `in_review → approved / superseded`。提交和批准前校验背景、目标和 non-goals；项目简报不包含成功指标，指标由需求分析阶段定义。Agent 不能批准。

MCP 工具：`get_project_brief`、`list_project_brief_versions`、`get_project_brief_version`、`create_project_brief_draft`、`list_project_brief_drafts`、`get_project_brief_draft`、`apply_project_brief_draft_operation`、`submit_project_brief_draft`、`approve_project_brief`、`list_project_brief_annotations`、`annotate_project_brief`、`resolve_project_brief_annotation`。

## Task 链接

Task 可以附加指向 Issue、需求文档、设计文档、ADR、契约、决策、其他 Task、Artifact 或外部 URL 的链接。

```text
GET    /api/v1/tasks/{id}/links
POST   /api/v1/tasks/{id}/links
DELETE /api/v1/tasks/{id}/links/{link_id}
```

请求体：

```json
{
  "kind": "url",
  "url": "https://example.com/refund-spec",
  "title": "退款规范",
  "relation": "references",
  "metadata": {}
}
```

内部对象使用 `target_id`：

```json
{
  "kind": "issue",
  "target_id": "<issue-uuid>",
  "title": "项目澄清",
  "relation": "clarifies"
}
```

规则：`kind=url` 必须有 `url`；其他 kind 必须有 `target_id`；同一 Task 下相同 `(kind, relation, target/url)` 去重。
