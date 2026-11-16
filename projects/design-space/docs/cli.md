# design-space CLI

- 状态：第一版 CLI 已实现为 `ds`
- 适用模型：`docs/storage-format.md`
- 二进制：`ds`

第一版已实现：

```text
init
create（所有 kind）
edit（design、source、parent/namespace/file/module/name、继承/接口/mixin/override 关系）
dep add/rm/list（结构关系自动计入显式依赖）
source fetch/checkout/status/ls/cat/log
remote add/show/set-url/set-token
push/fetch/pull（本地目录、HTTP、SSH；HTTP 支持 Bearer token）
gc
repack
add
reset（全部或按名称）
diff（worktree/index 或 --staged）
rm
checkout（已有分支或 -b 新建）
ls
status
commit（--allow-empty）
show
log
```

网络传输由 `ds-server` 提供：

```bash
ds-server --root <remote-dir>                     # HTTP, 默认 127.0.0.1:7700
ds-server --root <remote-dir> --listen 0.0.0.0:7700
ds-server --root <remote-dir> --token-file <token-file>   # 启用 Bearer 认证
ds-server --stdio --root <remote-dir>             # SSH 传输协议
```

## 1. 基本约定

- 主要地址是 `qualified_name`。
- object hash 只作为内部身份、调试和消歧入口。
- 子对象通过 parent 建立，不存在 `--children`。
- kind 不可编辑。
- method 的 signature 不可编辑；改签名就是新 method。
- content_hash 由 source 计算，不能直接填写。
- 设计文档不是 inline 字段；每次修改生成新的 DocHash。

下面是完整语法。

```bash
ds init --source <git-url> [--dir <root>]

ds source fetch
ds source checkout <ref>
ds source status
ds source ls [<path>]
ds source cat <path>
ds source log [<ref>]

ds create <kind> <name> [options]
ds edit <qname> [--name <local>] [options]
ds rm <qname>

ds dep add <src> <dst> --explicit [--evidence path:lines]
ds dep add <src> <dst> --implicit [--rationale-file file]
ds dep rm <src> <dst> --explicit|--implicit
ds dep list <name>

ds status
ds diff
ds diff --staged
ds add <name>...
ds add -A
ds reset [<name>...]
ds reset --hard
ds commit -m "..." [--allow-empty]

ds checkout <branch> [--force]
ds checkout -b <branch>

ds show <name> [--source]
ds log [--limit N]
ds ls [--files|--namespaces|--design]
ds gc
ds repack
```

## 2. qualified name 生成

| kind | qualified name |
|---|---|
| directory / file / namespace | parent qname + `/` + name |
| 顶层 class / interface / function / mixin | namespace qname + `/` + name |
| 内部 class / interface / mixin | parent qname + `/` + name |
| method | parent qname + `/` + name + `#` + signature hash |
| part | parent qname + `/part#` + content hash |

根对象 `files`、`namespaces` 由 `init` 创建，不能通过 `create` 创建。

## 3. source 语法

```text
--source path            整个文件
--source path:start-end  1-based 行范围
```

- directory/file：只接受 path；
- namespace/class/interface/method/function/mixin：path 或 path:start-end；
- part：必须 path:start-end；
- source 原始字节计算 content_hash；
- 无 source 时 content_hash 为零；
- 有 source 时 content_hash 必须非零。

## 4. create 矩阵

| kind | 必填 | 可选 | 禁止 / 派生 |
|---|---|---|---|
| directory | `--parent` | `--source path`、`--design` | content_hash、dependencies、children |
| file | `--parent` | `--source path`、`--design` | dependencies、children |
| namespace | `--parent` | `--source path[:lines]`、`--design` | dependencies、children |
| class 顶层 | `--namespace` | `--file`、`--module`、`--source`、`--design`、`--superclass`、`--interface`、`--mixin` | — |
| class 内部 | `--parent` | `--source`、`--design`、`--superclass`、`--interface`、`--mixin` | namespace、file、module |
| interface 顶层 | `--namespace` | `--file`、`--module`、`--source`、`--design`、`--extends` | superclass、interface、mixin |
| interface 内部 | `--parent` | `--source`、`--design`、`--extends` | namespace、file、module、superclass、mixin |
| method | `--parent`、`--signature` | `--source`、`--design`、`--overrides` | children、superclass、interface、mixin |
| function | `--namespace` | `--file`、`--module`、`--source`、`--design` | parent、superclass、interface、mixin |
| part | `--parent`、`--source path:start-end` | `--design` | signature、namespace、file、module |
| mixin 顶层 | `--namespace` | `--file`、`--module`、`--source`、`--design`、`--on`、`--superclass`、`--interface`、`--mixin` | — |
| mixin 内部 | `--parent` | `--source`、`--design`、`--on`、`--superclass`、`--interface`、`--mixin` | namespace、file、module |

重复参数：

- `--interface` 可重复；
- `--mixin` 可重复；
- `--on` 可重复；
- `--superclass`、`--extends`、`--overrides` 最多一个。

## 5. edit 矩阵

```bash
ds edit <qname> [options]
```

通用：

- `--design` / `--design-file` / `--clear-design`
- `--source`：改变 source，重新计算 content_hash

| kind | 允许编辑 |
|---|---|
| directory | source、parent、design |
| file | source、parent、design |
| namespace | source、parent、design |
| class | source、parent/namespace/file/module、superclass、interface add/rm、mixin add/rm、design |
| interface | source、parent/namespace/file/module、extends、design |
| method | source、parent、overrides、design |
| function | source、file/namespace/module、design |
| part | source、parent、design |
| mixin | source、parent/namespace/file/module、on add/rm、superclass、interface add/rm、mixin add/rm、design |

不可编辑：

- kind；
- method signature；
- children（通过创建子对象或移动 parent）；
- content_hash；
- structural relation 不得重复加入 dependencies。

## 6. 校验

1. qname 在同一 parent 下唯一；part 显示名允许重复；
2. parent 类型必须符合 kind；
3. 顶层 class/interface/function/mixin 的 namespace 必须存在且非零；
4. 内部 class/interface/mixin 的 parent 必须是 class/interface；
5. method 的 parent 必须是 class/interface/part；
6. part 的 parent 必须是 class/interface；
7. superclass 必须是 class；
8. extends 必须是 interface；
9. interface 目标必须是 interface；
10. mixin 目标必须是 class/mixin；
11. on 目标必须是 class/mixin；
12. overrides 必须是 method，且位于关系闭包中；
13. source 类型必须匹配 kind；
14. 有 source 必须有 content_hash；part 必须两者都有；
15. 显式/隐式依赖分开，但 structural relation 自动计入显式依赖；
16. 依赖允许循环；
17. 关系不能自指；
18. 继承/接口/mixin/extends/on 关系不能成环；
19. source span 必须合法；
20. 所有引用 symbol/doc 必须存在，或提交时能解析。

## 7. 暂存语义

与 Git 对齐：

- 整个对象暂存，不做字段级暂存；
- `add` 一个对象时更新它所有父容器；
- `status` 按 object hash 去重，显示所有 qname；
- `diff` 展示对象级和字段级变化；
- `commit` 只用 index；
- `reset` 只取消暂存；
- `reset --hard` 丢弃 worktree；
- checkout 有未提交修改时拒绝，除非 `--force`；
- 空提交用 `--allow-empty`。

### 7.1 ls

```bash
ds ls                    # 所有对象
ds ls --files            # directory / file
ds ls --namespaces       # namespace / class / interface / method / function / part / mixin
ds ls --design           # 有设计文档的对象，输出 qname<TAB>dochash
```

### 7.2 diff

```bash
ds diff                  # worktree 对 index
ds diff --staged         # index 对 HEAD
```

输出：

```text
--- index
+++ worktree
M namespaces/app/Auth
  design: "old" -> "new"
A namespaces/app/New
D namespaces/app/Old
```

只比较对象字段；children 是派生字段，不单独显示。

### 7.3 rm

```bash
ds rm <qname>
```

- 根对象不能删除；
- 有子对象时拒绝，先删除子对象；
- 被关系或依赖引用时拒绝；
- 只修改 worktree，不自动暂存。

### 7.4 reset

```bash
ds reset                    # index 恢复到 HEAD，worktree 不动
ds reset <name>...          # 只恢复这些对象的 index 项
ds reset --hard             # index 和 worktree 都恢复到 HEAD
```

`add <name>` 会先补齐该对象在 index 中缺失的父容器。

### 7.5 checkout

```bash
ds checkout <branch>
ds checkout -b <branch>
ds checkout <branch> --force
```

切换分支时 index/worktree 必须与当前 HEAD 一致，否则拒绝；`--force` 丢弃修改。
`-b` 在当前 HEAD 上创建新分支并切换。

### 7.6 edit --name / reparent

`ds edit <qname> --name <local>` 修改对象在父容器下的本地名。修改 parent、
namespace 或本地名时，所有子孙对象的 qualified name、引用和依赖目标会一起迁移。

### 7.7 gc / repack

```bash
ds gc                        # 展开 pack，删除不可达对象/提交/文档/符号
ds repack                    # 把所有 loose records 收进一个 pack
```

store 读取时优先 loose file，找不到时回退到 pack。

## 8. 第一版范围

已实现命令见文首。

暂缓：

- 字段级暂存；
- 多设计空间共享对象。

## 9. 远程

remote URL：

| URL | 传输 |
|---|---|
| `/path` 或 `file:///path` | 本地目录 |
| `http://host:port[/prefix]` | `ds-server` HTTP API |
| `https://host[:port][/prefix]` | 同上，TLS |
| `ssh://[user@]host[:port]/absolute/path` | SSH 上运行 `ds-server --stdio --root <path>` |

`ds-server` 的存储根与工作区 `.ds` 使用相同的 `objects/`、`commits/`、
`docs/`、`symbols/`、`packs/` 和 `refs/heads/` 布局。

HTTP API：

```text
GET  /v1/refs                 -> "<branch>\t<commit>\n" ...
PUT  /v1/refs                 <- "<branch>\t<commit>\n" ...
GET  /v1/files                -> "<relative-path>\n" ...
GET  /v1/files/<relative>     -> file bytes
PUT  /v1/files/<relative>     <- file bytes
```

规则：

- push 先上传所有缺失的不可变文件，最后更新 ref；
- fetch 只下载本地缺失的文件，然后写入 `refs/remotes/origin/<branch>`；
- pull 先 fetch，再 fast-forward；index 或 worktree 有修改时拒绝；
- 服务端校验路径必须是 canonical 的 `<kind>/<aa>/<bb>/<hash>`，并校验上传内容与
  hash 一致；refs 只能指向已上传的 commit；
- HTTP 未配置 `--token-file` 时不认证；配置后所有 `/v1/*` 请求都必须带
  `Authorization: Bearer <token>`，否则返回 401；
- token 文件只包含一个 token，首尾空白会被忽略，token 内不能有空白或控制字符；
- 客户端用 `ds remote add <url> --token-file <path>` 配置 token 文件路径，
  用 `ds remote set-token [<path>]` 更换或清除；
- SSH 传输不单独校验 token，认证由 SSH 完成；`--stdio` 与 `--token-file` 不能同时使用；
- 没有内置 TLS。Bearer token 在纯 HTTP 上是明文；对外暴露时应使用反向代理或 SSH。
