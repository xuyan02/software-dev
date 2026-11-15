# ds：详细设计（原 design）

- 状态：实现中
- 日期：2026-11-15
- 工程：`projects/design-space`
- 二进制：客户端 `ds`，服务端 `ds-server`

## 1. 目标

把一个代码仓库的设计变成一套可版本化、可增量同步的内容寻址数据：

- 设计空间里的“元素”是**概念对象**（能力、领域概念、组件职责、约束），不是语法节点；
- 智能体阅读代码后，通过 `ds create` 显式创建元素并写设计；
- 每个概念元素是一个独立对象；
- 对象不可变，按内容 hash 寻址；
- 版本像 git commit，指向一棵由索引节点组成的树；
- 同一批对象可以按多种组织方式（namespace、file、package）浏览；
- 上传下载按树差异走，和改动量成正比。

> 2026-11-15 模型修正：删除 `scan` 和全部代码解析（tree-sitter 适配器）。
> 概念对象无法从 AST 推导，必须由智能体阅读代码后创建。源码只作为只读输入
> 和可选锚点。

## 2. 边界

工具负责：

- 拉取源码、提供只读源码访问；
- 校验并存储智能体创建的概念元素、设计、关系；
- 计算 hash；
- 存储节点、打包；
- 版本、ref、同步。

工具不负责：

- 解析代码、抽取语言元素；
- 生成设计文本和概念划分（智能体负责）；
- 执行仓库代码；
- 语义类型推导（只做语法解析）。

## 3. 模型

四类节点：

```text
OBJ   元素对象
DES   设计说明
IDX   索引节点（视图树、虚拟根）
VER   版本
```

版本树：

```text
VER1
  └ root IDX（虚拟根）
      ├ "namespaces" → IDX 树 → OBJ 叶子
      ├ "files"      → IDX 树 → OBJ 叶子
      └ "packages"   → IDX 树 → OBJ 叶子
```

- 索引节点只负责分组，没有设计；
- 语言元素对象有设计；
- 文件也是对象：每个 git blob 对应一个 `file` 对象（`design_hash = 0`，不总结），无论语言是否被支持；
- 每个节点的 hash 包含子节点的 hash，所以子变父必变；
- 虚拟根把所有视图串成一个根，版本只指向这个根。

## 4. 通用编码

- 小端；
- hash 一律 32 字节 SHA-256；
- 字符串 = `u32 长度 + UTF-8`，不带 NUL；
- commit 统一 32 字节，SHA-1 靠左补 12 个 0；
- 文件名 = hash 的小写十六进制。

每个节点开头：

```text
0    1   version
1    3   reserved
4    4   type
8    ... payload
```

`type` 统一编码语言和种类：

```text
type = (language << 16) | kind
```

`version` 在最前，因为读的人必须先知道格式版本，才能决定后面怎么解析。
`type` 决定 payload 的布局。

## 5. 类型常量

完整常量表见 `src/types.rs`。摘录：

### 语言 ID（高 16 位）

```text
0x0000 universal    0x0001 C           0x0002 C++
0x0003 Rust         0x0004 Java        0x0005 Kotlin
0x0006 C#           0x0007 Go          0x0008 Python
0x0009 JavaScript   0x000A TypeScript  0x000B JSX
0x000C TSX          0x000D Swift       0x000E Ruby
0x000F PHP          0x0010 Scala       0x0011 Shell
0x0012 SQL          0x0013 Lua         0x0014 Dart
0x0015 Elixir       0x0016 Haskell     0x0017 text
```

### 结构与通用类型（language 0）

```text
0x0000_0001 directory       0x0000_0002 file
0x0000_0003 symlink         0x0000_0004 submodule
0x0000_0005 virtual_root    0x0000_0006 view_root
0x0000_0007 design          0x0000_0008 version
0x0000_0009 binary_file     0x0000_000A generated_file
0x0000_000B text_file
```

### 布局映射

| type | 布局 |
|---|---|
| `directory`、`virtual_root`、`view_root` | IDX |
| `design` | DES |
| `version` | VER |
| 其余 | OBJ |

规则：

- 常量只增不改，删除的作废不复用；
- 每个语言 `kind = 0` 保留为 unknown；
- `type = 0` 表示 unknown，读者必须能跳过；
- 常量表版本写进版本的 pipeline 元数据。

## 6. 节点结构

### 6.1 OBJ1：元素对象

```text
偏移  长度  字段
0     1    version
1     3    reserved
4     4    type
8     32   commit
40    8    start_byte
48    8    end_byte
56    4    start_line
60    4    end_line
64    32   content_hash
96    32   input_hash
128   32   design_hash
160   4    name_len
164   n    name
...   4    path_len
...   p    path
```

- `name` 是语言层全称，不含路径；
- `path` 是文件路径；
- 不存 children：结构在索引树里；
- 容器的 `content_hash` 已包含子元素的 content_hash；
- `input_hash` 已包含子元素的 design_hash；
- 语言元素的 `design_hash` 指向 DES 节点。

**文件对象**（`type = file`）：

- `name` = `path` = 文件路径；
- `content_hash` = git blob 的 oid（SHA-1 按 commit 的规则左补 12 个 0）；
- `input_hash` = `content_hash`；
- `design_hash` = 0，永远不总结；
- 文件名不含 NUL，`files` 视图里文件对象的 key 就是 path 本身；
- 不支持解析的语言、二进制、超大文件同样有文件对象，只是没有语言元素。

### 6.2 DES1：设计说明

```text
0    1   version
1    3   reserved
4    4   type = design
8    8   length
16   n   UTF-8 文本
```

### 6.3 IDX1：索引节点

```text
0    1   version
1    3   reserved
4    4   type
8    1   leaf          // 0 = 内部，1 = 叶子
9    3   reserved
12   4   count
16   entries:
       key_len  u32
       key      bytes
       hash     32
```

- 条目按 key 字节序严格升序；
- 叶子条目指向 OBJ，内部条目指向 IDX；
- 内部条目的 key = 该子树的最大 key（上界）；
- hash = SHA-256(整个 payload)；
- 建议 fanout 32，溢出分裂；v1 删除允许 underfull。

**视图 key 编码**

```text
namespaces : name
files      : path \x00 name        // 文件对象用 path 本身
packages   : package \x00 name
虚拟根      : "namespaces" | "files" | "packages"
```

路径和名字都不含 NUL，所以 NUL 分隔无歧义。

### 6.4 VER1：版本

```text
0     1    version
1     3    reserved
4     4    type = version
8     32   parent           // 全 0 = 无父
40    32   root             // 虚拟根 IDX
72    32   source_commit
104   4    pipeline_len
108   n    pipeline
...   4    author_len
...   n    author
...   4    message_len
...   n    message
...   8    timestamp
```

版本 hash = SHA-256(VER1 payload)，ref 指向它。

## 7. Hash 规则

工具内部计算：

```text
content_hash(叶子)   = H(0x01 || own_text)
content_hash(容器)   = H(0x02 || own_text || 子.content_hash ...)
input_hash(叶子)     = content_hash
input_hash(容器)     = H(0x03 || content_hash || 子.design_hash ...)
design_hash          = H(DES1 payload)
object_hash          = H(OBJ1 payload)
```

子列表按源码顺序。

- `input_hash` 没变 → 复用旧 design_hash，不调模型；
- `content_hash` 没变但位置变 → 新对象，设计复用；
- 容器的 `content_hash` 和 `input_hash` 由工具在解析时算出。

## 8. 本地存储

### 8.1 loose

```text
.ds/objects/<aa>/<bb>/<hash>
```

所有节点统一存放，类型在 payload 里。先写 `tmp/` 再 rename。

节点被收进 pack 并删除 loose 后，空的 `objects/<aa>/<bb>` 和 `objects/<aa>` 目录也会清理；顶层 `objects`、`packs` 等目录保留。

读取顺序：先 loose，再查 `packs/` 的多包索引。多包索引启动时只读每个 pack 的 trailer 和 index，建立 `hash → (pack, offset)`，不读 payload；命中后按 offset 只读那一条记录。

### 8.2 本地目录

工作区根目录 = 运行 `ds init` 的目录，它就是用户的目录；工具只写其中的 `.ds/`。
`--dir` 指工作区根目录；不传时从当前目录向上找 `.ds`，因此在工作区任意子目录都能执行命令。

```text
<工作区根>/
  ...               # 用户目录的其他内容，工具不碰
  .ds/
    .gitignore      # 内容为 *：工作区根恰好是 git worktree 时不污染 git status
    config          # [source] url，init 后只读
    HEAD            # ref: refs/heads/<current>，和 git 一样指向当前本地分支
    refs/heads/main # 每个本地分支的已提交版本
    refs/remotes/origin/main
    objects/
    packs/
    source/         # 唯一的 bare 克隆；只有 refs/remotes/origin，没有 refs/heads
    source-head     # 当前源码 ref：branch <name> 或 commit <sha>
    branches/<name>  # 尚未产生版本的分支标记（空文件）
    cache/trees/<version>.json  # 元素父子关系（本地缓存）
    cache/names/<version>.idx
    tmp/
```

源码仓库生命周期：

1. `ds init`：`git clone --bare` 到 `.ds/source`，把 clone 出来的 `refs/heads/*` 复制成 `refs/remotes/origin/*` 后全部删除；默认分支记到 `.ds/source-head`；
2. `ds source fetch`：显式更新 `refs/remotes/origin/*` 和 tags；fetch 后再次清掉任何 `refs/heads`；
3. `ds source checkout <ref>`：只写 `source-head`（远端分支记 `branch <name>`，tag/commit 解析成 `commit <sha>`），不创建 work tree，也不创建本地分支；
4. `create` / `describe` 记录当前 `source-head` 指向的 commit，作为版本和元素锚点的来源；
5. 智能体通过 `git cat-file blob` 读取原始字节；工具本身不解析代码。

源码仓库不保留本地分支：`origin/<name>`、`refs/remotes/origin/<name>` 和 `<name>` 都表示同一个远端分支。

不创建 work tree，不应用 smudge/LFS filter，也不执行仓库代码。当前实现直接在宿主机跑只读 git；后续再把 git 操作移入隔离 capsule。

## 9. Pack 格式

```text
header (16)
  "PAK1" | version u32 | reserved u64

record（每条 8 字节对齐）
  record_size      u32   // 含记录头 + 压缩数据 + padding
  compressed_size  u32
  type             u8    // 1=OBJ 2=DES 3=IDX 4=VER
  codec            u8    // 0=raw 1=zstd
  reserved         u16
  original_size    u32
  payload          compressed_size 字节
  padding          到 8 的倍数

index
  "PKI1" | version u32 | count u64 | entry_size u32=40 | reserved u32
  entries: hash 32 | offset u64        // 按 hash 升序

trailer (64)
  "TRL1" | version u32 | pack_size u64
  index_offset u64 | index_size u64
  pack_sha256 32                        // 覆盖 [0, pack_size-64)
```

- `pack_sha256` 是文件名；
- 整包校验在上传和复制时做一次；
- 当前实现 `codec = 0`；zstd 留位。

### 9.1 多包索引

`MultiPackIndex` 扫描 `packs/*.pack`，打开每个 pack 的 trailer + index，合并成一张 `hash → pack` 表。读取一个节点时：

```text
loose 存在 → 直接读
否则 → MIDX 找到 pack → PackIndex 按 offset 读 record → 校验 payload hash → decode
```

因此 pack 之后可以安全删除 loose 副本。MIDX 是进程内构建的缓存，不落盘。

## 10. 写入流程

```text
create   智能体给出概念元素（名称/设计/锚点/父节点），算 hash，写 OBJ/DES/IDX，建虚拟根和 VER，推进 ref
describe 基于当前版本重建元素，写入设计，生成新的 OBJ/祖先 OBJ/IDX/VER，推进 ref
```

没有暂存区，也没有单独的 commit 命令：**create 和 describe 本身就是一次版本提交**。

- create 允许元素没有设计，`design_hash = 0`；`missing` 就是没有设计的数量；
- 版本 parent 是当前分支 ref 指向的版本；ref 原子更新；
- 没变的 OBJ/DES 复用原节点，只有变化的元素、受影响的祖先、视图 IDX、虚拟根和 VER 是新的；
- 版本记录当前 `source-head` 的 commit；元素可选记录同一条 commit + path/lines 作为证据；
- `status`、`show`、`list`、`log` 都直接读当前版本。

> 旧的 `scan`、增量文件 diff、tree-sitter 解析、按 input_hash 自动复用设计均已删除。

### 10.1 本地分支

- 本地分支 = `refs/heads/<name>`；尚未产生版本的分支有一个 `branches/<name>` 标记文件；
- `HEAD` 记录当前分支，默认 `main`；
- `create` / `describe` 只写入当前分支；切分支用 `ds checkout <name>`，新分支用 `ds checkout -b <name>`；
- `checkout <name>` 切换当前分支；`checkout -b <name>` 创建并切换；
- `branch` 列出所有本地分支及其版本；`branch <name>` 创建但不切换；`branch -d <name>` 删除非当前分支；
- 每个分支独立复用自己基点版本的设计；不同分支互不影响；
- `show` / `list` / `log` 的 `--at` 缺省为当前分支。

### 10.2 版本与源码 commit

- 每次 `create` / `describe` 都读取当前 `.ds/source-head` 指向的 commit，写入版本根节点 `VER1.source_commit`；
- 元素可以记录同一条 commit + `path` + 行范围作为源码锚点，供 `show --source` 和证据展示；
- 版本不要求源码 commit 前进，也不再有“只能向前扫描”的限制；设计空间是作者产出的模型，不是源码的自动投影；
- 源码 ref 的切换仍然显式：`ds source checkout <branch|tag|commit>`。

### 10.3 元素创建

- `ds create <name>` 创建一个概念元素；名称在同一分支内唯一；
- 可选 `--design` / `--design-file`；缺省时不写设计，计入 `missing`；
- 可选 `--path` / `--lines`：只作为证据锚点，不参与 identity；
- 可选 `--parent`；未指定时，在已存在元素中寻找最长的 `::` 名称前缀作为父节点；
- 父/子链接写入本地 tree cache；版本本身只保存对象、视图和 hash；缺失 cache 时按 `::` 名称重建；
- 删除的旧解析流程不再存在，也不再有文件级增量 diff。

## 11. 上传（push）

### 11.1 前置

```text
V_new = 本地要推的版本
V_old = 远端当前 ref
```

要求 `V_old` 是 `V_new` 的祖先。

### 11.2 算差异，不算全量闭包

```text
diff(V_old.root, V_new.root)
```

两个 IDX 节点：

- hash 相同 → 整棵子树相同，跳过；
- 都是叶子 → 按 key 合并，收集新增/变化 key 的 OBJ 和 DES；
- 都是内部 → 按 key 区间对齐子节点，hash 不同才递归；
- 结构不同 → 收集新树里整棵子树。

得到：

```text
new_nodes = { 新 IDX（含虚拟根和各视图根）, 新 OBJ, 新 DES, V_new }
```

代价 = 变化路径 × 树深。

### 11.3 组包上传

```text
1. 客户端 diff 出 new_nodes，加上 V_new，打成 pack（按 hash 排序）
2. POST /v1/packs        原始 pack 字节；服务端校验 checksum、解包进 objects/
3. POST /v1/push         { ref, old: V_old, new: V_new }
```

### 11.4 服务端校验

```text
1. old 等于当前 ref                        → 否则 409 conflict
2. V_old 存在且是 V_new 的祖先              → 否则 409 conflict
3. 服务端自己再算 diff(V_old.root, V_new.root)（无 V_old 时算全量闭包）
4. 逐个检查节点是否存在
   缺 → 422 { status: "missing", missing: [hash...] }
5. 原子更新 refs/heads/<ref>
```

客户端收到 `missing` 后，把这些节点补进 pack 重传一次。
成功后客户端更新 `refs/remotes/origin/<branch>`。

## 12. 下载（fetch / pull）

```text
GET  /v1/refs
POST /v1/fetch { want: V_want, have: V_have }
  → 原始 pack 字节
```

服务端：

```text
diff(V_have.root, V_want.root)（V_want 不在本地时按全量）
把差集节点 + V_want 现场打成 pack 返回
```

客户端：

```text
1. 校验 pack 的 trailer checksum
2. 解包，逐个节点校验 hash 后落盘
3. 检查 V_want 的整棵闭包是否齐全
4. 更新 refs/remotes/origin/<branch>
5. pull 时：
   - V_have 必须是 V_want 的祖先（快进）
   - 更新 refs/heads/<branch>
```

## 13. 远端接口

HTTP（`Authorization: Bearer $DS_TOKEN`，服务端和客户端都从环境变量读 token）：

```text
POST /v1/projects            { name } → { name, slug, design_space }
GET  /v1/projects
GET  /v1/projects/{slug}
```

每个项目的同步端点：

```text
GET  /v1/projects/{slug}/v1/refs                    → { refs: { name: version } }
POST /v1/projects/{slug}/v1/fetch  { want, have }   → 原始 pack
POST /v1/projects/{slug}/v1/packs  原始 pack         → { checksum }
POST /v1/projects/{slug}/v1/push   { ref, old, new } → 200 ok / 409 conflict / 422 missing
```

SSH：

```text
ds-server stdio --root <root> --project <id>   # 由 sshd / ssh 启动
ssh://[user@]host[:port]/<id>
ds://[user@]host[:port]/<id>   # 等价，推荐
```

`<id>` 是项目的 16 位十六进制 id（`project.toml` 中记录），地址里不含文件系统路径；服务端在自己的 root（`--root` 或 `DESIGN_SPACE_ROOT`，默认 `~/.foundry/design-spaces`）下按 id 查找仓库。多段路径仍作为显式仓库路径兼容（`--repo <dir>`），但不用于新地址。

客户端用系统 `ssh` 启动 `ds-server stdio --project <id>`，在同一 stdin/stdout 上跑同样的 4 种消息（1 refs、2 fetch、3 pack、4 push），帧格式为 `op u8 | len u32 LE | payload`，响应为 `status u8 | len u32 | payload`。

服务端不需要自己实现 SSH 协议，交给 OpenSSH（和 git 的 upload-pack 一样）。sshd 配置示例：

```text
Match User ds
  ForceCommand /usr/local/bin/ds-server stdio --root /srv/design-spaces
  AllowTcpForwarding no
  X11Forwarding no
```

ForceCommand 会忽略客户端命令行，但 OpenSSH 把它保留在 `SSH_ORIGINAL_COMMAND` 中；`ds-server stdio` 只从里面取 `--project <id>`，不接受客户端传入的 `--root` 或 `--repo`，root 始终由服务端配置决定。

认证、加密和用户隔离由 sshd 完成；HTTP 模式下用 `DS_TOKEN`。

服务端存储：

```text
<repo>/
  refs/heads/<name>     # 版本 hash
  objects/              # 解包后的 loose 节点
  packs/<sha256>.pack   # 收到的原始 pack（归档）
  tmp/
```

## 14. CLI

```text
ds clone <ds-url> [--source <git-url>] [--branch <name>] [<dir>]  # init + fast-forward
ds init --source <url>                    # 只创建本地仓库，不接触远端
ds remote add <url>                       # 给已创建的仓库设置 origin
ds remote show
ds remote set-url <url>
ds source status                          # 当前源码 ref 和 commit
ds source branch                          # 远端分支，git branch 风格
ds source fetch                           # 显式拉取
ds source checkout <branch|tag|commit>    # 切换源码 ref
ds source log [<ref>] [--limit N]         # 源码提交日志
ds create <name> [--design T|--design-file F] [--path P] [--lines A-B] [--parent N]  # 创建概念元素并推进版本
ds branch [<name>] [-d <name>]
ds checkout [-b] <name>
ds status [--missing] [--path P] [--type T] [--limit N]
ds describe <name> --design-file <file> | --design <text>          # 修改设计并推进版本
ds show <name> [--at <ref>] [--source]
ds list [--ref <ref>] [--path P] [--type T]
ds log [ref]
ds fetch [--branch <name>]
ds pull [--branch <name>]
ds push [--branch <name>]
ds pack [--output <file>]     # 默认打包 loose 并删除；--output 仅导出全部节点
ds repack                     # 全部打进一个 pack，删除旧 pack 和 loose
ds gc                         # 清不可达 + repack + 清旧 cache
ds verify <pack>
ds cat <hash>
ds ls
```

独立服务端 `ds-server`：

```text
ds-server serve --root <dir> --listen <addr> [--host H] [--user U] [--port P]
ds-server stdio --root <dir> --project <id>   # 给 sshd / ssh 用
ds-server stdio --repo <dir>                  # 兼容：显式仓库路径
ds-server project create --root <dir> --name <name> [--source <git-url>] [--host H] [--user U] [--port P]
ds-server project list --root <dir>
ds-server project show <name> --root <dir>
ds-server project set-source <name> --source <git-url> --root <dir>
ds-server gc --repo <dir>
ds-server repack --repo <dir>
```

约定：

- 输出是给人看的 git 风格文本；进度日志走 stderr，结果走 stdout；
- 工作区 = 运行 `ds init` 的目录；`--dir` 指工作区根目录，默认从当前目录向上找 `.ds`；工具状态只在 `<根>/.ds/` 里；
- `-q` 关闭进度日志，`-v` 输出详细信息；
- `init` 在返回前完成 bare clone；源码仅供智能体只读阅读，不产生元素；
- `clone <ds-url> [--source <git-url>] [--branch <name>] [<dir>]` 等价于在 `<dir>`（默认取 URL 最后一段）里 `init` 再 `pull` 选中的分支（默认 `main`、其次 `master`、否则唯一分支）；失败时删除自己新建的目录；源码仓库地址优先取 `--source`，否则取设计空间项目元数据里的 `source`（refs 响应里的 `source` 字段，创建项目时由 `--source` 或 Foundry 写入）；两者都没有时提示传 `--source`；
- `create` 输出元素名、推导/指定的父节点、新版本短 hash 和缺失设计数；每个元素一次版本提交；
- 元素是概念对象：名称、设计文本、可选源码锚点（`--path`、`--lines`）、可选父节点都由作者给出；工具不解析代码；
- `status` 输出当前分支、版本和源码 commit，然后每个元素一行 `type name path:lines designed|not designed`，末尾汇总；
- `branch` 像 `git branch`：`* name <短版本>`；
- `source branch` 像 `git branch`；`source status`、`source fetch`、`source checkout`、`source log` 用句子和短 hash；
- `describe` 输出一句结果和新版本短 hash；
- `pack` 输出打包节点数、新 pack 短 hash、删除的 loose 数量；`--output` 模式明确标注“仅导出”；
- `repack` 输出打包节点数、新 pack 短 hash、删除的 pack/loose 数量；`gc` 额外输出可达节点数和清理的过期 cache 数；
- `push` / `fetch` / `pull` 输出句子，例如 `Pushed main to origin: 7c9eed9b (12 node(s))`；无变化时是 `Everything up-to-date ...`；
- `init` 只创建本地仓库，不设置、不访问远端；远端由 `remote add/set-url <url>` 或 `clone <url>` 设置；支持 `http(s)://...`、`ssh://[user@]host[:port]/<id>` 和 `ds://[user@]host[:port]/<id>`（`ds://` 走 OpenSSH + `ds-server stdio --project <id>`）；`ds://localhost/<id>`、`ds://127.0.0.1/<id>` 在本机 root（`DESIGN_SPACE_ROOT`，默认 `~/.foundry/design-spaces`）存在该项目时直接走本地仓库，便于没有 sshd 的本地 Foundry；需要强制 SSH 时用 `ssh://localhost/<id>`；`<id>` 是项目 id，地址不含服务器路径，服务端在自己的 root 下解析；多段路径按兼容方式当作显式仓库路径；
- HTTP 认证用环境变量 `DS_TOKEN`（客户端和服务端都读），走 `Authorization: Bearer`；token 不进入命令行和日志；
- `show --source` 只输出元素源码：用元素的 commit + path 从 `.ds/source` 的 git blob 里按 byte span 取，不读工作区、不 checkout；`show` 和 `log` 里的 commit 用 git 原始十六进制（SHA-1 为 40 位），不是内部补零的 64 位；
- 命令行不接收 JSON 输入；
- `clone`、`init`、`remote add|show|set-url`、`source status|branch|fetch|checkout|log`、`create`、`branch`、`checkout`、`status`、`describe`、`show`、`list`、`log`、`pack`、`verify`、`cat`、`ls` 已经实现；
- `init` 完成 bare clone；源码只读，不 fetch，不 checkout，不执行代码；
- 没有代码解析器，也没有 tree-sitter 适配器；所有元素由 `ds create` 显式创建；
- 文件对象、语言元素都不会自动产生；智能体按需要创建概念元素，并用 `--path`/`--lines` 记录证据；
- `status` 默认只列语言元素；`status --files` 包含文件对象（显示为 file）；`list --files` 走 files 视图；`show <path>` 可以查看文件对象，`show <path> --source` 输出整个文件；
- `describe` 文件对象会报错（文件不总结）；

## 15. 不变式

- 节点不可变，hash 即地址；
- 语言元素的 design_hash 指向 DES，尚未写设计时为 0；文件对象恒为 0；
- ref 只能快进，更新必须 CAS；create/describe 各产生一个版本；
- 子节点变 → 视图路径上的 IDX 变 → 虚拟根变 → VER 变；
- 元素对象的 content/input hash 由工具计算；
- 上传下载按树差异走。

## 16. 已实现 / 下一步

已实现：

- 类型常量表；
- 节点编码/解码；
- hash；
- loose store；
- pack 写入/读取/校验；
- 本地分支：`branch`、`checkout`、`HEAD`；未产生版本的分支用 `branches/<name>` 标记；
- `create` 生成概念元素/根/VER 并推进分支；`describe` 生成新版本并推进分支；`status` 读当前版本；
- 元素由智能体/人显式创建：名称、可选设计、可选 `--path`/`--lines` 证据锚点、可选 `--parent`；
- 没有代码解析器、tree-sitter 适配器或自动文件元素；源码只读；
- 父子链接由 `--parent` 或已存在的 `::` 名称前缀推导；缺失 cache 时从名称重建；
- 每次 `create` / `describe`：自底向上算 hash、写 OBJ/DES/IDX/VER、建 views、原子推进 ref；缺少设计的元素 `design_hash = 0`；
- 读取：`status`、`show`（有锚点时可按 span 打印 `--source`）、`list`、`log`；
- 存储维护：`pack` 把 loose 节点打进 `packs/` 后删除 loose（`--output` 模式只导出、不删除）；`repack` 把全部节点合成一个 pack 并删旧 pack/loose；`gc` 从 refs 求可达闭包、重打包、删不可达节点和过期 tree cache；
- 服务端：独立二进制 `ds-server`；负责项目与设计空间仓库、多项目 HTTP API、`stdio`（给 sshd）以及服务端 `gc/repack`；
- 项目：ds 只按名字分配设计空间仓库并生成 `ds://user@host[:port]/<16 位 id>`，不要求 git 仓库；可选记录源码仓库地址（`project create --source`，由 Foundry 传入）供 `clone` 使用；git 必填是上层（Foundry）项目创建的业务规则；地址不含文件系统路径，服务端按 id 在 root 下解析；名字只是显示信息，允许重名——每次创建都分配新 id 和独立仓库，目录为 `<root>/<id>`；旧版本创建的 `<root>/<slug>` 目录仍可读；可用 `project list/show` 查看地址；
- 远端：`push`、`fetch`、`pull`；按版本根做树差异，只传变化节点；服务端校验 pack、祖先关系和节点闭包，ref 原子更新；
- 传输：HTTP（axum，`/v1/refs`、`/v1/fetch`、`/v1/packs`、`/v1/push`）；SSH 用系统 `ssh` 启动 `ds-server stdio`，同一套消息走 framed stdio；
- CLI：上述命令加 `init`、`pack`、`verify`、`cat`、`ls`；
- 61 个单元测试，clippy 通过。

已知限制：

- 概念元素的完整性和正确性依赖智能体阅读代码的质量；工具只做结构校验；
- 名称是分支内的身份；重命名需要创建新元素并迁移设计（尚无 rename 命令）；
- 父/子链接只存在本地 tree cache；从远端 clone 后按 `::` 名称重建；
- 源码锚点只是证据，不保证覆盖全部代码；

下一步：

1. 元素重命名/合并/拆分与设计迁移；
2. `packages` 视图与视图差异 `diff`；
3. 原生 SSH 监听（当前 SSH 通过 OpenSSH + `ds-server stdio` 提供）、TLS；
4. zstd 压缩、MIDX、repack/gc。
