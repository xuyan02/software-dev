# Arche v2 设计

- 状态：设计定稿，实现中
- 日期：2026-11-23
- 工程：`projects/arche`
- 目标：在 v1 的功能基础上，重建对象模型、文档模型、需求空间和概念空间

## 1. 范围

v2 从头实现，不迁移 v1 数据。v1 继续保留，直到 v2 达到功能对等并通过验收，再整体替换 v1。

v2 覆盖：

- 技术设计对象及其源码锚定；
- 技术文档作为对象内容，而不是独立文档树；
- 需求空间；
- 三种概念空间；
- 独立评论；
- 对象仓库、提交、分支、暂存、worktree；
- 本地、HTTP、SSH 传输；
- 搜索、校验、CLI；
- Foundry 设计空间页面、合并请求和评审；
- 设计写作技能和智能体提示。

v2 明确不做：

- 不迁移 v1 仓库；
- 不做字段级暂存；
- 不做独立文档树；
- 不做 `InnerClass` / `InnerStruct` / `InnerEnum` / `InnerInterface` 对象；
- 不做表格、引用块、图片段落；
- 不做依赖证据和依赖理由字段；
- 不做文档锚点文件。

## 2. 核心决定

1. 所有设计对象都对应磁盘上的一个内容寻址文件。
2. 对象的 qname 和 name 都是 hash；落盘时不再回查符号表。
3. 文档是对象结构的投影，不是独立对象树。
4. 每个文档字段有固定目标类型，不允许字段自由指向任意文档对象。
5. `ObjectList` 是对象；`DocumentParagraphList` 是对象；列表都有顺序。
6. 评论不是对象的子对象；评论独立存在，通过 `target` 挂到目标对象。
7. 技术、需求、概念三个空间分开；名称前缀区分空间。
8. 技术对象使用 `Technical` 前缀；需求对象使用 `Requirement` 前缀；文档对象使用 `Document` 前缀；概念使用目录区分，不拆对象类型。
9. 需求空间使用显式对象树；交互是显式节点，UI 图是交互下的 HTML 内容，流程图是功能下的 Mermaid 内容。
10. 技术文档和需求文档都按字段类型校验，不再接受任意文档树。

## 3. 身份、名称和 qname

### 3.1 基本类型

```rust
pub struct Hash32([u8; 32]);
pub struct SymbolHash(Hash32);
pub struct ObjectHash(Hash32);
pub struct CommitHash(Hash32);
pub struct GitOid([u8; 32]);
```

- `Hash32`：SHA-256 原始 32 字节；
- `SymbolHash`：名称、路径、字段引用使用的符号 hash；
- `ObjectHash`：对象内容 hash；
- `CommitHash`：提交 hash；
- `GitOid`：源码 Git commit，零表示无。

可选 hash 一律使用全零 hash 表示 `None`。全零 hash 永远不是合法内容值。

### 3.2 qname

每个对象都有：

```rust
pub qname: SymbolHash,
pub name: SymbolHash,
```

- `name` 是对象在父容器或字段下的本地名字；
- `qname` 是完整限定名；
- 同一个 `name` 可以出现在不同父对象、不同目录、不同空间中；
- qname 全局唯一；
- qname 和 name 都存 hash，不存拼接后的字符串。

qname 的生成规则：

| 情况 | qname |
|---|---|
| 命名子对象 | `<parent-qname>/<name>` |
| 对象字段指向的对象 | `<owner-qname>/<field>` |
| 列表项 | `<list-qname>.<item-id>` |
| 方法 | `<parent-qname>/<name>#<signature-hash>` |
| Part | `<parent-qname>/part#<content-hash>` |

`.` 只用于列表项定位，不是通用 qname 分隔符。文件路径中的点号不受影响。

列表项顺序由 `items` 的顺序决定；`item-id` 是稳定 ID，不是序号。插入、删除、移动不重排已有 item-id。

示例：

```text
technical/.../class.Foo                     TechnicalClass
technical/.../class.Foo/responsibilities    DocumentList
technical/.../class.Foo/responsibilities.1  DocumentText
technical/.../class.Foo/usage               DocumentParagraphList
technical/.../class.Foo/usage.1             DocumentTopic
technical/.../class.Foo/usage.1/body        DocumentParagraphList
technical/.../class.Foo/usage.1/body.1      DocumentText
technical/.../class.Foo/design_points.1     DocumentTopic
```

### 3.3 根

```text
technical
requirements
concepts
```

- `technical`：技术空间根；
- `requirements`：需求空间根；
- `concepts`：概念空间根。

全局视图对象：

```text
view
```

`View` 不属于技术空间，不加 `Technical` 前缀。

## 4. 对象总表

### 4.1 技术对象

```text
Technical
TechnicalSystem
TechnicalModule
TechnicalLayer
TechnicalDirectory
TechnicalFile
TechnicalNamespace
TechnicalClass
TechnicalStruct
TechnicalEnum
TechnicalInterface
TechnicalMethod
TechnicalFunction
TechnicalMixin
TechnicalPart
```

### 4.2 需求对象

```text
Requirements
RequirementUser
RequirementStory
RequirementSystem
RequirementModule
RequirementFunction
RequirementInteraction
```

### 4.3 概念对象

```text
ConceptDirectory
Concept
```

概念维度由目录区分：

```text
concepts/domain/...
concepts/requirements/...
concepts/technical/...
```

### 4.4 共享结构对象

```text
ObjectList
View
Comment
```

### 4.5 文档对象

```text
DocumentParagraphList
DocumentText
DocumentList
DocumentTopic
DocumentCode
DocumentMermaid
DocumentHtml
```

`Paragraph` 是联合类型，不是对象：

```rust
pub enum Paragraph {
    Text(DocumentTextRef),
    List(DocumentListRef),
    Topic(DocumentTopicRef),
    Code(DocumentCodeRef),
    Mermaid(DocumentMermaidRef),
    Html(DocumentHtmlRef),
}
```

对象类型总数：技术 15 种、需求 7 种、概念 2 种、共享 3 种、文档 7 种，共 **34 种**；其中 `Comment` 是侧对象。排除 `Comment` 后，设计对象为 **33 种**。

## 5. 公共字段

### 5.1 源码对象公共字段

技术代码对象使用：

```rust
pub parent: Option<SymbolHash>,
pub namespace: Option<SymbolHash>,
pub file: Option<SymbolHash>,
pub module: Option<SymbolHash>,
pub source: Option<SpanSource>,
pub source_hash: Hash32,
pub explicit_dependencies: Vec<SymbolHash>,
pub implicit_dependencies: Vec<SymbolHash>,
```

- `parent` 用于内部类型、方法、Part 等；
- `namespace`、`file`、`module` 用于关联命名空间、文件和模块；
- 不再有 `Inner*` 类型，内部类型通过 `parent` 表达；
- `source` 是路径或行范围；
- `source_hash` 由源码字节计算，不能手填；
- 依赖只存目标 qname，不存证据和理由。

### 5.2 结构对象公共字段

```rust
pub name: SymbolHash,
pub members: Option<SymbolHash>,  // ObjectList
pub modules: Option<SymbolHash>,  // ObjectList
pub layers: Option<SymbolHash>,   // ObjectList
```

- `members`、`modules`、`layers` 都是 `ObjectList`；
- `ObjectList` 自己存 `owner`、`next_id` 和 `items`，不存字段名；
- 字段名由父对象 schema 决定，qname 为 `<owner>/<field>`。

### 5.3 ObjectList

```rust
pub struct ObjectList {
    pub qname: SymbolHash,
    pub owner: SymbolHash,
    pub next_id: u64,
    pub items: Vec<SymbolHash>,
}
```

- 列表有顺序；
- 每个 item 有 qname；
- item 可以是命名对象，也可以是自动编号对象；
- 列表本身是对象，有独立文件；
- `ObjectList` 不存字段名。

## 6. 技术对象

### 6.1 Technical

```rust
pub struct Technical {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub files: Option<SymbolHash>,       // ObjectList
    pub namespaces: Option<SymbolHash>,  // ObjectList
    pub systems: Option<SymbolHash>,     // ObjectList
}
```

### 6.2 View

```rust
pub struct View {
    pub qname: SymbolHash,
    pub source_commit: GitOid,
    pub technical: ObjectHash,
    pub requirements: ObjectHash,
    pub concepts: ObjectHash,
    pub comments: Option<ObjectHash>, // ObjectList<Comment>
}
```

`View` 绑定源码 commit 和三个空间根的对象 hash；评论通过独立评论列表引用，不属于任何目标对象。对象字段保存 qname，commit 另外保存 qname → object hash 快照，因此同一个 qname 的历史版本可以被准确定位。

### 6.3 TechnicalSystem / TechnicalModule / TechnicalLayer

```rust
pub struct TechnicalSystem {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,             // technical
    pub members: Option<SymbolHash>,    // ObjectList
    pub modules: Option<SymbolHash>,    // ObjectList
    pub layers: Option<SymbolHash>,     // ObjectList
    // 文档字段见 7.1
}

pub struct TechnicalModule {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,             // system/module/layer
    pub members: Option<SymbolHash>,
    pub modules: Option<SymbolHash>,
    pub layers: Option<SymbolHash>,
    // 文档字段见 7.1
}

pub struct TechnicalLayer {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,             // system/module
    pub order: u32,
    pub members: Option<SymbolHash>,
    pub position: Option<DocumentTextRef>,
}
```

- 层不能嵌套层；
- 只有相邻层可以依赖；
- 有层的容器，其代码成员和子模块必须归入层；
- 层顺序从 1 开始，同级唯一。

### 6.4 TechnicalDirectory / TechnicalFile / TechnicalNamespace

```rust
pub struct TechnicalDirectory {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub source: Option<PathSource>,
    pub children: Option<SymbolHash>, // ObjectList
    pub position: Option<DocumentTextRef>,
}

pub struct TechnicalFile {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub source: Option<PathSource>,
    pub source_hash: Hash32,
    pub children: Option<SymbolHash>, // ObjectList
    pub position: Option<DocumentTextRef>,
}

pub struct TechnicalNamespace {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,           // namespace
    pub source: Option<SpanSource>,
    pub source_hash: Hash32,
    pub children: Option<SymbolHash>, // ObjectList
    pub position: Option<DocumentTextRef>,
}
```

### 6.5 TechnicalClass

```rust
pub struct TechnicalClass {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: Option<SymbolHash>,
    pub namespace: Option<SymbolHash>,
    pub file: Option<SymbolHash>,
    pub module: Option<SymbolHash>,
    pub source: Option<SpanSource>,
    pub source_hash: Hash32,

    pub superclass: Option<SymbolHash>,
    pub interfaces: Vec<SymbolHash>,
    pub mixins: Vec<SymbolHash>,
    pub explicit_dependencies: Vec<SymbolHash>,
    pub implicit_dependencies: Vec<SymbolHash>,

    pub methods: Option<SymbolHash>,     // ObjectList
    pub inner_types: Option<SymbolHash>, // ObjectList
    pub parts: Option<SymbolHash>,       // ObjectList

    // 文档字段见 7.2
}
```

### 6.6 TechnicalStruct / TechnicalEnum

```rust
pub struct TechnicalStruct {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: Option<SymbolHash>,
    pub namespace: Option<SymbolHash>,
    pub file: Option<SymbolHash>,
    pub module: Option<SymbolHash>,
    pub source: Option<SpanSource>,
    pub source_hash: Hash32,
    pub explicit_dependencies: Vec<SymbolHash>,
    pub implicit_dependencies: Vec<SymbolHash>,
    pub methods: Option<SymbolHash>,
    pub inner_types: Option<SymbolHash>,
    pub parts: Option<SymbolHash>,
    // 文档字段见 7.2
}

pub struct TechnicalEnum {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: Option<SymbolHash>,
    pub namespace: Option<SymbolHash>,
    pub file: Option<SymbolHash>,
    pub module: Option<SymbolHash>,
    pub source: Option<SpanSource>,
    pub source_hash: Hash32,
    pub explicit_dependencies: Vec<SymbolHash>,
    pub implicit_dependencies: Vec<SymbolHash>,
    pub methods: Option<SymbolHash>,
    pub inner_types: Option<SymbolHash>,
    // 文档字段见 7.2
}
```

### 6.7 TechnicalInterface

```rust
pub struct TechnicalInterface {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: Option<SymbolHash>,
    pub namespace: Option<SymbolHash>,
    pub file: Option<SymbolHash>,
    pub module: Option<SymbolHash>,
    pub source: Option<SpanSource>,
    pub source_hash: Hash32,

    pub extends: Option<SymbolHash>,
    pub explicit_dependencies: Vec<SymbolHash>,
    pub implicit_dependencies: Vec<SymbolHash>,

    pub methods: Option<SymbolHash>,
    pub inner_types: Option<SymbolHash>,
    pub parts: Option<SymbolHash>,

    // 文档字段见 7.2
}
```

### 6.8 TechnicalMethod / TechnicalFunction

```rust
pub struct TechnicalMethod {
    pub qname: SymbolHash,       // parent/name#signature-hash
    pub name: SymbolHash,
    pub parent: SymbolHash,      // class/interface/part
    pub signature: SymbolHash,
    pub source: Option<SpanSource>,
    pub source_hash: Hash32,
    pub overrides: Option<SymbolHash>,
    pub explicit_dependencies: Vec<SymbolHash>,
    pub implicit_dependencies: Vec<SymbolHash>,
    // 文档字段见 7.2
}

pub struct TechnicalFunction {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub namespace: Option<SymbolHash>,
    pub file: Option<SymbolHash>,
    pub module: Option<SymbolHash>,
    pub source: Option<SpanSource>,
    pub source_hash: Hash32,
    pub explicit_dependencies: Vec<SymbolHash>,
    pub implicit_dependencies: Vec<SymbolHash>,
    // 文档字段见 7.2
}
```

方法签名不可编辑；改签名就是新方法。函数原型是文档字段，源码签名由 `source` 和 `signature` 关联。

### 6.9 TechnicalMixin

```rust
pub struct TechnicalMixin {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: Option<SymbolHash>,
    pub namespace: Option<SymbolHash>,
    pub file: Option<SymbolHash>,
    pub module: Option<SymbolHash>,
    pub source: Option<SpanSource>,
    pub source_hash: Hash32,

    pub on: Vec<SymbolHash>,
    pub superclass: Option<SymbolHash>,
    pub interfaces: Vec<SymbolHash>,
    pub mixins: Vec<SymbolHash>,
    pub explicit_dependencies: Vec<SymbolHash>,
    pub implicit_dependencies: Vec<SymbolHash>,

    pub methods: Option<SymbolHash>,
    pub inner_types: Option<SymbolHash>,
    pub parts: Option<SymbolHash>,

    // 文档字段见 7.2
}
```

### 6.10 TechnicalPart

```rust
pub struct TechnicalPart {
    pub qname: SymbolHash, // parent/part#content-hash
    pub name: SymbolHash,
    pub parent: SymbolHash, // class/interface/...
    pub source: SpanSource, // 必须有行范围
    pub source_hash: Hash32,
    pub explicit_dependencies: Vec<SymbolHash>,
    pub implicit_dependencies: Vec<SymbolHash>,
    pub children: Option<SymbolHash>, // ObjectList
}
```

Part 不关联设计文档，不建文档字段。

## 7. 技术文档字段

字段仍然是对象的字段，值是对文档对象的 qname 引用。所有引用按 schema 校验目标类型。

### 7.1 TechnicalSystem / TechnicalModule

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |
| `responsibilities` | `DocumentList<DocumentText>` |
| `non_responsibilities` | `DocumentList<DocumentText>` |
| `features` | `DocumentList<DocumentText>` |
| `non_features` | `DocumentList<DocumentText>` |
| `invariants` | `DocumentList<DocumentText>` |
| `constraints` | `DocumentList<DocumentText>` |
| `design_principles` | `DocumentList<DocumentText>` |
| `external_dependencies` | `DocumentList<DocumentText>` |
| `usage` | `DocumentParagraphList` |
| `design_points` | `DocumentParagraphList<DocumentTopic>` |

### 7.2 代码类型

TechnicalClass：

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |
| `responsibilities` | `DocumentList<DocumentText>` |
| `non_responsibilities` | `DocumentList<DocumentText>` |
| `features` | `DocumentList<DocumentText>` |
| `non_features` | `DocumentList<DocumentText>` |
| `invariants` | `DocumentList<DocumentText>` |
| `constraints` | `DocumentList<DocumentText>` |
| `design_principles` | `DocumentList<DocumentText>` |
| `usage` | `DocumentParagraphList` |
| `design_points` | `DocumentParagraphList<DocumentTopic>` |

TechnicalStruct：

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |
| `invariants` | `DocumentList<DocumentText>` |

TechnicalEnum：

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |

TechnicalInterface：

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |
| `responsibilities` | `DocumentList<DocumentText>` |
| `non_responsibilities` | `DocumentList<DocumentText>` |
| `features` | `DocumentList<DocumentText>` |
| `non_features` | `DocumentList<DocumentText>` |
| `invariants` | `DocumentList<DocumentText>` |
| `usage` | `DocumentParagraphList` |
| `design_points` | `DocumentParagraphList<DocumentTopic>` |

TechnicalMixin：

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |
| `responsibilities` | `DocumentList<DocumentText>` |
| `non_responsibilities` | `DocumentList<DocumentText>` |
| `features` | `DocumentList<DocumentText>` |
| `non_features` | `DocumentList<DocumentText>` |
| `invariants` | `DocumentList<DocumentText>` |
| `usage` | `DocumentParagraphList` |
| `design_points` | `DocumentParagraphList<DocumentTopic>` |

TechnicalMethod / TechnicalFunction：

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |
| `prototype` | `DocumentText` |
| `parameters` | `DocumentList<DocumentText>` |
| `returns` | `DocumentText` |
| `usage` | `DocumentParagraphList` |
| `design_points` | `DocumentParagraphList<DocumentTopic>` |

TechnicalLayer / TechnicalDirectory / TechnicalFile / TechnicalNamespace：

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |

不记录字段列表。Class、Struct、Enum 的字段信息不进入对象文档字段，也不建立 `field_list` 对象。

## 8. 文档内容模型

### 8.1 DocumentText

```rust
pub struct DocumentText {
    pub qname: SymbolHash,
    pub content: Vec<Inline>,
}
```

一行或多行行内内容，但语义上是一段。多段使用 `DocumentParagraphList`。

### 8.2 Inline

```rust
pub enum Inline {
    Text(String),
    Code(String),
    Strong(Vec<Inline>),
    Emphasis(Vec<Inline>),
    Link {
        text: Vec<Inline>,
        href: String,
    },
    ConceptRef {
        qname: SymbolHash,
        label: Vec<Inline>,
    },
    Break,
}
```

概念引用的 qname 自带目录，因此不需要三种引用变体。

### 8.3 DocumentList

```rust
pub struct DocumentList {
    pub qname: SymbolHash,
    pub next_id: u64,
    pub ordered: bool,
    pub items: Vec<Paragraph>,
}
```

列表项是 `Paragraph`，可以是 `DocumentText`、嵌套 `DocumentList`、`DocumentTopic`、代码、Mermaid 或 HTML。

### 8.4 DocumentTopic

```rust
pub struct DocumentTopic {
    pub qname: SymbolHash,
    pub title: Vec<Inline>,
    pub body: SymbolHash, // DocumentParagraphList
}
```

### 8.5 DocumentParagraphList

```rust
pub struct DocumentParagraphList {
    pub qname: SymbolHash,
    pub next_id: u64,
    pub items: Vec<Paragraph>,
}
```

用于字段的段落序列、Topic 正文和用法示例正文。

### 8.6 DocumentCode / DocumentMermaid / DocumentHtml

```rust
pub struct DocumentCode {
    pub qname: SymbolHash,
    pub language: String,
    pub source: String,
}

pub struct DocumentMermaid {
    pub qname: SymbolHash,
    pub source: String,
}

pub struct DocumentHtml {
    pub qname: SymbolHash,
    pub source: String,
}
```

- Mermaid 只存源码，不存渲染结果；
- HTML 只存源码，渲染时使用无脚本沙箱 iframe，默认高度 360px，可滚动；
- HTML 禁止脚本、表单、顶层跳转和外部资源；图片使用 data URI；
- 流程图只允许 Mermaid，不再支持其他图表类型。

### 8.7 字段 schema

字段目标类型：

```text
DocumentText
DocumentList<DocumentText>
DocumentParagraphList
DocumentParagraphList<DocumentTopic>
DocumentParagraphList<DocumentHtml>
DocumentMermaid
ObjectList
```

- `DocumentList<DocumentText>`：列表项必须是 `DocumentText`；
- `DocumentParagraphList<DocumentTopic>`：段落项必须是 `DocumentTopic`；
- `DocumentParagraphList<DocumentHtml>`：UI 图段落必须是 `DocumentHtml`；
- `ObjectList`：设计对象的结构列表，不是文档内容。

### 8.8 文档投影

技术对象的文档按以下顺序投影：

1. 对象名称；
2. schema 中声明的字段顺序；
3. 字段内容按 `DocumentParagraphList`、`DocumentList`、`DocumentTopic` 的顺序渲染；
4. 结构关系投影为只读段落：
   - 父类 / 父接口 -> `DocumentText`
   - 接口列表 -> `DocumentList<DocumentText>`
   - 混入列表 -> `DocumentList<DocumentText>`
   - 结构依赖 -> `DocumentList<DocumentText>`
5. 评论从独立评论索引加载，不进入对象内容。

Markdown 和 JSON 都是投影，不是存储格式。

### 8.9 JSON 投影

```json
{
  "qname": "technical/.../class.Foo",
  "kind": "TechnicalClass",
  "name": "Foo",
  "fields": {
    "position": {
      "type": "DocumentText",
      "content": [{"type": "text", "text": "认证服务。"}]
    },
    "responsibilities": {
      "type": "DocumentList",
      "ordered": false,
      "items": [
        {"type": "text", "content": [{"type": "text", "text": "负责令牌签发。"}]}
      ]
    },
    "design_points": {
      "type": "DocumentParagraphList",
      "items": [
        {
          "type": "topic",
          "title": [{"type": "text", "text": "状态"}],
          "body": []
        }
      ]
    }
  }
}
```

导入时按字段路径和 schema 校验；不能把一个 `DocumentList` 写进 `position`，也不能把一个 `DocumentText` 写进 `usage`。

### 8.10 Markdown 投影

- 对象标题：`# <name>`；
- 字段标题：`## <字段名>`；
- Topic：标题级别递增；
- Text：普通段落；
- List：`- item` 或 `1. item`；
- Code：围栏代码块，保留 language；
- Mermaid：` ```mermaid `；
- Html：` ```html ` 或直接沙箱预览；
- 评论不输出。

## 9. 需求空间

### 9.1 层级

```text
Requirements
├─ users: ObjectList<RequirementUser>
├─ stories: ObjectList<RequirementStory>
└─ systems: ObjectList<RequirementSystem>
   └─ modules: ObjectList<RequirementModule>
      └─ functions: ObjectList<RequirementFunction>
         └─ interactions: ObjectList<RequirementInteraction>
```

```rust
pub struct Requirements {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub users: Option<SymbolHash>,   // ObjectList<RequirementUser>
    pub stories: Option<SymbolHash>, // ObjectList<RequirementStory>
    pub systems: Option<SymbolHash>, // ObjectList<RequirementSystem>
}
```

需求系统和技术系统是不同对象；需求模块和技术模块是不同对象；需求功能和技术功能是不同对象。

### 9.2 RequirementUser

对象名称是用户姓名。

| 字段 | 目标类型 |
|---|---|
| `age` | `DocumentText` |
| `basic_situation` | `DocumentText` |
| `hobbies` | `DocumentText` |
| `occupation` | `DocumentText` |
| `typical_scenario` | `DocumentParagraphList` |
| `needs` | `DocumentList<DocumentText>` |
| `priority` | `DocumentText` |

### 9.3 RequirementStory

对象名称是故事名称。

| 字段 | 目标类型 |
|---|---|
| `preconditions` | `DocumentParagraphList` |
| `frequency` | `DocumentText` |
| `description` | `DocumentParagraphList` |
| `related_requirements` | `DocumentList<DocumentText>` |

### 9.4 RequirementSystem

对象名称是软件名称。

| 字段 | 目标类型 |
|---|---|
| `form` | `DocumentText` |
| `covered_users` | `DocumentList<DocumentText>` |
| `closed_loop_goal` | `DocumentParagraphList` |
| `relationships` | `DocumentParagraphList` |
| `boundary` | `DocumentList<DocumentText>` |

结构字段：

```rust
pub modules: Option<SymbolHash>, // ObjectList<RequirementModule>
```

### 9.5 RequirementModule

对象名称是模块名称。

| 字段 | 目标类型 |
|---|---|
| `core_concept` | `DocumentText` |
| `included_concepts` | `DocumentList<DocumentText>` |
| `concept_relations` | `DocumentParagraphList` |
| `boundary` | `DocumentList<DocumentText>` |

`core_concept` 和 `included_concepts` 中的概念引用必须指向 `concepts/requirements/` 下的概念。

结构字段：

```rust
pub functions: Option<SymbolHash>, // ObjectList<RequirementFunction>
```

### 9.6 RequirementFunction

对象名称是功能名称。

| 字段 | 目标类型 |
|---|---|
| `initiator` | `DocumentText` |
| `system_change` | `DocumentParagraphList` |
| `preconditions` | `DocumentParagraphList` |
| `feedback` | `DocumentText` |
| `description` | `DocumentParagraphList` |
| `flow` | `DocumentMermaid` |
| `interactions` | `ObjectList<RequirementInteraction>` |

- `flow` 是一个 Mermaid 对象，qname 为 `<function>/flow`；
- 交互是显式对象，不从 Mermaid 解析；
- 交互列表顺序就是流程图顺序。

### 9.7 RequirementInteraction

对象名称是交互名称。

| 字段 | 目标类型 |
|---|---|
| `description` | `DocumentParagraphList` |
| `ui` | `DocumentParagraphList<DocumentHtml>` |

UI 图直接挂在交互下：

```text
<interaction>/ui
<interaction>/ui.1    DocumentHtml
<interaction>/ui.2    DocumentHtml
```

不建 `RequirementUi` 对象。

## 10. 概念空间

### 10.1 目录

```text
concepts/domain/...
concepts/requirements/...
concepts/technical/...
```

三个顶层目录固定：

- `domain`：现实中存在、被公认的事物；
- `requirements`：为了描述产品需求而发明的概念；
- `technical`：为了实现而发明的概念。

```rust
pub struct ConceptDirectory {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: Option<SymbolHash>,
    pub children: Option<SymbolHash>, // ObjectList
}
```

`concepts` 根本身也是一个 `ConceptDirectory`；`domain`、`requirements`、`technical` 是它下面三个固定目录。

同一个概念名可以在三个目录中同时存在：

```text
concepts/domain/agent
concepts/requirements/agent
concepts/technical/agent
```

它们是三个独立对象，不是别名或继承关系。

### 10.2 Concept

```rust
pub struct Concept {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub summary: Option<DocumentTextRef>,
    pub explanation: Option<DocumentParagraphListRef>,
    pub characteristics: Option<DocumentListRef>,
    pub relations: Option<DocumentListRef>,
    pub aliases: Option<DocumentListRef>,
    pub links: Vec<ConceptLink>,
}
```

- 概念维度由 qname 目录决定；
- 同一目录内名称受控；不同目录允许同名；
- `relations` 用文字说明差异和联系；
- `links` 用结构链接表达跨维度关系。

### 10.3 ConceptLink

```rust
pub struct ConceptLink {
    pub target: SymbolHash,
    pub relation: ConceptRelation,
}

pub enum ConceptRelation {
    Refines,
    Realizes,
    Corresponds,
}
```

推荐方向：

```text
Requirement concept -> Domain concept      Refines
Technical concept   -> Requirement concept Realizes
```

反向关系由索引推导，不存储重复反向链接。

## 11. 评论

### 11.1 Comment

```rust
pub struct Comment {
    pub qname: SymbolHash,
    pub target: SymbolHash,
    pub target_hash: Option<Hash32>,
    pub body: Vec<Inline>,
    pub reply_to: Option<SymbolHash>,
    pub resolved: bool,
    pub author: String,
    pub created_at: i64,
    pub updated_at: i64,
}
```

### 11.2 规则

- 评论不是目标对象的子对象；
- 目标对象不存评论字段；
- 修改评论不改变目标对象的 content hash；
- 评论通过 `target` 挂在任意对象上；
- 评论列表由 `View.comments` 指向的 `ObjectList<Comment>` 维护；
- 评论 qname 为 `comments.<comment-id>`，它只表示评论在评论列表中的位置，不表示目标对象的子对象；
- 评论索引可以按 target 重建；
- 设计 diff 默认不比较评论；评论使用独立命令查看和比较；
- 目标移动、重命名或删除后，评论保留为孤儿，`arche comment check` 报告。

可评论目标示例：

```text
TechnicalClass
TechnicalClass/position
TechnicalClass/responsibilities.1
TechnicalClass/usage.1
RequirementInteraction/ui.1
```

## 12. 存储格式

### 12.1 布局

```text
<workspace>/.arche/
  config
  HEAD
  source-head
  index
  worktree
  refs/
    heads/<branch>
    remotes/origin/<branch>
  source/
  repo.toml
  objects/<aa>/<bb>/<object-hash>
  symbols/<aa>/<bb>/<symbol-hash>
  commits/<aa>/<bb>/<commit-hash>
  packs/<pack-sha256>.pack
  incoming/                 # 服务端 quarantine
  tmp/
```

- 不设 `docs/`：文档内容全部是对象；
- `objects/` 存 DSO2 记录；
- `symbols/` 存纯文本符号；
- `commits/` 存 CMT2 提交；
- `packs/` 存 PAK2 pack；
- `tmp/` 只用于临时写入和原子 rename；
- `incoming/` 只用于服务端上传隔离。

### 12.2 DSO2

- 所有对象使用 canonical 二进制编码；
- 小端整数；
- hash 固定 32 字节；
- 可选 hash 全零表示 None；
- 集合排序、去重；
- 列表使用 `u32 count`；
- 文本使用 `u32 byte_len || UTF-8`；
- 不存可推导字段；
- 对象文件包含类型 tag；
- qname、name 都是 `SymbolHash`。

### 12.3 CMT2

提交包含：

- 父提交；
- View 对象 hash；
- 作者、时间、消息；
- qname → object hash 快照，覆盖该提交可见的全部对象。

对象字段保存 qname，因此需要快照把 qname 解析到该提交下的具体 object hash。快照是提交的一部分，不写入目标对象；同一 qname 的历史版本由不同提交的快照区分。

评论列表通过 View 引用，但修改评论不会修改技术、需求、概念对象。

### 12.4 Symbols

符号文件保存 qname 路径、名称、字段名等文本，用于显示和调试。对象落盘不依赖符号查找。

### 12.5 Pack

PAK2 包含对象、符号和提交记录。读取时优先 loose file，找不到时回退 pack。

### 12.6 原子写入

所有 canonical 文件先写入 `.arche/tmp/`，校验后 rename 到最终路径。跨文件系统不允许 rename；`.` 内 tmp 与最终目录必须同一文件系统。

### 12.7 refs

- 分支 ref 指向 CMT2；
- ref 更新使用 CAS，比较 expected-old；
- 非 force 要求 fast-forward；
- push 在 ref 锁内更新；
- refs.transaction journal 记录多 ref 事务；
- `flock` 保护 ref 临界区；
- GC/repack 使用同一 ref 锁。

### 12.8 GC / repack

- GC 从 refs 和 View 出发计算可达对象；
- 删除不可达对象、符号、提交；
- 评论通过 View.comments 可达；
- repack 把 loose records 收进 pack；
- 未引用但已校验的上传记录由 GC 回收。

## 13. 工作区、暂存和源码

### 13.1 工作区

- `.arche/config` 保存 remote；
- `.arche/repo.toml` 保存仓库元数据；
- `.arche/index` 保存暂存对象；
- `.arche/worktree` 保存工作区对象图；
- `.arche/HEAD` 指向当前分支/提交；
- `.arche/source-head` 指向源码 commit。

### 13.2 暂存

与 v1 对齐：

- 对象级暂存，不做字段级暂存；
- `add` 对象时补齐父容器和必要引用；
- `status` 显示未暂存和已暂存 qname；
- `diff` 显示对象级和字段级差异；
- `reset` 只恢复 index；
- `reset --hard` 恢复 index 和 worktree；
- `commit` 只使用 index；
- 空提交需要 `--allow-empty`；
- checkout 有未提交修改时拒绝，`--force` 丢弃。

### 13.3 源码 worktree

- `.arche/source` 是 bare Git clone；
- 根目录 `.git` 是指向 `.arche/source/worktrees/root` 的文件；
- `.arche/` 通过 `.arche/source/info/exclude` 隐藏；
- hooks、submodule 自动执行禁用；
- `source fetch` 更新 bare clone 并创建/复用 worktree；
- `source checkout` 更新 `source-head`；
- `source sync` 对齐 File/Directory 对象与源码树；
- `source changes` 输出 A/M/D/R；
- `source ls/cat/log/status` 保留；
- 源码仓库包含 `.arche` 时拒绝根 worktree。

### 13.4 source 语法

```text
--source path
--source path:start-end
```

- Directory/File 只接受 path；
- Namespace/Class/Struct/Enum/Interface/Method/Function/Mixin 接受 path 或行范围；
- Part 必须 path:start-end；
- source 字节计算 source_hash；
- 无 source 时 source_hash 零；有 source 时非零。

## 14. 远程与传输

### 14.1 URL

| URL | 传输 |
|---|---|
| `/path` / `file:///path` | 本地目录 |
| `http://host:port/<repo>` | `arche-server` HTTP |
| `https://host[:port]/<repo>` | 同上，TLS |
| `ssh://[user@]host[:port]/<path>` | SSH 上的 `arche-server --stdio` |

### 14.2 arche-server

```bash
arche-server --root <repos-root>
arche-server --root <repos-root> --listen 0.0.0.0:7700
arche-server --root <repos-root> --token-file <token> --admin-token-file <admin>
arche-server --stdio --root <repo-dir>
```

HTTP API 保留：

```text
GET  /repos
POST /repos
GET  /<repo>/refs
PUT  /<repo>/refs
GET  /<repo>/files
GET  /<repo>/files/<relative>
PUT  /<repo>/files/<relative>
POST /<repo>/push
```

### 14.3 PUSH2

- 一次上传缺失对象、符号、提交和 ref 命令；
- bundle 先进入 `incoming/` quarantine；
- 校验 hash、路径、对象闭包、ref expected-old；
- 在 `flock` 保护的 ref 临界区发布并原子更新 ref；
- 失败不改变可见分支；
- 非 force 要求 fast-forward；
- 管理员可移动受保护分支；
- `repo.toml` 不进入 push。

### 14.4 认证

- HTTP：Bearer token；控制平面使用 admin token；智能体不使用 bearer token；
- SSH：使用宿主 sshd 和服务账户 `foundry-arche`；
- 智能体通过 `AuthorizedKeysCommand` 读取授权 key；
- forced command：

```text
/usr/local/bin/foundry-arche-stdio <allowlist> <repos-root> <arche-server>
```

- 只允许指定仓库；
- 仓库 ACL 使用 POSIX ACL 和 `umask 0002`；
- 客户端使用 host known_hosts；
- `.arche/config` 在工作区创建/刷新时从凭证 URL 更新，避免旧 remote 残留。

### 14.5 repo.toml

- 服务端建库时写入；
- 客户端 clone/fetch 读取；
- 客户端不能修改；
- 包含源码 URL 和 object format。

## 15. 搜索与查询

保留 v1 `search`，扩展到新对象：

```bash
arche search [QUERY]
  [--kind <kind>[,<kind>...]]
  [--namespace <qname>]
  [--file <qname>]
  [--module <qname>]
  [--requirement <qname>]
  [--concept <qname>]
  [--has-design]
  [--text <text>]
  [--format text|json]
  [--ignore-case]
  [--staged]
```

- `QUERY` 匹配 qname、name、源码路径；
- `--kind` 支持技术、需求、概念和文档对象类型；
- `--namespace` 匹配命名空间子树；
- `--file` 精确匹配 File；
- `--module` 匹配技术模块或需求模块子树；
- `--requirement` 匹配需求对象子树；
- `--concept` 匹配概念目录子树；
- `--has-design` 表示至少一个文档字段非空；
- `--text` 搜索文档内容；
- `--kind` 可重复或逗号分隔；
- JSON 输出包含 qname、kind、name、source、parent、namespace、file、module、fields、concepts。

`arche ls` 保留：

```bash
arche ls
arche ls --technical
arche ls --requirements
arche ls --concepts
arche ls --documents
arche ls --design
```

文档搜索不再使用 `documents` 树；`arche doc grep` 保留为 `arche search --text` 的便捷命令。

## 16. 校验

`arche check` 统一检查：

- qname 唯一；
- 父对象类型正确；
- 文件、命名空间、模块引用存在；
- 方法签名不可变；
- Part 必须有行范围；
- source 和 source_hash 一致；
- 依赖目标存在，依赖允许循环；
- 继承、接口、混入、extends、on 不能成环；
- 结构关系自动计入显式依赖；
- 字段目标类型符合 schema；
- `DocumentList` 项类型正确；
- `DocumentTopic.body` 必须是 `DocumentParagraphList`；
- 概念引用能解析；
- 概念链接目标存在，方向合法；
- 需求层级完整；
- 交互是显式对象；
- 流程图是 Mermaid；
- 评论 target 存在或显式标记孤儿；
- 文档 HTML/Mermaid 格式合法。

`arche doc lint` 保留参数、返回值、字段格式等规则检查。

## 17. CLI

v2 保留 v1 的命令组，替换文档命令的内部模型。

### 17.1 仓库和分支

```bash
arche init
arche clone
arche push
arche fetch
arche pull
arche remote add/show/set-url
arche checkout
arche checkout -b
arche branch delete
arche log
arche show
```

### 17.2 对象

```bash
arche create <kind> <name> [options]
arche edit <qname> [options]
arche rm <qname> [--force] [--recursive]
arche dep add/rm/list
arche show <qname> [--source] [--staged] [--format text|json|md]
arche ls [...]
arche search ...
```

`kind` 使用 v2 类型名，例如：

```text
technical-system
technical-module
technical-class
technical-method
requirement-system
requirement-function
concept
```

### 17.3 文档内容

```bash
arche doc text add <target> --text <text>
arche doc text set <qname> --text <text>
arche doc list add <target>
arche doc list item add <target> --text <text>
arche doc topic add <target> --title <text>
arche doc paragraph add <target>
arche doc code add/set <target/qname> [--language <lang>] --text <text>
arche doc mermaid add/set <target/qname> --text <text>
arche doc html add/set <target/qname> --text <text>
arche doc show <qname> [--format text|json|md]
arche doc import --file <path>
arche doc grep <pattern>
arche doc lint
arche doc layout
arche doc check
arche batch --file <path>
```

参数和返回值仍使用 `DocumentList<DocumentText>`、`DocumentText`。

### 17.4 结构

保留 v1 结构命令作为便捷包装：

```bash
arche structure init
arche structure system create
arche structure module create
arche structure layer create
arche structure layer set/rm
arche structure assign/unassign
arche structure list/show/check/unassigned
```

内部写入 `TechnicalSystem`、`TechnicalModule`、`TechnicalLayer`、`ObjectList`。

### 17.5 需求

```bash
arche requirement init
arche requirement user create
arche requirement story create
arche requirement system create
arche requirement module create
arche requirement function create
arche requirement interaction create
arche requirement show
arche requirement check
```

需求和技术的对象不同，命令也分开。

### 17.6 概念

```bash
arche concept mkdir concepts/domain/<dir>
arche concept create concepts/domain/<dir>/<name> --name <term> --summary <text>
arche concept create concepts/requirements/<dir>/<name> ...
arche concept create concepts/technical/<dir>/<name> ...
arche concept set/show/ls/mv/rm
arche concept feature add/rm
arche concept relation add/rm
arche concept alias add/rm
arche concept link add/rm
arche concept check
```

概念链接命令写入 `ConceptLink`。

### 17.7 评论

```bash
arche comment add <target> --text <text>
arche comment resolve <qname>
arche comment list [<target>] [--open] [--format text|json]
arche comment clear
arche comment check
```

### 17.8 暂存和源码

```bash
arche add <qname>...
arche add -A
arche add --file <path>
arche add --directory <dir>
arche unstage ...
arche reset
arche reset --hard
arche status [--porcelain] [--detail]
arche diff
arche diff --staged
arche verify
arche commit -m <message> [--allow-empty]
arche source fetch/checkout/worktree/status/ls/cat/log/sync/changes
arche gc
arche repack
```

## 18. v1 功能覆盖对照

| v1 功能 | v2 对应 |
|---|---|
| 设计对象创建/编辑/删除 | `arche create/edit/rm`，类型名带空间前缀 |
| 源码锚定 | `source`、`source_hash`，Part 强制行范围 |
| 依赖 | `explicit_dependencies` / `implicit_dependencies`，只存 qname |
| 内部类型 | 通过 `parent` 表达，不建 `Inner*` |
| 设计结构 System/Module/Layer | `TechnicalSystem/Module/Layer` + `ObjectList` |
| 独立文档树 | 删除；文档字段投影 |
| 文档 text/list/topic/block/mermaid/html | `DocumentText/List/Topic/Code/Mermaid/Html` |
| 文档评论 | 独立 `Comment`，target 挂接 |
| 概念空间 | 三个目录 + 一个 `Concept` 类型 + links |
| 需求锚点文档 | 需求对象树 |
| `doc import` / JSON / Markdown | 对象字段投影的 JSON / Markdown |
| `doc grep/lint/check` | `arche search --text` / `arche doc lint` / `arche check` |
| `doc layout` | schema 字段顺序和投影顺序 |
| `batch` 计划 | `arche batch --file`，格式升级到 v2 |
| `doc comment` | `arche comment` |
| 暂存/add/reset/diff/status/commit | 保留 |
| `add --file/--directory` | 保留 |
| `status --porcelain` | 保留 |
| `source fetch/checkout/worktree/sync/changes` | 保留 |
| checkout/branch delete | 保留 |
| `show/log/ls/search` | 保留并扩展 |
| `gc/repack` | 保留，PAK2 |
| 本地/HTTP/SSH remote | 保留 |
| HTTP token / SSH key | 保留 |
| 宿主 sshd、强制命令、ACL | 保留 |
| PUSH1 bundle | 升级 PUSH2 |
| ref CAS、fast-forward、flock | 保留 |
| MR 文档差异、对象差异 | 保留，按字段/对象投影 |
| MR 懒加载、折叠 | 保留 |
| 设计空间树类型标记 | 保留并扩展 |
| 长文档按需加载 | 保留 |
| 设计写作技能 | 更新 v2 字段、前缀、概念 |
| enum/class 分类和模块分治规则 | 保留并更新 |

## 19. Foundry 集成和界面

### 19.1 设计空间页面

- 技术树：System/Module/Layer、File/Namespace、Class/Struct/Enum/Interface/Method/Function/Mixin/Part；
- 需求树：Requirements/User/Story/System/Module/Function/Interaction；
- 概念树：`concepts/domain`、`concepts/requirements`、`concepts/technical`；
- 每项显示类型徽标，不依赖语言标记推断类型；
- 长文档只加载字段/章节大纲，展开后按需获取内容；
- Topic 和字段可折叠；
- HTML 使用沙箱 iframe；Mermaid 使用内置渲染；
- 评论侧栏按 target 加载，不嵌入对象。

### 19.2 合并请求

- MR 保存源/目标分支、创建时 commit、合并结果；
- 审批状态从 Task 派生；
- 页面初始只加载 MR 摘要，不计算完整 diff；
- 展开 changes 时再请求 `/changes`；
- 差异按对象 qname 和字段分组；
- 文档差异按字段投影生成 Markdown 行 diff；
- 需求/技术/概念对象分别展示；
- 评论不进入设计 diff，评论活动独立展示；
- 支持快进合并和分支删除。

### 19.3 API

保留：

```text
GET  /api/v1/projects/{project}/merge-requests
POST /api/v1/projects/{project}/merge-requests
GET  /api/v1/projects/{project}/merge-requests/{id}
POST /api/v1/projects/{project}/merge-requests/{id}/merge
GET  /api/v1/projects/{project}/merge-requests/{id}/changes
POST /api/v1/projects/{project}/arche/branches/delete
```

新增或调整：

```text
GET  /api/v1/projects/{project}/arche/search
GET  /api/v1/projects/{project}/arche/elements/{hash}
GET  /api/v1/projects/{project}/arche/elements/{hash}/section
GET  /api/v1/projects/{project}/arche/requirements
GET  /api/v1/projects/{project}/arche/concepts
```

### 19.4 评审

- MR 详情使用 `review` 读取元数据、Task、状态，不计算 diff；
- changes 端点负责对象/字段/文档差异；
- MR 页面使用懒加载和折叠；
- 文档字段按需加载；
- 对象类型徽标和字段标签由 schema 生成。

## 20. 技能和智能体

### 20.1 design-writing

更新：

- 技术对象类型前缀 `Technical`；
- 文档对象前缀 `Document`；
- 字段目标类型表；
- 不再有 `field_list`；
- Enum 只包含常量变体；数据携带 enum 按 Class；
- Module 用于复杂度分治，拆出内聚子模块；
- 父类、接口、混入由结构字段投影；
- 使用 `DocumentText/List/Topic/Code/Mermaid/Html` 描述内容。

### 20.2 arche

更新：

- `arche search` 新过滤器和 JSON；
- 技术/需求/概念三个空间；
- 需求对象命令；
- 概念目录和链接；
- 独立评论；
- 文档字段路径。

### 20.3 需求和概念技能

- 需求分析、交互设计、UI 设计技能改为写入需求对象；
- 交互设计使用 `RequirementFunction.flow` Mermaid 和 `RequirementInteraction.ui` HTML；
- 概念写作技能区分 `concepts/domain`、`concepts/requirements`、`concepts/technical`；
- 同名概念必须建立 link 并写清差异。

### 20.4 agent runtime

- design baseline 和 codesummary 使用 v2 类型；
- 枚举/类分类规则更新；
- 搜索使用 `arche search`，不逐对象调用 `arche show`；
- 设计写入技能和测试同步更新。

## 21. 实现阶段

1. **对象和存储内核**：DSO2/CMT2、对象仓库、符号、View、refs、pack。
2. **技术对象和文档字段**：schema、字段校验、投影、CLI 基础。
3. **暂存和源码**：index/worktree、source、diff、commit。
4. **远程**：PUSH2、HTTP/SSH、host sshd、ACL。
5. **需求空间**：需求对象、交互、UI、层级校验。
6. **概念空间**：三个目录、Concept、links、引用检查。
7. **评论**：独立 Comment、target 索引、孤儿检查。
8. **搜索和校验**：search、check、lint、JSON/Markdown 投影。
9. **Foundry UI/MR**：树、懒加载、字段投影、changes、评论。
10. **技能和智能体**：设计写作、需求、概念、命令提示。
11. **v1 功能对等验收**：逐项核对第 18 节。
12. **替换 v1**：新仓库使用 v2；v1 停止写入。

## 22. 验收

### 22.1 对象模型

- 所有对象都有 qname、name、文件和类型；
- qname 由父对象/字段/列表项规则生成；
- 所有列表有顺序；
- `ObjectList` 和 `DocumentParagraphList` 是对象；
- 同一名称可在不同父对象、概念目录和空间中出现；
- 不存在 `Inner*` 对象类型；
- 不存在 `field_list`；
- 不存在 Table/Quote/Image 文档对象。

### 22.2 技术文档

- 每个字段目标类型符合 schema；
- 父类、接口、混入、依赖由结构字段投影；
- 文档变更不改变结构引用；
- 技术对象可完整渲染文档；
- MR 文档差异按字段生成。

### 22.3 需求

- 需求系统/模块/功能/交互是显式对象；
- 交互挂在功能下；
- 流程图是 Mermaid；
- UI 图是交互下的 Html；
- 需求节点可完整文档化；
- 需求概念引用 `concepts/requirements`。

### 22.4 概念

- 三个顶层目录存在；
- 同一名称可在三个目录出现；
- 概念有 qname；
- 概念链接方向正确；
- `relations` 说明差异；
- `arche concept check` 通过。

### 22.5 评论

- 评论修改不改变目标对象 hash；
- 评论可按 target 查询；
- 设计 diff 不包含评论；
- 孤儿评论可报告。

### 22.6 v1 对等

- 第 18 节全部通过；
- 远端、SSH、worktree、push、GC、repack、搜索、UI、MR、技能均有测试；
- 现有 Foundry 项目可创建 v2 设计空间；
- 智能体可用 v2 完成一轮技术设计、需求设计和概念维护。

## 23. 当前实现与剩余工作

本节记录截至 2026-11-23 的实际实现状态。它不改变前面的目标设计，只说明哪些已经落地、哪些仍缺。

### 23.1 已实现

- `DSO2` 对象记录、`CMT2` 提交记录、SHA-256 内容寻址；
- `.arche/` 对象、符号、提交、快照和临时目录；
- worktree/index、`add`、`unstage`、`status`、`diff`、`commit`、`reset`；
- 分支列表、创建、删除和 `checkout`；
- `TechnicalSystem`、`TechnicalModule`、`TechnicalDirectory`、`TechnicalFile`；
- `DocumentText`、`DocumentList`、`DocumentParagraphList`、`DocumentTopic`、`DocumentMermaid`、`DocumentHtml`；
- 技术、需求和概念的文档字段写入与类型校验；
- 需求用户、故事、系统、模块、功能、交互对象；
- `RequirementFunction.flow` 和 `RequirementInteraction.ui`；
- 独立 `Comment`、按 target 查询、解决和 check；
- 三个概念目录、同名概念、概念链接；
- `search`、`check`、`show --format text|json|md`；
- `source fetch`、`source checkout`、`source worktree`、`source status`、`source sync`（按 tracked file 建 File 对象）；
- `add --file`（当前只级联 File、files 列表和 technical root）；
- 本地路径远程、HTTP 远程、SSH stdio 远程；
- `arche-server` HTTP 和 `--stdio`；
- HTTP Bearer token 客户端读取和服务端校验。

### 23.2 剩余工作

#### A. 完整源码对象模型与同步

当前 `source sync` 只创建 `TechnicalFile`，没有完整建立代码对象树。

还需要：

- `TechnicalDirectory` 树和父目录 `ObjectList`；
- `TechnicalNamespace`；
- `TechnicalClass`、`TechnicalStruct`、`TechnicalEnum`；
- `TechnicalInterface`、`TechnicalMixin`；
- `TechnicalMethod`、`TechnicalFunction`；
- `TechnicalPart`；
- 源码锚定 `SpanSource`、`source_hash` 和 `content_hash`；
- 继承、接口、混入、`on`、`overrides`、依赖关系；
- `source sync` 删除、重命名和移动对象时的 qname 迁移；
- `add --file` 级联 Directory/Namespace/Class/Method 等对象，而不是只暂存 File 和 files 列表。

验收：

- 一个真实源码仓库 sync 后能在技术空间重建 Directory/File/Namespace/Class/Method 树；
- 重命名文件和类型时，qname、引用和文档字段正确迁移；
- `arche check` 能校验继承、接口、混入、override 和依赖。

#### B. 远程协议硬化

当前远程是文件级传输，不是设计文档中的完整 PUSH2。

还需要：

- `PUSH2` bundle；
- `incoming/` quarantine；
- 对象 hash、路径和闭包校验；
- ref `expected-old` CAS；
- 非 force push 的 fast-forward；
- `flock` 保护的 ref 临界区；
- `refs.transaction` 多 ref 事务；
- 受保护分支和 admin token；
- `gc`、`repack`、pack 传输；
- HTTP 服务端逐文件协议升级为 bundle 协议；
- SSH stdio 协议升级为 bundle 协议。

验收：

- 并发 push 不丢失 ref；
- 失败 push 不改变任何可见分支；
- 非 fast-forward push 被拒绝；
- 未引用但已上传记录可被 GC；
- pack 能完整同步对象、符号和提交。

#### C. SSH 部署

当前已有 `arche-server --stdio`，但没有生产部署。

还需要：

- 宿主 sshd 配置；
- `foundry-ds` 或 `arche` 服务账户；
- `AuthorizedKeysCommand`；
- forced command；
- allowlist；
- 仓库 ACL；
- host known_hosts；
- `arche` 服务安装脚本；
- 工作区 `.arche/config` 凭证 URL 刷新。

验收：

- 新胶囊只能访问允许的仓库；
- 不支持的命令被拒绝；
- push/fetch 使用宿主 sshd 和服务账户完成；
- 工作区不会保留旧 remote URL。

#### D. 文档和 CLI 完整性

还需要：

- `doc code add/set`；
- `doc import` 和 batch envelope；
- `doc grep`、`doc lint`、`doc layout`；
- `show --format json` 输出完整对象字段和文档内容，而不只是字段引用；
- `arche log` 展示提交快照；
- `arche check` 的完整 schema 校验；
- `unstage --file`、`reset <qname>`、`add --directory`；
- `gc`、`repack`；
- `clone --branch`；
- tag 和更多 ref 操作。

验收：

- v1 文档导入/导出工作流可以在 Arche 中完成；
- 单对象和整棵文档树的 JSON 往返幂等；
- `check` 能定位字段类型、qname 和引用错误。

#### E. 技能和智能体

还需要：

- `design-space` 技能切换到 `arche` 命令；
- `design-writing` 技能切换到 Arche 对象和字段；
- 需求分析、交互设计、UI 设计技能切换到需求对象；
- 概念维护技能切换到三个概念目录和 `concept link`；
- agent runtime 的 design baseline、codesummary 和测试同步更新；
- 技能中不再使用 `ds`、`documents/`、`field_list` 等 v1 概念。

验收：

- 智能体只读技能就能完成一套 Arche 技术/需求/概念设计；
- 技能中的命令、qname、字段名和实际 CLI 一致；
- 技能测试覆盖 `arche search`、需求交互、概念链接和评论。

#### F. 测试和验收

还需要：

- HTTP/SSH 端到端测试；
- token 认证测试；
- 分支和远程分支测试；
- source sync 的真实 Git 仓库测试；
- 完整 v1 功能对照测试；
- 并发 push / ref CAS 测试；
- pack / GC / repack 测试；
- 大型文档投影和搜索性能测试。

### 23.3 明确不在本轮范围

- Foundry 设计空间页面；
- 合并请求页面、MR 变更接口和 MR 懒加载；
- Foundry UI 中的对象类型徽标、折叠和按需加载；
- Task/MR 审批流程接入。

### 23.4 建议顺序

1. 完整源码对象模型；
2. `source sync` 和 `add --file` 级联；
3. PUSH2/ref CAS/quarantine；
4. SSH 部署；
5. 文档 CLI 和 JSON/Markdown 完整输出；
6. 技能切换；
7. v1 功能对照和端到端测试；
8. 最后接 Foundry UI/MR。
