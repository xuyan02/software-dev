# design-space（ds）使用技能

`ds` 是设计空间的命令行。设计空间不是代码的副本，而是对代码的**设计说明**：每个对象回答“这里为什么这样组织、承担什么职责、边界在哪里”。对象、设计文档和关系都提交到设计空间分支；源码只读。

## 1. 基本模型

- 工作区里有一个 `.ds/`。不要手改 `.ds/` 下的文件，只用 `ds` 命令。
- 设计对象是内容寻址的。对象有稳定的 qualified name（qname）、kind、源码锚点和设计文档。
- 工作流：worktree（未暂存）→ index（`ds add` 暂存）→ commit（`ds commit`）→ remote（`ds push`）。
- 两种根对象由 `ds init` 创建，不能自行创建或删除：
  - `files`：源码目录和文件的根；
  - `namespaces`：跨文件逻辑命名空间的根。
- 对象之间有两类关系：
  - 包含关系：parent / children；
  - 语义关系：superclass、interfaces、mixins、on、extends、overrides，以及显式/隐式依赖。
- 设计文档是给人和后续智能体读的说明，不是源码摘要，也不是逐行注释。

## 2. 对象类型

qname 由 parent 的 qname 加 `/` 加本地名生成。源码锚点用于把对象和源码位置对应起来。

| kind | 是什么 | qname 形式 | 源码锚点 | 常见子对象 |
|---|---|---|---|---|
| Directory | 源码目录 | `files/<目录路径>` | 目录路径 | 子目录、File |
| File | 源码文件 | `files/<文件路径>` | 文件路径 | Namespace、Class、Interface、Function、Part |
| Namespace | 跨文件/文件的逻辑命名空间，不等于物理目录 | `namespaces/<...>`，或某个 File/Directory/Namespace 下的 `<parent>/<name>` | 可选的 `path:start-end` | Namespace、Class、Interface、Function、Mixin |
| Class | 类、结构体、trait 等类型声明 | 顶层：`<namespace>/<name>`；嵌套：`<parent>/<name>` | `path:start-end` | Method、内部类型 |
| Interface | 接口、协议、抽象契约 | 同 Class | `path:start-end` | Method |
| Method | 类型内部的可调用成员 | `<parent>/<name>#<签名哈希>` | `path:start-end` | 无 |
| Function | 不属于某个类型的函数/过程 | 同 Class | `path:start-end` | 无 |
| Part | 没有独立名字、但需要说明的代码片段（配置块、宏体、特殊常量段等） | `<parent>/part#<内容哈希>` | 必须 `path:start-end` | 无 |
| Mixin | 可复用的行为片段 | 同 Class | `path:start-end` | Method |
| View | 根视图/快照，由 `ds` 维护 | 不可创建 | 无 | `files`、`namespaces` |

不要把外部库、标准库、第三方类型建成对象；只建当前仓库的设计空间内的对象。

### 2.1 Directory

表示源码目录。子对象是该目录下的子目录和 File。目录对象的设计文档是**目录规则**，不是子项清单。

写什么：

- 这个目录负责哪一层、哪一类内容；
- 什么应该放在这里，什么不应该；
- 目录内部的划分约定、命名约定、依赖方向；
- 如果目录是分层/分行/分模块的边界的，说明边界。

不要写：文件清单、某个函数的实现、逐项罗列子目录或文件。

### 2.2 File

表示一个源码文件。子对象是这个文件里声明的 Namespace、Class、Interface、Function、Part 等。File 的设计文档是**文件职责与边界**，不是成员清单。

写什么：

- 文件的主要职责和存在理由；
- 对外暴露什么、被谁使用；
- 不该放在这个文件里的内容；
- 关键的约束（例如无副作用、无 I/O、只依赖某层）；
- 变更时需要注意的影响面。

不要写：逐个成员的功能、逐行代码解释、把所有 Class/Method 再抄一遍。

### 2.3 Namespace

表示逻辑上的命名空间/模块，用来组织跨文件的概念；物理目录由 Directory 表示，两者不必一一对应。

写什么：

- 这个概念边界包含什么，不包含什么；
- 允许依赖哪些层/模块，禁止依赖哪些；
- 与相邻命名空间的分工；
- 对外稳定性和兼容性要求。

顶层命名空间通常挂在 `namespaces/` 下；只属于单个文件的命名空间可以挂在对应 File 下。

### 2.4 Class

表示类、结构体、trait、enum 等类型声明。

写什么：

- 类型承担的责任和核心抽象；
- 关键不变量（构造后哪些条件必须成立）；
- 生命周期、所有权、可变性、线程/并发约束；
- 与哪些对象协作，通过什么契约协作；
- 扩展点、继承关系、需要子类/实现者遵守的规则；
- 明确它**不是**什么，避免被误用。

不要写：每个方法的逐行实现、重复接口签名。

### 2.5 Interface

表示接口、协议或抽象契约。

写什么：

- 契约的语义：实现者必须保证什么，调用者可以假设什么；
- 成功、失败、异常和边界条件；
- 实现者集合或典型实现；
- 兼容性、版本化和弃用约束；
- 与 Class 的关系（`extends`）。

### 2.6 Method / Function

表示可调用成员。Method 必须给出 `--signature`，qname 里的签名哈希由 `ds` 生成，不要手写。

写什么：

- 行为：做什么，返回什么；
- 输入、输出、错误/异常；
- 前置条件、后置条件、不变量；
- 副作用（I/O、状态修改、并发、资源）；
- 性能和复杂度上必须满足的约束；
- 与调用方/被调用方的重要协作。

不要写：逐行解释实现、复制函数体。实现细节只在影响契约时写。

### 2.7 Part

表示没有独立名字、但设计上需要说明的代码片段，例如宏体、配置块、内嵌 DSL 段、特殊常量表。必须用 `--source path:start-end` 指定行范围。

写什么：

- 这个片段属于哪个结构、为什么单独存在；
- 它保证了什么、依赖什么格式/协议；
- 修改时哪些约束不能破坏。

### 2.8 Mixin

表示可复用、可组合的行为片段。

写什么：

- 它给使用者增加什么行为；
- 使用前提和假设；
- 与 superclass、interfaces、其他 mixin 的冲突/覆盖规则（`--on`）；
- 不应被用在什么场景。

## 3. qname、parent 和 source

### 3.1 qname

- Directory / File：`files/<源码路径>`，例如 `files/src/main.rs`、`files/src/app`；
- 顶层 Namespace：`namespaces/<name>[/<name>...]`；
- 顶层 Class/Interface/Function/Mixin：`<namespace qname>/<name>`；
- 嵌套类型：`<parent qname>/<name>`；
- Method：`<parent qname>/<name>#<signature hash>`；
- Part：`<parent qname>/part#<content hash>`。

Method 和 Part 的 qname 后缀由 `ds` 计算，创建后用 `ds ls` 或 `ds show` 拿完整 qname。

### 3.2 parent 选择

- Directory / File / Namespace：`--parent <qname>`，必须是已存在的 Directory、File 或 Namespace；
- 顶层 Class / Interface / Function / Mixin：`--namespace <namespace qname>`（必填），可选 `--file <file qname>` 和 `--module <module qname>` 记录文件/模块归属；
- 嵌套 Class / Interface / Mixin：`--parent <parent qname>`；
- Method：`--parent <class/interface/mixin qname>`，并给 `--signature`；
- Part：`--parent <qname>`，并给 `--source path:start-end`。

### 3.3 source

```text
--source path              整个文件（File / Directory）
--source path:start-end    1-based 行范围，包含 start 和 end
```

- Directory / File 只接受路径；
- Namespace / Class / Interface / Method / Function / Mixin 可用路径或行范围；
- Part 必须使用行范围；
- 有源码时内容哈希由 `ds` 计算，不要自己写；
- 不要给源码不存在的路径建锚点；先 `ds source cat` 确认。

## 4. 常用命令

### 4.1 读取源码

```bash
ds source fetch
ds source checkout <branch>
ds source status
ds source ls [<path>]
ds source cat <path>
ds source log [<ref>]
ds source changes [--base <ref>]
```

- `ds source fetch` 拉取源码分支；`checkout` 切换当前源码 ref；
- `ds source ls` 列出当前源码提交中的文件路径；
- `ds source cat` 读取源码内容；
- `ds source changes` 比较“最近一次设计提交记录的源码 commit”和当前源码 HEAD，输出 A/M/D/R；
- `ds source sync` 一次把 `files/` 下的 File/Directory 对象和当前源码树对齐（新增、删除、重命名），通常由上层准备阶段调用，普通对象编辑不需要手动跑。

### 4.2 查看对象

```bash
ds show <qname> [--source] [--staged]
ds ls [--files] [--namespaces] [--design] [--staged]
```

- `ds show` 显示 kind、parent、source、content hash、设计文档，以及 `children:`；
- `ds show --source` 同时输出锚定的源码；
- `ds show --staged` 读 index；
- `ds ls --files` 列 Directory/File 的 qname；
- `ds ls --namespaces` 列非 File/Directory 的 qname；
- `ds ls --design` 列已有设计文档的 qname 和文档 hash。

### 4.3 创建对象

```bash
ds create directory <name> --parent <qname> --source <path> [--design <text>]
ds create file <name> --parent <qname> --source <path> [--design <text>]
ds create namespace <name> --parent <qname> [--source path:start-end] [--design <text>]
ds create class <name> --namespace <ns> [--file <file>] [--source path:start-end] \
  [--superclass <qname>] [--interface <qname>]... [--mixin <qname>]... [--design <text>]
ds create interface <name> --namespace <ns> [--extends <qname>] [--source path:start-end]
ds create function <name> --namespace <ns> [--file <file>] [--source path:start-end]
ds create method <name> --parent <type qname> --signature 'foo(x)' [--source path:start-end] \
  [--overrides <method qname>]
ds create part <name> --parent <qname> --source path:start-end [--design <text>]
ds create mixin <name> --namespace <ns> [--on <qname>]... [--superclass <qname>] \
  [--interface <qname>]... [--mixin <qname>]...
```

- `part` 的 `<name>` 只是 CLI 的占位参数；qname 由 `<parent>/part#<内容哈希>` 决定，不是本地名；
- 长文档用 `--design-file <path>`，避免 shell 转义问题；
- `--interface`、`--mixin`、`--on` 可以重复；
- 关系目标可以暂时不存在，最后用 `ds verify` 解析。

### 4.4 编辑对象

```bash
ds edit <qname> [--design <text> | --design-file <path> | --clear-design]
ds edit <qname> --source path:start-end
ds edit <qname> --name <new local name>
ds edit <qname> --parent <new parent>
ds edit <qname> --interface-add <qname> --interface-rm <qname>
ds edit <qname> --mixin-add <qname> --mixin-rm <qname>
ds edit <qname> --on-add <qname> --on-rm <qname>
ds edit <qname> --superclass <qname> | --extends <qname> | --overrides <qname>
```

- 改 parent、namespace、file、module 或本地名时，`ds` 会一起迁移子孙的 qname、关系和依赖；
- 改名/移动后，用 `ds show` 确认新 qname；
- 不要用 `ds edit` 删对象；删对象用 `ds rm`。

### 4.5 删除对象

```bash
ds rm <qname> [--force]
```

- 有 children 时先删 children；
- `--force` 只清理指向它的外部关系/依赖，不会级联删 children；
- 源码树已删除的文件/目录优先用 `ds source sync` 对齐，不要手工一个个删。

### 4.6 关系与依赖

```bash
ds dep add <src> <dst> --explicit [--evidence path:start-end]
ds dep add <src> <dst> --implicit --rationale-file <path>
ds dep rm <src> <dst> --explicit|--implicit
ds dep list <qname>
```

- 包含关系（parent/children）自动计入结构依赖，不用 `dep add`；
- `--explicit` 表示源码中可直接看到的依赖，给证据行；
- `--implicit` 表示运行时/设计上的依赖，给理由文件；
- 关系目标暂时不存在也可以，`ds verify` 会检查。

### 4.7 暂存、审查、提交

```bash
ds status                 # HEAD/index/worktree 以及未暂存/已暂存变化
ds diff                   # worktree 和 index 的差异
ds diff --staged          # index 和 HEAD 的差异
ds add -A                 # 暂存全部 worktree 对象
ds verify                 # 校验 qname、parent、source、关系和依赖
ds commit -m "..." --author <name>
ds push --branch <branch>
```

流程：

1. 创建/编辑/删除对象，写设计文档和关系；
2. `ds status` / `ds diff` 查看未暂存改动；
3. `ds add -A`；
4. `ds diff --staged` 和 `ds ls --staged` 复核；
5. `ds verify`；
6. `ds commit`，再 `ds push`。

不要跳过 review 和 verify 直接提交。

## 5. 设计文档的写法

### 5.1 通用规则

- 用纯文本，简洁、可判断；不要空话、TODO、占位符或“待补充”；
- 写“是什么、为什么、边界、约束、契约、什么不属于这里”，不要逐行复述源码；
- 不写实现细节，除非它会改变其他对象的契约；
- 不写密钥、内部 URL、个人数据；
- 文档语言与项目文档保持一致（本项目以中文为主）；
- File/Directory 的文档不要枚举子对象；子对象的细节放在子对象自己的文档里；
- 一个对象没有设计可写时，不要创建空文档；但一旦创建了对象，就写清它的职责和边界。

### 5.2 按类型的检查清单

- **Directory**：职责分层；放置规则；禁止内容；命名/依赖约定；边界。
- **File**：文件职责；对外暴露；不该出现的内容；依赖方向；变更影响。
- **Namespace**：语义边界；包含/不包含；允许/禁止的依赖；与相邻模块分工。
- **Class**：核心抽象；不变量；生命周期/所有权/并发；协作对象；扩展点；明确不是什么。
- **Interface**：实现者保证；调用者可假设；错误与边界；兼容性；实现者集合。
- **Method/Function**：行为；输入输出；错误；前置/后置条件；副作用；性能/并发约束。
- **Part**：所属结构；存在理由；格式/协议约束；修改禁忌。
- **Mixin**：增加的行为；使用前提；冲突/覆盖规则；适用和不适用场景。

### 5.3 关系怎么写

- 只写当前仓库内的对象；外部库/标准库类型不建模；
- superclass/interfaces/mixins 表示语义关系，不是源码文本的复制；
- `overrides` 指向被覆盖的 Method；
- 依赖分 explicit（源码可见）和 implicit（运行时/设计约束），并给证据或理由；
- 关系目标可以是稍后创建的对象，最终必须通过 `ds verify`。

## 6. 常见错误

- 用源码路径直接当 qname（File 需要 `files/<path>`）；
- 创建或删除 `files` / `namespaces` 根对象；
- 给 Method 手写 `#hash`，或漏掉 `--signature`；
- 给 Part 用没有行范围的 `--source`；
- 目录/文件文档写成子项清单；
- 关系目标写成外部库类型；
- 未 `ds verify` 就 commit；
- 直接用 `ds rm` 删除源码已不存在的文件树，而不是 `ds source sync`；
- 改 qname 后没有重新 `ds show` 确认，仍用旧名字继续操作。

## 7. 速查

```bash
# 看当前状态
ds status
ds source changes
ds ls --files
ds ls --namespaces

# 看一个对象
ds show files/src/main.rs
ds show namespaces/data_core/Value

# 建文件/目录/类型
ds create directory app --parent files/src --source src/app
ds create file main.rs --parent files/src --source src/main.rs
ds create namespace data_core --parent namespaces --source src/data_core.rs:1-80
ds create class Value --namespace namespaces/data_core --file files/src/data_core.rs \
  --source src/data_core.rs:10-120 --design-file /tmp/value.md
ds create method get --parent namespaces/data_core/Value --signature 'get(path)' \
  --source src/data_core.rs:40-70

# 写文档
ds edit files/src/main.rs --design-file /tmp/main.md
ds edit namespaces/data_core/Value --design-file /tmp/value.md

# 关系
ds dep add namespaces/data_core/Value namespaces/data_core/Path --explicit \
  --evidence src/data_core.rs:40-50
ds verify

# 暂存与提交
ds add -A
ds diff --staged
ds commit -m "summarize data-core value"
ds push --branch master
```
