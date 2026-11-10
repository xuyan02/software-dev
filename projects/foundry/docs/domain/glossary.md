# 术语表

| 术语 | 含义 |
|---|---|
| Control plane（控制平面） | 所有工程动作的必经入口：记录状态、校验权限、调用执行面。 |
| Execution plane（执行面） | 真正干活的外部系统：Git、CI、沙箱、部署、聊天、模型推理。 |
| Problem space（问题空间） | 世界上发生了什么、谁想要什么、哪里需要讨论。Issue 属于这一层。 |
| Commitment space（承诺空间） | 我们决定解决什么、不解决什么、怎样算解决。Requirement 属于这一层。 |
| Issue（议题） | 问题空间的统一入口：反馈、缺陷、问题、想法、任务请求、事故。先收集和讨论，再决定去向。分类在创建时确定。 |
| Disposition（处置） | Issue 的最终去向：关闭、不处理、重复、转需求分析、转任务等。 |
| Work item（工作项） | Requirement / Task / Issue 等需要推进的对象的统称。它是可执行契约，不是便签。 |
| Evidence（证据） | 支撑某个状态转换的可验证产物：测试报告、契约校验、基准、评审记录。 |
| Gate（门禁） | 进入下一状态前必须满足的条件集合。由状态机 + 策略 + 证据共同决定。 |
| Actor | 执行动作的主体：Human、Agent 或 System。它由登录会话账户派生，不由请求体声明。 |
| Account（账户） | 稳定身份。人类和智能体都是账户；全局角色为 `admin` 或 `member`。 |
| Session（会话） | 登录后签发的短期凭证。数据库中只保存 token 的 SHA-256 哈希，可过期和撤销。 |
| Project（项目） | 隔离边界。Issue、Requirement 和事件都属于恰好一个项目。归档后只读；删除会永久移除项目范围内全部数据。 |
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

## 生命周期与 AI 原生术语（RFC 0008–0016）

| 术语 | 含义 |
|---|---|
| Lifecycle（生命周期） | 项目从产品判断到退役的完整阶段序列。 |
| Stage（阶段） | 根 Task 的 workflow stage；项目在生命周期中的位置。 |
| Artifact（产物） | 生命周期中可版本化的结果；有类型、所有者、版本和血缘。 |
| ArtifactVersion | 产物的一次不可变快照；内容寻址。 |
| Lineage（血缘） | 产物之间有类型的关系图，用于追溯和影响分析。 |
| Baseline（基线） | 某一时刻被批准的一组产物版本。 |
| ChangeRequest（变更请求） | 对基线或已批准产物的受控变更提案。 |
| ImpactAnalysis（影响分析） | 沿血缘图计算受影响对象、风险和验证需求。 |
| Gate（门禁） | 进入下一状态/阶段前的条件：产物、证据、策略、批准。 |
| Waiver（豁免） | 有期限、有批准、有补偿措施的跳过或降低门禁的记录。 |
| Auxiliary system（辅助系统） | 提供分析、设计、生成、验证、编排能力的可替换系统。 |
| Capability（能力） | 辅助系统提供的一项有输入/输出契约的操作。 |
| Evidence（证据） | 支撑状态转换的可验证产物；有信任等级。 |
| Trust level（信任等级） | self_reported / tool_verified / independently_verified / human_approved / attested。 |
| Provenance（来源版本） | 产物或证据使用的工具、模型、提示词、配置、数据版本。 |
| Holdout | 不向实现者公开的验收场景；由独立验证方持有。 |
| AgentDefinition | Agent 的定义：名称、所有者、能力、角色和默认版本。 |
| AgentVersion | Agent 的不可变配置快照：模型、提示词、工具、限制、策略。 |
| AgentBinding | AgentVersion 与项目/工作流/角色的绑定。 |
| EvaluationSuite | Agent 或辅助能力的评测套件。 |
| AgentTrajectory | 一次 Agent 执行的轨迹：步骤、工具/模型调用、成本、结果。 |
| ClosureLevel（闭环层级） | L1 单任务、L2 单迭代、L3 单项目、L4 自举、L5 组织。 |
| Self-hosting（自举） | 用 Foundry 管理 Foundry 自身的研发。 |
| Human takeover | 人工接管：Agent 失败或主动升级时人类接手。 |

## 产品、知识、治理术语（RFC 0017–0019）

| 术语 | 含义 |
|---|---|
| Opportunity | 组合层的机会候选：市场、客户、技术、合规或内部改进。 |
| Portfolio | 项目组合：预算、容量、依赖、风险和优先级。 |
| ProjectBrief | 立项简报：问题、目标、成功指标、范围、约束、风险、干系人。 |
| SuccessMetric | 可度量的成功标准：基线、目标、测量方式、时间窗。 |
| KnowledgeItem | 知识条目：约定、术语、模式、FAQ、runbook、摘要。 |
| Authority（知识） | 知识权威性：canonical / reference / draft / deprecated / stale。 |
| ContextBundle | 为一次任务组装的上下文包：条目、来源、版本、预算。 |
| MemoryEntry | 记忆条目：task/project/agent 作用域，带来源和保留期。 |
| Policy | 可执行规则：谁能做什么、需要什么证据、谁批准。 |
| PolicyDecision | 策略评估结果：allow / deny / require_approval。 |
| CapabilityToken | 给 Agent/辅助系统的短期、限定范围凭证。 |
| RiskRegister | 风险登记：风险、概率、影响、缓解、负责人。 |
| RiskAcceptance | 显式接受风险：理由、批准、补偿、到期。 |
| AuditRecord | 审计记录：actor、动作、对象、策略、证据、结果、时间。 |
| DataClassification | 数据分类：public / internal / confidential / restricted。 |
| ComplianceEvidence | 合规证据：映射到控制项的可验证记录。 |
| KillSwitch | 紧急停止 Agent/工具/集成的手段。 |
