如何选择对象类型

TechnicalSystem / TechnicalModule / TechnicalLayer

TechnicalSystem 和 TechnicalModule 都要有明确的能力或领域边界，
内部耦合强于外部耦合，拆分后能降低理解成本。

  TechnicalSystem：能独立发布、独立对外提供价值。
  TechnicalModule：不能独立发布，但边界清晰，用于复杂度分治。
  TechnicalLayer：不是职责边界，只表达模块之间的依赖上下关系。

TechnicalLayer 不能独立交付；TechnicalSystem 可以没有 TechnicalModule；
TechnicalModule 可以没有 TechnicalLayer。
不要按文件数、目录或技术角色切分 TechnicalModule。

TechnicalClass / TechnicalStruct / TechnicalEnum / TechnicalInterface / TechnicalMixin

按顺序判断：

  1. 只有不携带数据的常量变体：TechnicalEnum。
  2. 只有数据，没有方法，且不是携带数据的变体：TechnicalStruct。
  3. 只有抽象方法，没有数据：TechnicalInterface。
  4. 有数据 + 具体方法，能独立提供功能或职责：TechnicalClass。
  5. 只提供可复用行为，不能独立提供功能，且没有概念继承意图：TechnicalMixin。
  6. 只有具体方法，没有数据：按 TechnicalNamespace 处理，
     方法建 TechnicalFunction。

变体是否携带数据是 TechnicalEnum 的分界：Rust enum 的变体携带数据时
按 TechnicalClass 处理。嵌套类型使用对应的对象类型和 parent。

语言提示：

  C++：class/struct 关键字不决定类型；纯虚函数无数据 -> TechnicalInterface；
        数据+方法 -> TechnicalClass；抽象但有数据 -> TechnicalClass；
        只有静态方法 -> TechnicalNamespace + TechnicalFunction。
  Java/C#：interface -> TechnicalInterface；抽象类有状态或实现 -> TechnicalClass；
        只有静态方法的工具类 -> TechnicalNamespace + TechnicalFunction。
  Rust：trait 契约 -> TechnicalInterface；trait 默认实现只用于复用 -> TechnicalMixin；
        struct + impl -> TechnicalClass；只有字段 -> TechnicalStruct；
        enum 只常量变体 -> TechnicalEnum，携带数据 -> TechnicalClass。
  Go：interface -> TechnicalInterface；带方法 struct -> TechnicalClass；
      只有字段 -> TechnicalStruct；package -> TechnicalNamespace。
  Python：抽象基类只有抽象方法 -> TechnicalInterface；
        dataclass/只有数据 -> TechnicalStruct；
        数据+方法 -> TechnicalClass；mixin class -> TechnicalMixin。
  C：只有明确采用 OO 设计时才把 struct + 关联函数识别为 TechnicalClass；
     否则 struct 是 TechnicalStruct，关联函数是 TechnicalFunction。

TechnicalFunction / TechnicalMethod / TechnicalPart

  TechnicalFunction：不属于类型，挂在 TechnicalNamespace 下。
  TechnicalMethod：定义在 TechnicalClass / TechnicalInterface / TechnicalMixin 内部。
  TechnicalPart：没有独立名字、但需要说明的代码片段，parent 必须是 TechnicalClass。

TechnicalStruct / TechnicalEnum 没有方法；字段和枚举变体不是独立对象。
