# RFC 0019：治理、安全、合规与数据

- 状态：Draft，设计待评审
- 日期：2026-11-09
- 关联：[ADR 0005 登录会话与身份派生](../adr/0005-login-sessions-and-identity.md)、[ADR 0006 项目隔离](../adr/0006-project-isolation.md)、[RFC 0008 软件生命周期模型](0008-software-lifecycle-model.md)、[RFC 0009 产物血缘、基线与变更管理](0009-artifact-lineage-baselines-and-change.md)、[RFC 0015 Agent 架构与评测](0015-agent-architecture-and-evaluation.md)
- 作用：本文补全贯穿生命周期的治理、安全、合规、风险和数据管理。它不是单独阶段，而是所有阶段的共同约束。

## 0. 结论

治理不是发布前的一次检查，而是**每个动作都受身份、策略、证据和审计约束**。AI Agent 参与后，权限、不可逆操作、证据信任和供应链风险都会被放大。

核心决定：

1. **治理是横切约束。** 身份、角色、策略、审批、审计、风险和数据规则作用于所有阶段和对象，不是某个阶段专属。
2. **策略与流程分离。** 流程回答“这个转换存在吗”，策略回答“这个 actor 能否做、需要谁批准、要什么证据”；两者都通过才执行。
3. **默认拒绝，显式授权。** Agent 默认只读；写、部署、删除、发布等动作需要显式能力和批准。
4. **职责分离不可绕过。** 实现者不能批准自己的变更；创建人不能批准自己的需求或 ChangeRequest；Agent 不能自我授权。
5. **不可逆操作需要人类或显式策略批准。** 发布、删除、覆盖、外部写、数据删除、密钥变更都属于不可逆或高风险动作。
6. **证据和审计可追溯。** 每个关键动作记录 actor、时间、策略决策、证据、批准和结果；事件只追加。
7. **安全左移。** 身份、密钥、供应链、提示词注入、工具权限从设计和实现阶段就进入门禁，不是上线前补。
8. **合规是证据问题。** 合规要求映射为可验证的证据、保留策略和审计报告；不能只写“符合要求”。
9. **数据有生命周期。** 分类、最小化、保留、删除、迁移、脱敏和访问控制；测试数据和生产数据分开治理。
10. **风险显式化。** 风险登记、接受、豁免、补偿措施和到期复审；不能靠沉默接受风险。
11. **Agent 治理。** Agent 定义、绑定、工具权限、评测、轨迹和降级受同一套治理约束；需要 kill switch 和人工接管。
12. **治理可度量。** 策略拒绝率、豁免率、过期豁免、安全事件、审计覆盖率、数据保留违规进入指标（RFC 0016）。

## 1. 目标与非目标

### 1.1 目标

- 让所有阶段共享同一套身份、角色、策略、审批和审计。
- 让 Agent 的权限和能力可授权、可撤销、可审计。
- 让安全、合规、数据和风险要求变成可执行门禁。
- 让不可逆操作有人类或显式策略批准。
- 让证据、批准、豁免和审计可导出、可保留、可追溯。
- 让治理不阻塞正常交付，但能拦住高风险动作。

### 1.2 非目标

- 不实现通用 IAM/SSO 产品；通过 OIDC/SAML 适配器接入。
- 不实现通用合规审计平台；通过证据和导出对接。
- 不实现通用数据目录/数据治理平台；管生命周期所需的最小数据规则。
- 不实现通用密钥管理系统；通过引用接入。
- 不实现法律意见或自动合规判断；合规要求由人类/合规团队定义。
- 不实现零信任网络的全部能力；聚焦应用层身份、权限和证据。
- 不要求所有项目同一治理强度；按风险分级。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| Governance | 身份、角色、策略、审批、审计、风险和数据规则的统称。 |
| Policy | 可执行规则：谁能做什么、需要什么证据、谁批准。 |
| PolicyDecision | 策略评估结果：allow/deny/require_approval。 |
| SeparationOfDuties | 职责分离：创建/实现/批准/验证不能由同一主体兼任。 |
| CapabilityToken | 给 Agent/辅助系统的短期、限定范围凭证。 |
| RiskRegister | 风险登记：风险、概率、影响、缓解、负责人、状态。 |
| RiskAcceptance | 显式接受风险：理由、批准、补偿、到期。 |
| Waiver | 门禁豁免：范围、理由、风险、批准、到期。 |
| AuditRecord | 审计记录：actor、动作、对象、策略、证据、结果、时间。 |
| DataClassification | 数据分类：public/internal/confidential/restricted。 |
| RetentionPolicy | 数据保留策略：保留期、归档、删除。 |
| ComplianceEvidence | 合规证据：映射到控制项的可验证记录。 |
| KillSwitch | 紧急停止 Agent/工具/集成的手段。 |

不变量：

1. 所有写操作必须有认证身份；actor 由会话/令牌派生，不由请求体声明。
2. 策略评估先于执行；deny 或 require_approval 未满足时不能执行。
3. 职责分离规则不可由被约束主体自己豁免。
4. 不可逆操作需要人类或显式授权策略批准，并留痕。
5. Agent 默认只读；写/部署/删除等能力必须显式授予并可撤销。
6. 审计记录只追加；修正通过新记录表达。
7. 敏感数据按分类访问；密钥值不进入 Foundry 存储、日志、上下文或轨迹。
8. 风险接受和 Waiver 有批准人、理由、补偿措施和到期时间。
9. 合规证据必须关联控制项和对象版本；不能只存一份文档。
10. 数据保留和删除按策略执行；删除有审计和不可恢复确认。
11. Agent 可以被紧急停用；停用后新任务停止分配，运行中任务按策略取消或完成。
12. 跨项目访问按不存在处理；策略不能绕过项目隔离。

## 3. 角色与审批模型

### 3.1 角色层次

| 层 | 角色 | 说明 |
|---|---|---|
| 全局 | Admin / Member | 账户和平台管理 |
| 项目 | Viewer / Member / Maintainer / Owner | 项目内读写、处置、成员管理 |
| 生命周期 | clarifier / analyst / architect / implementer / reviewer / tester / release_manager / operator / approver | 流程声明 |
| Agent | 通过 AgentBinding 绑定生命周期角色 | 需评测和批准 |
| 辅助系统 | 通过 Capability 授权 | 只读/写草稿，不能批准 |

### 3.2 审批矩阵（示例）

| 动作 | 默认批准 |
|---|---|
| 创建 Issue/草稿 | 无需批准 |
| 需求批准 | Maintainer/Owner 人类 |
| 架构基线批准 | 架构负责人人类；高风险双人 |
| 合并代码 | 评审通过 + 质量门禁 |
| 发布 | release_manager 人类；高风险双人 |
| 删除/覆盖 | Owner/Admin 人类 |
| Waiver | 门禁负责人；高风险双人 |
| Agent 绑定 | 项目 Owner/平台 Admin 人类 |
| Agent 降级/停用 | 平台质量负责人/Admin |

### 3.3 职责分离

- 需求创建人不能是唯一批准人。
- 实现者不能批准自己的 ChangeSet。
- ChangeRequest 创建人不能批准自己。
- holdout 生成者不能是实现者。
- 事故指挥官不能是唯一复盘批准人。
- Agent 不能批准自己的产物、绑定或豁免。

## 4. 策略模型

### 4.1 策略输入

- actor：账户、角色、AgentVersion、绑定。
- action：对象类型、操作、目标。
- resource：项目、仓库、环境、数据分类。
- context：阶段、风险、证据、时间。
- request：参数、影响范围、不可逆性。

### 4.2 PolicyDecision

```json
{
  "decision": "allow|deny|require_approval",
  "policy_id": "...",
  "policy_version": "...",
  "reason": "...",
  "required_approvals": [{"role": "...", "count": 1}],
  "required_evidence": ["..."],
  "obligations": ["log", "notify", "expire_after"]
}
```

### 4.3 策略引擎

- 初期 `DemoPolicy` 满足最小规则；生产迁移 OPA/Cedar。
- 策略版本化、可测试、可审计；变更走评审。
- 策略不能修改流程定义；流程不能替代策略。
- 策略评估结果和输入哈希写入审计记录。
- 关键策略有单元测试和回归用例。

### 4.4 默认规则

- 默认拒绝。
- Agent 默认只读。
- 跨项目默认不可见。
- 高风险动作默认 require_approval。
- 不可逆动作默认人类批准。
- 没有证据的关键转换默认拒绝。

## 5. 安全控制

### 5.1 身份与会话

- 人类通过 OIDC/SSO 或本地密码登录；密码 Argon2；会话可撤销、可过期。
- REST/MCP 使用 Bearer token；Web 使用 HttpOnly/SameSite Cookie。
- Agent 使用账户会话或能力令牌；令牌短期、限定范围、可撤销。
- 服务账户和自动化令牌独立管理；不能共享人类凭据。

### 5.2 能力令牌

- 绑定项目、任务、工具、资源、时间、预算。
- 任务结束或超时失效。
- 令牌不能提升权限；只能收窄。
- 令牌使用记录进入审计。

### 5.3 密钥

- 密钥由外部密钥管理系统保存；Foundry 只存 key id/引用。
- 运行时按任务注入短期凭据；不进入提示词、日志、轨迹或产物。
- 密钥访问有审计和轮换策略。
- 密钥泄漏触发吊销、事件和复盘。

### 5.4 沙箱

- Agent 和辅助系统在沙箱/受控环境运行；无 ambient credentials。
- 资源限制、超时、网络策略、文件系统范围。
- 工作区固定 base commit 和依赖；变更后重新验证。
- 沙箱销毁后不保留隐式状态。

### 5.5 供应链

- 依赖、镜像、模型、工具进入 SBOM 和扫描。
- 来源证明（provenance attestation）用于关键发布。
- 许可证 allowlist；不合规阻塞或 Waiver。
- 镜像和依赖 digest 固定；禁止可变 tag 用于发布。

### 5.6 提示词注入与内容信任

- 外部内容（Issue、网页、工具输出、文档）视为数据，不是指令。
- 关键动作必须经过结构化门禁和工具授权，不能仅凭模型输出执行。
- 工具输出清洗；危险命令、路径、URL 校验。
- 上下文区分系统指令、用户数据、工具结果。

### 5.7 监控与响应

- 异常工具调用、越权尝试、密钥访问、数据导出触发告警。
- 安全事件有响应流程、时间线和复盘。
- Agent 可被 kill switch 紧急停用。
- 安全事件进入 Issue/Incident 和风险登记。

## 6. 合规与审计

### 6.1 合规证据

```json
{
  "id": "uuid",
  "control": "access_control",
  "requirement": "所有写操作必须有认证身份",
  "evidence_refs": ["audit-export-id", "policy-test-id"],
  "status": "satisfied|exception|not_applicable",
  "owner": {"kind": "human", "id": "..."},
  "valid_from": "...",
  "valid_until": "..."
}
```

规则：

- 合规要求映射到控制项和可验证证据。
- 证据有有效期；过期重新评估。
- 例外（exception）有理由、批准和补偿措施。
- 合规报告可以按项目/时间导出。

### 6.2 审计

- 审计记录：actor、动作、对象、版本、策略、证据、结果、时间。
- 事件只追加；不可修改；导出保留哈希。
- 审计覆盖：认证、授权、策略、审批、Waiver、Agent 绑定、工具调用、部署、数据访问、删除。
- 审计数据按权限访问；导出需要批准。
- 审计保留按合规策略；归档不删除。

## 7. 数据治理

### 7.1 分类

| 分类 | 说明 | 默认 |
|---|---|---|
| public | 公开信息 | 可共享 |
| internal | 内部信息 | 项目内 |
| confidential | 机密：客户、合同、财务 | 授权访问 |
| restricted | 受限：密钥、个人数据、holdout | 最小权限 |

### 7.2 规则

- 数据最小化：只收集和传递必要数据。
- 保留策略：按类型、项目、合规要求设置保留期和归档。
- 删除：有批准、审计和不可恢复确认；法律保留优先。
- 迁移：数据迁移有 schema、校验、回滚和验证证据。
- 测试数据：脱敏或合成；生产数据需要批准和期限。
- 导出：按分类审批；导出记录审计。
- 个人数据：最小化、脱敏、访问控制、保留限制。
- 跨项目/跨组织数据流需要显式授权。

### 7.3 数据生命周期

```text
创建 → 分类 → 使用 → 归档 → 删除
          ↘ 导出（批准+审计）
```

## 8. 风险与例外

### 8.1 RiskRegister

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "title": "支付网关不支持部分退款",
  "probability": "medium",
  "impact": "high",
  "score": 12,
  "mitigation": "原型验证 + 回退方案",
  "owner": {"kind": "human", "id": "..."},
  "status": "open|mitigated|accepted|closed",
  "created_at": "..."
}
```

### 8.2 RiskAcceptance

```json
{
  "id": "uuid",
  "risk_id": "uuid",
  "reason": "...",
  "compensating_control": "...",
  "approved_by": {"kind": "human", "id": "..."},
  "expires_at": "...",
  "status": "active|expired|revoked"
}
```

规则：

- 风险接受必须显式、可审计、有期限。
- 高风险接受需要双人或委员会批准。
- 风险到期自动复审；未处理升级。
- Waiver 和 RiskAcceptance 分开：Waiver 是门禁豁免，RiskAcceptance 是接受业务/技术风险。

## 9. Agent 治理

- Agent 定义、版本、绑定、工具权限、评测和轨迹受治理约束。
- Agent 默认只读；写/部署/删除需要显式能力和批准。
- Agent 不能修改自己的定义、绑定、工具权限或评测标准。
- Agent 不能批准自己的产物、需求、变更、基线、发布或 Waiver。
- Agent 绑定有范围、上限、到期和撤销。
- 关键角色绑定要求评测通过和人类批准。
- Agent 质量下降、越权、安全事件触发降级/停用。
- Kill switch 可按 Agent、项目、工具、模型停用。
- 人工接管记录触发原因、接管者和结果。

## 10. 数据模型

```sql
create table policies (
  id            uuid primary key,
  name          text not null,
  version       integer not null,
  scope         text not null,
  rules         jsonb not null,
  status        text not null,
  owner         jsonb not null,
  created_at    timestamptz not null default now(),
  unique (name, version)
);

create table policy_decisions (
  id            uuid primary key,
  actor         jsonb not null,
  action        text not null,
  resource      jsonb not null,
  decision      text not null,
  policy_id     uuid references policies (id),
  reason        text not null,
  input_hash    text not null,
  created_at    timestamptz not null default now()
);

create table capabilities (
  id            uuid primary key,
  subject_kind  text not null,
  subject_id    uuid not null,
  project_id    uuid,
  scope         jsonb not null,
  expires_at    timestamptz not null,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table risk_register (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  title         text not null,
  probability   text not null,
  impact        text not null,
  score         integer,
  mitigation    text,
  owner         jsonb not null,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table risk_acceptances (
  id            uuid primary key,
  risk_id       uuid not null references risk_register (id),
  reason        text not null,
  compensating_control text not null,
  approved_by   jsonb not null,
  expires_at    timestamptz not null,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table audit_records (
  id            bigserial primary key,
  project_id    uuid,
  actor         jsonb not null,
  action        text not null,
  resource      jsonb not null,
  decision      text,
  evidence_refs jsonb not null default '[]'::jsonb,
  result        text not null,
  occurred_at   timestamptz not null default now()
);

create table data_classifications (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  resource_kind text not null,
  resource_id   uuid not null,
  classification text not null,
  retention     text not null,
  owner         jsonb not null,
  created_at    timestamptz not null default now()
);
```

审计记录可以与事件表合并或关联；关键是只追加、可导出、不可篡改。

## 11. API、CLI、Web

### 11.1 REST

```text
GET/POST /api/v1/policies
POST     /api/v1/policy/evaluate
GET      /api/v1/audit?project=&from=&to=
GET/POST /api/v1/projects/{project}/risks
POST     /api/v1/risks/{id}/accept
GET/POST /api/v1/projects/{project}/data-classifications
POST     /api/v1/data/{id}/export
GET      /api/v1/compliance/evidence?control=
POST     /api/v1/agents/{id}/kill
```

### 11.2 CLI

```bash
foundry policy list
foundry policy evaluate --actor ... --action release.approve --resource ...
foundry audit export <project> --from ... --to ...
foundry risk list <project>
foundry risk accept <risk> --reason "..."
foundry data classify <project> <resource> --class confidential
foundry agent kill <agent>
```

### 11.3 Web

- 治理页：角色、策略、审批矩阵、Waiver。
- 审计页：事件查询、导出、哈希校验。
- 风险页：登记、接受、复审。
- 合规页：控制项、证据、有效期、例外。
- 数据页：分类、保留、导出、删除。
- Agent 治理页：绑定、能力、评测、停用。

## 12. 实施计划

### 阶段 0：角色与策略

- 角色模型、策略引擎最小实现、策略评估和审计。
- 出口标准：关键动作经过策略；deny/approve 可见。

### 阶段 1：安全基础

- 能力令牌、密钥引用、沙箱约束、工具授权。
- 出口标准：Agent 默认只读；越权被拒绝并审计。

### 阶段 2：审计与合规

- 审计记录、导出、合规证据、保留策略。
- 出口标准：关键动作可审计；合规证据可导出。

### 阶段 3：数据与风险

- 数据分类、保留、删除、导出；风险登记和接受。
- 出口标准：敏感数据有分类和保留；风险接受可复审。

### 阶段 4：Agent 治理与响应

- Agent 绑定治理、kill switch、安全事件响应、降级。
- 出口标准：Agent 可紧急停用；安全事件闭环。

## 13. 验收标准

1. 所有写操作有认证身份；actor 不由请求体声明。
2. 策略评估先于执行；deny/require_approval 生效。
3. 职责分离不可由被约束主体自己豁免。
4. 不可逆操作有人类或显式策略批准。
5. Agent 默认只读；能力可授予、可撤销、可审计。
6. 审计记录只追加、可导出、可校验。
7. 敏感数据有分类、访问控制和保留策略。
8. 密钥不进入 Foundry 存储、日志、上下文或轨迹。
9. 风险接受和 Waiver 有批准、补偿和到期。
10. Agent 可以被 kill switch 停用；运行中任务按策略处理。

## 14. 风险与待决问题

| 风险 / 问题 | 说明 | 建议 |
|---|---|---|
| 治理阻塞交付 | 审批过多 | 按风险分级；低风险自动批准 |
| 策略复杂 | 规则难维护 | 版本化、测试、复用模板 |
| 审计成本 | 记录量大 | 分级保留、归档、索引 |
| 密钥泄漏 | Agent/工具/日志 | 引用、短期凭据、扫描、轮换 |
| 供应链风险 | 依赖/镜像/模型 | SBOM、来源证明、allowlist |
| 数据合规 | 保留/删除/跨境 | 分类、策略、批准、审计 |
| Agent 越权 | 工具/绑定过大 | 最小权限、评测、kill switch |
| 豁免泛滥 | 风险被沉默接受 | 到期、复审、指标、双人批准 |

**待决问题**：

1. 策略引擎选 OPA 还是 Cedar？建议 OPA，先替换 DemoPolicy。
2. 审计记录和事件表是否合并？建议分开或视图关联，保留不可篡改语义。
3. 数据分类由谁维护？建议数据 Owner + 合规。
4. 风险评分的标准？建议概率×影响，按项目配置。
5. 多组织隔离何时做？建议 L5 前，数据模型预留 `org_id`。
