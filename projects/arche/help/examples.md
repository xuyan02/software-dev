示例：建立技术设计

  arche init
  arche create technical-system app
  arche doc text set technical/app position "认证与授权服务。"
  arche doc list item add technical/app responsibilities "签发令牌"
  arche create technical-namespace core --parent technical
  arche create technical-class Login --namespace technical/namespace/core
  arche doc text set technical/namespace/core/Login position "登录入口。"
  arche doc list item add technical/namespace/core/Login responsibilities "校验凭据"
  arche create technical-method --parent technical/namespace/core/Login \
    --prototype "login(String, String) -> Token"
  arche add -A
  arche commit -m "initial design"

示例：写文档字段

  arche doc topic add technical/namespace/core/Login usage "登录流程"
  arche doc code add technical/namespace/core/Login usage \
    --language rust "let token = login(user, pass);"
  arche doc mermaid set requirement/function/login flow "graph TD; U-->S;"

示例：概念

  arche concept mkdir concept/domain/identity
  arche concept create concept/domain/identity/agent \
    --name "智能体" \
    --explanation "能够感知环境并采取行动的系统。"
  arche concept list

示例：源码

  arche init --source git@example.com:app.git
  arche source fetch
  arche source sync
  arche source changes
  arche add --file src/main.rs
  arche commit -m "sync source files"
