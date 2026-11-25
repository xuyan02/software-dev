创建技术对象：

  arche create technical-system --name <name>
  arche create technical-module --name <name> --parent <system>
  arche create technical-layer --name <name> --parent <system|module>
  arche create technical-namespace --name <name> --parent technical
  arche create technical-class --name <name> --namespace <namespace>
  arche create technical-struct --name <name> --namespace <namespace>
  arche create technical-enum --name <name> --namespace <namespace>
  arche create technical-interface --name <name> --namespace <namespace>
  arche create technical-mixin --name <name> --namespace <namespace>
  arche create technical-function --name <name> --namespace <namespace>
  arche create technical-method --parent <type> --prototype "<name>(<types>) -> <return>"
  arche create technical-part --name <name> --parent <type> --source <path:start-end>

修改技术对象：

  arche update <qname> [与 create 相同的字段参数]
  arche update <qname> --namespace <namespace> --file <file> --module <module>
  arche update <qname> --source <path:start-end>
  arche update <qname> --field <字段>=<值> --ref <引用字段>=<目标>[,<目标>...]

`--parent` 和 `--prototype` 是对象身份的一部分，不能通过 update 修改，需要按新值
重新创建对象。文档字段的正文仍用 `arche doc` 命令修改。

常用文档写入：

  arche doc text set <qname> position "一句话定位"
  arche doc list item add <qname> responsibilities "职责"
  arche doc list item add <qname> features "特性"
  arche doc topic add <qname> usage "使用场景"
  arche doc text set <qname> design_points "设计要点"   # 仅 DocumentText 字段
  arche doc code add <qname> usage --language rust "fn main() {}"
  arche doc mermaid set <qname> flow "graph TD; A-->B;"
  arche doc html add <qname> usage "<b>预览</b>"

注意：usage/design_points 通常是 DocumentParagraphList；用 topic/code/html
追加块，不要用 doc text set。
