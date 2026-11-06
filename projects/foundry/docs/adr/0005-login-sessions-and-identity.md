# ADR 0005：登录会话与身份派生

- 状态：已接受
- 日期：2026-11-02

## 背景

早期纵向切片把 `actor` 放在请求体里。任何人只要写 `{"kind":"human","id":"pm"}` 就拥有该身份，因此不能暴露到共享网络，也无法让权限检查可信。

Foundry 需要让人类和智能体操作同一套对象，又必须区分“谁在操作”。身份不能由调用方声明。

## 决策

1. **账户是唯一身份来源。** 人类和智能体都是 `Account`，`kind` 为 `human` 或 `agent`。全局角色只有 `admin` / `member`。
2. **密码使用 Argon2。** 登录时校验密码哈希，成功后生成随机会话 token。
3. **会话 token 只显示一次。** 数据库保存 SHA-256 哈希、过期时间和撤销时间；原始 token 不落库。
4. **Web 与机器客户端使用不同承载方式。** Web 使用 `HttpOnly`、`SameSite=Lax` 的 `foundry_session` Cookie；REST/MCP 使用 `Authorization: Bearer <token>`。
5. **actor 由会话派生。** HTTP 层从 token 解析账户，命令只接收派生出的 actor；所有 API DTO 和 MCP 工具参数移除 `actor`。
6. **首个管理员由人在初始化流程中创建，配置里的初始身份只用于授权初始化。** 账户表为空时，服务只提供 `/setup`（以及 `/health`），其他页面重定向到 `/setup`，API/MCP 返回 `503 setup_required`；未配置 `bootstrap_password` / `FOUNDRY_BOOTSTRAP_PASSWORD` 时拒绝启动。`/setup` 分两步：第一步用配置中的 `bootstrap_handle` 和 `bootstrap_password` 登录；这组初始身份不是账户，验证通过后只签发一个短期、仅存于内存的 setup ticket（HttpOnly Cookie），不会创建账户。第二步要求持有 ticket 的浏览器设置真实的首个管理员账号和密码，服务端通过 `create_first_admin` 创建，成功后自动登录并永久关闭 `/setup`。初始账号/密码不写入数据库，不用于后续登录，也不写日志；账户表非空后完全忽略。（2026-11-03 修订：早期实现曾用该口令自动创建管理员；现在它只授权初始化，管理员账户必须由人现场创建，且初始化身份与管理员账户是两个不同的身份。）
7. **停用即失效。** 账户停用后，已有 token 不能继续认证；登出会撤销当前会话。
8. **保留最后一个管理员。** 不能降级或停用最后一个活跃 admin。

## 后果

- 策略和状态历史里的 actor 现在可信，权限检查可以成为真正的门禁。
- 所有客户端都要先登录；curl 示例和 CLI 必须携带 token。
- Web 控制台需要登录页、会话 Cookie、登出和受保护路由。
- 仍然只适合本机/受信网络：没有 TLS、CSRF token、速率限制、密码重置邮件或密钥轮换。公网部署前必须补齐。
- 智能体账户目前也使用密码登录；后续可以增加仅限服务端的短期凭证或 mTLS，而不改变“actor 来自会话”的模型。

## 相关

- [ADR 0006：项目隔离](0006-project-isolation.md)
- [运行手册](../operations/runbook.md)
- [HTTP / MCP API](../api/README.md)
