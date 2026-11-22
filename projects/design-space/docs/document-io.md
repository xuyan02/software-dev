# 文档 JSON 与 Markdown 格式

状态：规范 v1

## 1. 范围

- **JSON**：`ds doc import` 的输入、`ds doc show --format json` 的输出，以及批量操作的输入/输出。JSON 是可往返的规范格式。
- **Markdown**：`ds doc show --format md` 和 `ds show --format md` 的输出。只作为只读投影，不用于导入。

两种格式描述的是同一份设计内容；Markdown 更短，但不得丢失 JSON 中的标题、正文元素和 document 关联。

## 2. 文档树 JSON

### 2.1 根节点

```json
{
  "root": "documents/specs/auth",
  "body": [],
  "sections": [],
  "documents": []
}
```

`root` 可以是：

- 字符串：已存在的文档、文档目录或章节 qname；
- 对象：`{"qname":"...","kind":"document|directory|section|topic","title":"..."}`。对象不存在时按 `kind` 创建；`title` 只用于创建，已存在时不修改。

字段：

| 字段 | 说明 |
|---|---|
| `body` | 根节点的正文元素；只有 section、topic、list 等有正文的节点可以非空 |
| `sections` | 子章节，递归结构 |
| `documents` | 仅当 `root` 是 document-directory 时使用，子文档数组，每项是同构的根节点对象 |

### 2.2 章节

```json
{
  "qname": "documents/specs/auth/section1",
  "kind": "section",
  "title": "登录",
  "document": ["namespaces/app"],
  "body": [],
  "sections": []
}
```

- `qname`：章节在空间中的真实 qname，导出时必填；导入时可选。
- `kind`：固定为 `section`。
- `title` 必填。导入时先按 `qname` 查找已有章节；没有 `qname` 时按本层 `title` 查找，找到就复用，找不到才创建。
- `document`：把列出的设计对象（`files/...`、`namespaces/...`）关联到该章节，写入对象的 `document:` 字段。
- `body`：正文元素，顺序即输出顺序。
- `sections`：子章节，递归结构。

### 2.3 正文元素

| `type` | 字段 | 对应对象 |
|---|---|---|
| `text` | `qname`、`text` | document-text |
| `list` | `qname`、`items`：`{qname, text}` 数组 | document-list + document-text 项 |
| `block` | `qname`、`language`（可选）、`text` | document-block |
| `mermaid` | `qname`、`text` | document-mermaid |
| `html` | `qname`、`text` | document-html |
| `topic` | `qname`、`title`、`body` | document-topic |

`qname` 是对象在空间中的真实身份，导出时必填。导入时可选；提供时按 qname 定位已有对象。

`topic` 的 `body` 只能包含 `text`、`list`、`block`、`mermaid`、`html`，不能再嵌套 `topic`。

### 2.4 往返

`ds doc show <qname> --format json` 输出的就是上面的结构，可以直接作为 `ds doc import --file -` 的输入。重复导入是幂等的：

- 提供 `qname` 时按 qname 定位已有对象；qname 不存在、kind 不匹配或不在父对象下时直接报错；
- 没有 `qname` 时，同名章节复用，相同正文元素不重复追加；
- 相同的 `document` 关联重复设置不产生变化。

评论（comments）属于评审元数据，不进入 JSON 和 Markdown。

## 3. 批量操作

### 3.1 输入

```json
{
  "version": 1,
  "operations": [
    {"op": "upsert", "document": { "root": "...", "sections": [] }},
    {"op": "delete", "qname": "documents/specs/auth", "recursive": true, "force": true},
    {"op": "show", "qname": "documents/specs/auth", "format": "json"}
  ]
}
```

| `op` | 字段 | 含义 |
|---|---|---|
| `upsert` | `document` | 创建或更新一棵文档子树，语义同 `ds doc import` |
| `delete` | `qname`、`recursive`（默认 true）、`force`（默认 false） | 删除节点或子树，语义同 `ds rm` |
| `show` | `qname`、`format`（`json` 或 `md`） | 读取节点 |

默认按顺序执行，遇到错误停止。可选 `"continue_on_error": true` 继续执行后续操作。

### 3.2 输出

```json
{
  "version": 1,
  "results": [
    {"op": "upsert", "root": "documents/specs/auth", "created_sections": 2},
    {"op": "delete", "qname": "documents/specs/auth", "removed_objects": 7},
    {"op": "show", "qname": "documents/specs/auth", "format": "json", "content": {}}
  ]
}
```

失败的结果为 `{"op": "...", "error": {"message": "..."}}`。

### 3.3 当前实现

| 操作 | 当前命令 |
|---|---|
| upsert | `ds doc import --file <plan>` |
| delete | `ds rm <qname> -r [--force]` |
| show | `ds doc show <qname> --format json\|md` |

批量执行器 `ds batch --file <envelope.json>` 是后续实现；格式固定为上面的 envelope，因此可以先把批量计划写成这个结构。

## 4. Markdown 输出

Markdown 只作为输出，规则如下：

- 文档：`# <title>`。
- 章节：一级章节 `##`，每嵌套一层增加一个 `#`。
- topic：标题级别 = 所在章节级别 + 1，标题为 topic 的 `title`。
- text：普通段落，段落之间空一行。
- list：每项一行 `- <item>`。
- block：```` ```<language> ```` + 原文 + ```` ``` ````；`language` 为空时使用裸围栏。
- mermaid：```` ```mermaid ````。
- html：```` ```html ````。
- document 关联：在章节末尾输出一行 HTML 注释 `<!-- document: q1, q2 -->`；没有关联时省略。
- 评论不输出。

等价性：Markdown 包含 JSON 中所有标题、正文元素和 document 关联；章节和 topic 都用标题表示，因此不保证结构可逆。

## 5. show 输出

```bash
ds doc show <qname> [--format text|json|md]
ds show <name> [--format text|json|md]
```

- `text`：现有缩进视图，默认值。
- `json`：2 空格缩进的 JSON。
  - 文档对象输出第 2 节的结构，可回灌 `ds doc import`；
  - 其他对象输出 `{"qname","kind","source"?,"content_hash"?,"document"?,"children":[...]}` 及该 kind 的字段。
- `md`：Markdown 视图。
  - 文档对象按第 4 节渲染；
  - 其他对象输出 `# <qname>` 和字段列表。

## 6. 兼容性

- `version` 必须为 `1`；未知字段忽略。
- 未知 `type` 或 `op` 报错并停止。
- 新增字段不改变现有字段的语义。
