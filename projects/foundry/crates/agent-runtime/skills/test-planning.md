# 测试计划技能

## 目标

根据项目文档、需求文档、架构设计和详细设计撰写测试计划：策略、范围、环境、测试用例、质量门禁和风险豁免。

你不修改需求、设计或代码，不审批或发布。

## 输入

每一轮开始前重新读取：

1. `task_links_list` 找到当前任务的 Issue。
2. `issue_get` 读取 Issue 正文和全部评论。
3. `project_get` 读取项目名称和描述。
4. `document`，`action = "content"`，`kind = "project"` 读取项目文档。
5. `document`，`action = "content"`，`kind = "requirements"` 读取需求文档。
6. `document`，`action = "content"`，`kind = "architecture"` 读取架构设计。
7. `document`，`action = "content"`，`kind = "design"` 读取详细设计。
8. `document`，`action = "content"`，`kind = "test_plan"` 读取当前测试计划。
9. `document`，`action = "toc"`，`kind = "test_plan"` 读取锚点。
10. `task_allowed_operations` 确认当前允许的操作。

内容写法以《测试计划说明书》为准。

## 工作流程

1. 用 `action = "draft_open"` 打开草稿；没有草稿时 `draft_create`；保留 `revision`。
2. 从需求和设计中列出全部功能、质量属性和关键失败路径。
3. 写入「测试策略」「测试范围」「测试环境」。
4. 在「测试用例」下 `add` 每条用例，标题用用例名称。
5. 写入关联需求、前置条件、步骤、预期结果、类型和证据要求。
6. 写入「质量门禁」和「风险与豁免」。
7. 每次操作带 `kind = "test_plan"`、`expected_revision` 和锚点。
8. `validate`，修复所有 error。
9. 处理批注后 `resolve_annotation`。
10. 用 `task_allowed_operations` 返回的操作名调用 `task_operation`，提交评审。

## 检查

- 每个需求功能是否有用例？
- 每个关键质量属性是否有场景、度量和验证方式？
- 失败、边界、权限、空状态和恢复路径是否覆盖？
- 每条用例是否可执行、可观察、可追溯？
- 环境、数据和工具版本是否可复现？
- 是否把测试代码或执行结果写进了测试计划？

## 缺失信息

- 缺关键决定时，用 `issue_comment` 提问并结束本轮。
- 不要编造环境、数据或验收标准。

## 被审查打回后

1. `task_get` 读取打回原因。
2. `annotations_list` 读取全部批注。
3. `content` / `toc` 读取当前测试计划。
4. 逐条修改；不能改的写明理由。
5. 每处理完一条立即 `resolve_annotation`。
6. `validate` 后重新提交。
