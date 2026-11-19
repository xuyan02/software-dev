# 触发器

触发器是项目级配置：它在计划时间或用户点击“立即运行”时，用固定流程、输入和模板创建一个 Task。任务仍由 Task Dispatcher 分配角色，触发器不直接执行工作。

## 模型

```text
Trigger
  project_id
  name
  workflow_id
  input
  schedule          # manual / every:<minutes> / daily@HH:MM (UTC)
  enabled
  title_template
  description_template
  created_by
  next_run_at
  last_run_at
```

```text
TriggerRun
  trigger_id
  task_id
  reason            # scheduled / manual
  triggered_by
  created_at
```

## 行为

- 定时：控制平面每 30 秒扫描 `enabled`、非 `manual` 且 `next_run_at <= now` 的触发器。
- 同一触发器已有未完成任务时跳过本次运行，不并发、不排队。
- 手动运行不改变 `next_run_at`，只记录一次 `manual` 运行。
- 任务 input 会自动加入 `trigger_id`、`trigger_name`、`trigger_reason`。
- 可触发的流程限制在白名单内：日常检查、检查类流程和技术设计一致性维护。

## API

```text
GET    /api/v1/projects/{project}/triggers
POST   /api/v1/projects/{project}/triggers
PATCH  /api/v1/projects/{project}/triggers/{trigger_id}
DELETE /api/v1/projects/{project}/triggers/{trigger_id}
POST   /api/v1/projects/{project}/triggers/{trigger_id}/run
GET    /api/v1/projects/{project}/triggers/{trigger_id}/runs
```

## 项目页

项目页的“触发器”区域列出名称、流程、计划、状态、上次/下次运行，并提供：

- 立即运行；
- 启用/停用；
- 删除；
- 创建触发器：名称、流程、计划、可选范围。

## 创建示例

```json
{
  "name": "每日设计检查",
  "workflow_id": "design_inspection",
  "schedule": "daily@09:00",
  "input": { "scope": "design/app" }
}
```

`manual` 表示只允许手动运行；`every:60` 表示每 60 分钟；`daily@09:00` 使用 UTC。
