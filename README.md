# software-dev

多个软件工程的单一仓库。Rust 工程共享根目录的 Cargo workspace：一个 `Cargo.lock`、一个 `target/`、一次 `cargo test` 覆盖全部。

## 工程索引

| 工程 | 说明 | 状态 |
|---|---|---|
| [foundry](projects/foundry/) | AI 原生的软件工程控制平面 | MVP 纵向切片可运行；完整生命周期设计见 RFC 0008–0019（设计稿） |

## 构建

```bash
# 编译整个 workspace
cargo build

# 只编译 foundry 控制平面
cargo build -p foundry-control-plane

# 测试 / 检查
cargo test --workspace
./scripts/check-all.sh
```

产物在仓库根目录的 `target/debug/`，例如 `target/debug/foundry-control-plane`。

## 目录

```
Cargo.toml              # 根 workspace：成员、共享依赖、版本
Cargo.lock
projects/<name>/        # 一个工程的代码、文档、部署与迁移
  crates/               # 库 crate
  services/             # 可执行服务（组合根）
  xtask/
shared/                 # 跨工程模板与 schema
infra/compose/          # 本地共享依赖（Postgres、MinIO、OPA）
scripts/                # 构建与新建工程脚本
docs/                   # 跨工程约定
```

## 约定

- 单一 git 仓库，根目录即构建单元。
- Cargo 不允许嵌套 workspace：所有 Rust crate 都列在根 `Cargo.toml` 的 `members` 中。
- 每个工程的代码放在 `projects/<name>/`；文档、迁移、部署配置跟随工程。
- 共享依赖版本在根 `[workspace.dependencies]` 中声明，crate 用 `.workspace = true` 继承。
- 新建工程：`scripts/new-project.sh <name>`，脚本会把新 crate 插入根 workspace。

详见 [docs/conventions.md](docs/conventions.md)。
