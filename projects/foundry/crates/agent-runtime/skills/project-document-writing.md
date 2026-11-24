# 项目文档撰写技能

## 目标

根据澄清 Issue、用 `project` 工具读取的项目信息和当前项目说明书，撰写 Arche 对象 `project/document`（项目说明书），然后提交评审。

你只负责撰写，不向创建者提问，不审批、不发布。

## 写作前必须读取

每一轮开始前重新确认以下信息，不能依赖上一轮的记忆：

1. **澄清 Issue**
   - `task_links_list` 找到当前任务的 Issue。
   - `issue_get` 读取标题、正文和全部评论。

2. **项目信息**
   - 用 `project` 工具读取项目信息（git_repository、git_branch、design_space_url、design_space_branch）。

3. **Arche 工作区**
   - 写作分支固定为 `project-document-<task-id>`，不要直接写 `<design_space_branch>`。
   - 不存在时：

     ```bash
     mkdir -p /work/<project-id>
     arche clone <design_space_url> /work/<project-id>/arche \
       --branch <design_space_branch>
     cd /work/<project-id>/arche
     arche fetch
     arche checkout project-document-<task-id> \
       || arche checkout -b project-document-<task-id>
     ```

     源码地址由设计空间记录，clone 自动继承。

   - 已存在时：

     ```bash
     cd /work/<project-id>/arche
     arche fetch
     arche status
     ```

     当前分支不是 `design_space_branch` 时先切回：
     ```bash
     arche checkout <design_space_branch>
     arche pull
     ```

     然后进入写作分支（已存在则检出，不存在则创建）：
     ```bash
     arche checkout project-document-<task-id> \
       || arche checkout -b project-document-<task-id>
     ```

     动笔前必须确认 `arche status` 显示：
     ```text
     branch: project-document-<task-id>
     ```

4. **当前项目说明书**
   - 用 `arche show project/document --format md` 读取全文。

5. **评审意见**
   - 用 `arche comment list project/document --open` 读取未解决的意见；没有意见时输出为空。

## 项目说明书的结构

`project/document` 的文档字段是 `document`（`DocumentParagraphList`）。章节固定为：

```text
定位
  是什么       文本
  不是什么     列表
背景
  现状         文本
  问题         列表（至少 1 项）
目标           列表（至少 1 项）
非目标         列表
分期计划       列表
约束           列表
风险           列表
```

没有术语表。不要增加其他章节。

## 写作流程

1. 按《项目文档写作说明书》准备每个章节的内容。
2. 在 `/work/<project-id>/project-document.json` 写完整的导入文件，例如：

   ```json
   {
     "fields": {
       "document": {
         "type": "DocumentParagraphList",
         "items": [
           {
             "type": "topic",
             "title": "定位",
             "body": {
               "items": [
                 {
                   "type": "topic",
                   "title": "是什么",
                   "body": {"items": [{"type": "text", "text": "..."}]}
                 },
                 {
                   "type": "topic",
                   "title": "不是什么",
                   "body": {"items": [{"type": "list", "items": ["..."]}]}
                 }
               ]
             }
           },
           {
             "type": "topic",
             "title": "背景",
             "body": {
               "items": [
                 {
                   "type": "topic",
                   "title": "现状",
                   "body": {"items": [{"type": "text", "text": "..."}]}
                 },
                 {
                   "type": "topic",
                   "title": "问题",
                   "body": {"items": [{"type": "list", "items": ["..."]}]}
                 }
               ]
             }
           },
           {"type": "topic", "title": "目标", "body": {"items": [{"type": "list", "items": ["..."]}]}},
           {"type": "topic", "title": "非目标", "body": {"items": [{"type": "list", "items": ["..."]}]}},
           {"type": "topic", "title": "分期计划", "body": {"items": [{"type": "list", "items": ["..."]}]}},
           {"type": "topic", "title": "约束", "body": {"items": [{"type": "list", "items": ["..."]}]}},
           {"type": "topic", "title": "风险", "body": {"items": [{"type": "list", "items": ["..."]}]}}
         ]
       }
     }
   }
   ```

3. 导入：

   ```bash
   cd /work/<project-id>/arche
   arche doc import project/document /work/<project-id>/project-document.json
   ```

4. 用 `arche show project/document --format md` 检查导入结果；章节缺失、重复或顺序错误时修正 JSON 后重新导入。

## 写作依据

- 章节内容以《项目文档写作说明书》为准；它规定每节写什么、不写什么，以及正例和反例。
- 只写 Issue、任务上下文和项目说明书中能够确认的事实；不把推断写成事实。
- 分期计划只写用户明确要求或说明过的分期；用户没有说明就留空。
- 行业常识和一般业务实践可以用于描述通用背景；不要把它们写成该项目的特有事实，也不要编造数据。
- 没有来源的数字、比例、频率不写；能用定性描述就用定性描述。
- 风险由你根据项目定位、背景、目标、非目标、约束和依赖自行判断；每条只写一句简单描述，不写概率、影响、缓解措施或负责人。
- 不修改项目名称和描述。

## 校验与提交

```bash
cd /work/<project-id>/arche
arche status
# 必须显示 branch: project-document-<task-id>
arche doc check
arche check
arche add -A
arche commit -m "Write the project specification"
arche push
```

`arche push` 只推送当前写作分支，不要推送或强制推送 `source`/`main`。

然后调用 `task_allowed_operations`，再调用 `task_operation`，operation 使用 `task_allowed_operations` 返回的 `completed` 操作名，把项目说明书提交给文风审查。

## 被审查打回后

1. `task_get` 读取任务详情和打回原因。
2. 确认仍在写作分支：
   ```bash
   cd /work/<project-id>/arche
   arche fetch
   arche checkout project-document-<task-id>
   arche pull
   arche status
   ```
3. `arche comment list project/document --open` 读取全部未解决意见。
4. 逐条修改 JSON 并重新 `arche doc import`；不要漏掉意见。
5. 重新校验、`arche add -A`、`arche commit`、`arche push`。
6. 每条已处理的意见用：

   ```bash
   arche comment resolve --agent <你的 handle> --conclusion "已修改：<说明>" <comment-id>
   ```

7. 重新提交评审。

## 不该做的事

- 不读写锚点文档（不要使用 `document` 工具处理项目说明书）。
- 用 `project` 工具读取项目信息。
- 不直接修改 `.arche/` 下的文件。
- 不修改需求、架构、详细设计或代码。
- 不审批、不发布。
