# 运行手册

## 本地启动

```bash
cd projects/foundry
docker compose -f deploy/compose/docker-compose.yml up -d postgres
export DATABASE_URL=postgres://foundry:foundry@localhost:5432/foundry
# 控制平面启动同目录下的 foundry-agentd；只重建控制平面会让 agentd 停留在旧版本
cargo build --bins
cargo run -p foundry-control-plane
```

首次启动时账户库为空，打开 <http://127.0.0.1:8080/setup>，先用配置中的初始账号和初始密码登录初始化流程，再创建首个管理员。没有默认账户、默认密码或默认项目；初始身份只用于授权初始化，不是系统账户，也不是管理员密码。创建管理员后会进入项目页，由管理员显式创建第一个项目。

数据库连接串可以放在用户配置文件 `~/.foundry/foundry.toml` 中，参考 `foundry.toml.example`：

```toml
addr = "127.0.0.1:8080"
database_url = "postgres://foundry:foundry@localhost:5432/foundry"
bootstrap_handle = "admin"
bootstrap_name = "Administrator"
bootstrap_password = "change-this-initialization-passphrase"
session_ttl_hours = 168
editor_dir = "web/workflow-editor/dist"
```

也可以继续用 `DATABASE_URL` 环境变量。环境变量优先于配置文件；配置文件优先于内置默认值。默认配置文件是 `~/.foundry/foundry.toml`，不存在时跳过；`FOUNDRY_CONFIG` 可指定其他路径（此时文件必须存在）。配置文件可能含密码，建议放在权限为 700 的目录中并 `chmod 600`。未知键会导致启动失败。旧版本的 `project` / `project_name` 键会被接受但忽略，启动时打印警告，可以删除。

默认监听 `127.0.0.1:8080`。启动时自动执行 `migrations/`。

引导规则：

- **账户表为空**：必须配置 `bootstrap_password`（初始密码），否则拒绝启动。服务只提供 `/setup` 与 `/health`；`/setup` 第一步校验配置中的初始账号和密码，第二步才允许操作者设置首个管理员的账号和密码。初始身份不是系统账户，不写入数据库，后续不能用于登录。管理员创建完成后不生成项目，操作者需要在项目页创建第一个项目。
- **已有账户**：`/setup` 只重定向到 `/login`，不会再创建或提升账户；初始账号/密码被忽略。没有活跃管理员时拒绝启动，需要在数据库中恢复一个管理员账户。

之后启动会复用已有账户和项目。数据库不可用时进程直接退出，不会退回到内存存储。

| 变量 | 默认值 | 说明 |
|---|---|---|
| `FOUNDRY_CONFIG` | `~/.foundry/foundry.toml`（存在时） | 配置文件路径 |
| `DATABASE_URL` | 无（必填） | Postgres 连接串；缺失时启动失败 |
| `FOUNDRY_ADDR` | `127.0.0.1:8080` | 监听地址 |
| `FOUNDRY_BOOTSTRAP_HANDLE` | `admin` | 初始化登录的初始账号；不是系统账户 |
| `FOUNDRY_BOOTSTRAP_NAME` | `Administrator` | 新建管理员表单预填的显示名 |
| `FOUNDRY_BOOTSTRAP_PASSWORD` | 无 | 与初始账号配对的初始密码；账户表为空时必填，用于登录 `/setup`；不是管理员密码，账户表非空后忽略 |
| `FOUNDRY_SESSION_TTL_HOURS` | `168` | 登录会话有效期（小时） |
| `FOUNDRY_EDITOR_DIR` | `web/workflow-editor/dist` | 流程编辑器构建产物；缺失时 `/workflows/editor/` 返回 503 |
| `RUST_LOG` | `foundry=info,tower_http=info` | 日志过滤 |

初始账号/密码和会话 token 不会写入日志；配置文件中的数据库密码和初始密码不要提交到仓库。

## 开发检查

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Postgres 集成测试需要独立的测试库，不能指向运行时数据库（测试会写入账户和项目）。首次创建：

```bash
createdb -h 127.0.0.1 -U foundry foundry_test
TEST_DATABASE_URL=postgres://foundry:foundry@127.0.0.1:5432/foundry_test cargo test --workspace
```

未设置 `TEST_DATABASE_URL` 时这些测试会跳过；指向名为 `foundry` 的库时测试直接失败。

或：

```bash
cargo xtask check
```

## 手工验收

### 通过 Web 控制台处理议题

1. 打开 `http://127.0.0.1:8080/`，未登录会跳转到 `/login`。
2. 首次启动先打开 `http://127.0.0.1:8080/setup`，用配置中的初始账号和密码登录初始化流程，再创建管理员；创建完成后进入空的项目页，创建第一个项目。之后用新管理员账号登录（密码由用户在创建页面设置，系统不会覆盖已有账户）。
3. 左侧菜单进入「议题」收件箱，点「＋ 新建议题」。
4. 提交后跳转到议题详情；发表评论，再用状态转换表单推进。
5. 侧边栏可切换项目；「项目」页管理成员和角色，「账户」页创建/停用账户。
6. 退出会在服务端撤销会话，不只是删除浏览器 Cookie。

校验失败时页面会回填已输入内容并显示原因。按钮会按角色隐藏，但服务端仍会独立校验。

### 查看智能体日志

智能体进程使用普通的 `tracing` 日志（stdout/stderr），控制平面把这些输出原样写入：

```text
~/.foundry/logs/agents/<agent-definition-id>.log
```

管理员在「智能体」页点某个智能体的「日志」，或直接打开 `/agents/{id}/logs`，可查看该文件最近 256 KB 的纯文本日志。页面只负责显示，不解析、不分条、不转成 JSON；需要更早的内容或完整文件时直接看上面的路径。

每次启动进程前，如果文件超过 5 MB 会被改名为 `<id>.log.1`，页面只看当前文件，上一份仍保留在磁盘上。

### 任务派发

PM Agent 每次扫描：

1. `tasks_ready_list`：每个任务一行，携带 workflow 中**所有** agent 角色（不只当前阶段）和已分配的 agent；
2. 对没有 enabled agent 的 agent 角色，分配已有 Agent 或按模板创建后再分配；
3. 调用 `task_start`（保留操作 `start`）把任务从 `Pending` 置为 `InProgress`。

`task_start` 是平台通用操作，不是 workflow 定义的；它只接受 Ready 任务，且要求所有 agent 角色都已有 enabled 的分配。所有角色在任务启动前一起绑定；之后进入新的 stage 时不会重新走 PM 分配。后续阶段角色的 task actor 先以 inactive 状态绑定，进入对应 stage 时再激活。领域层同样拒绝 `Pending → InProgress`：任一 agent 角色的绑定数低于 `min_actors` 时任务不能进入执行状态。没有 agent 角色的任务不进入 PM 队列。

任务进入 `InProgress` 后，Agent 进程在启动、重连和收到 `foundry_task_changes` 通知时，查询自己是 active actor 的任务，并自行决定是否执行。通知是瞬时的：不写 durable queue、不重试、不补发；通知丢失时，下一次通知或 Agent 重启会重新扫描。`issue_commented` 也走同一通知通道，因此人类在 Issue 回复会唤醒 Agent。

AgentRun 队列、lease、checkpoint 和运行历史已移除；Agent 执行只写进程日志，不再写 `agent_runs` / `agent_steps`。

启动时，control-plane 会对所有 `in_progress` Task 执行一次 transition 重算：按当前 stage 变量和 active actors 重新求值，条件满足则推进 stage 或完成任务。正常运行时 transition 只在 TaskEvent 处理过程中检查，这个启动扫描只用于恢复“已持久化的变量满足转移条件、但服务在事件后停止”的任务。

### 项目文档撰写

项目创建后，根 Task 创建 `goal_clarification` 子 Task 和统一的澄清 Issue：

1. PM 在任务开始时按 `goal_clarification:clarifier` 和 `goal_clarification:writer`
   分配或创建 **Clarifier** 和 **Project Document Writer**；
2. Clarifier 在澄清 Issue 中与创建者对话，只负责提问；澄清完毕后发送总结评论并调用
   `task_operation(operation = "clarification:completed")` 进入项目文档撰写；
3. Writer 读取澄清 Issue、项目信息和确认过的结论，用 `document` 工具维护项目文档
   （定位、背景、目标、非目标、分期计划、约束、风险、术语表），再用 `validate` 校验；
   写完后调用 `task_operation(operation = "writing:completed")` 进入文风审查；
4. 项目文档依次经过 **Project Layer Style Reviewer** 和 **Project Layer Content Reviewer**
   （都是 Agent）；两个 Reviewer 不修改文档，用 `document` 工具 `action = "content"`
   阅读，用 `action = "annotate"` 写锚点标注，打回时任务回到 `writing`；
5. Writer 读取文档标注，逐条处理后修改草稿，再次发送 `completed`；
6. 两位 Reviewer 通过后，人类在 `/projects/{id}/documents/project` 批准当前草案。
   **批准即发布不可变版本**；control-plane 只在任务处于 `human_review` 时发送保留操作
   `accept`。第一个批准版本就是项目基线。

### 需求分析

项目文档通过文风评审、内容评审并由人类批准后，项目直接进入 `requirements_analysis`：

1. PM 按 `requirements_analysis:analyst` 分配或创建 **Requirements Analyst**；
2. Analyst 用 `document` 工具 `action = "content"` 读取项目文档，用 `action = "content"` / `"toc"` / `"draft_open"` / `"draft_create"` / `"add"` / `"update_text"` / `"update_title"` / `"delete"` / `"move"` / `"validate"`，`kind = "requirements"` 维护需求锚点文档（领域模型 / 用户肖像 / 故事 / 子系统 / 功能集 / 功能；交互设计和 UI 设计是后续阶段维护的子系统树，交互设计按子系统 → 功能集 → 功能组织并包含信息架构和页面清单，UI 设计按子系统 → 页面组织并包含设计稿图片）；
   人类和 Reviewer 可以在需求文档页写锚点标注，Analyst 用 `action = "annotations_list"` 读取，用 `action = "resolve_annotation"` 在修改后标记解决；
3. Analyst 提交后依次经过 Style review、Requirements review、Feasibility review（都是 Agent）；每个 Reviewer 用标注写具体意见，打回时任务回到 `work`，Analyst 读取标注修改后重新提交；
4. 人类在 `/projects/{id}/documents/requirements` 做最终 review：**批准当前草案**会生成新的不可变版本，或填写打回原因退回 Analyst。项目文档和需求文档分别审批，每个草案 revision 都需要审批；
5. 发布后 control-plane 发送保留操作 `accept`，任务以 `result = approved` 完成，项目进入 `interaction_design`；交互设计和 UI 设计分别完成并批准后进入 `architecture`。

### 架构设计流程

项目进入 `architecture` 后创建 `architecture_design` 子 Task：

1. PM 按 `architecture_design:architect` 分配或创建 **Architect**；
2. Architect 读取项目文档、需求文档和当前架构设计，用 `document`，`kind = "architecture"` 维护架构锚点文档（架构概览 / 系统上下文 / 组件 / 接口与契约 / 数据与存储 / 部署与运行 / 质量属性 / 关键决策 / 技术债 / 风险）；
3. Architect 提交后进入 Architecture review（`architecture_design:reviewer`）；Reviewer 用锚点标注写具体意见，打回时任务回到 `work`；
4. 人类在 `/projects/{id}/documents/architecture` 做最终 review：**批准当前草案**生成新的不可变版本，或填写打回原因退回 Architect；
5. 发布后 control-plane 发送保留操作 `accept`，任务以 `result = approved` 完成，项目进入 `roadmap`。

### 通过 API 验收

```bash
# 1. 健康检查（无需认证）
curl -s localhost:8080/health

# 2. 登录（ADMIN_PASSWORD 为首次启动时设置的管理员密码）
ADMIN_PASSWORD='<管理员密码>'
TOKEN=$(curl -s localhost:8080/api/v1/auth/login \
  -H 'content-type: application/json' \
  -d "{\"handle\":\"admin\",\"password\":\"$ADMIN_PASSWORD\"}" | jq -r .token)

# 首次项目由管理员显式创建，创建者成为 Owner
PROJECT=$(curl -s -X POST localhost:8080/api/v1/projects \
  -H "Authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{"name":"Payments"}' | jq -r .id)
AUTH=(-H "Authorization: Bearer $TOKEN" -H "X-Foundry-Project: $PROJECT")

# 3. 创建议题
curl -s localhost:8080/api/v1/issues "${AUTH[@]}" \
  -H 'content-type: application/json' \
  -d @fixtures/issues/partial-refund-bug.json

# 4. 推进（Bug 在创建时必须带 severity）
curl -s -X POST localhost:8080/api/v1/issues/<ID>/transitions "${AUTH[@]}" \
  -H 'content-type: application/json' -d '{"to":"in_progress"}'

# 5. 未登录应返回 401
curl -s -o /dev/null -w '%{http_code}\n' localhost:8080/api/v1/issues

# 7. 跨项目读取应返回 404
curl -s -o /dev/null -w '%{http_code}\n' \
  localhost:8080/api/v1/issues/<ID> \
  -H "Authorization: Bearer $TOKEN" \
  -H 'X-Foundry-Project: 00000000-0000-0000-0000-000000000001'

# 8. 打开控制台
xdg-open http://127.0.0.1:8080/
```

创建 Agent 账户并验证角色限制：

```bash
curl -s -X POST localhost:8080/api/v1/accounts "${AUTH[@]}" \
  -H 'content-type: application/json' \
  -d '{"handle":"fixer","display_name":"Fixer","kind":"agent","role":"member","password":"fixer-password"}'

curl -s -X POST localhost:8080/api/v1/projects/$PROJECT/members "${AUTH[@]}" \
  -H 'content-type: application/json' -d '{"handle":"fixer","role":"member"}'

AGENT_TOKEN=$(curl -s localhost:8080/api/v1/auth/login \
  -H 'content-type: application/json' \
  -d '{"handle":"fixer","password":"fixer-password"}' | jq -r .token)

# 关闭议题应返回 403（项目角色 + DemoPolicy 双重限制）
curl -s -o /dev/null -w '%{http_code}\n' \
  -X POST localhost:8080/api/v1/issues/<ID>/transitions \
  -H "Authorization: Bearer $AGENT_TOKEN" -H "X-Foundry-Project: $PROJECT" \
  -H 'content-type: application/json' -d '{"to":"closed"}'
```

## CLI

```bash
export FOUNDRY_URL=http://127.0.0.1:8080

# 登录并保存 token（token 只显示一次）
export FOUNDRY_TOKEN=$(cargo run -q -p foundry-cli -- login admin "$ADMIN_PASSWORD" | jq -r .token)
export FOUNDRY_PROJECT="$PROJECT"

cargo run -q -p foundry-cli -- projects
cargo run -q -p foundry-cli -- accounts          # 仅 admin
cargo run -q -p foundry-cli -- issue list
cargo run -q -p foundry-cli -- issue get <ID>
cargo run -q -p foundry-cli -- issue create fixtures/issues/partial-refund-bug.json
cargo run -q -p foundry-cli -- issue comment <ID> "已复现"
cargo run -q -p foundry-cli -- issue transition <ID> --to in_progress
```

CLI 不再接受 `--actor-kind` / `--actor-id`：身份来自 `FOUNDRY_TOKEN`。

## 数据库

```bash
docker compose -f deploy/compose/docker-compose.yml up -d postgres
export DATABASE_URL=postgres://foundry:foundry@localhost:5432/foundry
```

控制平面启动时自动运行迁移，一般不需要 `sqlx migrate`。迁移：

- `0001_init.sql`：项目、工作项、事件、文档、决策、链接。
- `0002_issues.sql`：议题与评论。
- `0003_accounts_projects_auth.sql`：账户、会话、项目元数据、项目成员。
- `0004_drop_project_key.sql`：删除项目 key。
- `0005_drop_outbox.sql`：删除已废弃、无消费者的 `outbox` 表。
- `0006_tasks.sql`：Task、actor 绑定、子 Task 与事件投影。
- `0007_workflow_definitions.sql`：流程定义/版本/草稿/布局、Task 的 `workflow_version` 和项目 `root_task_id`；外键先以 `not valid` 创建，由启动引导校验。
- `0028_drop_work_items.sql`：删除已被需求文档取代的早期 `work_items` 表。
- `0029_requirements_approvals.sql`：需求文档审批记录。
- `0030_requirements_background.sql`：需求文档项目层字段（后续被 `0035` 移除）。
- `0034_project_documents.sql`：旧项目文档表；`0036` 之后接口不再读取，历史数据保留。
- `0035_remove_requirements_project_layer.sql`：从需求文档版本和草案中移除项目层字段；项目级内容由项目文档承载。
- `0036_anchor_documents.sql`：通用锚点文档、草案、版本、操作日志和审批记录。
- `0037_anchor_document_annotations.sql`：按锚点键和内容哈希记录的标注。
- `0038_agent_skills.sql`：智能体定义的技能 ID 列表。

存储布局：

- `issues.body`：聚合 JSONB 快照；标量列用于过滤和索引。
- `project_members`：项目成员与角色。
- `issue_comments`：评论投影，便于不反序列化聚合也能查询讨论。
- `sessions`：只保存 token 的 SHA-256；原始 token 只在登录响应中出现一次。
- `events`：领域事件的追加日志，是 `Task.operations` 等历史视图的数据源。
- 所有 `save` 使用 `UPDATE ... WHERE id = $1 AND version = $2`，零行结果映射为版本冲突或未找到。

## 工作流定义升级

流程定义随迁移 `0007` 引入。启动顺序是：运行迁移 → 播种 11 个平台 v1 → 检查所有 Task 的 pinned 版本 → 校验 `tasks_workflow_version_fk` → 回填 `projects.root_task_id`。

启动引导的硬性检查：

- 内置 id 已被账户定义占用 → 拒绝启动。
- 已存在的内置 v1 与嵌入 JSON 不一致 → 拒绝启动。已发布版本不可变，不能靠改 JSON 修复。
- 任何 Task 的 `(workflow, workflow_version)` 没有对应已发布版本 → 拒绝启动。
- `projects.root_task_id` 为空、或项目下不只有一个无父 Task 时不会自动修改，只打印警告；需要人工确认后设置。

升级步骤：

1. 备份数据库。
2. 同时部署控制面二进制和 `web/workflow-editor/dist`；只更新其中一个会让编辑器 503 或与 API 不匹配。
3. 启动并检查日志：除缺少唯一根 Task 的警告外，不应出现播种、版本固定或外键相关错误；出现时按下面的故障处理。
4. 打开 `/workflows` 确认 11 个平台流程存在且可进入编辑器。

故障处理：

- **内置 v1 不一致**：回退到与已播种 v1 匹配的二进制；或增加数据迁移发布新版本，绝不能直接修改 `workflow_versions` 中已有的 v1。
- **开发环境中的旧 AST**：本轮 AST 就地改写且 `schema_version` 仍为 1，旧定义（如 stage 带 `terminal` / `result`）无法反序列化。确认 `tasks` 为空后，删除 `workflow_drafts`、`workflow_layouts`、`workflow_versions`、`workflow_definitions` 的数据并重启，启动时会重新播种内置流程。不要在生产库这样做。
- **Task 指向未知版本**：为这些 id 插入显式定义和版本，或回滚部署；不要删除 Task 来“修复”外键。
- **项目没有唯一根 Task**：检查项目下的 Task 树；确认哪个 Task 是根后显式设置 `root_task_id`，不要删除 Task。
- **编辑器 503**：部署 `dist/` 或执行 `npm run build`，并确认 `FOUNDRY_EDITOR_DIR` 指向该目录。Docker 镜像会构建并打包编辑器，无需额外步骤。

修改内置流程的推荐方式见 [内置流程](../domain/builtin-workflows.md)；语法见 [AST 参考](../domain/workflow-ast.md)。

## 部署

Docker 构建上下文是仓库根目录（`software-dev/`），因为 Cargo workspace 在根上。镜像会在 Node 阶段构建流程编辑器并设置 `FOUNDRY_EDITOR_DIR`；根目录的 `.dockerignore` 排除 `target`、`node_modules` 和本地 `dist`。

```bash
cd /media/xuyan/work/software-dev
docker build -f projects/foundry/deploy/docker/Dockerfile -t foundry:dev .
docker compose -f projects/foundry/deploy/compose/docker-compose.yml up
```

不用 Docker 部署时，需要单独执行 `cd projects/foundry/web/workflow-editor && npm ci && npm run build`，否则 `/workflows/editor/` 返回 503。

首次启动（账户表为空）时服务只提供 `/setup`，必须先用配置中的初始账号和密码登录初始化流程，才能创建首个管理员；创建完成前其他功能不可用。当前没有 TLS，只应绑定本机或受信网络；初始密码不能替代 TLS，初始化应尽快完成并使用足够长的随机值。

## 故障排查

| 症状 | 检查 |
|---|---|
| 启动报 `DATABASE_URL must be set` | 在环境变量或 `~/.foundry/foundry.toml` 中设置 `database_url`，格式 `postgres://用户:密码@主机:5432/库名` |
| 启动报连接失败 / `Connection refused` | Postgres 是否已启动、端口和库名是否正确；容器部署检查 `depends_on` |
| 启动报迁移失败 | 看 `_sqlx_migrations` 和数据库权限；迁移需要建表权限 |
| 启动报 `Address already in use` | 改 `FOUNDRY_ADDR` 或结束占用端口的进程 |
| 启动报 `no initialization password is configured` | 账户表为空时必须配置 `bootstrap_password` / `FOUNDRY_BOOTSTRAP_PASSWORD` |
| API 返回 401 | token 缺失/过期/已撤销，或账户被停用；重新登录 |
| Web 一直跳回 `/login` | Cookie 是否被清除；会话是否已被登出或过期 |
| 返回 400 `project_required` | 账户没有任何可见项目；先创建项目或让 Owner 添加成员 |
| 返回 403 | 全局角色、项目角色或 DemoPolicy 拒绝；确认成员角色和 actor 类型 |
| 返回 404 但对象确实存在 | 对象属于另一个项目；检查 `X-Foundry-Project` / 项目 Cookie |
| 返回 409 `duplicate` | handle 已存在 |
| 转换返回 409 `invalid_transition` | 查看 `docs/domain/state-machines.md` 的允许转换表 |
| 创建返回 422 | Bug 缺少 severity；检查请求体 |
| 转换返回 409 `version_conflict` | 客户端持有的版本过期，重新 GET 后再提交 |
| MCP 返回 401 | `/mcp` 也需要 `Authorization: Bearer`，不再接受请求体 actor |
| MCP 返回 `-32601` | 方法名必须完全匹配；当前只支持 initialize/ping/tools/* |

## 已知限制

- 仍只适合本机/受信网络：无 TLS、无 CSRF token、无速率限制、无密钥轮换。
- 单实例：没有分布式锁；多实例部署需要额外的租约机制。
- 聚合写入与事件写入不在同一个事务中（两个端口分开调用）；进程在两者之间崩溃时可能出现“状态已变、事件缺失”。
- 事件只写入 `events`，不投递到外部系统。
- 会话撤销是逐条的；没有“撤销该账户全部会话”的管理界面。
