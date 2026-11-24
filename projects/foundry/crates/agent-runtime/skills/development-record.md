# 开发记录技能

## 目标

在项目工作区中实现代码，运行构建和测试，并把变更集、命令结果和证据写入任务评论。

你不修改需求、设计或架构，不审批或发布。

## 输入

每一轮开始前重新读取：

1. `task_links_list` 找到当前任务的 Issue。
2. `issue_get` 读取 Issue 正文和全部评论。
3. 用 `project` 工具读取项目信息（git_repository、git_branch、design_space_url、design_space_branch）。
4. 用 `shell` 在 Arche 工作区执行 `arche pull`，用 `arche show` / `arche search --kind requirement-*` 读取需求和交互；用 `arche ls` / `arche show` 读取技术设计对象。
5. 读取父任务和 Issue 评论中已有的实现范围、测试计划和证据。
6. `task_allowed_operations` 确认当前允许的操作。

## 工作区

工作区固定为 `/work/<project-id>/repo`。Git 凭据已经由 agentd 配好，直接用 `shell` 执行 `git`、`rg`、`cat`、构建和测试命令。

每次开始前先查看 `git status`、`git log`，确认当前分支和基础提交。

## 工作流程

1. 对照详细设计和测试计划，确认本次迭代要交付的模块、接口和验收条件。
2. 执行 `git fetch --all --prune`、`git checkout <project.git_branch>`、`git pull --ff-only`。
3. 逐个实现：
   - 用 `shell` 读取现有文件；
   - 用 `shell` 修改或创建文件；
   - 用 `shell` 运行构建、格式、lint 和测试（存在 `scripts/verify.sh` 时优先运行它）；
   - 失败就修复并重跑，不能把失败留到评审。
4. 完成后提交：
   - 如果工作区根有 `.arche/`，确保它在 `.gitignore` 中：`grep -qxF '.arche/' .gitignore 2>/dev/null || printf '.arche/\n' >> .gitignore`；
   - 确认 `.arche/` 已被忽略：`git check-ignore -q .arche || printf '\n.arche/\n' >> .git/info/exclude`；
   - `git add -A`；
   - 检查 `git diff --cached --name-only` 不含 `.arche/`；出现则停止，不要提交；
   - `git -c user.name=... -c user.email=... commit -m "..."`；
   - 记录分支和 commit hash；需要时推送当前开发分支。
5. 用 `task_comment` 写开发记录，标题用迭代任务标题，内容包含：
   - `实现说明：` 完成了什么、对应哪些模块和需求、没有做什么。
   - `代码引用：` 工作区 `/work/<project-id>/repo`、分支、base/head commit、diff 范围。
   - `构建与静态检查：` 实际命令和退出码、关键输出摘要。
   - `测试证据：` 实际测试命令、通过/失败/跳过数量、关键失败输出。
   - `关联需求：` 对应功能或需求名称。
   - `风险与遗留：` 未解决的风险、技术债和后续动作。
   每个可独立评审的变更集单独写一条评论。
6. 用 `task_allowed_operations` 返回的操作名调用 `task_operation`，提交评审。

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
2. `task_get` 读取任务评论和评审意见。
3. 回到工作区修改代码、补测试或补证据。
4. 用新的 `task_comment` 记录处理结果。
5. 重新提交评审。
