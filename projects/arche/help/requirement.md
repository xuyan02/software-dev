需求空间：

  requirement
    用户 user
    故事 story
    系统 system
      模块 module
        功能 function
          交互 interaction

下级帮助（写对象前先查对应帮助）：

  arche help requirement-user          用户：字段、situations/needs 写法、批量创建
  arche help story                     故事：字段、用户旅程、流程图、异常覆盖
  arche help requirement-system        系统：定位、字段、边界
  arche help requirement-module        模块：定位、字段、功能闭环
  arche help requirement-function      功能：定位、字段、交互拆分
  arche help requirement-interaction   交互：定位、字段、UI 描述

每类需求对象都有独立帮助，包含创建命令、全部字段和写作规则。创建或修改某类
对象前，先执行对应的 `arche help <主题>`，不要凭记忆填写字段。下级帮助中若还
列出更细的主题，继续用 `arche help <主题>` 查询。

对象定位：

  用户：为产品设计研究而设定的假想使用者（persona），不是真实人物，也不是账号。
  故事：从主角视角出发的一段完整用户旅程，用一系列功能满足需求。
  系统：用户可感知的一个完整软件，例如“开发系统安卓端”“仓库管理 Web 端”。
  模块：围绕一个需求概念的一组功能，完成该概念相关的功能闭环，例如“账户管理”。
  功能：用户与系统的一次有意义的交互，或系统自身的一次有意义的动作；标准是产生
        一次有意义的系统数据变化或信息展示，例如“创建单据”。
  交互：用户对系统的一次操作或观察、或系统的一次通知；它本身可能不产生有意义的
        系统变化，例如“填写单据名称”；若干次交互共同构成一次功能。

用户划分原则：

  1. 产品中处于不同角色的人是不同的用户，例如系统普通用户和系统管理员、仓库
     管理员和快递员。
  2. 同一角色下，因偏好或状况不同而产生对产品有意义的差异化行为的人，也是不同
     用户。例如学生客户更愿意比价和找优惠券，公司职员更愿意尽快完成交易。

  对每一个用户类型设计一个用户，并根据该类型特征设计人物；根据产品定位为每一个
  用户标定优先级：core、primary、secondary、peripheral。优先级是故事和交互设计
  的依据：核心用户的需求优先满足，核心用户的操作路径尽量短。详见
  `arche help requirement-user`。

需求完整性原则（必须保证，评审逐条检查，不满足即打回）：

  1. 每个用户需求都必须被一组故事完备覆盖，而不是只被一个故事覆盖。覆盖同一需求
     的全部故事必须共同解决满足需求过程中可能出现的各种问题，并形成闭环。故事集合
     至少覆盖：
       - 正常完成路径；
       - 需求发生前的问题：信息不足、不了解选项、资格或权限不足、前置条件不满足；
       - 需求进行中的问题：缺货、失败、取消、超时、重复提交、并发冲突、外部依赖
         失败、操作错误；
       - 需求完成后的后续问题：不满意、退货、退款、投诉、售后、撤销、再次使用；
       - 每个问题都要有处理结果和恢复方式，不留下无人处理的状态。
     反例：用户的“购物”需求不能只由“购买商品”故事满足；还需要“浏览商品”“与
     卖家沟通”（解决信息不足）、“预购/缺货登记”（解决缺货）、“退货/退款”
     （解决不满意）等故事联合覆盖。
  2. 每个故事中与系统直接交互的人必须是用户，并且出现在该故事的 users 中；故事中
     每次用户操作或系统交互的媒介必须是功能，并且出现在该故事的 functions 中。
  3. 每个功能都必须至少被一个故事覆盖。
  4. 功能必须闭环：功能产生的系统变化必须能被其他功能消费；只产生无人消费的数据、
     或只展示不可继续使用的信息，不构成完整功能。
  5. 每个故事必须完备：除正常路径外，必须覆盖本故事涉及的异常情况，包括拒绝、失败、
     取消、权限不足、输入非法、超时、重复提交、并发冲突、外部依赖失败等；异常路径
     要写结果和恢复方式。单个故事无法覆盖需求全过程时，必须用配套故事补齐。

需求覆盖检查方法（逐条 needs 执行）：

  1. 列出包含该用户的全部故事；
  2. 在每个故事的 requirement 中找该 needs 的对应条目，没有就补；
  3. 检查这些故事是否覆盖正常路径、需求前问题、进行中问题和完成后问题；
  4. 检查每个问题是否有结果和恢复方式，故事之间是否闭环；
  5. 缺哪类问题就补故事或补现有故事，不能用一个故事强行覆盖整条需求。

创建：

  arche create requirement-user <role-id> --display-name <假想角色姓名>
  arche create requirement-story <name>
  arche create requirement-system <name>
  arche create requirement-module <name> --parent <system>
  arche create requirement-function <name> --parent <module>
  arche create requirement-interaction <name> --parent <function>

修改：

  arche update <qname> [与 create 相同的字段参数]
  arche update <qname> --field <字段>=<值> --ref <引用字段>=<目标>[,<目标>...]

`arche update` 只修改传入的字段。需求用户可以直接使用与 create 相同的
`--display-name`、`--age`、`--occupation`、`--priority`、`--situation`、
`--hobby`、`--need`、`--story` 参数。

每个字段的写作说明都在各自的下级帮助中，统一按“含义 / 写法 / 正例 / 反例 /
约束”五项编写。下面是字段速查表；写字段前先执行对应对象的 `arche help`，
不要只看速查表。

字段：

RequirementUser (v2)
  display_name, age, occupation: String
  situations, hobbies, needs: DocumentList
  priority: core / primary / secondary / peripheral
  stories: Vec<SymbolHash>

  完整字段、`situations` 和 `needs` 的写作说明见 `arche help requirement-user`。

RequirementStory (v2)
  preconditions: DocumentList
  frequency: u8
  users, functions: Vec<SymbolHash>
  protagonist: SymbolHash
  description: DocumentParagraphList
  flow: DocumentMermaid
  requirement: DocumentList

RequirementSystem
  modules ObjectList<RequirementModule>
  form DocumentText
  covered_users DocumentList
  closed_loop_goal, relationships DocumentParagraphList
  boundary DocumentList

RequirementModule
  functions ObjectList<RequirementFunction>
  core_concept DocumentText
  included_concepts DocumentList
  concept_relations DocumentParagraphList
  boundary DocumentList

RequirementFunction
  flow DocumentMermaid
  interactions ObjectList<RequirementInteraction>
  initiator, feedback DocumentText
  system_change, preconditions, description DocumentParagraphList

RequirementInteraction
  description, ui DocumentParagraphList
