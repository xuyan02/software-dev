需求模块 RequirementModule

定义：围绕一个需求概念的一组功能，完成与该概念相关的功能闭环，例如“账户管理”。

判断：

- 模块以一个需求概念为中心，例如账户、库存、订单；
- 模块内的功能共同完成这个概念的生命周期或闭环；
- 模块不是代码目录、技术模块或单个功能。

反例：把“utils”“服务层”或一个孤立功能当作需求模块。

创建：

  arche create requirement-module <name> --parent <system>

修改：

  arche update <qname> --field <字段>=<值>
  arche update <qname> --ref <引用字段>=<目标>[,<目标>...]

`arche update` 只修改传入的字段，create 支持的字段参数同样适用；文档字段的正文
仍用 `arche doc` 命令修改。

字段顺序：

  functions
  core_concept
  included_concepts
  concept_relations
  boundary

字段写作说明（统一格式）：

  含义  这个字段回答什么问题。
  写法  内容粒度、顺序、语气和格式要求。
  正例  可以直接对照采用的例子。
  反例  常见错误，出现即应改写。
  约束  与其他字段、对象或检查的关系。

functions  ObjectList<RequirementFunction>（自动）
  含义：这个模块包含的需求功能。
  写法：用 `arche create requirement-function --parent <module>` 创建，不手工写
        这个字段；按功能在概念闭环中的先后或重要性排列。
  正例：
    arche create requirement-function create-account \
      --parent requirement/system/warehouse-android/account-management
  反例：手工把功能 qname 写进字段；把交互建在模块下；一个功能横跨多个模块。
  约束：每个功能必须属于一个模块，并至少被一个故事覆盖；模块的功能合起来应能
        完成 core_concept 描述的概念闭环。

core_concept  DocumentText
  含义：一句话定义模块中心的概念及其管理范围。
  写法：一句话，写成“<概念>从<起点>到<终点>的管理”；名称必须是用户能理解的领域
        概念，不用技术模块名。
  正例：账户从创建、授权、停用到删除的管理。订单从创建到完成或取消的全过程。
  反例：账户服务。用户中心后端。调用 AccountService 的增删改查。
  约束：included_concepts 和 functions 都必须围绕这个概念；concept_relations 要
        说明它的状态如何流转。

included_concepts  DocumentList
  含义：模块涉及的从属概念。
  写法：每条写一个概念名称，例如“账户”“角色”“权限”“审计记录”；只列本模块
        功能真正创建、修改或消费的概念。
  正例：账户；角色；权限；审计记录。
  反例：数据库；表；缓存；消息队列。
  约束：每个从属概念应能在 concept_relations 中找到关系，或被某个功能使用；
        没有用到的概念删除。

concept_relations  DocumentParagraphList
  含义：模块内概念之间的关系和闭环。
  写法：用段落写清谁创建谁、什么状态转换、哪些信息在概念之间流动、闭环在哪里
        结束；不写接口、数据库表或代码调用。
  正例：账户创建后授予角色，角色聚合权限；账户停用后保留审计记录，审计记录可
        按账户查询。角色删除前必须确认没有启用中的账户引用它。
  反例：account 表通过 role_id 关联 role 表。调用 RoleService 查询权限。
  约束：关系应覆盖 core_concept 的生命周期，并解释 functions 产生的系统变化如何
        被其他功能消费；异常状态转换也要写明。

boundary  DocumentList
  含义：这个模块的能力边界。
  写法：每条写“包含哪些功能、明确不包含什么、由哪个模块或系统承接”；用业务能力
        描述，不按代码目录划分。
  正例：包含账户创建、授权和停用；不包含登录认证，登录认证属于身份模块。
  反例：包含 account 目录。不包含 common 模块。
  约束：与系统的 boundary 不冲突；跨模块交接要与相关模块的 boundary 对应。

示例：

  arche create requirement-module account-management \
    --parent requirement/system/warehouse-android
  arche doc text set requirement/system/warehouse-android/account-management core_concept \
    "账户从创建、授权、停用到删除的管理。"
  arche doc list item add requirement/system/warehouse-android/account-management \
    included_concepts "账户"
  arche doc list item add requirement/system/warehouse-android/account-management \
    boundary "包含账户创建、授权和停用；不包含登录认证。"
