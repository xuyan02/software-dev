暂存和提交：

  arche status
  arche status --porcelain
  arche status --porcelain --detail
  arche add <qname>...
  arche add -A
  arche add --file <source-path>
  arche add --directory <source-dir>
  arche unstage <qname>...
  arche unstage --file <source-path>
  arche unstage --directory <source-dir>
  arche diff
  arche diff --staged
  arche reset
  arche reset <qname>...
  arche reset --hard
  arche commit -m "message" [--author <name>]
  arche rm <qname>... [--force] [--recursive]

状态：

  HEAD      上次提交
  index     下次提交
  worktree  当前状态

  status 的 Changes to be committed = HEAD ↔ index
  status 的 Changes not staged      = index ↔ worktree

规则：

  对象级暂存，不做字段级暂存；
  commit 只使用 index；
  空提交需要 --allow-empty；
  checkout 有未提交修改时拒绝，--force 丢弃；
  rm 默认拒绝删除有子对象或仍被引用的对象。
