远程：

  arche remote add <url>
  arche remote show
  arche clone <url> <dir>
  arche clone <url> <dir> --branch <name>
  arche clone <url> <dir> --source <git-url>   # 覆盖远程记录的源码地址
  clone 自动继承远程仓库记录的 source 地址；一般不需要 --source。
  arche push
  arche push --force
  arche fetch
  arche pull

本地路径：

  arche init --source <git-url>
  arche remote add /path/to/remote.arche

HTTP：

  arche remote add http://host:7700/<repo>
  arche serve --root <dir> --listen <addr> --token <token>

SSH：

  arche remote add ssh://foundry-arche@host/<repo>
  arche serve --stdio --root <repo>

push：

  - 记录先进入 incoming/ quarantine；
  - ref 使用 expected-old CAS；
  - 非 force 要求 fast-forward；
  - 多个 ref 使用 refs.transaction；
  - 受保护分支只能由 admin token 更新。
