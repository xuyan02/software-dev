# design-space（ds）使用技能

`ds` 是设计空间的命令行。设计空间保存源码对象、文档对象和关系；源码只读。

## 1. 基本模型

- 工作区里有一个 `.ds/`。不要手改 `.ds/` 下的文件，只用 `ds` 命令。
- 设计空间不是 git 仓库。`ssh://.../<project-id>` 只接受 `ds` 协议，不能用
  `git clone`、`git fetch`、`git push` 访问；服务端会拒绝 `git-upload-pack`。
  设计空间只用 `ds clone` / `ds fetch` / `ds push`，凭证由 `ds` 自己读取。
- 设计对象是内容寻址的。对象有稳定的 qualified name（qname）、kind、源码锚点和 document 引用。
- 工作流：worktree（未暂存）→ index（`ds add` 暂存）→ commit（`ds commit`）→ remote（`ds push`）。
- 三种根对象由 `ds init` 创建，不能自行创建或删除：
  - `files`：源码目录和文件的根；
  - `namespaces`：跨文件逻辑命名空间的根；
  - `documents`：文档对象的根。
- 对象之间有两类关系：
  - 包含关系：parent / children；
  - 语义关系：superclass、interfaces、mixins、on、extends、overrides，以及显式/隐式依赖。

## 2. 对象类型

qname 由 parent 的 qname 加 `/` 加本地名生成。源码锚点用于把对象和源码位置对应起来。

| kind | 是什么 | qname 形式 | 源码锚点 | 常见子对象 |
|---|---|---|---|---|
| Directory | 源码目录 | `files/<目录路径>` | 目录路径 | 子目录、File |
| File | 源码文件 | `files/<文件路径>` | 文件路径 | 通常无；文件中的类型挂在 Namespace 下，通过 `--file` 关联 |
| Namespace | 逻辑命名空间，不等于物理目录 | `namespaces/<...>`（父是 `namespaces` 或另一个 Namespace） | 可选的 `path:start-end` | Namespace、Class、Struct、Enum、Interface、Function、Mixin |
| Class | 同时有数据和方法，或变体携带数据的顶层类型 | `<namespace>/<name>` | `path:start-end` | InnerClass、InnerStruct、InnerEnum、Method |
| InnerClass | 嵌套在另一个类型里的 Class | `<parent type>/<name>` | `path:start-end` | InnerClass、InnerStruct、InnerEnum、Method |
| Struct | 只有数据、没有方法，且不是变体携带数据的 enum 的顶层类型 | `<namespace>/<name>` | `path:start-end` | InnerStruct、InnerEnum |
| InnerStruct | 嵌套在另一个类型里的 Struct | `<parent type>/<name>` | `path:start-end` | InnerStruct、InnerEnum |
| Enum | 只有常量变体、没有附加数据和方法的顶层类型 | `<namespace>/<name>` | `path:start-end` | InnerEnum |
| InnerEnum | 嵌套在另一个类型里的 Enum | `<parent type>/<name>` | `path:start-end` | 无 |
| Interface | 顶层接口、协议、抽象契约 | `<namespace>/<name>` | `path:start-end` | InnerInterface、Method |
| InnerInterface | 嵌套在类型里的接口/契约 | `<parent type>/<name>` | `path:start-end` | Method |
| Method | 类型内部的可调用成员 | `<parent>/<name>#<签名哈希>` | `path:start-end` | 无 |
| Function | 不属于某个类型的函数/过程 | 同 Class | `path:start-end` | 无 |
| Part | 没有独立名字、但需要说明的代码片段（配置块、宏体、特殊常量段等） | `<parent>/part#<内容哈希>` | 必须 `path:start-end` | 无 |
| Mixin | 可复用的行为片段 | 同 Class | `path:start-end` | Method |
| View | 根视图/快照，由 `ds` 维护 | 不可创建 | 无 | `files`、`namespaces`、`documents` |

不要把外部库、标准库、第三方类型建成对象；只建当前仓库的设计空间内的对象。

### 2.1 Directory

表示源码目录。子对象是该目录下的子目录和 File。

### 2.2 File

表示一个源码文件。File 通常不作为类型的 parent；文件中声明的顶层 Class/Struct/Enum/Interface/Function/Mixin 挂在 Namespace 下，并通过 `--file files/<路径>` 关联到这个文件。

### 2.3 Namespace

表示逻辑上的命名空间/模块，用来组织跨文件的概念；物理目录由 Directory 表示，两者不必一一对应。

命名空间统一挂在 `namespaces/` 下；即使只被一个文件使用，也用独立 Namespace，不要把 File 当成 namespace，避免出现 `files/<path>/Class` 这种带文件前缀的类型 qname。

### 2.4 Class / InnerClass

`Class` 是顶层类型，挂在 Namespace 下，可选 `--file`/`--module` 关联源码位置。
`InnerClass` 是嵌套类型，`--parent` 必须是另一个类型，qname 为 `<parent>/<name>`。

表示类、结构体、trait、enum 等类型声明。

### 2.5 Interface / InnerInterface

`Interface` 是顶层接口；`InnerInterface` 是嵌套接口，`--parent` 必须是类型。
qname 分别走 namespace 或 parent 类型，不使用 File 前缀。

表示接口、协议或抽象契约。

### 2.6 Method / Function

表示可调用成员。Method 必须给出 `--signature`，qname 里的签名哈希由 `ds` 生成，不要手写。

### 2.7 Part

表示没有独立名字、但设计上需要说明的代码片段，例如宏体、配置块、内嵌 DSL 段、特殊常量表。必须用 `--source path:start-end` 指定行范围。Part 不关联设计文档，不接受 `--document`。

### 2.8 Mixin

表示可复用、可组合的行为片段。

### 2.9 文档对象

`documents` 下的对象是文档片段，和源码对象共用 View、index、worktree、提交和评论系统。

| kind | 是什么 | qname 形式 |
|---|---|---|
| document-directory | 文档目录 | `<parent>/<name>` |
| document | 文档 | `<parent>/<name>` |
| document-section | 章节 | `<parent>/section<n>` |
| document-topic | 带标题的内容块 | `<parent>/body<n>` |
| document-list | 列表 | `<parent>/body<n>` 或 `<parent>/item<n>` |
| document-text | 文本 | `<parent>/body<n>` 或 `<parent>/item<n>` |
| document-block | 围栏代码块 | `<parent>/body<n>` |
| document-mermaid | Mermaid 图表 | `<parent>/body<n>` |
| document-html | HTML/SVG 预览 | `<parent>/body<n>` |

- `document-section` 的正文可以放 `document-text`、`document-list`、`document-topic`、`document-block`、`document-mermaid`、`document-html`；子章节单独放在 `sections`。
- `document-topic` 只属于 `document-section`；它有一个标题和段落列表，段落只能是 `document-text`、`document-list`、`document-block`、`document-mermaid` 或 `document-html`。
- `document-block` 是 Markdown 围栏代码块，只能放进 Section 正文或 Topic 段落，不能作为列表项；有 `language`（可为空）和多行 `text`。
- `document-mermaid` 是 Mermaid 图表（流程图、状态图、时序图、架构/拓扑图），只存源码 `text`，只能放进 Section 正文或 Topic 段落，不能作为列表项。
- `document-html` 是 HTML/SVG 预览，存原始源码 `text`，渲染在无脚本沙箱 iframe 中；只能放进 Section 正文或 Topic 段落。
- 自动名只取决于父对象和集合：子章节 `section<n>`，正文元素 `body<n>`，列表项 `item<n>`。不要手写这些名字。
- 正文元素插入到 Section 或 Topic 的正文：目标用 `<section-qname>/body`、`<topic-qname>/body` 头锚点，或已有正文元素 qname。
- 读取文档用 `ds doc show <qname> --format json|md`；`json` 输出包含每个 Section、正文元素和列表项的 qname，可直接定位后续修改，也可回灌 `ds doc import`；`md` 是更短的等价视图，不含 qname。通用对象用 `ds show <name> --format json|md`。
- 批量创建整棵文档子树用 `ds doc import --file plan.json`：一次进程完成全部 Section、正文和对象关联，按 `title` 复用已有 Section，可重复执行。计划形如：

  ```json
  {
    "root": "documents/design/directory",
    "sections": [
      {
        "title": "src",
        "document": ["files/src"],
        "body": [{"type": "text", "text": "置放源码。"}],
        "sections": []
      }
    ]
  }
  ```

  不要对大量对象循环调用 `ds doc section add` / `ds doc text add`，那会产生每个对象两三条进程。

### 2.10 批量操作

原则：一次进程完成尽量多的步骤，不要按对象循环起 `ds`。每条命令都会全量加载并重建工作区。

- **创建/更新文档树**：`ds doc import --file plan.json`。一次进程建完整棵子树，按 `title` 复用已有 Section，重复执行幂等；计划格式见上面的 `root` / `sections` / `body`。
- **导出再回灌**：`ds doc show <qname> --format json > plan.json`，然后 `ds doc import --file plan.json`。二次导入输出 `Imported 0 section(s)` 表示没有变化。
- **读取**：`ds doc show <qname> --format json|md`；通用对象用 `ds show <name> --format json|md`。不要为每个对象单独起一次 `ds show`，能用一次 `doc show` 拿到整棵树就用一次。
- **删除子树**：`ds rm <qname> --recursive --force`。`--force` 会清掉 `files/...` 指向该树的 `document:` 引用；删除只改 worktree，之后还需要 `ds add -A` 和 `ds commit`。
- **单对象修改**：用 `ds edit`；不要每改一个字段就 `add`/`commit`，把同一批改动集中到一次 `ds add -A` + `ds commit`。
- **批量操作 JSON 约定**（`upsert` / `delete` / `show` 的 envelope）见 design-space 仓库的 `docs/document-io.md`；`ds batch` 执行器尚未实现，当前用上述命令组合。

文档内容写什么、格式是什么，见 `design-writing` 技能。

## 3. qname、parent 和 source

### 3.1 qname

- Directory / File：`files/<源码路径>`，例如 `files/src/main.rs`、`files/src/app`；
- 顶层 Namespace：`namespaces/<name>[/<name>...]`；
- 顶层 Class/Struct/Enum/Interface/Function/Mixin：`<namespace qname>/<name>`；
- 嵌套类型：`<parent qname>/<name>`；
- Method：`<parent qname>/<name>#<signature hash>`；
- Part：`<parent qname>/part#<content hash>`。

Method 和 Part 的 qname 后缀由 `ds` 计算，创建后用 `ds ls` 或 `ds show` 拿完整 qname。

### 3.2 parent 选择

- Directory / File：`--parent <Directory qname>`（或 `files`）；
- Namespace：`--parent namespaces` 或另一个 Namespace，不要把 File/Directory 当成 namespace；
- 顶层 Class / Struct / Enum / Interface / Function / Mixin：`--namespace <namespace qname>`（必填），可选 `--file <file qname>` 和 `--module <module qname>` 记录文件/模块归属；
- 嵌套 Class/Struct/Enum/Interface 用 `inner-class`/`inner-struct`/`inner-enum`/`inner-interface`，并给 `--parent <type qname>`；parent 不是 File；
- Mixin 仍按 `--namespace`（顶层）或 `--parent`（嵌套）选择；
- Method：`--parent <class/interface/mixin/part qname>`，并给 `--signature`；
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
ds source worktree
ds source status
ds source ls [<path>]
ds source cat <path>
ds source log [<ref>]
ds source changes [--base <ref>]
```

- `ds source fetch` 拉取源码分支，并在工作区根目录创建/复用 Git worktree；`checkout` 切换当前源码 ref 和 worktree；
- 工作区根目录就是源码 Git worktree：源码文件直接出现在这里，可以直接运行 `git status`、`git diff`、`git commit`；
- `.ds/` 是设计空间元数据，已被源码 Git 排除；不要把 `.ds/` 纳入源码提交；
- `ds source ls` 列出当前源码提交中的文件路径；
- `ds source cat` 读取源码内容；
- `ds source changes` 比较“最近一次设计提交记录的源码 commit”和当前源码 HEAD，输出 A/M/D/R；
- `ds source sync` 一次把 `files/` 下的 File/Directory 对象和当前源码树对齐（新增、删除、重命名），通常由上层准备阶段调用，普通对象编辑不需要手动跑。

### 4.2 查看对象

```bash
ds show <qname> [--source] [--staged]
ds ls [--files] [--namespaces] [--design] [--staged]
ds search [QUERY] [--kind <kind>] [--namespace <qname>] [--file <qname>] [--module <qname>] [--document <qname>] [--has-design] [--format text|json]
ds status --porcelain
ds doc grep <pattern> [--format text|json]
ds doc lint [--rules parameter,return] [--format text|json]
ds doc comment list [--open] [--format text|json]
```

`ds status --porcelain` 输出未暂存对象：状态和 qname，每行一个；需要 kind/source 时用 `--detail`，否则用 `ds show <qname>` 查询。

- `ds show` 显示 kind、parent、source、content hash、`document:` 引用，以及 `children:`；
- `ds show --source` 同时输出锚定的源码；
- `ds show --staged` 读 index；
- `ds ls --files` 列 Directory/File 的 qname；
- `ds ls --namespaces` 列非 File/Directory 的 qname；
- `ds ls --design` 列已有 `document` 引用的对象，输出 qname 和 document qname；
- `ds search` 按 kind、namespace、file、module、document 和 qname/源码路径查询，不要循环 `ds show` 再 grep kind；`--format json` 返回 qname、kind、source、document、file、module；
- `ds doc grep` 在文档正文中搜索并返回 qname，不要导出 Markdown 后用 grep 找行号；
- `ds doc lint` 检查参数/返回值等字段格式，返回 qname、规则和违规文本；
- `ds doc comment list --open --format json` 返回评论、目标 qname 和当前正文。

### 4.3 创建对象

```bash
ds create directory <name> --parent <qname> --source <path> [--document <qname>]
ds create file <name> --parent <qname> --source <path> [--document <qname>]
ds create namespace <name> --parent <qname> [--source path:start-end] [--document <qname>]
ds create class <name> --namespace <ns> [--file <file>] [--source path:start-end] \
  [--superclass <qname>] [--interface <qname>]... [--mixin <qname>]... [--document <qname>]
ds create inner-class <name> --parent <type qname> [--source path:start-end] \
  [--superclass <qname>] [--interface <qname>]... [--mixin <qname>]...
ds create struct <name> --namespace <ns> [--file <file>] [--source path:start-end] [--document <qname>]
ds create inner-struct <name> --parent <type qname> [--source path:start-end] [--document <qname>]
ds create enum <name> --namespace <ns> [--file <file>] [--source path:start-end] [--document <qname>]
ds create inner-enum <name> --parent <type qname> [--source path:start-end] [--document <qname>]
ds create interface <name> --namespace <ns> [--extends <qname>] [--source path:start-end]
ds create inner-interface <name> --parent <type qname> [--extends <qname>]
ds create function <name> --namespace <ns> [--file <file>] [--source path:start-end]
ds create method <name> --parent <type qname> --signature 'foo(x)' [--source path:start-end] \
  [--overrides <method qname>]
ds create part <name> --parent <qname> --source path:start-end
ds create mixin <name> --namespace <ns> [--on <qname>]... [--superclass <qname>] \
  [--interface <qname>]... [--mixin <qname>]...
```

- `part` 的 `<name>` 只是 CLI 的占位参数；qname 由 `<parent>/part#<内容哈希>` 决定，不是本地名；
- 文档正文用 `ds doc text add <section-or-topic>/body --text ...` 写入，围栏代码块用 `ds doc block add ... --language ... --text ...` 写入，流程图用 `ds doc mermaid add ... --text ...` 写入，HTML/SVG 预览用 `ds doc html add ... --text ...` 写入；代码对象用 `ds edit <qname> --document <section-qname>` 关联；System/Module/成员的组织规则见 `design-writing` 技能。
- `--interface`、`--mixin`、`--on` 可以重复；
- 关系目标可以暂时不存在，最后用 `ds verify` 解析。

### 4.4 编辑对象

```bash
ds edit <qname> [--document <qname> | --clear-document]
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

### 4.4.1 文档布局与文档操作

技术设计的标准文档布局：

```text
documents/design/technical     # 统一的技术设计文档
documents/design/directory     # 目录设计
documents/design/namespace     # 命名空间设计
```

不要创建 `documents/specs`；它只是 CLI 示例里的临时名字，不是本项目的设计文档布局。

```bash
ds doc create documents/design/technical --title "技术设计"
ds doc section add documents/design/technical/section --title "登录"
ds doc topic add documents/design/technical/section1/body --title "函数原型"
ds doc text add documents/design/technical/section1/body1/body --text "login(&mut self) -> Result<(), Error>"
ds doc topic add documents/design/technical/section1/body --title "认证流程"
ds doc text add documents/design/technical/section1/body --text "登录使用 OAuth 2.0。"
ds doc block add documents/design/technical/section1/body1 --language rust --text $'let token = login();'
ds doc mermaid add documents/design/technical/section1/body2 --text $'flowchart TD\n  A[请求] --> B[校验]'
ds doc html add documents/design/technical/section1/body3 --text '<div style="padding:12px">登录页效果图</div>'
ds doc list add documents/design/technical/section1/body4
ds doc list item add documents/design/technical/section1/body5/item --text "第一项"
ds doc text set documents/design/technical/section1/body1 --text "新的文本"
ds doc block set documents/design/technical/section1/body2 --language text --text "changed"
ds doc mermaid set documents/design/technical/section1/body3 --text $'flowchart LR\n  A --> B'
ds doc html set documents/design/technical/section1/body4 --text '<div>新的效果图</div>'
ds doc show documents/design/technical
```

- `section add` 的目标是 `<document-or-section>/section` 头锚点，或已有兄弟 section 的 qname；
- `topic add` 的目标是 `<section>/body` 头锚点，或已有正文元素 qname；
- `text add` / `block add` / `mermaid add` / `html add` / `list add` 的目标是 `<section-or-topic>/body` 头锚点，或已有正文元素 qname；
- `list item add` 的目标是 `<list>/item` 头锚点，或已有列表项 qname；
- 所有 `add` 命令只接收一个目标位置，不传父对象，也没有 `--after`。

### 4.4.2 文档评论

每个文档对象（document-directory、document、document-section、document-list、document-text、document-topic、document-block、document-mermaid、document-html）都有独立评论列表。评论状态只有 `open` 和 `resolved`。

```bash
ds doc comment add <doc-qname> --text "需要补充错误码"
ds doc comment list <doc-qname>          # 单个对象
ds doc comment list --open               # 全仓库 open 评论
ds doc comment list <doc-qname> --open   # 单个对象只看 open
ds doc comment list <doc-qname> --open --subtree   # 对象及其整个子树
ds doc comment resolve <doc-qname> comment1
```

- 评论 id 是对象内的 `comment1`、`comment2`；
- 评论不是正文，不要混进文档文字；
- 问题未解决时不要 resolve；
- `ds doc show` 和 `ds show` 都会显示评论状态。
- 需要全仓库 open 评论时用 `ds doc comment list --open`，一次扫描完成；不要对每个文档对象逐个调用。全仓库输出每行第一列是对象 qname，格式为 `<qname>\tcomment<id>\t<status>\t<text>`。
- 需要某个对象及其子树的评论时用 `--subtree`；输出同样带 qname 前缀。不带 `--subtree` 时只查该对象自身，输出保持 `comment<id>\t<status>\t<text>`。
- CodeSummary review 用评论承载意见，JSON 只传 `passed` 和 `sections`；总结节点读取 open 评论，修复后 resolve。

### 4.4.3 概念空间

专业词汇用概念对象定义，组织在 `concepts` 目录树中。目录结构由智能体维护。

```bash
ds concept mkdir concepts/agent
ds concept create concepts/agent/agent --name "智能体" \
  --summary "能自主执行任务的主体。" \
  --explanation "智能体由身份、工具和运行状态组成。"
ds concept feature add concepts/agent/agent "有独立身份。"
ds concept relation add concepts/agent/agent "智能体执行任务。"
ds concept alias add concepts/agent/agent "Agent"
ds concept set concepts/agent/agent --summary "新的定义。"
ds concept show concepts/agent/agent
ds concept ls concepts/agent
ds concept mv concepts/agent/agent concepts --name agent
ds concept rm concepts/agent/agent
ds concept check
```

- 概念 qname 必须在 `concepts/` 下；`ds init` 和 ds-server 初始化会创建 `concepts` 根，旧工作区首次使用时补建。
- 概念字段：`name`、`summary`、`explanation`、`characteristics`、`relations`、`aliases`。
- `feature` / `relation` / `alias` 的 `rm` 使用 1-based 序号。
- `concept mv` 只能移动 Concept 或空目录；非空目录先移动子项。
- 文档中引用概念使用文本格式 `[[concept:<qname>|<label>]]`，Web 渲染为链接；`concept check` 检查引用能否解析到 Concept 对象。
- 概念的目录结构由智能体维护，不要手工改 `.ds`。
- 概念的更新、合并、拆分、移动、删除和目录维护规则见 `design-writing` 的“概念维护”。

### 4.5 删除对象

```bash
ds rm <qname> [--force] [--recursive]
```

- 有 children 时先删 children；`--recursive` 按子树后序一次删除对象和全部 containment 后代；
- `--force` 只清理指向它的外部关系/依赖，不会级联删 children；递归删除时子树内部的引用不算外部引用；
- 源码树已删除的文件/目录优先用 `ds source sync` 对齐；需要手工删整棵目录树时用 `ds rm --recursive`，不要写脚本逐个删。

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
ds add --file <path>      # 只暂存某个源文件关联的对象
ds add --directory <dir>  # 只暂存某个目录的 Directory 对象
ds unstage --file <path>  # 把该文件关联对象退回 worktree
ds unstage --directory <dir>
ds verify                 # 校验 qname、parent、source、关系和依赖
ds commit -m "..." --author <name>
ds push --branch <branch>
```

流程：

1. 创建/编辑/删除对象，关联文档和关系；
2. `ds status` / `ds diff` 查看未暂存改动；
3. `ds add -A`；
4. `ds diff --staged` 和 `ds ls --staged` 复核；
5. `ds verify`；
6. `ds commit`，再 `ds push`。

不要跳过 review 和 verify 直接提交。

## 5. 系统、模块与层

逻辑结构是独立于文件/命名空间树的一棵树：

- 根 qname 固定为 `design`；
- 系统 qname 为 `design/<system>`；
- 模块 qname 为 `<system-or-module-or-layer>/<module>`；
- 层 qname 为 `<system-or-module>/<layer>`，不能嵌套层；
- 容器（System/Module/Layer）的成员只引用顶层 Class/Struct/Enum/Interface/Function/Mixin；一个元素恰好属于一个容器；
- 没有层的系统可以同时有 Module 和直属成员，不要求二选一；
- 子模块通过 parent 关系嵌套，不要写进 members；
- 层是有序的：同级层的 `order` 从 1 开始、互不相同，只有相邻层之间允许依赖；
- 一个 System/Module 一旦有层，它的直属成员和子模块都必须放进层里，不能直接挂在它下面。

```bash
ds structure init
ds structure system create app --title '应用' --document <doc-section>
ds structure layer create api --parent design/app --title '接口层' --document <doc-section>
ds structure layer create core --parent design/app --title '核心层' --document <doc-section>
ds structure module create handlers --parent design/app/api --title '处理器' --document <doc-section>
ds structure assign design/app/api namespaces/app/EntryFunction
ds structure assign design/app/api/handlers namespaces/app/Auth namespaces/app/Session
ds structure unassign design/app/api/handlers namespaces/app/Session
ds structure list
ds structure show design/app
ds structure show design/app/api
ds structure show design/app/api/handlers
ds structure unassigned
ds structure check
```

- `ds structure layer create` 不传 `--order` 时追加到同级层末尾；用 `ds structure layer set` 调整标题、顺序或文档；`ds structure layer rm` 只能删除空层。
- `ds structure show` 对层显示 `order`。

`ds structure check` 会检查成员存在、唯一归属、空模块、空系统、空层、层顺序、层嵌套、有层容器的直属成员/子模块、相邻层依赖、所有顶层元素已归属；它和 `ds verify` 一样是提交前必须通过的检查。系统没有模块但有直属成员是合法的。

System/Module/层的定义和文档内容见 `design-writing` 技能；结构、归属和复杂度规则见本文档。

## 6. 文档写作

每种对象的文档写什么、格式是什么，由 `design-writing` 技能规定。本技能只负责 ds 对象模型、qname、命令、文档位置和 staging。

技术设计文档的 Section 层级必须与 `design` 结构树一致。用代码保证，不靠人工：

```bash
ds doc layout   # 按结构树创建/移动 Section
ds doc check    # 校验 Section 层级
```

Function/Method 的 Section 标题只用对象短名，不带 `#hash`；“定位”之后写 topic“函数原型”，完整函数原型写在该 topic 里。设计对象 qname 不变。

提交前 `ds doc check` 必须通过。

## 7. 常见错误

- 用源码路径直接当 qname（File 需要 `files/<path>`）；
- 创建或删除 `files` / `namespaces` 根对象；
- 给 Method 手写 `#hash`，或漏掉 `--signature`；
- 给 Part 用没有行范围的 `--source`；
- 关系目标写成外部库类型；
- 未 `ds verify` 就 commit；
- 直接用 `ds rm --recursive` 删除源码已不存在的文件树，而不是 `ds source sync`；
- 改 qname 后没有重新 `ds show` 确认，仍用旧名字继续操作。

## 8. 速查

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
  --source src/data_core.rs:10-120 --document <value-section-qname>
ds create inner-class Entry --parent namespaces/data_core/Value \
  --source src/data_core.rs:40-80
ds create method get --parent namespaces/data_core/Value --signature 'get(path)' \
  --source src/data_core.rs:40-70

# 关联文档
ds edit files/src/main.rs --document <file-section-qname>
ds edit namespaces/data_core/Value --document <value-section-qname>

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

## 9. 引用

设计文档、评论和提案中可以嵌入引用，渲染为超链接。引用不是 DS 对象，
只是文本约定，不参与 `ds verify`、`ds doc check` 或对象图。

```text
[[<target>|<label>]]
```

| target | 指向 |
|---|---|
| `issue:<uuid>` | Issue |
| `task:<uuid>` | Task |
| `ds:<qname>` | 设计空间对象、文档或章节 |
| `concept:<qname>` | 概念空间中的 Concept |
| `code:<path>[:<start>-<end>]` | 源码位置 |

示例：

```text
[[issue:01a1f3fe-909a-735b-bab8-8c8e1789a5ca|设计缺失]]
[[ds:namespaces/app/Auth|Auth]]
[[ds:documents/design/drafts/draft-7f3a2c9e|会话状态重构草案]]
[[concept:concepts/agent/agent|智能体]]
[[task:01a1f3fe-c18e-709d-9f2e-075ea5bf3165|建立设计基线]]
[[code:src/auth.rs:120-160|src/auth.rs:120-160]]
```

- `<label>` 省略时显示目标名称或 qname；
- `concept:` 引用必须指向 `concepts/` 下的 Concept 对象；用 `ds concept check` 校验；
- 目标不存在时不阻止保存，渲染为未解析链接或普通文本；
- 参考文献显示字面量 `[[...]]` 时用反斜杠转义：`\[\[...]]`；
- 引用可以出现在文档 text、topic 段落、list item 和评论中。
