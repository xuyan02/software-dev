Arche 是版本化的设计仓库。它保存技术、需求和概念设计对象，
以及每个对象上的文档字段。源码只读；设计对象和文档都以内容寻址对象保存。

基本工作流：

  arche init
  arche create ...          创建设计对象
  arche update ...          修改对象字段（参数同 create）
  arche doc ...             写文档字段正文
  arche status              查看 worktree/index/HEAD 差异
  arche add ...             暂存
  arche commit -m "..."     提交 index
  arche push                推送到远程

设计对象不是源码 AST。Arche 不解析源码；TechnicalNamespace、TechnicalClass、
TechnicalMethod 等由使用者根据源码手工创建。source sync 只维护
TechnicalDirectory 和 TechnicalFile。

文档没有独立对象树。文档就是对象字段；show --format json|md 从字段动态生成。

查看帮助主题：

  arche help objects
  arche help fields
  arche help design
  arche help writing
  arche help writing-detail
  arche help kinds
  arche help naming
  arche help technical
  arche help requirement
  arche help requirement-user
  arche help story
  arche help requirement-system
  arche help requirement-module
  arche help requirement-function
  arche help requirement-interaction
  arche help concept
  arche help document
  arche help staging
  arche help source
  arche help remote
  arche help examples
