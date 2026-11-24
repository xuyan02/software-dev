# Arche v2 设计

- 状态：设计定稿，实现中
- 日期：2026-11-23
- 工程：`projects/arche`
- 目标：在 v1 的功能基础上，重建对象模型、文档模型、需求空间和概念空间

## 1. 范围

v2 从头实现，不迁移 v1 数据。v1 实现已删除；Foundry 现在只使用 Arche。

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
pub struct GitCommit(Hash32);
```

- `Hash32`：SHA-256 原始 32 字节；
- `SymbolHash`：名称、路径、依赖和语义链接使用的符号 hash；
- `ObjectHash`：对象记录 hash，由对象字段计算，不是记录字节的 hash；
- `CommitHash`：提交 hash；
- `GitCommit`：源码 Git commit，零表示无。

可选 hash 一律使用全零 hash 表示 `None`。全零 hash 永远不是合法内容值。所有 hash 类型（`SymbolHash`、`ObjectHash`、`Hash32`、`CommitHash`）自带可空语义，字段、参数和属性值都不再用 `Option<Hash>` 包装；判断是否为空用 `is_zero()`，清除值写 `ZERO`。

所有设计对象都有 `qname`、`name`。除 `Root` 外，所有对象都有 `parent`。字段顺序固定为 `qname`、`name`、`parent`（`Root` 无）、其余字段。下文结构示例中部分公共字段可能省略。

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
| 字段容器（字段本身） | `<owner-qname>/<field>` |
| 无名对象（列表项、Part） | `<parent-qname>/<8 位随机 name>` |
| 方法 | `<parent-qname>/<name>`，`name` 是规范化 prototype 文本 |

object hash 是类型 tag、对象版本、`qname`、`name`、`parent` 和其他字段的 SHA-256；引用位置直接使用字段中的 ObjectHash。object hash 是对象记录的键，不从记录字节推导。

无名对象在创建时可以传入 name；不传时生成 8 位十六进制随机 name。qname 为 `<parent-qname>/<name>`，不再包含字段前缀和 `#`。name 和 qname 不依赖对象内容，内容变化不会改变它们。不再使用 `.N` 顺序号，也不保存分配计数器。

示例：

```text
technical/.../class/Foo                        TechnicalClass
technical/.../class/Foo/responsibilities       DocumentList
technical/.../class/Foo/a1b2c3d4               DocumentText
technical/.../class/Foo/usage                  DocumentParagraphList
technical/.../class/Foo/e5f60718               DocumentTopic
technical/.../class/Foo/e5f60718/body          DocumentParagraphList
technical/.../class/Foo/e5f60718/body/9021abcd DocumentText
technical/.../class/Foo/33445566               DocumentTopic
```

列表项顺序仍由 `items` 的顺序决定。

### 3.3 根

```text
technical
requirement
concept
```

- `technical`：技术空间根；
- `requirement`：需求空间根；
- `concept`：概念空间根。

全局根对象：

```text
root
```

`Root` 不属于技术空间，不加 `Technical` 前缀。

## 4. 对象总表

### 4.1 技术对象

```text
TechnicalRoot
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
TechnicalVariable
TechnicalConstant
TechnicalMember
TechnicalProperty
TechnicalEnumValue
TechnicalArgument
TechnicalReturnValue
```

### 4.2 需求对象

```text
RequirementRoot
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
concept/domain/...
concept/requirement/...
concept/technical/...
```

### 4.4 共享结构对象

```text
ObjectList
Root
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

对象类型总数：技术 22 种、需求 7 种、概念 2 种、共享 2 种、文档 7 种，共 **40 种**，全部是设计对象。评论不是对象类型，见第 11 节。

## 5. 公共字段

### 5.1 源码对象公共字段

技术代码对象使用：

```rust
pub parent: SymbolHash,
pub namespace: SymbolHash,
pub file: SymbolHash,
pub module: SymbolHash,
pub source: SourceSpan,
pub explicit_dependencies: Vec<SymbolHash>,
pub implicit_dependencies: Vec<SymbolHash>,
```

- `parent` 用于内部类型、方法、Part 等；
- `namespace`、`file`、`module` 用于关联命名空间、文件和模块；
- 不再有 `Inner*` 类型，内部类型通过 `parent` 表达；
- `source` 是值类型，不是对象，也不是 SymbolHash：

```rust
pub struct SourcePath {
    pub path: SymbolHash,
}

pub struct SourceSpan {
    pub path: SymbolHash,
    pub start: u32,
    pub end: u32,
}
```

- `TechnicalDirectory`、`TechnicalFile` 使用 `SourcePath`；
- 代码类型和 Part 使用 `SourceSpan`；
- 依赖只存目标 qname，不存证据和理由；
- 字段指向另一个设计对象时保存对方的 `ObjectHash`；依赖、语义链接等按名字解析的引用保存对方 qname（`SymbolHash`）。

### 5.2 结构对象公共字段

```rust
pub name: SymbolHash,
pub members: ObjectHash,  // ObjectList
pub modules: ObjectHash,  // ObjectList
pub layers: ObjectHash,   // ObjectList
```

- `member`、`module`、`layer` 都是 `ObjectList`；
- `ObjectList` 自己存 `owner` 和 `items`，不存字段名；
- 字段名由父对象 schema 决定，qname 为 `<owner>/<field>`。

### 5.3 ObjectList

```rust
pub struct ObjectList {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub items: Vec<ObjectHash>,
}
```

- 列表有顺序；
- 每个 item 按自身规则有 qname；
- item 可以是命名对象，也可以是随机 name 的无名对象；
- 列表本身是对象，有独立文件；
- `ObjectList` 不存字段名。

## 6. 技术对象

### 6.1 TechnicalRoot

```rust
pub struct TechnicalRoot {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub file: ObjectHash,       // ObjectList<TechnicalFile>
    pub directory: ObjectHash,  // ObjectList<TechnicalDirectory>
    pub namespace: ObjectHash,  // ObjectList<TechnicalNamespace>
    pub system: ObjectHash,     // ObjectList<TechnicalSystem>
}
```

### 6.2 Root

```rust
pub struct Root {
    pub qname: SymbolHash,
    pub source_commit: GitCommit,
    pub technical: ObjectHash,
    pub requirement: ObjectHash,
    pub concept: ObjectHash,
}
```

`Root` 绑定源码 commit 和三个空间根的对象 hash；评论通过独立评论列表引用，不属于任何目标对象。对象字段指向其他对象时保存对方的 `ObjectHash`，qname → object hash 的对应由提交中的快照保存。

### 6.3 TechnicalSystem / TechnicalModule / TechnicalLayer

```rust
pub struct TechnicalSystem {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,             // technical
    pub members: ObjectHash,     // ObjectList
    pub modules: ObjectHash,     // ObjectList
    pub layers: ObjectHash,      // ObjectList
    pub constants: ObjectHash,   // ObjectList
    pub variables: ObjectHash,   // ObjectList
    // 文档字段见 7.1
}

pub struct TechnicalModule {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,             // system/module/layer
    pub members: ObjectHash,
    pub dependencies: Vec<SymbolHash>,
    pub constants: ObjectHash,   // ObjectList
    pub variables: ObjectHash,   // ObjectList
    // 文档字段见 7.1
}

pub struct TechnicalLayer {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,             // system/module
    pub members: ObjectHash,
    pub dependencies: Vec<SymbolHash>,
    pub position: ObjectHash,
}
```

- 层不能嵌套层；
- 层的依赖由 `dependencies` 显式记录；
- 有层的容器，其代码成员和子模块必须归入层；
- 层顺序从 1 开始，同级唯一。

### 6.4 TechnicalDirectory / TechnicalFile / TechnicalNamespace

```rust
pub struct TechnicalDirectory {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub source: SourcePath,
    pub children: ObjectHash, // ObjectList
    pub position: ObjectHash,
}

pub struct TechnicalFile {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub source: SourcePath,
    pub entities: ObjectHash, // ObjectList
    pub position: ObjectHash,
}

pub struct TechnicalNamespace {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash, // namespace
    pub children: ObjectHash,  // 子 namespace ObjectList
    pub entities: ObjectHash,  // 直接元素 ObjectList
    pub constants: ObjectHash, // ObjectList
    pub variables: ObjectHash, // ObjectList
    pub position: ObjectHash,
}
```

属性系统里源码位置统一用一个 `source` 属性：

- `TechnicalDirectory`、`TechnicalFile` 用 `SourcePath`；
- 代码类型（Class、Struct、Enum、Interface、Method、Function、Mixin、Part）用 `SourceSpan`；
- `TechnicalNamespace` 没有 source。

### 6.5 TechnicalClass

```rust
pub struct TechnicalClass {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub namespace: SymbolHash,
    pub file: SymbolHash,
    pub module: SymbolHash,
    pub source: SourceSpan,

    pub superclass: SymbolHash,
    pub interfaces: Vec<SymbolHash>,
    pub mixins: Vec<SymbolHash>,
    pub explicit_dependencies: Vec<SymbolHash>,
    pub implicit_dependencies: Vec<SymbolHash>,

    pub method: ObjectHash,     // ObjectList
    pub types: ObjectHash, // ObjectList
    pub parts: ObjectHash,      // ObjectList
    pub constants: ObjectHash,  // ObjectList
    pub members: ObjectHash,    // ObjectList
    pub properties: ObjectHash, // ObjectList

    // 文档字段见 7.2
}
```

### 6.6 TechnicalStruct / TechnicalEnum

```rust
pub struct TechnicalStruct {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub namespace: SymbolHash,
    pub file: SymbolHash,
    pub module: SymbolHash,
    pub source: SourceSpan,
    pub dependencies: Vec<SymbolHash>,
    pub method: ObjectHash,
    pub types: ObjectHash,
    pub constants: ObjectHash, // ObjectList
    pub members: ObjectHash,   // ObjectList
    // 文档字段见 7.2
}

pub struct TechnicalEnum {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub namespace: SymbolHash,
    pub file: SymbolHash,
    pub module: SymbolHash,
    pub source: SourceSpan,
    pub dependencies: Vec<SymbolHash>,
    pub values: ObjectHash, // ObjectList
    // 文档字段见 7.2
}

pub struct TechnicalEnumValue {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash, // enum
    pub source: SourceSpan,
    pub position: ObjectHash,
}
```

枚举没有 `method`、`types`、`part`；枚举值由 `TechnicalEnumValue` 表示，不记录依赖。

### 6.7 TechnicalInterface

```rust
pub struct TechnicalInterface {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub namespace: SymbolHash,
    pub file: SymbolHash,
    pub module: SymbolHash,
    pub source: SourceSpan,

    pub extends: SymbolHash,
    pub dependencies: Vec<SymbolHash>,

    pub methods: ObjectHash, // ObjectList

    // 文档字段见 7.2
}
```

### 6.8 TechnicalMethod / TechnicalFunction

```rust
pub struct TechnicalMethod {
    pub qname: SymbolHash,       // parent/<name#hash8>
    pub name: SymbolHash,        // <simple-name>#<hash8>
    pub parent: SymbolHash,      // class/interface/mixin
    pub source: SourceSpan,
    pub is_static: bool,
    pub is_async: bool,
    pub is_abstract: bool,
    pub is_const: bool,
    pub overrides: SymbolHash,
    pub explicit_dependencies: Vec<SymbolHash>,
    pub implicit_dependencies: Vec<SymbolHash>,
    pub arguments: ObjectHash,     // ObjectList of TechnicalArgument
    pub return_value: ObjectHash,  // TechnicalReturnValue
    // 文档字段见 7.2
}

pub struct TechnicalFunction {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub namespace: SymbolHash,
    pub file: SymbolHash,
    pub module: SymbolHash,
    pub source: SourceSpan,
    pub is_async: bool,
    pub explicit_dependencies: Vec<SymbolHash>,
    pub implicit_dependencies: Vec<SymbolHash>,
    pub arguments: ObjectHash,     // ObjectList of TechnicalArgument
    pub return_value: ObjectHash,  // TechnicalReturnValue
    // 文档字段见 7.2
}
```

方法没有 `signature` 和 `prototype` 字段。创建方法时用规范原型文本计算 `name` 和 qname，文本不保存；原型可以从 `arguments`、`return_value` 和修饰符重建。规范原型不包含 `pub`、`public`、`private`、`protected`、`internal` 等可见性修饰符，但保留 `async`、`static` 等修饰符。签名变化就是新方法。

### 6.9 TechnicalMixin

```rust
pub struct TechnicalMixin {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub namespace: SymbolHash,
    pub file: SymbolHash,
    pub module: SymbolHash,
    pub source: SourceSpan,

    pub on: Vec<SymbolHash>,
    pub superclass: SymbolHash,
    pub interfaces: Vec<SymbolHash>,
    pub mixins: Vec<SymbolHash>,
    pub explicit_dependencies: Vec<SymbolHash>,
    pub implicit_dependencies: Vec<SymbolHash>,

    pub methods: ObjectHash,
    pub types: ObjectHash,

    // 文档字段见 7.2
}
```

### 6.10 TechnicalPart

```rust
pub struct TechnicalPart {
    pub qname: SymbolHash, // parent/<8 位随机 name>
    pub name: SymbolHash,
    pub parent: SymbolHash, // class
    pub source: SourceSpan, // 必须有行范围
    pub explicit_dependencies: Vec<SymbolHash>,
    pub implicit_dependencies: Vec<SymbolHash>,
    pub members: ObjectHash, // ObjectList
    pub methods: ObjectHash, // ObjectList
    pub types: ObjectHash,   // ObjectList
}
```

Part 不关联设计文档，不建文档字段。

### 6.11 TechnicalVariable / TechnicalConstant / TechnicalMember / TechnicalProperty

```rust
pub struct TechnicalVariable {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash, // system/module/namespace
    pub source: SourceSpan,
    pub ty: SymbolHash,
    pub position: ObjectHash,
}

pub struct TechnicalConstant {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash, // system/module/namespace/class/struct
    pub source: SourceSpan,
    pub ty: SymbolHash,
    pub position: ObjectHash,
}

pub struct TechnicalMember {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash, // class/struct
    pub source: SourceSpan,
    pub ty: SymbolHash,
    pub is_static: bool,
    pub is_const: bool,
    pub position: ObjectHash,
}

pub struct TechnicalProperty {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash, // class
    pub ty: SymbolHash,
    pub getter: SourceSpan,
    pub setter: SourceSpan,
    pub position: ObjectHash,
}
```

变量是不属于任何对象的全局变量，成员是类/结构体中的静态或非静态变量。这四类对象不记录依赖。属性没有独立 `source`，getter/setter 各自记录源码范围。

### 6.12 TechnicalArgument / TechnicalReturnValue

```rust
pub struct TechnicalArgument {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash, // method/function
    pub source: SourceSpan,
    pub ty: SymbolHash,
    pub position: ObjectHash,
}

pub struct TechnicalReturnValue {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash, // method/function
    pub source: SourceSpan,
    pub ty: SymbolHash,
    pub position: ObjectHash,
}
```

方法/函数通过 `arguments` 的 `ObjectList` 持有参数，通过 `return_value` 直接引用返回对象。参数和返回值不记录依赖。

## 7. 技术文档字段

字段仍然是对象的字段，值是对文档对象的 `ObjectHash` 引用。所有引用按 schema 校验目标类型。

### 7.1 TechnicalSystem / TechnicalModule

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |
| `responsibilities` | `DocumentList` |
| `non_responsibilities` | `DocumentList` |
| `features` | `DocumentList` |
| `non_features` | `DocumentList` |
| `invariants` | `DocumentList` |
| `constraints` | `DocumentList` |
| `design_principles` | `DocumentList` |
| `usage` | `DocumentParagraphList` |
| `design_points` | `DocumentParagraphList` |

`TechnicalSystem` 另有 `external_dependencies: DocumentList`；`TechnicalModule` 用 `dependencies: Vec<SymbolHash>` 表示模块依赖。

### 7.2 代码类型

TechnicalClass：

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |
| `responsibilities` | `DocumentList` |
| `non_responsibilities` | `DocumentList` |
| `features` | `DocumentList` |
| `non_features` | `DocumentList` |
| `invariants` | `DocumentList` |
| `constraints` | `DocumentList` |
| `design_principles` | `DocumentList` |
| `usage` | `DocumentParagraphList` |
| `design_points` | `DocumentParagraphList` |

TechnicalStruct：

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |
| `invariants` | `DocumentList` |

TechnicalEnum / TechnicalEnumValue：

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |

TechnicalInterface：

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |
| `responsibilities` | `DocumentList` |
| `non_responsibilities` | `DocumentList` |
| `features` | `DocumentList` |
| `non_features` | `DocumentList` |
| `invariants` | `DocumentList` |
| `usage` | `DocumentParagraphList` |
| `design_points` | `DocumentParagraphList` |

TechnicalMixin：

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |
| `responsibilities` | `DocumentList` |
| `non_responsibilities` | `DocumentList` |
| `features` | `DocumentList` |
| `non_features` | `DocumentList` |
| `invariants` | `DocumentList` |
| `usage` | `DocumentParagraphList` |
| `design_points` | `DocumentParagraphList` |

TechnicalMethod / TechnicalFunction：

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |
| `usage` | `DocumentParagraphList` |
| `design_points` | `DocumentParagraphList` |

TechnicalLayer / TechnicalDirectory / TechnicalFile / TechnicalNamespace：

| 字段 | 目标类型 |
|---|---|
| `position` | `DocumentText` |

不记录字段列表。Class、Struct、Enum 的字段信息不进入对象文档字段；字段由 `TechnicalMember`、`TechnicalProperty` 对象表示。

## 8. 文档内容模型

### 8.1 DocumentText

```rust
pub struct DocumentText {
    pub qname: SymbolHash,
    pub content: String,
}
```

`DocumentText` 是一个段落：内容不含回车/换行，内部没有格式差异。多段使用 `DocumentParagraphList`；代码、Mermaid、HTML 使用各自的块对象。

### 8.2 文本规则

- 一个 `DocumentText` 只对应一段；
- `content` 不允许包含 `\n` 或 `\r`；
- 不支持加粗、斜体、行内代码、链接、概念引用等行内格式；
- 需要在正文中提及其他概念时，直接写概念的 qname 文本。

### 8.3 DocumentList

```rust
pub struct DocumentList {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash, // qname 反向链接
    pub items: Vec<ObjectHash>,
}
```

列表项只存对象的 hash，不存对象本身；每项实际是什么 kind 在运行时解析，不在类型系统里约束。

### 8.4 DocumentTopic

```rust
pub struct DocumentTopic {
    pub qname: SymbolHash,
    pub title: String,
    pub body: ObjectHash, // DocumentParagraphList
}
```

`title` 同样是不含换行、无行内格式的一段文本。

### 8.5 DocumentParagraphList

```rust
pub struct DocumentParagraphList {
    pub qname: SymbolHash,
    pub items: Vec<ObjectHash>, // DocumentText | DocumentList | DocumentTopic | DocumentCode | DocumentMermaid | DocumentHtml
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
DocumentList
DocumentParagraphList
DocumentParagraphList
DocumentParagraphList
DocumentMermaid
ObjectList
```

- 字段类型不携带列表项约束，类型系统里没有泛型参数；
- 列表项的 kind 在运行时解析和校验：`DocumentList` 的项是 `DocumentText`，普通段落列表的项是 `DocumentTopic`，UI 段落列表的项是 `DocumentHtml`；
- `ObjectList`：设计对象的结构列表，不是文档内容。

### 8.8 文档投影

技术对象的文档按以下顺序投影：

1. 对象名称；
2. schema 中声明的字段顺序；
3. 字段内容按 `DocumentParagraphList`、`DocumentList`、`DocumentTopic` 的顺序渲染；
4. 结构关系投影为只读段落：
   - 父类 / 父接口 -> `DocumentText`
   - 接口列表 -> `DocumentList`
   - 混入列表 -> `DocumentList`
   - 结构依赖 -> `DocumentList`
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
      "items": [
        {"hash": "<object-hash>"}
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
RequirementRoot
├─ user: ObjectList
├─ story: ObjectList
└─ system: ObjectList
   └─ modules: ObjectList
      └─ functions: ObjectList
         └─ interactions: ObjectList
```

```rust
pub struct RequirementRoot {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub user: ObjectHash,   // ObjectList
    pub story: ObjectHash,  // ObjectList
    pub system: ObjectHash, // ObjectList
}
```

需求系统和技术系统是不同对象；需求模块和技术模块是不同对象；需求功能和技术功能是不同对象。

### 9.2 RequirementUser

`RequirementUser` 按无名对象处理：`name` 是自动生成的 8 位十六进制 key，不承载用户姓名；`real_name` 保存真实姓名。

| 字段 | 目标类型 |
|---|---|
| `real_name` | `String` |
| `age` | `String` |
| `occupation` | `String` |
| `situations` | `DocumentList` |
| `hobbies` | `DocumentList` |
| `needs` | `DocumentList` |
| `priority` | `u8` |
| `stories` | `Vec<SymbolHash>`，关联故事 qname 列表 |

### 9.3 RequirementStory

对象名称是故事名称。

| 字段 | 目标类型 |
|---|---|
| `preconditions` | `DocumentList` |
| `frequency` | `u8` |
| `description` | `DocumentParagraphList` |
| `requirement` | `DocumentList` |
| `users` | `Vec<SymbolHash>`，关联用户 qname 列表 |
| `functions` | `Vec<SymbolHash>`，关联功能 qname 列表 |

### 9.4 RequirementSystem

对象名称是软件名称。

| 字段 | 目标类型 |
|---|---|
| `form` | `DocumentText` |
| `covered_users` | `DocumentList` |
| `closed_loop_goal` | `DocumentParagraphList` |
| `relationships` | `DocumentParagraphList` |
| `boundary` | `DocumentList` |

结构字段：

```rust
pub modules: ObjectHash, // ObjectList
```

### 9.5 RequirementModule

对象名称是模块名称。

| 字段 | 目标类型 |
|---|---|
| `core_concept` | `DocumentText` |
| `included_concepts` | `DocumentList` |
| `concept_relations` | `DocumentParagraphList` |
| `boundary` | `DocumentList` |

`core_concept` 和 `included_concepts` 中的概念引用必须指向 `concept/requirement/` 下的概念。

结构字段：

```rust
pub functions: ObjectHash, // ObjectList
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
| `interactions` | `ObjectList` |

- `flow` 是一个 Mermaid 对象，qname 为 `<function>/flow`；
- 交互是显式对象，不从 Mermaid 解析；
- 交互列表顺序就是流程图顺序。

### 9.7 RequirementInteraction

对象名称是交互名称。

| 字段 | 目标类型 |
|---|---|
| `description` | `DocumentParagraphList` |
| `ui` | `DocumentParagraphList` |

UI 图直接挂在交互下：

```text
<interaction>/ui
<interaction>/ui#a1b2c3d4    DocumentHtml
<interaction>/ui#e5f60718    DocumentHtml
```

不建 `RequirementUi` 对象。

## 10. 概念空间

### 10.1 目录

```text
concept/domain/...
concept/requirement/...
concept/technical/...
```

三个顶层目录固定：

- `domain`：现实中存在、被公认的事物；
- `requirement`：为了描述产品需求而发明的概念；
- `technical`：为了实现而发明的概念。

```rust
pub struct ConceptDirectory {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub directories: ObjectHash, // ObjectList<ConceptDirectory>
    pub concepts: ObjectHash,    // ObjectList<Concept>
}
```

`concept` 根本身也是一个 `ConceptDirectory`；`domain`、`requirement`、`technical` 是它下面三个固定目录。

同一个概念名可以在三个目录中同时存在：

```text
concept/domain/agent
concept/requirement/agent
concept/technical/agent
```

它们是三个独立对象，不是别名或继承关系。

### 10.2 Concept

```rust
pub struct Concept {
    pub qname: SymbolHash,
    pub name: SymbolHash,        // qname 的本地 key
    pub parent: SymbolHash,      // ConceptDirectory qname
    pub display_name: String,    // 如“智能体”
    pub explanation: ObjectHash, // DocumentParagraphList
}
```

- 概念维度由 qname 目录决定；
- 同一目录内名称受控；不同目录允许同名；
- `relations` 是一个 `DocumentList`，用文字说明与其他概念的差异和联系；
- 不建独立的概念关系对象，也不存结构化的跨维度链接。

### 10.3 与其他概念的关系

概念之间的关系只用文字表达，挂在 `relations` 列表下：

```text
concept/requirement/agent/relations
concept/requirement/agent/relations#a1b2c3d4
```

每个列表项是一段 `DocumentText`；需要对比、举例或引用其他概念时，直接在文字中写概念 qname，不建立额外的链接对象。

## 11. 评论

评论不是设计对象，不进入对象图、提交、快照、refs 或属性系统，也不做内容寻址和版本控制。评论正文直接存在评论记录里，不是单独的文档对象。

### 11.1 存储

评论存放在 `<workspace>/.arche/comments`，独立于 `objects/`、`commits/`、`refs/`：

```text
CMT1
version: u8 = 5
count: u32
record:
  id: u64
  branch: text          // 评论所在分支
  target: text          // 被评论对象的 qname
  body: text            // 纯文本
  conclusion: text      // 解决时填写，未解决为空
  resolved_by: text     // 填写结论的智能体
  resolved: u8
  created_by: text
  created_at: i64
  updated_at: i64
  deleted: u8           // clear 后保留的删除标记
```

- `id` 在一个评论存储内唯一，push/fetch 时由服务端解决跨工作区冲突；
- resolve 时必须提供非空的 `conclusion` 和 `resolved_by`；
- 每次新增或修改都原子重写整个文件；
- 评论按分支记录；push 上传整个评论存储，服务端合并后返回权威 id，fetch/clone 拉取并合并远端评论；评论仍不属于提交或对象图；
- `clear` 把评论标记为 deleted 而不是物理删除，删除标记随 push/fetch 同步，合并请求门禁看到的是未删除评论数。

### 11.2 规则

- 评论不是目标对象的子对象，目标对象也不存评论字段；
- 评论通过 `target` 的 qname 挂在任意设计对象上；`comment list <target> --subtree` 返回目标本身、其所有子孙对象以及目标字段/锚点上的评论；
- 修改评论不改变目标对象，也不产生新的对象版本；
- 评论不参与 `check` 的对象图校验，孤儿评论由评论自己的检查处理；
- 设计 diff 默认不比较评论；评论使用独立命令查看和更新。

可评论目标示例：

```text
TechnicalClass
TechnicalClass/position
TechnicalClass/responsibilities#a1b2c3d4
TechnicalClass/usage#e5f60718
RequirementInteraction/ui#9021abcd
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
  comments                  # 独立评论存储，按分支记录并随 push/fetch 同步
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
- 除 schema 字段外不存可推导字段；
- loose 对象文件为 magic、版本号、类型 tag、qname、name、parent（Root 除外）、对象字段；当前版本为 1；
- magic 属于 loose Store 的文件封装，pack 不在每个对象前重复 magic；
- object hash 由类型 tag、版本号和对象字段计算，是记录存储键；
- qname、name 都是 `SymbolHash`。

### 12.3 CMT2

提交包含：

- 父提交；
- Root 对象 hash；
- 作者、时间、消息；
- qname → object hash 快照，覆盖该提交可见的全部对象。

对象字段指向其他对象时保存对方的 `ObjectHash`；依赖和语义链接保存对方 qname（`SymbolHash`），因此允许环和前置引用。qname 到具体版本的解析由提交中的快照完成，快照是提交的一部分，不写入目标对象；同一 qname 的历史版本由不同提交的快照区分。

评论列表通过 Root 引用，但修改评论不会修改技术、需求、概念对象。

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

- GC 从 refs 和 Root 出发计算可达对象；
- 删除不可达对象、符号、提交；
- 评论不在对象图中，不参与 GC；
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

对象内容不可变，修改一个对象会产生新的 `ObjectHash`。工作区写入时会把对象图中的 `ObjectHash` 引用归一化到 qname 映射的当前版本，因此列表项和父对象始终指向最新内容；旧版本仍留在对象库中，由提交快照和 GC 管理。

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

## 14. 远程与传输

### 14.1 URL

| URL | 传输 |
|---|---|
| `/path` / `file:///path` | 本地目录 |
| `http://host:port/<repo>` | `arche_server::serve` HTTP |
| `https://host[:port]/<repo>` | 同上，TLS |
| `ssh://[user@]host[:port]/<path>` | SSH 上的 `arche_server::serve_stdio` |

### 14.2 服务端库与 CLI

服务端是 Rust 库 `arche_server`，Foundry 可直接链接；`arche` CLI 只提供同一实现的入口：

```bash
arche serve --root <repos-root>
arche serve --root <repos-root> --listen 0.0.0.0:7700
arche serve --root <repos-root> --token <token>
arche serve --stdio --root <repo-dir>
```

库接口：

```rust
arche_server::serve(&arche_server::ServerOptions::new(root).listen(addr).token(token))?;
arche_server::serve_stdio(repo_root)?;
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
/usr/local/bin/foundry-arche-stdio <allowlist> <repos-root>
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
- object hash 与记录键一致；
- 依赖目标存在，依赖允许循环；
- 继承、接口、混入、extends、on 不能成环；
- 结构关系自动计入显式依赖；
- 字段目标类型符合 schema；
- `DocumentList` 项类型正确；
- `DocumentTopic.body` 必须是 `DocumentParagraphList`；
- 概念引用能解析；
- 需求层级完整；
- 交互是显式对象；
- 流程图是 Mermaid；
- 文档 HTML/Mermaid 格式合法。

`arche doc lint` 保留参数、返回值、字段格式等规则检查。

## 17. CLI

v2 保留 v1 的命令组，替换文档命令的内部模型。

### 17.1 仓库和分支

```bash
arche init
arche clone     # 从远程继承已记录的源码 Git URL，--source 仅用于覆盖
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

参数和返回值仍使用 `DocumentList`、`DocumentText`。

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
arche concept mkdir concept/domain/<dir>
arche concept create concept/domain/<dir>/<name> --name <term> --summary <text>
arche concept create concept/requirement/<dir>/<name> ...
arche concept create concept/technical/<dir>/<name> ...
arche concept set/show/ls/mv/rm
arche concept feature add/rm
arche concept relation add/rm
arche concept alias add/rm
arche concept check
```

### 17.7 评论

```bash
arche comment add <target> <text>
arche comment resolve <id> --conclusion <text> --agent <agent>
arche comment list [<target>] [--subtree] [--open] [--format text|json]
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
| 依赖 | `explicit_dependencies` / `implicit_dependencies`，只存 qname |
| 内部类型 | 通过 `parent` 表达，不建 `Inner*` |
| 设计结构 System/Module/Layer | `TechnicalSystem/Module/Layer` + `ObjectList` |
| 独立文档树 | 删除；文档字段投影 |
| 文档 text/list/topic/block/mermaid/html | `DocumentText/List/Topic/Code/Mermaid/Html` |
| 文档评论 | 独立评论存储，按 target 挂接，不进入对象图 |
| 概念空间 | 三个目录 + 一个 `Concept` 类型 + 文字 relations |
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
- 需求树：RequirementRoot/User/Story/System/Module/Function/Interaction；
- 概念树：`concept/domain`、`concept/requirement`、`concept/technical`；
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
- 概念写作技能区分 `concept/domain`、`concept/requirement`、`concept/technical`；
- 同名概念必须建立 link 并写清差异。

### 20.4 agent runtime

- design baseline 和 codesummary 使用 v2 类型；
- 枚举/类分类规则更新；
- 搜索使用 `arche search`，不逐对象调用 `arche show`；
- 设计写入技能和测试同步更新。

## 21. 实现阶段

1. **对象和存储内核**：DSO2/CMT2、对象仓库、符号、Root、refs、pack。
2. **技术对象和文档字段**：schema、字段校验、投影、CLI 基础。
3. **暂存和源码**：index/worktree、source、diff、commit。
4. **远程**：PUSH2、HTTP/SSH、host sshd、ACL。
5. **需求空间**：需求对象、交互、UI、层级校验。
6. **概念空间**：三个目录、Concept、文字 relations、引用检查。
7. **评论**：独立存储、target 索引、孤儿检查。
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
- 需求概念引用 `concept/requirement`。

### 22.4 概念

- 三个顶层目录存在；
- 同一名称可在三个目录出现；
- 概念有 qname；
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

**2026-11-24 补充**：

- 创作能力补齐：`create` 支持 `--file`、`--module`、`--source`；新增
  `structure assign/list/unassigned/check`、`doc check`、`comment clear`、
  `commit --author`、`source ls`、`source cat`。
- PUSH2 bundle、`incoming/` quarantine、fast-forward 检查和 ref CAS 已落地；
  `deploy/` 提供 SSH forced command 和 AuthorizedKeysCommand。
- Foundry 已切换：新项目创建为 Arche 仓库；v1 实现已删除；
  design_baseline、CodeSummary、技能和平台提示词改用 `arche` 命令和对象字段。
- 仍未实现：源码 AST 解析、Foundry 专属 Arche 页面、tag/更多 ref 操作、
  Arche client/server 的 HTTP/SSH 端到端与并发测试。

### 23.1 已实现

- `DSO2` 对象记录、`CMT2` 提交记录、SHA-256 内容寻址；
- `.arche/` 对象、符号、提交、快照和临时目录；
- worktree/index、`add`、`unstage`、`status`、`diff`、`commit`、`reset`；
- 分支列表、创建、删除和 `checkout`；
- `TechnicalSystem`、`TechnicalModule`、`TechnicalLayer`、`TechnicalDirectory`、`TechnicalFile`、`TechnicalNamespace`；
- `TechnicalClass`、`TechnicalStruct`、`TechnicalEnum`、`TechnicalInterface`、`TechnicalMethod`、`TechnicalFunction`、`TechnicalMixin`、`TechnicalPart`；
- `TechnicalVariable`、`TechnicalConstant`、`TechnicalMember`、`TechnicalProperty`、`TechnicalEnumValue`、`TechnicalArgument`、`TechnicalReturnValue`；
- `DocumentText`、`DocumentList`、`DocumentParagraphList`、`DocumentTopic`、`DocumentMermaid`、`DocumentHtml`；
- 技术、需求和概念的文档字段写入与类型校验；
- 需求用户、故事、系统、模块、功能、交互对象；
- `RequirementFunction.flow` 和 `RequirementInteraction.ui`；
- 独立评论存储、按 target 查询、解决和 check；
- 三个概念目录、同名概念；
- `search`、`check`、`show --format text|json|md`；
- `source fetch`、`source checkout`、`source worktree`、`source status`、`source sync`（按 tracked file 建 File 对象）；
- `add --file`（当前只级联 File、files 列表和 technical root）；
- 本地路径远程、HTTP 远程、SSH stdio 远程；
- `arche_server` HTTP 和 `--stdio`；
- HTTP Bearer token 客户端读取和服务端校验。

### 23.2 剩余工作

#### A. 完整源码对象模型与同步

对象模型（Layer、Directory、File、Namespace、Class、Struct、Enum、Interface、Method、Function、Mixin、Part）已经存在。

Arche 内部**不提供源码 AST 解析**：

- `source sync` 只建立 `TechnicalDirectory` 和 `TechnicalFile`；
- Namespace、Class、Struct、Enum、Interface、Method、Function、Mixin、Part 由技能/智能体通过 CLI 创建和维护；
- 继承、接口、混入、`on`、`overrides`、依赖关系也由调用方写入。

还需要：

- `source sync` 删除、重命名和移动 File/Directory 时的 qname 迁移；
- `add --file` 按 File 关联、source 路径和 parent 链级联暂存相关对象，而不是只暂存 File 和 file 列表；
- `add --file` 同时暂存这些对象的文档字段对象；
- `arche check` 校验继承、接口、混入、override 和依赖。

验收：

- 一个真实源码仓库 sync 后得到 Directory/File 树；
- 智能体创建的 Namespace/Class/Method 能通过 `add --file` 按文件正确暂存；
- 重命名文件时 File 对象和目录链正确迁移；
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

当前已有 `arche_server::serve_stdio`，但没有生产部署。

还需要：

- 宿主 sshd 配置；
- `foundry-arche` 服务账户；
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
- 概念维护技能切换到三个概念目录和文字 relations；
- agent runtime 的 design baseline、codesummary 和测试同步更新；
- 技能中不再使用 `ds`、`documents/`、`field_list` 等 v1 概念。

验收：

- 智能体只读技能就能完成一套 Arche 技术/需求/概念设计；
- 技能中的命令、qname、字段名和实际 CLI 一致；
- 技能测试覆盖 `arche search`、需求交互和评论。

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
