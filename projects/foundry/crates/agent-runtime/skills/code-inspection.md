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
- 空设计空间是否正常取决于检查类型和代码是否为空：
  - `design_inspection`：代码和设计空间都为空时正常；代码不为空而设计空间为空时是设计缺失，必须上报 Issue；
  - `consistency_inspection`：代码不为空而设计空间为空时，说明设计缺失，必须上报 Issue。

Git 凭据和配置已经由 agentd 预先配好，直接运行 `git` / `ds` 命令；不要自己设置 `HOME`、`GIT_CONFIG_GLOBAL` 或 `GIT_TERMINAL_PROMPT`。源码 URL 由设计空间仓库自带的 `repo.toml` 提供，`ds clone` 不需要指定。

设计空间工作区：

```text
/work/<project-id>/ds
```

缺失时创建：

```bash
ds clone <design_space_url> --branch source --dir /work/<project-id>/ds
```

源码由设计空间管理，不需要另建 git checkout：

```bash
ds source fetch
ds source ls
ds source cat <path>
ds source sync      # 生成 files/ 对象
```

`design_space_url`（`ssh://.../<project-id>`）只接受 `ds` 协议。不要对它使用
`git clone`、`git fetch` 或 `git push`；服务端会拒绝，主机密钥也不在 Git 的
`known_hosts` 里。设计空间的凭证由 `ds` 自己读取，不需要环境变量或 `-i` 参数。

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

先判断代码和设计空间：

1. 代码仓库没有源码，设计空间也为空：写评论说明，发送 `inspect:completed`。
2. 代码仓库有源码，但设计空间没有 `design` 结构根、没有系统/模块：
   - 这是设计缺失，不是正常空设计空间；
   - 创建 Issue，标题使用 `设计空间缺失：...`，设置 `labels: ["design-space-missing"]`，说明设计空间为空但代码存在，附上代码提交、源码文件数量、顶层元素数量和范围；
   - 通常聚合成一个 Issue，不要为每个文件创建 Issue；
   - 发送 `inspect:completed`。
3. 代码仓库有源码，但规范技术设计 `documents/design/technical` 不存在：
   - 创建 Issue，标题使用 `设计缺失：规范技术设计不存在`，设置 `labels: ["design-space-missing"]`；
   - 说明缺少统一技术设计文档，附上代码范围和证据；
   - 发送 `inspect:completed`。
4. 设计空间存在系统/模块时，继续比较：

1. 固定代码提交和设计空间提交/分支。
2. 找到代码对象与设计对象/文档/模块成员的映射。
3. 检查：
   - 代码对象没有对应设计；
   - 设计对象/模块成员指向不存在的代码；
   - 代码行为与设计文档明显不一致；
   - 系统/模块划分与代码结构严重漂移。

每个 Issue 描述同时包含代码位置和 DS qname，并明确是“设计缺失/设计过时/代码偏离设计”。

## 5. design_inspection

先判断代码和设计空间：

```bash
# 代码是否存在、阅读源码
ds source fetch
ds source ls
ds source cat <path>

# 设计空间是否为空
ds structure list
ds ls --namespaces
ds ls --documents
```

分支：

1. 代码为空且设计空间为空：这是正常情况。写任务评论说明，发送 `inspect:completed`，不创建 Issue。
2. 代码不为空且设计空间为空：这是设计缺失。创建 Issue，标题使用 `设计缺失：<范围>`，设置 `labels: ["design-space-missing"]`，说明代码提交、文件数量、顶层元素数量和范围；发送 `inspect:completed`。
3. 设计空间不为空：继续检查：
   - 设计文档是否完整、可理解、与需求一致；
   - 系统/模块边界、接口、依赖是否清晰；
   - 是否有空模块、无归属模块、职责重叠；
   - 关键非功能约束是否缺失。

不检查代码实现细节；代码可行性属于一致性检查或开发评审。发现的问题用 `issue_create` 上报，附 DS qname 和证据，不直接修改设计。

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
