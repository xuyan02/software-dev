设计对象：定义、判断原则、正例/反例

TechnicalSystem
  定义
    System 是最高一层的可交付单位，包括应用、命令行工具、库和 SDK。

  判断
    必须同时满足：
      1. 能单独发布；
      2. 能独立对外提供有意义的价值。
    “能单独发布”看结构和能力，不要求现在已有发布流程。
    “有价值”可以是潜在价值，不要求现在已有外部使用者。
    关键看它离开当前宿主后是否仍然成立。
    被多个 System 共同使用的公共库一定是 System，不能作为某一个 System 的 Module。

  对象正例
    可独立部署、对外提供 API 的后端应用。
    可独立安装、由用户直接使用的命令行工具。
    可单独发布、供其他项目引用的库或 SDK。
    可独立分发、面向最终用户的桌面或移动应用。
    当前只在工程内使用、但边界清晰、不依赖特定宿主、理论上可以单独发布，
    并且对其他项目也可能有价值的库。
    被工程内多个 System 共同使用的公共库。

  对象反例
    单独拆成 crate 的内部模块。
      问题：拆分原因只是编译缓存或依赖管理；离开父 System 没有独立使用者。
    独立部署、但只被一个 System 调用的内部服务。
      问题：接口、数据和生命周期由调用方定义，离开调用方没有独立价值。
    和主应用共用版本、数据和发布节奏的 worker/cron/agent。
      问题：能单独启动，但不是独立交付物，价值依附于主应用。
    发布到包仓库、但只定义一个 System 内部契约的 schema/proto/config 包。
      问题：只有使用它的那个 System 消费，没有独立价值。
    只作为另一个 System 薄入口的 CLI 或启动器。
      问题：只是转发到宿主 System，没有自己的设计价值。
    只在宿主中运行的插件或适配器。
      问题：离开宿主没有意义；除非有独立用户、独立契约和独立价值。

  创建
    arche create technical-system <name>

  字段
    position
      写什么：名词性短语，一句话说明这个交付单位是什么、为什么存在。
      不写：职责清单、成员、入口、实现步骤、父类/接口、线程安全、生命周期。
      正例：面向外部调用方的认证服务。
      正例：开发者使用的命令行发布工具。
      反例：负责认证、授权和用户管理。
        问题：职责清单，不是交付单位或能力边界。
      反例：AuthSystem。
        问题：只是名字，没有说明它是什么。
      反例：TokenUtils。
        问题：兜底工具名，没有能力边界。

    responsibilities
      写什么：System 在整体系统中必须承担的责任环节；缺了它整体功能会失效。
      格式：每行“负责<一个责任环节>。”，按重要性排序。
      省略：定位能自然推出全部职责时省略。
      正例：负责认证令牌的生命周期管理。
      正例：负责凭据校验策略的编排。
      反例：负责调用 validateToken() 校验 JWT 签名。
        问题：写到了方法和实现细节。
      反例：负责 login、logout、refresh 三个接口。
        问题：方法清单，不是职责。
      反例：负责保证线程安全。
        问题：线程安全属于设计要点。

    non_responsibilities
      写什么：明确不属于它的职责，用于排除容易混淆的边界。
      条件：该内容本身是系统中的必要环节，且不说明容易被误解为职责。
      格式：每行“不负责<职责>，由<对象>负责。”
      省略：没有容易混淆的内容时省略。
      正例：不负责凭据校验，由 CredentialVerifier 负责。
      正例：不负责令牌持久化，由 TokenStore 负责。
      反例：不负责发射火箭。
        问题：明显无关，不是容易混淆的职责。

    features
      写什么：System 对外提供哪些可观察的功能或行为。
      格式：每行“提供<一个功能>。”，按重要性排序。
      省略：职责已经完整表达功能时省略，不要换一种说法重复职责。
      正例：提供令牌签发和刷新的能力。
      反例：调用 sign() 和 refresh() 完成令牌处理。
        问题：方法名和实现步骤，不是可观察的功能。

    non_features
      写什么：明确不提供、且容易被误解为功能的能力。
      格式：每行“不提供<功能>，由<对象>提供。”
      省略：没有容易混淆的内容时省略。
      正例：不提供用户注册的能力，由 AccountService 提供。
      正例：不提供权限判定的能力，由 AuthorizationService 提供。

    invariants
      写什么：System 运行时必须始终为真的条件，例如数据所有权、一致性、隔离保证。
      要求：只写对推导系统正确性有重要作用的不变量；条件必须可判定。
      格式：每行一个条件，句号结尾，按重要性排序。
      正例：账户余额始终不小于零。
      正例：结束时间始终不早于开始时间。
      反例：保证数据一致。
        问题：不可判定，没有说明一致的条件。

    constraints
      写什么：System 必须满足的外部限制，是对设计的要求，为方案选型提供依据。
      内容：性能、平台、协议、兼容性、安全、合规、资源上限。
      格式：每行一个约束，句号结尾，按重要性排序。
      省略：没有影响设计方案选型的约束时省略。
      正例：必须运行在现有 PostgreSQL 上，不引入新的数据库。
      正例：部署环境不能访问外网，所有外部依赖必须可离线运行。
      反例：使用 Redis 做缓存。
        问题：这是实现方案，不是对设计的要求。

    design_principles
      写什么：对这个 System 及其所有子树统一生效的设计规范。
      条件：作用于子树、稳定、不随单次改动变化。
      格式：列表，每行一个可判定的短句；位置就是范围，不写范围、理由、代价或实现细节。
      正例：核心功能以外的新功能统一以插件的方式引入。
      正例：能用固定逻辑完成的步骤都用固定逻辑，只有固定逻辑无法完成的部分才调用 LLM。
      反例：遵循单一职责原则。
        问题：通用口号，没有本项目的具体决定，无法审查。
      反例：使用 Redis 缓存校验结果。
        问题：实现机制选择，属于设计要点。

    external_dependencies
      写什么：System 依赖的外部系统、库、服务或协议，以及依赖目的和约束。
      格式：列表，每行一个外部依赖。
      不写：内部 Module 或内部对象；它们用结构关系表达。
      正例：依赖 PostgreSQL 保存会话状态。
      正例：依赖 OAuth 2.0 身份提供方校验第三方凭据。

    usage
      写什么：使用这个 System 的闭环示例，覆盖基础、常用、边界和失败场景。
      格式：每个示例包含自然语言说明、代码和主干注释；每个示例闭环：
      构造/准备 -> 主要调用 -> 结果或清理。
      省略：接口简单、用法一目了然时省略。
      不写：单调用、泛泛说明、实现细节、前置条件和错误分支细节。
      正例：
        本示例用 AuthService 完成一次登录并刷新令牌。
        // 准备依赖并构造服务
        let service = AuthService::new(verifier, store);
        // 登录并签发令牌
        let token = service.authenticate(&credentials);
        // 刷新令牌
        service.refresh(&token);
      反例：service.authenticate(credentials);
        问题：只有单个调用，不是闭环。
      反例：本类用于认证。
        问题：写的是职责，不是用法。

    design_points
      写什么：无法从其他字段自然推定、需要单独说明的设计内容。
      内容：架构说明、类说明、继承结构、状态、流程、线程安全、并发模型、
      错误处理、缓存策略、数据库结构等。
      格式：每个设计点一个 topic，topic 标题是问题类别，正文写设计选择、
      约束和理由；多个设计点按重要性排序。
      正例：topic“状态”写会话状态的流转和约束。
      正例：topic“并发模型”写锁粒度和竞态处理。
      反例：把线程安全写进 responsibilities。
        问题：线程安全属于设计要点。

TechnicalModule
  定义
    Module 是 TechnicalSystem 或父 Module 内部的一个内聚职责边界。
    它把共同实现同一能力或领域概念的顶层代码元素组织在一起，对外隐藏内部细节；
    不独立发布，也不独立对外提供价值。
    作用是复杂度分治，不是按文件数、目录或技术角色切分。

  判断
    必须同时满足：
      1. 有明确的能力或领域边界：
         不谈及内部结构就能说清它负责什么或提供什么功能；
         往往围绕一个领域概念或一类边界明确的问题；
         内部耦合明显强于外部耦合。
      2. 复杂度分治能降低理解成本：
         内部复杂度已经影响整体理解；
         单独成 Module 后父级文档只需描述子 Module 的职责和关系；
         子 Module 之间不需要大量互引，每个子 Module 不只有一个元素。
    不是 System：Module 和 System 的唯一区别是能否独立发布并独立提供价值。

  支撑 Module 例外
    common / utils / base 等提供全系统支撑能力时，即使不具备强聚合条件，
    也可以作为 Module，但必须同时满足：
      一个 System 只设一个这样的支撑 Module；
      元素低复杂度，自身达不到 Module 条件；
      元素具有独立功能，无法与其他 Module 合并；
      元素具有广泛普遍性，不只服务少数特定 Module；
      业务能力不能塞进支撑 Module。

  对象正例
    认证令牌：负责令牌签发、刷新和撤销，内部有令牌服务、存储和编码，
    对外只暴露令牌能力，不单独发布。
    库存预留：负责下单时的库存预占、释放和超时回收。
    Markdown 渲染：编辑器 System 的一部分，内部有解析、语法树和渲染。
    common/base 支撑模块：提供日志、错误包装、时间处理等全系统通用能力。

  对象反例
    misc 或没有明确支撑范围的杂项桶。
      问题：只是“其他东西放这里”，不是支撑能力。
    models / services / handlers。
      问题：按技术角色划分，不是能力或领域边界。
    按文件数或目录平均拆出的子模块。
      问题：拆分后仍大量互引，每个也无法单独说明职责。
    只有一个函数的模块。
      问题：模块没有聚合任何东西；函数直接挂在 System 下。
    被多个 System 共同使用的公共库。
      问题：满足 System 条件，应建 System。
    入口、启动、配置模块。
      问题：只是结构角色，不是能力边界。

  创建
    arche create technical-module <name> --parent <system|module>

  字段
    position
      写什么：名词性短语，一句话说明这个 Module 是什么、为什么存在。
      不写：职责清单、成员、入口、实现步骤、父类/接口、线程安全、生命周期。
      正例：订单价格的折扣计算规则。
      正例：认证令牌的签发、刷新和撤销。
      反例：负责订单处理。
        问题：职责清单，不是能力边界。
      反例：业务模块。
        问题：泛词，没有说明边界。

    responsibilities
      写什么：Module 在整体中必须承担的责任环节；缺了它整体功能会失效。
      格式：每行“负责<一个责任环节>。”，按重要性排序。
      省略：定位能自然推出全部职责时省略。
      正例：负责库存预留的创建、释放和超时回收。
      反例：负责调用 reserve() 和 release()。
        问题：方法名和实现细节。

    non_responsibilities
      写什么：明确不属于它的职责，用于排除容易混淆的边界。
      格式：每行“不负责<职责>，由<对象>负责。”
      省略：没有容易混淆的内容时省略。
      正例：不负责支付扣款，由 PaymentModule 负责。

    features
      写什么：Module 对外提供哪些可观察的功能或行为。
      格式：每行“提供<一个功能>。”
      省略：职责已经完整表达功能时省略。
      正例：提供库存预占和释放的能力。

    non_features
      写什么：明确不提供、且容易被误解为功能的能力。
      格式：每行“不提供<功能>，由<对象>提供。”
      正例：不提供库存盘点能力，由 InventoryAuditModule 提供。

    invariants
      写什么：Module 运行时必须始终为真的条件，例如数据所有权、一致性、隔离保证。
      格式：每行一个可判定条件。
      正例：预留记录的状态始终是 Pending、Confirmed 或 Released 之一。

    constraints
      写什么：Module 必须满足的外部限制，为方案选型提供依据。
      内容：性能、平台、协议、兼容性、安全、合规、资源上限。
      格式：每行一个约束。
      正例：预留过期扫描必须在一分钟内完成一轮。

    design_principles
      写什么：对这个 Module 及其所有子树统一生效的设计规范。
      格式：列表，每行一个可判定短句；位置就是范围。
      正例：库存扣减统一通过库存模块完成，其他模块不得直接写库存表。

    usage
      写什么：使用这个 Module 的闭环示例，覆盖基础、常用、边界和失败场景。
      格式：每个示例“准备 -> 主要调用 -> 结果/清理”。
      省略：接口简单、用法一目了然时省略。
      正例：构造库存服务、预占库存、确认或释放。
      反例：reserve(item);
        问题：单调用，不是闭环。

    design_points
      写什么：无法从其他字段自然推定的设计内容。
      内容：架构说明、子模块关系、状态、流程、并发、错误处理、数据库结构等。
      格式：每个设计点一个 topic，标题是问题类别。
      正例：topic“并发模型”写预留记录的锁粒度。
      正例：topic“状态”写 Pending -> Confirmed -> Released 的流转。

TechnicalLayer
  定义
    Layer 是为了梳理容器内模块之间依赖关系而引入的虚拟顺序标签。
    它不承担职责，不是职责边界，也不是可交付单位。

  判断
    所在的 System 或 Module 复杂度极高，不分层就难以理解；
    分层必须让依赖关系更清楚或明确约束依赖方向；
    只是增加结构、没有降低理解成本时不要分层。
    领域有普遍采用的分层方式（如 MVC）且复杂度值得时，可以优先采用。

  上下关系
    依赖方是上层，被依赖方是下层。
    上层知道下层；下层不能依赖或知道具体上层。
    只有相邻层可以直接依赖，隔层不能直接依赖。
    同一容器内的 Layer 不能形成依赖环，也不能互相依赖。
    调整层的上下关系是设计变更，要同步调整受影响的依赖。

  对象正例
    置放对外接口和协议适配，只依赖下一层应用能力。
    置放领域规则，不依赖外层适配器。

  对象反例
    业务层。
      问题：只有层名，没有说明放什么、不放什么。
    认证层。
      问题：把职责边界当成了层。

  创建
    arche create technical-layer <name> --parent <system|module>

  字段
    position
      写什么：这一层放什么、不放什么，以及它在依赖方向中的位置。
      格式：一句话，句号结尾。
      正例：置放对外接口和协议适配，只依赖下一层应用能力。
      正例：置放领域规则，不依赖外层适配器。
      反例：业务层。
        问题：只有层名，没有说明放什么、不放什么。

    Layer 没有 responsibilities、features、usage 等文档字段。
    层内成员通过 members 列表表达，依赖通过 dependencies 表达。


TechnicalDirectory
  定义
    TechnicalDirectory 对应真实存在的源码目录。

  判断
    只对源码中真实存在的目录建 Directory；
    目录树由 source sync 维护，不手工建目录对象。

  对象正例
    src/auth/domain 目录对应 technical/directory/src/auth/domain。

  对象反例
    为一个不存在的虚拟分类建 Directory。
      问题：Directory 只对应源码目录，逻辑分类用 Namespace。

  创建
    arche source sync 维护；目录对象不需要手写文档树。

  字段
    source
      写什么：目录的源码路径，由 source sync 写入。
      不要手工改。

    children
      ObjectList<TechnicalDirectory | TechnicalFile>，由 source sync 维护。

    position
      写什么：置放单句，说明这个目录里放什么内容。
      格式：一句话，句号结尾，直接写在字段里。
      不写：路径、qname、具体类型或成员清单、职责、目的、边界、依赖方向。
      正例：置放认证模块的领域规则。
      正例：置放命令行程序入口的代码。
      反例：置放 src/auth/domain 下的代码。
        问题：写了路径，没有抽象说明放什么。
      反例：置放 User、Session、Token 和 LoginService。
        问题：列出了具体类型和成员，不是类别概括。
      反例：本目录负责认证。
        问题：写的是职责，不是“放什么”。
      反例：置放实现令牌校验的代码。
        问题：无法总结成模块时应写“无法总结”，不要改写成功能描述。

TechnicalFile
  定义
    TechnicalFile 对应真实存在的源码文件。

  判断
    只对源码中真实存在的文件建 File；
    文件对象由 source sync 维护。

  对象正例
    src/auth/login.rs 对应 technical/file/src/auth/login.rs。

  对象反例
    为一个函数或类型建 File。
      问题：函数和类型是独立对象，File 只对应源码文件。

  创建
    arche source sync 维护。

  字段
    source
      源码路径，由 source sync 写入，不要手工改。

    entities
      ObjectList，文件直接包含的代码实体，由使用者创建对象后维护。

    position
      写什么：文件设计目的单句，说明该文件承载的设计角色或能力。
      格式：一句话，句号结尾。
      不写：路径、qname、具体类型或成员清单、职责清单、依赖方向。
      正例：置放 AuthService 的实现。
      正例：置放命令行程序入口的代码。
      反例：负责认证。
        问题：写的是职责，不是文件承载的设计角色。
      反例：置放 User、Session、Token。
        问题：列出了具体类型，不是文件目的。

TechnicalNamespace
  定义
    TechnicalNamespace 是逻辑命名空间，用来组织跨文件概念。
    它不等于物理目录，也不等于 File。

  判断
    顶层类型、函数、常量、变量需要归属时使用 Namespace；
    即使只被一个文件使用，也用独立 Namespace；
    不要把 File 当成 namespace，避免出现 files/<path>/Class 这种带文件前缀的 qname。

  对象正例
    technical/namespace/core
    类型 qname：technical/namespace/core/Login

  对象反例
    files/src/auth/LoginService。
      问题：把文件路径当成命名空间。
    technical/file/src/auth。
      问题：用 File 当 namespace。

  创建
    arche create technical-namespace <name> --parent technical

  字段
    children
      ObjectList<TechnicalNamespace>，子命名空间。

    entities
      ObjectList，直接归属的 Class / Struct / Enum / Interface / Mixin / Function。

    constants
      ObjectList<TechnicalConstant>。

    variables
      ObjectList<TechnicalVariable>。

    position
      写什么：置放单句，说明这个命名空间里放什么内容。
      格式：一句话，句号结尾。
      不写：路径、qname、具体类型或成员清单、职责、边界、依赖方向。
      正例：置放认证模块的代码。
      正例：置放领域模型和领域服务。
      反例：本命名空间负责认证。
        问题：写的是职责，不是“放什么”。
      反例：置放 User、Session、Token。
        问题：列出了具体类型，不是类别概括。

TechnicalClass
  定义
    TechnicalClass 代表 OOP 中的“类型”，描述一类对象的结构和行为。
    基本特征：
      同时具备数据和方法；
      是技术概念在代码层面的对应物；
      能独立提供功能或完成职责；
      被其他类型继承时存在概念层面的继承意图：子类在概念上是父类的一种。
    Rust 等语言中变体携带数据的 enum 即使没有方法，也按 Class 处理。
    字段不作为设计子对象；字段用 TechnicalMember / TechnicalProperty 表示。

  判断
    按顺序：
      有数据 + 具体方法，能独立提供功能或完成职责 -> TechnicalClass。
      只有数据、没有方法 -> TechnicalStruct。
      只有抽象方法、没有数据 -> TechnicalInterface。
      只提供可复用行为、不能独立提供功能、没有概念继承意图 -> TechnicalMixin。
      只有具体方法、没有数据 -> TechnicalNamespace + TechnicalFunction。

  对象正例
    认证令牌的签发与刷新服务。
    领域层的订单价格计算策略。
    基于 Redis 的会话状态存储。
    Rust 中 struct + impl 且承担职责的类型。

  对象反例
    只有数据、没有方法。
      应改为 TechnicalStruct。
    只有纯虚函数、没有数据。
      应改为 TechnicalInterface。
    只为其他类型复用行为，没有概念继承意图。
      应改为 TechnicalMixin。
    只有静态方法的工具类。
      应改为 TechnicalNamespace + TechnicalFunction。
    Rust enum 的变体携带数据但没有方法。
      仍按 TechnicalClass 处理，不是 TechnicalStruct。

  创建
    arche create technical-class <name> --namespace <namespace> \
      --file technical/file/<path> --module technical/<system>/<module> \
      --source <path:start-end>

  结构归属
    arche structure assign <element> --parent <system|module|layer>
    arche structure list
    arche structure unassigned
    arche structure check

  结构字段
    namespace / file / module
      所属命名空间、文件、模块的 qname。
    source
      SourceSpan，源码范围。
    superclass
      父类 qname；只能有一个；只写代码限定名对应的对象。
    interfaces
      Vec<SymbolHash>，实现的接口 qname；按声明顺序。
    mixins
      Vec<SymbolHash>，混入的 mixin qname；按声明顺序。
    explicit_dependencies / implicit_dependencies
      依赖类型 qname 列表；不写文档。
    method / types / parts / constants / members / properties
      分别是 Method、嵌套类型、Part、常量、成员、属性的 ObjectList。
      这些是对象，不是文档字段。

  文档字段
    position
      名词性短语，一句话说明这个类型表达的概念是什么。
      内嵌类型说明它在外部类型中表达的概念，不写外部实例关系。
      正例：认证令牌的签发与刷新服务。
      正例：会话状态的只读存储抽象。
      反例：负责认证令牌的签发与刷新。
        问题：动词性短语，不是名词性短语。

    responsibilities
      这个类型必须承担的责任环节；缺了它整体功能会失效。
      格式：每行“负责<一个责任环节>。”
      省略：定位能自然推出全部职责时省略。
      正例：负责令牌的签发、刷新和撤销。
      反例：负责调用 sign() 和 refresh()。
        问题：方法名和实现细节。

    non_responsibilities
      明确不属于它的职责，用于排除容易混淆的边界。
      格式：每行“不负责<职责>，由<对象>负责。”
      正例：不负责令牌持久化，由 TokenStore 负责。

    features
      对外提供哪些可观察的功能或行为。
      格式：每行“提供<一个功能>。”
      省略：职责已经完整表达功能时省略。
      正例：提供令牌签发和刷新的能力。

    non_features
      明确不提供、且容易被误解为功能的能力。
      格式：每行“不提供<功能>，由<对象>提供。”
      正例：不提供用户注册的能力，由 AccountService 提供。

    invariants
      实例在生命周期内必须始终为真的状态条件。
      格式：每行一个可判定条件。
      正例：会话结束后 refresh token 不再可用。
      反例：保证数据一致。
        问题：不可判定。

    constraints
      该类在实现或使用中必须满足的条件。
      内容：线程模型、内存布局、兼容性、性能上限、平台、协议。
      格式：每行一个约束。
      正例：该类型必须在单线程事件循环中使用。
      反例：使用 Redis 做缓存。
        问题：实现方案，不是约束。

    design_principles
      对该类型及其所有子树统一生效的设计规范。
      格式：列表，每行一个可判定短句。
      正例：所有状态变更统一通过 apply 方法完成。

    usage
      使用该类型的闭环示例，覆盖基础、常用、边界和失败场景。
      格式：准备 -> 主要调用 -> 结果/清理。
      省略：接口简单、用法一目了然时省略。
      正例：构造服务、登录签发令牌、刷新令牌。
      反例：service.login();
        问题：单调用，不是闭环。

    design_points
      无法从其他字段自然推定的设计内容。
      内容：继承结构、状态、流程、并发模型、错误处理、缓存策略等。
      格式：每个设计点一个 topic，标题是问题类别。
      正例：topic“状态”写会话状态的流转。
      正例：topic“继承结构”写为什么这样分层。

TechnicalStruct
  定义
    只有数据，没有方法，且不是携带数据的枚举变体。

  判断
    只有字段；没有行为；字段之间可能有必须保持的关系。
    有数据 + 具体方法时应改为 TechnicalClass。

  对象正例
    用户身份的数据载体。
    会话状态的数据快照。

  对象反例
    数据 + 具体方法。
      应改为 TechnicalClass。
    Rust enum 的变体携带数据。
      应按 TechnicalClass 处理。

  创建
    arche create technical-struct <name> --namespace <ns>

  结构字段
    namespace / file / module / source
    dependencies
    method / types / constants / members
      都是对象列表，不是文档字段。

  文档字段
    position
      名词性短语，说明这个数据载体表达的概念。
      正例：用户身份的数据载体。
      反例：负责用户校验。
        问题：动词性短语，且 Struct 没有职责。
    invariants
      字段之间必须始终成立的关系。
      格式：每行一个可判定条件。
      正例：end_time 始终不早于 start_time。
      反例：保证数据一致。
        问题：不可判定。
    Struct 不写 responsibilities、features、usage、design_points。

TechnicalEnum
  定义
    只有不携带数据的常量变体。

  判断
    只有常量变体，没有数据字段，没有方法。
    Rust enum 的变体携带数据时按 TechnicalClass 处理。
    枚举值用 TechnicalEnumValue 对象表示，不写进文档。

  对象正例
    会话生命周期状态。
    订单状态。

  对象反例
    Rust enum 的变体携带数据。
      应按 TechnicalClass 处理。
    带方法的 enum。
      应改为 TechnicalClass。

  创建
    arche create technical-enum <name> --namespace <ns>

  结构字段
    namespace / file / module / source
    dependencies
    values       ObjectList<TechnicalEnumValue>
    position     DocumentText

  文档字段
    position
      名词性短语，说明这组状态或类别表达什么。
      正例：会话生命周期状态。
      反例：负责管理会话。
        问题：写成了职责。
    Enum 不写其他文档字段。

TechnicalInterface
  定义
    只有抽象方法，没有数据；行为由实现方承担。
    是契约类型，不承载实例状态。

  判断
    只有抽象方法；没有数据字段；没有具体实现。
    抽象类有数据或部分实现时应改为 TechnicalClass。

  对象正例
    令牌校验能力的抽象。
    可取消任务的契约。

  对象反例
    抽象类有数据或部分实现。
      应改为 TechnicalClass。
    只有具体方法、没有数据。
      应按 TechnicalNamespace + TechnicalFunction 处理。

  创建
    arche create technical-interface <name> --namespace <ns>

  结构字段
    namespace / file / module / source
    extends
    dependencies
    methods      ObjectList<TechnicalMethod>

  文档字段
    position
      名词性短语，说明这个契约表达什么。
      正例：令牌校验能力的抽象。
    responsibilities
      实现方必须承担的契约责任。
    non_responsibilities
      明确不属于契约的职责。
    features / non_features
      契约提供或不提供的能力。
    invariants
      实现方必须维持的不变量。
    usage
      如何实现接口、如何注入调用方的闭环示例。
    design_points
      每个设计点一个 topic，写契约设计选择。
    不写 constraints 和 design_principles。

TechnicalMixin
  定义
    提供可复用行为，但不能独立提供功能或职责，且没有概念继承意图。

  判断
    行为只为被其他类型复用；没有概念层面的 is-a 关系；不独立使用。
    被继承时子类在概念上是父类的一种 -> TechnicalClass。

  对象正例
    重试行为的可复用实现。
    对象池复用行为的 mixin。

  对象反例
    被继承时子类在概念上是父类的一种。
      应改为 TechnicalClass。

  创建
    arche create technical-mixin <name> --namespace <ns>

  结构字段
    namespace / file / module / source
    on / superclass / interfaces / mixins
    dependencies
    methods / types

  文档字段
    position
      名词性短语，说明复用了什么行为。
      正例：重试行为的可复用实现。
    responsibilities / non_responsibilities
      混入后在宿主中承担的职责和不承担的职责。
    features / non_features
      混入提供或不提供的能力。
    invariants
      宿主使用该 mixin 时必须维持的条件。
    usage
      如何把 mixin 应用到宿主、如何处理组合冲突的闭环示例。
    design_points
      每个设计点一个 topic，写组合、冲突和覆盖规则。
    不写 constraints。

TechnicalMethod
  定义
    定义在 TechnicalClass / TechnicalInterface / TechnicalMixin 内部的可调用成员。
    区别是归属，不是有没有参数或返回值。

  判断
    定义在类型内部 -> TechnicalMethod。
    不属于任何类型 -> TechnicalFunction。
    没有独立名字的代码片段 -> TechnicalPart。

  对象正例
    LoginService.login(String, String) -> Token。
    TokenValidator.validate(Token) -> Result。

  对象反例
    不属于任何类型的函数。
      应改为 TechnicalFunction。
    类内部的字段。
      应改为 TechnicalMember 或 TechnicalProperty。

  创建
    arche create technical-method --parent <type> \
      --prototype "<name>(<types>) -> <return>"

  结构字段
    source
    is_static / is_async / is_abstract / is_const
    overrides
    explicit_dependencies / implicit_dependencies
    arguments      ObjectList<TechnicalArgument>
    return_value   TechnicalReturnValue

  文档字段
    position
      动词性短语，说明这个方法做什么；入口函数等特殊作用函数可用名词短语写角色。
      正例：校验凭据并签发会话。
      反例：AuthService。
        问题：只是名字。
    usage
      方法层面的闭环示例：准备 -> 调用 -> 结果/清理。
    design_points
      每个设计点一个 topic，写参数契约之外的设计选择、并发、错误处理等。
    不写 responsibilities、features、invariants、constraints、design_principles。

TechnicalFunction
  定义
    不属于某个类型的函数或过程，挂在 TechnicalNamespace 下。

  判断
    不属于任何类型 -> TechnicalFunction。
    定义在类型内部 -> TechnicalMethod。

  对象正例
    解析配置文件的自由函数。
    计算哈希的独立函数。

  对象反例
    类内部的成员函数。
      应改为 TechnicalMethod。

  创建
    arche create technical-function <name> --namespace <ns>

  结构字段
    namespace / file / module / source
    is_async
    explicit_dependencies / implicit_dependencies
    arguments      ObjectList<TechnicalArgument>
    return_value   TechnicalReturnValue

  文档字段
    position
      动词性短语，说明这个函数做什么；入口函数等特殊作用函数可用名词短语写角色。
    usage
      函数层面的闭环示例。
    design_points
      每个设计点一个 topic。
    不写 responsibilities、features、invariants、constraints、design_principles。

TechnicalPart
  定义
    没有独立名字、但需要说明的代码片段。

  判断
    不能归为类型、方法或字段的代码块：宏展开体、配置块、特殊常量段等。
    有名字的类型或函数应创建对应对象。

  对象正例
    宏展开体。
    配置块。
    特殊常量段。

  对象反例
    有名字的类型或函数。
      应创建对应对象。
    类字段。
      应改为 TechnicalMember 或 TechnicalProperty。

  创建
    arche create technical-part <name> --parent <class> --source <path:start-end>

  结构字段
    source
    explicit_dependencies / implicit_dependencies
    members / methods / types

  文档字段
    Part 不关联设计文档，不建文档字段。

TechnicalVariable
  定义：不属于任何对象的全局变量。
  创建：作为对象挂到 TechnicalSystem / TechnicalModule / TechnicalNamespace。
  字段：source, ty, position。
  正例：模块级可配置开关。
  反例：类里的字段。
        应改为 TechnicalMember。

TechnicalConstant
  定义：常量。
  创建：挂到 TechnicalSystem / TechnicalModule / TechnicalNamespace /
        TechnicalClass / TechnicalStruct。
  字段：source, ty, position。
  正例：MaxRetries = 3。
  反例：运行时可变的全局值。
        应改为 TechnicalVariable。

TechnicalMember
  定义：TechnicalClass / TechnicalStruct 中的静态或非静态字段。
  创建：挂在 TechnicalClass / TechnicalStruct 下。
  字段：source, ty, is_static, is_const, position。
  正例：TechnicalClass 的缓存字段。
  反例：函数内的局部变量。
        局部变量不建对象。

TechnicalProperty
  定义：由 getter/setter 构成的属性。
  创建：挂在 TechnicalClass 下。
  字段：ty, getter, setter, position。
  正例：name 的 getName/setName 对。
  反例：普通业务方法。
        应改为 TechnicalMethod。
  注意：属性没有独立 source，getter/setter 各自是 SourceSpan。

TechnicalEnumValue
  定义：枚举的常量项。
  创建：挂在 TechnicalEnum 下。
  字段：source, position。
  正例：RED / GREEN / BLUE。
  反例：携带数据的 enum 变体。
        该 enum 应按 TechnicalClass 处理。

TechnicalArgument
  定义：TechnicalMethod / TechnicalFunction 的参数。
  创建：挂在 TechnicalMethod / TechnicalFunction 下。
  字段：source, ty, position。
  正例：login(String user, String password) 的两个参数。
  反例：方法内部临时变量。
        不建对象。
  注意：参数名不参与方法 hash；hash8 只取类型。

TechnicalReturnValue
  定义：TechnicalMethod / TechnicalFunction 的返回值。
  创建：挂在 TechnicalMethod / TechnicalFunction 下。
  字段：source, ty, position。
  正例：login -> Token。
  反例：无返回值方法。
        不创建 TechnicalReturnValue 对象。

RequirementUser
  定义：产品设计研究用的假想使用者（persona），不是真实人物，也不是系统账号。
  划分：产品角色不同 → 不同用户；同一角色下偏好/状况造成对产品有意义的差异化
        行为 → 不同用户。每种类型设计一个用户，并按产品定位标定 priority：
        core、primary、secondary、peripheral。优先级用于指导故事和交互设计：
        核心用户的需求优先满足，操作路径尽量短。
  创建：arche create requirement-user <role-id> --display-name <假想角色姓名>
  修改：arche update <qname> [与 create 相同的字段参数]
  完整字段、name / display_name / occupation、situations 和 needs 的写法见
  `arche help requirement-user`。

RequirementStory
  定义：用户故事。
  创建：arche create requirement-story <name>
  修改：arche update <qname> [与 create 相同的字段参数]
  字段：preconditions, frequency, users, protagonist, description, flow,
        functions, requirement。
  写法见 `arche help story`。

RequirementSystem / RequirementModule / RequirementFunction / RequirementInteraction
  定义：需求空间的分层：系统 -> 模块 -> 功能 -> 交互。
  创建：
    arche create requirement-system <name>
    arche create requirement-module <name> --parent <system>
    arche create requirement-function <name> --parent <module>
    arche create requirement-interaction <name> --parent <function>
  修改：arche update <qname> [与 create 相同的字段参数]
  判断：按需求层级归属，不按代码结构。
  写法见 `arche help requirement-system/module/function/interaction`。

Concept
  定义：需要明确定义的领域术语。
  创建：
    arche concept mkdir concept/domain/<dir>
    arche concept create concept/domain/<dir>/<key> \
      --name "<display-name>" --explanation "<definition>"
  判断：含义不显然、容易被不同读者理解成不同东西、或不同文档用法不一致时建立。
  正例：智能体：能够感知环境并采取行动的系统。
  反例：为“数据”“工具”这类日常词建概念。
        问题：没有定义价值，反而增加噪音。
