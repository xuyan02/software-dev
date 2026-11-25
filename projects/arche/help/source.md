源码：

  arche source fetch
  arche source checkout <git-ref>
  arche source worktree
  arche source status
  arche source ls
  arche source cat <path>
  arche source set-url <git-url>
  arche source sync
  arche source changes
  arche source changes --base <git-ref>

Arche 不解析源码 AST。

  source sync 只维护：
    TechnicalDirectory
    TechnicalFile

  其他技术对象由使用者创建：
    technical-namespace
    technical-class / technical-struct / technical-enum /
    technical-interface / technical-mixin / technical-function
    technical-method
    technical-part

  add --file 会按以下关系级联暂存：
    TechnicalFile
    TechnicalClass / TechnicalStruct / TechnicalEnum / TechnicalInterface /
    TechnicalFunction / TechnicalMixin 的 file 字段
    对象的 source 路径
    parent 链
    ObjectList / DocumentList / DocumentParagraphList
    文档字段对象
