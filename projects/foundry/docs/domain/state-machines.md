# 状态机

状态机是流程约束的第一层。它回答“这个转换是否存在”；策略引擎回答“这个 actor 是否可以做”。两者都通过，转换才会发生。

## Issue

```
Open ──► Triaged ──► InProgress ──► Resolved ──► Closed
  │          │  ▲          │            │
  │          │  └──────────┘            │
  │          ▼                          ▼
  ├──────► Rejected ◄───────────────────┘
  │          │
  └──────► Duplicate
             │
             └──────────────► Open ◄──── Closed / Rejected / Duplicate
```

| From | To |
|---|---|
| Open | Rejected, Duplicate（→ Triaged 通过分诊动作） |
| Triaged | InProgress, Resolved, Rejected, Duplicate |
| InProgress | Resolved, Triaged, Rejected |
| Resolved | Closed, InProgress |
| Closed | Open |
| Rejected | Open |
| Duplicate | Open |

额外证据要求：

- `Resolved` / `Rejected` 必须提供 `resolution` 说明。
- `Duplicate` 必须提供 `duplicate_of`。
- 只有 `Resolved` 可以进入 `Closed`。
- 分诊（`triage`）在 `Open` / `Triaged` 时可用；Bug 必须选择严重度。分诊把 `Open` 推进到 `Triaged`，这是进入该状态的唯一方式。

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
| 创建 Issue / Requirement、评论、分诊 | ❌ | ✅ | ✅ | ✅ | ✅ |
| 处置 Issue、编辑/转换 Requirement | ❌ | ❌ | ✅ | ✅ | ✅ |
| 修改项目、管理成员 | ❌ | ❌ | ❌ | ✅ | ✅ |

## 默认权限矩阵（DemoPolicy）

| 动作 | Human | Agent | System |
|---|---|---|---|
| 创建议题 / 评论 / 分诊 | ✅ | ✅ | ❌ |
| Triaged → InProgress | ✅ | ✅ | ❌ |
| Triaged / InProgress → Resolved | ✅ | ✅ | ❌ |
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
