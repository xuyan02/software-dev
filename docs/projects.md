# 工程清单

| 工程 | 路径 | 语言 | 负责人 | 状态 |
|---|---|---|---|---|
| foundry | `projects/foundry` | Rust | TBD | MVP 纵向切片可运行；完整生命周期设计见 RFC 0008–0019（设计稿，未实现） |
| design-space（`ds`） | `projects/design-space` | Rust | TBD | 节点、loose store、pack、tree-sitter 解析、分支/工作状态、HTTP/SSH 远端同步已实现，见 `docs/design.md` |

新增工程后在 `scripts/new-project.sh` 创建，并在这里登记。
