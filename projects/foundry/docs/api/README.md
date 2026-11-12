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

Issue、需求文档和 MCP 请求如果没有任何可见项目，返回 `400 project_required`。项目边界优先于一切：跨项目读取按 `404 not_found` 处理，不泄露对象是否存在。

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
- `GET /`：工作台，汇总当前项目的议题和任务，并提供项目切换与侧边导航。
- `GET /issues`：议题收件箱，带「新建议题」入口。
- `GET /issues/new`：新建议题表单。
- `POST /issues`：表单提交（`application/x-www-form-urlencoded`）。成功返回 `303 See Other` 并跳转到新议题详情；校验失败返回 `422` 并回填表单。
- `GET /issues/{id}`：议题详情、讨论和状态转换表单。
- `POST /issues/{id}/comments`：添加评论。
- `POST /issues/{id}/transitions`：执行状态转换。
- `GET /projects`、`POST /projects`：项目列表和创建。
- `POST /projects/select`：切换当前项目 Cookie。
- `GET /projects/{id}`：项目设置与成员管理。
- `POST /projects/{id}/edit`、`POST /projects/{id}/members`、成员角色/移除表单。
- `POST /projects/{id}/delete`：删除项目及其关联数据；需要 Owner 或全局管理员。
- `GET /accounts`、`GET /accounts/{id}`、账户创建和修改（Admin）。
- `GET /workflows`：流程列表；人类账户可新建或从平台版本复制。
- `POST /workflows`、`POST /workflows/{id}/rename`、`POST /workflows/{id}/publish`：表单创建、重命名和发布。
- `GET /workflows/editor/{id}`：可视化编辑器；历史版本在 `/workflows/editor/{id}/versions/{n}`。编辑器是独立 SPA，需要部署 `web/workflow-editor/dist`。

控制台是服务端渲染，不需要 JavaScript 构建。页面按钮按项目角色隐藏或禁用；服务端仍会独立校验，不能靠隐藏按钮绕过权限。

## 文档（锚点模型）

通用锚点文档把文档表示为“文档根 + 章节锚点 + 列表项锚点”的树。结构由 `DocumentSchema` 描述；每个锚点有稳定键 `key` 和内容哈希 `id`。当前注册了 `project`、`requirements` 和 `architecture` 三种类型。

```text
GET  /api/v1/projects/{project}/documents/{kind}?anchor={key|id}
GET  /api/v1/projects/{project}/documents/{kind}/draft      # open
POST /api/v1/projects/{project}/documents/{kind}/draft      # create
GET  /api/v1/projects/{project}/documents/{kind}/content?anchor={key|id}
POST /api/v1/projects/{project}/documents/{kind}/draft/operations
POST /api/v1/projects/{project}/documents/{kind}/draft/validate
GET  /api/v1/projects/{project}/documents/{kind}/draft/approvals
POST /api/v1/projects/{project}/documents/{kind}/draft/approvals
GET  /api/v1/projects/{project}/documents/{kind}/annotations
POST /api/v1/projects/{project}/documents/{kind}/annotations
POST /api/v1/projects/{project}/documents/{kind}/annotations/{id}/resolve
GET  /api/v1/projects/{project}/documents/{kind}/versions
GET  /api/v1/projects/{project}/documents/{kind}/versions/{version}
```

旧的 `/api/v1/projects/{project}/project-document` 接口已移除；控制台的 `/projects/{id}/project-document` 会重定向到 `/projects/{id}/documents/project`。

`anchor` 省略时返回根锚点。`GET /documents/{kind}` 返回 `text/markdown`：标题、元数据和 Markdown 目录；目录中每个条目带稳定位置键（`k:...`）。`content` 返回该锚点子树的 Markdown。单次章节由系统按描述文件预生成，`add` 只用于新增多项章节实例和列表项；新增多项章节时必须传 `title`，标题不从正文推导。已有锚点使用 `update_text`、`update_title`、`delete`、`move`，其中 `parent`/`anchor` 使用目录中的 `key`。`update_title` 只能修改多项章节的标题，单次章节和分组章节的标题固定。单次章节是结构骨架，不能删除；`delete` 只用于多项章节实例和列表项。一个多项章节必须是父节点的唯一子章节。`add` 的子类型由父锚点推断。操作使用 `If-Match: "<revision>"`。标注绑定锚点键和写入时的内容哈希；内容变化后状态为 `stale`，锚点消失后为 `orphaned`。

MCP 工具：`document`。通过 `action` 选择操作：`toc`、`draft_open`、`draft_create`、`content`、`add`、`update_text`、`update_title`、`delete`、`move`、`validate`、`annotations_list`、`annotate`、`resolve_annotation`、`approvals`、`versions`、`decide`。

Agent 工具：`document`，使用与 MCP 完全相同的 `action` 和参数。

控制台页面：`/projects/{id}/documents/{kind}`。

设计见 [锚点文档模型](../design/anchor-document-model.md)。

## 需求文档

每个项目一份需求文档，使用 `kind = "requirements"` 的锚点文档。章节按需求文档写作说明书分为：领域模型、用户肖像、故事、子系统（含功能集和功能）、交互设计、UI设计。交互设计按子系统 → 功能集 → 功能组织，并在子系统层包含信息架构和页面清单；UI 设计按子系统 → 页面组织，页面包含布局、组件、状态和设计稿图片。项目层内容属于项目文档，不在这里重复。

需求文档使用与项目文档完全相同的锚点文档接口：

```text
GET  /api/v1/projects/{project}/documents/requirements
GET  /api/v1/projects/{project}/documents/requirements/content
GET  /api/v1/projects/{project}/documents/requirements/draft
POST /api/v1/projects/{project}/documents/requirements/draft/operations
POST /api/v1/projects/{project}/documents/requirements/draft/validate
POST /api/v1/projects/{project}/documents/requirements/draft/approvals
GET  /api/v1/projects/{project}/documents/requirements/draft/approvals
GET  /api/v1/projects/{project}/documents/requirements/versions
GET  /api/v1/projects/{project}/documents/requirements/versions/{version}
```

草案操作、审批、标注和版本规则与项目文档相同。示例：向「用户」章节添加一个用户项，再修改其标题：

```json
{
  "operations": [
    {
      "op_id": "uuid-1",
      "type": "add",
      "parent": "users",
      "occurrence_key": "persona-1",
      "title": "张先生",
      "text": "姓名：张先生\n年龄：35 岁\n职业：买家\n诉求：随时查看订单状态\n主次：主要"
    },
    {
      "op_id": "uuid-2",
      "type": "update_title",
      "anchor": "users/user/persona-1",
      "title": "张先生（买家）"
    }
  ]
}
```

MCP 和 Agent 都使用统一的 `document` 工具，传入 `kind = "requirements"`。

## 架构设计

每个项目一份架构设计，使用 `kind = "architecture"` 的锚点文档。章节按架构设计说明书分为：架构概览、系统上下文、组件、接口与契约、数据与存储、部署与运行、质量属性、关键决策、技术债、风险；组件包含职责、接口、依赖和数据。

架构设计使用与项目文档完全相同的锚点文档接口：

```text
GET  /api/v1/projects/{project}/documents/architecture
GET  /api/v1/projects/{project}/documents/architecture/content
GET  /api/v1/projects/{project}/documents/architecture/draft
POST /api/v1/projects/{project}/documents/architecture/draft/operations
POST /api/v1/projects/{project}/documents/architecture/draft/validate
POST /api/v1/projects/{project}/documents/architecture/draft/approvals
GET  /api/v1/projects/{project}/documents/architecture/draft/approvals
GET  /api/v1/projects/{project}/documents/architecture/versions
GET  /api/v1/projects/{project}/documents/architecture/versions/{version}
```

草案操作、审批、标注和版本规则与项目文档相同。MCP 和 Agent 都使用统一的 `document` 工具，传入 `kind = "architecture"`。

## Task

Task 属于唯一项目，不能跨项目。`/api/v1/tasks/{id}` 及其操作、评论、历史和链接接口只按 `task_id` 解析所属项目，并校验当前账户对该项目可见；这些接口不使用 `X-Foundry-Project`。

任务操作使用 `stage:operation` 形式，例如 `human_review:approved`、`writing:completed`。引擎解析阶段名并校验它是否等于任务当前阶段；阶段不一致的操作会被丢弃。`allowed-operations` 返回的已经是带阶段前缀的操作名。

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
