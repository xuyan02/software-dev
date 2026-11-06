# RFC 0002：项目流程、Issue 流程与权限策略技术方案

- 状态：Draft，待确认后实施
- 日期：2026-11-03
- 关联：[ADR 0002 控制平面](../adr/0002-control-plane-form.md)、[ADR 0003 Git 存内容，Postgres 存状态](../adr/0003-git-content-postgres-state.md)、[ADR 0004 Issue 是问题空间，Requirement 是承诺空间](../adr/0004-issue-vs-requirement.md)、[ADR 0006 项目隔离](../adr/0006-project-isolation.md)、[RFC 0001 Foundry MVP](0001-foundry-mvp.md)

> 2026-11-03：Task/Issue 流程模型已由 [RFC 0003](0003-task-workflow-model.md) 重新设计。本 RFC 中关于 Issue 状态、Task 聚合、项目流程对象和 Issue 权限的部分仅作历史参考；策略、审批、证据和外部执行面的总体方向仍可参考。

## 0. 结论

Foundry 下一阶段不是增加一个“审批按钮”，而是把软件工程拆成四个互不相同、但相互耦合的平面：

1. **流程定义平面**：项目有哪些阶段，Issue 有哪些处理阶段，阶段之间允许什么转换，每个阶段的退出条件是什么。
2. **领域对象平面**：Project、Issue、Requirement、Design、Task、Review、Evidence、Release、Deployment、Decision 等对象及其状态。
3. **权限与策略平面**：谁可以执行哪个动作，是否需要人类批准，需要哪些证据，是否需要双人控制。**Issue 权限由人类任意配置，可以放宽也可以收紧。**
4. **执行与证据平面**：人类、智能体、Git、CI、沙箱、部署系统实际执行动作，并把结果作为证据写回控制平面。

核心关系：

- **项目流程**规定“为什么做、先做什么、什么时候可以发布、什么时候复盘”。
- **Issue 流程**是项目流程的实际驱动力：项目通过创建、排序、处理、验证和关闭 Issue 来推进。
- **智能体**处理 Issue，但只能在人类配置的权限空间内行动。
- **人类**拥有限权配置权、承诺权和最终责任；可以把操作完全授权给智能体，也可以随时收紧。
- **控制平面**不替人类做业务决定，只保证配置显式、身份真实、状态合法、证据可查、事实不可伪造。

本方案不写代码；确认后再按第 11 节的阶段实施。

## 1. 目标与非目标

### 1.1 目标

1. 把项目生命周期和 Issue 处理流程建模为两个不同层次，而不是一个巨大的状态机。
2. 支持项目目标、路线图、迭代、开发、代码审查、测试、发布、运维反馈和复盘。
3. 支持一个项目多次发布；开发循环和发布循环独立但可耦合。
4. 让 Issue 成为项目推进的基本单位：人类和智能体都通过 Issue 工作。
5. 让项目 Owner **任意配置 Issue 权限**：可以把某个动作完全交给智能体，也可以要求人类批准、双人批准或直接禁止；可以逐项目、逐 Issue、逐类型、逐风险配置。
6. 让智能体可以在没有人类卡点的情况下自主执行，也可以在需要时被人类拦住；两种模式都由配置决定，而不是代码写死。
7. 为所有状态转换、审批、证据和策略变更留下不可变审计记录。
8. 在现有 Rust workspace、Postgres、HTTP/MCP/CLI/Web 架构上增量实现。

### 1.2 非目标

- 不规定团队必须使用某一种敏捷方法。流程模板是默认值，不是宗教。
- 不替代 Git、CI、沙箱或部署系统；Foundry 记录和约束它们，不重新实现它们。
- 不把“人类批准”设为所有动作的默认前提。默认策略可以偏保守，但 Owner 有权改成全自动。
- 不允许智能体默认自行扩大权限；但人类可以显式把权限配置权委托给智能体。
- 不在本方案中实现代码生成、模型训练或完整分布式调度器。

## 2. 总体架构

### 2.1 四个平面

```text
┌─────────────────────────────────────────────────────────────┐
│ 流程定义平面                                                 │
│ workflow templates · stages · transitions · exit criteria   │
└──────────────────────────────┬──────────────────────────────┘
                               │ 规定“有哪些动作、是否合法”
┌──────────────────────────────▼──────────────────────────────┐
│ 领域对象平面                                                 │
│ Project · Issue · Requirement · Design · Task · Review      │
│ Evidence · Release · Deployment · Decision                   │
└──────────────────────────────┬──────────────────────────────┘
                               │ 被谁、以什么条件推进
┌──────────────────────────────▼──────────────────────────────┐
│ 权限与策略平面                                               │
│ project policy · issue overrides · approvals · audit        │
└──────────────────────────────┬──────────────────────────────┘
                               │ 触发执行并接收结果
┌──────────────────────────────▼──────────────────────────────┐
│ 执行与证据平面                                               │
│ Human · Agent · Git · CI · Sandbox · Deploy · Monitoring     │
└─────────────────────────────────────────────────────────────┘
```

关键约束：

- **流程定义**回答“这个动作是否存在、这个状态是否合法”。
- **权限策略**回答“这个 actor 能不能做、要不要人类批准、要什么证据”。
- **领域对象**保存“现在是什么状态、谁改的、依据是什么”。
- **执行面**负责实际执行，结果通过 Evidence 和事件回到控制平面。

策略不能修改流程定义，流程定义也不能替代策略。比如状态机允许 `Resolved → Closed`，但策略可以要求人类批准、双人批准或直接禁止某个智能体执行；状态机不允许的转换，策略也不能凭空允许。

### 2.2 运行时组件

沿用现有六边形架构：

```text
domain ← app ← adapters
             ← api-http / api-mcp / web / cli
services/* → 组合根
```

新增或扩展：

- `crates/domain`：Policy、Approval、Evidence、Iteration、Release、Deployment、ProjectBrief 等纯领域对象。
- `crates/app`：PolicyService、ApprovalService、IterationService、ReleaseService、EvidenceService、WorkflowService。
- `crates/adapters/policy`：内置确定性策略引擎；后续可替换 OPA/Cedar。
- `crates/adapters/postgres`：策略版本、决策日志、审批、证据、迭代、发布等持久化。
- `crates/adapters/git`、`ci`、`deploy`：外部执行面适配器。
- `crates/api-http`、`api-mcp`、`web`、`cli`：同一套用例的不同入口。
- `services/control-plane`：组合根，增加后台 worker。

### 2.3 事件与审计

所有重要动作都产生领域事件，并写入现有 `events` 表：

- `project.brief_updated`
- `issue.created` / `issue.triaged` / `issue.stage_changed` / `issue.status_changed`
- `policy.draft_created` / `policy.activated` / `policy.override_created` / `policy.override_revoked`
- `approval.requested` / `approval.granted` / `approval.rejected` / `approval.expired`
- `evidence.attached` / `evidence.invalidated`
- `iteration.created` / `iteration.started` / `iteration.closed`
- `release.created` / `release.verified` / `release.approved` / `release.deployed` / `release.rolled_back`
- `deployment.health_changed`

事件只追加。修正通过新事件表达，不覆盖历史。

## 3. 流程模型

### 3.1 四个循环

```text
                    ┌──────────────── 项目循环 ────────────────┐
                    │ 目标/章程 → 路线图 → 迭代规划 → 复盘/调整 │
                    └───────────────┬──────────────────────────┘
                                    │ 选入 Issue
                                    ▼
┌──────────────── 开发/迭代循环 ─────────────────┐
│ Issue → 分析 → 设计 → 实现 → 评审 → 测试 → 完成 │
└──────────────────────────────┬────────────────┘
                               │ 累积候选变更
                               ▼
┌──────────────── 发布循环 ──────────────────────┐
│ 候选版本 → 验证 → 批准 → 部署 → 观察 → 回滚?   │
└──────────────────────────────┬────────────────┘
                               │ 运行数据/事故/反馈
                               ▼
┌──────────────── 运维/反馈循环 ─────────────────┐
│ 监控 → 事故 → 反馈 → 新 Issue → 回到项目循环    │
└───────────────────────────────────────────────┘
```

- **项目循环**决定方向和承诺。
- **开发/迭代循环**把 Issue 转化为可验证的变更。
- **发布循环**把变更变成可运行版本。
- **运维/反馈循环**把运行事实变成新的 Issue。
- **测试不是第五个循环**，而是贯穿四个循环的验证网络：开发内环、合并门禁、迭代验收、发布验证、线上监控。

### 3.2 项目阶段

| 阶段 | 目的 | 主要产物 | 退出条件 |
|---|---|---|---|
| 目标/章程 | 说明为什么做、成功是什么、不做什么 | ProjectBrief：目标、成功指标、非目标、约束、干系人、初始风险 | Owner 确认章程 |
| 路线图 | 把目标分解为可交付的结果和发布主题 | Roadmap、ReleasePlan 草案 | 形成基线，允许滚动调整 |
| 迭代规划 | 选择本轮要推进的 Issue，明确容量和依赖 | Iteration、Issue 选择、迭代目标 | 迭代计划确认 |
| 开发 | 需求分析、架构设计、模块设计、编码、评审、测试 | Requirement、DesignDoc、ADR、Task、Review、Evidence | Issue 达到可发布/可交付 |
| 发布 | 构建候选版本、验证、批准、部署、观察 | Release、Deployment、发布证据 | 发布成功并验证，或回滚 |
| 运维/反馈 | 监控、事故、客户反馈、数据反馈 | Incident Issue、Feedback Issue、指标 | 形成下一轮输入 |
| 复盘/关闭 | 评估目标、质量、流程和策略 | 复盘记录、指标、策略调整、归档决定 | 项目继续/进入下一阶段/归档 |

项目阶段不是一次性瀑布。路线图、架构基线和发布计划都是**滚动基线**：可以变，但变更要走 Decision/ADR 和策略规定的审批，而不是静默修改。

### 3.3 开发循环内部

开发循环发生在迭代内，至少包含以下步骤。不同 Issue 类型可以跳过某些步骤，但不能跳过被流程模板和策略要求的证据。

1. **需求分析**
   - 输入：Issue、用户反馈、数据、约束。
   - 输出：Requirement：问题、用户、验收场景、非目标、约束、优先级、风险。
   - 关键问题：为谁解决什么问题？怎样算解决？明确不做什么？
   - 退出：Requirement 评审通过；需要人类承诺时由策略决定。

2. **架构设计**
   - 输入：Requirement、现有系统、质量属性、约束。
   - 输出：架构方案、模块边界、接口、数据流、失败模式、ADR。
   - 关键问题：放在哪里？和谁交互？为什么不是另一种方案？
   - 退出：DesignDoc 接受；重大且不可逆的决定形成 ADR。

3. **模块设计**
   - 输入：架构方案、模块职责。
   - 输出：模块内部设计、数据结构、API、错误处理、测试策略。
   - 退出：设计评审通过；开放问题清空。

4. **编码**
   - 输入：设计、任务拆分。
   - 输出：代码变更、单元测试、静态检查结果。
   - 退出：本地/沙箱验证通过，准备评审。

5. **代码审查**
   - 输入：变更、设计、测试。
   - 输出：Review：发现、结论、批准/要求修改。
   - 退出：策略要求的评审通过；未解决问题有明确处置。

6. **测试**
   - 单元/静态：开发内环，快速反馈。
   - 集成/契约：合并门禁。
   - 系统/验收/回归：迭代结束。
   - 非功能/安全：按风险和发布要求。
   - 输出：TestRun、覆盖率、失败分析、Evidence。
   - 退出：策略和流程要求的测试证据齐全。

7. **迭代评审**
   - 演示可工作的结果，核对验收场景。
   - 决定 Issue 是完成、回到开发、延期还是拒绝。
   - 记录指标：周期时间、返工、缺陷逃逸、证据覆盖率。

### 3.4 发布循环

发布是独立循环，不等同于“迭代结束”。一个迭代可以发布零次、一次或多次；一个发布可以包含多个迭代的变更。

```text
ReleasePlan
   │
   ▼
创建候选版本（冻结范围、生成变更集）
   │
   ▼
验证：回归、性能、安全、兼容性、发布检查
   │
   ▼
发布批准（按策略，可由人类或获授权智能体执行）
   │
   ▼
部署到环境（staging → canary → production）
   │
   ▼
观察：健康指标、错误率、业务指标、告警
   │
   ├── 正常 → 关闭发布，更新 Issue 验证状态
   └── 异常 → 回滚 / 热修复 / 新 Issue
```

发布对象记录：

- 版本号、目标环境、包含的 Issue/变更/提交。
- 候选构建产物和哈希。
- 验证证据：测试、扫描、性能、兼容性、发布检查。
- 批准记录和策略版本。
- 部署记录、健康观察、回滚记录。
- 发布说明和已知问题。

**热修复**是发布循环的一条快速路径，不是绕过流程：它仍然需要最小证据、策略规定的批准和发布后验证，只是模板更短、时限更紧。

### 3.5 运维/反馈循环

- 监控和告警产生 Incident Issue。
- 客户反馈、支持工单、数据分析产生 Feedback Issue。
- 事故处理形成时间线、缓解措施、根因、复盘和后续 Issue。
- 复盘结论进入 Decision 或 ADR，不散落在聊天记录里。
- 运行指标反馈到路线图和发布计划。
- 线上验证结果决定 Issue 是否真正关闭，而不是代码合并即关闭。

### 3.6 项目流程与 Issue 流程的关系

| 项目流程 | Issue 流程 |
|---|---|
| 回答“为什么做、先做什么、何时发布” | 回答“这一件事怎么处理、何时算完成” |
| 由 Owner/规划者维护 | 由处理者（人类或智能体）推进 |
| 产出章程、路线图、迭代、发布 | 产出分析、设计、代码、评审、测试、证据 |
| 项目状态不是 Issue 状态的聚合 | Issue 可以跨迭代、跨发布、被阻塞或延期 |
| 通过 Issue 落地 | 通过阶段、任务、证据和审批推进 |

一个 Issue 可以：

- 在一个迭代内完成；
- 跨多个迭代；
- 被拆成多个 Task；
- 产生一个 Requirement 和多个 DesignDoc；
- 进入一个或多个 Release；
- 在发布后被验证关闭；
- 被拒绝、判重或延期。

项目可以有很多 Issue，但项目阶段不会因为“所有 Issue 都关闭”就自动前进；项目阶段由项目流程和人类/策略决定。

**项目流程不是每个 Issue 的处理流程，也不是每个 Issue 都带一份完整的项目流程。** 项目流程属于项目本身，一个项目只有一个项目流程实例；Issue 处理流程属于工作项，每个 Issue 有自己的 Issue 流程实例，但它只是 Issue 的处理路径，不携带项目流程的全部阶段。多数 Issue 只经过项目流程的一个子集：一个问题可能只经历“分诊 → 回答 → 关闭”，一个反馈可能只经历“分诊 → 拒绝”，只有需要交付的 Issue 才会进入需求分析、设计、开发、评审、测试和发布。项目流程可以在没有任何活跃 Issue 时继续推进，Issue 也可以不属于任何迭代或发布。

代码上不能把 `ProjectStage` 塞进 `Issue`，也不能用 Issue 状态聚合出项目阶段。Project、Iteration、Release 是项目级对象；Issue 只保存自己的 `workflow_template`、`workflow_version` 和 `stage`，通过链接参与某个迭代或发布。项目流程是这些项目级对象的状态与事件，加上人类/策略的推进，不是每个 Issue 内部的完整副本。

### 3.6.1 项目流程的代码表达

项目流程不使用单一线性 `ProjectStage` 表达，因为开发和发布循环会并行，单一线性能把“正在开发下一版”和“正在运维上一版”错误地压成一个状态。建议用三层表达：

1. **项目生命周期**：`ProjectLifecycle`，粗粒度治理状态。
2. **项目里程碑**：`ProjectMilestone`，记录“章程已批准、路线图已批准、首个迭代已规划、首个版本已部署、复盘已完成”等事实。
3. **项目级子流程**：`Iteration`、`Release`、`Deployment` 各自有状态机，承载开发循环和发布循环。

```rust
pub enum ProjectLifecycle {
    Initiation,
    Active,
    Closing,
    Closed,
    Archived,
}

pub enum ProjectMilestone {
    CharterApproved,
    RoadmapApproved,
    FirstIterationPlanned,
    FirstReleaseDeployed,
    OperationsReviewed,
    RetrospectiveClosed,
}

pub struct ProjectFlowDefinition {
    pub version: u32,
    pub initial: ProjectLifecycle,
    pub milestones: BTreeMap<ProjectMilestone, MilestoneDefinition>,
}

pub struct MilestoneDefinition {
    pub requires: Vec<ProjectMilestone>,
    pub artifacts: Vec<ArtifactKind>,
}

pub struct Project {
    pub flow_version: u32,
    pub lifecycle: ProjectLifecycle,
    pub achieved: BTreeSet<ProjectMilestone>,
    pub version: u32,
}
```

`ProjectFlowService` 执行 `ApproveBrief`、`ApproveRoadmap`、`PlanIteration`、`CreateRelease`、`DeployRelease`、`CloseProject` 等命令。每个命令都先检查 `ProjectFlowDefinition` 的前置条件，再走 `PolicyEngine`，最后修改 `Project` 并写事件；需要人类批准时创建 `ApprovalRequest`，不直接修改项目。`Iteration` 和 `Release` 的状态转换由各自的服务执行，但它们是否开始、是否允许发布、需要什么证据，由项目流程和策略共同约束。Issue 只通过 `iteration_issues`、`release_issues` 链接参与，不拥有项目流程。

项目推进不会自行发生，必须由某个 actor 提交项目命令。触发者有四类：

- **人类 Owner/项目负责人**：批准章程、路线图、发布和关闭项目。
- **项目管理者智能体**：准备章程和路线图、规划迭代、创建发布、请求审批；能否直接执行由项目策略决定。
- **执行型智能体**：处理 Issue 和 Task；默认不能推进项目阶段，除非被显式授权。
- **自动化服务**：定时任务、CI、部署、监控通过服务账户提交命令或写入 Evidence。外部系统只提供事实，不能伪造审批；CI 完成可以触发 `VerifyRelease`，但是否发布仍由策略和人类批准决定。

外部 CI/部署系统不直接修改 `Project`、`Release` 或 `Issue` 状态；它们写入 Evidence、Deployment 或事件，再由服务账户提交命令。这样“触发”“授权”“执行”“证据”各自有明确归属，项目状态不会因为一个 webhook 被悄悄推进。


### 3.7 Issue 类型与工作流模板

Issue 保留现有 `kind`，并增加 `workflow_template`。模板决定阶段序列和退出证据，策略决定谁能推进、是否需要批准。

| 类型 | 默认模板 | 典型阶段 |
|---|---|---|
| `feedback` | `feature` | 分诊 → 需求分析 → 设计 → 实现 → 评审 → 验证 → 发布 → 关闭 |
| `bug` | `bugfix` | 分诊 → 复现 → 修复 → 评审 → 测试 → 发布/热修复 → 验证 → 关闭 |
| `incident` | `incident` | 分诊 → 缓解 → 根因 → 修复 → 复盘 → 关闭 |
| `idea` | `discovery` | 分诊 → 评估 → 转需求/拒绝 |
| `question` | `support` | 分诊 → 回答 → 关闭 |
| `task_request` | `task` | 分诊 → 计划 → 实现 → 评审 → 完成 |

模板是流程定义，不是权限。Owner 可以选择模板；如果模板不适用，应创建新的模板版本，而不是用策略绕过状态机。

### 3.8 迭代、发布与 Issue 的关系

```text
Project
 ├── ProjectBrief
 ├── Roadmap
 ├── Iteration 1 ── selected Issues
 │      └── Issue A ── Requirement / Design / Tasks / Review / Evidence
 ├── Iteration 2 ── selected Issues
 │      └── Issue B
 ├── Release 1.4 ── Issue A + Issue B + commits
 │      └── Deployment(staging) → Deployment(production)
 └── Release 1.4.1 ── hotfix Issue C
```

- 迭代是计划单位。
- 发布是交付单位。
- Issue 是工作单位。
- 三者是多对多关系，通过链接对象建立，不用状态字段硬编码。

## 4. 领域模型

### 4.1 对象总表

| 对象 | 回答的问题 | 关键状态 |
|---|---|---|
| Project | 哪个隔离边界 | Active / Archived |
| ProjectBrief | 为什么做、成功是什么、不做什么 | Draft / Approved / Superseded |
| RoadmapItem | 未来一段时间要达成什么结果 | Draft / Planned / InProgress / Done / Cancelled |
| Iteration | 本轮计划做什么 | Planned / Active / Closed |
| Issue | 世界上发生了什么、要处理什么 | Open / Triaged / InProgress / Resolved / Closed / Rejected / Duplicate |
| IssueStage | 当前处理到哪一步 | 由工作流模板定义 |
| Requirement | 为谁解决什么问题、怎样算解决 | Draft / InReview / Approved / InDesign / InDevelopment / InVerification / Done / Rejected / Deferred / Superseded |
| DesignDoc | 打算怎么实现、权衡是什么 | Draft / InReview / Accepted / Superseded |
| ADR | 为什么做长期决定 | Proposed / Accepted / Rejected / Superseded / Deprecated |
| Task | 谁在什么时候做什么 | Todo / InProgress / Blocked / Review / Done / Cancelled |
| Review | 谁按什么标准检查过 | Pending / Approved / ChangesRequested |
| Evidence | 什么事实支持这个决定 | Recorded / Invalidated |
| Approval | 谁批准了什么、绑定哪个版本 | Pending / Approved / Rejected / Expired / Invalidated |
| Release | 哪些变更一起上线 | Planned / Candidate / Verified / Released / RolledBack |
| Deployment | 哪个版本部署到哪个环境 | Pending / Deploying / Healthy / Unhealthy / RolledBack |
| Decision | 哪个选项被选中、谁负责 | Open / Decided / Reopened / Superseded |
| ProjectPolicy | 谁可以做什么、要什么条件 | Draft / Active / Superseded |
| PolicyDecision | 某次动作为什么被允许/拒绝/要求批准 | 只追加 |
| Event | 发生过什么 | 只追加 |

### 4.2 ProjectBrief

ProjectBrief 是项目目标的可版本化承诺，不塞进 Project 的普通字段里。

核心字段：

- `goal`：项目要达成的结果。
- `success_metrics`：可度量的成功标准。
- `non_goals`：明确不做什么。
- `stakeholders`：谁关心、谁提供资源、谁承担风险。
- `constraints`：时间、预算、合规、技术、依赖。
- `initial_risks`：已知风险和缓解假设。
- `status`：Draft / Approved / Superseded。
- `version`、`approved_by`、`approved_at`。

变更走新版本；已批准版本不被静默覆盖。

### 4.3 Iteration

- `id`、`project_id`、`name`、`goal`。
- `start_at`、`end_at`。
- `status`：Planned / Active / Closed。
- `capacity`：人力/智能体容量、预算或 WIP 限制。
- `issue_ids`：本轮选中的 Issue。
- `exit_criteria`：本轮怎样算完成。
- `review_notes`：迭代评审结论。
- 版本、时间戳。

迭代关闭不要求所有 Issue 关闭；未完成 Issue 明确回到 Backlog、转到下一迭代或被拒绝。

### 4.4 Release 与 Deployment

Release：

- `id`、`project_id`、`version`、`name`。
- `status`：Planned / Candidate / Verified / Released / RolledBack。
- `scope`：Issue、变更、提交、构建产物。
- `target_environment`、`release_notes`。
- `evidence_ids`、`approval_ids`。
- `candidate_artifact`、`artifact_hash`。
- `released_at`、`rolled_back_at`。

Deployment：

- `release_id`、`environment_id`。
- `status`：Pending / Deploying / Healthy / Unhealthy / RolledBack。
- `deployed_at`、`health_checked_at`。
- `metrics_snapshot`、`rollback_of`。

Environment：

- `name`、`kind`：development / staging / production。
- `protected`：是否受更严格策略约束。
- `owner`、`description`。

发布和 Issue 通过 `release_issues` 关联。Issue 的关闭可以要求“在目标环境验证通过”，具体由流程模板和策略决定。

### 4.5 Issue 扩展

保留现有 Issue 状态机，增加：

- `workflow_template`：feature / bugfix / incident / discovery / support / task。
- `stage`：当前处理阶段，由模板定义；只在 `InProgress` 期间有意义。
- `stage_history`：阶段、actor、时间、证据、策略决策。
- `links`：Requirement、Design、Task、Release、Decision。
- `visibility`：项目内可见范围；敏感 Issue 可限制。
- `policy_override_ids`：该 Issue 的显式权限覆盖。

状态与阶段的关系：

```text
Open ──triage──► Triaged ──start──► InProgress ──resolve──► Resolved ──close──► Closed
  │                 │                   │
  │                 │                   └── stage: analysis → design → implementation
  │                 │                              → review → verification → release
  │                 └──► Rejected / Duplicate
  └──► Rejected / Duplicate
```

- `status` 是粗粒度生命周期。
- `stage` 是 `InProgress` 内的细粒度流程。
- 不是所有 Issue 都需要 `Resolved → Closed`；具体模板和策略决定是否允许直接关闭、是否需要发布验证。
- 状态机负责合法性；策略负责权限和审批；两者不能互相替代。

### 4.6 Task、Review、Evidence、Approval

Task：

- 属于一个 Issue、一个迭代。
- 有负责人、阶段、租约、尝试次数。
- 状态：Todo / InProgress / Blocked / Review / Done / Cancelled。
- 完成时可附带 Evidence。

Review：

- 评审对象：Requirement、DesignDoc、代码变更、Release、Issue 处置。
- 评审标准、发现、结论、评审人。
- 评审人不能默认等于作者；是否允许自审由策略决定，系统如实记录。

Evidence：

- `kind`：test_run / ci_pipeline / review / security_scan / benchmark / deployment / monitoring / attestation / manual。
- `source`、`uri`、`hash`、`summary`、`produced_by`、`produced_at`。
- 可以绑定 Issue、Task、Requirement、Design、Release、Deployment。
- 内容大时存 Git/对象存储；Postgres 存元数据和哈希。
- 证据可失效；失效后依赖它的审批也失效。

Approval：

- 通用审批请求，不按动作类型建很多表。
- 绑定：subject 类型、ID、版本、动作、策略版本、规则 ID、证据哈希。
- 状态：Pending / Approved / Rejected / Expired / Invalidated。
- 记录每个批准/拒绝的 actor、时间、意见。
- 自审、代理审批、双人控制都如实记录，不伪装独立性。

## 5. 权限与策略

这是本方案的核心。

### 5.1 基本原则

1. **人类主权**：项目 Owner 对 Issue 权限有完全配置权。可以放宽，也可以收紧；没有“只能收紧”的单调约束。
2. **任意配置**：Owner 可以按动作、Issue 类型、严重度、风险、actor、阶段、环境、标签、单个 Issue 配置权限和审批要求。
3. **显式配置**：任何权限决定都来自显式规则、显式默认值或显式 Issue 覆盖；不存在藏在代码里的“人类必须批准”。
4. **智能体不自我授权**：智能体默认不能修改策略；人类可以显式把策略修改权委托给智能体，但这是独立的高风险权限，必须审计。
5. **系统事实不可伪造**：身份、审计、项目隔离、数据完整性、状态机合法性、策略版本记录不可被策略关闭。
6. **领域合法性独立**：策略决定“谁可以做”，领域规则决定“这个动作本身是否成立”。Owner 可以允许智能体关闭 Issue，但不能让系统接受一条没有 resolution 的关闭记录。
7. **可审计**：每次权限判断记录使用的策略版本、命中规则、Issue 覆盖、actor、subject 版本、证据哈希和结果。
8. **确定性**：相同输入、相同策略版本、相同对象版本，得到相同结果；不依赖隐式环境或随机性。

### 5.2 三个层次

```text
系统事实（不可配置）
  身份、会话、项目边界、审计、数据完整性、策略版本、真实 actor 记录
        │
        ▼
项目策略（Owner 任意配置）
  动作 × 条件 → auto / notify / human_required / dual_control / blocked / escalate
        │
        ▼
Issue 覆盖（人类显式配置，可双向）
  单个 Issue/动作 → 放宽或收紧；有理由、有效期、审计
```

#### 系统事实

这些不是“Issue 权限”，不能被策略关闭：

- actor 只能来自登录会话，不能伪造。
- 非项目成员不能访问项目对象；跨项目默认不可见。
- 状态机不允许的转换不能执行。
- 领域必填字段和引用完整性不能绕过。
- 策略变更必须版本化并记录修改者。
- 审批记录必须反映真实 actor；自审可以被允许，但必须标记。
- 事件只追加，审计不能删除。

#### 项目策略

Owner 可以配置任意 Issue 动作的结果。包括：

- 允许智能体直接执行（`auto`）。
- 执行但通知人类（`notify`）。
- 要求人类批准（`human_required`）。
- 要求两个不同 actor 批准（`dual_control`）。
- 直接禁止（`blocked`）。
- 升级到指定队列/角色（`escalate`）。

没有“高风险动作必须人类批准”的硬编码。系统可以提供模板、风险提示和模拟，但不替 Owner 做决定。

#### Issue 覆盖

人类可以对单个 Issue 设置显式覆盖：

- 把 `human_required` 放宽为 `auto`。
- 把 `auto` 收紧为 `human_required` 或 `dual_control`。
- 指定额外批准人、证据要求、有效期。
- 覆盖可以只针对某个动作，也可以针对某个阶段。
- 覆盖必须记录理由、配置者、创建时间、失效时间。
- 智能体不能自行创建覆盖；人类可以授予智能体覆盖权限，但默认不授予。

覆盖优先于项目策略，但仍在系统事实和领域合法性之下。

### 5.3 可配置维度

策略规则可以匹配：

- `action`：`issue.create`、`issue.comment`、`issue.triage`、`issue.assign`、`issue.stage.enter`、`issue.stage.exit`、`issue.transition`、`issue.close`、`issue.reopen`、`issue.reject`、`issue.duplicate`、`issue.override_policy`、`requirement.approve`、`design.approve`、`review.submit`、`review.approve`、`test.run`、`release.approve`、`release.deploy`、`release.rollback`、`policy.edit`。
- `issue.kind`、`issue.severity`、`issue.risk`、`issue.labels`、`issue.area`。
- `issue.stage`、`issue.status`、目标状态。
- `actor.kind`：human / agent。
- `actor.id`、`actor.role`、`actor.project_role`。
- `environment`、`release`、`deployment`。
- `reversibility`：可逆 / 不可逆。
- `evidence` 是否存在、类型、来源、新鲜度。
- `time`：生效时间、冻结窗口、临时授权。
- `custom`：项目自定义属性。

### 5.4 规则模型

策略是一个版本化文档，整体存 JSONB，关键字段建索引。规则按优先级和具体程度排序。

```json
{
  "policy_id": "0f8f...",
  "project_id": "6a2c...",
  "version": 3,
  "status": "active",
  "default_outcome": "human_required",
  "rules": [
    {
      "id": "agent-close-low-risk-bug",
      "priority": 100,
      "when": {
        "action": "issue.transition",
        "to": "closed",
        "issue.kind": ["bug"],
        "issue.severity": ["s3", "s4"],
        "actor.kind": ["agent"]
      },
      "outcome": "auto",
      "require": {
        "evidence": ["test_run", "review"]
      },
      "notify": ["#engineering"]
    },
    {
      "id": "security-incident-close",
      "priority": 200,
      "when": {
        "action": "issue.transition",
        "to": "closed",
        "issue.labels": ["security"]
      },
      "outcome": "dual_control",
      "approvers": {
        "roles": ["owner", "security_lead"],
        "distinct_actors": true
      },
      "require": {
        "evidence": ["security_review", "deployment"]
      }
    }
  ]
}
```

规则字段：

- `id`：稳定标识，进入决策日志。
- `priority`：数值越大越优先；同一优先级按具体程度决定。
- `when`：条件表达式；空表示匹配所有。
- `outcome`：`auto` / `notify` / `human_required` / `dual_control` / `blocked` / `escalate`。
- `approvers`：角色、账户、项目角色、最少人数、是否要求不同 actor。
- `require.evidence`：证据类型和条件。
- `notify`：通知目标。
- `expires_at`：临时规则。
- `reason`：规则目的，便于审计和 UI 展示。

Issue 覆盖示例：

```json
{
  "issue_id": "9b1d...",
  "action": "issue.transition",
  "to": "closed",
  "outcome": "auto",
  "reason": "该 Issue 已验证，Owner 临时授权智能体关闭",
  "created_by": "human-account-id",
  "expires_at": "2026-11-10T00:00:00Z"
}
```

覆盖可以是放宽，也可以是收紧。系统记录 `direction: loosen` 或 `direction: tighten`，但不把它当作非法。

### 5.5 决策算法

```text
1. 从会话解析 actor；检查账户 active、项目成员、项目 active。
2. 加载当前 active 策略版本和该 Issue 的显式覆盖。
3. 检查领域规则：状态转换存在、必填字段齐全、对象版本一致。
4. 匹配 Issue 覆盖；若存在且未过期，使用覆盖结果。
5. 否则匹配项目策略规则：条件全满足的规则中，按优先级和具体程度排序，取第一条。
6. 没有命中规则时使用 default_outcome；没有 default 时返回 human_required 并记录“策略未配置”，Owner 可以改成任何值。
7. 结果为 auto/notify：执行；notify 额外发通知。
8. 结果为 human_required/dual_control：创建 ApprovalRequest，动作进入 pending，不执行。
9. 结果为 blocked：拒绝，返回 403，并记录策略决策。
10. 结果为 escalate：创建升级请求，进入指定队列。
11. 每次判断写 PolicyDecision：策略版本、命中规则、覆盖、actor、subject 版本、证据哈希、结果、理由。
```

审批通过后：

```text
1. 重新加载 subject 当前版本、策略版本、证据哈希。
2. 如果 subject 版本变化、证据变化或策略版本变化，重新评估。
3. 新策略仍要求相同审批且条件满足：执行。
4. 新策略不再要求审批：执行（记录原因）。
5. 新策略要求更多审批：旧审批失效，创建新的 ApprovalRequest。
6. 执行必须幂等；同一个 approval 不能执行两次。
```

### 5.6 审批与职责分离

- 审批是通用对象，不按动作类型重复建模。
- `ApprovalRequest` 绑定 subject 版本和证据哈希；对象变化后旧审批失效。
- `dual_control` 默认要求不同 actor，但 Owner 可以配置是否允许同一 actor 多次批准；系统如实记录。
- 允许智能体作为批准人，前提是策略显式授权。
- 允许自审，前提是策略显式允许；系统标记 `self_approval=true`，不伪装成独立审批。
- 审批可以设置过期时间；过期后必须重新请求。
- 审批拒绝后，动作不执行；拒绝理由进入历史。
- 审批不能绕过领域规则和系统事实。

### 5.7 Issue 级覆盖的双向性

这是对之前设计的修正：

- **不再有“Issue 级别只能收紧”的限制。**
- 人类可以把项目策略的 `human_required` 放宽为某个 Issue 的 `auto`。
- 人类也可以把项目策略的 `auto` 收紧为某个 Issue 的 `human_required` 或 `dual_control`。
- 覆盖可以针对单个动作、整个 Issue 或某个阶段。
- 覆盖必须显式、有理由、有有效期、可撤销、可审计。
- 覆盖不改变项目策略本身，只影响该 Issue。
- 覆盖不能违反系统事实和领域合法性。
- 智能体可以提出覆盖请求，但不能自行批准；除非人类显式授予其覆盖权限。

### 5.8 智能体权限

智能体是账户，`kind=agent`。它可以：

- 读取其项目内可见的 Issue。
- 创建、评论、分诊、推进阶段、提交证据、请求审批。
- 在策略允许时直接执行动作。
- 在策略允许时批准动作。
- 在人类显式授权时修改策略或 Issue 覆盖。

智能体不能：

- 伪造 actor 身份。
- 绕过项目隔离。
- 自行扩大权限。
- 默认修改策略。
- 默认批准自己的产出。
- 绕过领域必填字段和状态机。

默认策略可以保守，但默认值不是硬限制。Owner 可以把默认策略改成全自动；系统只记录和展示风险。

### 5.9 策略变更与审计

- 策略有 Draft / Active / Superseded 状态。
- 每次修改生成新版本，记录修改者、时间、理由、旧值、新值。
- 同一项目同一时间只有一个 Active 版本。
- 策略激活是显式动作。
- 策略变更产生事件并写入 `events`。
- 运行中的决策记录策略版本；历史决策不因新版本而改写。
- Pending 审批在策略变更后重新评估，具体规则见 5.5。
- 策略模拟接口允许人类在激活前查看“某个动作会被允许、拒绝还是要求批准”，但模拟不产生业务副作用。

### 5.10 示例

#### 示例 A：低风险 Bug 自动关闭

项目策略：

```json
{
  "when": {
    "action": "issue.transition",
    "to": "closed",
    "issue.kind": ["bug"],
    "issue.severity": ["s3", "s4"],
    "actor.kind": ["agent"]
  },
  "outcome": "auto",
  "require": { "evidence": ["test_run"] }
}
```

结果：智能体可以在有测试证据时直接关闭低风险 Bug。人类在审计日志中看到每次关闭。

#### 示例 B：安全事件双人批准

```json
{
  "when": {
    "action": "issue.transition",
    "to": "closed",
    "issue.labels": ["security"]
  },
  "outcome": "dual_control",
  "approvers": { "roles": ["owner", "security_lead"], "distinct_actors": true }
}
```

结果：即使智能体完成了修复，也需要两个批准人；Owner 可以改成单人或自动，这是配置选择。

#### 示例 C：单个 Issue 放宽

项目默认要求人类关闭 Issue。Owner 对某个已验证的 Issue 设置覆盖：

```json
{
  "issue_id": "...",
  "action": "issue.transition.closed",
  "outcome": "auto",
  "reason": "已验证，允许智能体关闭",
  "expires_at": "2026-11-10T00:00:00Z"
}
```

结果：只有这个 Issue 可以由智能体关闭；其他 Issue 仍要求人类。覆盖到期后恢复项目策略。

#### 示例 D：单个 Issue 收紧

项目默认允许智能体关闭低风险 Bug。Owner 对某个客户投诉 Issue 设置：

```json
{
  "issue_id": "...",
  "action": "issue.transition.closed",
  "outcome": "human_required",
  "reason": "客户投诉，需要人工确认"
}
```

结果：只有这个 Issue 需要人类批准。系统记录 `direction: tighten`。

#### 示例 E：智能体获得策略修改权

Owner 显式授予某个智能体 `policy.edit`。该智能体可以创建策略草案，但激活仍可按策略要求人类批准。所有修改记录 actor、版本和理由。这是高风险权限，默认不授予。

## 6. 数据模型

### 6.1 新增表

| 表 | 用途 |
|---|---|
| `project_briefs` | 项目章程版本 |
| `roadmap_items` | 路线图条目 |
| `iterations` | 迭代 |
| `iteration_issues` | 迭代与 Issue 多对多 |
| `releases` | 发布 |
| `release_issues` | 发布与 Issue 多对多 |
| `environments` | 环境 |
| `deployments` | 部署记录 |
| `project_policies` | 策略版本快照 |
| `policy_overrides` | Issue 级覆盖 |
| `policy_decisions` | 权限决策日志，只追加 |
| `approval_requests` | 审批请求 |
| `approval_decisions` | 审批动作 |
| `evidence` | 证据元数据 |
| `evidence_links` | 证据与对象关联 |
| `idempotency_keys` | Webhook/审批/执行幂等 |

### 6.2 修改现有表

- `issues`：增加 `workflow_template`、`stage`、`visibility`、`stage_version`。
- `tasks`：增加 `issue_id`、`iteration_id`、`lease_expires_at`、`attempts`（如果现有模型没有）。
- `projects`：不把目标字段塞进名称；通过 `project_briefs` 关联。
- `events`：增加 `policy_decision_id`、`approval_id`、`evidence_id` 等可选关联字段，或通过 payload 存储。

### 6.3 存储约定

- 聚合快照存 JSONB；标量列用于过滤、索引和唯一约束。
- 所有写操作使用 `version` 乐观并发。
- 策略文档整体存 JSONB，`project_id + version` 唯一。
- 每个项目最多一个 Active 策略，用部分唯一索引保证。
- `policy_decisions` 只插入，不更新、不删除。
- 证据大文件存 Git/对象存储；数据库存 `uri`、`hash`、`kind`。
- 审批和执行使用 `idempotency_key`，防止重复执行。
- 迁移策略：先加可空列/新表，回填默认策略和 Issue 模板，再加约束；不删除现有数据。

### 6.4 默认策略迁移

已有项目在迁移时创建策略版本 1，内容等价于当前 DemoPolicy：

- 人类可以执行全部现有动作。
- 智能体可以创建、评论、分诊、推进到 Resolved，但不能关闭、拒绝、判重、重开、批准需求。
- 项目角色仍作为默认条件。
- 该策略是普通的可编辑策略，不是硬编码；Owner 可以立即放宽或收紧。

这样既保持现有行为，又不会把旧规则变成不可修改的系统事实。

## 7. API、MCP 与 CLI

### 7.1 项目流程

| 方法 | 路径 | 说明 |
|---|---|---|
| `GET` | `/api/v1/projects/{id}/brief` | 读取章程 |
| `PUT` | `/api/v1/projects/{id}/brief` | 创建/更新章程版本 |
| `POST` | `/api/v1/projects/{id}/brief/approve` | 批准章程 |
| `GET` | `/api/v1/projects/{id}/roadmap` | 路线图 |
| `POST` | `/api/v1/projects/{id}/roadmap` | 增加路线图条目 |
| `GET` | `/api/v1/projects/{id}/iterations` | 迭代列表 |
| `POST` | `/api/v1/projects/{id}/iterations` | 创建迭代 |
| `POST` | `/api/v1/iterations/{id}/issues` | 选入 Issue |
| `POST` | `/api/v1/iterations/{id}/close` | 关闭迭代 |
| `GET` | `/api/v1/projects/{id}/releases` | 发布列表 |
| `POST` | `/api/v1/projects/{id}/releases` | 创建发布 |
| `POST` | `/api/v1/releases/{id}/issues` | 加入 Issue |
| `POST` | `/api/v1/releases/{id}/transitions` | 推进发布状态 |
| `POST` | `/api/v1/releases/{id}/deployments` | 创建部署 |
| `POST` | `/api/v1/deployments/{id}/health` | 回写健康状态 |

### 7.2 策略与审批

| 方法 | 路径 | 说明 |
|---|---|---|
| `GET` | `/api/v1/projects/{id}/policy` | 当前策略和版本历史 |
| `POST` | `/api/v1/projects/{id}/policy/versions` | 创建草案 |
| `POST` | `/api/v1/projects/{id}/policy/versions/{version}/activate` | 激活 |
| `POST` | `/api/v1/projects/{id}/policy/simulate` | 模拟决策 |
| `POST` | `/api/v1/issues/{id}/policy-overrides` | 创建覆盖 |
| `DELETE` | `/api/v1/issues/{id}/policy-overrides/{override_id}` | 撤销覆盖 |
| `GET` | `/api/v1/policy-decisions` | 决策日志 |
| `GET` | `/api/v1/approvals` | 待审批列表 |
| `POST` | `/api/v1/approvals/{id}/approve` | 批准 |
| `POST` | `/api/v1/approvals/{id}/reject` | 拒绝 |
| `POST` | `/api/v1/evidence` | 记录证据 |
| `GET` | `/api/v1/evidence/{id}` | 查看证据 |

### 7.3 Issue 阶段

| 方法 | 路径 | 说明 |
|---|---|---|
| `POST` | `/api/v1/issues/{id}/stages` | 进入/退出阶段，带证据和版本 |
| `GET` | `/api/v1/issues/{id}/stages` | 阶段历史 |
| `POST` | `/api/v1/issues/{id}/transitions` | 保留现有状态转换 |

阶段转换请求：

```json
{
  "to": "review",
  "expected_version": 12,
  "evidence_ids": ["..."],
  "reason": "实现完成，提交评审"
}
```

如果策略要求审批，返回：

```json
{
  "status": "approval_required",
  "approval_id": "...",
  "policy_decision_id": "..."
}
```

而不是假装动作已经完成。

### 7.4 MCP 工具

新增：

- `get_project_brief` / `update_project_brief`
- `list_iterations` / `create_iteration` / `close_iteration`
- `list_releases` / `create_release` / `transition_release`
- `list_issue_stages` / `transition_issue_stage`
- `get_project_policy` / `simulate_policy`
- `create_issue_override` / `revoke_issue_override`
- `list_pending_approvals` / `approve` / `reject`
- `attach_evidence`
- `claim_task` / `heartbeat_task` / `complete_task`
- `create_deployment` / `report_deployment_health`

工具仍只操作当前项目；权限由会话、项目上下文和策略共同决定。权限不足通过 `tools/call` 的 `isError` 返回，不产生 JSON-RPC 协议错误。

### 7.5 CLI

新增命令组：

```bash
foundry project brief
foundry project roadmap
foundry iteration list|create|close
foundry release list|create|transition|deploy
foundry issue stage <ID> --to review --evidence <ID>
foundry policy get|simulate|activate
foundry policy override <ISSUE> --action close --outcome auto --reason "..."
foundry approval list|approve|reject
foundry evidence attach
```

## 8. 执行面集成

### 8.1 智能体执行

- 智能体使用账户登录，拿会话 token；项目通过 `X-Foundry-Project` 或 MCP 上下文确定。
- 智能体通过 `claim_task` 获取 Task 租约；租约包含 `lease_expires_at`，需要心跳续租。
- 执行在沙箱中进行；命令、日志、产物作为 Evidence 回传。
- 智能体不能直接改策略；只能按策略行动或请求审批。
- 人类可以在控制台看到任务时间线、工具调用、证据和策略决策。
- 任务失败/超时可重试；重试次数和租约由配置控制。

### 8.2 Git

- 变更关联 Issue/Task；PR/MR 作为 Review 和代码证据的来源。
- 提交、分支、合并结果记录哈希；分支策略不强制。
- 发布候选引用固定 commit/构建产物，保证可追溯。
- Git 事件通过签名 webhook 进入控制平面，使用幂等键去重。

### 8.3 CI

- Pipeline 结果、测试报告、扫描结果作为 Evidence。
- 合并门禁和发布门禁读取证据，不由 Foundry 重新跑 CI。
- CI 失败可以自动创建/更新 Issue，或阻止阶段转换。

### 8.4 沙箱

- 每个执行任务有隔离工作区和资源限制。
- 命令日志、退出码、diff、测试输出进入 Evidence。
- 凭据由沙箱注入，不进入 Issue 评论或模型上下文。

### 8.5 部署与监控

- 部署记录绑定 Release、Environment、artifact hash。
- 健康检查、错误率、业务指标作为 Deployment Evidence。
- 异常可以触发自动回滚或创建 Incident Issue，具体由策略决定。
- 线上验证通过后，Issue 才能按模板关闭。

## 9. 安全与隔离

- 身份只来自会话；请求体不接受 actor。
- REST/MCP 使用 Bearer token；Web 使用 HttpOnly Cookie。
- 项目隔离优先于一切：跨项目对象按 404 处理。
- 策略服务端评估，客户端不能提交“我已批准”作为事实。
- 审批绑定对象版本和证据哈希，防止“批准后偷换内容”。
- 策略修改和审批记录不可删除；审计只追加。
- 敏感 Issue 支持项目内可见性限制；系统事实仍保证非成员不可见。
- 智能体账户使用独立凭据；沙箱隔离；密钥不进入日志。
- 公网部署前必须补 TLS、CSRF、速率限制、密钥轮换；当前仍只适合本机/受信网络。
- 人类可以任意放宽 Issue 权限，但系统会记录风险决策；Owner 对配置后果负责，系统不提供隐藏的硬性否决。

## 10. 可观测性

关键指标：

- Issue 周期时间、阶段停留时间、返工次数。
- 自动执行 vs 人类审批比例。
- 审批等待时间、拒绝率、过期率。
- 策略拒绝次数、覆盖次数、放宽/收紧比例。
- 发布频率、变更失败率、回滚率、MTTR。
- 证据覆盖率：有多少转换有要求的证据。
- 智能体任务成功率、租约超时、重试次数。

日志和追踪：

- 每个 PolicyDecision 有唯一 ID，可在 Web、日志、事件中关联。
- 每个 Approval 关联 subject 和 policy decision。
- 每个 Release 关联 Issue、证据、部署和回滚。
- 事件 payload 带 actor、project、subject version。

## 11. 实施计划

### 阶段 0：确认设计

- 评审本 RFC。
- 确认 Issue 状态与 stage 的关系。
- 确认策略存储和默认策略迁移。
- 确认是否首期实现 Iteration/Release，还是先做 Policy。

### 阶段 1：策略与审批基础

交付：

- `ProjectPolicy`、`PolicyRule`、`PolicyDecision`、`ApprovalRequest` 领域对象。
- 确定性策略引擎，替换 `DemoPolicy`。
- Postgres 持久化、版本、默认策略迁移。
- REST/MCP/CLI/Web 的策略读取、编辑、模拟、审批。
- 策略决策日志和审计事件。
- 测试：放宽、收紧、覆盖、审批、版本冲突、自审标记。

验收：

- Owner 可以把低风险 Bug 关闭设为 `auto`，智能体执行成功。
- Owner 可以改回 `human_required`，智能体收到审批要求。
- Issue 覆盖可以双向配置，审计显示方向。
- 策略变更版本化，Pending 审批按新策略重新评估。

### 阶段 2：Issue 工作流与阶段

交付：

- `workflow_template`、`stage`、阶段历史。
- 阶段转换 API/MCP/Web。
- 任务、评审、证据关联。
- 按 Issue 类型选择模板。
- 默认模板和迁移。

验收：

- Feature Issue 可以走分析、设计、实现、评审、验证。
- Bug Issue 可以走复现、修复、评审、测试、发布验证。
- 策略可以在任意阶段要求或免除人类批准。

### 阶段 3：项目流程对象

交付：

- ProjectBrief、Roadmap、Iteration、Release、Environment、Deployment。
- 迭代选入 Issue、关闭迭代。
- 发布范围、候选版本、验证、批准、部署、回滚。
- 项目流程 UI 和 API/MCP/CLI。

验收：

- 一个项目可以有多个迭代和多个发布。
- 发布可以包含多个迭代的 Issue。
- 发布门禁读取证据和策略。
- Issue 关闭可以要求目标环境验证。

### 阶段 4：执行面集成

交付：

- Git/CI/部署签名 webhook。
- Evidence 自动采集。
- Task 租约、心跳、超时、重试。
- 沙箱适配器接口。

验收：

- CI 结果自动成为证据。
- PR 评审自动更新 Review。
- 部署健康状态回写 Release/Deployment。
- 智能体可以领取任务并在沙箱执行。

### 阶段 5：运维与度量

交付：

- Incident/Feedback 工作流。
- 监控指标接入。
- 项目/迭代/发布度量。
- 策略效果分析。
- 复盘模板和关闭流程。

验收：

- 线上事故可以生成 Incident Issue 并触发快速发布流程。
- 发布后指标异常可以创建 Issue 或回滚。
- 复盘结论进入 Decision，后续 Issue 可追溯。

## 12. 验收标准

1. 人类可以任意配置 Issue 权限，包括放宽和收紧；不存在“只能收紧”的代码限制。
2. 人类可以对单个 Issue、动作、类型、风险、actor 配置权限，并设置有效期。
3. 智能体在 `auto` 下无需人类批准即可执行；在 `human_required` 下只能创建审批请求。
4. 智能体不能自行修改策略或覆盖，除非人类显式授权；所有授权可撤销、可审计。
5. 审批绑定对象版本、证据哈希和策略版本；对象变化后旧审批失效。
6. 策略决策记录命中规则、覆盖、actor、版本和结果；历史不可改。
7. 项目流程包含目标、路线图、迭代、开发、发布、运维、复盘；Issue 流程嵌入其中。
8. 一个项目支持多个迭代和多个发布；发布和开发循环独立但可关联。
9. 测试证据贯穿开发、评审、迭代、发布和线上验证。
10. 现有 DemoPolicy 行为可以通过默认策略迁移保留，但不再是硬编码。
11. 项目隔离、身份、审计、状态机合法性、领域必填字段不可被策略绕过。
12. Web、REST、MCP、CLI 对同一对象和同一策略得到一致结果。

## 13. 风险与待决问题

1. **Issue 状态与 stage 双字段的复杂度**：需要确认是否保留现有 `status` 并新增 `stage`，还是把 stage 映射为 Task。建议先保留 `status`，用 `stage` 表达处理进度，避免破坏现有客户端。
2. **策略引擎选型**：首期用 Rust 内置确定性引擎，规则 JSONB；后续可接 OPA/Cedar。需要确认是否需要外部策略语言。
3. **策略编辑 UX**：任意配置意味着复杂规则；需要模板、模拟、差异对比和审计视图，否则 Owner 难以理解后果。
4. **自审和智能体批准**：本方案允许人类配置；需要确认 UI 是否默认展示风险提示，以及是否提供“建议独立审批”的模板。
5. **跨项目/组织策略**：当前只有项目级。未来如果需要组织级底线，应作为独立层，而不是偷偷塞进系统事实。
6. **流程模板的可配置性**：本方案首期模板由代码定义、版本化；如果 Owner 需要自定义阶段，需要单独的流程定义版本机制。
7. **发布与 Issue 关闭条件**：需要确认是否所有 Issue 都要求目标环境验证，还是按模板/策略配置。建议配置，不硬编码。
8. **证据信任**：外部 CI/部署证据需要签名或可信来源；首期可记录 URI 和哈希，后续增加来源验证。
9. **智能体租约与多实例**：需要 `FOR UPDATE SKIP LOCKED` 或外部队列；当前单实例，后续扩展。
10. **人类 Owner 缺失**：项目必须保留 Owner；如果 Owner 长期不在，需要恢复流程，但不能绕过审计。
