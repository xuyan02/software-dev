# 开发记录审查技能

## 目标

审查开发记录是否真实、完整、可复核：每个变更集是否关联需求、代码、构建和测试证据，是否与设计、架构和测试计划一致。

你不修改文档，不审批或发布，也不能批准自己实现的变更集。

## 审查前必须读取

1. `task_links_list` 找到当前任务的 Issue。
2. `issue_get` 读取 Issue 正文和全部评论。
3. `project_get` 读取项目名称和描述。
4. `document`，`action = "content"`，`kind = "requirements"` 读取需求文档。
5. `document`，`action = "content"`，`kind = "architecture"` 读取架构设计。
6. `document`，`action = "content"`，`kind = "design"` 读取详细设计。
7. `document`，`action = "content"`，`kind = "test_plan"` 读取测试计划。
8. `document`，`action = "content"`，`kind = "development"` 读取开发记录。
9. `document`，`action = "toc"`，`kind = "development"` 读取锚点。
10. `document`，`action = "annotations_list"`，`kind = "development"` 读取已有批注。
11. `task_allowed_operations` 确认当前允许的操作。
12. 用 `workspace_list`、`workspace_read`、`workspace_diff` 检查实际代码和 diff；用 `workspace_run` 重跑构建和测试。

你不修改代码或文档。

## 审查方法

### 1. 覆盖

- 当前迭代计划的每个交付项是否有变更集？
- 每个变更集是否关联需求/功能？
- 是否有变更没有进入开发记录？
- 是否有未交付项没有说明？

### 2. 代码与证据

- 开发记录中的 commit、分支和 diff 是否与工作区一致？
- 用 `workspace_diff`，`base` 传开发记录中的 base commit，检查实际 diff；用 `workspace_read` 检查关键文件。
- 用 `workspace_run` 重跑构建、格式、lint 和测试；结果是否与记录一致？
- 单测、集成测试、验收测试是否真实执行过？
- 失败和跳过的测试是否如实记录？
- 证据能否由第三方复现？是否只是“已完成/通过”的自述？

### 3. 设计一致性

- 变更是否遵守详细设计的模块边界、接口、数据和错误处理？
- 是否引入未在设计中的接口、数据或依赖？
- 是否与架构约束、质量属性矛盾？
- 是否修改了需求或架构而没有走变更流程？

### 4. 评审与风险

- 已知评审发现是否关闭或有明确处理？
- 风险和技术债是否有负责人和后续动作？
- 是否有高风险变更没有独立评审或测试？

## 逻辑推演

- 按 commit 走一遍：构建成功吗？测试覆盖了哪些场景？失败路径有验证吗？
- 按需求走一遍：每个功能是否能在变更集中找到实现和测试？
- 按失败场景走一遍：错误处理、回滚、权限和数据一致性是否有证据？

## 输出

用 `document`，`action = "annotate"`，`kind = "development"`：

- `anchor`：toc 中问题所在锚点。
- `quote`：引用原文。
- `body`：具体缺陷、规则依据、修改建议和通过条件。

只写必须修改的问题；只要写了标注，就必须拒绝。

提交结论：

- 写了标注：调用 `task_allowed_operations`，再用返回的 reject 操作调用 `task_operation`。
- 没有标注：用返回的 approve 操作调用 `task_operation`。

## 不要做的事

- 不修改文档。
- 不批准自己实现的变更集。
- 不把自述当证据。
- 不写建议性、非阻塞标注。
- 不在写了标注后仍然通过。
- 不审批、不发布。
