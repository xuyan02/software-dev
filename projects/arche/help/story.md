用户故事 RequirementStory（格式版本 2）

定义：从主角视角出发的一段完整用户旅程。故事说明主角为什么要做这件事、按什么
顺序使用哪些功能、其他用户如何配合、每一步看到什么结果。

故事中的用户都是 RequirementUser 假想角色，姓名只是代称；不得把真实人物写成
故事角色，也不得使用真实人物的姓名、身份或经历。

优先级与故事设计：

- 主角是 core 用户的故事优先设计，优先形成完整闭环；
- 同一需求下，先保证 core 用户的旅程完整，再扩展 primary、secondary、
  peripheral 用户的旅程；
- 不要为了覆盖低频用户或低频场景，把 core 用户的主旅程拉长。

创建：

  arche create requirement-story <name>

qname 为 requirement/story/<name>，name 使用 kebab-case，例如
first-time-setup、decision-traceability。

修改：

  arche update requirement/story/<name> --field <字段>=<值>
  arche update requirement/story/<name> --ref <引用字段>=<目标>[,<目标>...]

`arche update` 只修改传入的字段，create 支持的字段参数同样适用。`--ref` 用来
替换引用列表，例如 `--ref users=requirement/user/a,requirement/user/b`。

字段顺序：

  preconditions
  frequency
  users
  protagonist
  description
  flow
  functions
  requirement

按此顺序填写。空字段直接省略，不要写占位内容；不要调换顺序。

字段写作说明（统一格式）：

  含义  这个字段回答什么问题。
  写法  内容粒度、顺序、语气和格式要求。
  正例  可以直接对照采用的例子。
  反例  常见错误，出现即应改写。
  约束  与其他字段、对象或检查的关系。

preconditions  DocumentList
  含义：故事开始前已经成立、可以核对的条件或状态。
  写法：每条写一个事实，用陈述句，写“已经……/已具备……/已配置……”；不写操作
        步骤、目的或期望；与本旅程无关的条件不要写。
  正例：部署主机已经安装平台。项目已经创建并配置源码仓库。首个管理员已经完成
        首次登录。
  反例：用户想初始化平台（这是目的，不是前置条件）。管理员点击“开始”。系统
        运行正常（无法核对）。
  约束：与 users 的权限、functions 的 preconditions 一致；故事中不要再次完成这些
        条件。

frequency  u8
  含义：故事发生的频率等级，用于排序和版本安排。
  写法：0 表示未分级；1 表示最高频，数字越大频率越低；同一项目使用同一套等级
        含义；没有明确分级时省略（默认 0）。
  正例：1（每天多次）；3（每周一次）。
  反例：经常；高；重要。
  约束：只作排序参考，不参与完整性校验。

users  Vec<SymbolHash>
  含义：参与本故事的所有用户，包括主角和配合者。
  写法：写 RequirementUser qname 列表；只列真正与系统交互、被系统通知或参与人工
        交接的人；至少包含主角；按出场顺序排列。
  正例：
    arche update requirement/story/<name> \
      --ref users=requirement/user/<user-1>,requirement/user/<user-2>
  反例：只列主角、漏掉配合者；列无关角色；把部门、系统或外部服务写进 users。
  约束：protagonist 必须在此列表中；description 中出现的每个人都必须在此列表中；
        arche check 会校验。

protagonist  SymbolHash
  含义：本故事要满足其需求的主角。
  写法：写单个 RequirementUser qname，必须包含在 users 中；主角是需求被满足的人，
        不是旁观者、审批人或运维人员；一个故事只有一个主角。
  正例：
    arche update requirement/story/<name> \
      --ref protagonist=requirement/user/<user-1>
  反例：写两个主角；写不在 users 中的人；把执行操作的运维人员当主角。
  约束：必须属于 users；主角的 needs 应能在 requirement 字段中找到对应条目。

description  DocumentParagraphList
  含义：按时间顺序展开的完整用户旅程。
  写法：只放 DocumentText 普通段落，不用 topic、列表或代码块代替旅程。第一段写
        主角、触发场景和目标；中间每段写一个阶段，回答“谁、在什么情况下、用了
        哪个功能、其他用户如何配合、看到什么可观察结果”；最后一段写旅程结束后的
        状态。正常路径和异常路径都要写，用业务语言，不写实现细节。
  正例：
    平台管理员张伟首次部署平台后打开初始化页，填写管理员账户并提交。
    系统创建账户后让张伟登录并进入工作台；张伟随后在工作台邀请李娜，
    李娜接受邀请后可以登录。邀请失败时页面显示原因，张伟可以修改后重试。
  反例：用 topic 拆成“用户 / 场景 / 期望”；只列功能清单；写“调用
        CreateAdminService 并写入 users 表”；只写正常路径。
  约束：description 中的人物都在 users 中，功能都在 functions 中；与 flow 的节点
        和分支一一对应。

flow  DocumentMermaid
  含义：与 description 一一对应的旅程流程图。
  写法：整条旅程只画一张图，使用 Mermaid flowchart 或 sequenceDiagram；节点名称
        使用 description 中的步骤名；每个条件分支写清条件和结果；异常路径必须显式
        画出；不画实现模块、接口或数据表。
  正例：
    arche doc mermaid set requirement/story/<name> flow \
      "flowchart TD
         A[打开初始化页] --> B[填写管理员账户]
         B --> C[提交]
         C --> D{账户是否有效}
         D -- 是 --> E[登录并进入工作台]
         D -- 否 --> F[显示错误并返回表单]
         E --> G[邀请其他管理员]
         G --> H{邀请是否成功}
         H -- 是 --> I[被邀请人接受并登录]
         H -- 否 --> J[显示失败原因并允许重试]"
  反例：一张图拆成多张；只画正常路径；节点名与 description 不一致；把数据库表和
        内部服务画进流程图。
  约束：节点覆盖 description 的关键步骤，不增加未写过的步骤；异常分支与
        description 的异常描述一致。

functions  Vec<SymbolHash>
  含义：本故事中出现过的所有功能。
  写法：写 RequirementFunction qname 列表，覆盖 description 和 flow 中每次有意义
        的系统动作；不包含旅程里没有出现的功能；按出现顺序排列。
  正例：
    arche update requirement/story/<name> \
      --ref functions=requirement/system/<system>/<module>/<function>,requirement/system/<system>/<module>/<other-function>
  反例：漏掉配合者使用的功能；把交互 qname 当功能；列未出现的功能凑数。
  约束：每个功能必须至少被一个故事覆盖；arche check 会校验引用类型。

requirement  DocumentList
  含义：本故事满足用户的哪些需求，以及解决该需求的哪一部分问题。
  写法：每条一个用户目标，写成“满足<用户><目标>的需求”或“解决<用户><目标>
        过程中<问题>的需求”；与对应用户的 needs 能对应；可以补充验收结果，
        但不复述操作步骤。
  正例：
    满足平台管理员张伟完成首次初始化并进入工作台的需求。
    满足后续管理员李娜由首个账户邀请加入、无需再次初始化的需求。
    解决管理员邀请失败后无法恢复、只能重新初始化的问题。
  反例：调用创建账户接口。管理员点击提交按钮。
  约束：一条 needs 通常由多个故事共同覆盖；本故事只写自己承担的部分。故事涉及
        用户的每条 needs 都应能在相关故事集合中找到条目；找不到就补故事或补
        条目。

完整示例：

  arche create requirement-user platform-admin --display-name 张伟 --occupation 平台管理员
  arche create requirement-user invited-admin --display-name 李娜 --occupation 受邀管理员
  arche create requirement-system 治理
  arche create requirement-module 初始化 --parent requirement/system/治理
  arche create requirement-function 创建首个账户 --parent \
    requirement/system/治理/初始化
  arche create requirement-function 邀请管理员 --parent \
    requirement/system/治理/初始化
  arche create requirement-story first-time-setup

  arche update requirement/story/first-time-setup \
    --ref users=requirement/user/<管理员-id>,requirement/user/<受邀管理员-id>
  arche update requirement/story/first-time-setup \
    --ref protagonist=requirement/user/<管理员-id>
  arche update requirement/story/first-time-setup \
    --ref functions=requirement/system/治理/初始化/创建首个账户,requirement/system/治理/初始化/邀请管理员
  arche doc mermaid set requirement/story/first-time-setup flow \
    "flowchart TD
       A[打开初始化页] --> B[填写管理员账户]
       B --> C[提交]
       C --> D{账户是否有效}
       D -- 是 --> E[登录并进入工作台]
       D -- 否 --> F[显示错误并返回表单]
       E --> G[邀请其他管理员]
       G --> H[被邀请人接受并登录]"

description 和 requirement 用 `arche doc import` 一次写入（普通段落）：

  {
    "fields": {
      "description": {
        "type": "DocumentParagraphList",
        "items": [
          {"type": "DocumentText",
           "content": [{"type": "text", "text": "平台管理员张伟首次部署平台后打开初始化页，填写管理员账户并提交。"}]},
          {"type": "DocumentText",
           "content": [{"type": "text", "text": "系统创建账户后让张伟登录并进入工作台；张伟随后在工作台邀请李娜，李娜接受邀请后可以登录。"}]}
        ]
      },
      "requirement": {
        "type": "DocumentList",
        "items": [
          {"type": "text", "text": "满足平台管理员张伟完成首次初始化并进入工作台的需求。"},
          {"type": "text", "text": "满足后续管理员李娜由首个账户邀请加入、无需再次初始化的需求。"}
        ]
      }
    }
  }

  arche doc import requirement/story/first-time-setup story.json

故事完整性要求：

- 故事中每个与系统直接交互的人都必须是用户，并出现在 users 中；
- 每次操作或系统交互的媒介都必须是功能，并出现在 functions 中；
- 故事必须被一个功能闭环支撑：主角使用的功能产生的系统变化要能被后续功能消费；
- 必须写全异常情况：拒绝、失败、取消、权限不足、输入非法、超时、重复提交、
  并发冲突、外部依赖失败等；每种异常写清触发条件、系统结果、用户看到什么、
  如何恢复；
- 只写正常路径的故事视为不完整。
- 单个故事只承担需求的一部分。一条需求必须由一组故事完备覆盖：除正常路径外，
  还要覆盖需求发生前、进行中和完成后的各种问题，并形成闭环。
  例：“购物”需求不能只由“购买商品”故事满足；还需要“浏览商品/与卖家沟通”
  解决信息不足，“预购/缺货登记”解决缺货，“退货/退款”解决不满意。只有这些
  故事联合起来，才算完备覆盖“购物”需求。

检查：

  arche show requirement/story/<name>
  arche check
  arche doc check

  `arche check` 会验证 users 都是 RequirementUser、functions 都是
  RequirementFunction、protagonist 在 users 中。
