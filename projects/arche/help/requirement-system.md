需求系统 RequirementSystem

定义：用户可感知的一个完整软件，例如“开发系统安卓端”“仓库管理 Web 端”。

判断：

- 用户能把它当作一个软件来使用和感知；
- 它对外提供完整价值，不是内部模块、服务、仓库或数据库；
- 一个系统可以继续拆成多个需求模块。

反例：把代码仓库、部署单元、技术分层或内部服务当作需求系统。

创建：

  arche create requirement-system <name>

修改：

  arche update <qname> --field <字段>=<值>
  arche update <qname> --ref <引用字段>=<目标>[,<目标>...]

`arche update` 只修改传入的字段，create 支持的字段参数同样适用；文档字段的正文
仍用 `arche doc` 命令修改。

字段顺序：

  modules
  form
  covered_users
  closed_loop_goal
  relationships
  boundary

字段写作说明（统一格式）：

  含义  这个字段回答什么问题。
  写法  内容粒度、顺序、语气和格式要求。
  正例  可以直接对照采用的例子。
  反例  常见错误，出现即应改写。
  约束  与其他字段、对象或检查的关系。

modules  ObjectList<RequirementModule>（自动）
  含义：这个系统包含的需求模块。
  写法：用 `arche create requirement-module --parent <system>` 创建，不手工写这个
        字段；创建顺序按模块在用户旅程中的重要性或先后排列。
  正例：
    arche create requirement-module account-management \
      --parent requirement/system/warehouse-android
  反例：手工把模块 qname 写进字段；把代码目录或技术模块建成需求模块。
  约束：每个模块围绕一个需求概念，并挂在本系统下；模块的功能组成该系统的能力。

form  DocumentText
  含义：一句话说明这个软件是什么、给谁用。
  写法：一句话，包含软件形态和使用者；不写模块清单、技术栈、部署方式或实现步骤。
  正例：面向仓库管理员的安卓端软件。面向平台管理员的 Web 管理端。
  反例：由账户、库存、订单三个模块组成。使用 Rust 和 PostgreSQL 实现。
  约束：使用的用户应与 covered_users 一致；软件形态应与故事中的界面一致。

covered_users  DocumentList
  含义：使用这个系统的用户或角色。
  写法：每条写一个用户名称或 RequirementUser qname；只列真正使用这个软件的人，
        不列只维护服务器、数据库或网络的人。
  正例：仓管员；平台管理员；requirement/user/warehouse-admin。
  反例：运维人员（只维护服务器）；所有员工（范围不清）；系统本身。
  约束：这些用户应当出现在本系统模块下的故事 users 中；未出现在任何故事中的用户
        要说明原因或删除。

closed_loop_goal  DocumentParagraphList
  含义：这个软件为用户完成的完整闭环。
  写法：用一段话写清触发、关键动作、结果和结束状态；以用户可感知的价值收尾；
        不写成功能列表。
  正例：仓管员从收货开始登记，经过验收、上架，系统记录批次、库位和库存变化，
        完成后库存可查询，差异可追溯。
  反例：包含创建单据、审核单据、查询库存三个功能。使用消息队列保证最终一致。
  约束：闭环中的关键动作应能对应到模块和功能；结果应能被后续故事消费。

relationships  DocumentParagraphList
  含义：这个系统与其他系统的关系。
  写法：用段落写清输入、输出、共享数据、权限关系或人工交接；没有外部关系时省略；
        不写接口协议和部署拓扑。
  正例：本系统从 ERP 接收商品基础数据，向财务系统输出每日出入库汇总；与移动端
        共享登录态。
  反例：调用 /api/v1/items 接口。依赖 Redis 和 Kafka。
  约束：外部系统提供或消费的数据，应在相关功能的前置条件、system_change 或
        boundary 中有对应描述。

boundary  DocumentList
  含义：这个系统的能力边界。
  写法：每条写“包含什么、明确不包含什么、由哪个系统承接”；用用户可感知的能力
        描述，不按代码目录或团队分工划分。
  正例：包含收货、上架和库存查询；不包含财务结算，财务结算由财务系统承接。
  反例：包含 src/warehouse 目录。不包含 utils 模块。
  约束：与 modules 的 boundary 一致；跨系统交接要与 relationships 对应。

示例：

  arche create requirement-system warehouse-android
  arche doc text set requirement/system/warehouse-android form \
    "面向仓库管理员的安卓端软件。"
  arche doc list item add requirement/system/warehouse-android covered_users \
    "仓管员"
  arche doc import requirement/system/warehouse-android system.json
