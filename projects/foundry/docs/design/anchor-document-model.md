# 锚点文档模型

## 1. 结论

所有受治理文档共用一套机制：

- 文档是一个根锚点；
- 章节是锚点；
- 列表项也是锚点，但列表项只支持文本，不支持子章节；
- 结构由描述文件定义；
- 每个锚点有稳定定位键 `key` 和内容哈希 `id`；
- 版本、草案、目录、标注、审批围绕同一棵锚点树实现。

当前实现先落在项目文档（`kind = project`）上，其他文档类型通过增加描述文件和少量钩子接入。描述文件是服务端内部定义，不通过 API 暴露。

## 2. 描述文件

```text
DocumentSchema
├── kind
├── schema_version
└── root: ChapterSchema

ChapterSchema
├── key                 全局唯一结构键
├── title               固定标题
├── title_template      多项章节的标题模板，如 "{title} {index}"
├── occurrence          single | multiple
├── content             text | list
├── item                仅 content = list，列表项模板
├── children            子章节模板
├── required
├── min / max
└── text_required
```

规则：

- `single`：在父章节下最多出现一次。
- `multiple`：同一个章节模板可出现多次，每次是一个独立章节锚点。
- `text`：章节自身是一段 Markdown 文本。
- `list`：章节自身是有序列表，列表项是文本锚点。
- 列表项不能有子章节；需要结构化字段的条目应建模为 `multiple` 章节。
- 标题由描述文件/API 决定，不属于正文内容，不参与哈希。
- 创建草案时，系统按描述文件预生成所有“单次”章节及其单次子章节；`multiple` 章节实例和列表项不预生成，按需新增。
- 一个 `multiple` 章节必须是父节点的唯一子章节；否则 schema 校验失败。因此多项章节通常挂在一个单次容器章节下，例如 `goals` 容器下的 `goal`、`background.problems` 容器下的 `problem`。

项目文档中的对应关系：

| 内容 | 建模 |
|---|---|
| 定位.是什么 | 单次文本章节 |
| 定位.不是什么 | 列表章节，列表项为文本 |
| 背景.现状 | 单次文本章节 |
| 背景.问题 | 多项章节，每个问题一个实例 |
| 问题的影响对象、频率、代价、证据 | 问题实例的子章节或列表 |
| 目标、非目标、约束、风险、术语表 | 多项章节 |
| 别名、证据、引用列表 | 列表章节 |

## 3. 锚点

```text
Anchor
├── id           内容哈希，sha256:...
├── key          稳定位置哈希，k:...
├── path         内部可读路径，仅用于调试，不通过结构接口暴露
├── schema_key   对应描述文件中的结构键（内部字段，不通过结构接口暴露）
├── kind         document | chapter | item
├── title        API/描述文件决定的显示名称
├── content_kind text | list
├── text         Markdown 文本
├── items        list 章节的文本项
└── children     子章节
```

`key` 由服务端根据父键、章节键和出现键生成：

```text
key = "k:" + sha256(parent_key + schema_key + occurrence_key)
```

它与内容无关，所以修改文本不会改变 key。Agent 通过 `structure` 读取 key，不需要自己拼装。

内部 `path` 仍然存在，仅用于调试和测试，例如：

```text
document
positioning.what_is_not/e1
background.problems/problem/p1
goals/goal/g1
```

出现键由创建时确定：有业务 ID 用业务 ID，没有则由服务端生成 UUID，之后不复用。

## 4. 哈希

哈希输入：

```json
{
  "key": "k:...",
  "schema_key": "goal",
  "kind": "chapter",
  "occurrence_key": "g1",
  "content_kind": "text",
  "text": "员工可自助查询报销进度",
  "items": [],
  "children": ["sha256:..."]
}
```

```text
id = "sha256:" + hex(sha256(UTF-8 JSON))
```

规则：

- `items` 和 `children` 的顺序参与哈希。
- 子锚点 ID 参与哈希，因此任何后代内容变化都会改变祖先和根锚点的 ID。
- `title`、作者、时间不参与哈希。
- 文档根的哈希就是整份文档的内容哈希。

## 5. 操作

```text
add          在父锚点下新增章节或列表项
update_text  修改锚点文本
delete       删除多项章节实例或列表项；有子项时要求 cascade
move         在同一父锚点内排序
```

所有写操作使用 `If-Match: "<revision>"`，并带 `op_id` 做幂等。

操作约束：

- 文档根不能删除、不能移动、不能改文本。单次章节不能删除，但可以 update_text；多项章节实例和列表项可以删除、移动。
- 列表项不能作为父锚点。
- 单次章节重复新增会失败。
- 多项章节可以按出现键新增多个实例。
- 删除有多项子项的章节实例必须显式 `cascade=true`。

## 6. 存储

当前原型把锚点树序列化为快照：

```sql
anchor_documents           -- 每个项目、每种文档一份
anchor_document_drafts     -- 共享草案，revision + snapshot
anchor_document_versions   -- 不可变版本，snapshot + root_anchor_id
anchor_document_operations -- 操作日志，按 op_id 幂等
anchor_document_approvals  -- 审批记录
anchor_document_annotations -- 按锚点键 + 内容哈希记录的标注
```

- 草案保存完整快照和当前根锚点 ID。
- 版本保存批准时的快照；历史版本共享未变化的内容。
- 每个锚点已经有独立的 `id`、`key` 和内容字段，后续可以拆成“一行一锚点”的 `anchor_document_anchors` 表，API 不变。

## 7. 接口

```text
GET  /api/v1/projects/{project}/documents/{kind}?anchor={key|id}
GET  /api/v1/projects/{project}/documents/{kind}/draft      # open
POST /api/v1/projects/{project}/documents/{kind}/draft      # create
GET  /api/v1/projects/{project}/documents/{kind}/content?anchor={key|id}
POST /api/v1/projects/{project}/documents/{kind}/draft/operations
POST /api/v1/projects/{project}/documents/{kind}/draft/validate
GET  /api/v1/projects/{project}/documents/{kind}/draft/approvals
POST /api/v1/projects/{project}/documents/{kind}/draft/approvals
GET  /api/v1/projects/{project}/documents/{kind}/annotations
POST /api/v1/projects/{project}/documents/{kind}/annotations
POST /api/v1/projects/{project}/documents/{kind}/annotations/{id}/resolve
GET  /api/v1/projects/{project}/documents/{kind}/versions
GET  /api/v1/projects/{project}/documents/{kind}/versions/{version}
```

`anchor` 不传时返回根锚点，即整篇文档。

- `GET /documents/{kind}` 返回 `text/markdown`：标题、元数据和 Markdown 目录；目录条目带 `key`。
- `content` 单独返回该锚点子树的 Markdown；根锚点返回整篇文档。
- `add` 的 `parent` 使用目录返回的 `key`；示例用 `k:...` 表示。

```json
{
  "operations": [
    {
      "op_id": "uuid",
      "type": "add",
      "parent": "k:<goals 的 key>"
    },
    {
      "op_id": "uuid",
      "type": "add",
      "parent": "k:<goal_problem_refs 的 key>",
      "occurrence_key": "p1",
      "text": "p1"
    }
  ]
}
```

- `update_text` 示例：

```json
{
  "operations": [
    {
      "op_id": "uuid",
      "type": "update_text",
      "anchor": "k:<g1 的 key>",
      "text": "员工可自助提交并查询报销进度"
    }
  ]
}
```

- `delete` 示例：

```json
{
  "operations": [
    {
      "op_id": "uuid",
      "type": "delete",
      "anchor": "k:<g1 的 key>",
      "cascade": true
    }
  ]
}
```

- `move` 示例：

```json
{
  "operations": [
    {
      "op_id": "uuid",
      "type": "move",
      "anchor": "k:<e2 的 key>",
      "after": "k:<e1 的 key>"
    }
  ]
}
```

## 8. 版本与审批

1. 每个文档一份共享草案，初始 revision 为 1。
2. 每次有效修改产生新快照，revision 加一。
3. 批准的 revision 生成不可变版本，版本号加一，revision 不变。
4. 下一次修改才递增 revision。
5. 拒绝只记录审批记录。
6. 只有人类 Maintainer/Owner 可以审批；Agent 只能编辑草案。

## 9. 校验

审批前校验：

- 文档 `kind` 和 `schema_version` 匹配；
- 锚点 `schema_key` 存在于描述文件；
- `content_kind` 与描述文件一致；
- 必填章节存在；
- 多项章节满足 `min` / `max`；
- `text_required` 的章节和列表项文本非空；
- 锚点键在文档内唯一；
- 每个锚点的 `id` 与内容哈希一致；
- 列表项没有子章节；
- 文档总字节数不超过上限。

## 10. 当前范围

已实现：

- 描述文件模型和项目文档描述；
- 锚点树、哈希、增删改查、排序、Markdown 渲染；
- 草案、revision、版本、审批；
- 内存适配器和 Postgres 适配器；
- 按锚点标注，内容变化后自动标记为过期；
- HTTP 查询与写入接口；
- 控制台页面、MCP 工具和 Agent 运行时工具；
- 旧项目文档接口、页面和平台 Agent 模板已切换到 `kind = project`；
- 领域单元测试和 HTTP 端到端测试。

尚未接入：

- 通用 Web 页面；
- 需求文档、设计文档的描述文件迁移；
- 旧项目文档历史数据迁移（旧表保留，不再由接口读取）；
- 锚点一行一行的独立存储（当前为快照）。
