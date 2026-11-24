# 设计空间需求分析技能

## 目标

在 Arche 设计空间的 `requirement` 空间，把项目说明书、澄清 Issue 和现有源码转成结构化需求对象。

需求不再写锚点文档。不要使用 `document` 工具。

## 结构映射

- 领域模型 → `concept/requirement/...` 概念对象
- 用户 → `RequirementUser`
- 故事 → `RequirementStory`
- 子系统 → `RequirementSystem`
- 功能集 → `RequirementModule`
- 功能 → `RequirementFunction`
- 交互 → `RequirementInteraction`
- UI → `RequirementInteraction.ui`

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
arche create requirement-user "<真实姓名>"
arche create requirement-story "<故事名称>"
arche create requirement-system "<子系统名称>"
arche create requirement-module "<模块/功能集名称>" --parent <system-qname>
arche create requirement-function "<功能名称>" --parent <module-qname>
arche create requirement-interaction "<交互名称>" --parent <function-qname>
```

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
arche doc topic add <qname> description "说明"
arche doc mermaid set <qname> flow "graph TD; A-->B;"
arche doc html add <qname> ui "<p>页面结构</p>"
```

字段按对象类型使用：

- User：`real_name`, `age`, `occupation`, `situations`, `hobbies`, `needs`, `priority`
- Story：`preconditions`, `frequency`, `description`, `requirement`
- System：`form`, `covered_users`, `closed_loop_goal`, `relationships`, `boundary`
- Module：`core_concept`, `included_concepts`, `concept_relations`, `boundary`
- Function：`initiator`, `system_change`, `preconditions`, `feedback`, `description`, `flow`
- Interaction：`description`, `ui`

`arche show <qname> --format json` 检查字段。`arche doc check` 必须通过。

## 依据

- 只写项目说明书、Issue、现有需求和源码能确认的事实。
- 子系统按用户可感知的软件划分；模块表示可独立管理的功能集；功能必须产生用户可观测变化。
- 每个故事至少关联一个用户和一个功能；没有归属的功能不创建。
- 不写代码实现、技术方案和数据库设计。

## 提交评审

写完后调用 `task_allowed_operations`，再调用 `task_operation`，使用返回的 `completed` 操作名提交文风审查。
