文档字段顺序

TechnicalClass、TechnicalStruct、TechnicalEnum、TechnicalInterface、
TechnicalMethod、TechnicalFunction、TechnicalMixin 的文档字段按 schema 顺序出现。
可省略的字段直接省略，不能调换顺序。

  position
  responsibilities
  non_responsibilities
  features
  non_features
  invariants
  constraints
  usage
  design_principles
  design_points

参数和返回值不是文档字段：

  arguments       ObjectList<TechnicalArgument>
  return_value    TechnicalReturnValue

规则：

- position 通常是 DocumentText，直接写一句话；
- responsibilities / non_responsibilities / features / non_features /
  invariants / constraints 通常是 DocumentList，每个条目一个句子；
- usage / design_points 是 DocumentParagraphList，可放段落、topic、代码块、
  Mermaid、HTML；
- design_points 每个设计点一个 topic，标题写问题类别
  （如“状态”“继承结构”“并发模型”），按重要性排序；
- 不要使用对象 schema 之外的字段名；
- 空字段直接省略，不要写占位内容。

定位 position

TechnicalSystem / TechnicalModule / TechnicalClass / TechnicalStruct /
TechnicalEnum / TechnicalInterface / TechnicalMixin：
  名词性短语，一句话，说明它是什么、为什么存在。
  不写职责清单、成员、入口、实现步骤、父类/接口、线程安全、生命周期。

TechnicalMethod / TechnicalFunction：
  动词性短语；入口函数等特殊作用函数可用名词短语写角色。

TechnicalLayer：
  写这一层放什么、不放什么，以及它在依赖方向中的位置。

TechnicalDirectory / TechnicalNamespace：
  置放单句，例如“置放认证模块的领域规则。”

TechnicalFile：
  文件设计目的单句，例如“置放 AuthService 的实现。”
  无法总结时写“无法总结”，不要改写成功能描述。

职责与非职责

responsibilities：
  这个对象承担什么职责；用可验证的行为描述，不写实现细节。
non_responsibilities：
  明确不属于它的职责，用来划清边界。
features / non_features：
  对外可观察的能力或非能力，不写内部步骤。
invariants：
  始终成立的条件；写状态约束、关系约束、顺序约束。
constraints：
  外部施加的限制，如性能、平台、协议、兼容性、依赖限制。

参数与返回值

parameters：
  每个参数写含义、类型、取值约束、默认值和特殊说明。
returns：
  返回值含义、类型、失败/空值语义；不写实现过程。
prototype：
  函数原型，使用规范化签名，不包含 pub/private 等可见性修饰符。

用法与设计

usage：
  使用方式、调用场景、示例；可以放段落、代码块、Mermaid、HTML。
design_principles：
  这个对象遵循的设计原则，写取舍理由。
design_points：
  按问题类别分 topic：状态、继承结构、数据库结构、并发模型、错误处理等；
  写设计选择、约束和理由，不写泛泛总结。

代码块 / 图表 / HTML

  arche doc code add <qname> <field> --language rust "source"
  arche doc mermaid set <qname> <field> "graph TD; A-->B;"
  arche doc html add <qname> <field> "<b>preview</b>"

代码块保留 language；Mermaid 和 HTML 使用各自围栏，不写在普通文本里。
