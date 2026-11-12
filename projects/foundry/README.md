# Foundry

AI 原生的软件工程控制平面。人和 AI 操作同一套领域对象，只是入口、权限和节奏不同。

Foundry 不是 Jira 加一个 AI 字段，也不是 IDE 插件。它是所有工程动作的必经入口：**记录状态、校验权限、要求证据、调用执行面**。Git、CI、沙箱、部署、模型推理都是它集成的外部执行面。

```
                    ┌──────────────────────────────┐
   Human ── Web ───►│                              │──► Git
   Human ── CLI ───►│   Foundry control plane      │──► CI
   Agent ── HTTP ──►│   state · policy · evidence  │──► Sandbox
   Agent ── MCP ───►│                              │──► Deploy
                    └──────────────────────────────┘──► Chat / Models
```

## 当前状态：可运行的纵向切片

已实现：

- Issue 聚合（问题空间）：反馈、缺陷、问题、想法、任务请求、事故；评论、处置、状态机、乐观并发、历史。
- **账户与登录会话**：人类账户、智能体账户、全局 Admin/Member 角色、Argon2 密码哈希、可撤销会话、Web Cookie 与 REST/MCP Bearer Token。
- **项目与隔离**：项目成员及 Viewer / Member / Maintainer / Owner 角色；Issue、项目文档和需求文档全部按项目隔离，跨项目读取返回 404。
- 人类控制台：登录、侧边导航、**工作台**、议题收件箱、**任务**、**项目文档**、**需求文档**、**架构设计**、项目切换、项目成员管理、账户管理（Admin）、**智能体管理（Admin）**。
- **智能体管理**：每个智能体一个常驻 `foundry-agentd` 进程；控制平面 supervisor 负责启动/停止/重启、心跳和进程状态；PM Agent 作为调度者把 Ready 任务纳入派发：按 role 分配 Agent，没有合适的就从平台模板创建，然后调用 `task_start` 把任务置为 `in_progress`；用户不配置智能体。
- **通用锚点文档引擎**：描述文件定义章节树；文档、章节和列表项都是锚点，每个锚点有稳定键和 SHA-256 内容 ID；共享草案、不可变版本、操作日志、审批、按项标注和乐观并发。
- 项目文档（承诺空间）：使用 `kind = project` 的锚点文档，包含定位（是什么、不是什么）、背景（现状、问题）、目标、非目标、分期计划、约束、风险和术语表。
- 需求文档：每个项目一份锚点需求文档，包含领域模型、用户肖像、故事、子系统（含功能集和功能）、交互设计、UI设计；交互设计按子系统 → 功能集 → 功能组织并包含信息架构和页面清单，UI 设计按子系统 → 页面组织并包含设计稿图片；与项目文档分开审批和发布。
- 架构设计：每个项目一份 `kind = architecture` 的锚点文档，包含架构概览、系统上下文、组件、接口与契约、数据与存储、部署与运行、质量属性、关键决策、技术债和风险；与项目文档和需求文档分别审批。
- Task / DesignDoc / ADR / Contract / Decision / Link 的领域模型与状态机。缺陷不再单独建模，它是 `IssueKind::Bug`。
- 用例层与端口：`AccountRepository`、`SessionRepository`、`ProjectRepository`、`IssueRepository`、`EventSink`、`PolicyEngine`。
- 内存适配器（测试用）与 **Postgres 适配器**（运行时不失数据、乐观并发、事件持久化）。
- HTTP REST API + 服务端渲染控制台 + MCP JSON-RPC。
- CLI：登录、项目/账户列表、议题 list / get / create / comment / transition、流程定义与项目创建。
- Postgres schema 与启动时自动迁移。

尚未实现：AgentDefinition 版本、评测和绑定审批，沙箱与 CI 集成。详见 [RFC 0015](docs/rfcs/0015-agent-architecture-and-evaluation.md)。

## 完整生命周期设计（设计目标）

当前实现是控制平面骨架。要覆盖从产品判断、需求、架构、设计、实现、测试、发布到运维、反馈、退役的完整软件生命周期，并让 Agent 架构本身成为被管理的对象，按以下文档实施：

| 文档 | 内容 |
|---|---|
| [RFC 0008](docs/rfcs/0008-software-lifecycle-model.md) | 生命周期总纲：阶段、对象分类、门禁、证据、阶段与流程映射 |
| [RFC 0009](docs/rfcs/0009-artifact-lineage-baselines-and-change.md) | 产物血缘、基线、变更请求、配置与版本 |
| [RFC 0010](docs/rfcs/0010-auxiliary-systems-and-evidence.md) | 辅助系统注册、能力契约、调用记录、证据与信任等级 |
| [RFC 0011](docs/rfcs/0011-requirements-management.md) | 历史：需求池、需求分析、验收场景、holdout、需求基线（已被 RFC 0020 取代） |
| [RFC 0012](docs/rfcs/0012-architecture-and-design-management.md) | 架构基线、ADR、契约、质量属性、约束、技术债、详细设计 |
| [RFC 0013](docs/rfcs/0013-implementation-review-testing-and-quality.md) | 实现、评审、测试、覆盖率、质量与安全门禁、豁免 |
| [RFC 0014](docs/rfcs/0014-release-deployment-operations-and-retirement.md) | 发布、部署、环境、运维、事故、复盘、反馈、退役 |
| [RFC 0015](docs/rfcs/0015-agent-architecture-and-evaluation.md) | Agent 定义、版本、工具、绑定、评测、观测、生命周期 |
| [RFC 0016](docs/rfcs/0016-metrics-cost-and-self-hosting.md) | 度量、成本、闭环层级 L1–L5、自举 |
| [RFC 0017](docs/rfcs/0017-product-portfolio-and-initiation.md) | 产品、组合、立项、目标澄清、成功指标 |
| [RFC 0018](docs/rfcs/0018-knowledge-documentation-and-context.md) | 知识、文档、代码索引、上下文组装、记忆 |
| [RFC 0019](docs/rfcs/0019-governance-security-compliance-and-data.md) | 治理、策略、审批、审计、安全、合规、风险、数据 |
| [RFC 0020](docs/rfcs/0020-requirements-subsystem.md) | 需求子系统：项目文档与需求文档、用户/功能/故事/操作、版本/草案/冲突 |

设计原则见 [ADR 0007](docs/adr/0007-management-backbone-and-auxiliary-systems.md)（管理主干与辅助系统分离）和 [ADR 0008](docs/adr/0008-artifact-lineage-baselines-change.md)（产物血缘、基线与变更是贯穿主线）。这些 RFC 是设计稿，不代表已实现；实施顺序见 RFC 0008 §13。

## 快速开始

需要 Rust 1.93.1 和 PostgreSQL 14+。

本工程是仓库根 Cargo workspace 的成员。可以在 `software-dev/` 根目录构建，也可以在工程目录内构建（Cargo 会自动向上找到根 workspace）：

```bash
# 启动本地 Postgres（首次会初始化数据卷）
docker compose -f projects/foundry/deploy/compose/docker-compose.yml up -d postgres

export DATABASE_URL=postgres://foundry:foundry@localhost:5432/foundry

# 或者：把仓库根目录的 foundry.toml.example 复制到
# ~/.foundry/foundry.toml 并修改（环境变量仍然优先）

# 在 software-dev/ 根目录
# 控制平面会启动同目录下的 foundry-agentd 子进程；修改代码后两者要一起重建
cargo build --bins
cargo run -p foundry-control-plane
```

控制平面启动时自动执行 `migrations/`，不需要手工建表。第一次启动时账户库为空，打开 <http://127.0.0.1:8080/setup>，先用配置中的初始账号和初始密码登录初始化流程，再创建首个管理员；创建完成后会进入空的项目页，由管理员显式创建第一个项目。系统不会自动创建项目。

### 配置文件

启动时按以下优先级读取配置：

1. 环境变量（最高）
2. 配置文件
3. 内置默认值

配置文件默认位于 `~/.foundry/foundry.toml`；不存在时跳过。用 `FOUNDRY_CONFIG=/path/to/foundry.toml` 可以指定其他路径（此时文件必须存在，否则启动失败）。未知键会被拒绝，避免拼写错误被静默忽略。旧版本的 `project` / `project_name` 键仍会被接受但忽略，启动时打印警告，可以删除。参考仓库根目录的 `foundry.toml.example`。配置文件可能含数据库密码、模型 API key 和初始化密码，建议放在权限为 700 的目录中并 `chmod 600`。

```toml
addr = "127.0.0.1:8080"
database_url = "postgres://foundry:foundry@localhost:5432/foundry"
# 初始账号：只用于授权首次初始化，不是系统账户。
bootstrap_handle = "admin"
# 新建管理员表单预填的显示名。
bootstrap_name = "Administrator"
# 初始密码：与 bootstrap_handle 配对，账户表为空时用于登录初始化流程；
# 不是管理员密码。
bootstrap_password = "change-this-initialization-passphrase"
session_ttl_hours = 168

# 平台智能体使用的模型。环境变量 OPENAI_* 优先；API key 不会写入数据库。
openai_api_key = "sk-..."
openai_base_url = "https://api.deepseek.com/v1"
openai_model = "deepseek-chat"
```

首次启动时账户库为空，浏览器打开 `/setup`。第一步用配置中的 `bootstrap_handle` 和 `bootstrap_password` 登录初始化流程，确认操作者持有配置文件；这一步不会创建账户。第二步由操作者设置首个管理员的账号和密码，创建成功后自动登录并进入项目页。初始账号/密码只是授权身份，不会成为管理员账户或管理员密码，不写入数据库，也不用于后续登录；`bootstrap_name` 只是新建管理员表单的显示名预填值。首个项目不是自动生成的：管理员在项目页创建，创建者成为 Owner。下表列出环境变量；同名的配置文件键（小写，如 `database_url`）作用相同，环境变量优先：

| 环境变量 | 默认值 | 说明 |
|---|---|---|
| `FOUNDRY_CONFIG` | `~/.foundry/foundry.toml`（存在时） | 配置文件路径 |
| `DATABASE_URL` | 无（必填） | Postgres 连接串，如 `postgres://foundry:foundry@localhost:5432/foundry` |
| `FOUNDRY_ADDR` | `127.0.0.1:8080` | 监听地址 |
| `FOUNDRY_BOOTSTRAP_HANDLE` | `admin` | 初始化登录的初始账号；不是系统账户 |
| `FOUNDRY_BOOTSTRAP_NAME` | `Administrator` | 新建管理员表单预填的显示名 |
| `FOUNDRY_BOOTSTRAP_PASSWORD` | 无 | 与初始账号配对的初始密码；账户表为空时必填，用于登录 `/setup`；不是管理员密码，账户表非空后忽略 |
| `FOUNDRY_SESSION_TTL_HOURS` | `168` | 登录会话有效期 |
| `OPENAI_API_KEY` | 无 | 平台智能体模型 key；也可写 `openai_api_key`。不落库 |
| `OPENAI_BASE_URL` | 平台默认 `http://127.0.0.1:11434/v1` | 平台智能体模型 base URL；也可写 `openai_base_url` |
| `OPENAI_MODEL` | 平台默认 `qwen2.5-coder` | 平台智能体模型名；也可写 `openai_model` |

引导规则：

- **账户表为空**：必须配置 `bootstrap_password`（初始密码），否则拒绝启动；服务除 `/setup` 与 `/health` 外全部不可用（Web 重定向到 `/setup`，API/MCP 返回 `503 setup_required`）。`/setup` 第一步校验配置中的初始账号和密码；通过后第二步才允许创建首个管理员，管理员账号和密码由操作者现场设置。初始身份不会成为账户。
- **已有账户**：`/setup` 只重定向到 `/login`，不会再创建或提升账户；初始账号/密码被忽略。没有活跃管理员时启动失败，需要在数据库中恢复一个管理员账户。

系统没有默认账户、默认密码或默认项目；初始账号/密码只授权“创建首个管理员”这两步，项目由管理员创建。

访问：

- `http://127.0.0.1:8080/login` — 登录
- `http://127.0.0.1:8080/` — 工作台（人类控制台）
- `http://127.0.0.1:8080/issues` — 议题收件箱
- `http://127.0.0.1:8080/projects` — 项目列表与成员管理
- `http://127.0.0.1:8080/accounts` — 账户管理（Admin）
- `http://127.0.0.1:8080/api/v1/issues` — 议题 REST API
- `http://127.0.0.1:8080/mcp` — MCP JSON-RPC 入口
- `http://127.0.0.1:8080/health` — 健康检查

## 第一个闭环

先登录拿一个短期会话 token：

```bash
export ADMIN_PASSWORD='change-this-password'   # 首次启动时设置的管理员密码
TOKEN=$(curl -s http://127.0.0.1:8080/api/v1/auth/login \
  -H 'content-type: application/json' \
  -d "{\"handle\":\"admin\",\"password\":\"$ADMIN_PASSWORD\"}" | jq -r .token)

# 首个项目由管理员显式创建；创建者成为 Owner
PROJECT=$(curl -s -X POST http://127.0.0.1:8080/api/v1/projects \
  -H "Authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{"name":"Payments"}' | jq -r .id)

AUTH=(-H "Authorization: Bearer $TOKEN" -H "X-Foundry-Project: $PROJECT")
```

`X-Foundry-Project` 是项目 UUID，决定当前项目；Web 控制台使用 `foundry_project` Cookie。没有该头时使用第一个可见的活跃项目。

```bash
# 1. 报告一个议题
curl -s http://127.0.0.1:8080/api/v1/issues "${AUTH[@]}" \
  -H 'content-type: application/json' \
  -d @fixtures/issues/partial-refund-bug.json

# 2. 评论
curl -s -X POST http://127.0.0.1:8080/api/v1/issues/<ID>/comments "${AUTH[@]}" \
  -H 'content-type: application/json' \
  -d '{"body":"已复现，退款失败率约 5%。"}'

# 3. 推进并报告解决
curl -s -X POST http://127.0.0.1:8080/api/v1/issues/<ID>/transitions "${AUTH[@]}" \
  -H 'content-type: application/json' \
  -d '{"to":"in_progress"}'
curl -s -X POST http://127.0.0.1:8080/api/v1/issues/<ID>/transitions "${AUTH[@]}" \
  -H 'content-type: application/json' \
  -d '{"to":"resolved","resolution":"修复金额校验，已部署 1.4.1"}'

# 4. 关闭
curl -s -X POST http://127.0.0.1:8080/api/v1/issues/<ID>/transitions "${AUTH[@]}" \
  -H 'content-type: application/json' \
  -d '{"to":"closed"}'
```

让智能体参与：管理员创建一个 Agent 账户并加入项目。项目角色决定它能做什么。

```bash
# 创建智能体账户（Admin）
curl -s -X POST http://127.0.0.1:8080/api/v1/accounts "${AUTH[@]}" \
  -H 'content-type: application/json' \
  -d '{"handle":"fixer","display_name":"Fixer","kind":"agent","role":"member","password":"fixer-password"}'

# 加入项目，角色为 Member（可贡献，不可最终处置）
curl -s -X POST http://127.0.0.1:8080/api/v1/projects/$PROJECT/members "${AUTH[@]}" \
  -H 'content-type: application/json' \
  -d '{"handle":"fixer","role":"member"}'

# 智能体登录自己的 token
AGENT_TOKEN=$(curl -s http://127.0.0.1:8080/api/v1/auth/login \
  -H 'content-type: application/json' \
  -d '{"handle":"fixer","password":"fixer-password"}' | jq -r .token)

# 智能体可以评论、推进、报告解决；关闭会被 403 拒绝
curl -s -o /dev/null -w '%{http_code}\n' \
  -X POST http://127.0.0.1:8080/api/v1/issues/<ID>/transitions \
  -H "Authorization: Bearer $AGENT_TOKEN" -H "X-Foundry-Project: $PROJECT" \
  -H 'content-type: application/json' -d '{"to":"closed"}'
```

用 CLI：

```bash
export FOUNDRY_TOKEN=$(cargo run -q -p foundry-cli -- login admin "$ADMIN_PASSWORD" | jq -r .token)
export FOUNDRY_PROJECT="$PROJECT"

cargo run -q -p foundry-cli -- projects
cargo run -q -p foundry-cli -- issue list
cargo run -q -p foundry-cli -- issue create fixtures/issues/partial-refund-bug.json
cargo run -q -p foundry-cli -- issue transition <ID> --to in_progress
```

用 MCP（Bearer token，项目由请求头决定）：

```bash
curl -s http://127.0.0.1:8080/mcp \
  -H "Authorization: Bearer $TOKEN" \
  -H "X-Foundry-Project: $PROJECT" \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

## 约束模型

一个状态转换要同时通过四道检查：

1. **项目边界**：对象属于当前项目吗？跨项目访问按不存在处理。
2. **状态机**：这个转换存在吗？
3. **策略**：这个 actor 有权做吗？（全局角色 + 项目角色）
4. **证据**：支撑转换的材料齐全吗？（如批准要求 users 和 non_goals）

项目角色：

| 角色 | 读取 | 创建 / 评论 / 推进 | 处置 / 状态转换 | 管理成员 |
|---|---|---|---|---|
| Viewer | ✅ | ❌ | ❌ | ❌ |
| Member | ✅ | ✅ | ❌ | ❌ |
| Maintainer | ✅ | ✅ | ✅ | ❌ |
| Owner | ✅ | ✅ | ✅ | ✅ |
| 全局 Admin | ✅ | ✅ | ✅ | ✅ |

默认策略仍然是第二层约束：

| 动作 | Human | Agent |
|---|---|---|
| 创建议题 / 评论 | ✅ | ✅ |
| Open → InProgress → Resolved | ✅ | ✅ |
| Resolved → Closed | ✅ | ❌ |
| Rejected / Duplicate / 重开议题 | ✅ | ❌ |
| 创建需求 | ✅ | ✅ |
| Draft → InReview | ✅ | ✅ |
| Approved → InDesign → InDevelopment → InVerification | ✅ | ✅ |
| InReview → Approved | ✅ | ❌ |
| Rejected / Deferred / Superseded / Done | ✅ | ❌ |

智能体可以干活、讨论和报告结果，但不能自我批准需求，也不能替人类关闭或拒绝议题。策略在 `crates/adapters/policy`，生产环境应替换为 OPA/Cedar。

## 架构

```
domain ← app ← adapters
             ← api-http / api-mcp / web / cli
services/* → 组合根
```

- `crates/domain`：纯领域模型，无 I/O、无 async、无 Web 框架。
- `crates/app`：用例、端口 trait、密码/会话、策略检查。
- `crates/adapters/*`：内存、Postgres、Git、策略、对象存储。
- `crates/api-*`：入站适配器（HTTP、MCP）。
- `crates/web`：服务端渲染的人类控制台。
- `crates/agent-runtime`：Foundry 自己的 Agent 运行时（模型/工具/轨迹/预算/循环）；Phase 0 骨架。
- `web/workflow-editor`：独立的流程编辑器 SPA（Vite + React + React Flow）；构建产物部署到 `/workflows/editor/`。
- `services/*`：可执行程序。
- `migrations/`：Postgres schema（`accounts`、`sessions`、`projects`、`project_members`、`issues`、`events`、`decisions`、`links`、`tasks`、`workflow_definitions`、`workflow_versions`、`workflow_drafts`、`workflow_layouts`）。

## 开发

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# 或
cargo xtask check
```

## 文档

- [RFC 索引](docs/rfcs/README.md)：阅读顺序、状态和依赖关系
- [领域模型](docs/domain/model.md)
- [状态机](docs/domain/state-machines.md)
- [流程定义 AST](docs/domain/workflow-ast.md)
- [内置流程](docs/domain/builtin-workflows.md)
- [术语表](docs/domain/glossary.md)
- [API / MCP](docs/api/README.md)
- [运行手册](docs/operations/runbook.md)
- [流程编辑器](docs/operations/workflow-editor.md)
- ADR：[0001 Rust workspace 与六边形架构](docs/adr/0001-rust-workspace-and-hexagonal-architecture.md) · [0002 控制平面形态](docs/adr/0002-control-plane-form.md) · [0003 Git 存内容，Postgres 存状态](docs/adr/0003-git-content-postgres-state.md) · [0004 Issue 是问题空间，Requirement 是承诺空间](docs/adr/0004-issue-vs-requirement.md) · [0005 登录会话与身份派生](docs/adr/0005-login-sessions-and-identity.md) · [0006 项目隔离](docs/adr/0006-project-isolation.md) · [0007 管理主干与辅助系统分离](docs/adr/0007-management-backbone-and-auxiliary-systems.md) · [0008 产物血缘、基线与变更是贯穿主线](docs/adr/0008-artifact-lineage-baselines-change.md)
- [MVP RFC](docs/rfcs/0001-foundry-mvp.md)
- [项目流程、Issue 流程与权限策略技术方案](docs/rfcs/0002-project-issue-workflow-and-policy.md)（Task/流程部分已被 RFC 0003 取代）
- [Task Workflow 模型与项目运行设计](docs/rfcs/0003-task-workflow-model.md)
- [持久事件总线](docs/rfcs/0004-durable-event-bus.md)
- [Workflow 定义、版本与可视化编辑](docs/rfcs/0005-workflow-definitions.md)
- [Workflow 变量、Operation、Event 与转移](docs/rfcs/0006-workflow-variables.md)
- [Workflow 脚本语法](docs/rfcs/0007-workflow-script-syntax.md)
- 完整生命周期设计：[RFC 0008](docs/rfcs/0008-software-lifecycle-model.md) · [0009 产物血缘、基线与变更](docs/rfcs/0009-artifact-lineage-baselines-and-change.md) · [0010 辅助系统与证据](docs/rfcs/0010-auxiliary-systems-and-evidence.md) · [0011 需求管理](docs/rfcs/0011-requirements-management.md) · [0012 架构与设计管理](docs/rfcs/0012-architecture-and-design-management.md) · [0013 实现、评审、测试与质量](docs/rfcs/0013-implementation-review-testing-and-quality.md) · [0014 发布、部署、运维与退役](docs/rfcs/0014-release-deployment-operations-and-retirement.md) · [0015 Agent 架构与评测](docs/rfcs/0015-agent-architecture-and-evaluation.md) · [0016 度量、成本与自举](docs/rfcs/0016-metrics-cost-and-self-hosting.md) · [0017 产品、组合与立项](docs/rfcs/0017-product-portfolio-and-initiation.md) · [0018 知识、文档与上下文](docs/rfcs/0018-knowledge-documentation-and-context.md) · [0019 治理、安全、合规与数据](docs/rfcs/0019-governance-security-compliance-and-data.md) · [0020 需求子系统](docs/rfcs/0020-requirements-subsystem.md)

## 安全提示

身份由登录会话派生，请求体中的 `actor` 字段已移除。Web 使用 `HttpOnly`、`SameSite=Lax` 的 `foundry_session` Cookie；REST 与 MCP 使用 `Authorization: Bearer <token>`。会话 token 只在登录响应中出现一次，数据库只保存 SHA-256 哈希；密码使用 Argon2。

但这仍不是面向公网的部署：没有 TLS、没有 CSRF token、没有速率限制、没有密钥轮换。**只能在本机或受信网络运行。** 公网部署前必须补齐这些项。
