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

## 退出与残留进程

控制平面收到 Ctrl-C 或 SIGTERM 后会：

1. 停止 HTTP 服务并等待在途请求；
2. 对每个 `foundry-agentd` 发送 SIGTERM，并等待最多 5 秒；
3. 仍未退出的 agentd 用 SIGKILL 结束；
4. 按 `foundry.agent_id` 标签清理 agent 和 service 胶囊；
5. 正常 agentd 退出时也会通过 Drop 清理自己的胶囊。

控制平面启动时会读取 `agent_processes`，清理上次崩溃遗留、且 `/proc/<pid>/comm` 仍是 `foundry-agentd` 的进程和对应胶囊。agentd 还设置了 Linux `PR_SET_PDEATHSIG`：控制平面异常退出时内核会向它发送 SIGTERM。

手动检查：

```bash
pgrep -a foundry-agentd
podman ps -a --filter label=foundry.agent_id
```

## 本地源码仓库

可以从本地 git daemon 读取源码，避免依赖 GitHub 网络：

```bash
cd projects/foundry
scripts/dev-git-daemon.sh
```

脚本创建 `~/git/software-dev.git` 裸仓库，并在 `foundry-git` 容器里以**只读**方式提供：

```text
git://<host-ip>:9418/software-dev.git
```

把项目的 `git_repository` 设为该 URL。宿主仓库有新提交后同步裸仓库：

```bash
git -C /home/xuyan/work/software-dev push /home/xuyan/git/software-dev.git --all
```

切换 source URL 后，在工作区执行 `arche source set-url <url>`，再执行 `arche source fetch`。新工作区直接 `arche clone`，源码地址从设计空间自动继承。这个 daemon 没有认证，只应在受信网络使用；它只提供 fetch，不接受 push。

## 胶囊凭据布局

所有运行时凭据都在 `/run/foundry`（目录 0700，文件 0600）。智能体和任务都不传
凭据参数，工具直接读这些固定路径。

| 路径 | 作用域 | 注入/清理 |
|---|---|---|
| `/run/foundry/arche_key` | agent 账户，shell 生命周期 | 创建 shell 时由 agentd 从 `/api/v1/agent/arche-credential` 注入 |
| `/run/foundry/arche_known_hosts` | 同上 | 同上 |
| `/run/foundry/git-credentials` | 任务 | 任务开始时注入，每 30 分钟刷新，任务结束删除 |

- 镜像里的 `/etc/gitconfig` 固定配置
  `credential.helper = store --file=/run/foundry/git-credentials`；
- agentd 在 `~/.ssh/config` 写入 `Match host <host> user <user>` 块，把 Arche
  host 的 `IdentityFile` 和 `UserKnownHostsFile` 指向上面两个文件；
- 任务清理只删除 `git-credentials`，不会删除 Arche key；
- 设计空间不再使用 HTTP token，也没有 `ds_token`。

## 设计空间 SSH 端点

智能体只使用 Arche SSH。控制平面为每个 agent 账户生成 SSH key，公钥、
加密后的私钥和吊销状态保存在 Postgres 的 `agent_arche_keys` 表；AES-GCM master key
保存在 `~/.foundry/arche-key-master`（旧 `ds-key-master` 会在启动时自动改名）。

SSH 模型：

- 宿主机只运行系统 sshd；
- 所有 agent key 登录同一个服务账户（默认 `foundry-arche`）；
- 控制平面写出 `~/.foundry/arche-sshd/authorized_keys`，sshd 通过
  `AuthorizedKeysCommand` 读取；
- 每一行 key 都带 `restrict` 和 `command="/usr/local/bin/foundry-arche-stdio ..."`，不能开 shell；
- forced command 根据 key 对应的 allowlist 判断 repo，再执行
  `arche serve --stdio --root <repos-root>/<repo>`。

安装或更新（需要 root，操作幂等）：

```bash
cd projects/arche
sudo ./deploy/install.sh target/release/arche /srv/arche
```

并在宿主 sshd 的 `Match User foundry-arche` 块中，把 `AuthorizedKeysCommand`
指向 Foundry 生成的 `~/.foundry/arche-sshd/authorized_keys`。完整示例见
`projects/arche/deploy/README.md`。然后配置 `arche_ssh_url`，默认是
`ssh://foundry-arche@<宿主 IP>`，并重启控制平面。

控制平面需要用 `umask 002` 启动（systemd 下设置 `UMask=0002`），并给
`foundry-arche` 服务账户授予 `design_space_root` 的 ACL 读写权限。

agentd 启动并创建常驻 shell 时，通过 `/api/v1/agent/arche-credential` 取一次，
写到胶囊的 `/run/foundry/arche_key` 和 `/run/foundry/arche_known_hosts`。
`arche` 访问 SSH remote 时使用这两个文件，不经过环境变量。

删除 `agent_arche_keys` 里的 key 或将 `revoked_at` 置为非空，控制平面重新生成
`authorized_keys` 后即吊销该 agent 的访问。

`~/.foundry/arche-key-master` 是数据库私钥的 AES-GCM master key，必须和数据库备份分开保存。
丢失它会导致 `agent_arche_keys` 里的私钥无法解密，只能重新生成并轮换所有 agent key。

## 项目删除

删除项目是不可恢复的操作。控制平面会：

1. 停止该项目所有 agentd 进程；
2. 删除带 `foundry.agent_id` 标签的胶囊；
3. 删除每个 agent 的 `<workspace>/<project-id>` 工作区；
4. 删除 agent 日志文件和 `source-cache/<project-id>-*`；
5. 删除 `~/.foundry/design-spaces/<project-id>` 设计空间仓库（包括 `arche init` 创建的骨架对象、所有提交和分支）；
6. 删除数据库中的项目和 agent 定义，并删除未被其他定义使用的 agent 账户、SSH key、allowlist，刷新 `authorized_keys`。

删除前确认没有仍需保留的设计空间或工作区数据。

## 更新 agent shell 镜像

agent 的常驻 shell 使用 `shell_image`（默认 `localhost/openergasterion-shell:1`）。镜像里的 `arche` 是构建时复制进去的二进制，不会随 control plane 或工作区的重新编译自动更新。修改 `arche` 的 CLI 后，必须重建镜像：

```bash
cd projects/foundry
cargo build --release -p arche
cp ../../target/release/arche deploy/shell/arche
podman build -t localhost/openergasterion-shell:1 deploy/shell/
```

已经运行的 shell 容器仍持有旧二进制。重启 agentd 会创建新容器；不重启时，对运行中的容器执行：

```bash
for c in $(podman ps -q --filter ancestor=localhost/openergasterion-shell:1); do
  podman cp ../../target/release/arche "$c":/usr/local/bin/arche
  podman exec "$c" chmod 755 /usr/local/bin/arche
done
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

Foundry 只从 `~/.foundry/foundry.toml` 读取配置，不读环境变量；文件不存在时使用内置默认值。部署到其他位置时把配置文件挂载到这个路径。配置文件可能含密码，建议放在权限为 700 的目录中并 `chmod 600`。未知键会导致启动失败。旧版本的 `project` / `project_name` 键会被接受但忽略，启动时打印警告，可以删除。

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

每个项目有自己的 Task Dispatcher（任务调度员）；它每次扫描只看本项目的任务：

1. `tasks_ready_list`：每个任务一行，携带所有角色、当前绑定和人类候选者；
2. 对没有 enabled agent 的智能体角色，按 `assignment:name` 分配已有 Agent，或按模板创建后再分配；
3. 对人类角色，只有一个候选者时直接绑定；多个候选者留给模型选择；
4. 所有角色都绑定后调用 `task_start`（保留操作 `start`）把任务从 `Pending` 置为 `InProgress`。

`task_start` 是平台通用操作，不是 workflow 定义的；它只接受 Ready 任务，且要求每个角色都恰好有一个 actor。所有角色在任务启动前一起绑定；之后进入新的 stage 时不会重新走分配。后续阶段角色的 task actor 先以 inactive 状态绑定，进入对应 stage 时再激活。没有角色的任务直接启动。

人类角色引用的是身份而不是具体账户。项目创建者默认拥有所有身份，可以通过
`PUT /api/v1/projects/{id}/identities/{identity}`（`{"enabled": false}`）取消自己的某个身份，再用同样的接口恢复。其他用户暂不参与候选；一个角色有多个候选者时，Task Dispatcher 把任务描述、输入和候选者列表交给模型选择。

### Issue 分诊

项目根流程创建一个常驻的 `triage` 任务，只有一个分诊智能体：

1. `triage` 在 `waiting` 阶段等待 `foundry_issue_changes` 唤醒；有待分诊 Issue 时发送 `waiting:issue_arrived` 进入 `triage`；
2. `triage` 一次读取全部待分诊 Issue，分类、判断重复；
3. 需要较大设计（新的设计决定、边界/契约变化或设计缺失）的 Issue 置为 `pooled(pool=design)`，并在 Issue 中写清要求；不直接创建设计任务；
4. 局部设计修正创建 `design_correction` 后续任务，不进入设计池；
5. 重复 Issue 直接转 `Duplicate`，写入 `duplicate_of`，不创建任务；
6. 分诊完成后发送 `triage:done` 回到 `waiting`。

设计迭代由 `design_iteration` 任务驱动：设计分派智能体读取设计池，判断 `design` / `design_baseline`，创建子设计任务；Task Dispatcher 按空闲智能体分配，一个智能体同时只做一个任务。一轮验收通过后任务回到 `dispatch` 继续取下一批；设计池为空时进入 `waiting` 保持等待。需求、开发、测试迭代使用相同模型。

迭代不使用触发器。控制平面监听 Issue 变化：对应池首次出现 pooled Issue 时创建迭代任务；已有任务在 `waiting` 时发送 `waiting:work_arrived` 唤醒。另有 30 秒对账兜底通知丢失。旧 `triage_check` 和两级分诊已移除。

触发器只用于日常检查等显式计划任务；上一轮任务未 `Completed` 时不会重叠创建。`daily_check` 每天并行运行需求设计检查、技术设计检查、代码检查、日常测试四个子任务，结束后统一汇总并分流到四个 Issue 池。

当前自动处理范围是设计基线和设计缺陷：`design_baseline`、`design_correction`、`design`。旧 `issue_triage` 任务已停用。

任务进入 `InProgress` 后，Agent 进程在启动、重连和收到 `foundry_task_changes` 通知时，查询自己是 active actor 的任务，并自行决定是否执行。通知是瞬时的：不写 durable queue、不重试、不补发；通知丢失时，下一次通知或 Agent 重启会重新扫描。`issue_commented` 也走同一通知通道，因此人类在 Issue 回复会唤醒 Agent。

AgentRun 队列、lease、checkpoint 和运行历史已移除；Agent 执行只写进程日志，不再写 `agent_runs` / `agent_steps`。

启动时，control-plane 会对所有 `in_progress` Task 执行一次 transition 重算：按当前 stage 变量和 active actors 重新求值，条件满足则推进 stage 或完成任务。正常运行时 transition 只在 TaskEvent 处理过程中检查，这个启动扫描只用于恢复“已持久化的变量满足转移条件、但服务在事件后停止”的任务。

### 项目文档撰写

项目创建后，根 Task 创建 `goal_clarification` 子 Task 和统一的澄清 Issue：

1. Task Dispatcher 在任务开始时按 `goal_clarification:clarifier` 和 `goal_clarification:writer`
   分配或创建 **Clarifier** 和 **Project Document Writer**；
2. Clarifier 在澄清 Issue 中与创建者对话，只负责提问；澄清完毕后发送总结评论并调用
   `task_operation(operation = "clarification:completed")` 进入项目文档撰写；
3. Writer 读取澄清 Issue、项目信息和确认过的结论，在 `project-document-<task-id>`
   分支用 `arche` 维护设计空间中的 `project/document` 对象（定位、背景、目标、非目标、
   分期计划、约束、风险、术语表）；写完后调用
   `task_operation(operation = "writing:completed")` 进入文风审查；
4. 项目文档依次经过 **Project Layer Style Reviewer** 和 **Project Layer Content Reviewer**
   （都是 Agent）；两个 Reviewer 不修改文档，在写作分支上读取对象并用 `arche comment add`
   写意见，打回时任务回到 `writing`；
5. Writer 读取评论，逐条处理后修改并推送，再次发送 `completed`；
6. 两位 Reviewer 通过后，Content Reviewer 创建到 `design_space_branch` 的合并请求，
   人类在合并请求页批准并合并。项目文档的提交必须经过合并请求。


### 项目工作区

- 每个项目的工作区是 `$FOUNDRY_SANDBOX_ROOT/<project_id>`；未设置时默认 `~/.foundry/workspaces/<project_id>`。
- 工作区为空时，Implementer 可以用 `workspace_run` 执行 `git init` 从零开始；已有代码库由管理员提前放到该目录。
- Implementer 使用 `workspace_list` / `workspace_read` / `workspace_write` / `workspace_run` / `workspace_diff` 编辑代码、运行构建和测试；Reviewer 和 Test Executor 只读和运行测试。
- 工作区是本地开发目录，不是安全边界；生产部署应把它放进容器或 VM。

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
- `0036_anchor_documents.sql`：通用锚点文档、草案、版本、操作日志和审批记录（已被 `0067` 删除）。
- `0037_anchor_document_annotations.sql`：按锚点键和内容哈希记录的标注（已被 `0067` 删除）。
- `0067_drop_anchor_documents.sql`：删除锚点文档、旧需求文档和旧项目文档表；需求与技术设计改由设计空间 Arche 对象承载。
- `0068_strip_document_review_links.sql`：从已存储的流程版本和草稿中移除 `document_review_link` 阶段效果，保证旧数据库可以启动。
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
| 启动报 `DATABASE_URL must be set` | 在 `~/.foundry/foundry.toml` 中设置 `database_url`，格式 `postgres://用户:密码@主机:5432/库名` |
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
