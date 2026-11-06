# RFC 0001：Foundry MVP 纵向切片

- 状态：Accepted
- 日期：2026-11-02

## 目标

用一条可运行的纵向切片证明核心命题：**同一套领域对象，人类和智能体通过不同入口操作，流程约束和权限约束同时生效，所有变更可审计。**

2026-11-02 修订：先实现 **Issue（问题空间）** 全链路；Requirement 保留早期实现，后续按承诺空间重新设计。参见 [ADR 0004](../adr/0004-issue-vs-requirement.md)。

## 范围

### 包含

- `Issue` 聚合：评论、分诊、处置、状态机、版本号、状态历史。
- `Requirement` 聚合（早期实现）：校验、状态机、版本号、状态历史。
- `Task`、`DesignDoc`、`ADR`、`Contract`、`Decision`、`Link` 的领域模型与状态机。缺陷是 `IssueKind::Bug`，不单独建模。
- 用例层：Issue 创建、读取、列表、评论、分诊、转换；Requirement 旧用例保留。
- 账户与登录会话：账户、Argon2 密码、会话 token 哈希、Web Cookie / REST Bearer、引导管理员。
- 项目与隔离：项目成员及角色、Issue/Requirement 全链路 `project_id`、跨项目 404。
- 端口：`AccountRepository`、`SessionRepository`、`ProjectRepository`、`IssueRepository`、`RequirementRepository`、`EventSink`、`PolicyEngine`。
- 内存适配器（含乐观并发）、演示策略。
- HTTP API：REST + 服务端渲染控制台（登录、侧边导航、工作台、议题收件箱、项目与账户管理）。
- MCP：`initialize`、`tools/list`、`tools/call`（10 个工具）。
- CLI：议题 list / get / create / comment / triage / transition。
- Postgres 适配器：`accounts`、`sessions`、`projects`、`project_members`、`issues`、`issue_comments`、`work_items`、`events`；启动时自动迁移。

### 不包含

- OIDC / 外部身份提供商、TLS、CSRF token、速率限制。
- 文档 Git 同步。
- 任务租约、沙箱、CI 集成、发布门禁。
- 实时推送和前端框架。

## 关键接口

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/health` | 存活检查 |
| POST | `/api/v1/auth/login` | 登录，返回会话 token |
| POST | `/api/v1/auth/logout` | 撤销会话 |
| GET | `/api/v1/auth/me` | 当前账户 |
| GET/POST | `/api/v1/accounts` | 账户列表/创建（admin） |
| GET/PATCH | `/api/v1/accounts/{id}` | 账户详情/修改 |
| GET/POST | `/api/v1/projects` | 可见项目/创建项目 |
| GET/PATCH | `/api/v1/projects/{id}` | 项目详情/修改 |
| POST | `/api/v1/projects/{id}/members` | 添加成员 |
| PATCH/DELETE | `/api/v1/projects/{id}/members/{account_id}` | 改角色/移除 |
| GET | `/api/v1/issues` | 议题列表，支持 status/kind/source/assignee/text |
| POST | `/api/v1/issues` | 创建议题 |
| GET | `/api/v1/issues/{id}` | 议题详情 |
| POST | `/api/v1/issues/{id}/comments` | 评论 |
| POST | `/api/v1/issues/{id}/triage` | 分诊 |
| POST | `/api/v1/issues/{id}/transitions` | 状态转换 |
| GET | `/api/v1/requirements` | 需求列表（早期实现） |
| POST | `/api/v1/requirements` | 创建需求（早期实现） |
| GET | `/api/v1/requirements/{id}` | 需求详情（早期实现） |
| PATCH | `/api/v1/requirements/{id}` | 编辑（仅 Draft/InReview） |
| POST | `/api/v1/requirements/{id}/transitions` | 需求状态转换 |
| POST | `/mcp` | MCP JSON-RPC |
| GET | `/login`、`POST /login` | 登录页与登录提交 |
| GET | `/` | 工作台与全局导航 |
| GET/POST | `/projects` | 项目列表/创建 |
| GET | `/projects/{id}` | 项目设置与成员 |
| GET | `/accounts`、`/accounts/{id}` | 账户管理 |
| GET | `/issues` | 议题收件箱 |
| GET | `/issues/{id}` | 议题详情页 |

## 验收标准

1. `cargo test --workspace` 全绿，`cargo clippy -- -D warnings` 无告警。
2. 未登录访问 API 返回 401，Web 跳转登录；登录后 token 可用于 REST 与 MCP。
3. 项目之间 Issue 不可见；跨项目按 id 获取返回 404。
4. 项目 Viewer 可以读取但不能创建；Member 可以贡献但不能处置。
5. 人类可以创建议题、评论、分诊并确认关闭；智能体可以评论、分诊、推进到 Resolved，但关闭返回 403。
6. 使用过期 `expected_version` 的写入返回 409。
7. MCP 客户端能列出 10 个工具，并创建/查询/评论/分诊议题；工具参数不含 actor。
8. 控制台提供全局菜单和工作台；议题收件箱能列出议题，详情页能讨论、分诊和执行状态转换。
9. 需求旧流程仍然可用（`/requirements`）。

## 后续里程碑

1. **M1 持久化**：Postgres 适配器、迁移执行、事件持久化已完成；事件重放测试待做。
2. **M2 身份与权限**：登录会话、项目角色、API token 和 MCP 鉴权已完成；OIDC、OPA/Cedar 和公网加固待做。
3. **M3 执行面**：任务租约、worker、沙箱、CI 证据回传、合并队列。
4. **M4 决策与发布**：Decision API、审批流、发布门禁、自动回滚。
5. **M5 度量**：周期时间、返工率、证据覆盖率、智能体成本。

## 风险

- **认证已落地但未加固**：没有 TLS、CSRF token、速率限制或密钥轮换；仍只限本机/受信网络。
- **聚合与事件分开提交**：状态写入与事件发布是两个端口调用；进程中断可能丢失事件。需要把仓库和事件 sink 合并到同一事务。
- **策略硬编码**：规则变化需要发版；M2 迁移到 OPA。
