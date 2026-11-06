//! Server-rendered HTML console.
//!
//! The console is a thin, dependency-light view over the same API agents use.
//! It intentionally has no JavaScript build step: HTML + a small stylesheet is
//! enough for humans to observe and act.

use foundry_domain::{
    Account, AccountKind, AccountRole, AccountStatus, Issue, IssueKind, IssueSeverity,
    IssueSourceKind, IssueStatus, Project, ProjectRole, ProjectStatus, Requirement,
    RequirementStatus, WorkflowDefinition, WorkflowOption, WorkflowOwner,
};

const STYLE: &str = r#"
:root { color-scheme: light dark; --fg:#1b1f23; --muted:#57606a; --bg:#ffffff;
        --panel:#f6f8fa; --line:#d0d7de; --accent:#0969da; --accent-soft:rgba(9,105,218,.09); }
@media (prefers-color-scheme: dark) {
  :root { --fg:#e6edf3; --muted:#8b949e; --bg:#0d1117; --panel:#161b22;
          --line:#30363d; --accent:#4493f8; --accent-soft:rgba(68,147,248,.13); }
}
* { box-sizing: border-box; }
body { margin:0; font: 15px/1.55 ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
       color:var(--fg); background:var(--bg); }
.app { display:grid; grid-template-columns:232px minmax(0,1fr); min-height:100vh; }
.sidebar { display:flex; flex-direction:column; gap:18px; padding:22px 14px;
           border-right:1px solid var(--line); background:var(--panel);
           position:sticky; top:0; height:100vh; overflow-y:auto; }
.brand { font-size:18px; font-weight:700; color:var(--fg); text-decoration:none; padding:0 10px; }
.brand-sub { color:var(--muted); font-size:12px; margin:-14px 0 0; padding:0 10px; }
.nav-group { display:flex; flex-direction:column; gap:2px; }
.nav-heading { color:var(--muted); font-size:11px; letter-spacing:.08em; text-transform:uppercase;
               padding:8px 10px 4px; margin:0; }
.nav-item { display:flex; align-items:center; gap:8px; padding:7px 10px; border-radius:6px;
            color:var(--fg); text-decoration:none; font-size:14px; }
a.nav-item:hover { background:var(--bg); }
.nav-item.active { background:var(--accent-soft); color:var(--accent); font-weight:600; }
.nav-item.disabled { color:var(--muted); cursor:default; }
.nav-tag { margin-left:auto; border:1px solid var(--line); border-radius:999px;
           padding:0 6px; font-size:10px; font-style:normal; color:var(--muted); }
.nav-more { margin-top:4px; }
.nav-more summary { list-style:none; cursor:pointer; color:var(--muted); font-size:13px;
                    padding:7px 10px; border-radius:6px; }
.nav-more summary::-webkit-details-marker { display:none; }
.nav-more summary::before { content:"▸ "; }
.nav-more[open] summary::before { content:"▾ "; }
.nav-more summary:hover { background:var(--bg); }
.nav-more .nav-item { padding-left:22px; }
.sidebar-actions { margin-top:auto; display:flex; flex-direction:column; gap:8px; padding:0 10px; }
.sidebar-actions .button { width:100%; text-align:center; }
.project-switch { margin:0; padding:0 10px; display:flex; flex-direction:column; gap:6px; }
.project-switch select { width:100%; font:inherit; color:var(--fg); background:var(--bg);
                         border:1px solid var(--line); border-radius:6px; padding:6px 8px; }
.project-switch button { width:100%; }
.sidebar-user { border-top:1px solid var(--line); padding:12px 10px 0; display:flex;
                flex-direction:column; gap:6px; font-size:12px; color:var(--muted); }
.sidebar-user strong { display:block; color:var(--fg); font-size:13px; }
.sidebar-user form { margin:0; }
.sidebar-user button.link { background:none; border:0; color:var(--accent); padding:0;
                            font-size:12px; cursor:pointer; text-align:left; }
main { width:100%; max-width:980px; margin:0 auto; padding:32px 32px 72px; }
h1 { font-size: 24px; margin: 0 0 4px; }
h2 { font-size: 17px; margin-top: 28px; }
.sub { color:var(--muted); margin: 0 0 24px; }
table { width:100%; border-collapse:collapse; background:var(--panel);
        border:1px solid var(--line); border-radius:8px; overflow:hidden; }
th, td { text-align:left; padding:10px 12px; border-bottom:1px solid var(--line); }
tr:last-child td { border-bottom:0; }
a { color: var(--accent); }
.badge { display:inline-block; padding:1px 8px; border-radius:999px; font-size:12px;
         border:1px solid var(--line); background:var(--bg); }
.meta { display:flex; flex-wrap:wrap; gap:10px 22px; color:var(--muted); font-size:13px; }
.card { background:var(--panel); border:1px solid var(--line); border-radius:8px;
        padding:16px 18px; margin:12px 0; }
code { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size:13px; }
.empty { color:var(--muted); padding:24px; text-align:center; }
ul { padding-left: 20px; }
.field { display:flex; flex-direction:column; gap:6px; margin:14px 0; }
.field label { font-size:13px; color:var(--muted); }
.field input, .field textarea, .field select {
  font: inherit; color:var(--fg); background:var(--panel);
  border:1px solid var(--line); border-radius:6px; padding:8px 10px;
}
.field textarea { min-height:84px; resize:vertical; }
fieldset { border:1px solid var(--line); border-radius:8px; margin:18px 0; padding:6px 16px 16px; }
legend { padding:0 6px; color:var(--muted); font-size:13px; }
.grid { display:grid; grid-template-columns:1fr 1fr; gap:0 18px; }
.error { border:1px solid #cf222e; color:#cf222e; background:rgba(207,34,46,.08);
         border-radius:8px; padding:10px 12px; margin:16px 0; }
.actions { margin-top:22px; }
button, .button { font:inherit; background:var(--accent); color:#fff; border:0; border-radius:6px;
                  padding:8px 16px; cursor:pointer; text-decoration:none; display:inline-block; }
.button.secondary, button.secondary { background:transparent; color:var(--accent); border:1px solid var(--line); }
button.danger { background:transparent; color:#cf222e; border:1px solid #cf222e; padding:4px 10px; }
.hint { color:var(--muted); font-size:12px; margin:4px 0 0; }
.stats { display:grid; grid-template-columns:repeat(3, minmax(0,1fr)); gap:12px; margin:20px 0 8px; }
.stat { display:flex; flex-direction:column; gap:2px; padding:16px 18px; border:1px solid var(--line);
        border-radius:8px; background:var(--panel); color:var(--fg); text-decoration:none; }
a.stat:hover { border-color:var(--accent); }
.stat-label { color:var(--muted); font-size:12px; }
.stat-value { font-size:28px; font-weight:650; line-height:1.2; }
.stat-value small { font-size:12px; font-weight:400; color:var(--muted); }
.stat-foot { color:var(--muted); font-size:12px; }
.stat.disabled { opacity:.62; }
.flow { display:flex; flex-wrap:wrap; align-items:center; gap:8px; margin:12px 0; }
.flow-step { border:1px solid var(--line); border-radius:999px; padding:4px 12px; background:var(--panel); }
.flow-step.available { border-color:var(--accent); color:var(--accent); background:var(--accent-soft); }
.flow-arrow { color:var(--muted); }
.two-col { display:grid; grid-template-columns:1fr 1fr; gap:0 22px; }
.list { background:var(--panel); border:1px solid var(--line); border-radius:8px; overflow:hidden; }
.list-item { display:flex; align-items:center; justify-content:space-between; gap:12px;
             padding:10px 14px; border-bottom:1px solid var(--line); }
.list-item:last-child { border-bottom:0; }
.list-item a { color:var(--fg); text-decoration:none; }
.list-item a:hover { color:var(--accent); }
.list-item .badge { flex:none; }
.inline-form { display:flex; align-items:center; gap:8px; margin:0; flex-wrap:wrap; }
.inline-form select, .inline-form input { font:inherit; color:var(--fg); background:var(--bg);
  border:1px solid var(--line); border-radius:6px; padding:5px 8px; }
.inline-form button { padding:5px 10px; font-size:13px; }
.login { max-width:420px; margin:8vh auto; padding:0 20px; }
.login .card { padding:24px; }
@media (max-width: 800px) {
  .app { display:block; }
  .sidebar { position:static; height:auto; border-right:0; border-bottom:1px solid var(--line);
             padding:14px 16px; gap:12px; }
  .brand-sub { display:none; }
  .sidebar nav { display:flex; flex-wrap:wrap; align-items:center; gap:4px; }
  .nav-group { flex-direction:row; flex-wrap:wrap; align-items:center; }
  .nav-heading { padding:0 4px; }
  .nav-more { margin:0; }
  .nav-more .nav-item { padding-left:10px; }
  .sidebar-actions { margin-top:0; flex-direction:row; padding:0; }
  .sidebar-actions .button { width:auto; }
  .project-switch { padding:0; }
  .sidebar-user { border-top:0; padding:0; }
  main { padding:24px 16px 64px; }
  .two-col, .stats { grid-template-columns:1fr; }
  .grid { grid-template-columns:1fr; }
}
@media (max-width: 640px) { .grid { grid-template-columns:1fr; } }
"#;

fn escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[derive(Clone, Debug)]
pub struct ViewerInfo {
    pub handle: String,
    pub display_name: String,
    pub kind: AccountKind,
    pub role: AccountRole,
}

impl ViewerInfo {
    pub fn from_account(account: &Account) -> Self {
        Self {
            handle: account.handle.clone(),
            display_name: account.display_name.clone(),
            kind: account.kind,
            role: account.role,
        }
    }

    pub fn is_admin(&self) -> bool {
        self.role.is_admin()
    }
}

#[derive(Clone, Debug)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub status: ProjectStatus,
    pub role: Option<ProjectRole>,
}

impl ProjectInfo {
    pub fn from_project(project: &Project, account: &Account) -> Self {
        Self {
            id: project.id.to_string(),
            name: project.name.clone(),
            status: project.status,
            role: if account.is_admin() {
                project.role_of(account.id).or(Some(ProjectRole::Owner))
            } else {
                project.role_of(account.id)
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConsoleContext {
    pub viewer: ViewerInfo,
    pub projects: Vec<ProjectInfo>,
    pub current: Option<ProjectInfo>,
}

impl ConsoleContext {
    pub fn new(account: &Account, projects: &[Project], current: Option<&Project>) -> Self {
        let items: Vec<ProjectInfo> = projects
            .iter()
            .map(|project| ProjectInfo::from_project(project, account))
            .collect();
        let current = current
            .and_then(|project| {
                items
                    .iter()
                    .find(|item| item.id == project.id.to_string())
                    .cloned()
            })
            .or_else(|| items.first().cloned());
        Self {
            viewer: ViewerInfo::from_account(account),
            projects: items,
            current,
        }
    }

    pub fn can_contribute(&self) -> bool {
        self.viewer.is_admin()
            || self
                .current
                .as_ref()
                .and_then(|project| project.role)
                .is_some_and(ProjectRole::can_contribute)
    }

    pub fn can_dispose(&self) -> bool {
        self.viewer.is_admin()
            || self
                .current
                .as_ref()
                .and_then(|project| project.role)
                .is_some_and(ProjectRole::can_dispose)
    }

    pub fn can_manage_members(&self) -> bool {
        self.viewer.is_admin()
            || self
                .current
                .as_ref()
                .and_then(|project| project.role)
                .is_some_and(ProjectRole::can_manage_members)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Workbench,
    Issues,
    Requirements,
    Projects,
    Workflows,
    Accounts,
    None,
}

fn nav_item(active: Section, section: Section, href: &str, label: &str) -> String {
    let current = if active == section {
        r#" class="nav-item active" aria-current="page""#
    } else {
        r#" class="nav-item""#
    };
    format!(r#"<a{current} href="{href}">{label}</a>"#)
}

fn bare_layout(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} · Foundry</title>
<style>{STYLE}</style>
</head>
<body>{body}</body>
</html>"#,
        title = escape(title),
        body = body,
    )
}

fn layout(ctx: &ConsoleContext, active: Section, title: &str, body: &str) -> String {
    let project_switch = if ctx.projects.is_empty() {
        String::new()
    } else {
        let options = ctx
            .projects
            .iter()
            .map(|project| {
                let selected = ctx
                    .current
                    .as_ref()
                    .is_some_and(|current| current.id == project.id);
                format!(
                    r#"<option value="{id}"{selected}>{name}</option>"#,
                    id = escape(&project.id),
                    selected = if selected { " selected" } else { "" },
                    name = escape(&project.name),
                )
            })
            .collect::<String>();
        format!(
            r#"<form class="project-switch" method="post" action="/projects/select">
<label class="nav-heading" for="project-switch">当前项目</label>
<select id="project-switch" name="project">{options}</select>
<button class="secondary" type="submit">切换项目</button>
</form>"#
        )
    };

    let nav = format!(
        r#"<a class="brand" href="/">Foundry</a>
<p class="brand-sub">AI 原生软件工程控制平面</p>
{project_switch}
<nav aria-label="主导航">
  <div class="nav-group">
    {}
  </div>
  <div class="nav-group">
    <p class="nav-heading">问题空间</p>
    {}
  </div>
  <div class="nav-group">
    <p class="nav-heading">承诺空间</p>
    {}
  </div>
  <div class="nav-group">
    <p class="nav-heading">流程</p>
    {}
  </div>
  <div class="nav-group">
    <p class="nav-heading">组织</p>
    {}
    {}
  </div>
  <details class="nav-more">
    <summary>后续模块</summary>
    <div class="nav-group">
      <span class="nav-item disabled" aria-disabled="true">任务 <em class="nav-tag">待接入</em></span>
      <span class="nav-item disabled" aria-disabled="true">设计文档 <em class="nav-tag">待接入</em></span>
      <span class="nav-item disabled" aria-disabled="true">ADR <em class="nav-tag">待接入</em></span>
      <span class="nav-item disabled" aria-disabled="true">决策 <em class="nav-tag">待接入</em></span>
      <span class="nav-item disabled" aria-disabled="true">发布 <em class="nav-tag">待接入</em></span>
      <span class="nav-item disabled" aria-disabled="true">事件日志 <em class="nav-tag">待接入</em></span>
    </div>
  </details>
</nav>
<div class="sidebar-actions">
  {create_issue}
  {create_requirement}
</div>
<div class="sidebar-user">
  <strong>{display_name}</strong>
  <span>@{handle} · {kind}{admin}</span>
  <form method="post" action="/logout"><button class="link" type="submit">退出登录</button></form>
</div>"#,
        nav_item(active, Section::Workbench, "/", "工作台"),
        nav_item(active, Section::Issues, "/issues", "议题"),
        nav_item(active, Section::Requirements, "/requirements", "需求"),
        nav_item(active, Section::Workflows, "/workflows", "流程"),
        nav_item(active, Section::Projects, "/projects", "项目"),
        nav_item(active, Section::Accounts, "/accounts", "账户"),
        create_issue = if ctx.can_contribute() {
            r#"<a class="button" href="/issues/new">＋ 新建议题</a>"#.to_string()
        } else {
            String::new()
        },
        create_requirement = if ctx.can_contribute() {
            r#"<a class="button secondary" href="/requirements/new">＋ 新建需求</a>"#
        } else {
            ""
        },
        display_name = escape(&ctx.viewer.display_name),
        handle = escape(&ctx.viewer.handle),
        kind = if ctx.viewer.kind == AccountKind::Human {
            "人类"
        } else {
            "智能体"
        },
        admin = if ctx.viewer.is_admin() {
            " · Admin"
        } else {
            ""
        },
    );

    format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} · Foundry</title>
<style>{STYLE}</style>
</head>
<body>
<div class="app">
<aside class="sidebar">{nav}</aside>
<main>{body}</main>
</div>
</body>
</html>"#,
        title = escape(title),
        body = body,
    )
}

pub fn login_page(handle: &str, next: &str, error: Option<&str>) -> String {
    let error_html = error
        .map(|message| format!(r#"<div class="error">{}</div>"#, escape(message)))
        .unwrap_or_default();
    let body = format!(
        r#"<div class="login">
<h1>Foundry</h1>
<p class="sub">登录 AI 原生软件工程控制平面。</p>
{error_html}
<form method="post" action="/login">
  <input type="hidden" name="next" value="{next}">
  <div class="field"><label for="handle">账号</label>
    <input id="handle" name="handle" value="{handle}" autocomplete="username" required autofocus></div>
  <div class="field"><label for="password">密码</label>
    <input id="password" name="password" type="password" autocomplete="current-password" required></div>
  <div class="actions"><button type="submit">登录</button></div>
  <p class="hint">账号由管理员创建。若系统尚未初始化，请打开 <a href="/setup">/setup</a> 创建首个管理员。</p>
</form>
</div>"#,
        error_html = error_html,
        handle = escape(handle),
        next = escape(next),
    );
    bare_layout("登录", &body)
}

/// Step one of first-run setup: sign in with the initial account configured in
/// the config file. This is not a real account, so success only unlocks the
/// administrator creation form.
pub fn setup_login_page(handle: &str, error: Option<&str>) -> String {
    let error_html = error
        .map(|message| format!(r#"<div class="error">{}</div>"#, escape(message)))
        .unwrap_or_default();
    let body = format!(
        r#"<div class="login">
<h1>Foundry</h1>
<p class="sub">首次启动：使用初始账号登录。</p>
<p class="hint">初始账号和密码来自配置文件（bootstrap_handle / bootstrap_password）。它们只用于确认你是配置提供者，不是系统账户。验证通过后，你需要创建一个新的管理员账户。该页面只在账户库为空时出现。</p>
{error_html}
<form method="post" action="/setup">
  <div class="field"><label for="handle">初始账号</label>
    <input id="handle" name="handle" value="{handle}" autocomplete="username" required autofocus></div>
  <div class="field"><label for="password">初始密码</label>
    <input id="password" name="password" type="password" autocomplete="current-password" required></div>
  <div class="actions"><button type="submit">登录初始化</button></div>
</form>
</div>"#,
        error_html = error_html,
        handle = escape(handle),
    );
    bare_layout("初始化登录", &body)
}

/// Step two of first-run setup: after the initial identity is verified, the
/// operator creates the real administrator account and its password.
pub fn setup_admin_page(handle: &str, display_name: &str, error: Option<&str>) -> String {
    let error_html = error
        .map(|message| format!(r#"<div class="error">{}</div>"#, escape(message)))
        .unwrap_or_default();
    let body = format!(
        r#"<div class="login">
<h1>创建管理员</h1>
<p class="sub">初始化身份已验证。现在创建真实的管理员账户。</p>
<p class="hint">这个账户将成为系统的第一个管理员。创建后进入项目页，由你显式创建第一个项目。初始账号和密码只用于本次授权，创建完成后不再可用。</p>
{error_html}
<form method="post" action="/setup/admin">
  <div class="field"><label for="handle">管理员账号</label>
    <input id="handle" name="handle" value="{handle}" autocomplete="off" required autofocus></div>
  <div class="field"><label for="display_name">显示名称</label>
    <input id="display_name" name="display_name" value="{display_name}" autocomplete="name" required></div>
  <div class="field"><label for="password">管理员密码</label>
    <input id="password" name="password" type="password" autocomplete="new-password" required></div>
  <div class="field"><label for="confirm_password">确认管理员密码</label>
    <input id="confirm_password" name="confirm_password" type="password" autocomplete="new-password" required></div>
  <div class="actions"><button type="submit">创建管理员</button></div>
</form>
</div>"#,
        error_html = error_html,
        handle = escape(handle),
        display_name = escape(display_name),
    );
    bare_layout("创建管理员", &body)
}

pub fn workbench(ctx: &ConsoleContext, issues: &[Issue], requirements: &[Requirement]) -> String {
    let fresh = issues
        .iter()
        .filter(|issue| issue.status == IssueStatus::Open)
        .count();
    let active = issues
        .iter()
        .filter(|issue| matches!(issue.status, IssueStatus::Triaged | IssueStatus::InProgress))
        .count();
    let resolved = issues
        .iter()
        .filter(|issue| issue.status == IssueStatus::Resolved)
        .count();

    let issue_items = if issues.is_empty() {
        r#"<div class="empty">还没有议题。</div>"#.to_string()
    } else {
        issues
            .iter()
            .take(6)
            .map(|issue| {
                format!(
                    r#"<div class="list-item"><a href="/issues/{id}">{title}</a><span class="badge">{status}</span></div>"#,
                    id = issue.id,
                    title = escape(&issue.title),
                    status = issue_status_label(issue.status),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let requirement_items = if requirements.is_empty() {
        r#"<div class="empty">还没有需求。</div>"#.to_string()
    } else {
        requirements
            .iter()
            .take(6)
            .map(|requirement| {
                format!(
                    r#"<div class="list-item"><a href="/requirements/{id}">{title}</a><span class="badge">{status}</span></div>"#,
                    id = requirement.id,
                    title = escape(&requirement.title),
                    status = status_label(requirement.status),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let project_line = ctx
        .current
        .as_ref()
        .map(|project| {
            format!(
                r#"<p class="sub">当前项目：<strong>{}</strong></p>"#,
                escape(&project.name)
            )
        })
        .unwrap_or_else(|| {
            r#"<p class="sub">你还没有加入任何项目。请先创建一个项目。</p>"#.to_string()
        });

    let body = format!(
        r#"<h1>工作台</h1>
{project_line}
<p class="sub">Foundry 把软件工程变成一条可审计的流程：先收集问题，再形成承诺，最后进入设计与执行。</p>

<div class="stats">
  <a class="stat" href="/issues">
    <span class="stat-label">议题</span>
    <span class="stat-value">{fresh}<small> 待分诊</small></span>
    <span class="stat-foot">{active} 处理中 · {resolved} 待确认</span>
  </a>
  <a class="stat" href="/requirements">
    <span class="stat-label">需求（早期实现）</span>
    <span class="stat-value">{requirement_count}</span>
    <span class="stat-foot">承诺空间</span>
  </a>
  <div class="stat disabled">
    <span class="stat-label">任务</span>
    <span class="stat-value">—</span>
    <span class="stat-foot">执行面尚未接入</span>
  </div>
</div>

<h2>流程</h2>
<div class="flow">
  <span class="flow-step available">议题</span>
  <span class="flow-arrow">→</span>
  <span class="flow-step available">分诊</span>
  <span class="flow-arrow">→</span>
  <span class="flow-step available">需求</span>
  <span class="flow-arrow">→</span>
  <span class="flow-step">设计</span>
  <span class="flow-arrow">→</span>
  <span class="flow-step">任务</span>
  <span class="flow-arrow">→</span>
  <span class="flow-step">验证</span>
  <span class="flow-arrow">→</span>
  <span class="flow-step">发布</span>
</div>
<p class="hint">当前已接入议题全流程和需求早期流程；任务、设计文档、决策和发布尚未接入。</p>

<div class="two-col">
  <section>
    <h2>最近议题</h2>
    <div class="list">{issue_items}</div>
    <p><a href="/issues">查看全部议题 →</a></p>
  </section>
  <section>
    <h2>最近需求</h2>
    <div class="list">{requirement_items}</div>
    <p><a href="/requirements">查看全部需求 →</a></p>
  </section>
</div>"#,
        requirement_count = requirements.len(),
    );
    layout(ctx, Section::Workbench, "工作台", &body)
}

pub fn dashboard(ctx: &ConsoleContext, requirements: &[Requirement]) -> String {
    let rows = if requirements.is_empty() {
        r#"<tr><td colspan="5" class="empty">还没有需求。用 API 或 CLI 创建第一条。</td></tr>"#
            .to_string()
    } else {
        requirements
            .iter()
            .map(|requirement| {
                format!(
                    r#"<tr>
<td><a href="/requirements/{id}">{title}</a></td>
<td><span class="badge">{status}</span></td>
<td>{priority:?}</td>
<td>{owner}</td>
<td>{updated}</td>
</tr>"#,
                    id = requirement.id,
                    title = escape(&requirement.title),
                    status = status_label(requirement.status),
                    priority = requirement.priority,
                    owner = escape(requirement.owner.id()),
                    updated = requirement
                        .updated_at
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let body = format!(
        r#"<h1>需求</h1>
<p class="sub">需求是承诺空间：我们决定解决什么、不解决什么、怎样算解决。原始反馈先进入议题。</p>
{create}
<table>
<thead><tr><th>标题</th><th>状态</th><th>优先级</th><th>负责人</th><th>更新时间</th></tr></thead>
<tbody>{rows}</tbody>
</table>"#,
        create = if ctx.can_contribute() {
            r#"<p><a class="button" href="/requirements/new">＋ 新建需求</a></p>"#
        } else {
            ""
        },
    );
    layout(ctx, Section::Requirements, "需求", &body)
}

pub fn requirement_page(ctx: &ConsoleContext, requirement: &Requirement) -> String {
    let acceptance = if requirement.acceptance.is_empty() {
        "<li>未定义</li>".to_string()
    } else {
        requirement
            .acceptance
            .iter()
            .map(|scenario| {
                format!(
                    "<li><strong>{}</strong><br><code>given</code> {} → <code>when</code> {} → <code>then</code> {}</li>",
                    escape(&scenario.title),
                    escape(&scenario.given.join("; ")),
                    escape(&scenario.when.join("; ")),
                    escape(&scenario.then.join("; ")),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let history = if requirement.history.is_empty() {
        "<li>无状态变更</li>".to_string()
    } else {
        requirement
            .history
            .iter()
            .map(|change| {
                format!(
                    "<li>{:?} → {:?} · {} · {}</li>",
                    change.from,
                    change.to,
                    escape(change.actor.id()),
                    change
                        .at
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let body = format!(
        r#"<p><a href="/requirements">← 返回需求列表</a></p>
<h1>{title}</h1>
<p class="sub"><span class="badge">{status}</span> · 版本 {version}</p>
<div class="meta">
  <span>优先级 <code>{priority:?}</code></span>
  <span>风险 <code>{risk:?}</code></span>
  <span>负责人 <code>{owner}</code></span>
  <span>项目 ID <code>{project_id}</code></span>
  <span>ID <code>{id}</code></span>
</div>
<h2>问题</h2><div class="card">{problem}</div>
<h2>用户</h2><div class="card">{users}</div>
<h2>验收场景</h2><ul>{acceptance}</ul>
<h2>非目标</h2><div class="card">{non_goals}</div>
<h2>约束</h2><div class="card">{constraints}</div>
<h2>状态历史</h2><ul>{history}</ul>"#,
        title = escape(&requirement.title),
        status = status_label(requirement.status),
        version = requirement.version,
        priority = requirement.priority,
        risk = requirement.risk,
        owner = escape(requirement.owner.id()),
        project_id = requirement.project_id,
        id = requirement.id,
        problem = escape(&requirement.problem),
        users = if requirement.users.is_empty() {
            "未定义".to_string()
        } else {
            escape(&requirement.users.join("、"))
        },
        acceptance = acceptance,
        non_goals = if requirement.non_goals.is_empty() {
            "未定义".to_string()
        } else {
            escape(&requirement.non_goals.join("、"))
        },
        constraints = if requirement.constraints.is_empty() {
            "未定义".to_string()
        } else {
            escape(&requirement.constraints.join("、"))
        },
        history = history,
    );
    layout(ctx, Section::Requirements, &requirement.title, &body)
}

/// Values retained when the creation form is re-rendered after an error.
#[derive(Debug, Clone)]
pub struct RequirementFormValues {
    pub title: String,
    pub problem: String,
    pub users: String,
    pub scenario_title: String,
    pub given: String,
    pub when: String,
    pub then: String,
    pub non_goals: String,
    pub constraints: String,
    pub priority: String,
    pub risk: String,
}

impl Default for RequirementFormValues {
    fn default() -> Self {
        Self {
            title: String::new(),
            problem: String::new(),
            users: String::new(),
            scenario_title: String::new(),
            given: String::new(),
            when: String::new(),
            then: String::new(),
            non_goals: String::new(),
            constraints: String::new(),
            priority: "p3".to_string(),
            risk: "medium".to_string(),
        }
    }
}

pub fn new_requirement_page(
    ctx: &ConsoleContext,
    values: &RequirementFormValues,
    error: Option<&str>,
) -> String {
    let error_html = error
        .map(|message| format!(r#"<div class="error">{}</div>"#, escape(message)))
        .unwrap_or_default();

    let body = format!(
        r#"<p><a href="/requirements">← 返回需求列表</a></p>
<h1>新建需求</h1>
<p class="sub">需求是契约：说清问题、用户、验收场景，以及明确不做什么。</p>
{error_html}
<form method="post" action="/requirements">
  {title}
  {problem}
  <div class="grid">
    {priority}
    {risk}
  </div>
  {users}
  {non_goals}
  {constraints}
  <fieldset>
    <legend>验收场景（至少一条）</legend>
    {scenario_title}
    {given}
    {when}
    {then}
    <p class="hint">given / when / then 每行一条。场景编号自动生成为 S-1。</p>
  </fieldset>
  <p class="hint">将以 {viewer} 的身份记录创建者。</p>
  <div class="actions"><button type="submit">创建需求</button></div>
</form>"#,
        title = input_field("title", "标题 *", &values.title, true),
        problem = textarea_field(
            "problem",
            "问题 *",
            &values.problem,
            Some("描述当前的问题，而不是预设的解决方案。"),
        ),
        priority = select_field(
            "priority",
            "优先级",
            &values.priority,
            &[
                ("p0", "P0 · 最高"),
                ("p1", "P1 · 高"),
                ("p2", "P2 · 中"),
                ("p3", "P3 · 低")
            ],
        ),
        risk = select_field(
            "risk",
            "风险",
            &values.risk,
            &[
                ("low", "低"),
                ("medium", "中"),
                ("high", "高"),
                ("critical", "严重")
            ],
        ),
        users = textarea_field(
            "users",
            "用户 / 角色",
            &values.users,
            Some("每行一个。批准前必须填写。"),
        ),
        non_goals = textarea_field(
            "non_goals",
            "非目标",
            &values.non_goals,
            Some("每行一个。明确不做什么，批准前必须填写。"),
        ),
        constraints = textarea_field(
            "constraints",
            "约束",
            &values.constraints,
            Some("每行一个：兼容性、性能、合规等硬约束。"),
        ),
        scenario_title = input_field("scenario_title", "场景标题", &values.scenario_title, false,),
        given = textarea_field("given", "Given（前置条件）", &values.given, None),
        when = textarea_field("when", "When（动作）", &values.when, None),
        then = textarea_field("then", "Then（预期结果）", &values.then, None),
        viewer = escape(&ctx.viewer.display_name),
    );
    layout(ctx, Section::Requirements, "新建需求", &body)
}

pub fn issue_dashboard(ctx: &ConsoleContext, issues: &[Issue]) -> String {
    let fresh = issues
        .iter()
        .filter(|issue| issue.status == IssueStatus::Open)
        .count();
    let active = issues
        .iter()
        .filter(|issue| matches!(issue.status, IssueStatus::Triaged | IssueStatus::InProgress))
        .count();
    let resolved = issues
        .iter()
        .filter(|issue| issue.status == IssueStatus::Resolved)
        .count();

    let rows = if issues.is_empty() {
        r#"<tr><td colspan="7" class="empty">还没有议题。客户反馈、缺陷、问题和想法都从这里进入。</td></tr>"#
            .to_string()
    } else {
        issues
            .iter()
            .map(|issue| {
                format!(
                    r#"<tr>
<td><a href="/issues/{id}">{title}</a></td>
<td><span class="badge">{kind}</span></td>
<td><span class="badge">{status}</span></td>
<td>{severity}</td>
<td>{assignee}</td>
<td>{source}</td>
<td>{updated}</td>
</tr>"#,
                    id = issue.id,
                    title = escape(&issue.title),
                    kind = issue_kind_label(issue.kind),
                    status = issue_status_label(issue.status),
                    severity = issue.severity.map(issue_severity_label).unwrap_or("—"),
                    assignee = issue
                        .assignee
                        .as_ref()
                        .map(|actor| escape(actor.id()))
                        .unwrap_or_else(|| "未分配".to_string()),
                    source = issue_source_label(issue.source.kind),
                    updated = issue
                        .updated_at
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let body = format!(
        r#"<h1>议题</h1>
<p class="sub">问题空间：世界上发生了什么、谁想要什么、哪里需要讨论。议题不是需求，只有明确的处置才会变成工作。</p>
<div class="meta">
  <span>待分诊 <strong>{fresh}</strong></span>
  <span>处理中 <strong>{active}</strong></span>
  <span>待确认 <strong>{resolved}</strong></span>
</div>
{create}
<table>
<thead><tr><th>标题</th><th>类型</th><th>状态</th><th>严重度</th><th>负责人</th><th>来源</th><th>更新时间</th></tr></thead>
<tbody>{rows}</tbody>
</table>"#,
        create = if ctx.can_contribute() {
            r#"<p><a class="button" href="/issues/new">＋ 新建议题</a></p>"#
        } else {
            ""
        },
    );
    layout(ctx, Section::Issues, "议题", &body)
}

pub fn issue_page(ctx: &ConsoleContext, issue: &Issue) -> String {
    let bug_details = issue_bug_details(issue);
    let resolution = issue
        .resolution
        .as_ref()
        .map(|text| {
            format!(
                r#"<h2>处置结论</h2><div class="card">{}</div>"#,
                escape(text)
            )
        })
        .unwrap_or_default();
    let duplicate = issue
        .duplicate_of
        .map(|id| {
            format!(r#"<p class="meta">重复于 <a href="/issues/{id}"><code>{id}</code></a></p>"#)
        })
        .unwrap_or_default();

    let comments = if issue.comments.is_empty() {
        r#"<p class="empty">还没有讨论。</p>"#.to_string()
    } else {
        issue
            .comments
            .iter()
            .map(|comment| {
                format!(
                    r#"<div class="card">
<div class="meta"><strong>{author}</strong> · {at} {internal}</div>
<p>{body}</p>
</div>"#,
                    author = escape(comment.author.id()),
                    at = comment
                        .created_at
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                    internal = if comment.internal {
                        r#"<span class="badge">内部</span>"#
                    } else {
                        ""
                    },
                    body = escape(&comment.body),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let comment_form = if ctx.can_contribute() {
        format!(
            r#"<form method="post" action="/issues/{id}/comments">
  {body}
  <div class="field"><label><input type="checkbox" name="internal" value="on"> 内部备注（不对外部报告人展示）</label></div>
  <p class="hint">将以 {viewer} 的身份发表评论。</p>
  <div class="actions"><button type="submit">发表评论</button></div>
</form>"#,
            id = issue.id,
            viewer = escape(&ctx.viewer.display_name),
            body = textarea_field("body", "评论 *", "", None),
        )
    } else {
        String::new()
    };

    let triage_form = if ctx.can_contribute()
        && matches!(issue.status, IssueStatus::Open | IssueStatus::Triaged)
    {
        format!(
            r#"<h2>分诊</h2>
<form method="post" action="/issues/{id}/triage">
  <div class="grid">
    {kind}
    {severity}
  </div>
  {area}
  {impact}
  <div class="grid">
    {assignee_kind}
    {assignee_id}
  </div>
  {labels}
  <p class="hint">缺陷必须选择严重度。分诊会把议题移到 Triaged。将以 {viewer} 的身份记录分诊。</p>
  <div class="actions"><button type="submit">保存分诊</button></div>
</form>"#,
            id = issue.id,
            kind = select_field(
                "kind",
                "类型",
                issue.kind.as_str(),
                &[
                    ("feedback", "反馈"),
                    ("bug", "缺陷"),
                    ("question", "问题"),
                    ("idea", "想法"),
                    ("task_request", "任务请求"),
                    ("incident", "事故"),
                ],
            ),
            severity = select_field(
                "severity",
                "严重度",
                issue.severity.map(IssueSeverity::as_str).unwrap_or(""),
                &[
                    ("", "未设置"),
                    ("s1", "S1 · 致命"),
                    ("s2", "S2 · 严重"),
                    ("s3", "S3 · 一般"),
                    ("s4", "S4 · 轻微"),
                ],
            ),
            area = input_field(
                "area",
                "影响范围 / 组件",
                issue.area.as_deref().unwrap_or(""),
                false
            ),
            impact = textarea_field(
                "impact",
                "影响",
                issue.impact.as_deref().unwrap_or(""),
                Some("谁受影响、影响多大。"),
            ),
            assignee_kind = select_field(
                "assignee_kind",
                "负责人类型",
                issue
                    .assignee
                    .as_ref()
                    .map(|actor| if actor.is_agent() { "agent" } else { "human" })
                    .unwrap_or("human"),
                &[("human", "人类"), ("agent", "智能体")],
            ),
            assignee_id = input_field(
                "assignee_id",
                "负责人 ID",
                issue
                    .assignee
                    .as_ref()
                    .map(|actor| actor.id())
                    .unwrap_or(""),
                false,
            ),
            labels = textarea_field(
                "labels",
                "标签",
                &issue.labels.join("\n"),
                Some("每行一个。"),
            ),
            viewer = escape(&ctx.viewer.display_name),
        )
    } else {
        String::new()
    };

    let allowed = issue.status.allowed_transitions();
    let transition_form = if !allowed.is_empty() && ctx.can_dispose() {
        let options = allowed
            .iter()
            .map(|status| (status.as_str(), issue_status_label(*status)))
            .collect::<Vec<_>>();
        format!(
            r#"<h2>状态转换</h2>
<form method="post" action="/issues/{id}/transitions">
  <div class="grid">
    {to}
  </div>
  {resolution}
  {duplicate_of}
  <p class="hint">Resolved / Rejected 需要处置结论；Duplicate 需要目标议题 ID。将以 {viewer} 的身份记录处置。</p>
  <div class="actions"><button type="submit">执行转换</button></div>
</form>"#,
            id = issue.id,
            to = select_field("to", "目标状态", allowed[0].as_str(), &options),
            resolution = textarea_field(
                "resolution",
                "处置结论",
                issue.resolution.as_deref().unwrap_or(""),
                Some("Resolved 时说明怎么解决；Rejected 时说明为什么不处理。"),
            ),
            duplicate_of = input_field(
                "duplicate_of",
                "重复的目标议题 ID",
                &issue
                    .duplicate_of
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
                false,
            ),
            viewer = escape(&ctx.viewer.display_name),
        )
    } else {
        String::new()
    };

    let history = if issue.history.is_empty() {
        "<li>无状态变更</li>".to_string()
    } else {
        issue
            .history
            .iter()
            .map(|change| {
                format!(
                    "<li>{} → {} · {} · {}{}</li>",
                    issue_status_label(change.from),
                    issue_status_label(change.to),
                    escape(change.actor.id()),
                    change
                        .at
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                    change
                        .note
                        .as_ref()
                        .map(|note| format!(" · {}", escape(note)))
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let body = format!(
        r#"<p><a href="/issues">← 返回议题列表</a></p>
<h1>{title}</h1>
<p class="sub"><span class="badge">{kind}</span> <span class="badge">{status}</span> · 版本 {version}</p>
<div class="meta">
  <span>严重度 <code>{severity}</code></span>
  <span>负责人 <code>{assignee}</code></span>
  <span>报告人 <code>{reporter}</code></span>
  <span>来源 <code>{source}</code>{source_reference}</span>
  <span>项目 ID <code>{project_id}</code></span>
  <span>ID <code>{id}</code></span>
</div>
<h2>描述</h2><div class="card">{description}</div>
{bug_details}
{resolution}
{duplicate}
<h2>讨论</h2>
{comments}
{comment_form}
{triage_form}
{transition_form}
<h2>状态历史</h2><ul>{history}</ul>"#,
        title = escape(&issue.title),
        kind = issue_kind_label(issue.kind),
        status = issue_status_label(issue.status),
        version = issue.version,
        severity = issue.severity.map(issue_severity_label).unwrap_or("未设置"),
        assignee = issue
            .assignee
            .as_ref()
            .map(|actor| escape(actor.id()))
            .unwrap_or_else(|| "未分配".to_string()),
        reporter = escape(issue.reporter.id()),
        source = issue_source_label(issue.source.kind),
        source_reference = issue
            .source
            .reference
            .as_ref()
            .map(|reference| format!(" · <code>{}</code>", escape(reference)))
            .unwrap_or_default(),
        project_id = issue.project_id,
        id = issue.id,
        description = escape(&issue.description),
        bug_details = bug_details,
        resolution = resolution,
        duplicate = duplicate,
        comments = comments,
        comment_form = comment_form,
        triage_form = triage_form,
        transition_form = transition_form,
        history = history,
    );
    layout(ctx, Section::Issues, &issue.title, &body)
}

fn issue_bug_details(issue: &Issue) -> String {
    if issue.kind != IssueKind::Bug {
        return String::new();
    }
    let mut parts = Vec::new();
    if let Some(environment) = &issue.environment {
        parts.push(format!(
            "<p><strong>环境：</strong>{}</p>",
            escape(environment)
        ));
    }
    if !issue.affected_versions.is_empty() {
        parts.push(format!(
            "<p><strong>影响版本：</strong>{}</p>",
            escape(&issue.affected_versions.join("、"))
        ));
    }
    if let Some(reproduction) = &issue.reproduction {
        parts.push(format!(
            "<p><strong>复现步骤：</strong>{}</p>",
            escape(reproduction)
        ));
    }
    if let Some(expected) = &issue.expected {
        parts.push(format!(
            "<p><strong>预期：</strong>{}</p>",
            escape(expected)
        ));
    }
    if let Some(actual) = &issue.actual {
        parts.push(format!("<p><strong>实际：</strong>{}</p>", escape(actual)));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(
            r#"<h2>缺陷信息</h2><div class="card">{}</div>"#,
            parts.join("")
        )
    }
}

/// Values retained when the issue creation form is re-rendered after an error.
#[derive(Debug, Clone)]
pub struct IssueFormValues {
    pub title: String,
    pub description: String,
    pub kind: String,
    pub source_kind: String,
    pub source_reference: String,
    pub severity: String,
    pub area: String,
    pub labels: String,
    pub environment: String,
    pub affected_versions: String,
    pub reproduction: String,
    pub expected: String,
    pub actual: String,
}

impl Default for IssueFormValues {
    fn default() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            kind: "feedback".to_string(),
            source_kind: "internal".to_string(),
            source_reference: String::new(),
            severity: String::new(),
            area: String::new(),
            labels: String::new(),
            environment: String::new(),
            affected_versions: String::new(),
            reproduction: String::new(),
            expected: String::new(),
            actual: String::new(),
        }
    }
}

pub fn new_issue_page(
    ctx: &ConsoleContext,
    values: &IssueFormValues,
    error: Option<&str>,
) -> String {
    let error_html = error
        .map(|message| format!(r#"<div class="error">{}</div>"#, escape(message)))
        .unwrap_or_default();

    let body = format!(
        r#"<p><a href="/issues">← 返回议题列表</a></p>
<h1>新建议题</h1>
<p class="sub">把发生的事说清楚即可。分类、优先级和处置结论留给分诊。</p>
{error_html}
<form method="post" action="/issues">
  {title}
  {description}
  <div class="grid">
    {kind}
    {source_kind}
  </div>
  {source_reference}
  <div class="grid">
    {severity}
    {area}
  </div>
  {labels}
  <fieldset>
    <legend>缺陷信息（仅缺陷时需要）</legend>
    {environment}
    {affected_versions}
    {reproduction}
    {expected}
    {actual}
  </fieldset>
  <p class="hint">将以 {viewer} 的身份记录报告人。</p>
  <div class="actions"><button type="submit">创建议题</button></div>
</form>"#,
        title = input_field("title", "标题 *", &values.title, true),
        description = textarea_field(
            "description",
            "描述 *",
            &values.description,
            Some("发生了什么？谁受影响？已知的上下文。"),
        ),
        kind = select_field(
            "kind",
            "类型",
            &values.kind,
            &[
                ("feedback", "反馈"),
                ("bug", "缺陷"),
                ("question", "问题"),
                ("idea", "想法"),
                ("task_request", "任务请求"),
                ("incident", "事故"),
            ],
        ),
        source_kind = select_field(
            "source_kind",
            "来源",
            &values.source_kind,
            &[
                ("internal", "内部"),
                ("customer", "客户"),
                ("support", "客服"),
                ("monitoring", "监控"),
                ("analytics", "数据分析"),
                ("incident", "事故"),
                ("other", "其他"),
            ],
        ),
        source_reference = input_field(
            "source_reference",
            "来源引用",
            &values.source_reference,
            false,
        ),
        severity = select_field(
            "severity",
            "严重度",
            &values.severity,
            &[
                ("", "未设置"),
                ("s1", "S1 · 致命"),
                ("s2", "S2 · 严重"),
                ("s3", "S3 · 一般"),
                ("s4", "S4 · 轻微"),
            ],
        ),
        area = input_field("area", "影响范围 / 组件", &values.area, false),
        labels = textarea_field("labels", "标签", &values.labels, Some("每行一个。"),),
        environment = textarea_field("environment", "环境", &values.environment, None),
        affected_versions = textarea_field(
            "affected_versions",
            "影响版本",
            &values.affected_versions,
            Some("每行一个。"),
        ),
        reproduction = textarea_field("reproduction", "复现步骤", &values.reproduction, None),
        expected = textarea_field("expected", "预期结果", &values.expected, None),
        actual = textarea_field("actual", "实际结果", &values.actual, None),
        viewer = escape(&ctx.viewer.display_name),
    );
    layout(ctx, Section::Issues, "新建议题", &body)
}

// ---------------------------------------------------------------------------
// Projects and accounts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct MemberInfo {
    pub account_id: String,
    pub handle: String,
    pub display_name: String,
    pub kind: AccountKind,
    pub status: AccountStatus,
    pub role: ProjectRole,
}

pub fn projects_page(
    ctx: &ConsoleContext,
    projects: &[ProjectInfo],
    root_workflows: &[WorkflowOption],
    error: Option<&str>,
) -> String {
    let error_html = error
        .map(|message| format!(r#"<div class="error">{}</div>"#, escape(message)))
        .unwrap_or_default();
    let rows = if projects.is_empty() {
        r#"<tr><td colspan="4" class="empty">还没有项目。创建一个项目开始使用。</td></tr>"#
            .to_string()
    } else {
        projects
            .iter()
            .map(|project| {
                format!(
                    r#"<tr>
<td><a href="/projects/{id}">{name}</a></td>
<td><span class="badge">{status}</span></td>
<td>{role}</td>
<td>{current}</td>
</tr>"#,
                    id = escape(&project.id),
                    name = escape(&project.name),
                    status = project_status_label(project.status),
                    role = project.role.map(project_role_label).unwrap_or("（管理员）"),
                    current = if ctx
                        .current
                        .as_ref()
                        .is_some_and(|current| current.id == project.id)
                    {
                        "当前"
                    } else {
                        ""
                    },
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let default_root = root_workflows
        .iter()
        .find(|workflow| {
            matches!(workflow.owner, WorkflowOwner::Platform) && workflow.id.as_str() == "project"
        })
        .or_else(|| root_workflows.first())
        .map(|workflow| workflow.id.as_str().to_string())
        .unwrap_or_default();
    let root_options = root_workflows
        .iter()
        .map(|workflow| {
            let owner = match &workflow.owner {
                WorkflowOwner::Platform => "平台".to_string(),
                WorkflowOwner::Account { display_name, .. } => {
                    format!("账户：{display_name}")
                }
            };
            let selected = if workflow.id.as_str() == default_root {
                " selected"
            } else {
                ""
            };
            format!(
                r#"<option value="{}"{selected}>{}（{}）</option>"#,
                escape(workflow.id.as_str()),
                escape(&workflow.title),
                escape(&owner),
            )
        })
        .collect::<String>();
    let root_field = format!(
        r#"<div class="field"><label for="root_workflow_id">根流程 *</label><select id="root_workflow_id" name="root_workflow_id" required>{root_options}</select><p class="hint">项目创建时固定根 Task 的已发布版本；之后流程的修改不会影响这个项目。</p></div>"#
    );

    let body = format!(
        r#"<h1>项目</h1>
<p class="sub">项目是隔离边界：议题、需求和成员都在项目内。普通成员只能看到自己参与的项目。</p>
{error_html}
<table>
<thead><tr><th>名称</th><th>状态</th><th>我的角色</th><th></th></tr></thead>
<tbody>{rows}</tbody>
</table>
<h2>新建项目</h2>
<form method="post" action="/projects">
  {name}
  {description}
  {root_field}
  <p class="hint">创建者成为项目 Owner。项目名称可以修改。</p>
  <div class="actions"><button type="submit">创建项目</button></div>
</form>"#,
        error_html = error_html,
        name = input_field("name", "名称 *", "", true),
        description = textarea_field("description", "描述", "", None),
    );
    layout(ctx, Section::Projects, "项目", &body)
}

pub fn project_page(
    ctx: &ConsoleContext,
    project: &Project,
    members: &[MemberInfo],
    can_manage: bool,
    error: Option<&str>,
) -> String {
    let error_html = error
        .map(|message| format!(r#"<div class="error">{}</div>"#, escape(message)))
        .unwrap_or_default();
    let role_options = project_role_options();
    let member_rows = members
        .iter()
        .map(|member| {
            let role_cell = if can_manage {
                let options = role_options
                    .iter()
                    .map(|(value, label)| {
                        let selected = if *value == member.role.as_str() {
                            " selected"
                        } else {
                            ""
                        };
                        format!(r#"<option value="{value}"{selected}>{label}</option>"#)
                    })
                    .collect::<String>();
                format!(
                    r#"<form class="inline-form" method="post" action="/projects/{id}/members/{account_id}/role">
<select name="role">{options}</select>
<button type="submit">保存</button>
</form>"#,
                    id = project.id,
                    account_id = escape(&member.account_id),
                    options = options,
                )
            } else {
                project_role_label(member.role).to_string()
            };
            let remove = if can_manage {
                format!(
                    r#"<form class="inline-form" method="post" action="/projects/{id}/members/{account_id}/remove">
<button class="danger" type="submit">移除</button>
</form>"#,
                    id = project.id,
                    account_id = escape(&member.account_id),
                )
            } else {
                String::new()
            };
            format!(
                r#"<tr>
<td><strong>{display_name}</strong><br><code>@{handle}</code></td>
<td>{kind}</td>
<td>{status}</td>
<td>{role_cell}</td>
<td>{remove}</td>
</tr>"#,
                display_name = escape(&member.display_name),
                handle = escape(&member.handle),
                kind = if member.kind == AccountKind::Human {
                    "人类"
                } else {
                    "智能体"
                },
                status = account_status_label(member.status),
                role_cell = role_cell,
                remove = remove,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let manage = if can_manage {
        let role_options = role_options
            .iter()
            .map(|(value, label)| format!(r#"<option value="{value}">{label}</option>"#))
            .collect::<String>();
        let status_options = [
            (ProjectStatus::Active.as_str(), "活跃"),
            (ProjectStatus::Archived.as_str(), "已归档"),
        ]
        .iter()
        .map(|(value, label)| {
            let selected = if *value == project.status.as_str() {
                " selected"
            } else {
                ""
            };
            format!(r#"<option value="{value}"{selected}>{label}</option>"#)
        })
        .collect::<String>();
        let status_field = format!(
            r#"<div class="field"><label for="status">状态</label><select id="status" name="status">{status_options}</select></div>"#
        );
        let role_field = format!(
            r#"<div class="field"><label for="role">角色</label><select id="role" name="role">{role_options}</select></div>"#
        );
        format!(
            r#"<h2>项目设置</h2>
<form method="post" action="/projects/{id}">
  <div class="grid">
    {name}
    {status}
  </div>
  {description}
  <p class="hint">归档后项目只读。恢复为活跃后才能继续编辑和变更成员。</p>
  <div class="actions"><button type="submit">保存设置</button></div>
</form>
<h2>添加成员</h2>
<form method="post" action="/projects/{id}/members">
  <div class="grid">
    {handle}
    {role}
  </div>
  <p class="hint">输入已存在的账号 handle。只有项目 Owner 或全局管理员可以管理成员。</p>
  <div class="actions"><button type="submit">添加成员</button></div>
</form>"#,
            id = project.id,
            name = input_field("name", "名称", &project.name, true),
            status = status_field,
            description = textarea_field(
                "description",
                "描述",
                project.description.as_deref().unwrap_or(""),
                None,
            ),
            handle = input_field("handle", "账号 handle", "", true),
            role = role_field,
        )
    } else {
        String::new()
    };

    let body = format!(
        r#"<p><a href="/projects">← 返回项目列表</a></p>
<h1>{name}</h1>
<p class="sub"><span class="badge">{status}</span></p>
{error_html}
<div class="meta">
  <span>ID <code>{id}</code></span>
  <span>成员 {member_count}</span>
  <span>版本 {version}</span>
</div>
<h2>描述</h2><div class="card">{description}</div>
<h2>成员</h2>
<table>
<thead><tr><th>账号</th><th>类型</th><th>状态</th><th>项目角色</th><th></th></tr></thead>
<tbody>{member_rows}</tbody>
</table>
{manage}"#,
        name = escape(&project.name),
        status = project_status_label(project.status),
        error_html = error_html,
        id = project.id,
        member_count = members.len(),
        version = project.version,
        description = project
            .description
            .as_deref()
            .map(escape)
            .unwrap_or_else(|| "未填写".to_string()),
        member_rows = member_rows,
        manage = manage,
    );
    layout(ctx, Section::Projects, &project.name, &body)
}

pub fn accounts_page(ctx: &ConsoleContext, accounts: &[Account], error: Option<&str>) -> String {
    let error_html = error
        .map(|message| format!(r#"<div class="error">{}</div>"#, escape(message)))
        .unwrap_or_default();
    let rows = if accounts.is_empty() {
        r#"<tr><td colspan="6" class="empty">还没有账户。</td></tr>"#.to_string()
    } else {
        accounts
            .iter()
            .map(|account| {
                format!(
                    r#"<tr>
<td><a href="/accounts/{id}"><strong>{display_name}</strong></a><br><code>@{handle}</code></td>
<td>{kind}</td>
<td>{role}</td>
<td>{status}</td>
<td>{email}</td>
<td>{updated}</td>
</tr>"#,
                    id = account.id,
                    display_name = escape(&account.display_name),
                    handle = escape(&account.handle),
                    kind = account_kind_label(account.kind),
                    role = account_role_label(account.role),
                    status = account_status_label(account.status),
                    email = account
                        .email
                        .as_deref()
                        .map(escape)
                        .unwrap_or_else(|| "—".to_string()),
                    updated = account
                        .updated_at
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let kind_options = [("human", "人类"), ("agent", "智能体")]
        .iter()
        .map(|(value, label)| format!(r#"<option value="{value}">{label}</option>"#))
        .collect::<String>();
    let role_options = [("member", "Member"), ("admin", "Admin")]
        .iter()
        .map(|(value, label)| format!(r#"<option value="{value}">{label}</option>"#))
        .collect::<String>();

    let kind_field = format!(
        r#"<div class="field"><label for="kind">类型</label><select id="kind" name="kind">{kind_options}</select></div>"#
    );
    let role_field = format!(
        r#"<div class="field"><label for="role">全局角色</label><select id="role" name="role">{role_options}</select></div>"#
    );

    let body = format!(
        r#"<h1>账户</h1>
<p class="sub">账户是人类和智能体的统一身份。密码只以 Argon2 哈希保存；会话令牌只保存 SHA-256 哈希。</p>
{error_html}
<table>
<thead><tr><th>账号</th><th>类型</th><th>全局角色</th><th>状态</th><th>邮箱</th><th>更新时间</th></tr></thead>
<tbody>{rows}</tbody>
</table>
<h2>创建账户</h2>
<form method="post" action="/accounts">
  <div class="grid">
    {handle}
    {display_name}
  </div>
  <div class="grid">
    {kind}
    {role}
  </div>
  <div class="grid">
    {email}
    {password}
  </div>
  <p class="hint">密码不能为空。智能体账户也可以使用密码登录以获取会话令牌。</p>
  <div class="actions"><button type="submit">创建账户</button></div>
</form>"#,
        error_html = error_html,
        handle = input_field("handle", "Handle *", "", true),
        display_name = input_field("display_name", "显示名称 *", "", true),
        kind = kind_field,
        role = role_field,
        email = input_field("email", "邮箱", "", false),
        password = r#"<div class="field"><label for="password">初始密码 *</label><input id="password" name="password" type="password" autocomplete="new-password" required></div>"#,
    );
    layout(ctx, Section::Accounts, "账户", &body)
}

pub fn account_page(
    ctx: &ConsoleContext,
    account: &Account,
    can_manage: bool,
    error: Option<&str>,
) -> String {
    let error_html = error
        .map(|message| format!(r#"<div class="error">{}</div>"#, escape(message)))
        .unwrap_or_default();
    let edit = if can_manage {
        let role_options = [("member", "Member"), ("admin", "Admin")]
            .iter()
            .map(|(value, label)| {
                let selected = if *value == account.role.as_str() {
                    " selected"
                } else {
                    ""
                };
                format!(r#"<option value="{value}"{selected}>{label}</option>"#)
            })
            .collect::<String>();
        let status_options = [("active", "活跃"), ("suspended", "已停用")]
            .iter()
            .map(|(value, label)| {
                let selected = if *value == account.status.as_str() {
                    " selected"
                } else {
                    ""
                };
                format!(r#"<option value="{value}"{selected}>{label}</option>"#)
            })
            .collect::<String>();
        let role_field = format!(
            r#"<div class="field"><label for="role">全局角色</label><select id="role" name="role">{role_options}</select></div>"#
        );
        let status_field = format!(
            r#"<div class="field"><label for="status">状态</label><select id="status" name="status">{status_options}</select></div>"#
        );
        format!(
            r#"<h2>管理</h2>
<form method="post" action="/accounts/{id}">
  <div class="grid">
    {display_name}
    {email}
  </div>
  <div class="grid">
    {role}
    {status}
  </div>
  <div class="field"><label for="new_password">重置密码（留空则不修改）</label>
    <input id="new_password" name="new_password" type="password" autocomplete="new-password"></div>
  <p class="hint">不能停用或降级最后一个活跃管理员。修改密码会使该账户的旧密码失效，但不会撤销已有会话。</p>
  <div class="actions"><button type="submit">保存账户</button></div>
</form>"#,
            id = account.id,
            display_name = input_field("display_name", "显示名称", &account.display_name, true),
            email = input_field(
                "email",
                "邮箱",
                account.email.as_deref().unwrap_or(""),
                false
            ),
            role = role_field,
            status = status_field,
        )
    } else {
        String::new()
    };

    let body = format!(
        r#"<p><a href="/accounts">← 返回账户列表</a></p>
<h1>{display_name}</h1>
<p class="sub"><code>@{handle}</code> · {kind} · {role}</p>
{error_html}
<div class="meta">
  <span>状态 <span class="badge">{status}</span></span>
  <span>邮箱 <code>{email}</code></span>
  <span>创建于 {created}</span>
  <span>ID <code>{id}</code></span>
</div>
{edit}"#,
        display_name = escape(&account.display_name),
        handle = escape(&account.handle),
        kind = account_kind_label(account.kind),
        role = account_role_label(account.role),
        error_html = error_html,
        status = account_status_label(account.status),
        email = account
            .email
            .as_deref()
            .map(escape)
            .unwrap_or_else(|| "未填写".to_string()),
        created = account
            .created_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        id = account.id,
        edit = edit,
    );
    layout(ctx, Section::Accounts, &account.display_name, &body)
}

pub fn workflows_page(
    ctx: &ConsoleContext,
    workflows: &[WorkflowDefinition],
    error: Option<&str>,
) -> String {
    let error_html = error
        .map(|message| format!(r#"<div class="error">{}</div>"#, escape(message)))
        .unwrap_or_default();

    let platform: Vec<&WorkflowDefinition> = workflows
        .iter()
        .filter(|workflow| matches!(workflow.owner, WorkflowOwner::Platform))
        .collect();
    let accounts: Vec<&WorkflowDefinition> = workflows
        .iter()
        .filter(|workflow| !matches!(workflow.owner, WorkflowOwner::Platform))
        .collect();

    let create_form = if ctx.viewer.kind == AccountKind::Human {
        let scope_options: Vec<(&str, &str)> = if ctx.viewer.is_admin() {
            vec![("account", "账户流程"), ("platform", "平台流程")]
        } else {
            vec![("account", "账户流程")]
        };
        let mut copy_options: Vec<(String, String)> = vec![(String::new(), "空白定义".to_string())];
        for workflow in &platform {
            if let Some(version) = workflow.latest_version {
                copy_options.push((
                    format!("{}@{}", workflow.id, version),
                    format!("{}（v{version}）", workflow.title),
                ));
            }
        }
        let copy_options: Vec<(&str, &str)> = copy_options
            .iter()
            .map(|(value, text)| (value.as_str(), text.as_str()))
            .collect();
        format!(
            r#"<h2>新建流程</h2>
<form method="post" action="/workflows">
  {title}
  {scope}
  {copy}
  <p class="hint">从平台流程复制会生成新的账户流程，与源流程完全独立。</p>
  <div class="actions"><button type="submit">创建流程</button></div>
</form>"#,
            title = input_field("title", "标题 *", "", true),
            scope = select_field("scope", "归属", "account", &scope_options),
            copy = select_field("copy_from", "复制自", "", &copy_options),
        )
    } else {
        String::new()
    };

    let body = format!(
        r#"<h1>流程</h1>
<p class="sub">平台流程对所有账户可见；账户流程只对所有者和管理员可见。已发布版本不可变，项目根 Task 固定创建时的版本。</p>
{error_html}
{create_form}
<h2>平台流程</h2>
<table>
<thead><tr><th>流程</th><th>归属</th><th>最新版本</th><th>草稿</th><th></th></tr></thead>
<tbody>{platform_rows}</tbody>
</table>
<h2>账户流程</h2>
<table>
<thead><tr><th>流程</th><th>归属</th><th>最新版本</th><th>草稿</th><th></th></tr></thead>
<tbody>{account_rows}</tbody>
</table>"#,
        error_html = error_html,
        create_form = create_form,
        platform_rows = workflow_rows(&platform),
        account_rows = workflow_rows(&accounts),
    );
    layout(ctx, Section::Workflows, "流程", &body)
}

fn workflow_rows(workflows: &[&WorkflowDefinition]) -> String {
    if workflows.is_empty() {
        return r#"<tr><td colspan="5" class="empty">暂无流程。</td></tr>"#.to_string();
    }
    workflows
        .iter()
        .map(|workflow| {
            let owner = match &workflow.owner {
                WorkflowOwner::Platform => "平台".to_string(),
                WorkflowOwner::Account { display_name, .. } => format!("账户：{display_name}"),
            };
            let version = workflow
                .latest_version
                .map(|version| format!("v{version}"))
                .unwrap_or_else(|| "未发布".to_string());
            let draft = workflow
                .draft_revision
                .map(|revision| format!("#{revision}"))
                .unwrap_or_else(|| "—".to_string());
            let actions = if workflow.editable {
                let id = escape(workflow.id.as_str());
                let publish = workflow
                    .draft_revision
                    .map(|revision| {
                        format!(
                            r#"<form method="post" action="/workflows/{id}/publish"><input type="hidden" name="revision" value="{revision}"><button class="secondary" type="submit">发布草稿</button></form>"#
                        )
                    })
                    .unwrap_or_default();
                format!(
                    r#"<form method="post" action="/workflows/{id}/rename"><input name="title" value="{title}" aria-label="新标题" required><button class="secondary" type="submit">重命名</button></form>{publish}"#,
                    title = escape(&workflow.title),
                )
            } else {
                String::new()
            };
            format!(
                r#"<tr>
<td><strong><a href="/workflows/editor/{id}">{title}</a></strong><br><span class="hint">{id}</span></td>
<td>{owner}</td>
<td>{version}</td>
<td>{draft}</td>
<td>{actions}</td>
</tr>"#,
                title = escape(&workflow.title),
                id = escape(workflow.id.as_str()),
                owner = escape(&owner),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn not_found_page(ctx: &ConsoleContext) -> String {
    layout(
        ctx,
        Section::None,
        "未找到",
        r#"<h1>404</h1><p class="sub">没有这个对象，或者它不属于当前项目。</p><p><a href="/">返回工作台</a></p>"#,
    )
}

fn project_role_options() -> Vec<(&'static str, &'static str)> {
    vec![
        ("viewer", "Viewer · 只读"),
        ("member", "Member · 可贡献"),
        ("maintainer", "Maintainer · 可处置"),
        ("owner", "Owner · 可管理成员"),
    ]
}

fn account_kind_label(kind: AccountKind) -> &'static str {
    match kind {
        AccountKind::Human => "人类",
        AccountKind::Agent => "智能体",
    }
}

fn account_role_label(role: AccountRole) -> &'static str {
    match role {
        AccountRole::Admin => "Admin",
        AccountRole::Member => "Member",
    }
}

fn account_status_label(status: AccountStatus) -> &'static str {
    match status {
        AccountStatus::Active => "活跃",
        AccountStatus::Suspended => "已停用",
    }
}

fn project_status_label(status: ProjectStatus) -> &'static str {
    match status {
        ProjectStatus::Active => "活跃",
        ProjectStatus::Archived => "已归档",
    }
}

fn project_role_label(role: ProjectRole) -> &'static str {
    match role {
        ProjectRole::Viewer => "Viewer",
        ProjectRole::Member => "Member",
        ProjectRole::Maintainer => "Maintainer",
        ProjectRole::Owner => "Owner",
    }
}

fn issue_status_label(status: IssueStatus) -> &'static str {
    use IssueStatus::*;
    match status {
        Open => "待分诊",
        Triaged => "已分诊",
        InProgress => "处理中",
        Resolved => "待确认",
        Closed => "已关闭",
        Rejected => "不处理",
        Duplicate => "重复",
    }
}

fn issue_kind_label(kind: IssueKind) -> &'static str {
    use IssueKind::*;
    match kind {
        Feedback => "反馈",
        Bug => "缺陷",
        Question => "问题",
        Idea => "想法",
        TaskRequest => "任务请求",
        Incident => "事故",
    }
}

fn issue_source_label(source: IssueSourceKind) -> &'static str {
    use IssueSourceKind::*;
    match source {
        Internal => "内部",
        Customer => "客户",
        Support => "客服",
        Monitoring => "监控",
        Analytics => "数据分析",
        Incident => "事故",
        Other => "其他",
    }
}

fn issue_severity_label(severity: IssueSeverity) -> &'static str {
    use IssueSeverity::*;
    match severity {
        S1 => "S1",
        S2 => "S2",
        S3 => "S3",
        S4 => "S4",
    }
}

fn input_field(name: &str, label: &str, value: &str, required: bool) -> String {
    format!(
        r#"<div class="field"><label for="{name}">{label}</label><input id="{name}" name="{name}" value="{}"{}></div>"#,
        escape(value),
        if required { " required" } else { "" },
    )
}

fn textarea_field(name: &str, label: &str, value: &str, hint: Option<&str>) -> String {
    let hint = hint
        .map(|text| format!(r#"<p class="hint">{}</p>"#, escape(text)))
        .unwrap_or_default();
    format!(
        r#"<div class="field"><label for="{name}">{label}</label><textarea id="{name}" name="{name}">{}</textarea>{hint}</div>"#,
        escape(value),
    )
}

fn select_field(name: &str, label: &str, current: &str, options: &[(&str, &str)]) -> String {
    let options = options
        .iter()
        .map(|(value, text)| {
            let selected = if *value == current { " selected" } else { "" };
            format!(r#"<option value="{value}"{selected}>{text}</option>"#)
        })
        .collect::<String>();
    format!(
        r#"<div class="field"><label for="{name}">{label}</label><select id="{name}" name="{name}">{options}</select></div>"#
    )
}

fn status_label(status: RequirementStatus) -> &'static str {
    use RequirementStatus::*;
    match status {
        Draft => "草稿",
        InReview => "评审中",
        Approved => "已批准",
        InDesign => "设计中",
        InDevelopment => "开发中",
        InVerification => "验证中",
        Done => "已完成",
        Rejected => "已拒绝",
        Deferred => "已搁置",
        Superseded => "已被取代",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_user_content() {
        assert_eq!(escape("<script>"), "&lt;script&gt;");
    }
}
