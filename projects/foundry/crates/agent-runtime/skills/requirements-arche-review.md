# 设计空间需求审查技能

## 目标

审查 Arche 设计空间 `requirement` 空间的结构化需求，不修改需求正文，不审批、不发布。

不使用 `document` 工具。

## 分支

```text
requirements/<task-id>
```

```bash
cd /work/<project-id>/arche
arche fetch
arche checkout requirements/<task-id>
arche pull
arche status
```

## 检查

1. 对象结构
   - RequirementUser、RequirementStory、RequirementSystem、RequirementModule、RequirementFunction、RequirementInteraction 是否齐全；
   - 系统 → 模块 → 功能 → 交互的父子关系是否正确；
   - 系统是否是用户可感知的一个完整软件，而不是代码仓库、部署单元或内部服务；
   - 模块是否围绕一个需求概念完成功能闭环，而不是代码目录或单个功能；
   - 功能是否产生有意义的系统数据变化或信息展示，而不是只完成一次界面输入；
   - 交互是否是用户的一次操作/观察或系统通知，是否由若干交互组成一次功能。
2. 内容
   - 用户划分是否遵守两原则：产品角色不同 → 不同用户；同一角色下偏好或状况不同、且产生对产品有意义的差异化行为 → 不同用户；
   - 每一种用户类型是否都有独立用户，人物特征是否体现该类型差异，是否按产品定位标定了 `priority`（core/primary/secondary/peripheral）；
   - 优先级是否真正影响设计：core 用户的需求是否优先有故事覆盖，core 用户的关键交互路径是否比其他人更短；
   - 用户需求是否被一组故事完备覆盖，而不是只被一个故事覆盖；
   - 覆盖同一需求的故事是否解决需求前、进行中和完成后的各种问题，并形成闭环；
   - 故事 `users` 是否列出所有参与用户，`protagonist` 是否指向 `users` 中的一个用户；
   - 故事 `description` 是否用连续普通段落写成完整用户旅程（不是 topic 列表），每一步使用的功能、其他用户的配合和结果是否清楚；
   - 故事 `flow` 是否与 `description` 的步骤一致；
   - 故事 `functions` 是否列出旅程中出现的所有功能，且都能在需求空间中找到；
   - 故事 `requirement` 的每一项是否写清满足用户的什么需求；
   - 功能是否有用户可观测变化，是否有故事覆盖；
   - 交互和 UI 是否覆盖功能的关键路径、失败和恢复路径；
   - 领域概念是否只定义不显然的术语，是否与正文一致。
3. 需求完整性原则（必须全部满足，否则打回）
   - 每个用户 `needs` 中的每一项都被该用户参与的一组故事完备覆盖，并在这些故事的 requirement 中有对应条目；
   - 覆盖同一需求的故事集合覆盖正常路径、需求前问题、进行中问题和完成后问题，每个问题都有结果和恢复方式，故事之间闭环；
   - 故事中与系统直接交互的人都在 `users` 中，每次交互的媒介都是 `functions` 中的功能；
   - 每个功能都至少被一个故事覆盖；
   - 每个功能都闭环：产生的系统变化能被其他功能消费，不存在无人消费的数据或无后续用途的展示；
   - 每个故事都覆盖自身涉及的异常情况：拒绝、失败、取消、权限不足、输入非法、超时、
     重复提交、并发冲突、外部依赖失败等，并写清结果和恢复方式；单个故事不能覆盖
     整条需求时，必须联合配套故事。
4. 结构校验
   ```bash
   arche doc check
   arche check
   ```
   失败即打回。

## 意见

- 每个阻塞问题写一条 Arche 评论：
  ```bash
  arche comment add <qname> '问题：...。通过条件：...。'
  ```
- 不重复未解决评论；不修改需求正文。
- 评论提交并推送到 `requirements/<task-id>`：
  ```bash
  arche add -A
  arche commit -m "Requirements review comments"
  arche push
  ```

## 结论

- 写评论后发送 `rejected`。
- 没有评论才发送 `approved`。
- 可行性审查通过时，用 `merge_request` 工具创建/刷新到 `design_space_branch` 的 MR，再发送 `approved`；MR 失败不要发送 `approved`。
