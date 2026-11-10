# 状态机

状态机是流程约束的第一层。它回答“这个转换是否存在”；策略引擎回答“这个 actor 是否可以做”。两者都通过，转换才会发生。

## Issue

```
Open ──► InProgress ──► Resolved ──► Closed
  │           │            │
  │           ▼            │
  ├──────► Rejected ◄──────┘
  │
  └──────► Duplicate
             │
             └──────────────► Open ◄──── Closed / Rejected / Duplicate
```

| From | To |
|---|---|
| Open | InProgress, Resolved, Rejected, Duplicate |
| InProgress | Resolved, Rejected |
| Resolved | Closed, InProgress |
| Closed | Open |
| Rejected | Open |
| Duplicate | Open |

额外证据要求：

- `Resolved` / `Rejected` 必须提供 `resolution` 说明。
- `Duplicate` 必须提供 `duplicate_of`。
- 只有 `Resolved` 可以进入 `Closed`。
- Bug 在创建时必须提供 `severity`；没有分诊步骤，分类和严重度在创建时确定。

## Requirement

```
Draft ──► InReview ──► Approved ──► InDesign ──► InDevelopment ──► InVerification ──► Done
  │          │  ▲          │            │  ▲            │                │  ▲
  │          │  └──────────┘            │  └────────────┘                │  └───────────┐
  │          ▼                          ▼                               ▼              │
  └──────► Rejected ◄───────────────────┴─────────────────────────── Deferred ◄────────┘
             │                                                          │
             └──────────────────► Draft ◄───────────────────────────────┘
```

允许的转换：

| From | To |
|---|---|
| Draft | InReview, Rejected, Deferred |
| InReview | Approved, Draft, Rejected, Deferred |
| Approved | InDesign, Deferred, Superseded |
| InDesign | InDevelopment, Approved, Deferred, Superseded |
| InDevelopment | InVerification, InDesign, Deferred |
| InVerification | Done, InDevelopment, Deferred |
| Done | Superseded |
| Rejected | Draft, Superseded |
| Deferred | Draft, InReview, Superseded |
| Superseded | — |

`InReview → Approved` 额外要求 `users` 和 `non_goals` 非空。

## Task

| From | To |
|---|---|
| Todo | InProgress, Cancelled |
| InProgress | Blocked, Review, Cancelled |
| Blocked | InProgress, Cancelled |
| Review | Done, InProgress |
| Done / Cancelled | — |

## DesignDoc

| From | To |
|---|---|
| Draft | InReview, Superseded |
| InReview | Accepted, Draft, Superseded |
| Accepted | Superseded |
| Superseded | — |

接受前必须填写 context、decision、tradeoffs，且 `open_questions` 为空。

## ADR

| From | To |
|---|---|
| Proposed | Accepted, Rejected |
| Accepted | Superseded, Deprecated |
| Rejected / Superseded / Deprecated | — |

已接受的 ADR 不修改；纠正通过新的 ADR 标记 `supersedes`。

## Contract

| From | To |
|---|---|
| Draft | Active, Deprecated |
| Active | Deprecated |
| Deprecated | — |

## Decision

| From | To |
|---|---|
| Open | Decided, Superseded |
| Decided | Reopened, Superseded |
| Reopened | Decided, Superseded |
| Superseded | — |

## Account

| From | To |
|---|---|
| Active | Suspended |
| Suspended | Active |

系统必须保留至少一个 Active 的 `admin`。停用账户会使其所有会话立即失效。

## Session

| From | To |
|---|---|
| Active | Expired（超过 `expires_at`） |
| Active | Revoked（显式登出） |
| Expired / Revoked | — |

只有 Active 且未过期的会话能解析出账户。

## Project

| From | To |
|---|---|
| Active | Archived |
| Archived | Active |

归档项目只读：不能添加/移除成员、改角色、修改项目、创建或推进 Issue / Requirement。成员角色只能在 Active 项目中修改。

成员角色偏序：`Viewer < Member < Maintainer < Owner`。

| 动作 | Viewer | Member | Maintainer | Owner | 全局 Admin |
|---|---|---|---|---|---|
| 读取项目对象 | ✅ | ✅ | ✅ | ✅ | ✅ |
| 创建 Issue / Requirement、评论 | ❌ | ✅ | ✅ | ✅ | ✅ |
| 处置 Issue、编辑/转换 Requirement | ❌ | ❌ | ✅ | ✅ | ✅ |
| 修改项目、管理成员 | ❌ | ❌ | ❌ | ✅ | ✅ |

## 默认权限矩阵（DemoPolicy）

| 动作 | Human | Agent | System |
|---|---|---|---|
| 创建议题 / 评论 | ✅ | ✅ | ❌ |
| Open → InProgress | ✅ | ✅ | ❌ |
| Open / InProgress → Resolved | ✅ | ✅ | ❌ |
| Resolved → Closed | ✅ | ❌ | ❌ |
| 任意 → Rejected / Duplicate | ✅ | ❌ | ❌ |
| Closed / Rejected / Duplicate → Open | ✅ | ❌ | ❌ |
| 创建需求 | ✅ | ✅ | ❌ |
| Draft → InReview | ✅ | ✅ | ❌ |
| Approved → InDesign | ✅ | ✅ | ❌ |
| InDesign → InDevelopment | ✅ | ✅ | ❌ |
| InDevelopment → InVerification | ✅ | ✅ | ❌ |
| InReview → Approved | ✅ | ❌ | ❌ |
| 任意 → Rejected / Deferred / Superseded | ✅ | ❌ | ❌ |
| InVerification → Done | ✅ | ❌ | ❌ |

这张表是 `crates/adapters/policy` 中的演示策略，生产环境应替换为 OPA/Cedar 策略，并把“谁有权批准”绑定到项目角色、风险等级和变更影响面。

请求必须先通过项目角色检查，再进入这张策略表；两层都通过才执行转换。因此一个 `member` 智能体即使策略允许 `Resolved → Closed` 之外的动作，也不能执行处置，因为项目角色不足。全局 admin 只覆盖项目角色检查，不会绕过 DemoPolicy 的业务限制。

## 生命周期对象状态机（设计稿，RFC 0008–0019）

以下状态机是设计目标，不是当前实现。详细字段、门禁和 API 见对应 RFC。

### Artifact

| From | To |
|---|---|
| Draft | Active, Archived |
| Active | Approved, Superseded, Archived |
| Approved | Superseded, Archived |
| Superseded | Archived |
| Archived | — |

版本不可变；变更产生新版本。

### Baseline

| From | To |
|---|---|
| Draft | Approved, Cancelled |
| Approved | Superseded |
| Superseded | — |
| Cancelled | — |

批准后不可变；变更产生新基线。

### ChangeRequest

| From | To |
|---|---|
| Draft | ImpactAnalysis, Cancelled |
| ImpactAnalysis | Review, Cancelled |
| Review | Approved, Rejected |
| Approved | Implementing, Cancelled |
| Implementing | Verifying |
| Verifying | Closed, Implementing |
| Rejected / Cancelled / Closed | — |

紧急变更允许 `Draft → Implementing`，但必须在期限内补齐影响分析和批准。

### Release

| From | To |
|---|---|
| Planned | Candidate, Cancelled |
| Candidate | Verifying, Cancelled |
| Verifying | Approved, Rejected |
| Approved | Deploying, Cancelled |
| Deploying | Deployed, RolledBack |
| Deployed | Observing |
| Observing | Completed, RolledBack |
| RolledBack | Candidate, Cancelled |
| Completed / Cancelled | — |

部署完成不等于发布完成；必须通过观察期。

### Deployment

| From | To |
|---|---|
| Pending | Running, Cancelled |
| Running | Succeeded, Failed |
| Failed | RolledBack |
| Succeeded | RolledBack |
| RolledBack | — |

回滚产生新的 Deployment 记录，不覆盖旧记录。

### Review

| From | To |
|---|---|
| Pending | InReview, Dismissed |
| InReview | ChangesRequested, Approved, Dismissed |
| ChangesRequested | InReview |
| Approved / Dismissed | — |

### TestRun

| From | To |
|---|---|
| Queued | Running, Cancelled |
| Running | Passed, Failed, Error, Cancelled |
| Passed / Failed / Error / Cancelled | — |

重跑产生新 TestRun，不覆盖旧结果。

### AgentVersion

| From | To |
|---|---|
| Draft | Evaluating, Rejected |
| Evaluating | Active, Rejected |
| Active | Deprecated |
| Deprecated | Retired, Active（回退） |
| Rejected / Retired | — |

`Active` 需要评测通过和批准；降级/回退写入事件日志。

### ArchitectureBaseline

| From | To |
|---|---|
| Draft | Active, Cancelled |
| Active | Superseded |
| Superseded / Cancelled | — |

同一项目最多一个 `Active` 架构基线。

### ProjectBrief

| From | To |
|---|---|
| Draft | InReview, Cancelled |
| InReview | Approved, Rejected, Draft |
| Approved | Superseded |
| Rejected / Cancelled / Superseded | — |

批准后不可原地修改；变更走 ChangeRequest。

### Opportunity

| From | To |
|---|---|
| New | Assessed, Rejected |
| Assessed | Selected, Deferred, Rejected |
| Selected | Superseded |
| Deferred | Assessed, Selected, Rejected |
| Rejected / Superseded | — |

### KnowledgeItem

| From | To |
|---|---|
| Draft | InReview, Superseded |
| InReview | Approved, Draft, Rejected |
| Approved | Stale, Superseded |
| Stale | InReview, Superseded |
| Rejected / Superseded | — |

`canonical` 知识必须经过 InReview → Approved。

### RiskRegister / RiskAcceptance

| From | To |
|---|---|
| Open | Mitigated, Accepted, Closed |
| Mitigated | Closed, Open |
| Accepted | Expired, Revoked, Closed |
| Expired | Open（复审） |
| Revoked / Closed | — |

风险接受有期限；到期自动复审。

### Policy

| From | To |
|---|---|
| Draft | Active, Rejected |
| Active | Deprecated |
| Deprecated | — |

策略变更产生新版本；旧版本保留用于审计。
