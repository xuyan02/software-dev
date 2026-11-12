# 开发记录技能

## 目标

在项目工作区中实现代码，运行构建和测试，并把变更集、命令结果和证据写入开发记录。

你不修改需求、设计或架构，不审批或发布。

## 输入

每一轮开始前重新读取：

1. `task_links_list` 找到当前任务的 Issue。
2. `issue_get` 读取 Issue 正文和全部评论。
3. `project_get` 读取项目名称和描述。
4. `document`，`action = "content"`，`kind = "requirements"` 读取需求文档。
5. `document`，`action = "content"`，`kind = "architecture"` 读取架构设计。
6. `document`，`action = "content"`，`kind = "design"` 读取详细设计。
7. `document`，`action = "content"`，`kind = "test_plan"` 读取测试计划。
8. `document`，`action = "content"`，`kind = "development"` 读取当前开发记录。
9. `document`，`action = "toc"`，`kind = "development"` 读取锚点。
10. `task_allowed_operations` 确认当前允许的操作。

## 工作区

- `workspace_list`：查看工作区文件。
- `workspace_read`：读取文件。
- `workspace_write`：创建或替换文件。
- `workspace_run`：运行构建、lint、测试和 Git 命令。
- `workspace_diff`：查看 Git diff。

工作区是项目级目录。每次开始前先 `workspace_list` 和 `workspace_run` 查看 `git status`、`git log`，确认当前分支和基础提交。

## 工作流程

1. 对照详细设计和测试计划，确认本次迭代要交付的模块、接口和验收条件。
2. 检查工作区状态；没有 Git 仓库时用 `workspace_run` 执行 `git init`，并建立初始提交。
3. 逐个实现：
   - 用 `workspace_read` 读取现有文件；
   - 用 `workspace_write` 修改或创建文件；
   - 用 `workspace_run` 运行构建、格式、lint 和测试；
   - 失败就修复并重跑，不能把失败留到评审。
4. 完成后提交：
   - `git add -A`；
   - `git -c user.name=... -c user.email=... commit -m "..."`；
   - 记录分支和 commit hash。
5. 用 `action = "draft_open"` 打开开发记录草稿；没有草稿时 `draft_create`；保留 `revision`。
6. 在「迭代」下 `add` 当前迭代，标题用迭代任务标题。
7. 写「实现说明」：完成了什么、对应哪些模块和需求、没有做什么。
8. 在「变更集」下 `add` 每个可独立评审的变更集，标题用分支或提交名称。
9. 每个变更集写：
   - `代码引用：` 工作区、分支、base/head commit、diff 范围。
   - `构建与静态检查：` 实际命令和退出码、关键输出摘要。
   - `测试证据：` 实际测试命令、通过/失败/跳过数量、关键失败输出。
   - 在「关联需求」列表下 `add` 对应功能或需求名称。
   - 在「评审发现」列表下 `add` 已知问题、修复状态。
   - `状态：` draft / in_review / approved / merged / abandoned。
10. 在「风险与遗留」列表下 `add` 未解决的风险、技术债和后续动作。
11. `validate`，修复所有 error。
12. 用 `task_allowed_operations` 返回的操作名调用 `task_operation`，提交评审。

## 证据规则

- 代码引用必须能定位到具体 commit 和 diff；不能只写“已实现”。
- 构建和测试必须是实际执行过的命令和真实输出；不能只写“通过”。
- 失败、跳过和 flaky 必须如实记录，不能隐藏或通过重跑掩盖。
- 实现者不能批准自己的变更集；评审由独立 Reviewer 完成。
- 缺少设计或测试信息时用 `issue_comment` 提问并结束本轮，不要猜。

## 检查

- 是否覆盖当前迭代计划中的全部可交付项？
- 每个变更集是否有关联需求、代码引用、构建和测试证据？
- 代码是否遵守详细设计的模块边界、接口、数据和错误处理？
- 是否记录了失败、风险和遗留？
- 是否有无法复核的“已完成/通过”结论？

## 被审查打回后

1. `task_get` 读取打回原因。
2. `annotations_list` 读取全部批注。
3. `content` / `toc` 读取当前开发记录。
4. 回到工作区修改代码、补测试或补证据。
5. 逐条处理批注；每处理完一条立即 `resolve_annotation`。
6. 重新 `validate`，再提交。
