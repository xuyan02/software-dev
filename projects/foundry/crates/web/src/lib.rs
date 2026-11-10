//! Server-rendered HTML console.
//!
//! The console is a thin, dependency-light view over the same API agents use.
//! It intentionally has no JavaScript build step: HTML + a small stylesheet is
//! enough for humans to observe and act.

use foundry_domain::{
    Account, AccountKind, AccountRole, AccountStatus, AgentDefinition, AgentOverview,
    AnnotationStatus, Confidence, EntityStatus, Issue, IssueKind, IssueSeverity,
    IssueSourceKind, IssueStatus, Project, ProjectBriefAnnotation, ProjectBriefBody,
    ProjectBriefStatus, ProjectRole, ProjectStatus,
    Requirement, RequirementStatus, RequirementsAnnotation, RequirementsDocumentBody, RiskLevel,
    RoleKind, StoryPriority,
    Task, TaskDispatchState, TaskLink, TaskLinkKind, TaskLinkRelation, TaskState,
    WorkflowDefinition, WorkflowOption, WorkflowOwner,
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
.agent-log { background:var(--panel); border:1px solid var(--line); border-radius:8px;
             padding:14px 16px; max-height:72vh; overflow:auto; white-space:pre-wrap;
             word-break:break-word; font-family:ui-monospace, SFMono-Regular, Menlo, monospace;
             font-size:12px; line-height:1.5; }
.meta { display:flex; flex-wrap:wrap; gap:10px 22px; color:var(--muted); font-size:13px; }
.card { background:var(--panel); border:1px solid var(--line); border-radius:8px;
        padding:16px 18px; margin:12px 0; }
.markdown > :first-child { margin-top:0; }
.markdown > :last-child { margin-bottom:0; }
.markdown p { margin:8px 0; }
.markdown h1, .markdown h2, .markdown h3, .markdown h4 {
  margin:16px 0 8px; line-height:1.3;
}
.markdown h1 { font-size:20px; }
.markdown h2 { font-size:17px; }
.markdown h3 { font-size:15px; }
.markdown ul, .markdown ol { margin:8px 0; padding-left:22px; }
.markdown li { margin:2px 0; }
.markdown pre { background:var(--bg); border:1px solid var(--line); border-radius:6px;
                padding:10px 12px; overflow:auto; margin:10px 0; }
.markdown pre code { background:none; padding:0; }
.markdown blockquote { margin:10px 0; padding:2px 12px; border-left:3px solid var(--line);
                       color:var(--muted); }
.markdown hr { border:0; border-top:1px solid var(--line); margin:14px 0; }
.markdown .table-wrap { overflow-x:auto; margin:10px 0; }
.markdown table { width:100%; border-collapse:collapse; background:transparent; }
.markdown th, .markdown td { padding:6px 10px; border:1px solid var(--line); }
.markdown th { background:var(--bg); }
.markdown img { max-width:100%; }
.brief { margin-top:8px; }
.brief h2 { margin-top:26px; }
.brief h3 { margin:0 0 6px; font-size:14px; color:var(--muted); }
.brief .empty { padding:10px 0; text-align:left; }
.brief ul { margin:6px 0; }
.brief-editor { margin-top:28px; border-top:1px solid var(--line); padding-top:12px; }
.brief-editor summary { cursor:pointer; color:var(--accent); }
.annotation .markdown { margin:6px 0; }
.annotation .inline-form { margin-top:8px; }
.annotation-target { color:var(--accent); }
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

/// A small Markdown subset for Issue comments: headings, lists, code blocks,
/// inline code, emphasis, links and blockquotes. Raw HTML is always escaped.
fn markdown_to_html(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    let mut out = String::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let mut list: Option<&'static str> = None;
    let mut code: Option<(String, Vec<&str>)> = None;
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();

        if code.is_some() {
            if trimmed.starts_with("```") {
                let (language, body) = code.take().unwrap_or_default();
                out.push_str("<pre><code");
                if !language.is_empty() {
                    out.push_str(&format!(r#" class="language-{}""#, escape(&language)));
                }
                out.push('>');
                out.push_str(&escape(&body.join("\n")));
                out.push_str("</code></pre>");
            } else if let Some((_, body)) = code.as_mut() {
                body.push(line);
            }
            index += 1;
            continue;
        }

        if trimmed.starts_with("```") {
            flush_markdown_paragraph(&mut out, &mut paragraph);
            close_markdown_list(&mut out, &mut list);
            code = Some((trimmed.trim_start_matches('`').trim().to_string(), Vec::new()));
            index += 1;
            continue;
        }

        if trimmed.is_empty() {
            flush_markdown_paragraph(&mut out, &mut paragraph);
            close_markdown_list(&mut out, &mut list);
            index += 1;
            continue;
        }

        if let Some((level, rest)) = markdown_heading(trimmed) {
            flush_markdown_paragraph(&mut out, &mut paragraph);
            close_markdown_list(&mut out, &mut list);
            out.push_str(&format!("<h{level}>{}</h{level}>", inline_markdown(rest, 0)));
            index += 1;
            continue;
        }

        if markdown_hr(trimmed) {
            flush_markdown_paragraph(&mut out, &mut paragraph);
            close_markdown_list(&mut out, &mut list);
            out.push_str("<hr>");
            index += 1;
            continue;
        }

        if trimmed.starts_with('>') {
            flush_markdown_paragraph(&mut out, &mut paragraph);
            close_markdown_list(&mut out, &mut list);
            let mut quote = Vec::new();
            while index < lines.len() && lines[index].trim().starts_with('>') {
                quote.push(lines[index].trim().trim_start_matches('>').trim_start());
                index += 1;
            }
            out.push_str("<blockquote><p>");
            out.push_str(&inline_markdown(&quote.join("\n"), 0));
            out.push_str("</p></blockquote>");
            continue;
        }

        if let Some((tag, item)) = markdown_list_item(trimmed) {
            flush_markdown_paragraph(&mut out, &mut paragraph);
            if list != Some(tag) {
                close_markdown_list(&mut out, &mut list);
                out.push_str(&format!("<{tag}>"));
                list = Some(tag);
            }
            out.push_str("<li>");
            out.push_str(&inline_markdown(item, 0));
            out.push_str("</li>");
            index += 1;
            continue;
        }

        if index + 1 < lines.len()
            && let Some(header) = markdown_table_cells(line)
            && let Some(aligns) = markdown_table_aligns(lines[index + 1])
            && header.len() == aligns.len()
        {
            flush_markdown_paragraph(&mut out, &mut paragraph);
            close_markdown_list(&mut out, &mut list);
            out.push_str(r#"<div class="table-wrap"><table><thead><tr>"#);
            for (cell, align) in header.iter().zip(&aligns) {
                out.push_str(&format!(
                    "<th{}>{}</th>",
                    markdown_align_attr(*align),
                    inline_markdown(&cell.replace("\\|", "|"), 0)
                ));
            }
            out.push_str("</tr></thead><tbody>");
            index += 2;
            while index < lines.len() {
                let row_line = lines[index];
                if row_line.trim().is_empty() {
                    break;
                }
                let Some(cells) = markdown_table_cells(row_line) else {
                    break;
                };
                if cells.len() != header.len() {
                    break;
                }
                out.push_str("<tr>");
                for (cell, align) in cells.iter().zip(&aligns) {
                    out.push_str(&format!(
                        "<td{}>{}</td>",
                        markdown_align_attr(*align),
                        inline_markdown(&cell.replace("\\|", "|"), 0)
                    ));
                }
                out.push_str("</tr>");
                index += 1;
            }
            out.push_str("</tbody></table></div>");
            continue;
        }

        close_markdown_list(&mut out, &mut list);
        paragraph.push(line);
        index += 1;
    }

    if let Some((language, body)) = code {
        out.push_str("<pre><code");
        if !language.is_empty() {
            out.push_str(&format!(r#" class="language-{}""#, escape(&language)));
        }
        out.push('>');
        out.push_str(&escape(&body.join("\n")));
        out.push_str("</code></pre>");
    }
    flush_markdown_paragraph(&mut out, &mut paragraph);
    close_markdown_list(&mut out, &mut list);
    out
}

fn flush_markdown_paragraph(out: &mut String, paragraph: &mut Vec<&str>) {
    if paragraph.is_empty() {
        return;
    }
    out.push_str("<p>");
    out.push_str(&inline_markdown(&paragraph.join("\n"), 0));
    out.push_str("</p>");
    paragraph.clear();
}

fn close_markdown_list(out: &mut String, list: &mut Option<&'static str>) {
    if let Some(tag) = list.take() {
        out.push_str(&format!("</{tag}>"));
    }
}

fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let level = line.chars().take_while(|character| *character == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &line[level..];
    rest.starts_with(' ')
        .then(|| (level, rest.trim_start()))
}

fn markdown_hr(line: &str) -> bool {
    let Some(first) = line.chars().next() else {
        return false;
    };
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    line.chars().all(|character| character == first || character == ' ')
        && line.chars().filter(|character| *character == first).count() >= 3
}

fn markdown_list_item(line: &str) -> Option<(&'static str, &str)> {
    for prefix in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some(("ul", rest.trim_start()));
        }
    }
    let digits = line.chars().take_while(|character| character.is_ascii_digit()).count();
    if digits > 0 {
        if let Some(rest) = line[digits..].strip_prefix(". ") {
            return Some(("ol", rest.trim_start()));
        }
    }
    None
}

#[derive(Clone, Copy)]
enum MarkdownAlign {
    None,
    Left,
    Center,
    Right,
}

fn markdown_table_cells(line: &str) -> Option<Vec<&str>> {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return None;
    }
    let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut cells = Vec::new();
    let mut start = 0;
    let bytes = inner.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 1 < bytes.len() {
            index += 2;
            continue;
        }
        if bytes[index] == b'|' {
            cells.push(inner[start..index].trim());
            start = index + 1;
        }
        index += 1;
    }
    cells.push(inner[start..].trim());
    Some(cells)
}

fn markdown_table_aligns(line: &str) -> Option<Vec<MarkdownAlign>> {
    let cells = markdown_table_cells(line)?;
    let mut aligns = Vec::new();
    for cell in cells {
        let cell = cell.trim();
        let left = cell.starts_with(':');
        let right = cell.ends_with(':');
        let dashes = cell.trim_matches(':');
        if dashes.len() < 3 || !dashes.chars().all(|character| character == '-') {
            return None;
        }
        aligns.push(match (left, right) {
            (true, true) => MarkdownAlign::Center,
            (false, true) => MarkdownAlign::Right,
            (true, false) => MarkdownAlign::Left,
            (false, false) => MarkdownAlign::None,
        });
    }
    Some(aligns)
}

fn markdown_align_attr(align: MarkdownAlign) -> &'static str {
    match align {
        MarkdownAlign::None => "",
        MarkdownAlign::Left => r#" style="text-align:left""#,
        MarkdownAlign::Center => r#" style="text-align:center""#,
        MarkdownAlign::Right => r#" style="text-align:right""#,
    }
}

fn inline_markdown(text: &str, depth: usize) -> String {
    if depth > 6 {
        return escape(text);
    }
    let characters: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < characters.len() {
        match characters[index] {
            '`' => {
                if let Some(end) = markdown_find(&characters, index + 1, '`') {
                    let code: String = characters[index + 1..end].iter().collect();
                    out.push_str("<code>");
                    out.push_str(&escape(code.trim()));
                    out.push_str("</code>");
                    index = end + 1;
                } else {
                    out.push('`');
                    index += 1;
                }
            }
            '[' => {
                if let Some((label, url, end)) = markdown_link(&characters, index) {
                    if markdown_safe_url(&url) {
                        out.push_str(&format!(
                            r#"<a href="{}" rel="noreferrer noopener" target="_blank">{}</a>"#,
                            escape(&url),
                            inline_markdown(&label, depth + 1)
                        ));
                    } else {
                        out.push_str(&escape(&format!("[{label}]({url})")));
                    }
                    index = end;
                } else {
                    out.push('[');
                    index += 1;
                }
            }
            '*' => {
                if index + 1 < characters.len() && characters[index + 1] == '*' {
                    if let Some(end) = markdown_find_pair(&characters, index + 2, '*', '*') {
                        let inner: String = characters[index + 2..end].iter().collect();
                        out.push_str("<strong>");
                        out.push_str(&inline_markdown(&inner, depth + 1));
                        out.push_str("</strong>");
                        index = end + 2;
                    } else {
                        out.push('*');
                        index += 1;
                    }
                } else if let Some(end) = markdown_find(&characters, index + 1, '*') {
                    let inner: String = characters[index + 1..end].iter().collect();
                    out.push_str("<em>");
                    out.push_str(&inline_markdown(&inner, depth + 1));
                    out.push_str("</em>");
                    index = end + 1;
                } else {
                    out.push('*');
                    index += 1;
                }
            }
            '~' => {
                if index + 1 < characters.len() && characters[index + 1] == '~' {
                    if let Some(end) = markdown_find_pair(&characters, index + 2, '~', '~') {
                        let inner: String = characters[index + 2..end].iter().collect();
                        out.push_str("<del>");
                        out.push_str(&inline_markdown(&inner, depth + 1));
                        out.push_str("</del>");
                        index = end + 2;
                    } else {
                        out.push('~');
                        index += 1;
                    }
                } else {
                    out.push('~');
                    index += 1;
                }
            }
            '\n' => {
                out.push_str("<br>");
                index += 1;
            }
            _ => {
                let start = index;
                while index < characters.len()
                    && !matches!(characters[index], '`' | '[' | '*' | '~' | '\n')
                {
                    index += 1;
                }
                let literal: String = characters[start..index].iter().collect();
                out.push_str(&escape(&literal));
            }
        }
    }
    out
}

fn markdown_find(characters: &[char], start: usize, needle: char) -> Option<usize> {
    (start..characters.len()).find(|index| characters[*index] == needle)
}

fn markdown_find_pair(
    characters: &[char],
    start: usize,
    first: char,
    second: char,
) -> Option<usize> {
    (start..characters.len().saturating_sub(1))
        .find(|index| characters[*index] == first && characters[*index + 1] == second)
}

fn markdown_link(characters: &[char], start: usize) -> Option<(String, String, usize)> {
    let mut index = start + 1;
    let mut label = String::new();
    while index < characters.len() && characters[index] != ']' {
        label.push(characters[index]);
        index += 1;
    }
    if index >= characters.len() || characters[index] != ']' {
        return None;
    }
    index += 1;
    if index >= characters.len() || characters[index] != '(' {
        return None;
    }
    index += 1;
    let mut url = String::new();
    while index < characters.len() && characters[index] != ')' {
        if characters[index].is_whitespace() {
            return None;
        }
        url.push(characters[index]);
        index += 1;
    }
    if index >= characters.len() || characters[index] != ')' {
        return None;
    }
    Some((label, url, index + 1))
}

fn markdown_safe_url(url: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("mailto:")
        || url.starts_with('/')
        || url.starts_with('#')
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
    Brief,
    Tasks,
    Projects,
    Workflows,
    Accounts,
    Agents,
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

    let brief_nav = ctx
        .current
        .as_ref()
        .map(|project| {
            nav_item(
                active,
                Section::Brief,
                &format!("/projects/{}/brief", project.id),
                "项目简报",
            )
        })
        .unwrap_or_default();

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
    {brief_nav}
  </div>
  <div class="nav-group">
    <p class="nav-heading">执行</p>
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
    {}
  </div>
  <details class="nav-more">
    <summary>后续模块</summary>
    <div class="nav-group">
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
        nav_item(active, Section::Tasks, "/tasks", "任务"),
        nav_item(active, Section::Workflows, "/workflows", "流程"),
        nav_item(active, Section::Projects, "/projects", "项目"),
        nav_item(active, Section::Accounts, "/accounts", "账户"),
        if ctx.viewer.is_admin() {
            nav_item(active, Section::Agents, "/agents", "智能体")
        } else {
            String::new()
        },
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
        brief_nav = brief_nav,
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

pub fn workbench(
    ctx: &ConsoleContext,
    issues: &[Issue],
    requirements: &[Requirement],
    tasks: &[Task],
) -> String {
    let fresh = issues
        .iter()
        .filter(|issue| issue.status == IssueStatus::Open)
        .count();
    let active = issues
        .iter()
        .filter(|issue| issue.status == IssueStatus::InProgress)
        .count();
    let resolved = issues
        .iter()
        .filter(|issue| issue.status == IssueStatus::Resolved)
        .count();
    let task_total = tasks.len();
    let task_active = tasks
        .iter()
        .filter(|task| task.state == TaskState::InProgress)
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
    <span class="stat-value">{fresh}<small> 待处理</small></span>
    <span class="stat-foot">{active} 处理中 · {resolved} 待确认</span>
  </a>
  <a class="stat" href="/requirements">
    <span class="stat-label">需求（早期实现）</span>
    <span class="stat-value">{requirement_count}</span>
    <span class="stat-foot">承诺空间</span>
  </a>
  <a class="stat" href="/tasks">
    <span class="stat-label">任务</span>
    <span class="stat-value">{task_total}</span>
    <span class="stat-foot">{task_active} 处理中</span>
  </a>
</div>

<h2>流程</h2>
<div class="flow">
  <span class="flow-step available">议题</span>
  <span class="flow-arrow">→</span>
  <span class="flow-step available">需求</span>
  <span class="flow-arrow">→</span>
  <span class="flow-step">设计</span>
  <span class="flow-arrow">→</span>
  <span class="flow-step available">任务</span>
  <span class="flow-arrow">→</span>
  <span class="flow-step">验证</span>
  <span class="flow-arrow">→</span>
  <span class="flow-step">发布</span>
</div>
<p class="hint">当前已接入议题、需求和任务；设计文档、决策和发布尚未接入。</p>

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

pub fn tasks_page(ctx: &ConsoleContext, tasks: &[Task]) -> String {
    let ids: std::collections::HashSet<_> = tasks.iter().map(|task| task.id).collect();
    let mut children: std::collections::HashMap<Option<uuid::Uuid>, Vec<&Task>> =
        std::collections::HashMap::new();
    for task in tasks {
        let parent = task.parent_id.filter(|parent| ids.contains(parent));
        children.entry(parent).or_default().push(task);
    }
    let mut rows = String::new();
    for root in children.get(&None).cloned().unwrap_or_default() {
        render_task_tree(&mut rows, root, 0, &children);
    }
    if rows.is_empty() {
        rows = r#"<tr><td colspan="5" class="empty">还没有任务。项目创建后，流程脚本会创建第一个任务。</td></tr>"#.to_string();
    }

    let refresh = task_refresh_script("/tasks/state", &tasks_fingerprint(tasks));
    let body = format!(
        r#"<h1>任务</h1>
<p class="sub">执行空间：谁在什么时候做什么。任务由流程脚本创建；声明 agent 角色的任务由任务池调度。</p>
<table>
<thead><tr><th>标题 / 流程</th><th>状态</th><th>阶段</th><th>调度</th><th>Agent</th></tr></thead>
<tbody>{rows}</tbody>
</table>{refresh}"#
    );
    layout(ctx, Section::Tasks, "任务", &body)
}

pub fn tasks_fingerprint(tasks: &[Task]) -> String {
    let mut tasks = tasks
        .iter()
        .map(|task| {
            format!(
                "{}:{}:{}:{}",
                task.id,
                task.state.as_str(),
                task.stage
                    .as_ref()
                    .map(|stage| stage.as_str())
                    .unwrap_or("-"),
                task.updated_at.unix_timestamp_nanos()
            )
        })
        .collect::<Vec<_>>();
    tasks.sort();
    tasks.join(";")
}

pub fn task_fingerprint(task: &Task) -> String {
    format!(
        "{}:{}:{}:{}",
        task.id,
        task.state.as_str(),
        task.stage
            .as_ref()
            .map(|stage| stage.as_str())
            .unwrap_or("-"),
        task.updated_at.unix_timestamp_nanos()
    )
}

fn task_refresh_script(endpoint: &str, fingerprint: &str) -> String {
    format!(
        r#"<script>
(() => {{
  const endpoint = "{endpoint}";
  const known = "{fingerprint}";
  let busy = false;
  const dirty = () => Array.from(
    document.querySelectorAll('main textarea, main input[type="text"]')
  ).some((element) => element.value.trim().length > 0);
  const tick = async () => {{
    if (busy || document.hidden || dirty()) return;
    busy = true;
    try {{
      const response = await fetch(endpoint, {{
        cache: 'no-store',
        credentials: 'same-origin'
      }});
      if (response.ok) {{
        const next = (await response.text()).trim();
        if (next !== known && !dirty()) {{
          location.reload();
          return;
        }}
      }}
    }} catch (_) {{}}
    busy = false;
  }};
  setInterval(tick, 3000);
  document.addEventListener('visibilitychange', () => {{
    if (!document.hidden) tick();
  }});
}})();
</script>"#
    )
}

fn render_task_tree(
    out: &mut String,
    task: &Task,
    depth: usize,
    children: &std::collections::HashMap<Option<uuid::Uuid>, Vec<&Task>>,
) {
    let indent = "↳ ".repeat(depth);
    let agents = task
        .role_specs
        .iter()
        .filter(|role| role.kind == RoleKind::Agent)
        .map(|role| {
            let bound = task
                .actors
                .iter()
                .any(|actor| actor.role_key.as_str() == role.name);
            format!(
                "{} · {} · {}",
                escape(&role.name),
                role_kind_label(role.kind),
                if bound { "已绑定" } else { "未绑定" }
            )
        })
        .collect::<Vec<_>>();
    let agents = if agents.is_empty() {
        "—".to_string()
    } else {
        agents.join("<br>")
    };
    out.push_str(&format!(
        r#"<tr>
<td>{indent}<a href="/tasks/{id}">{title}</a><br><code>{workflow}</code></td>
<td><span class="badge">{state}</span></td>
<td>{stage}</td>
<td>{dispatch}</td>
<td>{agents}</td>
</tr>"#,
        id = task.id,
        indent = indent,
        title = escape(&task.title),
        workflow = escape(task.workflow.as_str()),
        state = task_state_label(task.state),
        stage = task
            .stage
            .as_ref()
            .map(|stage| escape(stage.as_str()))
            .unwrap_or_else(|| "—".to_string()),
        dispatch = task_dispatch_label(task.dispatch_state),
        agents = agents,
    ));
    if let Some(kids) = children.get(&Some(task.id)) {
        for kid in kids {
            render_task_tree(out, kid, depth + 1, children);
        }
    }
}

pub fn task_page(
    ctx: &ConsoleContext,
    task: &Task,
    links: &[TaskLink],
) -> String {
    let role_rows = if task.role_specs.is_empty() {
        r#"<tr><td colspan="4" class="empty">流程没有声明角色。</td></tr>"#.to_string()
    } else {
        task.role_specs
            .iter()
            .map(|role| {
                let actors = task
                    .actors
                    .iter()
                    .filter(|actor| actor.role_key.as_str() == role.name)
                    .map(|actor| {
                        format!(
                            "{} {}",
                            escape(&actor.display_name),
                            if actor.active {
                                r#"<span class="badge">活跃</span>"#
                            } else {
                                ""
                            }
                        )
                    })
                    .collect::<Vec<_>>();
                let actors = if actors.is_empty() {
                    "未绑定".to_string()
                } else {
                    actors.join("<br>")
                };
                format!(
                    r#"<tr><td><code>{name}</code></td><td>{kind}</td><td>{range}</td><td>{actors}</td></tr>"#,
                    name = escape(&role.name),
                    kind = role_kind_label(role.kind),
                    range = match role.max_actors {
                        Some(max) => format!("{} - {max}", role.min_actors),
                        None => format!("{} - ∞", role.min_actors),
                    },
                    actors = actors,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let link_rows = if links.is_empty() {
        r#"<tr><td colspan="4" class="empty">没有关联对象。</td></tr>"#.to_string()
    } else {
        links
            .iter()
            .map(|link| {
                let target = match (link.kind, link.target_id, link.url.as_deref()) {
                    (TaskLinkKind::Issue, Some(id), _) => {
                        format!(r#"<a href="/issues/{id}">打开议题</a>"#)
                    }
                    (TaskLinkKind::Url, _, Some(url)) => format!(
                        r#"<a href="{url}" rel="noreferrer">打开链接</a>"#,
                        url = escape(url)
                    ),
                    (_, Some(id), _) => format!("<code>{id}</code>"),
                    _ => "—".to_string(),
                };
                format!(
                    r#"<tr><td>{kind}</td><td>{relation}</td><td>{title}</td><td>{target}</td></tr>"#,
                    kind = task_link_kind_label(link.kind),
                    relation = task_link_relation_label(link.relation),
                    title = escape(&link.title),
                    target = target,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let dependency_rows = if task.dependencies.is_empty() {
        r#"<tr><td colspan="2" class="empty">没有依赖。</td></tr>"#.to_string()
    } else {
        task.dependencies
            .iter()
            .map(|dependency| {
                format!(
                    r#"<tr><td><a href="/tasks/{id}">{title}</a></td><td>{state}</td></tr>"#,
                    id = dependency.task_id,
                    title = escape(&dependency.title),
                    state = task_state_label(dependency.state),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let operation_rows = if task.operations.is_empty() {
        r#"<tr><td colspan="3" class="empty">还没有操作。</td></tr>"#.to_string()
    } else {
        task.operations
            .iter()
            .map(|operation| {
                format!(
                    r#"<tr><td>{kind:?}</td><td><code>{actor}</code></td><td>{at}</td></tr>"#,
                    kind = operation.kind,
                    actor = escape(operation.actor.id()),
                    at = operation
                        .occurred_at
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let parent = task
        .parent_id
        .map(|id| format!(r#"<a href="/tasks/{id}"><code>{id}</code></a>"#))
        .unwrap_or_else(|| "无".to_string());
    let refresh = task_refresh_script(
        &format!("/tasks/{}/state", task.id),
        &task_fingerprint(task),
    );
    let body = format!(
        r#"<p><a href="/tasks">← 返回任务列表</a></p>
<h1>{title}</h1>
<p class="sub"><span class="badge">{state}</span> <span class="badge">{dispatch}</span></p>
<div class="meta">
  <span>流程 <code>{workflow} v{version}</code></span>
  <span>阶段 {stage}</span>
  <span>父任务 {parent}</span>
  <span>更新 {updated}</span>
</div>
<h2>角色与成员</h2>
<table>
<thead><tr><th>角色</th><th>类型</th><th>人数</th><th>Actor</th></tr></thead>
<tbody>{role_rows}</tbody>
</table>
<h2>关联对象</h2>
<table>
<thead><tr><th>类型</th><th>关系</th><th>标题</th><th>目标</th></tr></thead>
<tbody>{link_rows}</tbody>
</table>
<h2>依赖</h2>
<table>
<thead><tr><th>任务</th><th>状态</th></tr></thead>
<tbody>{dependency_rows}</tbody>
</table>
<h2>操作历史</h2>
<table>
<thead><tr><th>事件</th><th>Actor</th><th>时间</th></tr></thead>
<tbody>{operation_rows}</tbody>
</table>{refresh}"#,
        title = escape(&task.title),
        state = task_state_label(task.state),
        dispatch = task_dispatch_label(task.dispatch_state),
        workflow = escape(task.workflow.as_str()),
        version = task.workflow_version,
        stage = task
            .stage
            .as_ref()
            .map(|stage| escape(stage.as_str()))
            .unwrap_or_else(|| "—".to_string()),
        parent = parent,
        updated = task
            .updated_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        refresh = refresh,
    );
    layout(ctx, Section::Tasks, &task.title, &body)
}

#[derive(Debug, Clone)]
pub struct ProjectBriefDraftData {
    pub id: uuid::Uuid,
    pub revision: u64,
    pub body: ProjectBriefBody,
    pub updated_at: time::OffsetDateTime,
}

pub struct ProjectBriefPageData {
    pub revision: u64,
    pub status: ProjectBriefStatus,
    pub body: ProjectBriefBody,
    pub approved_by: Option<uuid::Uuid>,
    pub updated_at: time::OffsetDateTime,
    pub has_current: bool,
    pub drafts: Vec<ProjectBriefDraftData>,
    pub selected_draft: Option<ProjectBriefDraftData>,
    pub can_edit: bool,
    pub can_annotate: bool,
    pub can_approve: bool,
    pub annotations: Vec<ProjectBriefAnnotation>,
    pub error: Option<String>,
    pub validation: Vec<String>,
}

pub fn project_brief_page(
    ctx: &ConsoleContext,
    project: &Project,
    data: &ProjectBriefPageData,
) -> String {
    let editable = data.can_edit && data.selected_draft.is_some();
    let editor_body = data
        .selected_draft
        .as_ref()
        .map(|draft| &draft.body)
        .unwrap_or(&data.body);
    let ro = "";
    let metrics = editor_body
        .success_metrics
        .iter()
        .map(|metric| {
            format!(
                "{} | {} | {} | {} | {} | {}",
                metric.name,
                metric.target,
                metric.unit,
                metric.measurement,
                metric.window,
                metric
                    .baseline
                    .map(|value| value.to_string())
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let risks = editor_body
        .risks
        .iter()
        .map(|risk| {
            format!(
                "{} | {} | {} | {} | {}",
                risk.description,
                risk_level_str(risk.probability),
                risk_level_str(risk.impact),
                risk.mitigation,
                risk.owner
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let stakeholders = editor_body
        .stakeholders
        .iter()
        .map(|stakeholder| {
            format!(
                "{} | {} | {}",
                stakeholder.role,
                stakeholder.account,
                stakeholder.decision_rights.join(",")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (budget_amount, budget_currency, budget_note) = editor_body
        .budget
        .as_ref()
        .map(|budget| {
            (
                budget.amount.to_string(),
                budget.currency.clone(),
                budget.note.clone().unwrap_or_default(),
            )
        })
        .unwrap_or_default();
    let error_html = data
        .error
        .as_ref()
        .map(|message| format!(r#"<div class="error">{}</div>"#, escape(message)))
        .unwrap_or_default();
    let validation_html = if data.validation.is_empty() {
        String::new()
    } else {
        format!(
            r#"<div class="error"><strong>简报未通过校验：</strong><ul>{}</ul></div>"#,
            data.validation
                .iter()
                .map(|issue| format!("<li>{}</li>", escape(issue)))
                .collect::<String>()
        )
    };
    let approved = data
        .approved_by
        .map(|id| format!(r#"<span>批准人 <code>{id}</code></span>"#))
        .unwrap_or_default();
    let buttons = if editable {
        r#"<div class="actions">
  <button type="submit" name="action" value="save">保存草稿</button>
  <button type="submit" name="action" value="submit">提交评审</button>
</div>"#
            .to_string()
    } else {
        String::new()
    };
    let approve = if data.can_approve && data.status == ProjectBriefStatus::InReview && data.has_current {
        format!(
            r#"<form method="post" action="/projects/{id}/brief/approve">
  <input type="hidden" name="version" value="{version}">
  <div class="actions"><button type="submit">批准当前版本</button></div>
</form>"#,
            id = project.id,
            version = data.revision,
        )
    } else {
        String::new()
    };
    let status_note = if !data.has_current {
        "还没有提交过项目简报。先创建一个草稿并提交评审。"
    } else {
        match data.status {
            ProjectBriefStatus::Draft => "草稿可以编辑；提交后等待人类批准。",
            ProjectBriefStatus::InReview => {
                "已提交，先由文风 Reviewer 和内容 Reviewer 审阅；通过后等待 Maintainer/Owner 批准。"
            }
            ProjectBriefStatus::Approved => {
                "已批准，作为项目基线。变更需要通过新草稿提交新版本。"
            }
            ProjectBriefStatus::Superseded => "已被新版本取代。",
        }
    };

    let document = if data.has_current {
        project_brief_document(&data.body, data.can_annotate)
    } else {
        r#"<p class="empty">还没有提交过项目简报。</p>"#.to_string()
    };
    let editor = if editable {
        let draft = data.selected_draft.as_ref().expect("editable draft");
        format!(
            r#"<h2>编辑草稿</h2>
<form method="post" action="/projects/{project_id}/brief">
  <input type="hidden" name="draft_id" value="{draft_id}">
  <input type="hidden" name="revision" value="{revision}">
  <div class="field">
    <label for="problem">问题 / 为什么做</label>
    <textarea id="problem" name="problem"{ro}>{problem}</textarea>
  </div>
  <div class="field">
    <label for="goals">项目目标（每行一个）</label>
    <textarea id="goals" name="goals"{ro}>{goals}</textarea>
  </div>
  <div class="field">
    <label for="success_metrics">成功指标（每行：名称 | 目标值 | 单位 | 测量方式 | 窗口 | 基线（可选））</label>
    <textarea id="success_metrics" name="success_metrics"{ro}>{metrics}</textarea>
  </div>
  <div class="grid">
    <div class="field">
      <label for="in_scope">范围内（每行一个）</label>
      <textarea id="in_scope" name="in_scope"{ro}>{in_scope}</textarea>
    </div>
    <div class="field">
      <label for="out_of_scope">非目标（每行一个）</label>
      <textarea id="out_of_scope" name="out_of_scope"{ro}>{out_of_scope}</textarea>
    </div>
  </div>
  <div class="field">
    <label for="constraints">约束（每行一个）</label>
    <textarea id="constraints" name="constraints"{ro}>{constraints}</textarea>
  </div>
  <div class="field">
    <label for="risks">风险（每行：描述 | 概率(low/medium/high) | 影响(low/medium/high) | 缓解 | 负责人）</label>
    <textarea id="risks" name="risks"{ro}>{risks}</textarea>
  </div>
  <div class="field">
    <label for="stakeholders">干系人（每行：角色 | 账号 | 决策权（逗号分隔））</label>
    <textarea id="stakeholders" name="stakeholders"{ro}>{stakeholders}</textarea>
  </div>
  <div class="grid">
    <div class="field"><label for="budget_amount">预算金额（可选）</label>
      <input id="budget_amount" name="budget_amount" value="{budget_amount}"{ro}></div>
    <div class="field"><label for="budget_currency">币种</label>
      <input id="budget_currency" name="budget_currency" value="{budget_currency}"{ro}></div>
  </div>
  <div class="field"><label for="budget_note">预算说明（可选）</label>
    <input id="budget_note" name="budget_note" value="{budget_note}"{ro}></div>
  <div class="field">
    <label for="assumptions">假设（每行一个）</label>
    <textarea id="assumptions" name="assumptions"{ro}>{assumptions}</textarea>
  </div>
  <div class="field">
    <label for="open_questions">待决问题（每行一个）</label>
    <textarea id="open_questions" name="open_questions"{ro}>{open_questions}</textarea>
  </div>
  {buttons}
</form>"#,
            project_id = project.id,
            draft_id = draft.id,
            revision = draft.revision,
            ro = ro,
            problem = escape(&editor_body.problem),
            goals = escape(&editor_body.goals.join("\n")),
            metrics = escape(&metrics),
            in_scope = escape(&editor_body.scope.in_scope.join("\n")),
            out_of_scope = escape(&editor_body.scope.out_of_scope.join("\n")),
            constraints = escape(&editor_body.constraints.join("\n")),
            risks = escape(&risks),
            stakeholders = escape(&stakeholders),
            budget_amount = escape(&budget_amount),
            budget_currency = escape(&budget_currency),
            budget_note = escape(&budget_note),
            assumptions = escape(&editor_body.assumptions.join("\n")),
            open_questions = escape(&editor_body.open_questions.join("\n")),
            buttons = buttons,
        )
    } else {
        String::new()
    };

    let drafts = if data.drafts.is_empty() {
        r#"<p class="empty">没有草稿。</p>"#.to_string()
    } else {
        let mut out = String::from("<ul>");
        for draft in &data.drafts {
            out.push_str(&format!(
                r#"<li><a href="/projects/{project_id}/brief?draft={draft_id}">草稿 {draft_id}</a> · revision {revision} · 更新 {updated}</li>"#,
                project_id = project.id,
                draft_id = draft.id,
                revision = draft.revision,
                updated = draft
                    .updated_at
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default(),
            ));
        }
        out.push_str("</ul>");
        out
    };
    let create_draft = if data.can_edit {
        format!(
            r#"<form method="post" action="/projects/{id}/brief/drafts">
  <div class="actions"><button type="submit">新建草稿</button></div>
</form>"#,
            id = project.id,
        )
    } else {
        String::new()
    };

    let body = format!(
        r#"<p><a href="/projects/{project_id}">← 返回项目</a></p>
<h1>{project_name} · 项目简报</h1>
<p class="sub">项目级承诺：为什么做、目标、成功指标、范围和非目标、约束、风险、干系人。</p>
<div class="meta">
  <span>当前版本 <strong>{status}</strong></span>
  <span>版本 {revision}</span>
  <span>更新 {updated}</span>
  {approved}
</div>
<p class="hint">{status_note}</p>
{error_html}
{validation_html}
<h2>我的草稿</h2>
{create_draft}
{drafts}
{annotations}
{document}
{editor}
{approve}"#,
        project_id = project.id,
        project_name = escape(&project.name),
        status = if data.has_current {
            project_brief_status_label(data.status)
        } else {
            "无"
        },
        revision = if data.has_current {
            data.revision.to_string()
        } else {
            "—".to_string()
        },
        updated = if data.has_current {
            data.updated_at
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default()
        } else {
            "—".to_string()
        },
        approved = approved,
        status_note = status_note,
        error_html = error_html,
        validation_html = validation_html,
        create_draft = create_draft,
        drafts = drafts,
        annotations = if data.has_current {
            project_brief_annotations_html(project, &data.annotations, data.can_annotate)
        } else {
            String::new()
        },
        document = document,
        editor = editor,
        approve = approve,
    );
    layout(
        ctx,
        Section::Brief,
        &format!("{} · 项目简报", project.name),
        &body,
    )
}

fn brief_annotate_link(
    editable: bool,
    target: &str,
    target_id: Option<&str>,
    field: Option<&str>,
) -> String {
    if !editable {
        return String::new();
    }
    let mut attrs = format!(r#"data-annotate-type="{}""#, escape(target));
    if let Some(target_id) = target_id {
        attrs.push_str(&format!(r#" data-annotate-id="{}""#, escape(target_id)));
    }
    if let Some(field) = field {
        attrs.push_str(&format!(r#" data-annotate-field="{}""#, escape(field)));
    }
    format!(r##" <a href="#annotation-form" {attrs}>批注</a>"##)
}

fn project_brief_document(body: &ProjectBriefBody, editable: bool) -> String {
    let mut out = String::from(r#"<div class="brief">"#);
    out.push_str(&format!(
        "<h2>问题 / 为什么做{}</h2>",
        brief_annotate_link(editable, "problem", None, None)
    ));
    if body.problem.trim().is_empty() {
        out.push_str(r#"<p class="empty">未填写</p>"#);
    } else {
        out.push_str(r#"<div class="card markdown">"#);
        out.push_str(&markdown_to_html(&body.problem));
        out.push_str("</div>");
    }

    out.push_str(&format!(
        "<h2>项目目标{}</h2>",
        brief_annotate_link(editable, "goals", None, None)
    ));
    out.push_str(&brief_list(&body.goals));

    out.push_str(&format!(
        "<h2>成功指标{}</h2>",
        brief_annotate_link(editable, "success_metrics", None, None)
    ));
    if body.success_metrics.is_empty() {
        out.push_str(r#"<p class="empty">未填写</p>"#);
    } else {
        out.push_str(
            r#"<div class="table-wrap"><table><thead><tr><th>指标</th><th>基线</th><th>目标</th><th>单位</th><th>测量方式</th><th>窗口</th></tr></thead><tbody>"#,
        );
        for metric in &body.success_metrics {
            out.push_str(&format!(
                "<tr><td>{}{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                inline_markdown(&metric.name, 0),
                brief_annotate_link(editable, "success_metrics", Some(&metric.id), None),
                metric
                    .baseline
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "—".to_string()),
                metric.target,
                inline_markdown(&metric.unit, 0),
                inline_markdown(&metric.measurement, 0),
                inline_markdown(&metric.window, 0),
            ));
        }
        out.push_str("</tbody></table></div>");
    }

    out.push_str(&format!(
        r#"<h2>范围与非目标{}</h2><div class="two-col"><div><h3>范围内{}</h3>"#,
        brief_annotate_link(editable, "scope", None, None),
        brief_annotate_link(editable, "scope", None, Some("in_scope"))
    ));
    out.push_str(&brief_list(&body.scope.in_scope));
    out.push_str(&format!(
        r#"</div><div><h3>非目标{}</h3>"#,
        brief_annotate_link(editable, "scope", None, Some("out_of_scope"))
    ));
    out.push_str(&brief_list(&body.scope.out_of_scope));
    out.push_str("</div></div>");

    out.push_str(&format!(
        "<h2>约束{}</h2>",
        brief_annotate_link(editable, "constraints", None, None)
    ));
    out.push_str(&brief_list(&body.constraints));

    out.push_str(&format!(
        "<h2>风险{}</h2>",
        brief_annotate_link(editable, "risks", None, None)
    ));
    if body.risks.is_empty() {
        out.push_str(r#"<p class="empty">未填写</p>"#);
    } else {
        out.push_str(
            r#"<div class="table-wrap"><table><thead><tr><th>风险</th><th>概率</th><th>影响</th><th>缓解</th><th>负责人</th></tr></thead><tbody>"#,
        );
        for risk in &body.risks {
            out.push_str(&format!(
                "<tr><td>{}{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                inline_markdown(&risk.description, 0),
                brief_annotate_link(editable, "risks", Some(&risk.id), None),
                risk_level_label(risk.probability),
                risk_level_label(risk.impact),
                inline_markdown(&risk.mitigation, 0),
                inline_markdown(&risk.owner, 0),
            ));
        }
        out.push_str("</tbody></table></div>");
    }

    out.push_str(&format!(
        "<h2>干系人{}</h2>",
        brief_annotate_link(editable, "stakeholders", None, None)
    ));
    if body.stakeholders.is_empty() {
        out.push_str(r#"<p class="empty">未填写</p>"#);
    } else {
        out.push_str(
            r#"<div class="table-wrap"><table><thead><tr><th>角色</th><th>账号</th><th>决策权</th></tr></thead><tbody>"#,
        );
        for stakeholder in &body.stakeholders {
            let rights = if stakeholder.decision_rights.is_empty() {
                "—".to_string()
            } else {
                stakeholder.decision_rights.join("、")
            };
            out.push_str(&format!(
                "<tr><td>{}{}</td><td>{}</td><td>{}</td></tr>",
                inline_markdown(&stakeholder.role, 0),
                brief_annotate_link(editable, "stakeholders", Some(&stakeholder.id), None),
                inline_markdown(&stakeholder.account, 0),
                escape(&rights),
            ));
        }
        out.push_str("</tbody></table></div>");
    }

    out.push_str(&format!(
        "<h2>预算{}</h2>",
        brief_annotate_link(editable, "budget", None, None)
    ));
    if let Some(budget) = &body.budget {
        out.push_str(&format!(
            "<p><strong>{}</strong> {}{}</p>",
            budget.amount,
            escape(&budget.currency),
            budget
                .note
                .as_ref()
                .map(|note| format!("（{}）", escape(note)))
                .unwrap_or_default(),
        ));
    } else {
        out.push_str(r#"<p class="empty">未填写</p>"#);
    }

    out.push_str(&format!(
        "<h2>假设{}</h2>",
        brief_annotate_link(editable, "assumptions", None, None)
    ));
    out.push_str(&brief_list(&body.assumptions));
    out.push_str(&format!(
        "<h2>待决问题{}</h2>",
        brief_annotate_link(editable, "open_questions", None, None)
    ));
    out.push_str(&brief_list(&body.open_questions));
    out.push_str("</div>");
    out
}

fn project_brief_annotations_html(
    project: &Project,
    annotations: &[ProjectBriefAnnotation],
    editable: bool,
) -> String {
    let open: Vec<&ProjectBriefAnnotation> = annotations
        .iter()
        .filter(|annotation| annotation.status == AnnotationStatus::Open)
        .collect();
    let resolved: Vec<&ProjectBriefAnnotation> = annotations
        .iter()
        .filter(|annotation| annotation.status == AnnotationStatus::Resolved)
        .collect();
    let mut out = String::new();
    out.push_str(&format!("<h2>待处理批注（{}）</h2>", open.len()));
    if open.is_empty() {
        out.push_str(r#"<p class="empty">没有待处理批注。</p>"#);
    } else {
        for annotation in open {
            out.push_str(&project_brief_annotation_card(project, annotation, editable));
        }
    }
    if !resolved.is_empty() {
        out.push_str(&format!("<h2>历史批注（{}）</h2>", resolved.len()));
        for annotation in resolved {
            out.push_str(&project_brief_annotation_card(project, annotation, false));
        }
    }
    if editable {
        out.push_str(&format!(
            r#"<h2>添加批注</h2>
<form id="annotation-form" method="post" action="/projects/{project_id}/brief/annotations">
  <div class="grid">
    <div class="field"><label for="target_type">目标</label>
      <select id="target_type" name="target_type">
        <option value="document">整个简报</option>
        <option value="problem">问题 / 为什么做</option>
        <option value="goals">项目目标</option>
        <option value="success_metrics">成功指标</option>
        <option value="scope">范围与非目标</option>
        <option value="constraints">约束</option>
        <option value="risks">风险</option>
        <option value="stakeholders">干系人</option>
        <option value="budget">预算</option>
        <option value="assumptions">假设</option>
        <option value="open_questions">待决问题</option>
      </select>
    </div>
    <div class="field"><label for="target_id">对象 ID（可选）</label>
      <input id="target_id" name="target_id" placeholder="例如指标、风险或干系人的 id"></div>
  </div>
  <div class="field"><label for="annotation-field">字段（可选）</label>
    <input id="annotation-field" name="field" placeholder="例如 target / mitigation / decision_rights"></div>
  <div class="field"><label for="annotation-body">批注</label>
    <textarea id="annotation-body" name="body" required placeholder="指出不清楚、不一致或需要修改的地方"></textarea></div>
  <div class="actions"><button type="submit">添加批注</button></div>
</form>"#,
            project_id = project.id,
        ));
        out.push_str(
            r#"<script>
(() => {
  const form = document.getElementById('annotation-form');
  if (!form) return;
  document.querySelectorAll('[data-annotate-type]').forEach((link) => {
    link.addEventListener('click', () => {
      form.querySelector('[name="target_type"]').value = link.dataset.annotateType;
      form.querySelector('[name="target_id"]').value = link.dataset.annotateId || '';
      form.querySelector('[name="field"]').value = link.dataset.annotateField || '';
      form.querySelector('[name="body"]').focus();
    });
  });
})();
</script>"#,
        );
    }
    out
}

fn project_brief_annotation_card(
    project: &Project,
    annotation: &ProjectBriefAnnotation,
    can_resolve: bool,
) -> String {
    let mut target = project_brief_annotation_target_label(annotation.target_type).to_string();
    if let Some(target_id) = &annotation.target_id {
        target.push_str(&format!(" · {}", escape(target_id)));
    }
    if let Some(field) = &annotation.field {
        target.push_str(&format!(" · {}", escape(field)));
    }
    let resolve = if can_resolve && annotation.status == AnnotationStatus::Open {
        format!(
            r#"<form class="inline-form" method="post" action="/projects/{project_id}/brief/annotations/{annotation_id}/resolve">
  <input name="resolution" placeholder="解决说明" required>
  <button class="secondary" type="submit">标记已解决</button>
</form>"#,
            project_id = project.id,
            annotation_id = annotation.id,
        )
    } else {
        String::new()
    };
    let resolution = if annotation.status == AnnotationStatus::Resolved {
        let at = annotation
            .resolved_at
            .map(|value| {
                value
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        format!(
            r#"<div class="meta">已解决 · {at}</div><div class="markdown">{}</div>"#,
            markdown_to_html(annotation.resolution.as_deref().unwrap_or("")),
        )
    } else {
        String::new()
    };
    let author = format!(
        "{} (@{})",
        escape(&annotation.author_name),
        escape(&annotation.author_handle)
    );
    format!(
        r#"<div class="card annotation">
<div class="meta"><strong>{author}</strong> · {at} · <span class="annotation-target">{target}</span></div>
<div class="markdown">{body}</div>
{resolution}
{resolve}
</div>"#,
        author = author,
        at = annotation
            .created_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        target = target,
        body = markdown_to_html(&annotation.body),
        resolution = resolution,
        resolve = resolve,
    )
}

fn project_brief_annotation_target_label(
    target: foundry_domain::ProjectBriefAnnotationTarget,
) -> &'static str {
    use foundry_domain::ProjectBriefAnnotationTarget::*;
    match target {
        Document => "整个简报",
        Problem => "问题 / 为什么做",
        Goals => "项目目标",
        SuccessMetrics => "成功指标",
        Scope => "范围与非目标",
        Constraints => "约束",
        Risks => "风险",
        Stakeholders => "干系人",
        Budget => "预算",
        Assumptions => "假设",
        OpenQuestions => "待决问题",
    }
}

fn brief_list(items: &[String]) -> String {
    if items.is_empty() {
        return r#"<p class="empty">无</p>"#.to_string();
    }
    let mut out = String::from("<ul>");
    for item in items {
        out.push_str("<li>");
        out.push_str(&inline_markdown(item, 0));
        out.push_str("</li>");
    }
    out.push_str("</ul>");
    out
}

fn risk_level_label(level: RiskLevel) -> &'static str {
    match level {
        RiskLevel::Low => "低",
        RiskLevel::Medium => "中",
        RiskLevel::High => "高",
    }
}

pub fn requirements_document_page(
    ctx: &ConsoleContext,
    project: &Project,
    body: &RequirementsDocumentBody,
    current_version: u32,
    latest_version: Option<u32>,
    draft_revision: Option<u64>,
    editable: bool,
    can_publish: bool,
    annotations: &[RequirementsAnnotation],
    error: Option<&str>,
) -> String {
    let error_html = error
        .map(|message| format!(r#"<div class="error">{}</div>"#, escape(message)))
        .unwrap_or_default();
    let publish = if can_publish {
        draft_revision
            .map(|revision| {
                format!(
                    r#"<form method="post" action="/projects/{id}/requirements/publish">
  <input type="hidden" name="revision" value="{revision}">
  <div class="actions"><button type="submit">发布当前草稿</button></div>
</form>"#,
                    id = project.id,
                    revision = revision,
                )
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    let reject = if can_publish {
        format!(
            r#"<h2>人工评审</h2>
<form method="post" action="/projects/{id}/requirements/reject">
  <div class="field"><label for="reject-reason">打回原因</label>
    <textarea id="reject-reason" name="reason" required placeholder="说明需要修改的地方；会作为文档注释保存"></textarea></div>
  <div class="actions"><button class="danger" type="submit">打回给 Analyst</button></div>
</form>"#,
            id = project.id,
        )
    } else {
        String::new()
    };
    let page = format!(
        r#"<p><a href="/projects/{id}">← 返回项目</a> · <a href="/projects/{id}/brief">项目简报</a></p>
<h1>{name} · 需求文档</h1>
<p class="sub">用户、功能、故事、操作的结构化需求模型。Agent 编辑草稿，人类发布版本。</p>
<div class="meta">
  <span>已发布版本 <strong>{current_version}</strong></span>
  <span>最新版本 <strong>{latest_version}</strong></span>
  <span>草稿 revision <strong>{draft_revision}</strong></span>
</div>
{error_html}
{publish}
{reject}
{annotations}
{document}"#,
        id = project.id,
        name = escape(&project.name),
        current_version = current_version,
        latest_version = latest_version
            .map(|version| version.to_string())
            .unwrap_or_else(|| "—".to_string()),
        draft_revision = draft_revision
            .map(|revision| revision.to_string())
            .unwrap_or_else(|| "—".to_string()),
        error_html = error_html,
        publish = publish,
        reject = reject,
        annotations = requirements_annotations_html(project, annotations, editable),
        document = requirements_document_html(body, editable),
    );
    layout(
        ctx,
        Section::Projects,
        &format!("{} · 需求文档", project.name),
        &page,
    )
}

fn requirements_document_html(body: &RequirementsDocumentBody, editable: bool) -> String {
    let mut out = String::from(r#"<div class="brief">"#);
    out.push_str("<h2>用户</h2>");
    if body.users.is_empty() {
        out.push_str(r#"<p class="empty">尚未填写</p>"#);
    } else {
        out.push_str(
            r#"<div class="table-wrap"><table><thead><tr><th>ID</th><th>名称</th><th>目标</th><th>痛点</th><th>证据</th><th>置信度</th><th>状态</th></tr></thead><tbody>"#,
        );
        for user in &body.users {
            let annotate = if editable {
                format!(
                    r##"<br><a href="#annotation-form" data-annotate-type="user" data-annotate-id="{}">注释</a>"##,
                    escape(&user.id)
                )
            } else {
                String::new()
            };
            out.push_str(&format!(
                "<tr><td><code>{}</code>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape(&user.id),
                annotate,
                inline_markdown(&user.name, 0),
                escape(&join_markdown(&user.goals)),
                escape(&join_markdown(&user.pain_points)),
                escape(&join_markdown(&user.evidence)),
                confidence_label(user.confidence),
                entity_status_label(user.status),
            ));
        }
        out.push_str("</tbody></table></div>");
    }

    out.push_str("<h2>功能</h2>");
    if body.functions.is_empty() {
        out.push_str(r#"<p class="empty">尚未填写</p>"#);
        out.push_str("</div>");
        return out;
    }
    for function in &body.functions {
        let annotate = if editable {
            format!(
                r##"<p><a href="#annotation-form" data-annotate-type="function" data-annotate-id="{}" data-annotate-field="description">注释</a></p>"##,
                escape(&function.id)
            )
        } else {
            String::new()
        };
        out.push_str(&format!(
            r#"<div class="card"><h3><code>{}</code> · {}</h3>{}"#,
            escape(&function.id),
            inline_markdown(&function.name, 0),
            annotate,
        ));
        if let Some(description) = function.description.as_deref() {
            out.push_str(&format!(r#"<div class="markdown">{}</div>"#, markdown_to_html(description)));
        }
        out.push_str(&format!(
            r#"<p class="hint">状态：{}</p>"#,
            entity_status_label(function.status)
        ));

        if function.stories.is_empty() {
            out.push_str(r#"<p class="empty">没有故事</p>"#);
        } else {
            out.push_str("<h4>故事</h4>");
            out.push_str(
                r#"<div class="table-wrap"><table><thead><tr><th>故事</th><th>用户</th><th>场景 / 目标</th><th>操作</th><th>优先级</th><th>状态</th></tr></thead><tbody>"#,
            );
            for story in &function.stories {
                let annotate = if editable {
                    format!(
                        r##" <a href="#annotation-form" data-annotate-type="story" data-annotate-id="{}" data-annotate-field="goal">注释</a>"##,
                        escape(&story.id)
                    )
                } else {
                    String::new()
                };
                out.push_str(&format!(
                    "<tr><td><code>{}</code><br>{}{}</td><td>{}</td><td>{}<br>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    escape(&story.id),
                    inline_markdown(&story.title, 0),
                    annotate,
                    escape(&join_markdown(&story.user_ids)),
                    escape(&story.scenario),
                    escape(&story.goal),
                    escape(&join_markdown(&story.operation_ids)),
                    story_priority_label(story.priority),
                    entity_status_label(story.status),
                ));
            }
            out.push_str("</tbody></table></div>");
        }

        if function.operations.is_empty() {
            out.push_str(r#"<p class="empty">没有操作</p>"#);
        } else {
            out.push_str("<h4>操作</h4>");
            out.push_str(
                r#"<div class="table-wrap"><table><thead><tr><th>操作</th><th>参与者</th><th>触发</th><th>输入</th><th>输出</th><th>状态</th></tr></thead><tbody>"#,
            );
            for operation in &function.operations {
                let annotate = if editable {
                    format!(
                        r##" <a href="#annotation-form" data-annotate-type="operation" data-annotate-id="{}" data-annotate-field="trigger">注释</a>"##,
                        escape(&operation.id)
                    )
                } else {
                    String::new()
                };
                out.push_str(&format!(
                    "<tr><td><code>{}</code><br>{}{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    escape(&operation.id),
                    inline_markdown(&operation.name, 0),
                    annotate,
                    escape(&join_markdown(&operation.actor_user_ids)),
                    escape(operation.trigger.as_deref().unwrap_or("—")),
                    escape(&join_markdown(&operation.inputs)),
                    escape(&join_markdown(&operation.outputs)),
                    entity_status_label(operation.status),
                ));
            }
            out.push_str("</tbody></table></div>");
        }
        out.push_str("</div>");
    }
    out.push_str("</div>");
    out
}

fn requirements_annotations_html(
    project: &Project,
    annotations: &[RequirementsAnnotation],
    editable: bool,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("<h2>待处理注释（{}）</h2>", annotations.len()));
    if annotations.is_empty() {
        out.push_str(r#"<p class="empty">没有待处理注释。</p>"#);
    } else {
        for annotation in annotations {
            let target = match &annotation.target_id {
                Some(target_id) => {
                    let mut label = format!(
                        "{} {}",
                        annotation.target_type.as_str(),
                        escape(target_id)
                    );
                    if let Some(field) = &annotation.field {
                        label.push_str(&format!(" · {}", escape(field)));
                    }
                    label
                }
                None => "整个文档".to_string(),
            };
            let resolve = if editable {
                format!(
                    r#"<form class="inline-form" method="post" action="/projects/{project_id}/requirements/annotations/{annotation_id}/resolve">
  <input name="resolution" placeholder="解决说明" required>
  <button class="secondary" type="submit">标记已解决</button>
</form>"#,
                    project_id = project.id,
                    annotation_id = annotation.id,
                )
            } else {
                String::new()
            };
            out.push_str(&format!(
                r#"<div class="card annotation">
<div class="meta"><strong>{author}</strong> · {at} · <span class="annotation-target">{target}</span></div>
<div class="markdown">{body}</div>
{resolve}
</div>"#,
                author = format!(
                    "{} (@{})",
                    escape(&annotation.author_name),
                    escape(&annotation.author_handle)
                ),
                at = annotation
                    .created_at
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default(),
                target = target,
                body = markdown_to_html(&annotation.body),
                resolve = resolve,
            ));
        }
    }
    if editable {
        out.push_str(&format!(
            r#"<h2>添加注释</h2>
<form id="annotation-form" method="post" action="/projects/{project_id}/requirements/annotations">
  <div class="grid">
    <div class="field"><label for="target_type">目标类型</label>
      <select id="target_type" name="target_type">
        <option value="document">整个文档</option>
        <option value="user">用户</option>
        <option value="function">功能</option>
        <option value="story">故事</option>
        <option value="operation">操作</option>
      </select>
    </div>
    <div class="field"><label for="target_id">目标 ID</label>
      <input id="target_id" name="target_id" placeholder="例如 F1 / S1 / O1"></div>
  </div>
  <div class="field"><label for="annotation-field">字段（可选）</label>
    <input id="annotation-field" name="field" placeholder="例如 goal / inputs / description"></div>
  <div class="field"><label for="annotation-body">注释</label>
    <textarea id="annotation-body" name="body" required placeholder="指出不清楚或需要修改的地方"></textarea></div>
  <div class="actions"><button type="submit">添加注释</button></div>
</form>"#,
            project_id = project.id,
        ));
        out.push_str(
            r#"<script>
(() => {
  const form = document.getElementById('annotation-form');
  if (!form) return;
  document.querySelectorAll('[data-annotate-type]').forEach((link) => {
    link.addEventListener('click', () => {
      form.querySelector('[name="target_type"]').value = link.dataset.annotateType;
      form.querySelector('[name="target_id"]').value = link.dataset.annotateId || '';
      form.querySelector('[name="field"]').value = link.dataset.annotateField || '';
      form.querySelector('[name="body"]').focus();
    });
  });
})();
</script>"#,
        );
    }
    out
}

fn join_markdown(items: &[String]) -> String {
    if items.is_empty() {
        "—".to_string()
    } else {
        items.join("、")
    }
}

fn confidence_label(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::Low => "低",
        Confidence::Medium => "中",
        Confidence::High => "高",
    }
}

fn story_priority_label(priority: StoryPriority) -> &'static str {
    match priority {
        StoryPriority::Must => "必须",
        StoryPriority::Should => "应该",
        StoryPriority::Could => "可以",
    }
}

fn entity_status_label(status: EntityStatus) -> &'static str {
    match status {
        EntityStatus::Draft => "草稿",
        EntityStatus::Active => "生效",
        EntityStatus::Deprecated => "弃用",
        EntityStatus::Retired => "退役",
    }
}

fn project_brief_status_label(status: ProjectBriefStatus) -> &'static str {
    match status {
        ProjectBriefStatus::Draft => "草稿",
        ProjectBriefStatus::InReview => "待批准",
        ProjectBriefStatus::Approved => "已批准",
        ProjectBriefStatus::Superseded => "已替代",
    }
}

fn risk_level_str(level: RiskLevel) -> &'static str {
    match level {
        RiskLevel::Low => "low",
        RiskLevel::Medium => "medium",
        RiskLevel::High => "high",
    }
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
        .filter(|issue| issue.status == IssueStatus::InProgress)
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
  <span>待处理 <strong>{fresh}</strong></span>
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
<div class="markdown">{body}</div>
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
                    body = markdown_to_html(&comment.body),
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
<h2>描述</h2><div class="card markdown">{description}</div>
{bug_details}
{resolution}
{duplicate}
<h2>讨论</h2>
{comments}
{comment_form}
{transition_form}
<h2>状态历史</h2><ul>{history}</ul>
{auto_refresh}"#,
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
        description = markdown_to_html(&issue.description),
        bug_details = bug_details,
        resolution = resolution,
        duplicate = duplicate,
        comments = comments,
        comment_form = comment_form,
        transition_form = transition_form,
        history = history,
        auto_refresh = issue_refresh_script(issue),
    );
    layout(ctx, Section::Issues, &issue.title, &body)
}

/// Reload the Issue page when the Issue version or comment count changes,
/// unless the viewer is currently editing a form.
fn issue_refresh_script(issue: &Issue) -> String {
    format!(
        r#"<script>
(() => {{
  const issueId = "{id}";
  let known = "{version}:{comments}";
  let busy = false;
  const dirty = () => Array.from(
    document.querySelectorAll('main textarea, main input[type="text"]')
  ).some((element) => element.value.trim().length > 0);
  const tick = async () => {{
    if (busy || document.hidden || dirty()) return;
    busy = true;
    try {{
      const response = await fetch(`/issues/${{issueId}}/state`, {{
        cache: 'no-store',
        credentials: 'same-origin'
      }});
      if (response.ok) {{
        const next = (await response.text()).trim();
        if (next !== known && !dirty()) {{
          location.reload();
          return;
        }}
      }}
    }} catch (_) {{}}
    busy = false;
  }};
  setInterval(tick, 3000);
  document.addEventListener('visibilitychange', () => {{
    if (!document.hidden) tick();
  }});
}})();
</script>"#,
        id = issue.id,
        version = issue.version,
        comments = issue.comments.len(),
    )
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
<p class="sub">把发生的事说清楚；分类和严重度在创建时一并填写。</p>
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
<td><a href="/projects/{id}">{name}</a> · <a href="/projects/{id}/brief">简报</a></td>
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
</form>
<h2>删除项目</h2>
<form method="post" action="/projects/{id}/delete" onsubmit="return confirm('删除项目会永久删除项目及其任务、需求、议题、简报、成员和事件记录，且无法恢复。确定继续吗？');">
  <p class="hint">只有项目 Owner 或全局管理员可以删除项目。删除后无法恢复。</p>
  <div class="actions"><button class="danger" type="submit">删除项目</button></div>
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
<p><a class="button secondary" href="/projects/{id}/brief">项目简报</a> <a class="button secondary" href="/projects/{id}/requirements">需求文档</a></p>
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

pub fn agents_page(ctx: &ConsoleContext, overview: &AgentOverview, error: Option<&str>) -> String {
    let error_html = error
        .map(|message| format!(r#"<div class="error">{}</div>"#, escape(message)))
        .unwrap_or_default();
    let processes: std::collections::HashMap<_, _> = overview
        .processes
        .iter()
        .map(|process| (process.agent_definition_id, process))
        .collect();
    let definition_rows = if overview.definitions.is_empty() {
        r#"<tr><td colspan="6" class="empty">平台还没有注册智能体。</td></tr>"#.to_string()
    } else {
        overview
            .definitions
            .iter()
            .map(|definition| {
                let process = processes.get(&definition.id);
                let status = process
                    .map(|process| process.status.as_str())
                    .unwrap_or("stopped");
                let heartbeat = process
                    .and_then(|process| process.heartbeat_at)
                    .map(|at| {
                        at.format(&time::format_description::well_known::Rfc3339)
                            .unwrap_or_default()
                    })
                    .unwrap_or_else(|| "—".to_string());
                let last_error = process
                    .and_then(|process| process.last_error.as_deref())
                    .map(escape)
                    .unwrap_or_default();
                let can_start = !matches!(status, "running" | "starting");
                let start = if can_start {
                    format!(
                        r#"<form class="inline-form" method="post" action="/agents/{id}/start">
<button type="submit">启动</button></form>"#,
                        id = definition.id,
                    )
                } else {
                    String::new()
                };
                let stop = if matches!(status, "running" | "starting" | "idle") {
                    format!(
                        r#"<form class="inline-form" method="post" action="/agents/{id}/stop">
<button class="danger" type="submit">停止</button></form>"#,
                        id = definition.id,
                    )
                } else {
                    String::new()
                };
                let restart = format!(
                    r#"<form class="inline-form" method="post" action="/agents/{id}/restart">
<button class="secondary" type="submit">重启</button></form>"#,
                    id = definition.id,
                );
                let logs = format!(
                    r#"<a href="/agents/{id}/logs">日志</a>"#,
                    id = definition.id
                );
                format!(
                    r#"<tr>
<td><strong>{name}</strong><br><code>@{account}</code><br><span class="hint">{role}</span></td>
<td><code>{model}</code><br><span class="hint">{base_url}</span></td>
<td>{enabled}</td>
<td><span class="badge">{status}</span></td>
<td>{heartbeat}<br><span class="hint">{last_error}</span></td>
<td>{logs} {start}{stop}{restart}</td>
</tr>"#,
                    name = escape(&definition.name),
                    account = escape(&definition.account_handle),
                    role = escape(&definition.role),
                    model = escape(&definition.model_name),
                    base_url = escape(&definition.model_base_url),
                    enabled = if definition.enabled {
                        "已启用"
                    } else {
                        "已停用"
                    },
                    status = agent_process_status_label(status),
                    heartbeat = heartbeat,
                    last_error = last_error,
                    logs = logs,
                    start = start,
                    stop = stop,
                    restart = restart,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let body = format!(
        r#"<h1>智能体</h1>
<p class="sub">智能体由平台定义，用户不能新建或修改；这里只管理运行状态。</p>
{error_html}
<p class="hint">目标澄清任务（<code>goal_clarification / clarifier</code>）由 PM Agent 自动承接。</p>
<table>
<thead><tr><th>名称 / 账户</th><th>模型</th><th>启用</th><th>进程</th><th>心跳 / 错误</th><th></th></tr></thead>
<tbody>{definition_rows}</tbody>
</table>"#,
        error_html = error_html,
        definition_rows = definition_rows,
    );
    layout(ctx, Section::Agents, "智能体", &body)
}

pub fn agent_logs_page(
    ctx: &ConsoleContext,
    definition: &AgentDefinition,
    log: &str,
) -> String {
    let content = if log.trim().is_empty() {
        r#"<p class="empty">还没有日志。智能体启动后，进程的 stdout/stderr 会写入这里。</p>"#
            .to_string()
    } else {
        format!(r#"<pre class="agent-log">{}</pre>"#, escape(log))
    };
    let body = format!(
        r#"<p class="hint"><a href="/agents">← 返回智能体</a> · <a href="/agents/{id}/logs">刷新</a></p>
<h1>{name} 日志</h1>
<p class="sub"><code>@{account}</code> · <code>{role}</code> · 最近 256 KB</p>
{content}"#,
        id = definition.id,
        name = escape(&definition.name),
        account = escape(&definition.account_handle),
        role = escape(&definition.role),
    );
    layout(ctx, Section::Agents, "智能体日志", &body)
}

fn agent_process_status_label(status: &str) -> &'static str {
    match status {
        "starting" => "启动中",
        "running" => "运行中",
        "idle" => "空闲",
        "stopped" => "已停止",
        "failed" => "失败",
        _ => "未知",
    }
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

fn task_state_label(state: TaskState) -> &'static str {
    match state {
        TaskState::Pending => "待处理",
        TaskState::InProgress => "进行中",
        TaskState::Completed => "已完成",
    }
}

fn task_dispatch_label(state: TaskDispatchState) -> &'static str {
    match state {
        TaskDispatchState::Blocked => "阻塞",
        TaskDispatchState::Ready => "就绪",
        TaskDispatchState::Running => "运行中",
        TaskDispatchState::Closed => "已关闭",
    }
}

fn role_kind_label(kind: RoleKind) -> &'static str {
    match kind {
        RoleKind::Human => "人类",
        RoleKind::Agent => "智能体",
        RoleKind::Any => "任意",
    }
}

fn task_link_kind_label(kind: TaskLinkKind) -> &'static str {
    match kind {
        TaskLinkKind::Issue => "议题",
        TaskLinkKind::Requirement => "需求",
        TaskLinkKind::Design => "设计",
        TaskLinkKind::Adr => "ADR",
        TaskLinkKind::Contract => "契约",
        TaskLinkKind::Decision => "决策",
        TaskLinkKind::Task => "任务",
        TaskLinkKind::Artifact => "产物",
        TaskLinkKind::Url => "链接",
    }
}

fn task_link_relation_label(relation: TaskLinkRelation) -> &'static str {
    match relation {
        TaskLinkRelation::Clarifies => "澄清",
        TaskLinkRelation::Implements => "实现",
        TaskLinkRelation::DependsOn => "依赖",
        TaskLinkRelation::References => "引用",
        TaskLinkRelation::Produces => "产出",
        TaskLinkRelation::Verifies => "验证",
        TaskLinkRelation::Related => "相关",
    }
}

fn issue_status_label(status: IssueStatus) -> &'static str {
    use IssueStatus::*;
    match status {
        Open => "待处理",
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

    #[test]
    fn markdown_renders_basic_blocks_and_escapes_html() {
        let html = markdown_to_html(
            "## 标题\n\n**粗体** 和 `code`\n\n- 一\n- 二\n\n```js\nalert(1)\n```\n\n[链接](https://example.com)\n<script>",
        );
        assert!(html.contains("<h2>标题</h2>"));
        assert!(html.contains("<strong>粗体</strong>"));
        assert!(html.contains("<code>code</code>"));
        assert!(html.contains("<li>一</li>"));
        assert!(html.contains(r#"<pre><code class="language-js">alert(1)</code></pre>"#));
        assert!(html.contains(r#"href="https://example.com""#));
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn markdown_renders_tables() {
        let html = markdown_to_html("| A | B |\n| --- | ---: |\n| 1 | **2** |\n");
        assert!(html.contains(r#"<div class="table-wrap"><table>"#));
        assert!(html.contains("<th>A</th>"));
        assert!(html.contains(r#"<th style="text-align:right">B</th>"#));
        assert!(html.contains("<td>1</td>"));
        assert!(html.contains(r#"<td style="text-align:right"><strong>2</strong></td>"#));
    }

    #[test]
    fn project_brief_document_renders_structured_sections() {
        let body = ProjectBriefBody {
            problem: "因为 <原因>".to_string(),
            goals: vec!["目标一".to_string()],
            success_metrics: vec![foundry_domain::SuccessMetric {
                name: "转化率".to_string(),
                baseline: Some(1.0),
                target: 2.0,
                unit: "%".to_string(),
                measurement: "埋点".to_string(),
                window: "30 天".to_string(),
                ..Default::default()
            }],
            risks: vec![foundry_domain::Risk {
                description: "风险一".to_string(),
                probability: RiskLevel::High,
                impact: RiskLevel::Medium,
                ..Default::default()
            }],
            stakeholders: vec![foundry_domain::Stakeholder {
                role: "用户".to_string(),
                account: "团队".to_string(),
                decision_rights: vec!["验收".to_string()],
                ..Default::default()
            }],
            ..Default::default()
        };

        let html = project_brief_document(&body, false);

        assert!(html.contains("问题 / 为什么做"));
        assert!(html.contains("&lt;原因&gt;"));
        assert!(html.contains("<h2>项目目标</h2>"));
        assert!(html.contains("转化率"));
        assert!(html.contains("<h2>风险</h2>"));
        assert!(html.contains("高"));
        assert!(html.contains("<h2>干系人</h2>"));
        assert!(html.contains("验收"));

        let editable = project_brief_document(&body, true);
        assert!(editable.contains(r#"data-annotate-type="problem""#));
        assert!(editable.contains(r#"data-annotate-type="success_metrics""#));
        assert!(editable.contains(r#"data-annotate-type="stakeholders""#));
    }

    #[test]
    fn markdown_rejects_unsafe_links() {
        let html = markdown_to_html("[x](javascript:alert(1))");
        assert!(!html.contains(r#"href="javascript"#));
    }

    #[test]
    fn project_page_offers_delete_only_to_managers() {
        let owner = Account::new(
            "owner".to_string(),
            "Owner".to_string(),
            AccountKind::Human,
            AccountRole::Member,
            None,
            Some("hash".to_string()),
        )
        .unwrap();
        let project = Project::new("Project", None, owner.id).unwrap();
        let ctx = ConsoleContext::new(&owner, std::slice::from_ref(&project), Some(&project));

        let manager_html = project_page(&ctx, &project, &[], true, None);
        assert!(manager_html.contains(&format!("/projects/{}/delete", project.id)));
        assert!(manager_html.contains("删除项目"));

        let member_html = project_page(&ctx, &project, &[], false, None);
        assert!(!member_html.contains("/delete"));
    }

    #[test]
    fn agent_logs_page_escapes_process_output() {
        let account = Account::new(
            "admin".to_string(),
            "Admin".to_string(),
            AccountKind::Human,
            AccountRole::Admin,
            None,
            Some("hash".to_string()),
        )
        .unwrap();
        let ctx = ConsoleContext::new(&account, &[], None);
        let definition = AgentDefinition {
            id: uuid::Uuid::now_v7(),
            name: "Clarifier <script>".to_string(),
            account_id: account.id,
            account_handle: "agent".to_string(),
            role: "goal_clarification:clarifier".to_string(),
            model_base_url: "http://127.0.0.1:11434/v1".to_string(),
            model_name: "test-model".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            system_prompt: String::new(),
            tools: Vec::new(),
            enabled: true,
            created_at: time::OffsetDateTime::now_utc(),
            updated_at: time::OffsetDateTime::now_utc(),
        };
        let log = "[info] tool <script>\n<b>bad</b>\n";

        let html = agent_logs_page(&ctx, &definition, log);

        assert!(html.contains("Clarifier &lt;script&gt;"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("&lt;b&gt;bad&lt;/b&gt;"));
        assert!(!html.contains("<script>"));
    }
}
