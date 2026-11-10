# ADR 0006：项目隔离

- 状态：已接受
- 日期：2026-11-02

## 背景

Issue 和 Requirement 早期只靠一个全局列表存在。账户和登录会话到位后，必须回答“这个对象属于谁、谁能看见”。Foundry 要同时服务多个产品线/团队，不能把不同项目的议题混在一起，也不能靠客户端自觉传对项目。

## 决策

1. **项目是隔离边界。** 每个 Issue、Requirement 和事件都带必填 `project_id`；聚合构造函数拒绝 nil 项目。
2. **所有读写都按项目限定。** 列表按 `project_id` 过滤；按 id 获取对象后还要校验其 `project_id`，不匹配返回 `NotFound`，不泄露对象是否存在。
3. **成员角色是权限层。** 项目成员角色为 `viewer < member < maintainer < owner`：
   - Viewer：读取；
   - Member：创建、评论、推进；
   - Maintainer：处置和状态转换；
   - Owner：修改项目和管理成员。
4. **全局 admin 是运维角色。** 可见所有项目，并覆盖项目角色检查；但不会绕过业务策略（例如智能体不能关闭议题）。
5. **项目必须至少有一个 Owner。** 降级或移除最后一个 Owner 被拒绝；归档项目只读。
6. **当前项目由请求决定。** REST/MCP 依次读取 `X-Foundry-Project`、`foundry_project` Cookie、第一个可见活跃项目；Web 使用 Cookie 和项目切换表单。
7. **没有默认项目。** 项目由账户显式创建，创建者成为 Owner。`/setup` 只创建首个管理员，不创建任何项目；管理员随后在项目页创建第一个项目。启动配置不播种领域数据，也不在启动时补 Owner。（2026-11-03 修订：早期实现曾按配置创建并修复默认项目，现已移除；项目只以 UUID 和可修改的显示名标识，不再有 `key`。）
8. **跨项目引用暂不允许。** `duplicate_of` 等引用只在同一项目内解析；未来若要跨项目链接，需要显式的共享/授权模型。

## 后果

- 仓库、CLI、MCP 工具和 Web 页面都必须携带项目上下文；没有可见项目时 API 返回 `400 project_required`。
- Postgres schema 增加 `project_members`，并要求 Issue/Requirement 行始终有 `project_id`。
- 全局 admin 拥有较大权力；生产环境应限制其数量并审计操作。
- 项目删除为物理删除：Owner 或全局 admin 可以删除项目，同时删除项目范围内的 Task、需求、议题、简报、成员和相关事件。删除不可恢复。归档仍用于需要保留数据、只读封存的场景。

## 相关

- [ADR 0005：登录会话与身份派生](0005-login-sessions-and-identity.md)
- [领域模型](../domain/model.md)
- [状态机](../domain/state-machines.md)
