# ds：详细设计（原 design）

- 状态：实现中
- 日期：2026-11-15
- 工程：`projects/design-space`
- 二进制：`ds`

## 1. 目标

把一个代码仓库的设计变成一套可版本化、可增量同步的内容寻址数据：

- 代码由工具自己解析，智能体只负责写设计说明；
- 每个语言元素是一个独立对象；
- 对象不可变，按内容 hash 寻址；
- 版本像 git commit，指向一棵由索引节点组成的树；
- 同一批对象可以按多种组织方式（namespace、file、package）浏览；
- 上传下载按树差异走，和改动量成正比。

## 2. 边界

工具负责：

- 拉取源码、解析、抽取元素；
- 计算 hash；
- 存储节点、打包；
- 版本、ref、同步。

工具不负责：

- 生成设计文本（智能体负责）；
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
    cache/trees/<version>.json  # 元素父子关系，供增量 scan 跳过未变文件
    cache/names/<version>.idx
    tmp/
```

源码仓库生命周期：

1. `ds init`：`git clone --bare` 到 `.ds/source`，把 clone 出来的 `refs/heads/*` 复制成 `refs/remotes/origin/*` 后全部删除；默认分支记到 `.ds/source-head`；
2. `ds source fetch`：显式更新 `refs/remotes/origin/*` 和 tags；`scan` 永远不 fetch；fetch 后再次清掉任何 `refs/heads`；
3. `ds source checkout <ref>`：只写 `source-head`（远端分支记 `branch <name>`，tag/commit 解析成 `commit <sha>`），不创建 work tree，也不创建本地分支；
4. `scan`：没有 ref 参数，只扫描当前 `source-head`；解析只用 `refs/remotes/origin/*`、`refs/tags/*` 和 commit SHA；
5. `git ls-tree -r -z` 列出 blob，`git cat-file blob` 读取原始字节。

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
scan     拉源码、解析、复用设计、算 hash，写 OBJ/DES/IDX，建虚拟根和 VER，推进当前分支 ref
describe 基于当前版本重建元素，写入设计，生成新的 OBJ/祖先 OBJ/IDX/VER，推进 ref
```

没有暂存区，也没有单独的 commit 命令：**scan 和 describe 本身就是一次版本提交**。

- scan 产生的版本允许元素没有设计，`design_hash = 0`；`missing` 就是不在这里的数量；
- 版本 parent 是当前分支 ref 指向的版本；ref 原子更新；
- 没变的 OBJ/DES 复用原节点，只有变化的元素、受影响的祖先、视图 IDX、虚拟根和 VER 是新的；
- `status`、`show`、`list`、`log` 都直接读当前版本。

### 10.1 本地分支

- 本地分支 = `refs/heads/<name>`；尚未产生版本的分支有一个 `branches/<name>` 标记文件；
- `HEAD` 记录当前分支，默认 `main`；
- `scan` 只写入当前分支；切分支用 `ds checkout <name>`，新分支用 `ds checkout -b <name>`；
- `checkout <name>` 切换当前分支；`checkout -b <name>` 创建并切换；
- `branch` 列出所有本地分支及其版本；`branch <name>` 创建但不切换；`branch -d <name>` 删除非当前分支；
- 每个分支独立复用自己基点版本的设计；不同分支互不影响；
- `show` / `list` / `log` 的 `--at` 缺省为当前分支。

### 10.2 只能向前扫描

- `scan` 没有 ref 参数：只扫描 `.ds/source-head` 当前的源码 ref；切换分支/commit 用 `ds source checkout`；
- 扫描得到的 commit 记录在版本根节点 `VER1.source_commit`；每个 OBJ 也记录自己的 commit，供 `show --source` 用；
- 基点 commit 取当前分支版本根节点的 `source_commit`；
- 新 commit 必须等于基点，或是基点的后代（`git merge-base --is-ancestor`）；否则 `scan` 拒绝：
  `scan only moves forward: <new> is not a descendant of <base> on branch <name>`；
- 空分支没有基点，可以扫描任意 commit；
- 目标 commit 等于基点时，`scan` 不产生新版本，直接输出 `branch=`、`commit=`、`status=up-to-date`；
- 不允许回退到旧 commit，也不允许切到与基点无祖先关系的其他分支历史。

### 10.3 增量扫描

- 用 git blob oid 对比基点和目标 commit 的文件列表：path 相同且 oid 相同即未变；
- 未变文件不读 blob、不解析 tree-sitter：
  - 从基点版本复用 OBJ 的 `content_hash`、span、类型；
  - 从 `.ds/cache/trees/<version>.json` 恢复 parent/children 和顺序；
  - design 由 `reuse_designs` 按 `input_hash` 拷回；子元素设计变了，父元素的 input 会在提交时重新算出；
- 变了的文件走完整解析；删除的文件不进入新版本；
- tree cache 是本地缓存：丢失（例如版本由远端 fetch 而来）时未变文件回退为重新解析，结果不变，只是变慢；
- 输出 `changed_files=` / `unchanged_files=` 表示实际解析和复用的文件数。

每次 `scan` 都会产生一个新版本；`describe` 也会产生一个新版本。设计编辑通过重建受影响的祖先 OBJ 来保持 `input_hash` 正确。

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
GET  /v1/refs                    → { refs: { name: version } }
POST /v1/fetch  { want, have }   → 原始 pack
POST /v1/packs  原始 pack         → { checksum }
POST /v1/push   { ref, old, new } → 200 ok / 409 conflict / 422 missing
```

SSH：

```text
ds serve --stdio --repo <dir>    # 由 sshd / ssh 启动
ssh://[user@]host[:port]/abs/path
```

客户端用系统 `ssh` 启动 `ds serve --stdio --repo <path>`，在同一 stdin/stdout 上跑同样的 4 种消息（1 refs、2 fetch、3 pack、4 push），帧格式为 `op u8 | len u32 LE | payload`，响应为 `status u8 | len u32 | payload`。

服务端不需要自己实现 SSH 协议，交给 OpenSSH（和 git 的 upload-pack 一样）。sshd 配置示例：

```text
Match User ds
  ForceCommand /usr/local/bin/ds serve --stdio --repo /srv/design
  AllowTcpForwarding no
  X11Forwarding no
```

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
ds init --source <url> [--remote <url>]   # 立即 clone
ds source status                          # 当前源码 ref 和 commit
ds source branch                          # 远端分支，git branch 风格
ds source fetch                           # 显式拉取；scan 不自动拉取
ds source checkout <branch|tag|commit>    # 切换源码 ref
ds source log [<ref>] [--limit N]         # 源码提交日志
ds scan                                   # 只扫当前 source-head，写入当前设计分支；不拉取
ds branch [<name>] [-d <name>]
ds checkout [-b] <name>
ds status [--missing] [--path P] [--type T] [--limit N]
ds describe <name> --design-file <file> | --design <text>
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
ds serve --listen <addr> --repo <dir>
ds serve --stdio --repo <dir>
```

约定：

- 输出是给人看的 git 风格文本；进度日志走 stderr，结果走 stdout；
- 工作区 = 运行 `ds init` 的目录；`--dir` 指工作区根目录，默认从当前目录向上找 `.ds`；工具状态只在 `<根>/.ds/` 里；
- `-q` 关闭进度日志，`-v` 输出每个文件的解析进度；
- `init` 在返回前完成 bare clone；`scan` 不 fetch，只扫描当前 `source-head`；
- `scan` 输出：当前分支、源码 ref 和短 commit、变化/未变/未解析文件数、扫描元素数、复用设计数、新版本短 hash 和缺失设计数；具体缺哪些设计用 `status --missing` 查；
- `status` 输出当前分支、版本和源码 commit，然后每个元素一行 `type name path:lines designed|not designed`，末尾汇总；
- `branch` 像 `git branch`：`* name <短版本>`；
- `source branch` 像 `git branch`；`source status`、`source fetch`、`source checkout`、`source log` 用句子和短 hash；
- `describe` 输出一句结果和新版本短 hash；
- `pack` 输出打包节点数、新 pack 短 hash、删除的 loose 数量；`--output` 模式明确标注“仅导出”；
- `repack` 输出打包节点数、新 pack 短 hash、删除的 pack/loose 数量；`gc` 额外输出可达节点数和清理的过期 cache 数；
- `push` / `fetch` / `pull` 输出句子，例如 `Pushed main to origin: 7c9eed9b (12 node(s))`；无变化时是 `Everything up-to-date ...`；
- remote URL 来自 `init --remote <url>`；支持 `http(s)://` 和 `ssh://[user@]host[:port]/abs/path`；
- HTTP 认证用环境变量 `DS_TOKEN`（客户端和服务端都读），走 `Authorization: Bearer`；token 不进入命令行和日志；
- `show --source` 只输出元素源码：用元素的 commit + path 从 `.ds/source` 的 git blob 里按 byte span 取，不读工作区、不 checkout；`show` 和 `log` 里的 commit 用 git 原始十六进制（SHA-1 为 40 位），不是内部补零的 64 位；
- 命令行不接收 JSON 输入；
- `init`、`source status|branch|fetch|checkout|log`、`scan`、`branch`、`checkout`、`status`、`describe`、`show`、`list`、`log`、`pack`、`verify`、`cat`、`ls` 已经实现；
- `init` 完成 bare clone；`scan` 只读本地对象，不 fetch，不 checkout，不执行代码；
- `scan` 已接入 tree-sitter 的 C++、Rust、Python、Go、Java 适配器；
- 每个 git blob 都有一个 `file` 对象；没有适配器、解析不出元素、非 UTF-8/超大/二进制只影响"是否解析"，不影响文件对象本身；`scan` 的“未解析”是本版本中没有语言元素的文件数，`-v` 对本次解析的失败文件列出原因；
- 文档、配置、锁文件等有文件对象，但不产生语言元素、不总结；
- `status` 默认只列语言元素；`status --files` 包含文件对象（显示为 file）；`list --files` 走 files 视图；`show <path>` 可以查看文件对象，`show <path> --source` 输出整个文件；
- `describe` 文件对象会报错（文件不总结）；

## 15. 不变式

- 节点不可变，hash 即地址；
- 语言元素的 design_hash 指向 DES，尚未写设计时为 0；文件对象恒为 0；
- ref 只能快进，更新必须 CAS；scan/describe 各产生一个版本；
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
- `scan` 生成对象/根/VER 并推进分支；`describe` 生成新版本并推进分支；`status` 读当前版本；
- tree-sitter 适配器：
  - C++：namespace/class/struct/union/enum/function/method/field/宏/include/#ifdef 条件块；类内声明与类外定义合并；
  - Rust：mod/struct/enum/trait/impl（方法合并到类型）/fn/const/static/type/宏/字段/变体；顶层名字按模块路径加前缀（`crate`、`src::foo` 等）；
  - Python：class/function/method/赋值（常量与变量）；顶层名字按模块路径加前缀；
  - Go：package/struct/interface/func/method/field/const/var/type；顶层名字按 package 加前缀；
  - Java：package/class/interface/enum/record/annotation_type/method/constructor/field/enum_constant/static initializer；名字按 package 分层；
- 文件对象：每个 blob 一个 `file` 对象（无设计），进入 files 视图；未解析的文件也照样有对象；
- `status --files`、`list --files`、`show <path> --source` 可以查看文件对象和未解析文件；
- `scan` 只能前进：新 commit 必须是分支基点 commit 的后代；
- `scan` 按 blob oid 跳过未变文件，只解析变化的文件；未变文件从基点版本和 tree cache 复用元素与设计；
- `scan` 复用 base version 的设计：自底向上算 content/input hash，`input_hash` 相同就拷回旧设计；父元素的 input 依赖子元素的 design_hash，所以子元素没命中时父元素不会误复用；
- 每次 `scan` / `describe`：自底向上算 hash、写 OBJ/DES/IDX/VER、建 views、原子推进 ref；缺少设计的元素 `design_hash = 0`；
- 读取：`show`（含按 span 打印源码的 `--source`）、`list`、`log`；
- 存储维护：`pack` 把 loose 节点打进 `packs/` 后删除 loose（`--output` 模式只导出、不删除）；`repack` 把全部节点合成一个 pack 并删旧 pack/loose；`gc` 从 refs 求可达闭包、重打包、删不可达节点和过期 tree cache；
- 远端：`serve`（HTTP / SSH stdio）、`push`、`fetch`、`pull`；按版本根做树差异，只传变化节点；服务端校验 pack、祖先关系和节点闭包，ref 原子更新；
- 传输：HTTP（axum，`/v1/refs`、`/v1/fetch`、`/v1/packs`、`/v1/push`）；SSH 用系统 `ssh` 启动 `ds serve --stdio`，同一套消息走 framed stdio；
- CLI：上述命令加 `init`、`pack`、`verify`、`cat`、`ls`；
- 59 个单元测试，clippy 通过。

已知限制：

- Go 的方法目前是顶层实体（`pkg::Type::Method`），没有挂到 struct 的 children 上；
- Java 的名字分隔符统一用 `::`，没有使用语言原生的 `.` / `#`；
- 条件编译块保留为容器，但同名实体在互斥分支里会用行号区分；
- 只有 C++、Rust、Python、Go、Java 有语法适配器，其余语言（包括 TS/JS）暂不产生元素，`scan` 会列入 skipped。

下一步：

1. 更多 tree-sitter 适配器；
2. `packages` 视图与视图差异 `diff`；
3. 原生 SSH 监听（当前 SSH 通过 OpenSSH + `ds serve --stdio` 提供）、TLS；
4. zstd 压缩、MIDX、repack/gc。
