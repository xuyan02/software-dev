# ADR 0001：Rust workspace 与六边形架构

- 状态：Accepted
- 日期：2026-11-02

## 背景

Foundry 需要同时提供 HTTP API、MCP 工具、CLI、服务端渲染控制台，并且要在内存、Postgres、Git、策略引擎之间替换实现。领域规则（状态机、审批条件）必须能被单元测试直接覆盖，不能依赖数据库或网络。

## 决定

1. 单个 Cargo workspace，按职责拆 crate：
   - `domain`：纯领域模型。禁止 I/O、async、Web 框架。
   - `app`：用例、端口 trait、错误类型。只依赖 `domain`。
   - `adapters/*`：实现 `app` 定义的端口（内存、Postgres、策略、Git、事件、对象存储）。
   - `api-http` / `api-mcp` / `web` / `cli`：入站适配器。
   - `services/*`：组合根，唯一知道具体适配器的地方。
2. 依赖方向固定为 `domain ← app ← adapters / 入站适配器`，用 `cargo deny` 和代码评审保证不反向。
3. 领域状态机写在 `domain`，权限判断通过 `app::PolicyEngine` 端口注入。
4. 写操作统一顺序：读取 → 鉴权 → 修改聚合 → 乐观并发保存 → 追加事件。

## 备选方案

- **单体 crate**：编译快，但领域规则会逐渐混入 axum/sqlx 类型，难以测试。否决。
- **微服务**：当前只有一个团队、一个写路径，拆分只会增加分布式事务。等出现独立扩展需求再拆。
- **把策略写进状态机**：会让领域模型依赖角色和项目配置。策略是外部输入，保持独立。

## 后果

- 编译期能阻止大部分架构腐蚀；新增适配器不影响用例。
- 泛型 + trait object 增加少量样板代码，由 `RequirementUseCases` 收敛。
- `domain` 不依赖 `time` 之外的运行时库，测试毫秒级完成。
