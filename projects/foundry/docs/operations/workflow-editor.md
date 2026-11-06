# 流程编辑器

流程编辑器是独立的 React SPA，挂在 `/workflows/editor/`。控制台其余页面仍是服务端渲染、无 JavaScript；打开「流程」页后点击流程标题进入编辑器。

## 构建与部署

编辑器不是 cargo 构建的一部分。部署必须包含 `web/workflow-editor/dist/`；缺失时 `/workflows/editor/` 返回 `503`，控制台其他页面不受影响。

```bash
cd web/workflow-editor
npm ci
npm run build
```

控制面默认从 `web/workflow-editor/dist` 读取产物；可以用 `FOUNDRY_EDITOR_DIR` 或配置文件中的 `editor_dir` 指向其他目录。

本地开发：

```bash
cd web/workflow-editor
npm install
npm run dev
```

Vite 开发服务器把 `/api` 和 `/login` 代理到 `http://127.0.0.1:8080`。生产构建的 `base` 固定为 `/workflows/editor/`。

## 权限

- 人类账户可以创建、编辑、发布和复制流程；Agent 账户只读。
- 平台流程对所有账户可见。普通账户只读；平台管理员可以编辑和发布。
- 账户流程只对所有者账户可见，平台管理员可以读取但不能编辑或发布。
- 通过「复制到我的账户」得到的新流程有独立 id 和草稿，与源流程不共享任何状态。

## 编辑模型

AST 是唯一真相。画布上的 stage 图形、operation 标签、边和 Start/End 都是 AST 的投影；画布操作直接修改 AST，不存在另一份图数据。

编辑器有三层状态，互不混写：

1. **AST**：草稿或已发布版本，参与 revision、校验、撤销和发布。
2. **布局**：每个画布节点的 `x/y`（stage 和 Start/End 标记），按流程单独保存、last-write-wins，不产生版本，也不进入版本对比。标记坐标存在同一份布局的 `__start` / `__end` 键下。
3. **视口**：缩放和平移，仅存在于当前页面，刷新后重置。

因此拖动 stage 图形或 Start/End 标记只改布局，不会让草稿变“未保存”或推进 revision；撤销一次拖动只恢复位置。

- **Stage**：图形只显示 stage name 和「初始」标记，不显示任何内部内容。点击图形在右侧检查器编辑 name、是否初始、`active_roles`、stage 变量、事件、进入效果和转移。
- **任务变量**：侧栏「任务变量」面板声明跨 stage 保留的变量；每个变量设置 name、类型、`display_name` 和可选的初始值。
- **变量**：`VariablesEditor` 支持新增、改 name、改类型、改 `display_name`、改初始值和删除；stage 变量不能与任务变量同名，`actor_set` 不能声明初始值。
- **事件、进入效果和转移**：只在检查器的 stage 分区按声明顺序列出，点击列表项编辑对应字段；画布上的 stage 图形不显示它们。operation 也不进入图形，而是用 stage 顶边上方的标签表示。
- **operation**：每个 operation 在所属 stage 顶边上方有一个带图标的标签，显示 `display_name`（为空时回退 name），并用短线接到 stage 顶边。点击标签选中 operation。允许发送它的角色不在画布上显示，只在检查器里编辑。
- **新增**：画布上方的对象工具栏提供「阶段」「操作」「子任务」。把「阶段」拖到画布空白处新建 stage，或单击「阶段」放到默认位置；把「操作」拖到 stage（或 operation 标签）新建 operation；把「子任务」拖到 stage 新建进入效果 `create_child`。单击「操作」「子任务」后再点击 stage 也可以放置。新 operation 初始没有角色，检查器会聚焦角色复选框；勾选角色会同时加入该 stage 的 active roles。事件和 `set_state` 仍在检查器里添加；画布空白处双击仍可新建 stage。转移只能通过连线创建，没有「+ 转移」按钮。
- **全员确认模板**：stage 检查器提供「全员确认」表单，选择角色和目标 stage 后生成三个普通 AST 对象——stage 变量 `approved: actor_set`、添加 `event.actor` 的操作、`on: approved ⊇ active_actors('<role>')` 的转移。它们可以像其他对象一样编辑或删除；「模板」标记由结构推导，不写入 AST。生成的 operation 仍以画布上的标签显示。
- **连线**：从 Start 拖到 stage 设置初始 stage；从 stage 右侧拖到另一个 stage 生成 `on = "false"`、`action = ""` 的 transition；拖到「结束」生成指向 `ended` 的 transition。生成后都在检查器里编辑。
- **角色**：右侧角色面板新增角色；检查器设置 `kind`、`min_actors`、`max_actors`。operation 的允许角色只在 operation 检查器里用复选框编辑；勾选角色会同时把该角色加入 stage 的 `active_roles`。stage 的 `active_roles` 也可以在 stage 检查器里用复选框调整。
- **脚本**：operation、event 和 transition 的 action 是语句脚本；transition 的 `on` 是必须返回 boolean 的表达式。两者都是文本编辑器，没有结构化 guard 编辑器。
- **Title 模板**：按段编辑文本、字段和 `count_children`，并提供预览；模板不合法时显示原始输入和错误。
- **顺序**：`created` 按声明顺序执行，transition 按声明顺序求值；检查器提供上移/下移。
- **删除**：删除 stage 前必须先移除所有 `to` 引用和初始标记；删除角色前必须先从所有 stage 的 `active_roles` 移除。
- **布局**：在可编辑的草稿中拖动 stage 或 Start/End 标记只保存布局，不产生版本；拖动一个节点不会移动其他节点，边仍会随端点更新。「自动布局」用 dagre 从左到右重新排列所有 stage 和两个标记。布局独立保存、last-write-wins。Start/End 的坐标与 stage 一样存在布局里（键为 `__start` / `__end`）；修改初始 stage 不会移动 Start 标记。只读视图（历史版本、无权编辑的流程、Agent 账户、非所有者的平台流程）中 stage 图形和标记都不响应拖动：没有对象工具栏、图形手柄和拖动手柄，光标保持默认；operation 标签仍可点击选中 operation，检查器的 stage 分区仍列出事件、进入效果和转移。拖动空白处、按住中键或空格才平移画布。只读属性同时体现在工具栏徽标（`只读 · vN` / `只读 · 未发布`）、状态栏、检查器（文档式只读，而非禁用表单）和连线上。
- **stage 变量与结果**：`result` 是内置任务级 string 变量，不在任务变量面板声明，也不能被声明为变量；脚本和 token 提示中包含它。转移进入 `ended` 时若非空会复制到 `task.result`。

## 自动保存

草稿每个流程一份，带 revision。编辑器在修改后：

- 600ms 防抖保存；连续修改时最多等待 5s 也会保存一次。
- 同一时刻只有一个保存请求；请求携带 `If-Match: "<revision>"` 和幂等的 `client_save_id`。
- 网络错误或 5xx 按 1s 到 30s 指数退避重试，并复用同一个 `client_save_id`，不会重复应用。
- 页面隐藏或关闭时尝试 keepalive 保存；未保存时关闭页面会提示。
- 每次修改都先写入 localStorage 备份；崩溃恢复后会对比服务端草稿，避免用旧备份覆盖新内容。

工具栏状态：

| 状态 | 含义 |
|---|---|
| 已保存 | 服务端草稿与当前 AST 一致 |
| 未保存 | 有修改等待保存 |
| 保存中 | 保存请求进行中 |
| 保存失败，重试中 | 可重试错误，自动退避 |
| 冲突 | revision 不一致，自动保存已停止 |

布局保存状态显示在状态栏，和 AST 保存徽标分开：布局保存失败时草稿仍可保存、发布，状态栏提供重试。

## 冲突

保存或发布时如果服务端 revision 已前进（其他标签页、其他人或一次发布），返回 `409 draft_conflict`。编辑器不会自动合并，只提供两个选择：

- **载入服务端版本**：丢弃本地未保存修改，载入服务端草稿。
- **用我的版本覆盖**：以服务端当前 revision 为 `If-Match` 重新提交本地版本，覆盖对方的修改。

发布也会推进 revision；发布成功后编辑器立即采用新 revision，避免下一次保存和自己冲突。

## 校验与发布

保存成功后自动调用 `POST /api/v1/workflows/{id}/validate` 校验当前草稿。校验只读、不产生副作用。

- 错误（error）阻止发布；错误列表可点击，定位到对应 stage 或对象，并在检查器里高亮目标字段。
- 警告（warning）允许发布，发布结果显示警告。
- 校验结果带有对应的 AST 快照。修改后旧结果保留但降低强调并标注“基于已保存版本”；过期结果不阻止发布，发布前会 flush 并由服务端重新校验。校验请求失败时保留上一次结果。
- 发布按钮在只读、冲突、未保存、校验未完成或存在错误时禁用，原因直接显示在按钮旁。

发布前编辑器会先 flush 未保存修改，然后以当前 revision 发布。服务端在同一事务中校验、插入 `version = max(version) + 1`、推进草稿 revision 并写审计事件。

## 版本查看与对比

版本选择器跳到只读版本页 `/workflows/editor/{id}/versions/{n}`。版本页：

- 显示发布者、发布时间和完整只读流程图；stage 图形不可拖动，拖动空白处才平移画布。检查器以文档式只读展示 stage、操作、事件、进入效果、转移、任务变量和角色。
- 可选择另一个版本进行对比。对比面板按阶段、角色、任务变量、初始 stage 和 schema 分组，列出新增、删除和修改；点击变化会选中画布或侧栏中对应的对象，已删除且无法定位的条目禁用。
- 平台流程提供「复制到我的账户」，生成新的账户流程并跳到它的草稿。

标题和布局不属于版本；对比只覆盖 AST。

## 快捷键

| 快捷键 | 动作 |
|---|---|
| `Ctrl/Cmd+S` | 立即保存草稿（只读时无效） |
| `Ctrl/Cmd+Z` | 撤销 |
| `Ctrl/Cmd+Shift+Z` 或 `Ctrl/Cmd+Y` | 重做 |
| `Escape` | 清除选择、关闭对话框或消息 |
| `Delete` / `Backspace` | 删除选中的操作、事件、进入效果、转移或角色（stage 需要引用清空后在检查器删除） |
| 方向键 | 移动选中的 stage 10px；`Shift` + 方向键 1px |
| `C` | 为选中的 stage 打开转移目标选择器（等价于拖线） |
| 对象工具栏上的 Enter/Space | 「阶段」在默认位置新建 stage；「操作」「子任务」进入放置模式，再点击或用键盘选择 stage；`Esc` 取消 |

在输入框、文本域或下拉框中，撤销/重做、`Delete` 和方向键交给浏览器处理，不会修改整个 AST。快捷键持续按下时合并为一次撤销记录。所有操作都有可见的焦点样式；只读模式不绑定删除和转移快捷键。

## 故障排查

| 现象 | 处理 |
|---|---|
| `/workflows/editor/` 返回 503，提示未构建 | 在 `web/workflow-editor` 执行 `npm ci && npm run build`，确认 `editor_dir` 指向 `dist` |
| 打开编辑器跳到登录页或提示会话失效 | 重新登录；API 返回 401 |
| 保存后出现冲突横幅 | 按需载入服务端版本或覆盖；不会自动合并 |
| 发布返回 422 | 校验错误阻止发布，按面板定位修复 |
| 流程只读 | 平台流程只有平台管理员可编辑；账户流程只有所有者可编辑 |
| 画布空白但版本存在 | 检查 `initial_stage` 和 stage 引用是否完整；残缺 AST 仍可查看，错误在检查器中显示 |

## 质量门禁

```bash
cd web/workflow-editor
npx tsc --noEmit
npm test
npm run build
```

Rust 侧沿用仓库门禁；Postgres 集成测试在未设置 `TEST_DATABASE_URL` 时跳过。
