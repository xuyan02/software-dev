# Arche 设计空间使用技能

`arche` 是 Arche v2 设计空间的命令行。设计空间保存技术、需求和概念对象；源码只读。

## 1. 基本模型

- 工作区里有一个 `.arche/`。不要手改 `.arche/` 下的文件，只用 `arche` 命令。
- `.arche/` 是设计空间，不是源码。提交源码前确保 `.arche/` 在 `.gitignore` 中或被 Git 忽略；绝不 `git add .arche`。
- 设计空间不是 git 仓库。远程只接受 Arche 协议，不能用 `git clone` / `git push` 访问。
- 设计对象是内容寻址的。对象有 `qname`、`kind` 和源码锚点。
- 工作流：worktree → `arche add` → index → `arche commit` → `arche push`。
- 固定根对象由 `arche init` 创建，不能自行创建或删除：
  - `project`（项目空间）
  - `technical`
  - `requirement`
  - `concept`
- 项目说明书固定在 `project/document`，字段 `document` 是 `DocumentParagraphList`；章节包括定位、背景、目标、非目标、分期计划、约束、风险。

## 2. qname

统一格式：

```text
<parent-qname>/<name>
```

方法：

```text
<parent-qname>/<name>#<hash8>
```

qname 只能包含字母、数字、`/`、`#`。中文名称放在 `display_name` / `real_name` 等字段，不放进 qname。

## 3. 技术对象

```text
technical
technical/<system>
technical/<system>/<module>
technical/<system>/<layer>
technical/directory/<path>
technical/file/<path>
technical/namespace/<name>
<namespace>/<type>
<type>/<method>#<hash8>
```

常用类型：

```bash
arche create technical-system <name>
arche create technical-module <name> --parent technical/<system>
arche create technical-layer <name> --parent technical/<system>
arche create technical-namespace <name> --parent technical
arche create technical-class <name> --namespace <ns> --file technical/file/<path> --module technical/<system>/<module> --source <path:start-end>
arche create technical-struct <name> --namespace <ns> --file technical/file/<path> --module technical/<system>/<module> --source <path:start-end>
arche create technical-enum <name> --namespace <ns> --file technical/file/<path> --module technical/<system>/<module> --source <path:start-end>
arche create technical-interface <name> --namespace <ns> --file technical/file/<path> --module technical/<system>/<module> --source <path:start-end>
arche create technical-mixin <name> --namespace <ns> --file technical/file/<path> --module technical/<system>/<module> --source <path:start-end>
arche create technical-function <name> --namespace <ns> --file technical/file/<path> --module technical/<system>/<module> --source <path:start-end>
arche create technical-method --parent <type> --prototype "<name>(<types>) -> <return>" --source <path:start-end>
arche create technical-part <name> --parent <type> --source <path:start-end>
```

对象类型：

| kind | 说明 | qname |
|---|---|---|
| System | 顶层技术系统 | `technical/<system>` |
| Module | 系统内的模块 | `technical/<system>/<module>` |
| Layer | 系统内的层 | `technical/<system>/<layer>` |
| Directory | 源码目录 | `technical/directory/<path>` |
| File | 源码文件 | `technical/file/<path>` |
| Namespace | 逻辑命名空间，不等于物理目录 | `technical/namespace/<name>` |
| Class | 有数据和方法，或变体携带数据的类型 | `<namespace>/<name>` |
| Struct | 只有数据、没有方法的类型 | `<namespace>/<name>` |
| Enum | 只有常量变体的类型 | `<namespace>/<name>` |
| Interface | 接口、协议或抽象契约 | `<namespace>/<name>` |
| Mixin | 只提供可复用行为的行为片段 | `<namespace>/<name>` |
| Function | 不属于某个类型的函数 | `<namespace>/<name>` |
| Method | 类型内部的可调用成员 | `<type>/<name>#<hash8>` |
| Part | 没有独立名字、但需要说明的代码片段 | `<parent>/part#<hash8>` |

判断规则：

- 变体携带数据的 enum 按 Class 处理；只有常量变体的用 Enum；
- 只有数据没有方法用 Struct；只有抽象方法用 Interface；
- 只有具体方法、没有数据的工具类按 Namespace + Function 处理；
- 嵌套类型使用对应类型并指定 `--parent <type>`；
- 继承、接口、混入、override 和依赖关系由调用方写入。

Arche 内部不解析源码 AST。Namespace、Class、Method 等由你根据源码手工创建。

## 4. 文档字段

文档（documents）不是独立对象树，而是对象字段。字段写作规范见 `design-writing` 技能：

```text
position             DocumentText
responsibilities     DocumentList
design_points        DocumentParagraphList
usage                DocumentParagraphList
```

写入：

```bash
arche doc text set <qname> position "入口"
arche doc list item add <qname> responsibilities "职责一"
arche doc topic add <qname> usage "状态"
arche doc code add <qname> usage --language rust "fn main() {}"
arche doc mermaid set <qname> flow "graph TD; A-->B;"
arche doc html add <qname> usage "<b>预览</b>"
```

批量导入：

```bash
arche doc import <qname> fields.json
arche batch plan.json
```

格式与 `arche show <qname> --format json` 的 `fields` 一致。

## 5. 查看

```bash
arche show <qname>
arche show <qname> --format json
arche show <qname> --format md
arche ls
arche search <text> [--kind <kind>] [--has-design]
arche check
arche doc check
arche structure list
arche structure unassigned
arche structure check
arche structure assign <element> --parent <system|module|layer>
arche comment add <qname> '<问题>'
arche comment list [--open]
arche comment resolve <id> --conclusion '<结论>' --agent <agent>
arche comment clear [<qname>]
```

`show --format md` 从对象字段动态生成文档，不存在单独的 Document 对象。

## 6. 暂存和提交

```bash
arche status
arche status --porcelain --detail
arche add <qname>...
arche add -A
arche add --file <source-path>
arche unstage <qname>...
arche diff
arche diff --staged
arche commit -m "message" --author <agent>
arche reset
arche reset --hard
```

- 对象级暂存，不做字段级暂存；
- `add --file` 会级联 File、目录链、关联类型和文档字段；
- `commit` 只使用 index。

## 7. 源码

```bash
arche source fetch
arche source checkout <git-ref>
arche source worktree
arche source status
arche source ls
arche source cat <path>
arche source set-url <git-url>
arche source sync
arche source changes
```

`source sync` 只维护 `TechnicalDirectory` / `TechnicalFile`，不解析代码结构。

## 8. 需求和概念

```bash
arche create requirement-user <real-name>
arche create requirement-story <name>
arche create requirement-system <name>
arche create requirement-module <name> --parent <system>
arche create requirement-function <name> --parent <module>
arche create requirement-interaction <name> --parent <function>

arche concept mkdir <qname>
arche concept create <qname> --name <display-name> --explanation "说明"
arche concept list
```

## 9. 远程

```bash
arche remote add <url>
arche clone <url> <dir> [--branch <branch>] [--source <git-url>]
# clone 自动继承远程仓库记录的源码地址；--source 只用于覆盖。
arche push
arche push --force
arche fetch
arche pull
```

## 10. 规则

- 不要创建外部库、标准库或第三方类型对象；
- 不要手改 `.arche/`；
- 不要用 git 命令访问设计空间远程；
- 文档写对象字段，不创建文档对象；
- 每次修改后先 `arche status`，再决定 `add` / `commit`。
