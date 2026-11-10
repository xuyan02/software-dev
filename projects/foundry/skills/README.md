# Skills

技能（skill）是给智能体阅读的 Markdown 文档，描述一类任务的标准、步骤或检查清单。技能不是可执行代码；智能体通过统一工具按名字加载。

## 目录约定

```
skills/
  <skill-name>.md
```

技能名使用小写 snake_case，例如 `project_brief_writing`，对应文件 `project-brief-writing.md`。文件名使用 kebab-case，技能名和元数据在 `crates/agent-runtime/src/skill.rs` 中登记。

## 智能体如何加载

所有智能体使用同一组工具，而不是为每个技能新增专用工具：

- `list_skills`：列出可用技能的 name、title、description；
- `load_skill`：传入 `name`，返回技能的完整内容。

提示词负责告诉智能体应该在什么时候加载哪个技能，例如：

> Before writing, call `load_skill` with name `project_brief_writing`.

## 新增技能

1. 在 `skills/` 下新增 Markdown 文件；
2. 在 `crates/agent-runtime/src/skill.rs` 的 `SKILLS` 数组中登记 name、title、description 和 `include_str!` 路径；
3. 把 `list_skills` / `load_skill` 加入需要它的 AgentDefinition 工具列表；
4. 在对应智能体的提示词里写明加载时机和技能名。

技能内容在编译时嵌入 `foundry-agentd`，修改 Markdown 后需要重新构建。
