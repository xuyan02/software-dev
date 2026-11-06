# 跨工程约定

## 仓库与构建单元

- 整个 `software-dev` 是一个 git 仓库。
- 根 `Cargo.toml` 是唯一的 workspace 根；所有 Rust crate 都是它的成员。
- 只有一个 `Cargo.lock` 和一个 `target/`，都位于仓库根目录。
- Cargo 不支持嵌套 workspace：不要在 `projects/<name>/` 下再放 `[workspace]`。

## 目录

- `projects/<name>/`：一个工程的代码、文档、部署和迁移。
  - `crates/`：库 crate（domain、app、adapters、api-*、web、cli）。
  - `services/`：可执行服务，作为组合根。
  - `xtask/`：工程任务。
- `shared/`：跨工程共享的模板、schema、协议定义。不要放业务代码。
- `infra/compose/`：本地开发共享依赖（Postgres、MinIO、OPA）。
- `scripts/`：容器级脚本。

## Rust 工程

- 共享依赖版本写在根 `[workspace.dependencies]`，成员 crate 用 `.workspace = true`。
- 根 `rust-toolchain.toml` 固定工具链；`rustfmt.toml`、`clippy.toml`、`deny.toml` 也在根目录。
- 采用六边形架构：`domain` 不依赖 I/O、async 和 Web 框架；`app` 定义用例和端口；`adapters` 实现端口；`api-*` 是入站适配器。
- 所有公开 API 的 DTO 与领域类型分离，避免把持久化或传输格式泄漏进 domain。
- 错误用 `thiserror` 定义，边界层再转成用户可见的错误。

## 新建工程

```bash
scripts/new-project.sh <name>
```

脚本创建 `projects/<name>/crates/<name>/`，并把该路径插入根 `Cargo.toml` 的 `members`。之后在 `docs/projects.md` 登记。

## 分支与提交

- 分支：`feat/<topic>`、`fix/<topic>`、`chore/<topic>`。
- 提交信息：`<type>(<scope>): <summary>`，type 用 `feat` / `fix` / `refactor` / `docs` / `test` / `chore`。
- 重要决策写 ADR，放工程自己的 `docs/adr/`。

## 质量门禁

在仓库根目录执行：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

或 `./scripts/check-all.sh`。CI 同样执行这三条，外加 `cargo deny check`。
