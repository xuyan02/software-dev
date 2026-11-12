# 测试计划审查技能

## 目标

审查测试计划是否覆盖需求、设计、架构和质量属性，是否可执行、可观察、可复现。

只审查，不修改文档，不审批或发布。

## 审查前必须读取

1. `task_links_list` 找到当前任务的 Issue。
2. `issue_get` 读取 Issue 正文和全部评论。
3. `project_get` 读取项目名称和描述。
4. `document`，`action = "content"`，`kind = "project"` 读取项目文档。
5. `document`，`action = "content"`，`kind = "requirements"` 读取需求文档。
6. `document`，`action = "content"`，`kind = "architecture"` 读取架构设计。
7. `document`，`action = "content"`，`kind = "design"` 读取详细设计。
8. `document`，`action = "content"`，`kind = "test_plan"` 读取测试计划。
9. `document`，`action = "toc"`，`kind = "test_plan"` 读取锚点。
10. `document`，`action = "annotations_list"`，`kind = "test_plan"` 读取已有批注。
11. `task_allowed_operations` 确认当前允许的操作。

## 审查方法

### 1. 覆盖

- 每个需求功能是否有至少一条用例？
- 每个关键质量属性是否有场景、度量和验证方式？
- 失败路径、边界、权限、空状态和恢复是否覆盖？
- 是否有需求只由实现者自测覆盖，没有独立用例？

### 2. 可执行与可观察

- 前置条件、步骤和数据是否足以执行？
- 预期结果是否可观察、可判断？
- 每条用例是否指定证据要求（命令、日志、报告或截图）？
- 环境、数据和工具版本是否可复现？

### 3. 风险与门禁

- 质量门禁是否明确、可执行？
- 风险与豁免是否有理由、批准人和有效期？
- 是否把“通过率”“覆盖率”当成唯一目标？

### 4. 一致性

- 是否与需求、架构、详细设计一致？
- 是否把测试代码或执行结果写进了测试计划？
- 是否缺少环境、数据或工具版本？

## 逻辑推演

- 对每条用例按步骤走一遍：能否执行，结果能否判断？
- 对每个失败路径问：是否有用例，预期结果是否明确？
- 对每个关键需求问：是否有独立证据，能否被自测蒙混？

## 输出

用 `document`，`action = "annotate"`，`kind = "test_plan"`：

- `anchor`：toc 中问题所在锚点。
- `quote`：引用原文。
- `body`：具体缺陷、规则依据、修改建议和通过条件。

只写必须修改的问题；只要写了标注，就必须拒绝。

提交结论：

- 写了标注：调用 `task_allowed_operations`，再用返回的 reject 操作调用 `task_operation`。
- 没有标注：用返回的 approve 操作调用 `task_operation`。

## 不要做的事

- 不修改文档。
- 不审查需求或设计本身。
- 不写建议性、非阻塞标注。
- 不在写了标注后仍然通过。
- 不审批、不发布。
