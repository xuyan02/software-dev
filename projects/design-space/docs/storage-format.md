# 设计空间存储格式（草案）

- 状态：核心格式和 pack 已实现
- 日期：2026-11-16
- 工程：`projects/design-space`
- 适用范围：设计对象、设计文档、符号、提交、暂存状态、pack

本文只定义物理存储格式和 canonical encoding。对象命名、视图、依赖语义、暂存工作流由设计文档另行定义。

## 1. 基本约定

### 1.1 字节序

所有整数使用小端序。

```text
u8    1 字节
u16   2 字节
u32   4 字节
u64   8 字节
i64   8 字节，二进制补码
```

### 1.2 固定 hash

所有内部 hash 都是 32 字节 SHA-256 原始字节，不存十六进制文本。

```text
Hash32 = 32 bytes
```

文件名和 ref 中使用的 hash 是 32 字节 hash 的小写十六进制。

### 1.3 类型化 hash

以下 hash 在类型上不同，但物理上都是 32 字节：

```text
ObjectHash   设计对象 hash
CommitHash   提交 hash
DocHash      原始文本文档 hash
SymbolHash   符号 hash
GitOid       源码 git 仓库的 commit OID，不是设计空间内部 hash；
             固定 32 字节，零表示没有，按 source.object_format 解释
```

不同 hash 类型不共享索引空间：

- 设计对象单独存在 `objects/`；
- 提交单独存在 `commits/`；
- 设计文档单独存在 `docs/`；
- 符号单独存在 `symbols/`。

因此不使用域前缀。设计文档和符号如果文本完全相同，会产生相同的 32 字节 hash，
但它们在不同的存储空间和不同的索引中，不会互相冲突。

如果未来某个索引、缓存或 API 同时包含多种记录，它除了 hash 之外还必须保存记录类型，
查找时先按记录类型过滤，再用 hash 定位。hash 本身仍然是裸 32 字节，不拼接类型，
hash 的计算和存储都不包含类型。

pack 索引就是这种混合索引，见第 9 节。

### 1.4 文本（Text）

`Text` 是二进制记录内部的长度前缀文本编码：

```text
Text = u32 byte_len || UTF-8 bytes
```

- 不带 NUL 结尾；
- 不允许无效 UTF-8；
- 长度为 UTF-8 字节数，不是字符数；
- 不做 Unicode normalization；
- 空字符串合法。

### Text 的使用位置

当前使用 `Text` 的字段：

| 记录 | 字段 | 类型 |
|---|---|---|
| `CMT1` | `payload.message` | `Text` |
| `CMT1` | `payload.author` | `Text` |
| `DocumentPayload` | `title` | `Text` |
| `DocumentSectionPayload` | `title` | `Text` |
| `DocumentTextPayload` | `text` | `Text` |
| `DocumentTopicPayload` | `title` | `Text` |
| `Comment` | `text` | `Text` |

这里的 `payload.message` 表示：`CMT1` 记录中，`payload` 里的 `message` 字段。
没有其他使用位置。

### 不是 Text 的内容

| 内容 | 实际存储方式 |
|---|---|
| 设计对象的名字、路径、父对象、子对象、依赖目标 | `SymbolHash`，不是 `Text` |
| 设计文档正文 | `docs/<doc_hash>` 裸 UTF-8 文件，不是 `Text` |
| 符号文本 | `symbols/<symbol_hash>` 裸 UTF-8 文件，不是 `Text` |
| `HEAD`、refs、`source-head` | 普通文本文件，不是二进制记录里的 `Text` 字段 |
| pack 记录的长度 | pack record header，不是 `Text` 的 `u32 byte_len` |

设计对象 payload 中的相关字段：

```text
qualified_name       SymbolHash
parent refs          ParentRef / file / namespace / module
source.path          SymbolHash
dependency.target    SymbolHash
document            SymbolHash
dependency.rationale DocHash
```

设计文档和符号文件的内容是裸文本：

```text
.ds/docs/<doc_hash>       文件内容 = 裸 UTF-8 文本
.ds/symbols/<symbol_hash> 文件内容 = 裸 UTF-8 文本
```

### 1.5 可选值和全零 hash

#### 32 字节 hash 字段

可选 32 字节 hash 字段使用全零 hash 表示 None：

```text
NULL_HASH = 32 个 0x00 字节

hash 字段 = 全零     -> None
hash 字段 = 非全零   -> 有值
```

规则：

- 可选 hash 字段固定 32 字节，没有 tag；
- 全零 hash 永远不是合法的内容值；
- 必填 hash 字段必须非全零；
- 如果计算出的 hash 恰好是全零，按内部错误处理，不能写入存储；
- 全零 hash 只表示 None，不表示真实的 hash。

#### 非 hash 可选值

非 hash 的可选结构使用 tag：

```text
Opt<T> = u8 tag || T?
```

```text
tag = 0   值不存在，后面没有 T
tag = 1   值存在，后面紧跟 T
```

其他 tag 非法。

例如：

```text
Opt<DependencyEvidence> =
    00                 # 不存在
  | 01 || evidence     # 存在
```

`Source` 自己有 tag 0 表示没有 source，因此不再套一层 `Opt<Source>`。

### 1.6 列表

```text
List<T> = u32 count || T[count]
```

count 是元素数量，不是字节数。

集合语义的列表必须：

- 去重；
- 按本文规定的顺序排序；
- 不接受尾部多余字节。

### 1.7 canonical encoding

同一个逻辑对象必须得到完全相同的字节：

- 整数小端；
- 不填充；
- 不存平台相关的对齐；
- 所有集合排序；
- 不存可推导字段；
- 不使用 JSON/TOML/YAML；
- 不把十六进制文本作为 hash 存储。

## 2. 物理布局

```text
<workspace>/
  .ds/
    config
    HEAD
    source-head
    index
    worktree
    refs/
      heads/<branch>
      remotes/origin/<branch>
    source/                 # bare git clone
    objects/<aa>/<bb>/<object_hash>
    commits/<aa>/<bb>/<commit_hash>
    docs/<aa>/<bb>/<doc_hash>
    symbols/<aa>/<bb>/<symbol_hash>
    packs/<pack_sha256>.pack
    tmp/
```

- `objects/` 存 DSO1 设计对象记录；
- `commits/` 存 CMT1 提交记录；
- `docs/` 存纯文本设计文档；
- `symbols/` 存纯文本符号；
- `packs/` 存 pack 文件；
- `tmp/` 只存临时写入文件，不是 canonical storage；
- `docs/` 和 `symbols/` 不是设计空间节点，不进入 View 树，也不进入提交链；
- 所有内容寻址文件的文件名都是内容 hash 的十六进制；
- 远程存储根使用同样的 `objects/`、`commits/`、`docs/`、`symbols/`、
  `packs/`、`refs/heads/` 布局，不包含工作区私有的 source/index/worktree。传输规则见
  `docs/cli.md` 第 10 节。

### 2.1 tmp/

`tmp/` 不是通用临时目录，只用于**准备并原子发布最终文件**。

规则是：

```text
写 tmp/<临时文件>
  -> 校验完整
  -> rename 到最终位置
```

这样其他进程不会看到写了一半的 canonical 文件。对不可变对象、文档、符号、ref、
index、worktree 都一样。

#### 具体使用位置

| 操作 | tmp 中的临时内容 | rename 目标 |
|---|---|---|
| 写入设计对象 | `tmp/object-<random>.tmp` | `objects/<aa>/<bb>/<object_hash>` |
| 写入提交 | `tmp/commit-<random>.tmp` | `commits/<aa>/<bb>/<commit_hash>` |
| 写入设计文档 | `tmp/doc-<random>.tmp` | `docs/<aa>/<bb>/<doc_hash>` |
| 写入符号 | `tmp/symbol-<random>.tmp` | `symbols/<aa>/<bb>/<symbol_hash>` |
| 接收 pack | `tmp/pack-<checksum>.tmp` | `packs/<pack_sha256>.pack` |
| 生成、导出 pack | `tmp/pack-<random>.tmp` | 目标 pack 路径 |
| 解包 pack | `tmp/unpack-<random>/` | 逐条 rename 到 `objects/`、`docs/`、`symbols/`、`commits/` |
| 更新 HEAD | `tmp/head-<random>.tmp` | `.ds/HEAD` |
| 更新分支 ref | `tmp/ref-<random>.tmp` | `refs/heads/<branch>` |
| 更新 index | `tmp/index-<random>.tmp` | `.ds/index` |
| 更新 worktree | `tmp/worktree-<random>.tmp` | `.ds/worktree` |
| 首次 clone 源码 | `tmp/source-<random>/` | `.ds/source/` |

`tmp/` 中可以有设计文档或符号的**临时副本**，但它们不是最终设计文档，也不是最终符号。
最终内容仍然只在 `docs/`、`symbols/`。

#### 为什么不用机器上的 `/tmp`

关键原因是原子 `rename`：

- `rename(2)` 只有在同一文件系统内才是原子的；
- 机器的 `/tmp` 经常是 tmpfs，或者位于根文件系统；
- workspace 可能位于另一块磁盘或另一个挂载点；
- 从 `/tmp` rename 到 `.ds/` 时，内核返回 `EXDEV`（cross-device link）；
- `mv` 命令会隐藏这个错误，退化成“复制 + 删除”，但复制不是原子的；
- 即使先复制到目标文件系统，仍然需要在目标文件系统里创建一个临时文件再 rename，
  这实际上就是 `.ds/tmp`；
- `/tmp` 还可能被系统清理、被其他用户共享、有容量限制，生命周期不受 workspace 控制。

例如：

```text
/tmp                 /dev/nvme0n1p2   ext4  根文件系统
/media/.../workspace /dev/sdb2        ext4  另一块磁盘
```

这种情况下：

```text
rename("/tmp/object.tmp", "/media/.../.ds/objects/...")
```

会失败。只能先复制到 workspace 文件系统，再 rename。

`.ds/tmp` 和所有最终目录在同一个文件系统，`rename` 可以直接使用。

#### tmp 的约束

- `tmp/` 内容不是对象、符号、文档、提交或版本；
- 不参与 hash，不进入 pack，不同步到远端；
- 没有运行时操作时可以删除；
- 进程崩溃后留下的文件在下次启动或 `gc` 时清理；
- 临时文件名必须唯一，避免冲突和 symlink 攻击；
- 读取者只接受完整校验通过并已落在最终目录的文件。

`tmp/` 不用于最终保存：

- 源码；
- 设计文档；
- 符号；
- 可恢复的工作状态；
- 需要同步的数据。

它只是这些内容发布过程中的中间文件。

### 2.2 config

`.ds/config` 是 TOML。源码相关字段：

```toml
[source]
url = "https://example.com/repo.git"
object_format = "sha1" # 或 "sha256"
```

- `source.object_format` 是源码 Git 仓库的 object format；
- 它由 `git rev-parse --show-object-format` 在 init 时确定；
- 它属于整个设计空间，不属于单个 View；
- Git 仓库的 object format 是仓库级属性，不能逐 commit 混用；
- 没有 source 时，`[source]` 可以不存在；此时 View 的 `source_commit` 必须是零；
- `source.url` 和 `source.object_format` 在 init 后不再改变。

`<aa>/<bb>` 是 hash 十六进制的前两个字节：

```text
hash = 0123456789abcdef...
path = objects/01/23/0123456789abcdef...
```

## 3. 记录类型

物理文件按目录和 magic 区分：

| 类型 | magic | 目录 | 内容 |
|---|---|---|---|
| 设计对象 | `DSO1` | `objects/` | 二进制 |
| 提交 | `CMT1` | `commits/` | 二进制 |
| 设计文档 | 无 | `docs/` | 纯 UTF-8 文本 |
| 符号 | 无 | `symbols/` | 纯 UTF-8 文本 |
| pack | `PAK1` | `packs/` | 二进制 |

设计文档和符号没有文件头，文件内容就是文本本身。类型由目录决定。

## 4. 设计对象 DSO1

### 4.1 文件结构

```text
DSO1 =
    "DSO1"      4 bytes
    version:u8  1 byte
    kind:u8     1 byte
    payload:bytes
```

当前：

```text
version = 1
```

`payload` 从 header 之后一直延伸到记录末尾。独立文件以文件末尾为界；pack 中的
记录以 pack record 的 `original_size` 为界。读取端必须恰好消费完 payload，不能
留下尾部字节。

### 4.2 对象 hash

```text
object_hash = SHA-256(
    version:u8
    || kind:u8
    || payload
)
```

`version`、`kind`、`payload` 参与 hash；magic 不参与 hash。

`kind` 参与 hash，因为所有设计对象共用一个 `objects/` 索引，不同 kind 必须区分。
这不是域前缀，而是对象本身的一部分。

读取时必须校验：

```text
SHA-256(...) == 文件名中的 hash
```

### 4.3 kind

```text
0x01  view
0x02  directory
0x03  file
0x04  namespace
0x05  class
0x06  interface
0x07  method
0x08  function
0x09  part
0x0A  mixin
0x0C  inner-class
0x0D  inner-interface
0x0E  document-directory
0x0F  document
0x10  document-section
0x11  document-list
0x12  document-text
0x13  document-topic
0x14  design-structure
0x15  system
0x16  module
0x17  concept-directory
0x18  concept
0x19  layer
0x1A  document-block
0x1B  document-mermaid
0x1C  document-html
```

`0x0B` 保留。逻辑模块使用 0x16；顶层代码对象上的 `module` 字段仍是源码语言模块引用，不是这里的逻辑模块。

### 4.4 字段使用

不存在适用于所有 kind 的 `ObjectCommon`。每个 kind 明确列出自己的字段。

| kind | qualified_name | source | content_hash | document ref | dependencies | children |
|---|---|---|---|---|---|---|
| view | 无 | 无 | 无 | 无 | 无 | 固定结构子对象 |
| directory | 有 | `Opt<PathSource>` | 无 | `SymbolHash`，零表示无 | 无 | `List<ObjectHash>` |
| file | 有 | `Opt<PathSource>` | `Hash32`，零表示无 | 同上 | 无 | `List<ObjectHash>` |
| namespace | 有 | `Opt<SpanSource>` | 同上 | 同上 | 无 | `List<ObjectHash>` |
| class/interface | 有 | `Opt<SpanSource>` | 同上 | 同上 | 有，显式/隐式 | `List<ObjectHash>` |
| method/function | 有 | `Opt<SpanSource>` | 同上 | 同上 | 有，显式/隐式 | 无 |
| part | 有 | `SpanSource` 必填 | `Hash32` 必填非零 | 无 | 有，显式/隐式 | `List<ObjectHash>` |
| mixin | 有 | `Opt<SpanSource>` | 同上 | 同上 | 有，显式/隐式 | `List<ObjectHash>` |
| document-directory | 有 | 无 | 无 | 无 | 无 | `List<ObjectHash>` |
| document | 有 | 无 | 无 | 无 | 无 | `List<ObjectHash>` |
| document-section | 有 | 无 | 无 | 无 | 无 | `List<ObjectHash>` |
| document-list | 有 | 无 | 无 | 无 | 无 | `List<ObjectHash>` |
| document-text | 有 | 无 | 无 | 无 | 无 | 无 |
| document-topic | 有 | 无 | 无 | 无 | 无 | `List<ObjectHash>` |
| document-block | 有 | 无 | 无 | 无 | 无 | 无 |
| document-mermaid | 有 | 无 | 无 | 无 | 无 | 无 |
| document-html | 有 | 无 | 无 | 无 | 无 | 无 |
| design-structure | 有 | 无 | 无 | `SymbolHash`，零表示无 | 无 | `List<ObjectHash>` |
| system | 有 | 无 | 无 | `SymbolHash`，零表示无 | 无 | `List<ObjectHash>` |
| module | 有 | 无 | 无 | `SymbolHash`，零表示无 | 无 | `List<ObjectHash>` |
| concept-directory | 有 | 无 | 无 | 无 | 无 | `List<ObjectHash>` |
| concept | 有 | 无 | 无 | 无 | 无 | 无 |
| layer | 有 | 无 | 无 | `SymbolHash`，零表示无 | 无 | `List<ObjectHash>` |

规则：

- `qualified_name` 是完整限定名的符号，必须非全零；
- `content_hash` 是源码片段原始字节的 SHA-256；零表示没有；有 source 时必须非零，非零时必须有 source；
- File 的 content_hash 是完整文件字节的 SHA-256；
- SpanSource 的 content_hash 是 `[start_byte, end_byte)` 原始字节的 SHA-256；
- 不转换换行、不去 BOM、不做 Unicode normalization；
- 空 span 的 content_hash 是 SHA-256(空字节)，不是零；
- Directory 和 View 没有 content_hash；
- `document` 零表示没有文档引用；
- `dependencies` 只允许出现在 class/interface/method/function/part/mixin；
- `children` 只允许出现在 view/directory/file/namespace/class/interface/part/mixin/document-directory/document/document-section/document-list/document-topic/concept-directory/system/module/layer；
- `children` 存 ObjectHash，不存子对象名字。

### 4.4.1 Parent

parent 不是统一字段。按 kind 区分：

```text
ParentRef =
    tag:u8
    tag 1 = View
    tag 2 = Object(SymbolHash)
```

| kind | parent |
|---|---|
| view | 无 |
| directory | `ParentRef`：根 directory 是 View，其他是 Directory |
| file | `SymbolHash`：Directory |
| namespace | `ParentRef`：虚拟根 namespace 是 View，其他是 Namespace |
| class/interface 顶层 | `file`、`namespace`、`module` 三个显式字段 |
| class/interface 内部 | `parent:SymbolHash`，上级 class/interface |
| function | `file`、`namespace`、`module` 三个显式字段 |
| method | `parent:SymbolHash`，class/interface/part |
| part | `parent:SymbolHash`，原对象 |
| mixin 顶层 | `file`、`namespace`、`module` 三个显式字段 |
| mixin 内部 | `parent:SymbolHash`，上级 class/interface |
| design-structure | `ParentRef`：View |
| system | `parent:SymbolHash`，DesignStructure |
| module | `parent:SymbolHash`，System、Module 或 Layer |
| layer | `parent:SymbolHash`，System 或 Module |
| concept-directory | `ParentRef`：根 `concepts` 是 View，其他是 ConceptDirectory |
| concept | `parent:SymbolHash`，ConceptDirectory |
| document-directory | `ParentRef`：根 `documents` 是 View，其他是 DocumentDirectory |
| document | `parent:SymbolHash`，DocumentDirectory |
| document-section | `parent:SymbolHash`，Document 或 DocumentSection |
| document-list | `parent:SymbolHash`，DocumentSection、DocumentTopic 或 DocumentList |
| document-text | `parent:SymbolHash`，DocumentSection、DocumentTopic 或 DocumentList |
| document-topic | `parent:SymbolHash`，DocumentSection |
| document-block | `parent:SymbolHash`，DocumentSection 或 DocumentTopic |
| document-mermaid | `parent:SymbolHash`，DocumentSection 或 DocumentTopic |
| document-html | `parent:SymbolHash`，DocumentSection 或 DocumentTopic |

顶层 class/interface/function/mixin 的规则：

- `namespace` 必须非零；
- `file` 可以为零，表示设计先于源码；
- `module` 可以为零，模块对象以后定义；
- `file` 非零时必须是 file；
- `namespace` 非零时必须是 namespace；
- `module` 非零时必须是已存在的源码语言模块引用；逻辑模块是独立对象，不通过这个字段表达；
- 三个字段都记录完整的 SymbolHash，不使用 parent 列表。

内部 class/interface/mixin：

- 只允许 class/interface 作为 parent；
- 不允许 module、namespace、file；
- 没有 file/namespace/module 三字段。

### 4.4.2 Class relations

Class 还记录语义继承、接口实现和 mixin：

```text
superclass:SymbolHash
interfaces:List<SymbolHash>
mixins:List<SymbolHash>
```

- `superclass`：0 或 1 个，目标必须是 class；
- `interfaces`：0..N，目标必须是 interface，排序去重；
- `mixins`：0..N，目标可以是 class 或 mixin，保留列表顺序，去重；
- `superclass`、`interfaces`、`mixins` 不能指向同一个对象；
- 这些是结构关系，不重复写入 `dependencies`；
- C++ 中一个 base class 属于 superclass 还是 mixin，由智能体分析意图后决定；
- 不记录 C++ 源码中 superclass 与 mixins 之间的相对顺序；
- `mixins` 的顺序仍然保留，因为 Dart mixin 的顺序影响方法解析。

### 4.4.3 Mixin relations

Mixin 还记录 Dart/C++ 的关系：

```text
on:List<SymbolHash>
superclass:SymbolHash
interfaces:List<SymbolHash>
mixins:List<SymbolHash>
```

- `on`：Dart 的 `on` 约束，目标 class 或 mixin，排序去重；
- `superclass`：C++ mixin 类自身的语义父类，0/1，目标 class；
- `interfaces`：mixin 实现的接口，目标 interface，排序去重；
- `mixins`：mixin 自身复用的 mixin，目标 class 或 mixin，保留顺序，去重；
- 这些是结构关系，逻辑上自动进入显式依赖，不重复存储；
- C++ 中一个源码类只创建一个设计对象，由智能体根据用途选择 kind=class 或 kind=mixin。

source 不是一种统一结构。按 kind 使用不同结构：

```text
PathSource =
    path:SymbolHash
```

```text
SpanSource =
    path:SymbolHash
    start_byte:u64
    end_byte:u64
    start_line:u32
    end_line:u32
```

- `[start_byte, end_byte)` 是半开字节区间；
- `[start_line, end_line]` 是 1-based 包含区间；
- `end_byte >= start_byte`；
- `end_line >= start_line`。

Source anchor 与 `View.source_commit` 相互独立：

- 对象可以有 source anchor，即使 `View.source_commit` 为零；
- `View.source_commit` 为零时，anchor 仍表示源码路径和片段，只是不绑定具体 git 版本；
- source anchor 不存储 commit，commit 只来自 View；
- `content_hash` 由 source 片段字节计算，不要求 View 有 source_commit。

### 4.5 View

```text
ViewPayload =
    source_commit:GitOid
    files:ObjectHash
    namespaces:ObjectHash
    design:ObjectHash
    documents:ObjectHash
    concepts:ObjectHash
```

- View 没有 qualified_name；
- View 没有 parent 字段；
- View 没有 source；
- View 没有 content_hash；
- View 没有 document；
- View 没有 dependencies；
- `source_commit` 使用 `GitOid` 的零值表示没有源码版本；
- `files`、`namespaces` 必须非零；
- `documents` 允许为零，表示早期 View 还没有文档根；新 View 必须非零；
- `design` 当前允许是 32 字节全零，表示 design 视图尚未定义；
- `concepts` 允许是 32 字节全零，表示概念空间尚未建立；`init` 和 `ds-server` 初始化会创建它；
- `concepts` 非零时 `documents` 必须非零；
- canonical encoding 省略全零的尾部字段：`documents` 为零时记录在 `design` 后结束；`documents` 非零而 `concepts` 为零时不写 `concepts`；这样旧 View 的 hash 保持不变；
- 根 directory 的 qualified name 是保留符号 `files`；
- 虚拟根 namespace 的 qualified name 是保留符号 `namespaces`；
- 根 document directory 的 qualified name 是保留符号 `documents`；
- 根 concept directory 的 qualified name 是保留符号 `concepts`；
- 根 directory 的 parent 是 View；
- 虚拟根 namespace 的 parent 是 View；
- 根 document directory 的 parent 是 View；
- 根 concept directory 的 parent 是 View；
- View 的结构性子对象；根 directory、虚拟根 namespace、根 document directory 和根 concept directory 的 parent 固定为 View；
- 顶层 namespace 的 qualified name 是 `namespaces/<name>`；
- 全局命名空间中的 class/interface/function/mixin 的 namespace parent 是 `namespaces`，qualified name 形如 `namespaces/<Name>`；
- `files`、`namespaces`、`documents` 和 `concepts` 是保留限定名，普通对象不能使用。

```text
GitOid = 32 bytes
```

- 32 字节全零表示没有 source_commit；
- 非零表示源码 git 仓库的 commit OID；
- 具体是 20 字节 SHA-1 还是 32 字节 SHA-256，由 `.ds/config` 的
  `source.object_format` 解释；
- `source.object_format = "sha1"` 时，前 20 字节是 OID，后 12 字节必须为零；
- `source.object_format = "sha256"` 时，32 字节全部是 OID；
- 无论哪种格式，OID 本身不能是全零；
- View 不保存算法字段。

Git 仓库的 object format 是仓库级属性。一个仓库的所有 native commit 使用同一种
算法，不能逐 commit 混用 SHA-1 和 SHA-256。

`source.object_format` 在 init 时通过源码仓库获取：

```sh
git rev-parse --show-object-format
```

输出 `sha1` 或 `sha256`。

`GitOid` 不是设计空间的 `object_hash`、`commit_hash`、`doc_hash` 或
`symbol_hash`。设计空间不重新计算它，也不使用自己的域前缀；它只是
`View.source_commit` 指向的源码版本标识。

### 4.6 Directory

```text
DirectoryPayload =
    qualified_name:SymbolHash
    parent:ParentRef
    source:Opt<PathSource>
    document:SymbolHash
    children:List<ObjectHash>
```

### 4.7 File

```text
FilePayload =
    qualified_name:SymbolHash
    parent:SymbolHash
    source:Opt<PathSource>
    content_hash:Hash32
    document:SymbolHash
    children:List<ObjectHash>
```

### 4.8 Namespace

```text
NamespacePayload =
    qualified_name:SymbolHash
    parent:ParentRef
    source:Opt<SpanSource>
    content_hash:Hash32
    document:SymbolHash
    children:List<ObjectHash>
```

### 4.9 Class / Interface

```text
ClassPayload =
    qualified_name:SymbolHash
    parent:ClassParent
    source:Opt<SpanSource>
    content_hash:Hash32
    document:SymbolHash
    superclass:SymbolHash
    interfaces:List<SymbolHash>
    mixins:List<SymbolHash>
    explicit_dependencies:List<Dependency>
    implicit_dependencies:List<Dependency>
    children:List<ObjectHash>

InterfacePayload =
    qualified_name:SymbolHash
    parent:ClassParent
    source:Opt<SpanSource>
    content_hash:Hash32
    document:SymbolHash
    extends:SymbolHash
    explicit_dependencies:List<Dependency>
    implicit_dependencies:List<Dependency>
    children:List<ObjectHash>
```

```text
ClassParent =
    tag:u8
    tag 1 = TopLevel:
        file:SymbolHash
        namespace:SymbolHash
        module:SymbolHash
    tag 2 = Nested:
        parent:SymbolHash
```

顶层 class/interface 的 `namespace` 必须非零；`file` 和 `module` 可以为零。
内部 class/interface 的 `parent` 只能指向 class/interface。

Interface 的 `extends` 为零表示没有父接口；非零时只能指向一个 interface。

### 4.10 Method / Function

```text
MethodPayload =
    qualified_name:SymbolHash
    parent:SymbolHash
    source:Opt<SpanSource>
    content_hash:Hash32
    document:SymbolHash
    overrides:SymbolHash
    explicit_dependencies:List<Dependency>
    implicit_dependencies:List<Dependency>

FunctionPayload =
    qualified_name:SymbolHash
    file:SymbolHash
    namespace:SymbolHash
    module:SymbolHash
    source:Opt<SpanSource>
    content_hash:Hash32
    document:SymbolHash
    explicit_dependencies:List<Dependency>
    implicit_dependencies:List<Dependency>
```

Method 的 `parent` 指向 class/interface/part。

Function 的 `namespace` 必须非零；`file` 和 `module` 可以为零。

Method/Function 没有 children。

### source 约定

Method/Function 的 `source` 指向实现（definition）：

- C++ 成员函数：指向 `.cpp` 中的定义；
- C++ inline/header-only：指向头文件中的定义；
- C 函数：指向 `.c` 中的定义；
- pure virtual / abstract / interface 方法：没有实现，指向声明；
- 声明和实现不拆成两个设计对象，声明位置不单独存储。

Method 的 `overrides`：

- 零表示没有重写；
- 非零时必须指向一个 method；
- 每个方法最多重写一个方法；
- 目标必须位于 superclass、interfaces、mixins 的关系闭包中；
- 签名必须匹配。

### 4.11 Part

```text
PartPayload =
    qualified_name:SymbolHash
    parent:SymbolHash
    source:SpanSource
    content_hash:Hash32
    explicit_dependencies:List<Dependency>
    implicit_dependencies:List<Dependency>
    children:List<ObjectHash>
```

Part 和普通对象不同：

- `source` 必填，类型是 `SpanSource`；
- `content_hash` 必填且不能是全零；
- qualified name 的本地名必须是 `part#<content_hash_hex>`；
- `content_hash` 是 source span 原始字节的 SHA-256，不包含位置；
- 同一父对象下允许出现 content_hash 相同的 part；这是显示名重复，不是对象身份冲突；
- 内容完全相同的 part 是重复数据，智能体应忽略或合并；
- `parent` 必须指向原对象（class/interface）；
- `children` 只能包含 method/function。

### 4.12 Mixin

```text
MixinPayload =
    qualified_name:SymbolHash
    parent:ClassParent
    source:Opt<SpanSource>
    content_hash:Hash32
    document:SymbolHash
    on:List<SymbolHash>
    superclass:SymbolHash
    interfaces:List<SymbolHash>
    mixins:List<SymbolHash>
    explicit_dependencies:List<Dependency>
    implicit_dependencies:List<Dependency>
    children:List<ObjectHash>
```

规则：

- 顶层 mixin 的 `namespace` 必须非零；`file`、`module` 可以为零；
- 内部 mixin 的 `parent` 只能指向 class/interface；
- `on` 是 Dart `on` 约束，目标 class/mixin，排序去重；
- `superclass` 0/1，目标 class；
- `interfaces` 目标 interface，排序去重；
- `mixins` 目标 class/mixin，保留顺序，去重；
- `on`、`superclass`、`interfaces`、`mixins` 逻辑上自动进入显式依赖，不重复存储；
- `children` 允许 class/interface/method/part；
- C++ 中一个源码类只创建一个设计对象，由智能体根据用途选择 kind=class 或 kind=mixin；
- 不创建 class/mixin 双重对象。

### 4.12.1 文档对象

文档片段也是设计对象，和源码对象共用 View、对象索引、提交和 pack。

```text
DocumentDirectoryPayload =
    qualified_name:SymbolHash
    parent:ParentRef
    children:List<ObjectHash>
    comments:List<Comment>

DocumentPayload =
    qualified_name:SymbolHash
    parent:SymbolHash
    title:Text
    sections:List<SymbolHash>
    children:List<ObjectHash>
    comments:List<Comment>

DocumentSectionPayload =
    qualified_name:SymbolHash
    parent:SymbolHash
    title:Text
    body:List<SymbolHash>
    sections:List<SymbolHash>
    children:List<ObjectHash>
    comments:List<Comment>

DocumentListPayload =
    qualified_name:SymbolHash
    parent:SymbolHash
    items:List<SymbolHash>
    children:List<ObjectHash>
    comments:List<Comment>

DocumentTextPayload =
    qualified_name:SymbolHash
    parent:SymbolHash
    text:Text
    comments:List<Comment>

DocumentTopicPayload =
    qualified_name:SymbolHash
    parent:SymbolHash
    title:Text
    paragraphs:List<SymbolHash>
    children:List<ObjectHash>
    comments:List<Comment>

BlockPayload =
    qualified_name:SymbolHash
    parent:SymbolHash
    language:Text
    text:Text
    comments:List<Comment>

MermaidBlockPayload =
    qualified_name:SymbolHash
    parent:SymbolHash
    text:Text
    comments:List<Comment>

HtmlBlockPayload =
    qualified_name:SymbolHash
    parent:SymbolHash
    text:Text
    comments:List<Comment>

DesignStructurePayload =
    qualified_name:SymbolHash
    parent:ParentRef
    title:Text
    document:SymbolHash
    children:List<ObjectHash>

SystemPayload =
    qualified_name:SymbolHash
    parent:SymbolHash
    title:Text
    document:SymbolHash
    children:List<ObjectHash>

ModulePayload =
    qualified_name:SymbolHash
    parent:SymbolHash
    title:Text
    document:SymbolHash
    members:List<SymbolHash>
    children:List<ObjectHash>

LayerPayload =
    qualified_name:SymbolHash
    parent:SymbolHash
    title:Text
    order:u32
    document:SymbolHash
    members:List<SymbolHash>
    children:List<ObjectHash>

ConceptDirectoryPayload =
    qualified_name:SymbolHash
    parent:ParentRef
    children:List<ObjectHash>

ConceptPayload =
    qualified_name:SymbolHash
    parent:SymbolHash
    name:Text
    summary:Text
    explanation:Text
    characteristics:List<Text>
    relations:List<Text>
    aliases:List<Text>

Comment =
    id:u32
    resolved:u8      # 0 open, 1 resolved
    text:Text
```

规则：

- 根 `documents` 是 DocumentDirectory，parent 为 View；
- 其他 DocumentDirectory 的 parent 必须是 DocumentDirectory；
- Document 的 parent 必须是 DocumentDirectory；
- DocumentSection 的 parent 必须是 Document 或 DocumentSection；
- DocumentSection 的 `body` 允许 DocumentText、DocumentList、DocumentTopic、DocumentBlock、DocumentMermaid、DocumentHtml；
- DocumentTopic 的 parent 必须是 DocumentSection；
- DocumentTopic 的 `paragraphs` 允许 DocumentText、DocumentList、DocumentBlock、DocumentMermaid、DocumentHtml；
- DocumentList 的 parent 必须是 DocumentSection、DocumentTopic 或 DocumentList；
- DocumentList 的 `items` 允许 DocumentText、DocumentList；
- DocumentText 的 parent 必须是 DocumentSection、DocumentTopic 或 DocumentList；
- DocumentBlock 的 parent 必须是 DocumentSection 或 DocumentTopic；
- DocumentMermaid 的 parent 必须是 DocumentSection 或 DocumentTopic；
- DocumentHtml 的 parent 必须是 DocumentSection 或 DocumentTopic；
- DesignStructure 的 qname 固定为 `design`，parent 为 View；它可选存在，存在时是结构树根；
- System 的 parent 必须是 DesignStructure；System、Module 和 Layer 的 qname 都从 `design/` 开始；
- Module 的 parent 必须是 System、Module 或 Layer；
- Layer 的 parent 必须是 System 或 Module，不能嵌套另一个 Layer；
- Layer 的 `order` 从 1 开始，同一父对象下的 Layer `order` 互不相同；只有 `order` 相差 1 的 Layer 之间允许依赖；
- 一个有 Layer 的 System/Module 不能直接拥有 `members` 或子 Module；这些必须放进 Layer；
- System/Module/Layer 的 `members` 只允许顶层 Class/Interface/Function/Mixin，且一个成员在所有容器中最多出现一次；
- 根 `concepts` 是 ConceptDirectory，parent 为 View；
- ConceptDirectory 的 parent 必须是 View 或 ConceptDirectory；
- Concept 的 parent 必须是 ConceptDirectory；Concept 的 qname 从 `concepts/` 开始；
- Concept 的 `name` 和 `summary` 是非空单行文本，`explanation` 允许为空；
- `characteristics`、`relations`、`aliases` 的每一项都是非空单行文本；
- System/Module/Layer 的 `children` 是子模块或层的 ObjectHash，按 hash 排序。
- `sections`、`body`、`paragraphs`、`items` 是**有序**的 qname 引用，不做排序；
- 同一个有序列表内不允许重复 qname；
- `children` 仍然存全部直接子对象的 ObjectHash，按 ObjectHash 升序排序；
- DocumentText 没有 children；DocumentBlock 没有 children；DocumentMermaid 没有 children；DocumentHtml 没有 children；
- `title` 和 DocumentText 的 `text` 都是单行文本，不能为空，不能包含 `\n` 或 `\r`；
- DocumentBlock 的 `language` 是单行文本，可为空，不能包含 `\n` 或 `\r`；
- DocumentBlock 的 `text` 非空，允许换行，按原样保存代码内容；
- DocumentMermaid 的 `text` 非空，允许换行，按原样保存 Mermaid 源码；
- DocumentHtml 的 `text` 非空，允许换行，按原样保存 HTML/SVG 源码；
- 每个文档对象都有 `comments` 列表；评论 id 从 1 开始，在对象内唯一且升序；
- `Comment.text` 非空，允许换行；`resolved` 只能是 0 或 1；
- 文档片段的本地名由创建规则决定，只取决于父对象和所属集合：
  - 子章节 `section<n>`；
  - 正文元素 `body<n>`，包括 DocumentText、DocumentList、DocumentTopic、DocumentBlock、DocumentMermaid、DocumentHtml；
  - 列表项 `item<n>`；
- `n` 在同一父对象的对应集合内唯一，已有编号不重排；
- 锚点 `section`、`body`、`item` 不是对象，不写入存储，只用于 CLI 定位。

### 4.13 Children

容器的 `children` 只存子对象的 object hash：

```text
children:List<ObjectHash>
```

- 不重复存子对象的 qualified name；
- 子对象的 qualified name 在子对象自己的 `qualified_name` 字段里；
- 列表按 ObjectHash 字节升序排序；
- 同一个父对象下不能有重复的 ObjectHash；
- Document/DocumentSection/DocumentList/DocumentTopic 的 `children` 也是按 ObjectHash 排序的全集；语义顺序单独存在 `sections`、`body`、`items` 中；
- 需要显示名字时，读取子对象并解析其 `qualified_name`；
- 同一个父对象下不同子对象的 qualified name 必须唯一，这个约束在读取子对象后校验。

### 4.14 Dependency

```text
Dependency =
    target:SymbolHash
    evidence:Opt<DependencyEvidence>
    rationale:DocHash
```

```text
DependencyEvidence =
    path:SymbolHash
    start_byte:u64
    end_byte:u64
    start_line:u32
    end_line:u32
```

- `target` 是目标对象限定名的符号；
- 显式依赖可以有 evidence；
- 隐式依赖可以有 rationale；
- `rationale` 是全零时表示没有，否则指向 `docs/` 中的纯文本文档；
- `explicit_dependencies` 和 `implicit_dependencies` 分别按 canonical 字节排序；
- 每个列表内不允许重复依赖；
- 允许循环依赖；
- `superclass`、`interfaces`、`mixins`、interface 的 `extends`、mixin 的 `on` 逻辑上自动进入显式依赖，不在列表中重复存储。

## 5. 原始文本文档

`docs/` 中保存的是原始 UTF-8 文本文档，不含格式版本、类型、长度或其他元数据。它用于依赖 rationale 等自由文本。

```text
.ds/docs/aa/bb/<doc_hash>
```

文件内容就是 UTF-8 文本本身。

```text
doc_hash = SHA-256(text_bytes)
```

原始文本文档有独立的 `docs/` 索引，因此不需要域前缀。相同文本如果也作为符号存在，
会产生相同的 hash，但两者位于不同的表和不同的目录中。

- 空文本合法；
- 不存 BOM；
- 不做换行转换；
- 不存格式版本；
- 不存设计对象 hash；
- 不做 Unicode normalization；

源码对象通过 `document: SymbolHash` 引用 `documents` 下的 Document 或 DocumentSection；全零表示没有文档引用。

`docs/` 中的纯文本文档现在只用于依赖 rationale 等自由文本；源码对象的正文文档改为 `documents` 根下的结构化文档对象。

原始文本文档不进入 View 树，不进入提交链；它通过依赖 rationale 等字段间接进入版本闭包。结构化文档对象位于 `documents` 根下，正常进入 View 树和提交链。

pack 中的 DOC record type 和长度字段只是传输封装，不属于文档内容本身。

原始文本文档是版本化的：修改文本产生新的 `doc_hash`，每个 `doc_hash` 就是一个版本文档。
没有单独的文档 commit 链；旧文档由 GC 回收。版本通过引用它的设计对象选择文档。

## 6. 符号

符号表是一条逻辑上的：

```text
SymbolHash -> SymbolText
```

映射。物理上不维护单独索引文件，而是把每个符号存成一个内容寻址的纯文本文件：

```text
.ds/symbols/aa/bb/<symbol_hash>
```

文件内容就是符号文本。

```text
symbol_hash = SHA-256(text_bytes)
```

符号有独立的 `symbols/` 索引，因此不需要域前缀。

符号文本：

- 非空；
- 合法 UTF-8；
- 不含 NUL；
- 不做 Unicode normalization；
- 不存任何前缀、类型或版本。

符号表：

- 每个设计空间一份；
- 追加式；
- 不分版本；
- 不进入 View 树；
- 不进入提交链；
- 同一个文本得到同一个 hash；
- 改名产生新符号，旧符号继续存在。

对象中以下字段使用 SymbolHash：

- `qualified_name`
- `ParentRef::Object`
- `file`
- `namespace`
- `module`
- `source.path`
- `superclass`
- `extends`
- `overrides`
- `interfaces`
- `mixins`
- `on`
- `explicit_dependencies.target`
- `implicit_dependencies.target`
- `DependencyEvidence.path`

显示对象时必须能解析所有 SymbolHash；缺符号视为存储不完整。

符号同步：

- push 时把远端没有的新符号随 pack 一起推送；
- pull 时拉完整符号表，并按 hash 合并；
- 符号不可变，合并不会冲突；
- commit 前校验所有引用的符号都存在；
- 符号和文档一样由 GC 回收。

### 6.1 符号和文档的 GC

GC 根集合：

- `refs/heads/*`；
- `refs/remotes/*`；
- 从这些 refs 可达的所有 commit；
- `index.root_view`；
- `worktree.root_view`。

从每个根 View 出发，沿以下引用求可达闭包：

- children；
- sections / body / items；
- source；
- superclass；
- extends；
- overrides；
- interfaces；
- mixins；
- on；
- explicit_dependencies；
- implicit_dependencies；
- document；

可达对象引用的 DocHash 和 SymbolHash 都保留。
不可达的 object、commit、doc、symbol 由 GC 删除。
pack 是存储容器，不是 GC 根；GC/repack 时清理不可达的 pack 记录。

## 7. Commit

### 7.1 文件结构

```text
CMT1 =
    "CMT1"          4 bytes
    version:u8      1 byte
    reserved:u8[3]  3 bytes
    payload:bytes
```

```text
version = 1
```

`payload` 从 header 之后一直延伸到记录末尾。独立文件以文件末尾为界；pack 中的
记录以 pack record 的 `original_size` 为界。

### 7.2 payload

```text
CommitPayload =
    view:ObjectHash
    parent:CommitHash
    message:Text
    author:Text
    timestamp:i64
```

- `view` 必须指向一个 `kind = view` 的 DSO，且不能是全零；
- `parent` 为全零表示根提交；
- `timestamp` 是 Unix 秒，UTC；
- `message` 和 `author` 是 UTF-8 文本；
- `message` 允许为空。

### 7.3 commit hash

```text
commit_hash = SHA-256(
    version:u8
    || payload
)
```

提交有独立的 `commits/` 索引，因此不需要域前缀。

`version` 和 `payload` 参与 hash；magic 和 reserved 不参与 hash。

## 8. 暂存状态和 refs

### 8.1 HEAD

纯文本：

```text
ref: refs/heads/main
```

### 8.2 分支 ref

纯文本：

```text
<commit_hash_hex>\n
```

### 8.3 index

```text
IDX1 =
    "IDX1"
    version:u8
    reserved:u8[3]
    base_commit:CommitHash
    root_view:ObjectHash
```

- `base_commit` 是暂存区基于的提交，全零表示无；
- `root_view` 是暂存区对应的 View 根对象，全零表示无；
- 两者都为空表示空暂存区；
- index 是可变本地状态，不是内容寻址对象。

### 8.4 worktree

```text
WRK1 =
    "WRK1"
    version:u8
    reserved:u8[3]
    base_commit:CommitHash
    root_view:ObjectHash
```

- `root_view` 是当前工作区的 View 根对象，全零表示无；
- worktree 是可变本地状态。

### 8.5 source-head

纯文本，沿用现有格式：

```text
branch <name>
```

或：

```text
commit <git_hex>
```

### 8.6 暂存语义

与 Git 对齐：

- `add` 按整个对象暂存，不做字段级暂存；
- `status` 区分 staged、unstaged、untracked；
- `diff` 比较 worktree 和 index；`diff --staged` 比较 index 和 HEAD；
- `reset` 只取消暂存，不修改 worktree；
- `reset --hard` 丢弃 worktree 修改；
- `commit` 只使用 index；提交后保留未暂存修改；
- 有未提交修改时 checkout 拒绝，除非 `--force`；
- 空提交使用 `--allow-empty`。

## 9. Pack

实现状态：已实现 raw codec。`store` 读取时会回退到 pack；`gc` 先展开 pack，
`repack` 把 loose records 收进一个 pack。

沿用 `PAK1` 结构。

### 9.1 header

```text
PAK1 =
    "PAK1"
    version:u32
    reserved:u64
```

### 9.2 record

```text
PackRecord =
    record_size:u32
    compressed_size:u32
    record_type:u8
    codec:u8
    reserved:u16
    original_size:u32
    payload:bytes[compressed_size]
    padding:bytes
```

```text
record_size 包含 record 头、payload 和 padding
codec = 0 raw；1 保留给 zstd
```

当前实现只写 `codec = 0`，`compressed_size = original_size`，不写 padding。

### 9.3 record type

```text
1  DSO1 原始记录
2  DOC  纯文本设计文档
3  SYM  纯文本符号
4  CMT1 原始提交记录
```

### 9.4 index / trailer

pack 同时包含 DSO、DOC、SYM、CMT 四种记录，因此索引 entry 除了 hash 之外还保存
一个独立的 record_type 字段。hash 本身不变，也不写入类型。

```text
PKI1 header:
    "PKI1"
    version:u32
    count:u64
    entry_size:u32 = 48
    reserved:u32

PKI1 entry:
    record_type:u8
    reserved:u8[7]
    hash:32
    offset:u64
```

entry 排序规则：

1. 先按 `record_type` 升序；
2. 同一 `record_type` 内按 `hash` 升序。

- DSO 和 CMT 的 hash 是二进制记录的 hash；
- DOC 和 SYM 的 hash 是纯文本的 hash；
- 不同类型可以出现相同的 32 字节 hash，它们是索引中两条独立 entry；
- 查询时先按 `record_type` 找到对应范围，再用 `hash` 查找；
- `record_type` 只是索引字段，不参与 hash，也不拼进 hash。

例如设计文档和符号的文本都是 `hello`：

```text
doc_hash    = SHA-256("hello") = H
symbol_hash = SHA-256("hello") = H
```

两个文件分别位于：

```text
.ds/docs/<H>
.ds/symbols/<H>
```

互不冲突。

如果 pack 同时包含这两条记录，索引里就是两行：

```text
record_type   hash   offset
2             H      A       # DOC
3             H      B       # SYM
```

`record_type` 和 `hash` 是两个独立字段。实际查找时：

- 找文档：在 `record_type = 2` 的范围内查 `hash = H`；
- 找符号：在 `record_type = 3` 的范围内查 `hash = H`；
- 只按 `hash = H` 查会同时命中两行，因此查询条件必须同时包含记录类型。

这不表示 `H` 变了，也不表示存储 hash 时要带类型。`H` 始终是裸 32 字节。

```text
TRL1 trailer：pack_size、index_offset、index_size、pack_sha256

```text
TRL1 =
    "TRL1"          4 bytes
    version:u32     4 bytes
    pack_size:u64   8 bytes
    index_offset:u64 8 bytes
    index_size:u64  8 bytes
    pack_sha256:32  32 bytes
```

- `pack_size` 是整个 pack 文件的长度，包括 trailer；
- `index_offset` 是 `PKI1` 的起始位置；
- `index_size` 是 `24 + count * 48`；
- `pack_sha256` 是 trailer 之前所有字节的 SHA-256；
- pack 文件名是 `packs/<pack_sha256>.pack`。

读取时同时校验 trailer、index 边界、record 边界和每条 record 的 hash。
```

pack 解包时按 record_type 写入对应目录：

```text
1 -> objects/
2 -> docs/
3 -> symbols/
4 -> commits/
```

设计对象引用的设计文档和符号必须进入同一个传输闭包，或者由单独的符号/文档同步补齐。

## 10. hash 计算汇总

```text
object_hash =
    SHA-256(version || kind || payload)

commit_hash =
    SHA-256(version || payload)

doc_hash =
    SHA-256(text_bytes)

symbol_hash =
    SHA-256(text_bytes)
```

所有 `version`、`kind` 均按小端编码。

hash 类型由存储目录、对象字段类型和 pack 索引的 `record_type` 区分，不依赖 hash
输入前缀，也不把类型拼进 hash。混合索引只是额外保存记录类型；hash 本身始终是裸
32 字节。

32 字节全零 hash 只作为 None sentinel；它不是合法的计算 hash。任何计算得到全零
hash 的情况按内部错误处理。

## 11. 校验规则

读取或提交时必须校验：

1. 文件名 hash 与内容重新计算的 hash 一致；
2. DSO magic、version、kind 合法；
3. CMT1 的 reserved 字节必须为 0；
4. kind 与 payload 结构一致；
5. DSO/CMT payload 必须恰好消费完文件或 pack record，不能有尾部多余字节；
6. View 没有 qualified_name、parent、source、content_hash、document、dependencies；
7. Directory/File 的 source 必须是 None 或 `PathSource`；Directory 没有 content_hash；
8. Namespace/Class/Interface/Method/Function/Mixin 的 source 必须是 None 或 `SpanSource`；
9. Part 的 source 必须是 `SpanSource`，不能没有；
10. `content_hash` 是 source 片段原始字节的 SHA-256；有 source 时必须非零，非零时必须有 source；
11. Part 的 content_hash 必须非零；
12. 必填 hash 字段非全零；
13. 可选 hash 字段允许全零；全零表示 None，不做存在性检查；
14. 所有非零 SymbolHash 在 `symbols/` 中存在；
15. 所有非零 DocHash 在 `docs/` 中存在；
16. 根 directory 的 parent 必须是 View；其他 directory 的 parent 必须是 directory；
17. File 的 parent 必须恰好是一个 directory；
18. 虚拟根 namespace 的 parent 必须是 View；其他 namespace 的 parent 必须是 namespace；
18a. 根 `documents` 的 parent 必须是 View；其他 document-directory 的 parent 必须是 document-directory；
18b. Document 的 parent 必须是 document-directory；
18c. DocumentSection 的 parent 必须是 document 或 document-section；
18d. DocumentList 的 parent 必须是 document-section、document-topic 或 document-list；
18e. DocumentText 的 parent 必须是 document-section、document-topic 或 document-list；
18f. DocumentTopic 的 parent 必须是 document-section；
18g. DocumentBlock 的 parent 必须是 document-section 或 document-topic；
18h. DocumentMermaid 的 parent 必须是 document-section 或 document-topic；
18i. DocumentHtml 的 parent 必须是 document-section 或 document-topic；
19. 顶层 class/interface 的 `namespace` 必须非零；`file` 为零或 file；`module` 为零或 module；
20. 内部 class/interface 的 `parent` 必须是 class/interface；
21. Function 的 `namespace` 必须非零；`file` 为零或 file；`module` 为零或 module；
22. Method 的 `parent` 必须是 class/interface/part；
23. Part 的 `parent` 必须是 class/interface；
24. 顶层 mixin 的 `namespace` 必须非零；`file` 为零或 file；`module` 为零或 module；
25. 内部 mixin 的 `parent` 必须是 class/interface；
26. Class 的 `superclass` 为零或指向 class，且最多一个；
27. Class 的 `interfaces` 排序去重，目标必须是 interface；
28. Class 的 `mixins` 有序、去重，目标必须是 class 或 mixin；
29. Class 的 `superclass`、`interfaces`、`mixins` 不能重复指向同一对象；
30. Interface 的 `extends` 为零或指向一个 interface；
31. Mixin 的 `on` 排序去重，目标必须是 class 或 mixin；
32. Mixin 的 `superclass` 为零或指向一个 class；
33. Mixin 的 `interfaces` 排序去重，目标必须是 interface；
34. Mixin 的 `mixins` 有序、去重，目标必须是 class 或 mixin；
35. Class/Interface/Mixin 的继承、接口、mixin、extends、on 关系必须无环；
36. Method 的 `overrides` 为零或指向一个 method，最多一个；
37. `overrides` 目标必须位于相关关系闭包中，且签名匹配；
38. `explicit_dependencies` 和 `implicit_dependencies` 各自排序、去重；
39. 依赖允许循环；
40. `superclass`、`interfaces`、`mixins`、`extends`、`on` 自动视为显式依赖，不重复存储；
41. `children` 按 ObjectHash 升序排序且无重复；
42. 每个 parent 字段对应的父对象 children 必须包含该子对象；
43. 根 directory、虚拟根 namespace 和根 document directory 必须分别由 View 的 `files`、`namespaces`、`documents` 引用；
44. 父关系无环；
45. 同一个父对象下不同子对象的 qualified name 不重复；Part 显示名允许重复；
46. Part 的 qualified name 本地名是 `part#<content_hash_hex>`；
47. Method/Function 的 qualified name 含 `#<signature_hash>`；
48. View 的 `files`、`namespaces`、`documents` 非零；
49. View 的 `design` 允许为零；
50. `source_commit` 非零时，config 中必须有 `source.object_format`；
51. `source.object_format = "sha1"` 时，source_commit 后 12 字节必须为零；
52. `source.object_format = "sha256"` 时，source_commit 使用全部 32 字节；
53. `source_commit` 非零时不能是 32 字节全零；
54. Commit 的 `view` 非零且指向 kind=view 的对象；
55. Commit 的 parent 为零表示根提交；非零时必须存在且无环；
56. 不允许对象引用不存在的符号或文档；
58. Document 的 `sections`、DocumentSection 的 `body`/`sections`、DocumentTopic 的 `paragraphs`、DocumentList 的 `items` 中的引用必须存在，且目标 kind 符合对应集合；
59. 有序引用的目标必须把当前对象作为 parent，并且必须出现在当前对象的 `children` 中；
60. 有序引用和 `children` 必须覆盖同一组子对象；
61. DocumentSection 的 `body` 和 `sections` 不能引用同一对象；
62. Document/DocumentSection/DocumentList/DocumentTopic 的 `children` 必须与对象图中以它为 parent 的子对象集合一致；
63. DocumentText 的 `text` 必须是单行非空文本；Document/DocumentSection/DocumentTopic 的 `title` 必须是单行非空文本；
64. 锚点 `section`、`body`、`item` 不是对象，不能出现在存储、`children` 或有序引用中；
65. 源码对象的 `document` 为零表示没有文档引用；非零时必须指向 kind=document 或 kind=document-section 的对象；
66. 文档对象没有 `document` 字段；
67. `document` 是语义引用，不是 containment；它不要求目标出现在 `children` 中；
68. 文档对象的 `comments` 按 id 升序排列，id 从 1 开始且不重复；
69. `Comment.text` 不能为空，允许换行；
70. `Comment.resolved` 只能是 0（open）或 1（resolved）；
71. Part 没有 `document` 字段，也不能关联文档对象。

## 12. 暂缓项

以下内容暂不定义，不阻塞当前模型：

1. Design 视图根对象的 kind 和格式（当前 `View.design` 允许为零）；
2. module 的 kind 和字段（当前 class/function/mixin 保留 module 字段）；
3. 文档正文的编辑体验和模板化写作规范（当前只提供基础片段操作）。

已明确：

- View 没有 source_commit 时，对象仍允许 source anchor；
- source anchor 的 commit 只来自 View，不支持对象级 commit；
- Commit message 允许为空；
- 旧数据不兼容，不提供旧 `.ds` 数据迁移。
