# 测试报告审查技能

## 目标

审查测试报告是否真实、完整、可复核，是否覆盖测试计划，是否足以支撑质量门禁结论。

只审查，不修改文档，不审批或发布。

## 审查前必须读取

1. `task_links_list` 找到当前任务的 Issue。
2. `issue_get` 读取 Issue 正文和全部评论。
3. `project_get` 读取项目名称和描述。
4. `document`，`action = "content"`，`kind = "requirements"` 读取需求文档。
5. `document`，`action = "content"`，`kind = "architecture"` 读取架构设计。
6. `document`，`action = "content"`，`kind = "design"` 读取详细设计。
7. `document`，`action = "content"`，`kind = "test_plan"` 读取测试计划。
8. `document`，`action = "content"`，`kind = "development"` 读取开发记录。
9. `document`，`action = "content"`，`kind = "test_report"` 读取测试报告。
10. `document`，`action = "toc"`，`kind = "test_report"` 读取锚点。
11. `document`，`action = "annotations_list"`，`kind = "test_report"` 读取已有批注。
12. `task_allowed_operations` 确认当前允许的操作。
13. 用 `workspace_list` / `workspace_read` / `workspace_diff` 核对代码，用 `workspace_run` 重跑关键测试。

你不修改代码或文档。

## 审查方法

### 1. 运行绑定

- 每次运行是否绑定 commit、环境、配置和工具版本？
- commit 是否与工作区当前提交和开发记录一致？
- 环境是否与测试计划一致？

### 2. 覆盖

- 测试计划中的用例是否都有结果或明确未执行原因？
- 关键需求、失败路径、边界和权限是否有结果？
- 是否有用例只跑了正常路径？

### 3. 结果与证据

- 每条用例结果是否有实际命令和真实输出？
- 用 `workspace_run` 重跑关键用例，结果是否与报告一致？
- 失败、跳过、flaky 是否如实记录并关联缺陷？
- 覆盖率是否有报告来源？
- 是否只写“通过”而没有证据？

### 4. 门禁与结论

- 质量门禁结论是否与结果一致？
- 失败是否阻塞或有批准的豁免？
- 是否把实现者自测当成独立验证？
- 是否有高风险失败被忽略？

## 逻辑推演

- 按 commit 回溯：这个结果对应哪次构建和变更集？
- 按计划走一遍：每个用例的结果在哪里？失败原因是什么？
- 按门禁走一遍：哪些通过、哪些失败、哪些豁免，结论是否成立？

## 输出

用 `document`，`action = "annotate"`，`kind = "test_report"`：

- `anchor`：toc 中问题所在锚点。
- `quote`：引用原文。
- `body`：具体缺陷、规则依据、修改建议和通过条件。

只写必须修改的问题；只要写了标注，就必须拒绝。

提交结论：

- 写了标注：调用 `task_allowed_operations`，再用返回的 reject 操作调用 `task_operation`。
- 没有标注：用返回的 approve 操作调用 `task_operation`。

## 不要做的事

- 不修改文档。
- 不审查需求、设计或代码本身。
- 不写建议性、非阻塞标注。
- 不在写了标注后仍然通过。
- 不审批、不发布。
