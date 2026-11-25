需求功能 RequirementFunction

定义：用户与系统的一次有意义的交互，或系统自身的一次有意义的动作。标准是产生
一次有意义的系统数据变化或信息展示，例如“创建单据”。

判断：

- 一次功能必须产生有意义的系统数据变化或信息展示；
- 功能可以由若干次交互组成，但交互本身不产生完整业务结果；
- 功能属于一个需求模块，必须能追溯到模块的中心概念。

反例：“填写单据名称”本身不改变系统数据，是交互，不是功能。

优先级与功能设计：

- core 用户的核心功能优先设计，并优先形成闭环；
- core 用户执行核心功能时，flow 中的步骤、分支和等待应尽量少；
- 低频用户或低频场景的功能可以放到次级入口，不要拉长 core 用户的核心流程。

创建：

  arche create requirement-function <name> --parent <module>

修改：

  arche update <qname> --field <字段>=<值>
  arche update <qname> --ref <引用字段>=<目标>[,<目标>...]

`arche update` 只修改传入的字段，create 支持的字段参数同样适用；文档字段的正文
仍用 `arche doc` 命令修改。

字段顺序：

  flow
  interactions
  initiator
  system_change
  preconditions
  feedback
  description

字段写作说明（统一格式）：

  含义  这个字段回答什么问题。
  写法  内容粒度、顺序、语气和格式要求。
  正例  可以直接对照采用的例子。
  反例  常见错误，出现即应改写。
  约束  与其他字段、对象或检查的关系。

flow  DocumentMermaid
  含义：功能内部从触发到系统变化和信息反馈的流程。
  写法：画一张 Mermaid flowchart 或 sequenceDiagram；节点对应交互或处理步骤；
        每个条件分支写清条件和结果；失败、取消、权限不足、校验不通过等异常路径
        必须画出来；不画实现模块、接口或数据表。
  正例：
    arche doc mermaid set <function-qname> flow \
      "flowchart TD
         A[打开新建单据] --> B[填写单头和明细]
         B --> C[提交]
         C --> D{校验通过}
         D -- 是 --> E[生成待审核单据]
         D -- 否 --> F[显示错误并保留输入]
         E --> G[新单据出现在待审核列表]"
  反例：只画正常路径；节点写“调用 Service”“写数据库”；一张图里混入其他功能。
  约束：节点应覆盖 description 的关键步骤；与 interactions 对应；异常结果必须
        在 description 中有文字说明。

interactions  ObjectList<RequirementInteraction>（自动）
  含义：组成这个功能的交互。
  写法：用 `arche create requirement-interaction --parent <function>` 创建，不手工
        写这个字段；按执行顺序排列。
  正例：
    arche create requirement-interaction fill-document-name \
      --parent requirement/system/warehouse-android/account-management/create-document
  反例：把整个功能拆成一次交互；把交互建在模块下；交互之间没有顺序关系。
  约束：若干交互共同构成一次功能；交互合起来应产生 system_change 描述的结果。

initiator  DocumentText
  含义：谁或什么触发这个功能。
  写法：一句话，写用户名称、系统任务或外部系统；与故事 users 和系统关系保持一致。
  正例：仓管员。系统定时任务。ERP 回调。
  反例：用户（太泛）；系统（不知道是哪个系统）；管理员点击按钮（这是操作，不是
        触发者）。
  约束：如果触发者是人，必须是 RequirementUser 并在相关故事 users 中；外部系统
        应在系统的 relationships 中出现。

system_change  DocumentParagraphList
  含义：这个功能产生的有意义的系统数据变化或信息展示。
  写法：用段落写清新增、修改、删除或展示了什么数据或状态，以及谁能在哪里看到；
        这是功能成立的核心判据。
  正例：创建一条待审核单据，写入创建人、时间和明细；提交后新单据出现在待审核
        列表并显示单号。
  反例：界面刷新。用户点击提交。写入一条日志（如果日志没有被任何功能消费）。
  约束：变化必须能被其他功能消费；如果没有任何有意义的系统变化或信息展示，应
        拆成 RequirementInteraction，而不是建功能。

preconditions  DocumentParagraphList
  含义：功能开始前必须成立的状态和权限。
  写法：用段落写清必要的数据状态、用户权限、前置功能结果；没有时省略；不写操作
        步骤。
  正例：单据类型已经配置；当前用户具有“创建单据”权限；商品基础数据已从 ERP
        同步。
  反例：用户打开页面。系统正在运行。先点击新建按钮。
  约束：与故事的 preconditions 一致；前置功能产生的状态应能对应到某个
        system_change。

feedback  DocumentText
  含义：用户或调用方最终看到的结果。
  写法：一句话，写可观察的结果、位置和关键信息；失败时也应有对应反馈，可在
        description 中展开。
  正例：新单据出现在待审核列表，并显示单号。提交失败时表单保留输入并显示原因。
  反例：操作成功。系统返回 200。数据已保存（用户看不到）。
  约束：与 system_change、flow 的结束节点一致；不要只写技术状态码。

description  DocumentParagraphList
  含义：功能的完整文字说明。
  写法：用段落依次写触发、参与的交互、系统处理、结果和异常；正常路径和异常路径
        都要覆盖；用业务语言，不写实现接口、数据库或内部算法。
  正例：仓管员在待审核列表点击“新建单据”，依次填写单头、明细并提交。系统校验
        必填项和商品有效性；校验通过后生成待审核单据并返回列表。校验失败时定位
        到第一个错误字段并保留已填内容；无权限时拒绝提交并提示联系管理员。
  反例：调用 DocumentService.create，写入 document 表并发送 MQ 消息。只写正常
        路径，不写拒绝、失败、取消和超时。
  约束：覆盖 flow 的每个分支；异常结果要写清触发条件、系统结果、用户看到什么、
        如何恢复；与 interactions 和 system_change 对应。

覆盖与闭环要求：

- 每个功能都必须至少被一个故事覆盖，并出现在该故事的 functions 中；
- 功能产生的系统变化必须能被其他功能消费：后续功能可以在前置条件或描述中引用
  这次变化产生的数据、状态或信息；
- 只产生无人消费的数据、或只展示不可继续使用的信息，不构成完整功能；
- 功能必须包含异常处理：拒绝、失败、取消、权限不足、输入非法、超时、重复提交、
  并发冲突、外部依赖失败等；异常结果写进 flow 和 description。

示例：

  arche create requirement-function create-document \
    --parent requirement/system/warehouse-android/account-management
  arche doc text set requirement/system/warehouse-android/account-management/create-document \
    initiator "仓管员"
  arche doc text set requirement/system/warehouse-android/account-management/create-document \
    feedback "新单据出现在待审核列表，并显示单号。"
  arche doc list item add requirement/system/warehouse-android/account-management/create-document \
    system_change "创建一条待审核单据，写入创建人、时间和明细。"
  arche doc mermaid set requirement/system/warehouse-android/account-management/create-document \
    flow "flowchart TD; A[打开新建单据] --> B[填写单头和明细]; B --> C[提交]; C --> D{校验通过}; D -- 是 --> E[生成待审核单据]; D -- 否 --> F[显示错误]"
