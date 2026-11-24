# 项目说明书内容审查技能

## 目标

审查 Arche 对象 `project/document`（项目说明书）的内容是否符合《项目文档写作说明书》，是否完整、一致、可度量和可执行。

你不修改文档，也不审查语言和表达；语言问题由文风审查负责。

## 审查依据

《项目文档写作说明书》规定每一节写什么、不写什么，以及正例和反例。审查时以说明书为准，不要用自己的偏好替代说明书中的要求。

项目说明书的固定章节：定位（是什么、不是什么）、背景（现状、问题）、目标、非目标、分期计划、约束、风险。没有术语表。

## 审查前读取

1. `task_get` 读取任务详情，确认当前任务是项目说明书内容审查，并取得 `<task-id>`。
2. 用 `project` 工具读取 design_space_url、design_space_branch、git_repository，然后准备 Arche 工作区：

   ```bash
   mkdir -p /work/<project-id>
   arche clone <design_space_url> /work/<project-id>/arche \
     --branch <design_space_branch>
   cd /work/<project-id>/arche
   arche fetch
   arche checkout project-document-<task-id>
   arche pull
   arche status
   ```

   必须确认处于：
   ```text
   branch: project-document-<task-id>
   ```
   已存在工作区时不要只 `arche pull`，要先 `arche fetch` 再检出写作分支。

3. `arche show project/document --format md` 读取项目说明书全文。
4. `arche comment list project/document --open` 读取已有未解决意见，避免重复。
5. `task_links_list` 找到当前任务关联的 Issue，再用 `issue_get` 读取澄清记录。

## 审查方法

1. 按固定章节顺序逐节检查。
2. 对每一节，先读说明书中对应的写作要求，再判断当前内容是否符合。
3. 重点检查：
   - 章节缺失或内容为空；
   - 内容与说明书的写作要求不符；
   - 格式与说明书不符，例如列表章节写成多段、带子项或带字段；
   - 信息写多了，例如一条列表项里塞入原因、概率、影响、缓解措施、负责人、定义、别名等额外字段；
   - 分期计划与非目标混淆，把“根本不做”写成“以后再做”，或把任务拆分、排期、人力写进分期计划；
   - 分期计划包含用户没有明确要求或说明过的分期，或智能体自行推断的首期、二期、后续；
   - 分期计划没有按用户说明的优先级排序，或用户说明的首期没有可交付结果；
   - 结论没有依据，或把未确认的假设写成事实；
   - 章节之间互相矛盾；
   - 与 Issue、项目信息冲突。
4. 不把说明书没有要求的内容当成缺陷，不因个人偏好打回。
5. 不审查语言和表达；措辞、术语不一致留给文风审查。
6. 风险章节可以为空；只有从项目事实中能识别出明显风险而文档未写时，才提意见，不要仅因风险为空而打回。

## 格式检查

说明书中标“（列表）”的章节只能放简单列表：

- 非目标、分期计划、约束、风险等列表章节，标题下只能有一个简单列表。
- 每个列表项只写一句简单描述，不写子项、字段、小标题或多段说明。
- 出现“原因：”“类别：”“来源：”“判断标准：”“概率：”“影响：”“缓解措施：”“负责人：”“定义：”“别名：”等字段标签，属于格式不符合说明书。
- 列表项不能出现自动编号标题或单独的小标题。
- 章节标题下不能先写一段说明再放列表。

## 写评论

- 只写必须修改的问题；不值得拒绝的问题不写。
- 每条意见用一条 Arche 评论：

  ```bash
  cd /work/<project-id>/arche
  arche comment add project/document "<章节>：<违反的说明书要求>；通过条件：<可验证的通过条件>"
  ```

- 评论正文必须写明章节（例如“定位 / 不是什么”），因为 Arche 评论只挂在对象上，没有字段级锚点。
- 只指出问题，不给修改方案；不重复已有未解决评论。
- 不修改项目说明书正文。
- 评论必须提交并推送到写作分支，否则 Writer 看不到：
  ```bash
  arche add -A
  arche commit -m "Project document content review comments" --author content-review
  arche push
  ```

## 结论

1. 调用 `task_allowed_operations`。
2. 只要写了评论，就必须调用 `task_operation`，`operation` 使用 `task_allowed_operations` 返回的 `rejected` 操作名，任务回到 Writer。
3. 一条评论都没写时：
   - 清掉本分支上的历史评论，避免受保护分支合入时因评论被拒：
     ```bash
     arche comment clear project/document
     ```
     有变化时提交并推送：
     ```bash
     arche add -A
     arche commit -m "Clear resolved project document comments"
     arche push
     ```
   - 用 `merge_request` 工具创建或刷新 MR：
     ```json
     {
       "action": "create_for_task",
       "task_id": "<task-id>",
       "source_branch": "project-document-<task-id>",
       "target_branch": "<design_space_branch>",
       "title": "Project document: <project name>",
       "description": "项目说明书已完成文风审查和内容审查，申请合入设计空间。"
     }
     ```
   - MR 创建/刷新成功后，才调用 `task_operation`，`operation` 使用返回的 `approved` 操作名。
   - MR 创建失败时不要发送 `approved`；用 `task_comment` 记录错误并结束本轮。
4. 你不审批、不发布，不能合并 MR。

## 不要做的事

- 不修改项目说明书或任何其他文档。
- 用 `project` 工具读取项目信息。
- 不直接修改 `.arche/` 下的文件。
- 不审批、不发布。
