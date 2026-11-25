# 设计空间需求分析技能

## 目标

在 Arche 设计空间的 `requirement` 空间，把项目说明书、澄清 Issue 和现有源码转成结构化需求对象。

需求不再写锚点文档。不要使用 `document` 工具。

## 结构映射

- 领域模型 → `concept/requirement/...` 概念对象
- 用户 → `RequirementUser`：产品设计研究用的假想角色（persona），不是真实人物，禁止使用真实人物信息
- 故事 → `RequirementStory`
- 系统 → `RequirementSystem`：用户可感知的一个完整软件，例如“开发系统安卓端”
- 模块 → `RequirementModule`：围绕一个需求概念的一组功能，完成该概念的功能闭环，例如“账户管理”
- 功能 → `RequirementFunction`：用户与系统的一次有意义的交互，或系统自身的一次有意义的动作；必须产生有意义的系统数据变化或信息展示，例如“创建单据”
- 交互 → `RequirementInteraction`：用户的一次操作/观察或系统的一次通知，本身可能不产生系统变化，例如“填写单据名称”；若干交互组成一次功能
- UI → `RequirementInteraction.ui`

用户划分两原则：

1. 产品中角色不同的人是不同用户，例如普通用户和管理员、仓库管理员和快递员。
2. 同一角色下，因偏好或状况不同而产生对产品有意义的差异化行为的人，也是不同用户。例如学生客户更愿意比价和找优惠券，公司职员更愿意尽快完成交易。

对每一种用户类型设计一个用户，按类型特征设计人物，并根据产品定位为每个用户标定 `priority`：`core`（核心用户）、`primary`（主要用户）、`secondary`（次要用户）、`peripheral`（边缘用户）。优先级用于指导故事和交互设计：核心用户的需求优先满足，并尽量让其操作路径最短；core 用户的故事优先形成完整闭环，关键任务步骤、页面跳转和等待最少。

非功能需求和全局 UI 规范当前没有独立对象；能落到系统、模块或交互字段的就写入，不能落下的不要伪造对象。

## 分支与提交

当前任务分支固定为：

```text
requirements/<task-id>
```

准备：

```bash
cd /work/<project-id>/arche
arche fetch
arche checkout <design_space_branch>
arche pull
arche checkout requirements/<task-id> || arche checkout -b requirements/<task-id>
arche status
```

修改只在 `requirements/<task-id>` 分支进行，不直接写 `source`。

提交：

```bash
arche doc check
arche check
arche add -A
arche commit -m "Requirements for issue <issue-id>"
arche push
```

## 创建对象

```bash
arche create requirement-user "<角色 id>" --display-name "<假想角色姓名>"
arche create requirement-story "<故事名称>"
arche create requirement-system "<子系统名称>"
arche create requirement-module "<模块/功能集名称>" --parent <system-qname>
arche create requirement-function "<功能名称>" --parent <module-qname>
arche create requirement-interaction "<交互名称>" --parent <function-qname>
```

修改已有对象统一用 `arche update <qname>`，字段参数与 `arche create` 相同；
任意标量或引用字段用 `--field 字段=值` / `--ref 字段=目标1,目标2`。

领域概念：

```bash
arche concept mkdir concept/requirement/<dir>
arche concept create concept/requirement/<dir>/<key> \
  --name "<展示名>" --explanation "<定义>"
```

## 写字段

```bash
arche doc text set <qname> position "一句话定位"
arche doc list item add <qname> needs "需求项"
arche doc list item add <qname> preconditions "前置条件"
arche doc list item add <qname> boundary "边界"
arche doc mermaid set <qname> flow "graph TD; A-->B;"
arche doc html add <qname> ui "<p>页面结构</p>"
arche update <qname> --ref users=<user-qname>,<user-qname>
arche update <qname> --ref protagonist=<user-qname>
arche update <qname> --ref functions=<function-qname>,<function-qname>
```

字段按对象类型使用：

- User：字段、`display_name`、`situations`、`needs` 的完整写作规则以
  `arche help requirement-user` 为准；写用户前先执行该命令并遵守。
- Story：`preconditions`, `frequency`, `users`, `protagonist`, `description`, `flow`, `functions`, `requirement`
- System：`form`, `covered_users`, `closed_loop_goal`, `relationships`, `boundary`
- Module：`core_concept`, `included_concepts`, `concept_relations`, `boundary`
- Function：`initiator`, `system_change`, `preconditions`, `feedback`, `description`, `flow`
- Interaction：`description`, `ui`

## 用户故事

故事格式版本为 2：

- `users`：本故事所有参与用户的 qname 列表，单独字段；
- `protagonist`：主角，必须是 `users` 中的一个用户 qname；
- `description`：用连续普通段落写完整用户旅程——主角如何用一系列功能满足需求，其他用户如何配合，每一步发生什么；不要拆成 topic；
- `flow`：同一旅程的流程图，用 `arche doc mermaid set <story> flow "..."` 写入；
- `functions`：旅程中出现过的所有功能 qname 列表；
- `requirement`：DocumentList，每一项写清满足用户的什么需求。

`arche show <qname>` 检查字段，`arche check` 验证引用。`arche doc check` 必须通过。

需求完整性原则（必须保证）：

1. 每个用户需求都必须被一组故事完备覆盖，而不是只被一个故事覆盖。覆盖同一需求的故事要共同解决满足需求过程中的问题，并形成闭环：正常路径、需求前问题（信息不足、资格/权限不足、前置条件不满足）、进行中问题（缺货、失败、取消、超时、重复提交、并发冲突、外部依赖失败）、完成后问题（不满意、退货、退款、投诉、售后、撤销、再次使用）。每个问题都要有结果和恢复方式。
   例：“购物”需求不能只由“购买商品”故事满足；还需要“浏览商品/与卖家沟通”（信息不足）、“预购/缺货登记”（缺货）、“退货/退款”（不满意）等故事联合覆盖。
2. 故事中与系统直接交互的人必须是用户并出现在 `users`；交互媒介必须是功能并出现在 `functions`。
3. 每个功能都必须至少被一个故事覆盖。
4. 功能必须闭环：产生的系统变化必须能被其他功能消费，不能是无人消费的数据或无后续用途的展示。
5. 每个故事必须完备：覆盖本故事的正常路径和所有异常情况（拒绝、失败、取消、权限不足、输入非法、超时、重复提交、并发冲突、外部依赖失败等），并写清结果和恢复方式；单个故事无法覆盖需求全过程时，必须补配套故事。

## 依据

- 只写项目说明书、Issue、现有需求和源码能确认的事实。
- 子系统按用户可感知的软件划分；模块表示可独立管理的功能集；功能必须产生用户可观测变化。
- 每个故事至少关联一个用户和一个功能；`protagonist` 必须是 `users` 中的用户；没有归属功能的故事不创建。
- 不写代码实现、技术方案和数据库设计。

## 提交评审

写完后调用 `task_allowed_operations`，再调用 `task_operation`，使用返回的 `completed` 操作名提交文风审查。
