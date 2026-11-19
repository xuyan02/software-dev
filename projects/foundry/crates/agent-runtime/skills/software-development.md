# 软件开发流程技能

本技能覆盖 `software_development` 根流程和 `daily_check` 日常检查任务。

## software_development

根流程只保留一个空的运行阶段。默认“日常检查”触发器由控制面在项目创建/启动时确定性创建，不再由智能体执行初始化。

## daily_check / dispatch

日常检查任务由触发器创建。调度阶段：

1. `task_get` 读取当前任务 input；如果 input 中有 `scope`，传给设计检查子任务。
2. 用 `task_spawn_children` 创建设计检查子任务：

```json
{
  "sequential": true,
  "tasks": [
    {
      "workflow": "design_inspection",
      "title": "设计检查",
      "description": "检查设计文档、系统/模块边界、接口和依赖，并把问题上报为 Issue。",
      "input": { "scope": "<scope 或空>" }
    }
  ]
}
```

3. 发送 `dispatch:dispatched`；之后任务进入等待阶段，设计检查子任务完成后自动结束。
4. 创建失败发送 `dispatch:failed`。

日常检查本身不直接检查、不修改代码或设计；检查由设计检查子任务的专门智能体完成，发现的问题通过 Issue 进入分诊。
