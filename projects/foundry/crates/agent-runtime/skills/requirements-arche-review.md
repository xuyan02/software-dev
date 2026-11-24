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
   - 模块是否代表可独立管理的功能集，而不是代码目录。
2. 内容
   - 用户是否有需求，需求是否有故事覆盖；
   - 故事是否只满足一个需求，前置条件和关联需求是否完整；
   - 功能是否有用户可观测变化，是否有故事覆盖；
   - 交互和 UI 是否覆盖功能的关键路径、失败和恢复路径；
   - 领域概念是否只定义不显然的术语，是否与正文一致。
3. 结构校验
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
