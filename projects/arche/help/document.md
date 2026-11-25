文档没有独立对象树；文档就是对象字段。

字段类型：

  DocumentText              单段文本
  DocumentList              DocumentText 列表
  DocumentParagraphList     段落块列表
  DocumentTopic             标题 + 正文 ParagraphList
  DocumentCode              language + source
  DocumentMermaid           Mermaid 源文本
  DocumentHtml              HTML/SVG 源文本

命令：

  arche doc text set <qname> <field> <text>
  arche doc list add <qname> <field>
  arche doc list item add <qname> <field> <text>
  arche doc topic add <qname> <field> <title>
  arche doc code add <qname> <field> --language <lang> <source>
  arche doc mermaid set <qname> <field> <source>
  arche doc html add <qname> <field> <source>
  arche doc import <qname> <file|->
  arche doc grep <text>
  arche doc lint
  arche doc layout <qname>
  arche doc check

标量字段：

  arche update <qname> --field <field>=<value>

  String、bool 和整数属性（例如 display_name、age、priority）不是文档字段，用
  `arche update --field` 修改；`doc import` 和 `batch` 的 fields 也可以直接写这些
  标量字段，值不需要 type 包装。

  `priority` 使用级别名：core、primary、secondary、peripheral；例如
  `--field priority=core`。

导入格式：

  {
    "fields": {
      "position": {
        "type": "DocumentText",
        "content": [{"type": "text", "text": "认证服务。"}]
      },
      "usage": {
        "type": "DocumentParagraphList",
        "items": [
          {"type": "topic", "title": "状态", "body": {"items": []}},
          {"type": "code", "language": "rust", "source": "fn main() {}"},
          {"type": "mermaid", "source": "graph TD; A-->B;"}
        ]
      }
    }
  }

批量：

  arche batch plan.json

  {
    "objects": [
      {"kind": "technical-module", "name": "core", "parent": "technical/app"}
    ],
    "documents": [
      {"qname": "technical/app/core", "fields": {"position": {"type": "DocumentText", "content": [{"type": "text", "text": "核心模块"}]}}}
    ]
  }

查看：

  arche show <qname> --format json
  arche show <qname> --format md
