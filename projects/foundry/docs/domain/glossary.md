# 术语表

| 术语 | 含义 |
|---|---|
| Control plane（控制平面） | 所有工程动作的必经入口：记录状态、校验权限、调用执行面。 |
| Execution plane（执行面） | 真正干活的外部系统：Git、CI、沙箱、部署、聊天、模型推理。 |
| Problem space（问题空间） | 世界上发生了什么、谁想要什么、哪里需要讨论。Issue 属于这一层。 |
| Commitment space（承诺空间） | 我们决定解决什么、不解决什么、怎样算解决。Requirement 属于这一层。 |
| Issue（议题） | 问题空间的统一入口：反馈、缺陷、问题、想法、任务请求、事故。先收集和讨论，再决定去向。 |
| Triage（分诊） | 给 Issue 定性：类型、严重度、影响、负责人、范围，并决定下一步。 |
| Disposition（处置） | Issue 的最终去向：关闭、不处理、重复、转需求分析、转任务等。 |
| Work item（工作项） | Requirement / Task / Issue 等需要推进的对象的统称。它是可执行契约，不是便签。 |
| Evidence（证据） | 支撑某个状态转换的可验证产物：测试报告、契约校验、基准、评审记录。 |
| Gate（门禁） | 进入下一状态前必须满足的条件集合。由状态机 + 策略 + 证据共同决定。 |
| Actor | 执行动作的主体：Human、Agent 或 System。它由登录会话账户派生，不由请求体声明。 |
| Account（账户） | 稳定身份。人类和智能体都是账户；全局角色为 `admin` 或 `member`。 |
| Session（会话） | 登录后签发的短期凭证。数据库中只保存 token 的 SHA-256 哈希，可过期和撤销。 |
| Project（项目） | 隔离边界。Issue、Requirement 和事件都属于恰好一个项目。归档后只读。 |
| Project role（项目角色） | `viewer < member < maintainer < owner`，决定读取、贡献、处置和成员管理权限。 |
| Bootstrap admin | 由启动配置创建的第一个管理员，用于建立后续账户和项目。 |
| Capability token | 授予智能体在限定范围、限定时间内调用某类工具的短期凭证。 |
| Decision | 需要明确选择的事项，包含选项、标准、证据和负责人。 |
| ADR | Architecture Decision Record，长期架构决定的记录。 |
| Contract | 服务间接口约定（OpenAPI / Protobuf / JSON Schema / DB schema）。 |
| Lease | 智能体对任务的限时占用；过期后任务可被重新领取。 |
| Projection | 从事件或写模型派生的只读视图，用于看板和查询。 |
| Digital twin | 可重复运行的仿真环境，用来在执行前验证变更。 |
| Holdout scenario | 智能体看不到的验收场景，用于防止针对测试过拟合。 |
| Merge queue | 串行化合并、自动重跑验证的队列，避免并发合并互相破坏。 |
