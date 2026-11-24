# Arche 设计对象撰写详细指南

本指南规定 Arche v2 中每种设计对象该建什么、边界是什么、要说明哪些字段。
命令、字段表、导入格式和暂存规则以 `arche help` 为准。

Arche 没有独立文档对象树。文档是对象字段；`show --format json|md` 从字段生成。
Arche 不解析源码 AST；Namespace、Class、Method 等由使用者根据源码创建，
`source sync` 只维护 Directory 和 File。

## 1. 概念与命名

### 1.1 概念

概念用于定义含义不显然、容易被不同读者理解成不同东西、或不同文档用法不一致的术语。

当前 Concept 只有两个内容字段：

```rust
pub struct Concept {
    pub name: SymbolHash,        // qname 本地 key，如 agent
    pub display_name: String,    // 展示名称，如“智能体”
    pub parent: SymbolHash,      // ConceptDirectory qname
    pub explanation: ObjectHash, // DocumentParagraphList
}
```

创建：

```bash
arche concept mkdir concept/domain/identity
arche concept create concept/domain/identity/agent \
  --name "智能体" \
  --explanation "能够感知环境并采取行动的系统。"
```

规则：

- 只给需要定义的术语建概念；日常词和显而易见的词不建；
- 概念写定义、边界和区别，不写实现细节；
- 同一个 key 可以同时存在于 `concept/domain`、`concept/requirement`、
  `concept/technical`，它们是三个独立对象；
- 定义变化时更新 `explanation`，不要新建重复概念；
- 删除或重命名前先确认没有文档引用。

旧模型中的 `summary`、`characteristics`、`relations`、`aliases` 已移除。
需要表达属性、关系或别名时，写进 `explanation` 的段落；不要假装存在这些字段。

### 1.2 命名

名字是设计的一部分。读者只看名字，就应该能判断对象是什么、边界在哪里。

- 名字和定位必须一致；名字需要额外解释才能不误解时，改名或修正定位。
- 读者的第一理解必须正确。
- 不要挪用有强约定含义的常见术语。例如“数据库”“服务”“平台”“系统”
  “模块”“管理器”“适配器”不能拿来指别的东西。
- 需要借用常见词时，加限定词或改用领域术语，并在概念空间定义。
- 不用泛词掩盖边界：“核心”“公共”“基础”“工具”“数据”单独出现，
  通常说明对象边界没有想清楚。
- 不用生造词、缩写或堆叠名词；缩写只在已定义且读者熟悉时使用。
- 准确优先于简短。
- 不同概念不能共用名字，也不能用别名掩盖区别。
- 改名是设计变化：同步更新所有引用、文档内容和概念。
- 审阅者遇到看不懂、有歧义的名字必须提出质疑。

qname 只能包含字母、数字、`/`、`#`。中文名称放 `display_name` / `real_name`，
不要放进 qname。

## 2. 如何选择对象类型

### 2.1 System / Module / Layer

System 和 Module 都要有明确的能力或领域边界，内部耦合强于外部耦合，
拆分后能降低理解成本。

- **System**：能独立发布、独立对外提供价值。可以是应用、命令行工具、库、SDK。
  “能单独发布”看结构和能力，不要求现在已有发布流程；“有价值”可以是潜在价值。
- **Module**：不能独立发布，但边界清晰，用于复杂度分治。
- **Layer**：不是职责边界，只表达模块之间的依赖上下关系，不能独立交付。

被多个 System 共同使用的公共库按 System 处理，不作为某一个 System 的 Module。
crate/package 边界不等于 System 边界。

反例：

- 单独拆成 crate、但只为编译缓存拆分的内部模块：Module，不是 System。
- 独立部署、但只被一个 System 调用、接口和生命周期由调用方定义的内部服务：
  Module，不是 System。
- 发布到包仓库、但只定义一个 System 内部契约的 schema/proto 包：
  Module，不是 System。
- 只作为另一个 System 薄入口的 CLI 或启动器：Module，不是 System。

### 2.2 Class / Struct / Enum / Interface / Mixin

分类遵循两个原则：

1. 真实对应：对象必须对应真实存在的代码元素。
2. 意图优先：分类取决于设计意图和承担的职责，不取决于语言关键字或语法形式。

按顺序判断：

1. 只有不携带数据的常量变体，没有别的内容：Enum。
2. 只有数据，没有方法，且不是变体携带数据的 enum：Struct。
3. 只有抽象方法，没有数据：Interface，行为由实现方承担。
4. 同时有数据和具体方法，能独立提供功能或完成职责：Class。
   如果被其他类型继承，子类在概念上是它的一种。
5. 能提供可复用行为，但不能独立提供功能或职责，只为被其他类型复用，
   且没有概念层面的继承意图：Mixin。
6. 只有具体方法，没有数据：不作为类型对象，按 Namespace 处理，方法建 Function。

变体是否携带数据是枚举的分界：只有常量变体是 Enum；Rust enum 的变体携带数据时，
它已经是一个有数据的类型，按 Class 处理，即使没有方法也不归为 Struct。

嵌套在另一个类型里的对象，使用同一对象类型，通过 `parent` 指向外部类型，
不再有 Inner* 对象类型。

语言提示：

- C++：class/struct 关键字不决定类型；纯虚函数无数据 -> Interface；
  数据+方法 -> Class；抽象但有数据 -> Class；只有静态方法 -> Namespace + Function。
- Java/C#：interface -> Interface；抽象类有状态或部分实现 -> Class；
  只有静态方法的工具类 -> Namespace + Function。
- Rust：trait 契约 -> Interface；trait 默认实现只用于复用 -> Mixin；
  struct + impl -> Class；只有字段 -> Struct；enum 只常量变体 -> Enum，
  携带数据 -> Class。
- Go：interface -> Interface；带方法 struct -> Class；只有字段 -> Struct；
  package -> Namespace。
- Python：抽象基类只有抽象方法 -> Interface；dataclass/只有数据 -> Struct；
  数据+方法 -> Class；mixin class -> Mixin。
- C：只有明确采用 OO 设计时才把 struct + 关联函数识别为 Class；
  否则 struct 是 Struct，关联函数是 Function。

### 2.3 Function / Method / Part

- Function：不属于某个类型，挂在 Namespace 下。
- Method：定义在 Class / Interface / Mixin 内部。
- Part：没有独立名字、但需要说明的代码片段，parent 必须是 Class。

Function 和 Method 的区别是归属，不是有没有参数或返回值。
Struct / Enum 没有方法；字段和枚举变体不是独立对象。

### 2.4 其他对象

- Variable：不属于任何对象的全局变量。
- Constant：常量。
- Member：Class / Struct 中的静态或非静态字段。
- Property：由 getter/setter 构成的属性。
- EnumValue：枚举的常量项。
- Argument：Method / Function 的参数。
- ReturnValue：Method / Function 的返回值。

字段不建 Part；字段、常量、成员、属性分别是 Member / Constant / Property 对象。

## 3. 每类对象写什么

代码类型、System、Module 的文档字段按固定顺序出现；可省略的字段直接省略，
不能调换顺序。空的字段不要写占位内容。

### 3.1 TechnicalSystem

```text
position
responsibilities          可省略
non_responsibilities      可省略
features                  可省略
non_features              可省略
invariants                可省略
constraints               可省略
design_principles         可省略
external_dependencies     可省略
usage                     可省略
design_points             可省略
```

System 只描述交付单位、边界和 Module 的关系；Module 的内部设计留在 Module。

### 3.2 TechnicalModule

```text
position
responsibilities
non_responsibilities
features
non_features
invariants
constraints
design_principles
usage
design_points
```

Module 用 `dependencies` 记录对子模块的依赖，不写“对外依赖”文档字段。

### 3.3 TechnicalLayer

```text
position
```

Layer 只说明这一层放什么、不放什么，以及依赖方向中的位置。

### 3.4 TechnicalDirectory / TechnicalFile / TechnicalNamespace

```text
position
```

- Directory / Namespace：置放单句；
- File：文件设计目的单句。

不写职责、成员清单、路径或依赖方向。无法总结时写“无法总结”。

### 3.5 TechnicalClass

```text
position
responsibilities
non_responsibilities
features
non_features
invariants
constraints
design_principles
usage
design_points
```

结构信息用对象字段表达，不写进文档：

```text
superclass, interfaces, mixins
method       ObjectList<TechnicalMethod>
types        ObjectList<嵌套类型>
parts        ObjectList<TechnicalPart>
constants    ObjectList<TechnicalConstant>
members      ObjectList<TechnicalMember>
properties   ObjectList<TechnicalProperty>
```

字段不作为设计子对象；字段用 Member / Property 对象表示。

### 3.6 TechnicalStruct

```text
position
invariants
```

结构体不写职责、功能、用法和设计要点。字段用 Member / Constant 对象表示。

### 3.7 TechnicalEnum

```text
position
```

枚举值用 TechnicalEnumValue 对象表示，不写进文档。
只有常量变体的 enum 才是 TechnicalEnum；携带数据的变体按 Class 处理。

### 3.8 TechnicalInterface

```text
position
responsibilities
non_responsibilities
features
non_features
invariants
usage
design_points
```

接口没有 constraints 和 design_principles 字段。
方法用 `methods` 对象列表表示；不写实现。

### 3.9 TechnicalMethod

```text
position
usage
design_points
```

参数和返回值是对象：

```text
arguments      ObjectList<TechnicalArgument>
return_value   TechnicalReturnValue
```

Method 的定位用动词性短语；参数只取类型参与方法 hash，参数名不参与 hash。
原型不再作为独立字段保存。

### 3.10 TechnicalFunction

```text
position
usage
design_points
```

参数和返回值同样是对象：

```text
arguments      ObjectList<TechnicalArgument>
return_value   TechnicalReturnValue
```

Function 的定位用动词性短语；入口函数等特殊作用函数可用名词短语写角色。

### 3.11 TechnicalMixin

```text
position
responsibilities
non_responsibilities
features
non_features
invariants
usage
design_points
```

Mixin 没有 constraints 字段。方法用 `methods` 对象列表表示。

### 3.12 TechnicalPart

Part 不关联设计文档，不建文档字段。

### 3.13 Requirement 对象

RequirementUser：

```text
real_name      String
age            String
occupation     String
situations     DocumentList
hobbies        DocumentList
needs          DocumentList
priority       u8
stories        Vec<SymbolHash>
```

RequirementStory：

```text
preconditions        DocumentList
frequency            u8
description          DocumentParagraphList
requirement          DocumentList
users                Vec<SymbolHash>
functions            Vec<SymbolHash>
```

RequirementSystem：

```text
modules              ObjectList<RequirementModule>
form                 DocumentText
covered_users        DocumentList
closed_loop_goal     DocumentParagraphList
relationships        DocumentParagraphList
boundary             DocumentList
```

RequirementModule：

```text
functions            ObjectList<RequirementFunction>
core_concept         DocumentText
included_concepts    DocumentList
concept_relations    DocumentParagraphList
boundary             DocumentList
```

RequirementFunction：

```text
flow                 DocumentMermaid
interactions         ObjectList<RequirementInteraction>
initiator            DocumentText
system_change        DocumentParagraphList
preconditions        DocumentParagraphList
feedback             DocumentText
description          DocumentParagraphList
```

RequirementInteraction：

```text
description          DocumentParagraphList
ui                   DocumentParagraphList
```

### 3.14 Concept

```text
display_name    String
explanation     DocumentParagraphList
```

概念写定义、边界和区别，不写实现细节，也不复制设计正文。

## 4. 字段怎么写

### 4.1 定位 position

一句话，句末加句号，说明该对象是什么、为什么存在。

- System / Module / Class / Struct / Enum / Interface / Mixin：名词性短语；
- Method / Function：动词性短语，入口函数等特殊作用函数可用名词短语写角色；
- Layer：写这一层放什么、不放什么，以及依赖方向中的位置；
- Directory / Namespace：置放单句；
- File：文件设计目的单句。

不写职责清单、成员、入口、实现步骤、父类/接口、线程安全、生命周期。

正例：

```text
面向外部调用方的认证服务。
认证令牌的签发与刷新服务。
会话状态的只读存储抽象。
置放认证模块的领域规则。
置放 AuthService 的实现。
```

反例：

```text
负责认证、授权和用户管理。
问题：职责清单，不是交付单位。
TokenUtils。
问题：兜底工具名，没有能力边界。
负责订单处理。
问题：写的是职责，不是层的置放规则。
业务层。
问题：只有层名，没有说明放什么、不放什么。
```

### 4.2 职责与非职责

- responsibilities：对象承担什么职责；用可验证的行为描述。
- non_responsibilities：明确不属于它的职责，用来划清边界。

不写实现步骤、内部字段或调用链。边界不清楚时先修定位，不要靠非职责补救。

### 4.3 功能与非功能

- features：对外可观察的能力。
- non_features：明确不提供的能力。

功能写“能做什么”，不写“怎么实现”。非功能用来防止读者误以为对象承担了某项能力。

### 4.4 不变量

写始终成立的条件：

- 状态约束：某个字段在所有合法状态下必须满足什么；
- 关系约束：对象与其他对象之间必须保持什么关系；
- 顺序约束：调用或生命周期的顺序要求。

不变量必须可验证；不要写“设计良好”“性能优秀”这类无法判断的句子。

### 4.5 约束

写外部施加的限制：

- 性能、延迟、吞吐；
- 平台、语言、运行时、依赖版本；
- 协议、兼容性、安全、合规；
- 资源上限。

约束不是设计目标；目标写进设计原则或设计要点。

### 4.6 参数与返回值

参数和返回值已经是对象，不是文档字段：

```text
TechnicalArgument { source, ty, position }
TechnicalReturnValue { source, ty, position }
```

规则：

- 参数按声明顺序；
- 参数名不参与方法 hash；
- hash8 只由参数类型和修饰符计算；
- 没有返回值时不要创建 ReturnValue 对象；
- 参数说明写在 Argument 的 position 里。

### 4.7 用法 usage

写调用方式、使用场景和示例：

- 典型调用顺序；
- 前置条件和失败方式；
- 简短示例；
- 可以放段落、代码块、Mermaid、HTML。

不写实现细节，不重复参数和返回值说明。

### 4.8 设计原则 design_principles

写这个对象遵循的设计取舍和理由。

- 一条原则一个意思；
- 写“为什么这样设计”，不是“做了什么”；
- 与约束区分：约束是外部限制，设计原则是内部取舍。

### 4.9 设计要点 design_points

每个设计点一个 topic，topic 标题写问题类别，例如：

```text
状态
继承结构
数据库结构
并发模型
错误处理
缓存策略
```

正文写这个点的设计选择、约束和理由。多个设计点按重要性排序。
不要写泛泛总结，也不要重复职责。

### 4.10 代码块 / 图表 / HTML

```bash
arche doc code add <qname> <field> --language rust "source"
arche doc mermaid set <qname> <field> "graph TD; A-->B;"
arche doc html add <qname> <field> "<b>preview</b>"
```

- 代码块保留 language；
- Mermaid 和 HTML 使用各自围栏；
- 不要把代码原文塞进普通段落。

## 5. 创建示例

### 5.1 技术设计

```bash
arche init
arche create technical-system app
arche doc text set technical/app position "认证与授权服务。"
arche doc list item add technical/app responsibilities "签发令牌"
arche create technical-namespace core --parent technical
arche create technical-class Login --namespace technical/namespace/core
arche doc text set technical/namespace/core/Login position "登录入口。"
arche doc list item add technical/namespace/core/Login responsibilities "校验凭据"
arche create technical-method --parent technical/namespace/core/Login \
  --prototype "login(String, String) -> Token"
arche add -A
arche commit -m "initial design"
```

### 5.2 需求设计

```bash
arche create requirement-system warehouse
arche create requirement-module inbound --parent requirement/system/warehouse
arche create requirement-function receive --parent \
  requirement/system/warehouse/module/inbound
arche doc text set requirement/system/warehouse form "仓库管理系统"
arche doc list item add requirement/system/warehouse covered_users "仓管员"
```

### 5.3 概念

```bash
arche concept mkdir concept/domain/identity
arche concept create concept/domain/identity/agent \
  --name "智能体" \
  --explanation "能够感知环境并采取行动的系统。"
```

### 5.4 批量

```bash
arche batch plan.json
arche doc import technical/app fields.json
arche show technical/app --format json
arche show technical/app --format md
```

## 6. 提交前检查

```bash
arche status
arche check
arche doc lint
arche add -A
arche commit -m "design update"
```

- 未暂存的修改不会进入提交；
- `show --format md` 从字段生成文档，不存在独立 Document 对象；
- 不要用 git 命令访问设计空间远程。
