需求系统 RequirementSystem

定义：需求系统是用户能够单独感知、识别和使用的一个应用。它是不是一个系统，看用户
是否把它当成一个独立应用来理解，而不是看代码怎么拆分、服务怎么部署。

判断：

- 用户能不能接触到它。用户看不到、只能作为实现手段存在的东西，不构成需求系统。
  例如应用里引入的 SDK 只是实现细节，用户不会把它当成一个可以单独使用的应用。
- 用户会不会把它当成一个独立的事物。嵌在应用中的广告页只是应用的一部分，用户
  不会认为它是一个独立应用，因此不是需求系统。
- 同一个产品以用户能区分的不同形态出现时，这些形态是不同系统。例如同一电商平台
  的手机应用和网页版，用户会把它们当作两个入口，应分别建模为需求系统。
- 判断依据是用户的理解，不是技术实现。代码仓库、部署单元、服务拆分、技术分层
  都不能直接决定需求系统的边界。

反例：把代码仓库、部署单元、技术分层、内部服务、SDK、应用内页面当作需求系统。

创建（一次传入全部字段）：

  arche create requirement-system --name <name> \
    --display-name "<系统名称>" \
    --position "<系统定位>" \
    --app-form "<应用形态>" \
    --user <user-qname> \
    --user <another-user-qname>

修改（只传要改的字段，参数同创建）：

  arche update <qname> \
    --display-name "<新系统名称>" \
    --position "<新系统定位>" \
    --app-form "<新应用形态>" \
    --user <user-qname> \
    --user <another-user-qname>

通用修改：

  arche update <qname> --field <字段>=<值>
  arche update <qname> --ref <引用字段>=<目标>[,<目标>...]

JSON 创建和更新：

  arche create requirement-system --name <name> --json system.json
  arche update <qname> --json system.json

  {
    "display_name": "仓库管理系统",
    "position": "面向仓库管理员的移动收货系统",
    "app_form": "手机应用",
    "users": [
      "requirement/user/warehouse-admin",
      "requirement/user/platform-admin"
    ]
  }

`--json` 可以传文件路径、`-`（stdin）或直接传 JSON 对象。JSON 支持
`display_name`、`position`、`app_form`、`users` 等当前对象的全部字段；同时传
JSON 和其他参数时，JSON 后应用，优先于其他参数。复杂文档字段也可以用
`"fields": {"<field>": {"type": ...}}` 传入，格式同 `arche show --format json`。

字段顺序：

  display_name
  position
  app_form
  users
  modules

display_name  String
  含义：系统的用户可见名称，例如“仓库管理系统”“电商平台”。
  写法：智能体必须为系统起一个简短、稳定的业务名称；用用户能理解的名称，不用
        qname key、技术名、团队代号或“系统1”这类占位名。
  正例：仓库管理系统；无人化软件开发平台；电商平台。
  反例：warehouse-android；RequirementSystem；系统1；新系统。
  约束：name 是 qname 的本地 key；display_name 用于目录树和页面展示，同一项目
        内保持唯一和稳定。

position  DocumentText
  含义：这个系统是什么东西的简要定位，例如“无人化软件开发平台”“电商平台”。
  写法：用一句话写清产品类型、面向的领域和主要能力；标准是让人立刻明白它大概
        解决什么问题、有什么功能。不写模块清单、功能列表、技术栈或实现方式。
  正例：无人化软件开发平台。面向仓库管理员的移动收货系统。
  反例：一个系统。包含收货、上架、库存查询三个模块。基于 Rust 和 PostgreSQL。
  约束：与 app_form 分工：position 回答“它是什么、大概能做什么”，app_form 只
        回答“它以什么应用形态出现”。

app_form  DocumentText
  含义：系统的应用形态，例如“手机应用”“桌面应用”“Web 应用”“命令行工具”。
  写法：只写一个非常短的应用形态，不写使用者、产品定位、模块清单、技术栈或
        部署方式；使用者由 users 表达。
  正例：手机应用；桌面应用；Web 应用；命令行工具。
  反例：面向仓库管理员的安卓端应用；基于浏览器的仓库管理软件；使用 Rust 开发。
  约束：与 position 分工，position 写“是什么、大概能做什么”；应用形态应与
        故事中的界面一致。

users  Vec<SymbolHash>
  含义：会使用这个系统的 RequirementUser 列表。
  写法：写 RequirementUser 的 qname 列表，一个用户一个 qname；只列真正使用这个
        应用的人，不列只维护服务器、数据库或网络的人，也不写角色名称或自由文本。
  正例：
    requirement/user/warehouse-admin
    requirement/user/platform-admin
  反例：仓管员（应写用户 qname）；运维人员（只维护服务器）；所有员工（范围不清）。
  约束：每个 qname 必须指向 RequirementUser；这些用户应当出现在本系统模块下的
        故事 users 中；未出现在任何故事中的用户要说明原因或删除。

  修改：
    arche update requirement/system/<name> \
      --ref users=requirement/user/warehouse-admin,requirement/user/platform-admin

modules  ObjectList<RequirementModule>（自动）
  含义：这个系统包含的需求模块。
  写法：用 `arche create requirement-module --name <name> --parent <system>
        --display-name <模块名称>` 创建，不手工写这个字段；创建顺序按模块在用户
        旅程中的重要性或先后排列。
  正例：
    arche create requirement-module --name account-management \
      --parent requirement/system/warehouse-android --display-name "账户管理"
  反例：手工把模块 qname 写进字段；把代码目录或技术模块建成需求模块。
  约束：每个模块围绕一个需求概念，并挂在本系统下；模块的功能组成该系统的能力。

示例（一次传入全部字段）：

  arche create requirement-system --name warehouse-android \
    --display-name "仓库管理系统" \
    --position "面向仓库管理员的移动收货系统" \
    --app-form "手机应用" \
    --user requirement/user/warehouse-admin

JSON 示例：

  arche create requirement-system --name warehouse-android --json system.json

  {
    "display_name": "仓库管理系统",
    "position": "面向仓库管理员的移动收货系统",
    "app_form": "手机应用",
    "users": ["requirement/user/warehouse-admin"]
  }
