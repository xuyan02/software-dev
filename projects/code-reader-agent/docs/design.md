# 代码阅读 Agent 设计（草案）

- 状态：草案，未开始实现
- 日期：2026-11-13
- 暂定项目名：`code-reader-agent`
- 相关模块：`graph-agent`、`linux-capsule`

## 1. 目标

做一个按固定 workflow 阅读代码的 Agent：

1. 只读取来自 git 仓库的代码；
2. 通过一个 shell 能力下载代码，该 shell 必须运行在隔离容器中；
3. 遍历目标 commit 中的每一个文件；
4. 每个文件生成唯一一份代码总结；
5. 总结通过工具保存到服务端，不保存在本地；
6. git 的文件版本与总结版本一一对应。

总结文档的具体格式暂不定义。本文只定义流程、版本模型、存储边界和前置条件。

## 2. 非目标

第一版明确不做：

- 不执行仓库里的代码，不构建、不测试、不运行脚本；
- 不允许读取宿主机上的任意文件；
- 不允许并发处理文件，也不使用多 worker；
- 不设置 `max_steps`；workflow 的终止只依赖文件列表耗尽；
- 不在本地持久化总结；
- 不把通用 shell 暴露给 LLM；
- 暂不定义总结正文的章节和字段结构。

## 3. 术语

| 术语 | 含义 |
|---|---|
| 文件版本 | `path + blob_oid`。`blob_oid` 是 git 对文件内容计算出的对象 ID |
| commit | 一次 git 快照，运行时必须解析成不可变的 commit SHA |
| manifest | 从 `git ls-tree` 得到的文件清单，包含路径、模式、blob OID |
| 总结键 | 服务端唯一确定一份总结的键，推荐为 `repo_id + path + blob_oid` |
| fetch | 在隔离容器中下载 git 数据并生成 manifest 的节点 |
| summarize | 只调用 LLM 阅读单个文件并产出总结的节点 |
| save | 把总结确定性地写入服务端的节点 |

## 4. 总体结构

```text
┌─────────────────────────────────────────────┐
│ code-reader-agent                           │
│                                             │
│  graph-agent workflow                       │
│    fetch → next_file → summarize → save     │
│                                             │
│  git 取数能力 ── linux-capsule（隔离容器）   │
│  总结写入能力 ── summary store client        │
└─────────────────────────────────────────────┘
                    │
                    ▼
          summary 服务端（版本化存储）
```

边界划分：

- `graph-agent` 只提供通用图执行、节点、转移、LLM 节点、Context；
- `code-reader-agent` 保存具体 workflow、提示词、manifest 解析和总结逻辑；
- `linux-capsule` 只提供隔离容器和 `exec` / 文件传输；
- summary 服务端负责版本唯一性和持久化，不依赖本地文件。

## 5. 固定 workflow

```text
fetch
  ↓
next_file ──有文件──→ summarize ──→ save ──→ next_file
  │
  └──无文件──→ exit
```

### 5.1 节点职责

| 节点 | 类型 | 职责 | 转移 |
|---|---|---|---|
| `fetch` | 确定性节点 | 创建容器、下载 git 数据、解析 commit、生成 manifest、得到文件内容 | 1 个：到 `next_file` |
| `next_file` | 确定性节点 | 读取游标；没有下一个文件则结束；否则设置当前文件上下文并前进游标 | 2 个：有文件到 `summarize`，无文件到 `exit` |
| `summarize` | LLM 节点 | 只阅读当前文件，产出一份总结；不调用任何工具 | 1 个：到 `save` |
| `save` | 确定性节点 | 把当前总结按版本键写入服务端 | 1 个：回到 `next_file` |

### 5.2 循环和终止

- 不使用 `max_steps`；
- 循环终止条件只有一个：`next_file` 发现游标已经到达文件列表末尾；
- `next_file` 必须保证每次成功执行都推进游标，否则会无限循环；
- 空仓库或 commit 中没有任何可处理文件时，`next_file` 直接走 `exit`；
- 整个 workflow 串行执行，与 `graph-agent` 当前“不允许并发、不允许多 worker”的约束一致。

### 5.3 Context 状态

建议把运行状态放在 `Context` 变量里，而不是放在节点内部的可变字段里，保证同一个 Agent 可以被多次独立运行。

建议的变量：

| 变量 | 用途 |
|---|---|
| `job.repo_url` | 原始仓库地址 |
| `job.repo_id` | 规范化后的仓库 ID |
| `job.ref` | 用户请求的 branch/tag/commit |
| `job.commit_oid` | 解析后的固定 commit SHA |
| `job.temp_dir` | 宿主临时目录，仅存本次运行的数据 |
| `job.manifest_path` | manifest 文件路径 |
| `job.cursor` | 当前文件下标 |
| `job.total` | 文件总数 |
| `current.path` | 当前文件路径 |
| `current.blob_oid` | 当前文件版本 |
| `current.content` | 当前文件内容 |

`current.content` 可能很大，第一版必须限制单文件大小。具体限制见未决定项。

## 6. fetch 的详细设计

### 6.1 容器

使用：

```rust
LinuxCapsule::headless()
```

每次运行创建一个 headless capsule，本次运行结束后关闭。容器内只执行 git 命令，不执行仓库代码。

当前 `linux-capsule` 镜像只安装了 Xvfb、openbox、xterm 等，**没有 git**。因此前置条件是：

- 在镜像中加入 `git`；
- 加入 `ca-certificates`，否则 HTTPS clone 可能失败；
- 私有仓库还需要 SSH 或凭据方案；
- 重新构建镜像并验证 `git --version`。

### 6.2 下载步骤

1. 在容器内初始化或克隆仓库；
2. 解析 ref：

   ```sh
   git rev-parse <ref>^{commit}
   ```

3. 生成文件清单：

   ```sh
   git ls-tree -r -z <commit_oid>
   ```

   输出格式：

   ```text
   <mode> SP <type> SP <blob_oid> TAB <path> NUL
   ```

4. 只保留 `type == blob` 的条目；
5. 把 blob OID 列表上传到容器；
6. 在容器内批量导出精确的 blob 内容：

   ```sh
   git cat-file --batch < /tmp/oids > /tmp/blobs
   ```

7. 下载 manifest 和 blob 数据到宿主临时目录；
8. 关闭容器；
9. 后续节点只读取这份从 git 生成的数据。

### 6.3 为什么不用 checkout 或 archive

不要用 `git checkout` 后遍历工作区：

- 会应用 smudge filter、LFS 等，得到的不是原始 blob；
- 工作区可能包含仓库之外的文件；
- 需要额外的过滤逻辑来排除 `.git`、未跟踪文件等。

不建议用 `git archive`：

- 会应用 `export-ignore` / `export-subst`；
- 内容可能和 blob 不一致；
- 会把某些文件排除掉，破坏“和 git 文件一一对应”。

只有 `git cat-file` 读取的是 git 对象本身，适合作为版本依据。

### 6.4 文件类型

`git ls-tree -r` 可能返回：

| 类型 | 处理 |
|---|---|
| 普通 blob | 第一版的主要处理对象 |
| 可执行 blob | 仍是 blob，按普通文件处理 |
| 符号链接 | 内容是链接目标，是否总结待定 |
| submodule | 类型是 commit，不是 blob，是否递归待定 |
| LFS 文件 | blob 是 LFS 指针，不是实际内容，是否获取实际内容待定 |
| 二进制文件 | 无法直接交给 LLM，策略待定 |

### 6.5 性能和资源

- 一个 capsule 只服务一次运行，避免多个仓库之间互相影响；
- 不要每个文件执行一次 `podman exec`，否则文件多时开销很大；
- 使用一次 `cat-file --batch` 批量导出；
- 宿主临时目录需要设置大小上限，并在成功或失败后清理；
- 容器默认资源限制为 2048 MB 内存、2 CPU、512 pids，第一版先沿用。

## 7. 文件版本模型

### 7.1 推荐方案

```text
文件版本 = repo_id + path + blob_oid
```

其中：

- `repo_id`：规范化后的仓库标识，不是 branch；
- `path`：文件在仓库中的路径；
- `blob_oid`：文件内容在 git 中的对象 ID；
- `commit_oid`：只作为来源信息，不作为唯一键。

行为：

| 场景 | 结果 |
|---|---|
| 同一文件在两个 commit 中内容未变 | `blob_oid` 相同，不生成新总结 |
| 文件内容改变 | `blob_oid` 改变，生成新总结 |
| 文件改名但内容未变 | `path` 改变，生成新总结 |
| 同一内容出现在两个路径 | 分别总结，因为路径语义不同 |

### 7.2 备选方案

| 方案 | 唯一键 | 优点 | 缺点 |
|---|---|---|---|
| A（推荐） | `repo_id + path + blob_oid` | 跨 commit 去重，版本语义准确 | 文件改名会产生新总结 |
| B | `repo_id + commit_oid + path` | 每个 commit 的每个文件都有独立记录 | 未变化文件会重复总结 |
| C | `repo_id + blob_oid` | 最大程度去重 | 同一内容在不同路径共享总结，可能丢失路径语义 |

### 7.3 总结格式变化

如果总结格式、提示词或模型版本变化，需要决定：

- 覆盖旧总结，保持“一文件版本一总结”；
- 还是把 `format_version` 也放入唯一键，允许同一文件版本存在多份不同格式的总结。

当前推荐：第一版只保留一份，服务端记录 `format_version` 和 `generator`，格式变化时由调用方决定覆盖或重新生成。

## 8. summary 服务端

### 8.1 存储信封

总结正文暂不定义，服务端只存不透明内容和元数据：

```json
{
  "repo_id": "normalized-repo-id",
  "path": "src/main.rs",
  "blob_oid": "abc123...",
  "commit_oid": "def456...",
  "format_version": 1,
  "generator": {
    "model": "model-name",
    "prompt_version": "prompt-v1"
  },
  "content": "..."
}
```

### 8.2 唯一性和幂等

- 唯一约束：`(repo_id, path, blob_oid)`；
- 保存接口必须幂等；
- 同一个键重复提交不能产生第二条总结；
- 保存接口返回“新建”或“已存在”；
- 服务端自己生成时间戳和内部 ID，不接受客户端指定。

### 8.3 接口草案

```text
PUT /v1/summaries
```

请求体为上面的信封。第一版可以不做复杂查询，但建议至少提供：

```text
GET /v1/summaries?repo_id=...&path=...&blob_oid=...
```

用于重复运行时跳过已存在总结。

### 8.4 认证

Agent 写总结必须经过认证。模型 API key 不应进入容器；仓库凭据也不应出现在 shell 命令行中。

## 9. 与现有工程的差距

### 9.1 `graph-agent` 没有工具执行循环

当前接口：

```rust
pub trait ToolRegistry: Send + Sync {
    fn spec(&self, tool: &ToolRef) -> Result<ToolSpec, ToolError>;
}
```

它只能把工具定义交给模型。`LlmResponse` 只有 `output`，没有 tool call、tool result 和多轮工具循环。

因此：

- 如果 shell 只是确定性 `fetch` 节点使用的底层能力，当前架构可以扩展实现；
- 如果要求 LLM 自己调用 shell，需要新增工具执行、消息历史、工具调用循环、错误回传等，工作量明显更大，也会引入 prompt injection 执行命令的风险。

当前推荐：**shell 不暴露给 LLM**。`fetch` 节点直接调用受控的容器工具，LLM 只负责总结。

### 9.2 没有 `foreach` 节点

`graph-agent` 没有动态遍历原语。但图允许回边，所以可以用：

```text
next_file → summarize → save → next_file
```

表达串行遍历，不需要新增引擎机制。

### 9.3 Context 没有资源槽

`Context` 目前用于变量和节点输出，不适合直接持有 `LinuxCapsule` 之类的运行时资源。

第一版通过“fetch 批量导出到宿主临时目录，然后关闭容器”避免这个问题。代价是代码内容会短暂存在于宿主临时目录中，但不会执行，也不持久化。

如果要求代码内容完全不落到宿主，则需要让 `Context` 支持生命周期资源，并让 `fetch` 节点保持 capsule 存活，后续节点逐文件读取。这个方案更复杂，暂不作为第一版。

### 9.4 镜像和凭据

当前 `LinuxCapsule` 的公开 API 只有 `desktop()` / `headless()`，没有自定义镜像、`setup` 或秘密注入参数。

这意味着：

- 第一版只能依赖默认镜像，因此必须把 git 加进默认镜像；
- 私有仓库的凭据注入需要扩展 capsule API，或暂时只支持公开仓库。

### 9.5 服务端和客户端缺失

目前没有 summary 服务端，也没有 `code-reader-agent` 的存储客户端。两者都需要新建。

## 10. 安全设计

### 10.1 仓库内容是数据，不是指令

被阅读的代码可能包含恶意注释、README 或测试数据，试图让模型改变行为。

第一版必须：

- 不给 LLM 任何工具；
- 在 system prompt 中明确文件内容只是待分析数据；
- 对总结输出做结构校验；
- 不把总结内容直接当作后续执行指令。

### 10.2 shell 注入

- blob OID 只允许十六进制字符；
- 仓库 URL 必须经过解析和校验，不能直接拼进 `sh -c`；
- 路径来自 `git ls-tree -z`，不能由模型生成；
- 不接受模型提供的任意 shell 命令。

### 10.3 网络和 SSRF

如果仓库 URL 由用户输入，需要限制：

- 只允许 `https://` 或明确的 `ssh://`；
- 使用仓库域名 allowlist；
- 拒绝 localhost、link-local、云元数据地址等内部目标。

### 10.4 容器边界

- 不挂载宿主目录；
- 不挂载 X11 socket、Docker/Podman socket；
- 不使用 privileged；
- 容器共享宿主内核，不是硬安全边界；如果以后要执行不可信代码，应改用 microVM。

### 10.5 临时数据

- 临时目录权限限制为当前用户；
- 运行结束或失败后删除；
- 不把总结写到本地；
- 不把仓库内容写入日志。

## 11. 错误处理

第一版建议：

- `fetch` 失败：整个运行失败，不进入总结；
- 单个文件读取失败：整个运行失败，返回文件路径和错误；
- 单个文件总结失败：整个运行失败；
- 保存失败：整个运行失败，保留错误；
- 保存接口幂等，允许安全重试；
- capsule 通过 `Drop` 或显式 `close()` 清理。

后续可考虑：

- 记录失败文件并继续；
- 断点续跑；
- 按游标恢复；
- 跳过已存在总结。

这些都是未决定项。

## 12. 前置条件

### 12.1 需求语义

- [ ] 确定一次运行只读一个 commit，还是遍历全部历史版本；
- [ ] 确定仓库是公开还是私有；
- [ ] 确定版本键采用哪种方案；
- [ ] 确定“每一个文件”的范围：普通文本、二进制、符号链接、submodule、LFS；
- [ ] 确定大文件、生成文件、压缩文件的处理方式；
- [ ] 确定总结格式变化时是覆盖还是保留多版本；
- [ ] 确定重复运行时跳过还是重新生成；
- [ ] 确定单文件失败是整体失败还是继续。

### 12.2 基础设施

- [ ] rootless Podman 可用；
- [ ] `linux-capsule` 已安装，cgroup 控制器已委派；
- [ ] 镜像包含 `git` 和 `ca-certificates`；
- [ ] 容器可以访问目标仓库；
- [ ] 有模型网关和 API key；
- [ ] 有 summary 服务端、数据库和认证；
- [ ] 宿主机临时目录有容量限制和清理策略。

### 12.3 代码能力

- [ ] 新建 `code-reader-agent` 项目，不把具体业务放进 `graph-agent`；
- [ ] 实现受控的 git fetch 能力；
- [ ] 实现 manifest 解析和 blob 批量读取；
- [ ] 实现环式遍历 workflow；
- [ ] 实现 summary 客户端；
- [ ] 实现单文件提示词和输出校验；
- [ ] 实现大文件限制和错误处理；
- [ ] 如果确实需要 LLM 调用 shell，先扩展 `graph-agent` 的工具执行和多轮工具循环。

### 12.4 安全

- [ ] 不把 shell 暴露给 LLM；
- [ ] 仓库 URL allowlist；
- [ ] shell 参数不允许模型生成；
- [ ] 容器不挂载宿主目录；
- [ ] 私有仓库凭据不进入命令行和日志；
- [ ] 临时代码运行后清理；
- [ ] 明确容器不是硬安全边界。

## 13. 最小可行版本

第一版建议只做：

- 单个公开 HTTPS 仓库；
- 用户指定一个 branch/tag/commit，解析成固定 commit SHA；
- 只处理 `git ls-tree` 中类型为 blob 的文件；
- 文本文件生成总结；
- 顺序遍历，不并发；
- 每个文件一次 LLM 总结；
- 保存到服务端，按 `(repo_id, path, blob_oid)` 唯一；
- 失败即停止；
- 不处理历史版本、不做增量、不做并发；
- 不把 shell 交给 LLM。

暂缓：

- 全部历史版本；
- 私有仓库；
- 二进制/LFS/submodule；
- 大文件分块；
- 断点续跑；
- 已有总结跳过；
- 总结格式的正式定义。

## 14. 未决定项汇总

| 编号 | 问题 | 推荐默认 | 影响 |
|---|---|---|---|
| D1 | 一次读一个 commit，还是读全部历史 | 先读一个固定 commit | 决定 fetch 复杂度、数据量和是否有历史版本遍历 |
| D2 | 仓库访问方式 | 先支持公开 HTTPS | 决定是否需要凭据注入和 capsule API 扩展 |
| D3 | 版本键 | `repo_id + path + blob_oid` | 决定跨 commit 去重、改名行为和服务端唯一约束 |
| D4 | 文件范围 | 先处理文本 blob；其他类型记录策略 | 决定“每一个文件”的实际语义 |
| D5 | 大文件处理 | 先设置上限，超限失败或跳过 | 决定 token 成本和总结完整性 |
| D6 | 二进制文件 | 暂不调用 LLM，保存元数据或跳过 | 影响“每文件一总结”的承诺 |
| D7 | 总结格式变化 | 单份总结，记录 `format_version` | 决定覆盖策略和是否允许多份 |
| D8 | 重复运行 | 先每次重新生成并 upsert，后续加跳过 | 决定成本和是否需要查询接口 |
| D9 | 错误策略 | 第一版整体失败 | 决定 workflow 是否需要失败分支 |
| D10 | LLM 是否可调用 shell | 不开放 | 决定是否需要扩展 `graph-agent` 的工具执行循环 |
| D11 | 总结服务端位置 | 待定：Foundry 内或独立服务 | 决定部署、认证和数据模型 |
| D12 | 非 UTF-8 路径 | 第一版要求路径可 UTF-8 表示 | 决定 manifest 和 API 编码方式 |
| D13 | capsule 是否保持到总结结束 | 第一版批量导出后关闭 | 决定是否需要给 Context 增加资源槽 |
| D14 | 历史版本增量策略 | 暂不做 | 决定是否按 blob 建立全局缓存和差量任务 |

## 15. 验收标准（第一版）

给定一个公开仓库和一个 ref：

1. 能在隔离容器中完成 clone/fetch；
2. 能解析出固定 commit SHA；
3. 能生成完整的 blob manifest；
4. 每个被选中的文件只产生一份总结；
5. 总结通过服务端接口保存，不在本地留下总结文件；
6. 同一 `(repo_id, path, blob_oid)` 重复保存不会产生重复记录；
7. 运行结束后容器被删除，临时代码被清理；
8. 仓库代码没有被执行；
9. LLM 没有获得 shell 或其他执行工具；
10. 任何失败都能定位到仓库、commit、路径和阶段。
