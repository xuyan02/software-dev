交互 RequirementInteraction

定义：用户对系统的一次操作或观察，或系统的一次通知。它本身可能不产生有意义的
系统变化，例如“填写单据名称”；若干次交互共同构成一次功能。

判断：

- 交互是最小的用户可感知步骤；
- 单次交互可以只改变界面状态或输入内容，不一定改变业务数据；
- 交互必须属于一个功能，若它已经产生完整业务结果，应建为功能。

反例：把“提交并生成单据”只建成交互，它产生了业务数据变化，应升级为功能。

优先级与交互设计：

- core 用户完成关键任务的路径最短：步骤、页面跳转和等待最少；
- 不要为了覆盖低频场景，在 core 用户的主路径上增加步骤；
- 次要用户和边缘用户可以使用次级入口、批量操作或低频页面，不必和 core 用户
  共用同一条长路径。

创建：

  arche create requirement-interaction <name> --parent <function>

修改：

  arche update <qname> --field <字段>=<值>
  arche update <qname> --ref <引用字段>=<目标>[,<目标>...]

`arche update` 只修改传入的字段，create 支持的字段参数同样适用；文档字段的正文
仍用 `arche doc` 命令修改。

字段顺序：

  description
  ui

字段写作说明（统一格式）：

  含义  这个字段回答什么问题。
  写法  内容粒度、顺序、语气和格式要求。
  正例  可以直接对照采用的例子。
  反例  常见错误，出现即应改写。
  约束  与其他字段、对象或检查的关系。

description  DocumentParagraphList
  含义：这一次操作、观察或通知的具体过程。
  写法：用段落写清谁在什么界面、做什么、看到什么、下一步是什么；只写交互级结果，
        不写整个功能的业务结果；异常提示也要写。
  正例：仓管员在新建单据页填写单据名称，输入框实时显示字数，名称为必填项；
        为空时离开输入框会提示“请输入单据名称”。
  反例：提交后生成待审核单据（这是功能的 system_change）；调用表单接口校验。
  约束：若干交互合起来应能产生所属功能的 system_change；交互的结果应与功能的
        flow 节点对应。

ui  DocumentParagraphList
  含义：这个交互涉及的界面结构和内容。
  写法：用普通段落，或用 HTML 片段描述布局、字段、按钮、提示和状态；只写与本次
        交互有关的界面，不重复整个功能或其他交互；写清默认值、必填、校验提示和
        禁用状态。
  正例：
    arche doc html add <interaction-qname> ui \
      "<p>单据名称输入框，必填，右侧显示字数；为空时输入框下方显示红色提示。</p>"
  反例：整页 ERP 的所有按钮和菜单。用截图代替文字说明。只写“有一个输入框”。
  约束：字段和按钮应与 description 中的操作一致；ui 中出现的操作不能超出本次
        交互的范围。

示例：

  arche create requirement-interaction fill-document-name \
    --parent requirement/system/warehouse-android/account-management/create-document
  arche doc import \
    requirement/system/warehouse-android/account-management/create-document/fill-document-name \
    interaction.json
  arche doc html add \
    requirement/system/warehouse-android/account-management/create-document/fill-document-name \
    ui "<p>单据名称输入框，必填，右侧显示字数。</p>"
