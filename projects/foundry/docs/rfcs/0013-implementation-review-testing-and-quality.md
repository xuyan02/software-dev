# RFC 0013：实现、评审、测试与质量门禁

- 状态：Draft，设计待评审
- 日期：2026-11-09
- 关联：[RFC 0008 软件生命周期模型](0008-software-lifecycle-model.md)、[RFC 0009 产物血缘、基线与变更管理](0009-artifact-lineage-baselines-and-change.md)、[RFC 0010 辅助系统注册与产物契约](0010-auxiliary-systems-and-evidence.md)、[RFC 0011 需求管理与需求分析](0011-requirements-management.md)、[RFC 0012 架构与设计管理](0012-architecture-and-design-management.md)
- 作用：本文定义实现、代码评审、测试管理、质量与安全门禁的对象、状态、证据和 Agent 边界。

## 0. 结论

实现与验证阶段的难点不是“能不能写代码”，而是**证据是否独立、测试是否真实验证了需求、评审是否不是自我确认**。

核心决定：

1. **实现以 ChangeSet 为中心。** Task 在沙箱中执行，产生 ChangeSet（分支、提交、diff、PR、构建）；Git 是代码真相，Foundry 只存引用和证据。
2. **评审独立于实现。** Review 有评审人、发现项和决定；实现者不能批准自己的 ChangeSet；Agent 评审必须与实现 Agent 分离。
3. **测试有计划、有用例、有运行记录。** TestPlan、TestCase、TestRun、TestResult、CoverageReport 是可追溯对象；测试结果必须绑定 commit、环境、配置和工具版本。
4. **holdout 由独立验证方持有。** 实现者只能看到公开验收场景；holdout 通过独立验证产生 `independently_verified` 证据。
5. **测试生成不等于测试验证。** 同一 Agent 生成的测试可能共享同一个误解；关键需求必须有独立测试、holdout 或人工验收。
6. **质量与安全门禁可执行。** 构建、单测、集成、验收、覆盖率、静态分析、SAST/DAST/SCA、密钥扫描、许可证和 SBOM 都是门禁输入；失败可定位、可豁免、可审计。
7. **证据分级。** 自测、工具验证、独立验证、人类批准、签证实证是不同等级；关键门禁不能只接受自测。
8. **flaky 测试隔离。** 不稳定测试进入 quarantine，有负责人和到期时间；不能通过重跑掩盖。
9. **Agent 不能自我批准、不能豁免自己、不能关闭自己造成的缺陷。** 关闭 Issue、批准发布、接受高风险风险需要人类或策略授权。
10. **变更可追溯。** ChangeSet 关联 Requirement、Design、Task、TestRun、Release；缺失链接在门禁中提示。
11. **成本与延迟可度量。** 测试执行、Agent 实现、评审等待和返工进入指标（RFC 0016）。
12. **不重造 Git/CI。** Foundry 通过适配器读取 commit、PR、CI run、构建产物和测试报告；不复制内容作为第二真相。

## 1. 目标与非目标

### 1.1 目标

- 让实现、评审、测试和质量门禁形成可验证的闭环。
- 让每个变更都能追溯到需求、设计和任务。
- 让测试结果可信、可复现、可定位。
- 让 holdout 真正独立，降低针对测试过拟合。
- 让质量与安全门禁可执行、可豁免、可审计。
- 让 Agent 和人类在同一套证据和权限下协作。
- 让返工、flaky、缺陷逃逸和人工接管可度量。

### 1.2 非目标

- 不实现 Git 托管、CI 引擎或容器运行时；通过适配器接入。
- 不实现通用测试框架；接入现有测试工具。
- 不实现自动批准或自动关闭缺陷。
- 不要求所有变更都写完整测试计划；按风险分级。
- 不把覆盖率当成唯一质量指标；覆盖率高不等于验证充分。
- 不实现跨项目测试共享；测试按项目隔离。
- 不实现生产流量回放；可以作为后续实验能力。
- 不实现通用代码编辑器；代码编辑仍在 IDE/Agent 工作区。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| ChangeSet | 一次实现产生的变更集合：分支、提交、diff、PR、构建。 |
| Review | 对 ChangeSet 的独立评审：评审人、发现项、决定。 |
| ReviewFinding | 评审发现：严重度、位置、类别、状态。 |
| TestPlan | 测试策略和范围：需求、风险、环境、holdout 策略、入口/出口条件。 |
| TestCase | 可执行或可人工验证的测试用例；可关联验收场景。 |
| TestRun | 一次测试执行：commit、环境、配置、工具版本、结果。 |
| TestResult | 单个用例的结果：通过/失败/跳过/错误、日志、证据。 |
| CoverageReport | 覆盖率报告：行、分支、需求覆盖。 |
| QualityGate | 质量门禁：检查项、阈值、阻塞策略。 |
| ScanResult | 安全/质量扫描结果：SAST/DAST/SCA/密钥/许可证。 |
| SBOM | 软件物料清单：组件、版本、哈希、来源。 |
| Waiver | 门禁豁免：范围、理由、风险、批准人、有效期。 |
| FlakyQuarantine | 不稳定测试隔离：原因、负责人、到期时间。 |
| Independent Verification | 由非实现者执行的验证；可达到 `independently_verified`。 |

不变量：

1. 每个 ChangeSet 必须关联至少一个 Task；高风险变更必须关联 Requirement 或 Design。
2. ChangeSet 的代码真相在 Git；Foundry 存 branch/commit/diff/PR 引用和哈希。
3. 实现者不能批准自己的 ChangeSet；评审人不能是实现者。
4. 每个 TestRun 必须绑定 commit、环境、配置、工具版本和执行者。
5. 测试结果不能只存“通过/失败”；必须能定位到用例、日志和证据。
6. holdout 不进入实现者工作区、日志、上下文或代码注释。
7. 关键门禁不能只接受 `self_reported` 证据。
8. 覆盖率低于阈值时阻塞或警告，取决于风险等级；不能用无意义测试刷覆盖率。
9. flaky 测试不能通过无限重跑视为通过；必须隔离并跟踪。
10. 高危安全发现不能静默忽略；必须有修复、Waiver 或明确的风险接受。
11. Agent 不能豁免自己的失败、不能关闭自己造成的缺陷、不能批准发布。
12. 所有评审、测试、扫描、豁免和门禁评估写入事件日志。

## 3. 实现

### 3.1 ChangeSet

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "task_id": "uuid",
  "title": "修复部分退款金额校验",
  "requirements": ["requirement-id"],
  "design_doc": "design-id|null",
  "architecture_baseline": "baseline-id",
  "repo": "payments",
  "branch": "fix/partial-refund",
  "base_commit": "abc123",
  "head_commit": "def456",
  "diff_ref": {"type": "git", "commit": "def456", "path": "..."},
  "pr_ref": {"provider": "github", "number": 42, "url": "..."},
  "builds": ["build-id"],
  "status": "draft|in_review|approved|merged|abandoned",
  "created_by": {"kind": "agent", "id": "..."},
  "created_at": "..."
}
```

规则：

- ChangeSet 一次实现一个可评审的变更；不要把无关改动混在一起。
- base/head commit 必须可解析；force-push 后旧引用标记失效。
- PR 是外部事实；Foundry 存引用和状态，不复制讨论全文（可存摘要和链接）。
- 合并后 ChangeSet 不可修改；后续变更产生新 ChangeSet。
- ChangeSet 关联 Requirement、Design、Task、TestRun、Release。

### 3.2 实现流程

```text
Task 被领取
  → 准备沙箱工作区（固定 base commit、配置、依赖）
  → Agent/人类实现
  → 本地构建和自测
  → 提交分支和 PR
  → 创建 ChangeSet 和证据
  → 进入评审与测试
```

规则：

- 工作区固定 base commit、依赖锁、工具版本；变更后必须重新验证。
- 实现过程中产生的命令、日志、退出码可以作为证据，但不能替代独立验证。
- 大变更拆分为多个 ChangeSet；每个 ChangeSet 可独立评审和回滚。
- 不允许在实现中静默修改需求或架构；需要变更走 ChangeRequest。

### 3.3 实现门禁

| 门禁 | 条件 | 阻塞 |
|---|---|---|
| changeset.linked | 关联 Task；高风险关联 Requirement/Design | 是 |
| build.pass | 构建成功，产物可复现 | 是 |
| unit.pass | 单测通过 | 是 |
| lint.pass | 格式、lint、静态分析通过 | 是（按严重度） |
| self_test.pass | 实现者自测通过 | 否（参考） |
| secrets.clean | 无密钥泄漏 | 是 |
| license.ok | 许可证合规 | 是（发布） |

## 4. 评审

### 4.1 Review

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "changeset_id": "uuid",
  "reviewers": [{"kind": "human", "id": "..."}, {"kind": "agent", "id": "..."}],
  "status": "pending|in_review|changes_requested|approved|dismissed",
  "findings": ["finding-id"],
  "decision_reason": "...",
  "created_at": "...",
  "decided_at": "..."
}
```

### 4.2 ReviewFinding

```json
{
  "id": "uuid",
  "review_id": "uuid",
  "severity": "blocker|major|minor|nit",
  "category": "correctness|security|performance|maintainability|style|test",
  "file": "src/refund.rs",
  "line": 42,
  "message": "...",
  "status": "open|fixed|wont_fix|accepted",
  "resolved_by": {"kind": "agent", "id": "..."}
}
```

规则：

- `blocker`/`major` 未关闭不能批准。
- 发现项必须可定位；不能只写“这里有问题”。
- `wont_fix`/`accepted` 需要理由和批准（高风险需要人类）。
- 自动评审发现项标记来源（工具/模型版本）；不能冒充人类评审。

### 4.3 评审规则

1. 实现者不能评审自己的 ChangeSet。
2. 高风险变更需要至少一名人类评审者；Agent 评审可以补充，不能替代。
3. 评审必须覆盖：正确性、需求符合性、架构约束、错误处理、测试充分性、安全、可维护性。
4. 评审可以要求补充测试或证据；未补充不能批准。
5. 评审批准是门禁输入，不是合并的充分条件；测试和质量门禁仍需通过。
6. 评审决定和发现项写入事件日志，保留历史。

### 4.4 自动评审

- 自动评审输出是发现项候选，不是批准。
- 必须记录工具/模型版本、输入 diff 哈希和置信度。
- 高置信度 blocker 可以阻塞；低置信度只提示。
- 自动评审不能作为唯一评审者；高风险变更需要人类。

## 5. 测试管理

### 5.1 TestPlan

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "title": "部分退款迭代测试计划",
  "scope": ["requirement-id"],
  "risks": ["并发退款", "审计日志"],
  "strategy": {
    "unit": true,
    "integration": true,
    "e2e": true,
    "contract": true,
    "performance": true,
    "security": true
  },
  "environments": ["staging"],
  "holdout_policy": "independent_verifier",
  "entry_criteria": ["build.pass", "changeset.in_review"],
  "exit_criteria": ["public_scenarios.pass", "holdout.pass", "coverage >= 80%"],
  "status": "draft|approved|active|closed",
  "created_at": "..."
}
```

规则：

- 测试计划按风险和需求制定；高风险需求需要更强的验证。
- 计划声明环境、数据、holdout 策略和出口条件。
- 计划变更走 ChangeRequest（低风险可以版本化更新）。

### 5.2 TestCase

```json
{
  "id": "uuid",
  "plan_id": "uuid",
  "requirement_id": "uuid",
  "scenario_id": "uuid",
  "title": "部分退款金额校验",
  "type": "unit|integration|e2e|contract|performance|security",
  "automation_ref": {"framework": "cargo test", "path": "tests/refund.rs", "name": "partial_refund"},
  "steps": ["..."],
  "expected": "...",
  "holdout": false,
  "owner": {"kind": "agent", "id": "..."},
  "status": "active|quarantined|retired",
  "created_at": "..."
}
```

规则：

- 用例必须关联验收场景或风险；无来源的用例标记为探索性。
- 自动化用例必须有稳定标识，能与 TestResult 对应。
- holdout 用例单独存储，访问受控。
- 用例变更保留历史；删除只归档。

### 5.3 TestRun / TestResult

```json
{
  "id": "uuid",
  "plan_id": "uuid",
  "commit": "def456",
  "environment": "staging",
  "config_version": "config-id",
  "tool": {"name": "cargo test", "version": "1.93.1"},
  "started_at": "...",
  "finished_at": "...",
  "status": "passed|failed|error|cancelled",
  "results": ["result-id"],
  "coverage": "coverage-id",
  "logs_ref": {"type": "s3", "key": "...", "hash": "sha256:..."},
  "executed_by": {"kind": "agent", "id": "..."}
}
```

规则：

- TestRun 必须绑定 commit、环境、配置和工具版本；否则证据降级。
- 结果必须能定位到用例、日志和截图/trace。
- 重跑不覆盖旧结果；新结果产生新 TestRun。
- 测试环境必须可复现；数据库、依赖、种子数据版本记录。
- 并行执行和分片结果合并为一个 TestRun 视图，保留分片引用。

### 5.4 覆盖率

- 行覆盖、分支覆盖、需求覆盖分开统计。
- 覆盖率阈值按项目/风险配置；低于阈值阻塞或警告。
- 覆盖率不能替代验收；高覆盖但无断言、无 holdout 仍然不充分。
- 覆盖率报告是证据，绑定 commit 和工具版本。

### 5.5 Flaky 测试

- 连续失败/通过交替或超时进入 quarantine。
- quarantine 有负责人、原因、到期时间和替代验证方式。
- quarantine 用例不能计入通过率；到期未处理升级。
- 不能通过无限重跑或跳过断言掩盖不稳定。

### 5.6 测试数据

- 测试数据必须可复现、脱敏、版本化。
- 生产数据用于测试需要批准和脱敏。
- 大数据集存对象存储，记录哈希和生成脚本。
- 数据迁移测试使用真实 schema 和迁移脚本。

### 5.7 独立验证与 holdout

- 独立验证者不能是实现者，也不能是同一 Agent 的另一个会话。
- holdout 用例由独立验证方持有；实现者不可见。
- holdout 结果产生 `independently_verified` 证据。
- 泄漏检测：holdout 内容出现在实现者上下文、日志、注释或测试中，记录事件并重新生成。
- 低风险需求可以豁免 holdout，但需要 Waiver。

## 6. 质量与安全门禁

### 6.1 QualityGate

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "stage": "merge|release",
  "checks": [
    {"type": "build", "threshold": "pass"},
    {"type": "unit", "threshold": "pass"},
    {"type": "coverage", "threshold": {"lines": 0.8, "branches": 0.7}},
    {"type": "sast", "threshold": {"blocker": 0, "major": 0}},
    {"type": "sca", "threshold": {"critical": 0, "high": 0}},
    {"type": "secrets", "threshold": "pass"},
    {"type": "license", "threshold": "allowlist"},
    {"type": "review", "threshold": "approved"}
  ],
  "blocking": true,
  "created_at": "..."
}
```

### 6.2 ScanResult

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "type": "sast|dast|sca|secrets|license|container",
  "tool": {"name": "...", "version": "..."},
  "target": {"commit": "...", "artifact": "..."},
  "findings": [{"id": "...", "severity": "critical|high|medium|low", "location": "...", "status": "open|fixed|accepted"}],
  "sbom_ref": "sbom-id|null",
  "status": "passed|failed|error",
  "created_at": "..."
}
```

### 6.3 SBOM 与来源证明

- 构建产生 SBOM：组件、版本、哈希、许可证、来源。
- 关键发布要求 SBOM 和来源证明（provenance attestation）。
- SBOM 存对象存储，哈希进入证据。
- 供应链来源不可信时阻塞发布或需要 Waiver。

### 6.4 Waiver

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "scope": {"gate": "sast", "finding": "..."},
  "reason": "...",
  "risk": "...",
  "compensating_control": "...",
  "approved_by": {"kind": "human", "id": "..."},
  "expires_at": "...",
  "status": "active|expired|revoked"
}
```

规则：

- Waiver 必须有理由、风险、补偿措施、批准人和有效期。
- 高风险 Waiver 需要双人批准。
- 过期自动失效；未处理则门禁重新阻塞。
- Waiver 不能用于绕过不可逆发布的 `attested` 证据要求，除非策略显式允许。

## 7. 证据与独立性

| 证据 | 最低信任 | 说明 |
|---|---|---|
| 实现者自测 | self_reported | 只能作为参考 |
| CI 构建/测试 | tool_verified | 独立执行，可复现 |
| 覆盖率 | tool_verified | 绑定 commit 和工具 |
| 静态分析/扫描 | tool_verified | 工具版本和规则集记录 |
| 自动评审 | self_reported | 需要人类确认 |
| 人类评审 | human_approved | 评审记录和理由 |
| holdout 验证 | independently_verified | 独立验证方 |
| 签名证明 | attested | 内容寻址 + Provenance |

规则：

- 关键门禁不能只接受自测或自动评审。
- 同一 Agent 不能同时实现和验证。
- 证据必须绑定 commit、环境、配置和工具版本。
- 证据失效（新 commit、依赖更新、环境变化）后必须重新生成。
- 证据内容存对象存储，哈希和元数据存 Postgres。

## 8. 辅助系统

| 能力 | 输入 | 输出 | 信任 |
|---|---|---|---|
| `code.implement` | Task、设计、仓库 | ChangeSet 草稿 | self_reported |
| `code.understand` | 仓库、查询 | 代码地图、影响面 | tool_verified |
| `review.code` | diff、设计、规范 | 发现项候选 | self_reported |
| `test.generate` | 需求、场景、设计、代码 | 测试用例候选 | self_reported |
| `test.run` | commit、计划、环境 | TestRun、覆盖率 | tool_verified |
| `verify.independent` | holdout、产物 | 通过/失败、理由 | independently_verified |
| `security.scan` | commit、依赖、镜像 | ScanResult、SBOM | tool_verified |
| `flaky.detect` | 测试历史 | flaky 候选 | tool_verified |
| `impact.code` | 变更、血缘 | 受影响需求/测试/发布 | tool_verified |

规则：

- 辅助输出是草稿或证据，不是批准。
- 自动评审和测试生成必须记录工具/模型版本和输入哈希。
- 独立验证能力不能由实现 Agent 调用。
- 能力可替换、可评测；低质量能力降级。

## 9. 数据模型

```sql
create table changesets (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  task_id       uuid not null,
  artifact_id   uuid not null,
  title         text not null,
  repo          text not null,
  branch        text not null,
  base_commit   text not null,
  head_commit   text not null,
  diff_ref      jsonb not null,
  pr_ref        jsonb,
  status        text not null,
  created_by    jsonb not null,
  created_at    timestamptz not null default now()
);

create table reviews (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  changeset_id  uuid not null references changesets (id),
  status        text not null,
  reviewers     jsonb not null default '[]'::jsonb,
  decision_reason text,
  created_at    timestamptz not null default now(),
  decided_at    timestamptz
);

create table review_findings (
  id            uuid primary key,
  review_id     uuid not null references reviews (id),
  severity      text not null,
  category      text not null,
  file          text,
  line          integer,
  message       text not null,
  status        text not null,
  resolved_by   jsonb,
  created_at    timestamptz not null default now()
);

create table test_plans (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  title         text not null,
  scope         jsonb not null default '[]'::jsonb,
  risks         jsonb not null default '[]'::jsonb,
  strategy      jsonb not null,
  environments  jsonb not null default '[]'::jsonb,
  holdout_policy text not null,
  entry_criteria jsonb not null default '[]'::jsonb,
  exit_criteria jsonb not null default '[]'::jsonb,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table test_cases (
  id            uuid primary key,
  plan_id       uuid not null references test_plans (id),
  requirement_id uuid,
  scenario_id   uuid,
  artifact_id   uuid not null,
  title         text not null,
  type          text not null,
  automation_ref jsonb,
  expected      text,
  holdout       boolean not null default false,
  owner         jsonb not null,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table test_runs (
  id            uuid primary key,
  plan_id       uuid not null references test_plans (id),
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  commit_sha    text not null,
  environment   text not null,
  config_version text,
  tool          jsonb not null,
  status        text not null,
  started_at    timestamptz not null,
  finished_at   timestamptz,
  coverage_ref  jsonb,
  logs_ref      jsonb,
  executed_by   jsonb not null
);

create table test_results (
  id            uuid primary key,
  run_id        uuid not null references test_runs (id),
  test_case_id  uuid not null references test_cases (id),
  status        text not null,
  duration_ms   integer,
  error         text,
  evidence_ref  jsonb
);

create table coverage_reports (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  run_id        uuid not null references test_runs (id),
  lines         numeric(5,4),
  branches      numeric(5,4),
  requirement_coverage numeric(5,4),
  content_ref   jsonb not null,
  created_at    timestamptz not null default now()
);

create table quality_gates (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  stage         text not null,
  checks        jsonb not null,
  blocking      boolean not null default true,
  created_at    timestamptz not null default now()
);

create table scan_results (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  artifact_id   uuid not null,
  type          text not null,
  tool          jsonb not null,
  target        jsonb not null,
  findings      jsonb not null default '[]'::jsonb,
  sbom_ref      uuid,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table waivers (
  id            uuid primary key,
  project_id    uuid not null references projects (id),
  scope         jsonb not null,
  reason        text not null,
  risk          text not null,
  compensating_control text not null,
  approved_by   jsonb not null,
  expires_at    timestamptz not null,
  status        text not null,
  created_at    timestamptz not null default now()
);
```

## 10. API、MCP、CLI、Web

### 10.1 REST

```text
GET/POST /api/v1/projects/{project}/changesets
GET      /api/v1/changesets/{id}
POST     /api/v1/changesets/{id}/reviews
POST     /api/v1/reviews/{id}/findings
POST     /api/v1/reviews/{id}/decide
GET/POST /api/v1/projects/{project}/test-plans
POST     /api/v1/test-plans/{id}/runs
GET      /api/v1/test-runs/{id}
GET      /api/v1/projects/{project}/coverage
GET/POST /api/v1/projects/{project}/quality-gates
POST     /api/v1/projects/{project}/quality-gates/evaluate
GET/POST /api/v1/projects/{project}/waivers
POST     /api/v1/waivers/{id}/revoke
```

### 10.2 MCP

- `changeset.create` / `changeset.get` / `changeset.list`
- `review.submit` / `review.findings` / `review.decide`
- `test.plan.get` / `test.run.start` / `test.run.result`
- `coverage.get`
- `quality.evaluate`
- `waiver.request`
- `verify.independent`（仅独立验证方）
- `scan.run`（受权限限制）

Agent 可以创建 ChangeSet、提交评审、发起测试；不能批准自己的 ChangeSet、不能读取 holdout（除非授权）、不能批准 Waiver。

### 10.3 CLI

```bash
foundry changeset list <project>
foundry changeset show <id>
foundry review submit <changeset>
foundry review decide <review> --approve|--changes
foundry test plan new <project>
foundry test run <plan> --commit <sha> --env staging
foundry coverage <project>
foundry quality evaluate <project> --stage merge
foundry waiver request --gate sast --reason "..."
```

### 10.4 Web

- ChangeSet 页：diff、PR、构建、测试、评审、证据。
- 评审页：发现项、严重度、定位、决定。
- 测试页：计划、用例、运行、覆盖率、flaky。
- 质量页：门禁、扫描、SBOM、Waiver。
- 追溯页：需求 → 变更 → 测试 → 发布。

## 11. 权限与安全

- 人类可以创建 ChangeSet、评审、批准、请求 Waiver；Agent 可以创建、提交、测试，不能自我批准。
- 高风险变更需要人类评审；自动评审不能替代。
- 独立验证者不能是实现者；holdout 访问受控。
- 扫描和测试使用沙箱/受控环境；密钥不进入代码、日志或模型上下文。
- 测试数据脱敏；生产数据需要批准。
- 所有评审、测试、扫描、Waiver 和门禁写入审计日志。
- 供应链来源不可信时阻塞发布或需要 Waiver。

## 12. 与现有实现的关系

- 现有 Task workflow 和沙箱规划（RFC 0003/0004/0008）提供执行基础；本 RFC 增加 ChangeSet、Review、TestPlan/TestRun 和 QualityGate。
- 现有 `crates/adapters/git` 是空壳；本 RFC 需要 clone、diff、commit、PR 引用和 webhook。
- 现有 `crates/adapters/storage` 是空壳；本 RFC 需要大证据存储。
- 现有 `api-http` 增加变更、评审、测试、质量端点。
- 现有 Issue 的 `kind=bug` 继续作为缺陷；测试失败可以创建/更新 Issue，但测试结果本身是 Record。
- 现有事件日志继续记录变更、评审、测试和门禁事件。

## 13. 实施计划

### 阶段 0：ChangeSet 与评审

- 表结构、Git 适配器、ChangeSet 创建、Review/finding。
- 出口标准：一个 PR 可以被创建、评审、批准，证据可追溯。

### 阶段 1：测试运行与覆盖率

- TestPlan/TestCase/TestRun/TestResult、覆盖率、flaky 隔离。
- 出口标准：测试结果绑定 commit/环境/配置，可定位、可复现。

### 阶段 2：质量与安全门禁

- QualityGate、ScanResult、SBOM、Waiver、门禁评估。
- 出口标准：门禁可阻塞、可豁免、可审计。

### 阶段 3：独立验证与 holdout

- holdout 存储/访问控制、独立验证、泄漏检测。
- 出口标准：关键需求有独立验证证据；实现者无法读取 holdout。

### 阶段 4：辅助能力与优化

- 自动评审、测试生成、影响分析、flaky 检测接入。
- 出口标准：辅助输出可评测、可替换；不自动批准。

## 14. 验收标准

1. ChangeSet 关联 Task/Requirement/Design；代码引用可解析。
2. 实现者不能批准自己的 ChangeSet；高风险变更有独立人类评审。
3. TestRun 绑定 commit、环境、配置和工具版本；结果可定位。
4. 关键门禁不接受仅 `self_reported` 证据。
5. holdout 不出现在实现者上下文/日志/代码中；独立验证产生高信任证据。
6. 覆盖率低于阈值时阻塞或警告；不能用无断言测试刷覆盖率。
7. flaky 测试进入 quarantine，有负责人和到期时间。
8. 高危安全发现不能静默忽略；Waiver 有理由、风险、批准和有效期。
9. 门禁失败返回可定位原因；豁免可审计。
10. 变更、测试、扫描和门禁事件可重放。

## 15. 风险与待决问题

| 风险 / 问题 | 说明 | 建议 |
|---|---|---|
| 自测当验证 | Agent 写测试又跑测试 | 独立验证、holdout、人类评审 |
| 覆盖率刷分 | 无断言测试提高覆盖率 | 变异测试、需求覆盖、人工抽查 |
| flaky 掩盖 | 重跑通过即放行 | quarantine、重跑次数上限、根因跟踪 |
| 自动评审过度信任 | 模型漏报/误报 | 高置信度才阻塞；高风险人类评审 |
| 证据不可复现 | 环境/配置/数据缺失 | Provenance、固定环境、降级 |
| 安全扫描噪音 | 误报导致 Waiver 泛滥 | 规则调优、风险分级、补偿措施 |
| 变更过大 | 评审和测试不充分 | 变更大小限制、拆分、按风险分级 |
| 成本 | 测试和 Agent 执行成本高 | 预算、增量测试、缓存、分层执行 |

**待决问题**：

1. 高风险变更的人类评审是否强制？建议强制。
2. holdout 由 tester 角色还是独立验证服务持有？建议角色 + 能力双重约束。
3. 覆盖率阈值按项目还是按模块？建议按模块/风险，项目汇总。
4. 自动评审能否阻塞？建议只有高置信度 blocker 可以阻塞。
5. 测试环境如何固定？建议环境配置版本 + 容器镜像 + 数据种子哈希。
