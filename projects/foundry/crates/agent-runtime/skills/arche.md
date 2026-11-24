# Arche 设计空间使用技能

`arche` 是 Arche v2 设计空间的命令行。设计空间保存技术、需求和概念对象；源码只读。

## 1. 基本模型

- 工作区里有一个 `.arche/`。不要手改 `.arche/` 下的文件，只用 `arche` 命令。
- 设计空间不是 git 仓库。远程只接受 Arche 协议，不能用 `git clone` / `git push` 访问。
- 设计对象是内容寻址的。对象有 `qname`、`kind` 和源码锚点。
- 工作流：worktree → `arche add` → index → `arche commit` → `arche push`。
- 三个根对象由 `arche init` 创建，不能自行创建或删除：
  - `technical`
  - `requirement`
  - `concept`

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
technical/system/<name>
technical/module/<name>
technical/layer/<name>
technical/directory/<path>
technical/file/<path>
technical/namespace/<name>
<owner>/<type>
<owner>/<method>#<hash8>
```

常用类型：

```bash
arche create technical-system <name>
arche create technical-module <name> --parent <system>
arche create technical-layer <name> --parent <system|module>
arche create technical-namespace <name> --parent technical
arche create technical-class <name> --namespace <ns>
arche create technical-struct <name> --namespace <ns>
arche create technical-enum <name> --namespace <ns>
arche create technical-interface <name> --namespace <ns>
arche create technical-mixin <name> --namespace <ns>
arche create technical-function <name> --namespace <ns>
arche create technical-method --parent <type> --prototype "<name>(<types>) -> <return>"
arche create technical-part <name> --parent <type> --source <path:start-end>
```

Arche 内部不解析源码 AST。Namespace、Class、Method 等由你根据源码手工创建。

## 4. 文档字段

文档不是独立对象树，而是对象字段：

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
arche search <text> [--kind <kind>]
arche check
```

`show --format md` 从对象字段动态生成文档，不存在单独的 Document 对象。

## 6. 暂存和提交

```bash
arche status
arche status --porcelain
arche add <qname>...
arche add -A
arche add --file <source-path>
arche unstage <qname>...
arche diff
arche diff --staged
arche commit -m "message"
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
arche clone <url> <dir> [--branch <branch>]
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
