# design-space CLI

- 状态：第一版 CLI 已实现为 `ds`
- 适用模型：`docs/storage-format.md`
- 二进制：`ds`

第一版已实现：

```text
init
clone
create（源码对象）
edit（design、source、parent/namespace/file/module/name、继承/接口/mixin/override 关系）
doc mkdir/create/section/text/list（文档对象）
dep add/rm/list（结构关系自动计入显式依赖）
source fetch/checkout/status/ls/cat/log/sync
remote add/show/set-url
push/fetch/pull（本地目录、HTTP、SSH；HTTP 支持 Bearer token）
gc
repack
add（qname、-A、--file、--directory）
unstage（--file、--directory）
reset（全部或按名称）
diff（worktree/index 或 --staged）
rm
checkout（已有分支或 -b 新建）
branch delete（删除本地分支）
ls
status
commit（--allow-empty）
show
log
doc mkdir/create/section add/text add/text set/list add/list item add/comment add/resolve/list/show
```

网络传输由 `ds-server` 提供。一个 HTTP `ds-server` 管理一个仓库集合，类似 Git 服务器：

```bash
ds-server --root <repos-root>                     # HTTP, 默认 127.0.0.1:7700
ds-server --root <repos-root> --listen 0.0.0.0:7700
ds-server --root <repos-root> --token-file <agent-token> --admin-token-file <admin-token>
                                                          # Agent token 只能写临时分支；admin token 可移动受保护分支
ds-server --stdio --root <repo-dir>               # SSH 传输协议（单个仓库）
```

`<repos-root>` 下的每个子目录是一个独立 ds 仓库。HTTP remote URL 形如
`http://host:port/<repo>`。

## 1. 基本约定

- 主要地址是 `qualified_name`。
- object hash 只作为内部身份、调试和消歧入口。
- 子对象通过 parent 建立，不存在 `--children`。
- kind 不可编辑。
- method 的 signature 不可编辑；改签名就是新 method。
- content_hash 由 source 计算，不能直接填写。
- 其他设计对象的 `document` 字段是 Document/DocumentSection 的 qname；修改文档正文不需要改引用。

下面是完整语法。

```bash
ds init --source <git-url> [--dir <root>]
ds clone <remote> [--branch main]
ds push [--branch <branch>] [--force]

ds source fetch
ds source checkout <ref>
ds source worktree
ds source status
ds source ls [<path>]
ds source cat <path>
ds source log [<ref>]
ds source sync
ds source changes [--base <git-ref>]

ds create <kind> <name> [options]
ds edit <qname> [--name <local>] [options]
ds rm <qname> [--force] [--recursive]

ds dep add <src> <dst> --explicit [--evidence path:lines]
ds dep add <src> <dst> --implicit [--rationale-file file]
ds dep rm <src> <dst> --explicit|--implicit
ds dep list <name>

ds status
ds verify
ds diff
ds diff --staged
ds add <name>...
ds add -A
ds reset [<name>...]
ds reset --hard
ds commit -m "..." [--allow-empty]

ds checkout <branch> [--force]
ds checkout -b <branch>
ds branch delete <branch>

ds show <name> [--source] [--staged] [--format text|json|md]
ds log [--limit N]
ds ls [--files|--namespaces|--documents|--design] [--staged]
ds doc mkdir <qname>
ds doc create <qname> --title <text>
ds doc section add <target> --title <text>
ds doc topic add <target> --title <text>
ds doc text add <target> --text <text>
ds doc text set <qname> --text <text>
ds doc block add <target> [--language <lang>] --text <text>
ds doc block set <qname> [--language <lang>] [--text <text>]
ds doc mermaid add <target> --text <text>
ds doc mermaid set <qname> --text <text>
ds doc html add <target> --text <text>
ds doc html set <qname> --text <text>
ds doc list add <target>
ds doc list item add <target> --text <text>
ds doc comment add <qname> --text <text>
ds doc comment resolve <qname> <comment-id>
ds doc comment list <qname>
ds doc comment clear
ds doc show <qname> [--format text|json|md]
ds doc import --file <path>
ds doc layout
ds doc check
ds structure init
ds structure system create <name> --title <text> [--document <qname>]
ds structure module create <name> --parent <system-or-module-or-layer> --title <text> [--document <qname>]
ds structure layer create <name> --parent <system-or-module> [--title <text>] [--order <n>] [--document <qname>]
ds structure layer set <qname> [--title <text>] [--order <n>] [--document <qname>]
ds structure layer rm <qname>
ds structure assign <container> <element...>
ds structure unassign <container> <element...>
ds structure list
ds structure show <qname>
ds structure check
ds structure unassigned
ds concept mkdir <qname>
ds concept create <qname> --summary <text> [--name <text>] [--explanation <text>]
ds concept set <qname> [--name <text>] [--summary <text>] [--explanation <text>]
ds concept feature add <qname> <text>
ds concept feature rm <qname> <n>
ds concept relation add <qname> <text>
ds concept relation rm <qname> <n>
ds concept alias add <qname> <text>
ds concept alias rm <qname> <n>
ds concept show <qname>
ds concept ls [<qname>]
ds concept mv <qname> <parent> [--name <local>]
ds concept rm <qname>
ds concept check
ds gc
ds repack
```

## 2. 系统、模块与层

逻辑结构树独立于文件和命名空间树：

- 根固定为 `design`；
- 系统是 `design/<system>`；
- 模块是 `<system-or-module-or-layer>/<module>`；
- 层是 `<system-or-module>/<layer>`，只在 System 或 Module 内部存在，不能嵌套层；
- 容器（System/Module/Layer）的成员只引用顶层 Class/Interface/Function/Mixin，且一个元素恰好属于一个容器；
- 层是有序的：同级层的 `order` 从 1 开始、互不相同；只有相邻层之间允许依赖；
- 一个 System/Module 一旦有层，它的其他代码成员和子模块都必须放进层里，不能直接挂在它下面；
- 没有层的 System 可以有模块和直属成员；模块通过 parent 嵌套，不写进 members。

```bash
ds structure init
ds structure system create app --title '应用' --document documents/design/technical/section1
ds structure layer create api --parent design/app --title '接口层' --document documents/design/technical/section2
ds structure layer create core --parent design/app --title '核心层' --document documents/design/technical/section3
ds structure module create handlers --parent design/app/api --title '处理器' --document documents/design/technical/section4
ds structure assign design/app/api namespaces/app/Entry
ds structure assign design/app/api/handlers namespaces/app/Auth namespaces/app/Session
ds structure check
```

`ds structure layer create` 不传 `--order` 时追加到同级层末尾；`ds structure layer rm` 只能删除空层。

`ds structure check` 检查成员存在、唯一归属、空模块、空系统、空层、层顺序、层嵌套、有层容器的直属成员/子模块、相邻层依赖以及所有顶层元素是否已归属。

## 2.1 概念空间

专业词汇用概念对象定义，组织在 `concepts` 目录树中：

- 根固定为 `concepts`，由 `ds init` 创建；旧工作区首次使用概念命令时补建；
- 概念目录是 `concepts/<dir>`，可以多级嵌套；
- 概念是 `<concept-directory>/<concept>`，parent 必须是概念目录；
- 概念包含 `name`、`summary`、`explanation`、`characteristics`、`relations`、`aliases`；
- 文档正文用 `[[concept:<qname>|<label>]]` 引用概念，`ds concept check` 校验引用能解析到 Concept 对象；
- `ds concept mv` 只能移动概念或空目录，非空目录先移动子项；
- `ds concept rm` 只能删除概念或空目录。

```bash
ds concept mkdir concepts/agent
ds concept create concepts/agent/agent --name '智能体' --summary '能自主执行任务的主体。'
ds concept feature add concepts/agent/agent '有独立身份。'
ds concept relation add concepts/agent/agent '智能体执行任务。'
ds concept alias add concepts/agent/agent 'Agent'
ds concept check
```

## 3. clone

```bash
ds clone <remote> [--branch main] [--dir <root>]
```

- `remote` 可以是本地路径、`file://`、HTTP(S) 或 SSH；
- clone 不接受 `--source`：源码 URL 和 object format 来自远端仓库根的
  `repo.toml`，fetch 时一起取到 `.ds/repo.toml`；`.ds/config` 只保存 `remote`；
- `ds init` 同样写 `.ds/repo.toml`，所以 init 和 clone 的工作区结构一致；
- `repo.toml` 由服务端建库时写入，客户端 push 不会传输或修改它；
- `--branch` 默认 `main`；
- clone 会初始化 workspace、fetch remote、创建本地分支、设置 HEAD、index 和 worktree；
- clone 不自动拉取源码；执行 `ds source fetch` 或 `ds source worktree` 后，工作区根目录成为源码 Git worktree；
- 不接受 token 参数或环境变量。HTTP remote 自动读取预配置 token：
  `/run/foundry/ds_token`、`$XDG_CONFIG_HOME/ds/token`、`~/.config/ds/token`；
- SSH remote 使用 SSH 认证。如果 `/run/foundry/ds_key` 存在，`ds` 直接读取它和
  `/run/foundry/ds_known_hosts` 作为 SSH 身份，不需要环境变量或命令参数。

## 4. qualified name 生成

| kind | qualified name |
|---|---|
| directory / file / namespace | parent qname + `/` + name |
| 顶层 class / interface / function / mixin | namespace qname + `/` + name |
| 内部 class / interface / mixin | parent qname + `/` + name |
| method | parent qname + `/` + name + `#` + signature hash |
| part | parent qname + `/part#` + content hash |
| document-directory / document | parent qname + `/` + name |
| section | parent qname + `/section<n>` |
| 正文元素（text / list / topic） | parent qname + `/body<n>` |
| list item（text / list） | list qname + `/item<n>` |
| system / module / layer | parent qname + `/` + name |
| concept-directory / concept | parent qname + `/` + name |

根对象 `files`、`namespaces`、`documents`、`concepts` 由 `init` 创建；`design` 由 `structure init` 创建。它们都不能通过 `create` 创建。

## 5. source 语法

```text
--source path            整个文件
--source path:start-end  1-based 行范围
```

- directory/file：只接受 path；
- namespace/class/struct/enum/interface/method/function/mixin：path 或 path:start-end；
- part：必须 path:start-end；
- source 原始字节计算 content_hash；
- 无 source 时 content_hash 为零；
- 有 source 时 content_hash 必须非零。

## 6. create 矩阵

| kind | 必填 | 可选 | 禁止 / 派生 |
|---|---|---|---|
| directory | `--parent` | `--source path`、`--document` | content_hash、dependencies、children |
| file | `--parent` | `--source path`、`--document` | dependencies、children |
| namespace | `--parent` | `--source path[:lines]`、`--document` | dependencies、children |
| class | `--namespace` | `--file`、`--module`、`--source`、`--document`、`--superclass`、`--interface`、`--mixin` | `--parent` |
| inner-class | `--parent` | `--source`、`--document`、`--superclass`、`--interface`、`--mixin` | namespace、file、module |
| struct | `--namespace` | `--file`、`--module`、`--source`、`--document` | `--parent`、superclass、interface、mixin |
| inner-struct | `--parent` | `--source`、`--document` | namespace、file、module、superclass、interface、mixin |
| enum | `--namespace` | `--file`、`--module`、`--source`、`--document` | `--parent`、superclass、interface、mixin |
| inner-enum | `--parent` | `--source`、`--document` | namespace、file、module、superclass、interface、mixin |
| interface | `--namespace` | `--file`、`--module`、`--source`、`--document`、`--extends` | `--parent`、superclass、interface、mixin |
| inner-interface | `--parent` | `--source`、`--document`、`--extends` | namespace、file、module、superclass、mixin |
| method | `--parent`、`--signature` | `--source`、`--document`、`--overrides` | children、superclass、interface、mixin |
| function | `--namespace` | `--file`、`--module`、`--source`、`--document` | parent、superclass、interface、mixin |
| part | `--parent`、`--source path:start-end` | — | signature、namespace、file、module、document |
| mixin 顶层 | `--namespace` | `--file`、`--module`、`--source`、`--document`、`--on`、`--superclass`、`--interface`、`--mixin` | — |
| mixin 内部 | `--parent` | `--source`、`--document`、`--on`、`--superclass`、`--interface`、`--mixin` | namespace、file、module |

顶层类型只用 `--namespace`（可选 `--file`、`--module`）；`--parent` 只用于内部类型，parent 必须是 class、struct、enum、interface 或 mixin。directory/file 的 parent 必须是 directory，namespace 的 parent 必须是 namespace。

重复参数：

- `--interface` 可重复；
- `--mixin` 可重复；
- `--on` 可重复；
- `--superclass`、`--extends`、`--overrides` 最多一个。

## 6.1 文档对象

文档片段也是设计对象，和源码对象共用 View、index、worktree 和提交。

根对象：

```text
documents
```

文档对象 kind：

```text
document-directory
document
document-section
document-list
document-text
document-topic
document-block
document-mermaid
document-html
```

层级：

```text
documents
  DocumentDirectory
    Document
      DocumentSection
        body: DocumentText | DocumentList | DocumentTopic | DocumentBlock | DocumentMermaid | DocumentHtml
          DocumentTopic.paragraphs: DocumentText | DocumentList | DocumentBlock | DocumentMermaid | DocumentHtml
          DocumentList.items: DocumentText | DocumentList
        sections: DocumentSection
          ...
    DocumentDirectory
```

- `DocumentSection.body` 只放正文元素；子章节单独存在 `sections`；
- `DocumentTopic` 是正文里带标题的内容块，标题下是段落列表；
- `DocumentTopic` 的段落只允许 `DocumentText`、`DocumentList`、`DocumentBlock`、`DocumentMermaid` 或 `DocumentHtml`；
- `DocumentBlock` 是 Markdown 围栏代码块，只能放进 Section 正文或 Topic 段落；
- `DocumentMermaid` 是 Mermaid 流程图，只能放进 Section 正文或 Topic 段落；
- `DocumentHtml` 是 HTML/SVG 预览，只能放进 Section 正文或 Topic 段落；
- `DocumentList` 的项只允许 `DocumentText` 或 `DocumentList`；
- `DocumentTopic` 只能直接属于 `DocumentSection`，不能嵌套 `DocumentTopic` 或 `DocumentSection`。

### 命名

- DocumentDirectory、Document 的名字由调用方给：小写字母、数字、`-`、`_`；
- 子章节自动命名为 `section1`、`section2`；
- 正文元素自动命名为 `body1`、`body2`，不论它是 text、list、topic、block、mermaid 还是 html；
- 列表项自动命名为 `item1`、`item2`；
- 自动编号只取决于父对象和集合，与子对象 kind 无关；
- 编号在同一个父对象的对应集合内分配，从 1 开始，已有编号不重排。

### 目标位置

`add` 只接受一个目标位置 qname，不传父对象，也没有 `--after`：

- 目标是头锚点 → 插到集合最前面；
- 目标是真实元素 → 插到该元素后面；
- 集合为空时，头锚点等价于追加；
- 追加到末尾时，目标指向集合最后一个真实元素。

锚点：

```text
<document-or-section-qname>/section     Document / Section 的子章节头锚点
<section-or-topic-qname>/body           Section / Topic 的正文头锚点
<list-qname>/item                       DocumentList 项头锚点
```

锚点不是对象，只用于 `add` 定位；不存储、不输出、不参与 verify。

### 创建文档目录和文档

```bash
ds doc mkdir documents/specs
ds doc create documents/specs/auth --title "认证"
```

### 章节

```bash
# 第一个章节：文档为空，头锚点即追加
ds doc section add documents/specs/auth/section --title "登录"

# 插到 section1 后面
ds doc section add documents/specs/auth/section1 --title "验证"

# 给 section1 加子章节：用它自己的 section 锚点
ds doc section add documents/specs/auth/section1/section --title "登录流程"
```

真实 Section qname 表示“作为兄弟插到它后面”；加子章节必须指向该 Section 下的 `section` 锚点，或它最后一个子章节。

Function/Method 的文档 Section 标题只用对象短名，不带 `#<signature-hash>`；“定位”之后写 topic“函数原型”，完整函数原型写在该 topic 里。对象 qname 不变。

### 正文

```bash
# 正文头插，得到 body1
ds doc text add documents/specs/auth/section1/body --text "登录使用 OAuth 2.0。"

# 插到 body1 后面，得到 body2
ds doc text add documents/specs/auth/section1/body1 --text "补充说明"

# 修改已有文本
ds doc text set documents/specs/auth/section1/body1 --text "新的文本"
```

### 列表

```bash
# 在 body2 后面创建一个列表，得到 body3
ds doc list add documents/specs/auth/section1/body2

# 列表头插，得到 body3/item1
ds doc list item add documents/specs/auth/section1/body3/item --text "第一项"

# 第一项后面，得到 body3/item2
ds doc list item add documents/specs/auth/section1/body3/item1 --text "第二项"
```

### 带标题内容

```bash
# 在 body3 后面加一个 topic，得到 body4
ds doc topic add documents/specs/auth/section1/body3 --title "认证流程"

# topic 的段落头插，得到 body4/body1；topic 的正文锚点同样是 body
ds doc text add documents/specs/auth/section1/body4/body --text "先校验凭据。"
```

`DocumentTopic` 有 `title` 和 `paragraphs`。`paragraphs` 的命名规则与 Section 正文相同：`body1`、`body2`。

### 代码块

```bash
# 在 body2 后面加一个 rust 代码块，得到 body3
ds doc block add documents/specs/auth/section1/body2 --language rust --text $'let token = login();\nnext_step();'

# 不带语言时输出裸围栏
ds doc block add documents/specs/auth/section1/body3 --text "plain text"

# 修改语言或内容
ds doc block set documents/specs/auth/section1/body3 --language text --text "changed"
```

- `--language` 是 Markdown 围栏的 info string；省略时是裸围栏；
- `--text` 允许换行，是代码块的原始内容；
- 代码块只能放在 Section 正文或 Topic 段落，不能作为列表项；
- 渲染为 Markdown 围栏代码块；代码内容不参与 `[[concept:...]]` 引用检查。

### Mermaid 流程图

```bash
# 在 body2 后面加一个流程图，得到 body3
ds doc mermaid add documents/specs/auth/section1/body2 --text $'flowchart TD\n  A[请求] --> B[校验]'

# 替换流程图源码
ds doc mermaid set documents/specs/auth/section1/body3 --text $'flowchart LR\n  A --> B'
```

- 只存 Mermaid 源码，不存渲染结果；
- 智能体用 `ds doc mermaid add` 创建，不必手写围栏；
- 流程图只能放在 Section 正文或 Topic 段落，不能作为列表项；
- Foundry 设计空间会用内置的 mermaid.js 渲染成图；渲染失败时显示错误信息；
- Mermaid 源码不参与 `[[concept:...]]` 引用检查。

### HTML/SVG 预览

```bash
# 插入 UI 效果图或任意 HTML/SVG
ds doc html add documents/specs/auth/section1/body2 --text '<div style="padding:12px">登录页效果图</div>'

# 直接插入 SVG
ds doc html add documents/specs/auth/section1/body3 --text '<svg viewBox="0 0 100 40"><rect width="100" height="40" fill="#eee"/></svg>'

# 替换内容
ds doc html set documents/specs/auth/section1/body3 --text '<div>新的效果图</div>'
```

- 存原始 HTML/SVG 源码，不存渲染结果；
- 只能放在 Section 正文或 Topic 段落，不能作为列表项；
- Foundry 设计空间使用无脚本沙箱 iframe 渲染，默认高度 360px，超出可滚动；
- 沙箱不执行脚本、不允许表单和顶层跳转；外部资源被 CSP 禁止，图片使用 data URI；
- HTML/SVG 源码不参与 `[[concept:...]]` 引用检查。

### 批量导入

`ds doc import` 在一个进程里按 JSON 计划创建整棵文档子树，适合为大量 File/Directory 批量建立初始文档。重复执行是幂等的：同名 Section 会复用，相同的正文元素不会重复追加。

```bash
ds doc import --file plan.json
# 或从标准输入读
ds doc import --file -
```

计划格式：

```json
{
  "root": {"qname": "documents/specs/auth", "kind": "document", "title": "认证"},
  "sections": [
    {
      "title": "登录",
      "document": ["namespaces/app"],
      "body": [{"type": "text", "text": "登录使用 OAuth 2.0。"}],
      "sections": [
        {
          "title": "验证码",
          "body": [
            {"type": "list", "items": ["第一项", "第二项"]},
            {"type": "block", "language": "rust", "text": "fn main() {}"},
            {"type": "mermaid", "text": "graph TD; A-->B;"},
            {"type": "html", "text": "<b>hi</b>"},
            {"type": "topic", "title": "示例", "body": [{"type": "text", "text": "主题段落"}]}
          ]
        }
      ]
    }
  ]
}
```

- `root`：字符串表示已存在的文档或文档目录 qname；对象形式在不存在时创建，`kind` 为 `document`（默认）或 `directory`，`title` 默认取 qname 最后一段。
- `sections`：递归子树；每层按 `title` 查找已有 Section，找不到才创建。
- `body`：`text`、`list`、`block`、`mermaid`、`html`、`topic`；`topic` 内不能再嵌套 `topic`。
- `document`：把列出的设计对象（如 `files/src/main.rs`、`namespaces/app`）关联到当前 Section。
- `ds doc show <qname> --format json` 的输出就是同一种结构，可以直接回灌 `ds doc import`。完整规范见 [document-io.md](document-io.md)。

### 评论

每个文档对象都有独立评论列表。评论 id 是对象内的 `comment1`、`comment2`，状态只有 `open` 和 `resolved`。

```bash
# 添加评论
ds doc comment add documents/specs/auth/section1/body1 --text "补充错误码"

# 解决评论
ds doc comment resolve documents/specs/auth/section1/body1 comment1

# 列出评论和状态
ds doc comment list documents/specs/auth/section1/body1
# comment1<TAB>open<TAB>补充错误码

# 删除工作区中的所有评论（用于合并前清理；不会自动提交）
ds doc comment clear
```

`ds doc show` 会在对应对象下输出评论：

```text
comment1 [resolved] 补充错误码
```

`ds show <文档对象>` 会输出 `comments:` 字段，`ds diff` 也会把评论变化计入对象差异。

### 查看

```bash
ds doc show documents/specs/auth                 # 默认 text：缩进视图
ds doc show documents/specs/auth --format json   # 可回灌 ds doc import
ds doc show documents/specs/auth --format md     # Markdown 视图
```

按顺序输出文档、章节、正文元素（text / list / topic）、topic 段落、列表和列表项。
JSON 和 Markdown 的格式规范见 [document-io.md](document-io.md)。

### 给设计对象关联文档

`create` / `edit` 的 `--document` 接收 Document 或 DocumentSection 的 qname：

```bash
ds create class Auth --namespace namespaces/app \
  --document documents/design/technical/section1/section1

ds edit namespaces/app/Auth --document documents/design/technical/section1/section2
ds edit namespaces/app/Auth --clear-document
```

- `document` 是 qname 引用，不是 DocHash；
- 修改 Section 正文不会改变引用它的对象；
- Part 不关联文档，不能使用 `--document`；
- `document` 零表示没有文档；
- `ds show` 输出 `document: <qname>`；
- `ds add --file` / `--directory` 会一并暂存对象关联的 Section；
- 目标不存在或 kind 不是 Document/DocumentSection 时，`ds verify` 和 `ds commit` 会拒绝。

## 7. edit 矩阵

```bash
ds edit <qname> [options]
```

通用：

- `--document` / `--clear-document`
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
| part | source、parent |
| mixin | source、parent/namespace/file/module、on add/rm、superclass、interface add/rm、mixin add/rm、design |

不可编辑：

- kind；
- method signature；
- children（通过创建子对象或移动 parent）；
- content_hash；
- structural relation 不得重复加入 dependencies。

## 8. 校验

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

## 9. 暂存语义

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
ds ls --documents        # document-directory / document / document-section / document-list / document-text / document-topic / document-block / document-mermaid / document-html
ds ls --design           # 有 document 引用的对象，输出 qname<TAB>document qname
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
ds rm <qname> [--force] [--recursive]
```

- 根对象不能删除；
- 有子对象时拒绝，先删除子对象；`--recursive` 按子树后序删除对象和全部 containment 后代；
- 被关系或依赖引用时，默认拒绝；`--force` 先移除其他对象对它的关系/依赖，
  再删除，并输出被修改的引用方；递归删除时，同一规则作用于子树中的每个对象，
  子树内部的引用不算外部引用；
- 文档片段的 containment 引用（父对象的 `body` / `sections` / `items`）自动移除，不需要 `--force`；
- 只修改 worktree，不自动暂存。

### 7.3.0 source fetch / checkout / worktree

```bash
ds source fetch
ds source checkout <ref>
ds source worktree
```

- `ds source fetch` 更新 `.ds/source` bare clone，并在工作区根目录创建或复用 Git worktree；
- `ds source checkout <ref>` 更新 `source-head`，并把根目录 worktree checkout 到该 commit；
- `ds source worktree` 显式创建/确认根目录 worktree，输出其路径；
- 根目录 `.git` 是文件，指向 `.ds/source/worktrees/root`；
- `.ds/` 通过 `.ds/source/info/exclude` 对源码 Git 隐藏；
- 源码仓库如果包含 `.ds` 目录，拒绝使用根目录 worktree；
- hooks 和 submodule 自动执行被禁用。

### 7.3.1 source sync

```bash
ds source sync
```

- 一次把 `files/` 下的 File/Directory 对象与当前 source HEAD 对齐，不逐个对象调用命令；
- 新增、删除按源码路径处理；用 `git diff -M` 识别重命名时，移动原对象及其子孙，保留 document 引用、关系和依赖，并把 source 路径改到新路径；
- 目录对象按源码树中的目录生成；源码中已不存在的 File/Directory 连同子孙删除；
- 只修改 worktree，不自动暂存；需要时再 `ds add -A` / `ds commit`；
- 需要先执行 `ds source fetch`。

### 7.3.2 status --porcelain

```bash
ds status --porcelain [--detail]
```

只输出未暂存对象，不输出分支/HEAD/Index/Worktree 头。默认每行：

```text
status<TAB>qname
```

需要脚本读取 kind 和 source 时加 `--detail`，每行变为：

```text
status<TAB>kind<TAB>source<TAB>qname
```

普通使用只用 qname；kind、source、document 等信息用 `ds show <qname>` 查询。

### 7.3.3 file/directory staged

```bash
ds add --file <source path>
ds add --directory <source dir>
ds unstage --file <source path>
ds unstage --directory <source dir>
```

- `add --file`：按 source 路径、顶层类型的 `file` 关联以及 `files/<path>/` 前缀收集对象；存在的写入 index，worktree 已删除的从 index 移除；
- `add --directory`：只暂存该目录的 Directory 对象；
- `unstage --file` / `unstage --directory`：把该文件/目录相关对象从 index 恢复到 HEAD，worktree 不变；目录会包含整个 `files/<path>/` 子树、source 位于该目录下的对象及关联文档，避免移走父对象后留下孤儿子对象；
- 命令名使用 `--directory`，因为全局 `--dir` 已用于指定工作区。

### 7.3.4 source changes

```bash
ds source changes [--base <git-ref>]
```

- 默认 base 是当前 ds 分支 HEAD 提交的 `View.source_commit`；
- 对比当前 `ds source` HEAD，输出 `A/M/D/R` 行：`A<TAB>path`、
  `M<TAB>path`、`D<TAB>path`、`R<TAB>old<TAB>new`；
- base 为零、找不到、或不是当前 HEAD 的祖先时，全部文件按 `A` 输出；
- 需要先执行 `ds source fetch`。

### 7.3.5 status / verify

```bash
ds status
ds verify
```

- `status` 打印 HEAD/index/worktree，并列出 `Changes to be committed` 和
  `Changes not staged for commit` 的对象 qname；
- `verify` 严格检查 worktree 对象图：关系/依赖目标、kind、关系环、
  overrides、依赖、parent 结构；失败时返回非零；
- `ds commit` 在提交前执行同样的校验。

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

### 7.7 show / ls

```bash
ds show <name> [--source] [--staged] [--format text|json|md]
ds ls [--files|--namespaces|--documents|--design] [--staged]
```

- `show` 输出对象字段、`document:` 引用和 `children:` 列表；
- `--staged` 从 index 读取，默认从 worktree 读取；
- `ls --staged` 同理。

### 7.8 前向引用

`create` / `edit` / `dep add` 写 `superclass`、`interfaces`、`mixins`、`on`、
`extends`、`overrides` 和依赖时，目标对象可以暂时不存在。`ds add` 不校验目标
存在；`ds verify` 和 `ds commit` 负责解析。容器类引用（parent、namespace、
file、module）仍然必须存在。

### 7.9 gc / repack

```bash
ds gc                        # 展开 pack，删除不可达对象/提交/文档/符号
ds repack                    # 把所有 loose records 收进一个 pack
```

store 读取时优先 loose file，找不到时回退到 pack。

## 10. 第一版范围

已实现命令见文首。

暂缓：

- 字段级暂存；
- 多设计空间共享对象。

## 11. 远程

remote URL：

| URL | 传输 |
|---|---|
| `/path` 或 `file:///path` | 本地仓库目录 |
| `http://host:port/<repo>` | `ds-server` HTTP API |
| `https://host[:port]/<repo>` | 同上，TLS |
| `ssh://[user@]host[:port]/absolute/path` | SSH 上运行 `ds-server --stdio --root <path>` |

`ds-server` 的每个仓库与工作区 `.ds` 使用相同的 `objects/`、`commits/`、
`docs/`、`symbols/`、`packs/` 和 `refs/heads/` 布局。

HTTP API：

```text
GET  /repos                       -> "<repo>\n" ...
POST /repos                       <- {"name":"<repo>","branch":"main"}
GET  /<repo>/refs                 -> "<branch>\t<commit>\n" ...
PUT  /<repo>/refs                 <- "<branch>\t<commit>\n" ...
GET  /<repo>/files                -> "<relative-path>\n" ...
GET  /<repo>/files/<relative>     -> file bytes
PUT  /<repo>/files/<relative>     <- file bytes
POST /<repo>/push                 <- PUSH1 bundle
```

`POST /<repo>/push` 一次上传缺失记录和 ref 命令。服务端校验记录哈希、ref 的
`expected-old`、fast-forward 和设计对象闭包，然后在短 ref 锁内原子更新 ref。
bundle 失败不会改变任何可见分支；已验证但未引用的记录由 GC 回收。

`PUT /<repo>/files/<relative>` 和 `PUT /<repo>/refs` 作为 legacy 逐文件协议保留。

`POST /repos` 创建一个空设计空间仓库，并在 `<branch>`（默认 `main`）上
创建初始 View/commit。请求体可以带 `source_url`，服务端据此写入仓库根的
`repo.toml`。已经存在时返回 409。

规则：

- push 把所有缺失记录和 ref 命令打进一个 bundle；记录先在 `incoming/` quarantine 校验，再在 `flock` 保护的 ref 临界区内发布并原子更新 ref；非 force 时要求 fast-forward；`repo.toml` 不在 push 范围内；
- `gc` 和 `repack` 使用同一把 `refs.lock`，执行期间会阻塞 ref 更新；
- fetch 只下载本地缺失的文件，然后写入 `refs/remotes/origin/<branch>`；
- pull 先 fetch，再 fast-forward；index 或 worktree 有修改时拒绝；
- 服务端校验路径必须是 canonical 的 `<kind>/<aa>/<bb>/<hash>`，并校验上传内容与
  hash 一致；refs 只能指向已上传的 commit；
- 仓库必须已存在于 `<repos-root>/<repo>`；服务端不会自动创建仓库；
- HTTP 未配置 token 时不认证；配置后所有请求都必须带
  `Authorization: Bearer <token>`，否则返回 401；
- token 文件只包含一个 token，首尾空白会被忽略，token 内不能有空白或控制字符；
- 客户端不接受 token 参数或环境变量，而是自动读取预配置 token：
  `/run/foundry/ds_token`、`$XDG_CONFIG_HOME/ds/token`、`~/.config/ds/token`
  （按顺序取第一个存在的文件）；
- 客户端不能写 `repo.toml`；服务端对 `PUT repo.toml` 返回错误；
- SSH 传输不单独校验 token，认证由 SSH 完成；`--stdio` 与 `--token-file` 不能同时使用；
- SSH remote 使用 `ssh` 默认身份；`/run/foundry/ds_key` 存在时，`ds` 直接读取该文件和
  `/run/foundry/ds_known_hosts`，并对该 remote 使用 `IdentitiesOnly=yes` 和
  `StrictHostKeyChecking=yes`；
- 没有内置 TLS。Bearer token 在纯 HTTP 上是明文；对外暴露时应使用反向代理或 SSH。
