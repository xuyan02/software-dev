# RFC 0020：需求子系统

- 状态：Draft，设计待评审
- 日期：2026-11-09
- 关联：[ADR 0004 Issue 是问题空间，Requirement 是承诺空间](../adr/0004-issue-vs-requirement.md)、[RFC 0008 软件生命周期模型](0008-software-lifecycle-model.md)、[RFC 0009 产物血缘、基线与变更管理](0009-artifact-lineage-baselines-and-change.md)、[RFC 0010 辅助系统注册与产物契约](0010-auxiliary-systems-and-evidence.md)、[RFC 0011 需求管理与需求分析](0011-requirements-management.md)、[RFC 0012 架构与设计管理](0012-architecture-and-design-management.md)
- 取代：本 RFC 取代 RFC 0011 中以 Requirement 聚合为中心的需求管理模型和“需求文档为自由文本”的假设。Issue 仍是未批准候选的问题空间；需求分析的结构化真相改为项目唯一的需求文档。

## 实现状态（2026-11-09，第一切片）

已实现：

- 领域模型：users / functions / operations / stories / interactions / screens / images，结构化操作与校验。
- 应用服务与端口：读取文档、草案、应用操作、校验、发布、版本查询。
- 存储：Postgres（迁移 `0010_requirements_documents.sql`）和内存适配器；版本不可变、草案 revision、操作日志、幂等、冲突。
- HTTP API：GET 文档/草案/版本，POST 操作/校验/发布（`If-Match` 传递 revision）。
- MCP 工具：`get_requirements_document`、`get_requirements_draft`、`apply_requirements_operations`、`validate_requirements_document`、`publish_requirements_document`。
- 测试：领域单元测试、HTTP 端到端测试、Postgres 集成测试（无 `TEST_DATABASE_URL` 时跳过）。

尚未实现：

- Web 人类文档渲染和结构化编辑器。
- 图片二进制上传与对象存储；当前只有图片元数据和引用模型。
- 交互设计/UI 设计的独立阶段评审门禁和阶段版本；当前操作已支持这些内容，但阶段审批未强制。
- CLI 命令。
- Postgres 集成测试尚未在本地数据库上运行。

## 0. 结论

系统为每个项目维护**唯一的“需求文档”**。它是需求分析阶段的结构化真相：人类看到的是渲染后的文档，Agent 操作的是结构化对象。

核心决定：

1. **一个项目一份需求文档。** 文档属于项目，不存在跨项目需求，也不存在项目之外的需求。
2. **两个维度：用户和功能。** 用户是用户模型（persona）；功能是功能模块。
3. **每个功能下面有故事和操作两个维度。** 操作归属功能模块；故事引用用户和操作。
4. **操作是用户的一次不可分割交互。** 例如“搜索”：输入关键字和点击搜索按钮是交互概念，在操作层面不区分。
5. **故事是一个场景下用户为实现一个目标进行的操作的总和。** 故事有场景、目标、用户、操作序列和优先级。
6. **用户模型是想象中的具体的人。** 包含姓名、年龄、性别、职业、爱好、习惯、目标、痛点、权限、场景、证据和置信度，不是抽象角色。
7. **结构化数据是唯一真相。** 人类文档由结构化数据渲染生成；Agent 不编辑文档文本，只发结构化操作。
8. **文档有版本、草案和操作日志。** 已发布版本不可变；编辑在草案上进行；草案带 revision 和 base_version。
9. **多 Agent 并发采用乐观并发和显式冲突。** 不同对象可顺序应用；同一对象/字段冲突返回 409 和 diff；不允许 last-write-wins 静默覆盖。
10. **Issue 管理未批准候选。** 需求文档的批准版本是需求基线；未批准的需求、讨论和证据留在 Issue。
11. **没有跨项目需求。** 外部系统是当前需求的依赖、约束或假设，不是另一个项目的需求；新系统/新产品由人类决定是否新建项目。
12. **Agent 默认做需求分析。** Agent 建立和更新用户模型、写故事、发现操作、检查覆盖；人类评审和批准。
13. **需求设计的三个阶段都写在同一个需求文档里。** 需求分析产出用户/功能/故事/操作；交互设计产出交互规格和交互流程；UI 设计产出界面和 UI 规格。三个阶段各自评审，但文档只有一个。
14. **文档是项目产物。** 它进入产物血缘，可被基线、变更和下游设计引用。
15. **交互和 UI 是文档的结构化维度。** 每个操作可以有交互规格；每个故事可以有交互流程；每个功能可以有界面；界面引用设计系统、组件和原型，不把像素数据塞进文档。
16. **阶段评审是文档版本的一部分。** 每个阶段通过后形成阶段评审记录和版本快照；下游发现上游问题，回到对应阶段，不在下游打补丁。
17. **交互/UI 设计不能改变已批准的需求分析。** 发现需求问题回到需求分析/需求管理；交互和 UI 只细化和实现已批准的用户、功能、故事、操作。
18. **像素级设计工具不进入 Foundry。** Foundry 管结构化规格、界面结构和外部设计引用；具体视觉稿可以在 Figma 等工具中完成。
19. **UI 设计支持图片。** 界面可以关联界面稿、截图、线框、视觉参考、图标、插图和标注图；图片存对象存储，按 SHA-256 内容寻址，文档只存引用、元数据、标注和版本哈希。
20. **图片是补充，不是行为真相。** 行为、状态和交互规则仍以结构化规格为准；图片帮助评审视觉和布局，不能替代结构化需求。

## 1. 目标与非目标

### 1.1 目标

- 让项目只有一份结构化、可版本化、可并发的需求真相。
- 让 Agent 通过结构化操作高效、安全地做需求分析，而不是编辑长文本。
- 让人类能阅读、评审、批准一份清晰的需求文档。
- 让用户模型、功能、故事、操作之间有明确关系和粒度。
- 让需求分析从“写文档”变成“可校验的结构化建模”。
- 让交互设计和 UI 设计与需求共用同一份结构化文档，而不是另起文档或第二真相。
- 让 UI 设计可以引用、版本化和评审图片资产，同时保持结构化规格为真相。
- 让多个 Agent 可以同时工作，冲突显式可见、可处理。
- 让需求文档与 Issue、设计、任务、测试、发布形成追溯。
- 让需求分析阶段有明确的评审和基线。

### 1.2 非目标

- 不实现跨项目需求、需求共享或需求市场。
- 不实现 Opportunity、产品线或组合层级。
- 不把需求文档做成自由文本 Wiki；文本编辑不是真相。
- 不实现像素级设计工具；交互和 UI 以结构化规格、界面结构和外部设计引用进入需求文档，不把 Foundry 做成 Figma。
- 不实现图片像素编辑；图片在外部工具制作或生成，Foundry 管引用、版本、标注、评审和访问控制。
- 不实现实时协同编辑/CRDT；先做乐观并发和显式冲突。
- 不实现自动批准；批准由人或授权角色完成。
- 不实现通用流程建模；故事和操作不是 BPMN。
- 不要求所有需求一开始就完整；草案允许不完整，发布前必须通过校验。
- 不实现代码生成；需求文档不直接生成实现。

## 2. 术语与不变量

| 术语 | 含义 |
|---|---|
| RequirementsDocument | 项目唯一的需求文档，结构化数据。 |
| UserModel / Persona | 想象中的具体用户：年龄、性别、职业、习惯、目标、痛点等。 |
| Function | 功能模块，例如“购物”。 |
| Operation | 用户的一次不可分割交互，例如“搜索”。 |
| Story | 一个场景下用户为实现目标进行的操作总和。 |
| InteractionSpec | 操作或故事的交互规格：状态、反馈、错误、加载、空状态、无障碍、文案。 |
| InteractionFlow | 故事层面按操作顺序组织的交互步骤。 |
| Screen | 功能模块下的界面结构：布局、组件、状态、设计引用、可访问性。 |
| UISpec | 界面规格集合：界面、组件、设计 token、资源和原型引用。 |
| DesignRef | 外部设计引用：Figma、组件库、设计 token、原型、资源。 |
| ImageAsset | 图片资产：界面稿、截图、线框、视觉参考、图标、插图、标注图；内容寻址。 |
| AssetRef | 文档中对图片资产的引用：asset id + hash + mime + 尺寸。 |
| ImageAnnotation | 图片标注：坐标、文本、关联操作/状态/界面。 |
| StageReview | 需求分析、交互设计、UI 设计的阶段评审记录。 |
| Draft | 可编辑的文档草案，带 revision 和 base_version。 |
| DocumentVersion | 已发布的不可变文档版本。 |
| OperationLog | 结构化操作的只追加日志。 |
| Conflict | 两个操作修改同一对象/字段且无法自动合并。 |
| RequirementsBaseline | 已批准的需求文档版本。 |
| ExternalDependency | 外部系统提供的能力，记录为依赖/假设，不是需求。 |

不变量：

1. 每个项目恰好一份需求文档；文档不能跨项目引用。
2. 已发布 DocumentVersion 不可变；变更产生新版本。
3. 草案有单调递增的 revision 和 base_version；每个结构化操作带 `op_id`、`base_revision`。
4. 用户、功能、故事、操作都有稳定 id；重命名不改 id。
5. 每个故事至少引用一个 UserModel 和一个 Operation。
6. Operation 归属一个 Function；Story 归属一个 Function。
7. Story 引用的 Operation 默认属于同一 Function；跨 Function 场景由多个 Story 通过链接表达，不硬塞进一个 Story。
8. 结构化数据是真相；渲染文档是投影；导出物不是真相。
9. Agent 只能通过结构化操作修改文档；不能直接改文本或版本快照。
10. 同一对象/字段的并发修改必须显式冲突；不允许 last-write-wins。
11. 已批准版本进入需求基线；基线引用具体版本。
12. 未批准的需求、讨论和证据留在 Issue；批准后进入需求文档。
13. 外部系统是依赖/约束/假设，不是跨项目需求。
14. 需求分析的批准由人负责；Agent 不能批准。
15. 文档、版本、操作日志和评审记录可追溯、可审计。

## 3. 模型

### 3.1 层级

```text
Project
└─ RequirementsDocument
   ├─ users: UserModel[]
   └─ functions: Function[]
        ├─ operations: Operation[]
        │    └─ interaction: InteractionSpec
        ├─ stories: Story[]
        │    └─ interaction_flow: InteractionFlow
        └─ screens: Screen[]
             └─ ui: UISpec / DesignRef
```

### 3.2 UserModel（Persona）

```json
{
  "id": "persona-customer-zhang",
  "name": "张先生",
  "age": 35,
  "gender": "male",
  "occupation": "上班族",
  "habits": ["晚上下单", "先看评价"],
  "goals": ["快速找到商品", "确认到货时间"],
  "pain_points": ["搜索结果不准确", "退款进度不透明"],
  "permissions": ["只能看自己的订单"],
  "scenarios": ["通勤时浏览", "晚上下单"],
  "devices": ["手机"],
  "accessibility": ["字号偏大"],
  "evidence": ["访谈-2026-11-01", "工单-8812"],
  "confidence": "medium",
  "status": "draft|active|retired"
}
```

规则：

- Persona 是“想象中的具体的人”，但必须有证据锚点，不能是刻板印象。
- 每个项目 3–7 个主要 persona，分 primary / secondary / negative。
- Persona 与项目角色/权限是两个概念：persona 是具体的人，role 是系统权限。
- Persona 有版本和置信度；证据不足时标记低置信度。

### 3.3 Function（功能模块）

```json
{
  "id": "func-shopping",
  "name": "购物",
  "description": "浏览、搜索、加购、下单",
  "owner": "product-shopping",
  "status": "draft|active|deprecated",
  "operations": ["op-search", "op-add-to-cart", "op-checkout"],
  "stories": ["story-search-add-to-cart"],
  "screens": ["screen-search", "screen-product"]
}
```

规则：

- 功能是功能模块，不是单个页面或单个按钮。
- 功能有 owner；无主功能是风险。
- 功能可以拆分子功能，但不要过早层级化。

### 3.4 Operation（操作）

```json
{
  "id": "op-search",
  "function_id": "func-shopping",
  "name": "搜索",
  "description": "用户输入关键字并查看结果",
  "actor_user_ids": ["persona-customer-zhang"],
  "trigger": "用户在购物页发起搜索",
  "preconditions": ["用户在购物页"],
  "inputs": ["keyword"],
  "outputs": ["product_list"],
  "states": ["idle", "searching", "results", "empty", "error"],
  "exceptions": ["network_error", "invalid_keyword"],
  "permissions": ["所有已登录用户"],
  "side_effects": ["记录搜索日志"],
  "audit": ["搜索关键字不落个人敏感信息"],
  "interaction_id": "int-op-search",
  "status": "draft|active|deprecated"
}
```

规则：

- 操作粒度是**一次不可分割的用户交互**；“输入关键字 + 点击搜索”是一个操作，不拆成两个。
- 操作归属一个功能模块。
- 操作可以被多个故事引用。
- 操作必须声明 actor、trigger、inputs/outputs；复杂操作声明状态和异常。
- 操作不是实现步骤；不描述内部函数调用。

### 3.5 Story（故事）

```json
{
  "id": "story-search-add-to-cart",
  "function_id": "func-shopping",
  "title": "搜索商品并加入购物车",
  "user_ids": ["persona-customer-zhang"],
  "scenario": "客户想买某件商品",
  "goal": "找到商品并加入购物车",
  "preconditions": ["用户已登录"],
  "postconditions": ["商品在购物车"],
  "operation_ids": ["op-search", "op-add-to-cart"],
  "interaction_flow_id": "flow-story-search-add-to-cart",
  "screen_ids": ["screen-search", "screen-product"],
  "priority": "must",
  "source_issue_ids": ["issue-123"],
  "acceptance_notes": ["搜索结果相关", "加入购物车成功"],
  "status": "draft|review|approved|deprecated"
}
```

规则：

- 故事是一个场景下用户为实现目标进行的操作总和。
- 故事必须引用至少一个 UserModel 和至少一个 Operation。
- 故事的操作有序；同一操作可以在不同故事中复用。
- 故事默认在同一个功能模块内；跨功能场景由多个故事通过链接组成。
- 故事不是用户故事模板的一句话；它包含场景、目标、前置/后置、操作序列和验收要点。

### 3.6 关系

```text
UserModel ──< Story >── Operation
                  │           │
                  └── Function ┘
```

- 一个用户可以有多个故事；
- 一个故事可以引用多个用户（例如客服和客户）；
- 一个故事引用多个操作；
- 一个操作可以被多个故事引用；
- 故事和操作都属于一个功能模块。

### 3.7 InteractionSpec（交互规格）

交互设计阶段为操作和故事补充用户侧的反馈和状态。

```json
{
  "id": "int-op-search",
  "operation_id": "op-search",
  "story_ids": ["story-search-add-to-cart"],
  "states": [
    {
      "state": "idle",
      "user_sees": "搜索框和热门推荐",
      "actions": ["输入关键字"],
      "transitions": ["searching"]
    },
    {
      "state": "searching",
      "user_sees": "加载指示",
      "actions": ["取消"],
      "transitions": ["results", "empty", "error"]
    },
    {
      "state": "results",
      "user_sees": "结果列表和筛选",
      "actions": ["筛选", "加入购物车"],
      "transitions": ["idle"]
    }
  ],
  "feedback": {
    "success": "...",
    "error": "...",
    "loading": "...",
    "empty": "...",
    "permission": "..."
  },
  "accessibility": ["键盘可达", "屏幕阅读器标签", "对比度", "焦点顺序"],
  "copy": {
    "title": "搜索",
    "placeholder": "输入商品",
    "errors": { "network_error": "网络异常，请重试" }
  },
  "prototype_ref": { "type": "figma|html|image", "ref": "..." },
  "status": "draft|review|approved"
}
```

规则：

- 每个操作在交互设计阶段应有 InteractionSpec，或显式标记“无需交互”。
- 交互状态必须覆盖操作的状态和异常：成功、失败、加载、空、权限不足。
- 交互不能改变操作语义；发现语义问题回到需求分析。
- 交互可以有原型引用，但原型不是真相。

### 3.8 InteractionFlow（交互流程）

故事层面把操作和交互状态串起来：

```json
{
  "story_id": "story-search-add-to-cart",
  "steps": [
    { "operation_id": "op-search", "interaction_state": "idle", "screen_id": "screen-search" },
    { "operation_id": "op-search", "interaction_state": "results", "screen_id": "screen-search" },
    { "operation_id": "op-add-to-cart", "interaction_state": "idle", "screen_id": "screen-product" },
    { "operation_id": "op-add-to-cart", "interaction_state": "added", "screen_id": "screen-product" }
  ],
  "status": "draft|review|approved"
}
```

规则：

- 交互流程按故事的操作顺序组织；
- 每一步引用操作、交互状态和界面；
- 流程必须覆盖故事的主路径和关键异常；
- 交互流程不能改变故事目标；发现故事问题回到需求分析。

### 3.9 Screen / UISpec（界面与 UI 规格）

UI 设计阶段为功能模块和故事补充界面结构。

```json
{
  "id": "screen-search",
  "function_id": "func-shopping",
  "name": "商品搜索页",
  "purpose": "让用户输入关键字并浏览结果",
  "layout": "顶部搜索框 + 筛选 + 结果列表",
  "components": ["search-bar", "filter-chips", "product-card"],
  "states": ["idle", "loading", "results", "empty", "error"],
  "operation_ids": ["op-search"],
  "story_ids": ["story-search-add-to-cart"],
  "design_system_refs": ["ds:search-bar@2.1", "ds:product-card@1.4"],
  "images": ["asset-screen-search-main"],
  "accessibility": ["触控目标 >= 44px", "对比度 AA", "焦点顺序"],
  "prototype_ref": { "type": "figma", "ref": "..." },
  "status": "draft|review|approved"
}
```

UISpec 是界面、组件、设计 token 和资源的集合：

```json
{
  "function_id": "func-shopping",
  "screens": ["screen-search", "screen-product"],
  "design_tokens": ["color.primary", "spacing.4", "font.body"],
  "component_refs": ["ds:search-bar@2.1", "ds:product-card@1.4"],
  "status": "draft|review|approved"
}
```

规则：

- 屏幕必须链接到至少一个操作或故事；无孤儿屏幕。
- UI 必须覆盖故事的主路径和关键异常；缺失状态是校验错误。
- UI 不能改变交互语义；发现交互问题回到交互设计。
- Foundry 存界面结构和外部设计引用，不存像素级视觉数据。
- 设计系统/组件引用必须可解析；不可解析时标记无效。

#### ImageAsset（图片资产）

```json
{
  "id": "asset-screen-search-main",
  "screen_id": "screen-search",
  "type": "mockup|screenshot|wireframe|reference|icon|illustration|annotation",
  "storage": {
    "provider": "s3",
    "key": "projects/.../assets/sha256/...",
    "hash": "sha256:...",
    "mime": "image/png",
    "width": 1440,
    "height": 900,
    "size_bytes": 123456
  },
  "alt_text": "商品搜索页：顶部搜索框、筛选标签和结果列表",
  "caption": "主界面稿 v3",
  "annotations": [
    {
      "id": "ann-1",
      "x": 0.12,
      "y": 0.08,
      "text": "搜索框",
      "operation_id": "op-search"
    }
  ],
  "source": { "type": "upload|figma|screenshot|generated", "ref": "..." },
  "sensitivity": "internal",
  "status": "draft|review|approved|superseded"
}
```

规则：

- 图片内容存对象存储；文档和 Postgres 只存引用、哈希、尺寸和标注。
- 图片按 SHA-256 内容寻址；替换图片产生新 asset，旧版本保留。
- 有意义的图片必须有 `alt_text`；图片不能是唯一的信息载体。
- 标注坐标归一化到 0–1，可关联操作、状态或界面。
- 允许 PNG/JPEG/WebP/SVG；SVG 必须清洗，禁止脚本和外部引用。
- 敏感图片设置 `sensitivity`，访问控制、脱敏和保留策略适用。
- 图片是视觉参考，不是行为真相；行为和状态仍以结构化规格为准。

### 3.10 三个阶段的文档组合

同一份需求文档在三个阶段逐步丰富：

```text
阶段 1 需求分析：
  users / functions / stories / operations

阶段 2 交互设计：
  + InteractionSpec / InteractionFlow

阶段 3 UI 设计：
  + Screen / UISpec / DesignRef
```

- 三个阶段共享同一个文档、同一套版本和草案机制。
- 每个阶段有独立评审和阶段版本快照。
- 下游阶段只能细化和实现上游阶段，不能静默改变上游语义。
- 发现上游问题时，回到对应阶段，不在下游打补丁。

## 4. 结构化操作

### 4.1 操作类型

| 操作 | 说明 |
|---|---|
| `user.add` / `user.update` / `user.remove` | 用户模型增删改 |
| `function.add` / `function.update` / `function.remove` | 功能模块增删改 |
| `operation.add` / `operation.update` / `operation.remove` | 操作增删改 |
| `story.add` / `story.update` / `story.remove` | 故事增删改 |
| `story.set_operations` | 设置故事的操作序列 |
| `story.link_user` / `story.unlink_user` | 故事与用户关联 |
| `operation.set_interaction` | 设置操作的交互规格 |
| `story.set_interaction_flow` | 设置故事的交互流程 |
| `screen.add` / `screen.update` / `screen.remove` | 界面增删改 |
| `screen.link_operations` / `screen.link_stories` | 界面关联操作和故事 |
| `screen.add_design_ref` | 添加设计系统、组件、原型或资源引用 |
| `asset.upload` | 上传图片资产，返回 asset id 和内容哈希 |
| `asset.replace` | 替换图片资产引用，产生新版本 |
| `screen.add_image` / `screen.remove_image` | 界面关联/移除图片 |
| `asset.annotate` | 添加/更新图片标注 |
| `asset.set_alt_text` | 设置图片替代文本 |
| `ui.set_design_tokens` | 设置 UI 设计 token |
| `stage.review` / `stage.approve` | 需求分析、交互设计、UI 设计的阶段评审和批准 |
| `document.get` | 读取当前草案或指定版本 |
| `document.diff` | 比较草案与版本、或两个版本 |
| `document.validate` | 校验结构和覆盖 |
| `draft.publish` | 发布草案为新版本（需评审/批准） |
| `conflict.list` / `conflict.resolve` | 查看和解决冲突 |

### 4.2 操作格式

```json
{
  "op_id": "op-7f3a...",
  "base_revision": 17,
  "type": "operation.add",
  "target": { "function_id": "func-shopping" },
  "payload": {
    "id": "op-search",
    "name": "搜索",
    "actor_user_ids": ["persona-customer-zhang"],
    "inputs": ["keyword"],
    "outputs": ["product_list"],
    "states": ["idle", "searching", "results", "empty", "error"]
  },
  "actor": { "kind": "agent", "id": "agent-analyst-1" },
  "idempotency_key": "..."
}
```

规则：

- `op_id` 全局唯一；重试使用同一 `op_id`，服务端幂等。
- `base_revision` 是操作基于的草案 revision；不等于当前 revision 时返回 409。
- `target` 明确对象；批量操作可选，但每个子操作有独立 id。
- 服务端按操作类型校验 schema、权限和引用完整性。
- 操作日志只追加；失败的操作也记录原因。

### 4.3 查询

- 按 id 读取用户、功能、故事、操作。
- 按功能列故事和操作。
- 按用户列相关故事。
- 按操作列引用它的故事。
- 全文/关键词搜索（投影，不是真相）。
- 读取指定版本或当前草案。

## 5. 人类文档

### 5.1 渲染

从结构化数据生成人类可读文档：

```text
# 需求文档：电商系统

## 1. 用户模型
### 张先生（persona-customer-zhang）
...

## 2. 功能：购物
### 2.1 操作
- 搜索（op-search）
- 加入购物车（op-add-to-cart）
### 2.2 故事
#### 搜索商品并加入购物车
- 用户：张先生
- 场景：...
- 目标：...
- 操作：搜索 → 加入购物车
### 2.3 交互流程
- 搜索 idle → 输入关键字 → searching → results
- 加入购物车 idle → added
### 2.4 界面与 UI 规格
- 商品搜索页（screen-search）：搜索框、筛选、结果列表
- 商品详情页（screen-product）：商品信息、加入购物车
- 设计引用：ds:search-bar@2.1、ds:product-card@1.4
- 图片：
  - 图片：商品搜索页主界面稿（asset-screen-search-main，主界面稿 v3）
  - 标注：搜索框 → 搜索操作
```

规则：

- 渲染是投影；修改必须通过结构化操作。
- 人类可以在渲染文档上评论、评审、批准，但不能直接改文本作为真相。
- 支持导出 Markdown/HTML/PDF，导出物明确标记为快照。
- 文档显示版本、草案 revision、校验状态和评审状态。
- 图片在渲染文档中以内联缩略图/灯箱显示，带 caption 和 alt text。
- 图片 diff 显示新增、替换、删除；替换时显示新旧哈希和并排对比。
- 图片下载/预览使用短期签名 URL，按项目权限和图片分类控制。

### 5.2 人类编辑

- 人类通过结构化编辑器（表单/表格/树）修改用户、功能、故事、操作。
- 编辑器使用同一套结构化操作和 revision。
- 人类不能绕过校验直接写入非法结构。
- Agent 和人类共用同一套操作和冲突规则。

## 6. 版本、草案、冲突

### 6.1 版本

- 文档有 `version`，从 1 开始；每次发布递增。
- DocumentVersion 保存完整结构化快照、操作日志区间、评审记录和批准人。
- 已发布版本不可变；旧版本保留用于追溯。
- 需求基线引用具体版本。

### 6.2 草案

- 每个项目一份当前草案，带 `revision` 和 `base_version`。
- 草案从某个已发布版本派生；也可以是初始草案。
- 草案可编辑，不直接影响基线。
- 发布前必须通过校验和评审。
- 草案可以放弃；放弃后回到 base_version 或新的草案。

### 6.3 多 Agent 并发

```text
Agent A: op 1 (base_revision 17) → 服务端应用到 revision 18
Agent B: op 2 (base_revision 18) → 服务端应用到 revision 19
Agent C: op 3 (base_revision 17) → 409，返回当前 diff
```

规则：

1. 服务端串行化操作，按 `base_revision` 检查。
2. 不同对象/字段的操作可以顺序应用。
3. 同一对象/字段的并发修改产生冲突：
   - 返回 409 和当前对象状态、冲突操作、diff；
   - Agent 重新读取、重放或请求人工决策；
   - 不允许 last-write-wins。
4. 可选：热点对象使用 ETag 或租约，减少冲突。
5. 操作日志支持审计和重放；重放不产生副作用。
6. 冲突解决写入日志，记录解决者和理由。

### 6.4 发布与批准

```text
draft → validate → review → approve → publish(version+1)
```

- 校验必须通过。
- 评审由产品、用户代表、技术、QA 等参与。
- 批准由人或授权角色完成；Agent 不能批准。
- 发布后可创建需求基线。
- 发布失败不改变已发布版本。

### 6.5 阶段版本与评审

需求设计的三个阶段共享同一份文档，但有独立的阶段评审：

```text
阶段 1 需求分析：users / functions / stories / operations
  → 阶段评审 → 阶段版本快照 vN
阶段 2 交互设计：+ InteractionSpec / InteractionFlow
  → 阶段评审 → 阶段版本快照 vN+1
阶段 3 UI 设计：+ Screen / UISpec / DesignRef
  → 阶段评审 → 阶段版本快照 vN+2
```

规则：

- 每个阶段有独立的评审人、发现项和批准记录。
- 阶段版本是文档版本，不是另一份文档。
- 阶段批准后才能进入下一阶段。
- 下游发现上游问题，回到对应阶段；已经通过的阶段版本保留，但新版本会取代它作为当前基线。
- 最终 UI 设计批准的版本是完整设计基线。
- Agent 可以起草和检查每个阶段的内容，但不能批准任何阶段。

### 6.6 图片资产版本

- 图片内容按 SHA-256 内容寻址；文档版本引用具体 hash。
- 替换图片产生新 asset 和新的文档操作；旧 asset 保留，不覆盖。
- 同一图片引用被两个 Agent 并发替换时产生冲突，不能静默覆盖。
- 图片标注、alt text、caption 等元数据跟随文档 revision 和冲突规则。
- 已发布版本固定图片 hash；旧图片按保留策略归档。
- 移除图片只解除引用；物理删除按项目保留策略执行。

## 7. 需求设计三阶段流程

### 7.1 输入

- 项目 ProjectBrief 和项目级 non-goals；
- 已批准的需求/Issue；
- 用户研究、工单、数据、访谈；
- 外部依赖和约束；
- 历史决策和证据。

### 7.2 阶段 1：需求分析

```text
1. 建立/更新用户模型
   Agent 从证据提炼 persona，标注证据和置信度。

2. 建立功能结构
   识别功能模块，分配 owner。

3. 写故事
   从 persona 和场景出发，定义目标、前置/后置、优先级、来源 Issue。

4. 发现操作
   沿故事走查，提炼不可分割的用户交互；补状态、权限、异常、副作用。

5. 迭代
   操作暴露故事缺口，故事补充变体；用户模型补充新角色/约束。

6. 处理派生需求
   同项目内新建功能/操作/故事；外部系统记录为依赖/假设。

7. 校验
   结构、引用、覆盖、孤儿、ID、跨项目检查。

8. 阶段评审/批准
   产品、用户代表、技术、QA 评审；通过后形成阶段版本，进入交互设计。
```

输出：users、functions、stories、operations、阶段版本。

### 7.3 阶段 2：交互设计

输入：已通过的阶段 1 版本。

```text
1. 为每个操作建立 InteractionSpec
   状态、反馈、错误、加载、空、权限不足、无障碍、文案。

2. 为每个故事建立 InteractionFlow
   按操作顺序串联操作、交互状态和界面；覆盖主路径和关键异常。

3. 原型验证
   必要时用原型验证流程；原型引用不是真相。

4. 校验
   每个操作有交互规格或显式“无需交互”；状态覆盖操作状态和异常；
   无障碍要求明确；交互没有改变操作语义。

5. 阶段评审/批准
   交互设计师、产品、无障碍、技术、QA 评审；通过后进入 UI 设计。
```

输出：InteractionSpec、InteractionFlow、原型引用、阶段版本。

### 7.4 阶段 3：UI 设计

输入：已通过的阶段 2 版本。

```text
1. 建立 Screen
   每个故事/流程需要的界面；布局、组件、状态、用途。

2. 建立 UISpec
   界面集合、设计 token、组件引用、资源和原型引用。

3. 图片资产
   上传/关联界面稿、截图、线框、参考图、图标、插图；
   设置 alt text、caption 和标注；图片引用内容哈希。

4. 关联
   屏幕关联操作和故事；设计引用关联设计系统；图片关联屏幕和标注目标。

5. 校验
   每个故事有界面覆盖；每个屏幕至少关联一个操作/故事；
   设计引用可解析；图片引用可解析且有 alt text；
   可访问性和设计系统一致；UI 没有改变交互语义。

6. 阶段评审/批准
   设计负责人、品牌、前端、无障碍、产品评审；通过后形成完整设计基线。
```

输出：Screen、UISpec、DesignRef、最终设计基线。

### 7.5 派生需求

- 同项目派生：新建功能、操作、故事、交互规格或界面；链接 `derives_from` / `refines` / `depends_on`。
- 外部依赖：记录在需求的 assumptions/dependencies，写清外部系统、负责人、状态、契约；不是跨项目需求。
- 新系统/新产品：由人类决定是否新建项目；当前项目记录提案/依赖。
- 派生需求不能静默扩大范围；独立走优先级和批准。
- 交互/UI 阶段发现的新需求回到阶段 1，不在下游直接补需求。

### 7.6 阶段门禁

| 阶段 | 门禁 |
|---|---|
| 需求分析 | 每条已批准需求有用户、功能、故事覆盖；每个故事有用户和操作；每个操作有 actor/trigger/inputs/outputs；用户模型有证据；无孤儿；外部依赖有状态 |
| 交互设计 | 每个操作有交互规格或显式无需交互；状态覆盖成功/失败/加载/空/权限；无障碍明确；交互未改变操作语义 |
| UI 设计 | 每个故事有界面覆盖；屏幕关联操作/故事；设计引用可解析；图片引用可解析且有 alt text；可访问性和设计系统一致；UI 未改变交互语义 |

### 7.7 失败回退

- 阶段 1 发现：本阶段修订；需求问题回到需求管理。
- 阶段 2 发现：交互问题本阶段修订；故事/操作问题回到阶段 1；需求问题回到需求管理。
- 阶段 3 发现：UI 问题本阶段修订；交互问题回到阶段 2；故事/操作问题回到阶段 1；需求问题回到需求管理。
- 连续多轮不收敛：拆分需求、缩小范围、补充调研或重写需求，不增加新阶段。

## 8. 校验规则

### 8.1 结构

- 文档必须有至少一个用户、一个功能。
- 用户 id、功能 id、故事 id、操作 id 唯一且稳定。
- 故事必须引用至少一个用户和一个操作。
- 操作必须归属一个功能；故事必须归属一个功能。
- 故事引用的操作默认属于同一功能。
- 不允许跨项目引用。
- 名称可以重复，但 id 不能重复。

### 8.2 质量

| 检查 | 处理 |
|---|---|
| 用户缺目标/痛点 | 警告 |
| 故事缺场景/目标 | 错误，阻止发布 |
| 操作缺 actor/trigger/inputs | 错误，阻止发布 |
| 孤儿操作（无故事引用） | 警告 |
| 孤儿故事（无用户/操作） | 错误 |
| 同一功能内重复操作名 | 警告 |
| 外部依赖无负责人/状态 | 警告或阻止，按风险 |
| 用户模型无证据 | 警告；低置信度 |
| 故事操作跨功能未说明 | 错误或警告 |

### 8.3 交互校验

- 每个操作有 InteractionSpec，或显式标记“无需交互”。
- 交互状态覆盖操作状态和异常：成功、失败、加载、空、权限不足。
- 每个故事有 InteractionFlow；流程步骤引用存在的操作、交互状态和界面。
- 流程覆盖故事主路径和关键异常。
- 无障碍要求明确：键盘、屏幕阅读器、对比度、焦点。
- 交互没有新增操作、角色或状态语义；发现语义问题回到阶段 1。
- 没有孤儿 InteractionSpec。

### 8.4 UI 校验

- 每个故事有界面覆盖；关键异常有对应界面状态。
- 每个屏幕至少关联一个操作或故事；没有孤儿屏幕。
- 屏幕状态覆盖交互状态。
- 设计系统、组件、原型和资源引用可解析；不可解析时标记无效。
- 图片引用可解析，hash 匹配；mime、尺寸和大小符合限制。
- 有意义的图片有 alt text；图片不是唯一的信息载体。
- SVG 必须清洗，禁止脚本和外部引用。
- 图片标注引用有效的操作、状态或界面。
- 敏感图片有分类、访问控制和保留策略。
- 可访问性要求满足；设计系统一致。
- UI 没有改变交互语义；发现交互问题回到阶段 2。
- 没有未处理的无效设计引用。

### 8.5 阶段发布校验

- 每个阶段发布前执行该阶段的完整校验；错误阻止发布，警告允许发布但记录。
- 阶段 1 只校验用户/功能/故事/操作；阶段 2 增加交互；阶段 3 增加 UI。
- 上一阶段已批准的版本不能被静默修改；变更走新版本。
- 校验结果带对象 id 和路径，供 Agent/人类定位。

## 9. 权限、角色与 Agent 边界

### 9.1 权限

- Viewer：读取文档和版本。
- Member：编辑草案（结构化操作）。
- Maintainer/Owner：评审、批准、发布。
- 全局 Admin：管理权限；不绕过业务评审。
- 图片上传/关联：Member 及以上；敏感图片需要分类和批准。
- 图片下载/预览：按项目角色和图片分类控制；使用短期签名 URL。
- Agent：通过结构化操作编辑草案；不能批准、不能发布（除非策略显式授权且留痕）。

### 9.2 阶段角色与批准

| 阶段 | 编辑主力 | 批准 | 评审 |
|---|---|---|---|
| 需求分析 | 产品/分析 Agent | 产品负责人/需求负责人 | 用户代表、技术、QA |
| 交互设计 | 交互设计师/Agent | 交互设计负责人/产品 | 无障碍、技术、QA |
| UI 设计 | UI 设计师/Agent | 设计负责人/品牌/产品 | 前端、无障碍 |

规则：

- 每阶段批准只覆盖该阶段内容；不能代替需求批准。
- 作者不能是唯一批准人；实现者不能是唯一评审者。
- Agent 可以在三个阶段起草和检查，但不能批准任何阶段。
- 阶段批准记录进入文档版本和操作日志。

### 9.3 Agent 边界

- Agent 可以：建立/更新 persona、添加功能/操作/故事、设置操作序列、编写交互规格和界面结构、上传/关联图片资产、设置 alt text 和标注、校验、生成 diff、发现缺口。
- Agent 不能：批准任何阶段、发布版本、创建项目、接受外部依赖、绕过校验、把敏感图片标记为公开。
- Agent 起草的内容标记来源和置信度；假设和开放问题不能写成事实。
- 实现者不能是唯一评审者；holdout/外部验证由独立方负责（如适用）。

### 9.4 审计

- 所有结构化操作、发布、批准、冲突解决写入操作日志和事件。
- 记录 actor、时间、base_revision、op_id、结果。
- 操作日志不可修改；修正通过新操作。

## 10. 数据模型

```sql
create table requirements_documents (
  id            uuid primary key,
  project_id    uuid not null unique references projects (id),
  current_version integer not null default 0,
  status        text not null,
  created_at    timestamptz not null default now(),
  updated_at    timestamptz not null default now()
);

create table requirements_document_versions (
  id            uuid primary key,
  document_id   uuid not null references requirements_documents (id),
  version       integer not null,
  content       jsonb not null,
  content_hash  text not null,
  created_by    jsonb not null,
  approved_by   jsonb,
  created_at    timestamptz not null default now(),
  published_at  timestamptz,
  unique (document_id, version)
);

create table requirements_document_drafts (
  id            uuid primary key,
  document_id   uuid not null references requirements_documents (id),
  revision      integer not null default 0,
  base_version  integer not null default 0,
  content       jsonb not null,
  updated_at    timestamptz not null default now()
);

create table requirements_operations (
  id            bigserial primary key,
  draft_id      uuid not null references requirements_document_drafts (id),
  op_id         uuid not null,
  base_revision integer not null,
  type          text not null,
  target        jsonb not null,
  payload       jsonb not null,
  actor         jsonb not null,
  result        text not null,
  error         text,
  applied_at    timestamptz not null default now(),
  unique (draft_id, op_id)
);

create table requirements_reviews (
  id            uuid primary key,
  document_id   uuid not null references requirements_documents (id),
  version       integer,
  status        text not null,
  reviewers     jsonb not null default '[]'::jsonb,
  findings      jsonb not null default '[]'::jsonb,
  created_at    timestamptz not null default now(),
  decided_at    timestamptz
);

create index requirements_operations_draft_idx
  on requirements_operations (draft_id, id);
create index requirements_versions_document_idx
  on requirements_document_versions (document_id, version);

-- 交互规格和界面是文档内容的投影表，便于查询和校验；
-- 真相仍在文档草案/版本 JSON 和操作日志中。
create table requirements_interactions (
  id            uuid primary key,
  document_id   uuid not null references requirements_documents (id),
  operation_id  uuid not null,
  story_id      uuid,
  spec          jsonb not null,
  status        text not null,
  updated_at    timestamptz not null default now()
);

create table requirements_screens (
  id            uuid primary key,
  document_id   uuid not null references requirements_documents (id),
  function_id   uuid not null,
  name          text not null,
  layout        jsonb not null default '{}'::jsonb,
  components    jsonb not null default '[]'::jsonb,
  states        jsonb not null default '[]'::jsonb,
  accessibility jsonb not null default '[]'::jsonb,
  status        text not null,
  updated_at    timestamptz not null default now()
);

create table requirements_design_refs (
  id            uuid primary key,
  document_id   uuid not null references requirements_documents (id),
  screen_id     uuid references requirements_screens (id),
  type          text not null,
  ref           text not null,
  hash          text,
  status        text not null,
  created_at    timestamptz not null default now()
);

create table requirement_assets (
  id            uuid primary key,
  document_id   uuid not null references requirements_documents (id),
  type          text not null,
  storage_provider text not null,
  storage_key   text not null,
  content_hash  text not null,
  mime_type     text not null,
  width         integer,
  height        integer,
  size_bytes    bigint,
  alt_text      text,
  caption       text,
  sensitivity   text not null default 'internal',
  source        jsonb not null default '{}'::jsonb,
  status        text not null,
  created_by    jsonb not null,
  created_at    timestamptz not null default now(),
  unique (document_id, content_hash)
);

create table requirement_asset_links (
  id            uuid primary key,
  asset_id      uuid not null references requirement_assets (id),
  screen_id     uuid references requirements_screens (id),
  operation_id  uuid,
  story_id      uuid,
  role          text not null,
  created_at    timestamptz not null default now()
);

create index requirement_asset_links_asset_idx
  on requirement_asset_links (asset_id, role);

create table requirement_asset_annotations (
  id            uuid primary key,
  asset_id      uuid not null references requirement_assets (id),
  x             numeric(5,4) not null,
  y             numeric(5,4) not null,
  text          text not null,
  operation_id  uuid,
  state         text,
  created_by    jsonb not null,
  created_at    timestamptz not null default now()
);

create index requirement_assets_document_idx
  on requirement_assets (document_id, content_hash);

create table requirements_stage_reviews (
  id            uuid primary key,
  document_id   uuid not null references requirements_documents (id),
  stage         text not null,
  version       integer,
  status        text not null,
  reviewers     jsonb not null default '[]'::jsonb,
  findings      jsonb not null default '[]'::jsonb,
  approved_by   jsonb,
  created_at    timestamptz not null default now(),
  decided_at    timestamptz
);
```

说明：

- 草案和版本用 JSONB 保存结构化快照；操作日志是增量真相，快照用于读取和重放校验。
- 图片内容在对象存储；`requirement_assets` 只存元数据、哈希、尺寸、alt text、分类和链接。
- 大文档可以拆投影表（用户、功能、故事、操作）用于查询；投影可重建。
- 文档与 Artifact/血缘通过 `artifact_id` 关联；版本哈希进入产物基线。

## 11. API、MCP、CLI、Web

### 11.1 REST

```text
GET    /api/v1/projects/{project}/requirements-document
GET    /api/v1/projects/{project}/requirements-document/versions
GET    /api/v1/projects/{project}/requirements-document/versions/{v}
GET    /api/v1/projects/{project}/requirements-document/draft
GET    /api/v1/projects/{project}/requirements-document/interactions?operation={id}
GET    /api/v1/projects/{project}/requirements-document/screens?function={id}
POST   /api/v1/assets
GET    /api/v1/assets/{id}
POST   /api/v1/screens/{id}/images
DELETE /api/v1/screens/{id}/images/{asset}
POST   /api/v1/assets/{id}/annotations
POST   /api/v1/projects/{project}/requirements-document/draft/operations
POST   /api/v1/projects/{project}/requirements-document/draft/validate
POST   /api/v1/projects/{project}/requirements-document/draft/stages/{stage}/review
POST   /api/v1/projects/{project}/requirements-document/draft/stages/{stage}/approve
POST   /api/v1/projects/{project}/requirements-document/draft/publish
POST   /api/v1/projects/{project}/requirements-document/draft/abandon
GET    /api/v1/projects/{project}/requirements-document/diff?from=&to=
GET    /api/v1/projects/{project}/requirements-document/conflicts
POST   /api/v1/projects/{project}/requirements-document/conflicts/{id}/resolve
```

### 11.2 MCP

- `reqdoc.get` / `reqdoc.draft`
- `reqdoc.user.add|update|remove`
- `reqdoc.function.add|update|remove`
- `reqdoc.operation.add|update|remove`
- `reqdoc.story.add|update|remove`
- `reqdoc.story.set_operations`
- `reqdoc.operation.set_interaction`
- `reqdoc.story.set_interaction_flow`
- `reqdoc.screen.add|update|remove`
- `reqdoc.screen.link_operations|link_stories`
- `reqdoc.screen.add_design_ref`
- `reqdoc.asset.upload`
- `reqdoc.asset.attach` / `reqdoc.asset.detach`
- `reqdoc.asset.annotate`
- `reqdoc.asset.set_alt_text`
- `reqdoc.screen.add_image` / `reqdoc.screen.remove_image`
- `reqdoc.validate`
- `reqdoc.diff`
- `reqdoc.conflict.list`
- `reqdoc.stage.review` / `reqdoc.stage.approve`（approve 仅人类/授权角色）
- `reqdoc.publish`（默认仅人类/授权角色）
- `reqdoc.approve`（仅人类/授权角色）

MCP 调用带项目、actor 和 idempotency key；Agent 不能批准/发布，除非策略显式授权。

### 11.3 CLI

```bash
foundry reqdoc get <project>
foundry reqdoc user add <project> --file persona.json
foundry reqdoc function add <project> --name "购物"
foundry reqdoc operation add <project> --function func-shopping --file op.json
foundry reqdoc operation interaction <project> --operation op-search --file interaction.json
foundry reqdoc story add <project> --function func-shopping --file story.json
foundry reqdoc story interaction-flow <project> --story story-search-add-to-cart --file flow.json
foundry reqdoc screen add <project> --function func-shopping --file screen.json
foundry reqdoc asset upload <project> --screen screen-search --file mockup.png
foundry reqdoc asset annotate <project> --asset asset-screen-search-main --file annotation.json
foundry reqdoc validate <project>
foundry reqdoc stage review <project> --stage interaction
foundry reqdoc stage approve <project> --stage interaction
foundry reqdoc diff <project> --from 2 --to draft
foundry reqdoc publish <project>
```

### 11.4 Web

- 渲染文档：用户模型、功能、故事、操作、交互规格、交互流程、界面、UI 规格、追溯、开放问题。
- 结构化编辑器：增删改用户/功能/故事/操作/交互/界面，设置操作序列和交互流程。
- 阶段视图：需求分析、交互设计、UI 设计各自的状态、评审和批准。
- 图片：上传、关联界面、内联缩略图/灯箱、caption、alt text、标注、替换 diff、下载/预览。
- 图片 diff：新增、替换、删除；并排显示新旧版本和哈希。
- 版本与 diff：草案 vs 版本、版本 vs 版本、阶段版本 diff。
- 评审与批准：发现项、评论、阶段批准。
- 冲突：显示冲突对象、当前值、双方操作、解决入口。
- 校验：错误和警告定位到对象；按阶段显示校验结果。

## 12. 追溯与集成

### 12.1 追溯

```text
Issue / 来源
  → RequirementsDocument（用户/功能/故事/操作）
  → InteractionSpec / InteractionFlow
  → Screen / UISpec / DesignRef / ImageAsset
  → 架构/契约
  → Task / 测试 / 发布
```

- 文档版本是 Artifact；进入产物血缘。
- 故事和操作可以被下游设计、任务和测试引用。
- 操作 → 交互规格 → 界面形成追溯链；界面必须能追溯到操作和故事。
- 图片资产按内容哈希进入文档版本；图片与屏幕、故事、操作关联，可追溯。
- Issue 保留未批准候选和来源；文档保留已批准的结构化需求。

### 12.2 外部依赖

- 外部系统记录为 `assumption` / `constraint` / `dependency`。
- 记录外部系统、能力、负责人、状态、契约、风险。
- 不是跨项目需求；不创建、不管理其他项目的需求。

### 12.3 需求变更

- 已发布版本不可变；变更在草案上进行。
- 变更评审通过后发布新版本；旧版本保留。
- 变更影响下游设计/任务/测试时，通过血缘传播。
- 需求基线引用具体版本。
- 变更传播按阶段：
  - 需求/用户/功能/故事/操作变更 → 作废交互和 UI 阶段版本；
  - 交互变更 → 作废 UI 阶段版本；
  - UI 变更 → 如果只影响视觉，在 UI 阶段解决；如果影响行为，回到交互；
  - 任一阶段发现上游语义问题，回到对应阶段，不在下游打补丁。

## 13. 与现有实现的关系

- 现有 Issue 继续作为问题空间和未批准候选。
- 现有早期 Requirement 聚合和 RFC 0011 的 Requirement 字段模型被本 RFC 取代；迁移为需求文档中的功能/故事/操作，或保留为兼容视图。
- 现有 `documents` 表可作为渲染投影/元数据；结构化真相在需求文档表。
- 图片资产使用对象存储适配器（`crates/adapters/storage`）；Foundry 存哈希、尺寸、alt text、分类和引用，不存二进制。
- 现有 Workflow 的“AST 真相 + 草案 revision + 冲突不自动合并”模式可直接复用。
- 交互设计和 UI 设计并入同一需求文档，不另建文档；阶段评审和版本链复用同一套机制。
- 外部设计工具（Figma、设计系统、原型、资源）作为 `DesignRef` 接入；Foundry 不复制像素数据。
- 现有 `links`/`artifact_edges` 用于追溯；需求文档版本作为 Artifact。
- 现有权限模型扩展为文档编辑/评审/批准。
- 迁移策略：先只读导入现有 Requirement；人工确认后再切换为文档模型；旧数据保留。

## 14. 实施计划

### 阶段 0：文档模型与存储

- 表结构、JSON schema、版本/草案/操作日志、阶段状态。
- 迁移现有 Requirement 为只读快照。
- 出口标准：一个项目可创建文档、保存草案、读取版本。

### 阶段 1：需求分析结构化操作

- user/function/operation/story 增删改；set_operations；校验。
- 幂等、revision、409。
- 出口标准：Agent 可以通过 MCP 完整编辑用户/功能/故事/操作。

### 阶段 2：需求分析渲染与评审

- 人类文档渲染、diff、校验定位、阶段评审/批准/发布。
- 出口标准：人类可以阅读、评审、批准需求分析阶段版本。

### 阶段 3：交互设计

- InteractionSpec、InteractionFlow、交互操作、交互校验、原型引用。
- 阶段评审和版本。
- 出口标准：每个操作和故事有交互规格或显式无需交互；交互状态覆盖完整。

### 阶段 4：UI 设计

- Screen、UISpec、DesignRef、设计系统/组件/资源引用、UI 校验。
- 图片资产：对象存储、上传、内容寻址、缩略图、标注、替换、渲染、diff、访问控制。
- 阶段评审和最终设计基线。
- 出口标准：每个故事有界面覆盖；设计引用可解析；图片引用可解析且有 alt text；可访问性和设计系统一致。

### 阶段 5：并发与冲突

- 多 Agent 并发、冲突检测、冲突解决、操作重放。
- 出口标准：多个 Agent 同时编辑不同阶段不丢数据，冲突可见可解决。

### 阶段 6：需求设计助手

- Agent 自动草拟 persona、故事、操作、交互规格、界面结构；覆盖检查、派生需求建议、外部依赖记录。
- 出口标准：Agent 能完成一次端到端需求设计，人类只做评审和决策。

## 15. 验收标准

1. 每个项目恰好一份需求文档；没有跨项目需求。
2. 文档有用户、功能、故事、操作四个结构化对象和稳定 id。
3. 操作粒度是一次不可分割交互；故事是场景下操作的总和。
4. Agent 通过结构化操作编辑，不直接改文本。
5. 人类文档由结构化数据渲染生成；导出物标记为快照。
6. 文档有版本、草案 revision 和操作日志；已发布版本不可变。
7. 多 Agent 并发时，冲突返回 409 和 diff；不允许 last-write-wins。
8. 发布前校验结构、引用、覆盖、孤儿和跨项目；错误阻止发布。
9. 批准由人完成；Agent 不能批准/发布（除非显式授权并留痕）。
10. 需求文档版本进入产物血缘；可追溯到 Issue、设计、任务、测试、发布。
11. 外部依赖记录为假设/约束/依赖，不是跨项目需求。
12. 版本、草案、操作和评审可审计、可重放。
13. 交互规格和交互流程写在同一个需求文档里；每个操作/故事有交互覆盖或显式无需交互。
14. 界面和 UI 规格写在同一个需求文档里；每个故事有界面覆盖；设计引用可解析。
15. 需求分析、交互设计、UI 设计各有阶段评审和阶段版本；下游不能静默改变上游语义。
16. 最终 UI 设计批准的版本是完整设计基线；三个阶段共享一份文档。
17. UI 设计支持图片资产：上传、关联界面、内容寻址、alt text、caption 和标注。
18. 图片在渲染文档中可查看，diff 显示新增/替换/删除。
19. 图片引用可解析；SVG 清洗；敏感图片有分类和访问控制。
20. 图片是补充，不是行为真相；结构化规格仍是需求和交互的真相。

## 16. 风险与待决问题

| 风险 / 问题 | 说明 | 建议 |
|---|---|---|
| 粒度争议 | 操作和故事边界不清 | 明确“一次不可分割交互”和“场景目标”；评审校准 |
| 跨功能故事 | 一个场景跨多个功能模块 | 默认拆成多个故事并链接；不强行合并 |
| Persona 质量 | 虚构、刻板印象 | 证据锚点、置信度、用户验证 |
| 文档膨胀 | 功能/故事/操作过多 | 分层、搜索、投影、归档；发布前校验 |
| 并发冲突 | 多 Agent 同时编辑 | 乐观并发、对象级冲突、操作日志、可选租约 |
| 迁移成本 | 旧 Requirement 数据 | 只读导入、逐步切换、兼容视图 |
| 评审负担 | 文档太长 | diff、校验定位、按功能评审 |
| 交互/UI 结构过重 | 结构化文档难以表达视觉细节 | 只存结构、状态和外部引用；视觉稿在 Figma 等工具 |
| 阶段混杂 | 交互/UI 内容过早进入需求分析 | 阶段状态和校验隔离；阶段批准后才进入下一步 |
| 设计引用失效 | 设计系统/组件/原型变更 | 引用可解析校验；失效标记；版本固定 |
| 屏幕覆盖不足 | 故事有流程但无界面 | UI 阶段校验故事→屏幕覆盖 |
| 图片敏感内容 | 截图/稿含客户数据或密钥 | 分类、脱敏、访问控制、保留策略 |
| 图片体积/成本 | 大量高分辨率图片 | 压缩、缩略图、大小限制、对象存储生命周期 |
| SVG 安全 | SVG 可含脚本/外部引用 | 清洗、禁止脚本和外部引用 |
| 图片引用失效 | 资产被删或迁移 | 哈希校验、失效标记、保留策略 |
| alt text 质量 | 图片不可访问 | 必填、评审检查、图片不是唯一信息载体 |
| 外部依赖遗漏 | 未记录外部系统 | 校验和评审检查依赖字段 |
| 批准权限 | Agent 越权 | 策略强制、审计、职责分离 |

**待决问题**：

1. 草案是项目共享一份，还是每个 Agent 一份再合并？当前建议共享一份 + revision。
2. 是否需要批量操作？建议支持，但每个子操作有独立 id。
3. 渲染格式是 Markdown 还是 HTML？建议 Markdown 为默认，HTML/PDF 为导出。
4. 旧 Requirement 聚合是否保留为兼容视图？建议保留只读视图，逐步迁移。
5. 验收场景和 holdout 放在需求文档还是下游测试？建议故事带验收要点，完整场景和 holdout 在下游测试/验证。
6. 用户模型是否需要独立的生命周期（draft/active/retired）？建议需要。
7. 跨功能故事是否允许一个故事引用多个功能的操作？当前建议默认不允许，用链接组合。
8. 三个阶段是共享一个文档版本链，还是每阶段独立发布版本？当前建议阶段版本快照，同一文档。
9. 屏幕是否允许跨功能复用？建议组件复用，屏幕归主功能。
10. 外部设计引用支持哪些类型？建议 Figma、组件库、原型、资源 URL + 哈希。
11. 交互/UI 是否允许 Agent 直接生成原型？建议生成结构和引用，视觉稿由人确认。
12. 图片支持哪些格式和大小限制？建议 PNG/JPEG/WebP/SVG（SVG 清洗），单图上限按项目配置。
13. 缩略图是否由 Foundry 生成？建议异步生成并缓存，原图存对象存储。
14. 外部 Figma 引用和上传图片如何统一？建议都作为 `DesignRef`/`ImageAsset`，文档只存引用。
15. AI 生成图片是否标注？建议记录生成工具/模型版本和人工确认状态。
