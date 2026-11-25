需求模块 RequirementModule

定义：围绕一个需求概念的一组功能，完成与该概念相关的功能闭环，例如“账户管理”。

判断：

- 模块以一个需求概念为中心，例如账户、库存、订单；
- 模块内的功能共同完成这个概念的生命周期或闭环；
- 模块不是代码目录、技术模块或单个功能。

反例：把“utils”“服务层”或一个孤立功能当作需求模块。

创建（一次传入全部字段）：

  arche create requirement-module --name <name> --parent <system> \
    --display-name "<模块名称>" \
    --position "<模块定位>"

修改：

  arche update <qname> \
    --display-name "<新模块名称>" \
    --position "<新模块定位>"

通用修改：

  arche update <qname> --field <字段>=<值>
  arche update <qname> --ref <引用字段>=<目标>[,<目标>...]

JSON 创建和更新：

  arche create requirement-module --name <name> --parent <system> --json module.json
  arche update <qname> --json module.json

  {
    "display_name": "账户管理",
    "position": "围绕账户概念，完成账户创建、授权、停用和删除的管理"
  }

`--json` 可以传文件路径、`-`（stdin）或直接传 JSON 对象；JSON 最后应用，优先于
普通参数。

字段顺序：

  display_name
  position
  functions

display_name  String
  含义：模块的用户可见名称，例如“账户管理”“库存管理”。
  写法：智能体必须为模块起一个简短、稳定的业务名称；用用户能理解的名称，不用
        qname key、技术模块名或“模块1”这类占位名。
  正例：账户管理；库存管理；订单履约。
  反例：account-management；RequirementModule；模块1；核心模块。
  约束：name 是 qname 的本地 key；display_name 用于目录树和页面展示，同一系统
        内保持唯一和稳定。

position  DocumentText
  含义：模块的简要定位，说明它围绕什么概念、完成什么闭环。
  写法：一句话写清“围绕<概念>，完成<生命周期或闭环>”；让人立刻明白这个模块
        负责什么、边界在哪里。不写功能清单、接口、数据库或实现方式。
  正例：围绕账户概念，完成账户创建、授权、停用和删除的管理。
        围绕订单概念，完成订单从创建到完成或取消的全过程。
  反例：包含创建账户、授权、停用三个功能。调用 AccountService。account 表管理。
  约束：functions 必须围绕 position 描述的概念和闭环；跨模块交接要与相关模块的
        position 一致。

functions  ObjectList<RequirementFunction>（自动）
  含义：这个模块包含的需求功能。
  写法：用 `arche create requirement-function --name <name> --parent <module>`
        创建，不手工写这个字段；按功能在概念闭环中的先后或重要性排列。
  正例：
    arche create requirement-function --name create-account \
      --parent requirement/system/warehouse-android/account-management
  反例：手工把功能 qname 写进字段；把交互建在模块下；一个功能横跨多个模块。
  约束：每个功能必须属于一个模块，并至少被一个故事覆盖；模块的功能合起来应能
        完成 position 描述的概念闭环。

示例：

  arche create requirement-module --name account-management \
    --parent requirement/system/warehouse-android \
    --display-name "账户管理" \
    --position "围绕账户概念，完成账户创建、授权、停用和删除的管理"
  arche create requirement-function --name create-account \
    --parent requirement/system/warehouse-android/account-management
