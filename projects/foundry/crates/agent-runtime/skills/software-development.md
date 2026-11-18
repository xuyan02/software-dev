# 软件开发流程技能

本技能覆盖 `software_development` 根流程的初始化和 `daily_check` 日常检查任务。

## software_development / setup

根流程只有一个初始化阶段。项目启动后：

1. 用 `trigger_list` 查看项目触发器。
2. 如果已经存在名称为“日常检查”的触发器，不要重复创建。
3. 否则用 `trigger_create` 创建：

```json
{
  "name": "日常检查",
  "description": "按计划运行代码-设计一致性检查和设计检查。",
  "workflow_id": "daily_check",
  "schedule": "daily@09:00",
  "input": {},
  "title_template": "日常检查",
  "description_template": "运行代码-设计一致性检查和设计检查。"
}
```

4. 创建成功或复用已有触发器后，发送 `setup:ready`。
5. 失败发送 `setup:failed`。

`daily@09:00` 使用 UTC。用户可以在项目页手动运行这个触发器，也可以改成 `manual` 或 `every:<minutes>`。

## daily_check / dispatch

日常检查任务由触发器创建。调度阶段：

1. `task_get` 读取当前任务 input；如果 input 中有 `scope`，传给两个检查子任务。
2. 用 `task_spawn_children` 创建两个子任务，`sequential = true`，保证不并发：

```json
{
  "sequential": true,
  "tasks": [
    {
      "workflow": "consistency_inspection",
      "title": "代码-设计一致性检查",
      "description": "检查现有代码与设计空间是否一致，并把问题上报为 Issue。",
      "input": { "scope": "<scope 或空>" }
    },
    {
      "workflow": "design_inspection",
      "title": "设计检查",
      "description": "检查设计文档、系统/模块边界、接口和依赖，并把问题上报为 Issue。",
      "input": { "scope": "<scope 或空>" }
    }
  ]
}
```

3. 发送 `dispatch:dispatched`；之后任务进入等待阶段，两个子任务都完成后自动结束。
4. 创建失败发送 `dispatch:failed`。

日常检查本身不直接检查、不修改代码或设计；检查由两个子任务的专门智能体完成，发现的问题通过 Issue 进入分诊。
