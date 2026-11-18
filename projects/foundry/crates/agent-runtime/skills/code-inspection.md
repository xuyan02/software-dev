# 代码检查技能

本技能用于日常检查任务：`style_inspection`、`logic_inspection`、`cohesion_inspection`、`consistency_inspection`、`design_inspection`。

## 共同规则

- 检查现有仓库/设计空间，不修改代码或设计。
- 一次只做当前工作流指定的检查重点，不混入其他检查。
- 发现的问题用 `issue_create` 上报；不要只写在任务评论里。
- 上报前先调用 `issue_list` 查看已有 Issue；相同根因已存在时不要重复创建，可在已有 Issue 下补充证据。
- 每个 Issue 要有可定位证据：提交、文件/行、DS qname、测试/命令结果。
- 按根因聚合，不要每个文件/每行生成一个 Issue。
- 检查完成发送 `inspect:completed`；无法检查发送 `inspect:failed`。

工作区：

```bash
export DS_TOKEN_FILE=/run/foundry/ds-token
GIT_HOME=/work/<project-id>/.credentials/git
export HOME=$GIT_HOME
export GIT_CONFIG_GLOBAL=$GIT_HOME/gitconfig
export GIT_TERMINAL_PROMPT=0
```

代码和设计空间：

```text
/work/<project-id>/repo
/work/<project-id>/ds
```

任务 input 可以包含 `scope`（文件、目录、模块或 DS qname）。没有 scope 时检查项目全量。

## 1. style_inspection

只检查代码风格和可读性：

- 命名是否符合项目惯例；
- 格式、结构、注释是否可读；
- 是否有明显的风格不一致；
- 不检查逻辑正确性、设计合理性或架构问题。

问题 Issue 的 `area` 写文件/模块，描述中包含具体位置和建议。

## 2. logic_inspection

只检查代码逻辑：

- 正确性、边界条件、空值/错误处理；
- 并发、资源释放、异常路径；
- 明显不可达/重复逻辑；
- 不检查文风、设计边界或需求。

每个 Issue 尽量给出复现条件、输入、实际结果和期望结果。`kind = bug`，必须设置 severity。

## 3. cohesion_inspection

只检查聚合度和边界：

- 模块/文件/类型职责是否混杂；
- 依赖是否混乱、是否存在循环或跨层依赖；
- 公开接口是否泄漏内部细节；
- 变化是否集中，是否存在“万能类/万能模块”。

这类问题通常是设计问题，Issue `kind = task_request` 或 `bug`，描述中说明需要设计优化，而不是直接给代码修复方案。

## 4. consistency_inspection

比较代码和设计空间：

1. 固定代码提交和设计空间提交/分支。
2. 找到代码对象与设计对象/文档/模块成员的映射。
3. 检查：
   - 代码对象没有对应设计；
   - 设计对象/模块成员指向不存在的代码；
   - 代码行为与设计文档明显不一致；
   - 系统/模块划分与代码结构严重漂移。

每个 Issue 描述同时包含代码位置和 DS qname，并明确是“设计缺失/设计过时/代码偏离设计”。

## 5. design_inspection

只检查设计空间：

- 设计文档是否完整、可理解、与需求一致；
- 系统/模块边界、接口、依赖是否清晰；
- 是否有空模块、无归属模块、职责重叠；
- 关键非功能约束是否缺失。

不检查代码实现细节；代码可行性属于一致性检查或开发评审。

## 6. 上报格式

`issue_create` 的 description 建议包含：

```text
检查类型: style / logic / cohesion / consistency / design
位置: 文件/行 或 DS qname
证据: 命令、测试、代码片段、设计版本
问题: ...
影响: ...
建议: ...
置信度: high / medium / low
```

Issue 的 `area` 写模块/文件/子系统；bug 必须设置 severity。若没有发现问题，在任务评论中写明检查范围和结论，不需要创建空 Issue。
