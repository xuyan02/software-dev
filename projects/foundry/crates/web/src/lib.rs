//! Server-rendered HTML console.
//!
//! The console is a thin, dependency-light view over the same API agents use.
//! It intentionally has no JavaScript build step: HTML + a small stylesheet is
//! enough for humans to observe and act.

use foundry_domain::{
    Account, AccountKind, AccountRole, AccountStatus, AgentDefinition, AgentOverview,
    CapsuleProfile, Issue, IssueClassification, IssueConfidence, IssueKind, IssueSeverity,
    IssueSourceKind, IssueStatus, MergeRequestChangeStatus, MergeRequestDetail, MergeRequestReview,
    MergeRequestStatus, Project, ProjectRole, ProjectStatus, RoleKind, Task, TaskDispatchState,
    TaskLink, TaskLinkKind, TaskLinkRelation, TaskState, Trigger, WorkflowDefinition,
    WorkflowOption, WorkflowOwner,
};
use serde::Serialize;
use std::collections::BTreeSet;

/// Vendored Mermaid bundle served at `/assets/mermaid.min.js`.
pub const MERMAID_SCRIPT: &str = include_str!("../../../web/vendor/mermaid.min.js");

/// One design-space element as sent to the tree endpoint and rendered in the
/// console. `hash` addresses the immutable element object.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignSpaceElement {
    pub hash: String,
    pub qname: String,
    pub name: String,
    pub path: String,
    /// Language prefix from the type tag (`rust`, `cpp`, ...), empty for files.
    pub lang: String,
    /// Kind part of the type tag (`function`, `class`, `file`, ...).
    pub kind: String,
    pub start_line: u32,
    pub end_line: u32,
    pub has_design: bool,
    pub is_file: bool,
    /// True for design-structure roots, systems, modules and layers.
    pub is_structure: bool,
    /// Direct code member qualified names; empty for non-container kinds.
    pub members: Vec<String>,
}

/// Everything the tree view needs in one payload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignSpaceOverview {
    pub branch: String,
    /// Version hex, absent when the branch has no version yet.
    pub version: Option<String>,
    pub source_commit: String,
    pub file_count: usize,
    pub element_count: usize,
    pub document_count: usize,
    pub designed_count: usize,
    pub elements: Vec<DesignSpaceElement>,
}

/// Element metadata plus its design text.
#[derive(Debug, Clone)]
pub struct DesignSpaceDetail {
    pub hash: String,
    pub name: String,
    pub lang: String,
    pub kind: String,
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub design: Option<String>,
    pub comments: Vec<DesignSpaceComment>,
    /// Direct modules or code members of a system/module.
    pub members: Vec<DesignSpaceMember>,
    /// Section outline for a document whose body is loaded on demand.
    pub sections: Vec<DesignSpaceSection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DesignSpaceSection {
    pub qname: String,
    pub title: String,
    pub children: Vec<DesignSpaceSection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DesignSpaceMember {
    pub hash: String,
    pub qname: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone)]
pub struct DesignSpaceComment {
    pub id: u32,
    pub resolved: bool,
    pub target: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct DesignSpaceLine {
    pub number: u32,
    pub text: String,
    pub in_element: bool,
}

#[derive(Debug, Clone)]
pub struct DesignSpaceSource {
    pub commit: String,
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub first_line: u32,
    pub last_line: u32,
    pub total_lines: u32,
    pub lines: Vec<DesignSpaceLine>,
}

const STYLE: &str = r#"
:root { color-scheme: light dark; --fg:#1b1f23; --muted:#57606a; --bg:#ffffff;
        --panel:#f6f8fa; --line:#d0d7de; --accent:#0969da; --accent-soft:rgba(9,105,218,.09);
        --annotation-bg:#fff8c5; --annotation-border:#bf8700; --annotation-fg:#6b4e00; }
@media (prefers-color-scheme: dark) {
  :root { --fg:#e6edf3; --muted:#8b949e; --bg:#0d1117; --panel:#161b22;
          --line:#30363d; --accent:#4493f8; --accent-soft:rgba(68,147,248,.13);
          --annotation-bg:#3d2e00; --annotation-border:#d4a72c; --annotation-fg:#f2cc60; }
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
.details { display:grid; grid-template-columns:max-content minmax(0,1fr); gap:10px 22px;
           align-items:baseline; background:var(--panel); border:1px solid var(--line);
           border-radius:8px; padding:16px 18px; margin:14px 0 22px; }
.details dt { color:var(--muted); font-size:12px; white-space:nowrap; }
.details dd { margin:0; min-width:0; font-family:ui-monospace, SFMono-Regular, Menlo, monospace;
              font-size:13px; word-break:break-all; }
.details dd.plain { font-family:inherit; }
.card { background:var(--panel); border:1px solid var(--line); border-radius:8px;
        padding:16px 18px; margin:12px 0; }
.mr-changes { background:var(--panel); border:1px solid var(--line); border-radius:8px;
              margin:12px 0; padding:10px 14px; }
.mr-changes > summary { cursor:pointer; font-weight:600; }
.mr-changes-body { margin-top:12px; }
.mr-doc { background:var(--panel); border:1px solid var(--line); border-radius:8px;
          margin:12px 0; padding:10px 14px; }
.mr-doc > summary { cursor:pointer; display:flex; gap:8px; align-items:center; flex-wrap:wrap; }
.mr-doc > summary code { font-size:12px; color:var(--muted); }
.mr-objects { margin:12px 0 0; }
.mr-objects > summary { cursor:pointer; color:var(--muted); font-size:13px; margin-bottom:8px; }
.mr-object { border-top:1px solid var(--line); padding:8px 0; }
.mr-object > summary { cursor:pointer; display:flex; gap:8px; align-items:center; flex-wrap:wrap; }
.mr-object code { font-size:12px; }
.mr-object .muted, .mr-doc .muted { color:var(--muted); font-size:12px; }
pre.diff { margin:10px 0 0; padding:10px 12px; overflow:auto; max-height:60vh;
           background:var(--bg); border:1px solid var(--line); border-radius:6px;
           font-family:ui-monospace, SFMono-Regular, Menlo, monospace;
           font-size:12px; line-height:1.45; white-space:pre; }
.diff-line { display:block; min-height:1.45em; padding:0 4px; }
.diff-add { background:rgba(46,160,67,.16); color:#1a7f37; }
.diff-del { background:rgba(248,81,73,.16); color:#b42318; }
.diff-context { color:var(--muted); }
.diff-gap { color:var(--muted); font-style:italic; }
table.field-diff { margin-top:10px; background:transparent; }
table.field-diff th { width:120px; color:var(--muted); font-size:12px; vertical-align:top; }
table.field-diff td { font-family:ui-monospace, SFMono-Regular, Menlo, monospace; font-size:12px;
                      white-space:pre-wrap; word-break:break-word; }
table.field-diff td.diff-before { background:rgba(248,81,73,.08); }
table.field-diff td.diff-after { background:rgba(46,160,67,.10); }
.markdown > :first-child { margin-top:0; }
.markdown > :last-child { margin-bottom:0; }
.markdown p { margin:8px 0; }
.markdown h1, .markdown h2, .markdown h3, .markdown h4, .markdown h5, .markdown h6 {
  margin:16px 0 8px; line-height:1.3; scroll-margin-top:12px;
}
.markdown h1 { font-size:24px; }
.markdown h2 { font-size:21px; }
.markdown h3 { font-size:18px; }
.markdown h4 { font-size:17px; }
.markdown h5, .markdown h6 { font-size:16px; }
.doc-view { display:flex; flex-direction:column; gap:12px; }
.doc-heading-row { display:flex; align-items:baseline; gap:8px; margin:16px 0 8px; }
.doc-heading-row h1, .doc-heading-row h2, .doc-heading-row h3,
.doc-heading-row h4, .doc-heading-row h5, .doc-heading-row h6 { margin:0; }
.doc-fold { flex:none; background:none; border:1px solid var(--line); border-radius:4px;
            color:var(--muted); cursor:pointer; font-size:11px; line-height:1.4;
            padding:1px 6px; }
.doc-fold:hover { color:var(--fg); border-color:var(--muted); }
.doc-section-body[hidden] { display:none; }
.doc-section { margin:6px 0; border-left:1px solid var(--line); padding-left:10px; }
.doc-section > summary { cursor:pointer; font-weight:600; padding:2px 0; }
.doc-section-body { margin-left:4px; }
.doc-section-content { margin:6px 0; }
.doc-section-content > :first-child { margin-top:0; }
.doc-section-children { margin-left:8px; }
.lazy-doc-body { display:flex; flex-direction:column; gap:4px; }
.doc-toc-head { display:flex; flex-wrap:wrap; align-items:center; gap:10px; margin-bottom:6px; }
.doc-toc-head strong { font-size:13px; }
.doc-toc-head button { padding:3px 8px; font-size:12px; }
.doc-toc { background:var(--panel); border:1px solid var(--line); border-radius:8px;
           padding:10px 14px; }
.doc-toc ul { list-style:none; margin:0; padding:0; }
.doc-toc li { margin:1px 0; }
.doc-toc a { color:var(--accent); text-decoration:none; font-size:13px; }
.doc-toc a:hover { text-decoration:underline; }
.doc-toc[hidden] { display:none; }
.doc-body { min-width:0; }
.structure-members { margin-top:14px; border-top:1px solid var(--line); padding-top:10px; }
.structure-members h3 { margin:0 0 8px; font-size:14px; }
.structure-members ul { list-style:none; margin:0; padding:0; }
.structure-members li { margin:4px 0; font-size:13px; overflow-wrap:anywhere; }
.doc-comment-chip { display:inline-flex; align-items:center; gap:2px; margin-left:5px;
                    padding:1px 4px; border:1px solid var(--line); border-radius:999px;
                    background:var(--panel); color:var(--accent); font-size:11px;
                    line-height:1; vertical-align:middle; cursor:pointer; }
.doc-comment-chip:hover { background:var(--accent-soft); border-color:var(--accent); }
.doc-comment-chip svg { display:block; }
.doc-comment-chip-count { font-weight:700; }
.doc-comment-popover { position:fixed; z-index:80; width:min(340px, calc(100vw - 24px));
                       max-height:60vh; overflow:auto; padding:10px 12px;
                       background:var(--panel); border:1px solid var(--line);
                       border-radius:8px; box-shadow:0 12px 32px rgba(0,0,0,.18); }
.doc-comment-popover-head { display:flex; align-items:center; justify-content:space-between;
                            gap:10px; margin-bottom:6px; }
.doc-comment-popover-head span { color:var(--muted); font-size:12px; }
.doc-comment { padding:9px 11px; margin:8px 0; color:var(--fg);
               background:var(--bg); border:1px solid var(--line);
               border-left:3px solid var(--accent); border-radius:6px; }
.doc-comment[data-resolved="true"] { color:var(--muted); border-left-color:var(--muted); }
.doc-comment-head { display:flex; align-items:center; gap:8px; flex-wrap:wrap; font-size:12px; }
.doc-comment-head code { color:var(--muted); font-size:11px; overflow-wrap:anywhere; }
.doc-comment-body { margin-top:6px; line-height:1.55; overflow-wrap:anywhere; }
.markdown ul, .markdown ol { margin:8px 0; padding-left:22px; }
.markdown li { margin:2px 0; }
.markdown pre { background:var(--bg); border:1px solid var(--line); border-radius:6px;
                padding:10px 12px; overflow:auto; margin:10px 0; }
.markdown pre code { background:none; padding:0; }
.mermaid-diagram { margin:10px 0; padding:12px; border:1px solid var(--line); border-radius:6px;
                   background:var(--bg); overflow:auto; text-align:center; }
.mermaid-diagram svg { max-width:100%; height:auto; }
.mermaid-error { margin:10px 0; padding:10px 12px; border:1px solid #d33; border-radius:6px;
                 color:#b00; background:var(--bg); white-space:pre-wrap; font-size:13px; }
.html-preview { width:100%; height:360px; margin:10px 0; border:1px solid var(--line);
                border-radius:6px; background:#fff; display:block; }
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
.approval-actions { display:flex; align-items:center; gap:12px; flex-wrap:wrap;
                    margin:24px 0 8px; padding-top:16px; border-top:1px solid var(--line); }
.approval-actions .inline-form { display:inline; }
.annotation-toolbar { display:flex; align-items:center; gap:12px; flex-wrap:wrap;
                      margin:18px 0 8px; }
.annotation-toolbar .hint { margin:0; }
.annotation-card { margin:10px 0; padding:10px 12px; color:var(--fg);
                  background:var(--annotation-bg); border:1px solid var(--annotation-border);
                  border-left:5px solid var(--annotation-border); border-radius:6px;
                  box-shadow:0 1px 2px rgba(0,0,0,.08); }
.annotation-card-head { display:flex; align-items:center; gap:8px; flex-wrap:wrap;
                        font-size:12px; color:var(--annotation-fg); }
.annotation-label { font-weight:700; letter-spacing:.08em; }
.annotation-meta { color:var(--muted); }
.annotation-card-body { margin:6px 0; }
.annotation-card-body > :first-child { margin-top:0; }
.annotation-card-body > :last-child { margin-bottom:0; }
.annotation-quote { margin:6px 0; padding:2px 10px; border-left:3px solid var(--annotation-border);
                    color:var(--muted); font-size:13px; }
.annotation-card .inline-form { margin:4px 0 0; }
.annotation-resolve-form .field { margin:4px 0; gap:2px; }
.annotation-resolve-form .field label { font-size:12px; }
.annotation-resolve-form textarea { min-height:44px; border-radius:2px; padding:5px 7px; }
.annotation-card[data-status="resolved"] { background:var(--panel); border-left-color:var(--muted);
                                           color:var(--muted); }
.annotation-card[data-status="resolved"] .annotation-label,
.annotation-card[data-status="resolved"] .badge { color:var(--muted); }
.annotation-card[data-status="resolved"] .annotation-resolution { border-top-color:var(--line); }
.annotation-resolution { margin-top:8px; padding-top:8px; border-top:1px dashed var(--annotation-border); }
.annotation-resolution p { margin:4px 0; }
.annotations-hidden .annotation-card { display:none; }
.only-unresolved .annotation-card[data-resolved="true"] { display:none; }
.annotation-source[hidden], .annotation-orphans[hidden] { display:none; }
.annotation-orphans { margin-top:24px; }
.annotation-dialog { width:min(560px, calc(100vw - 32px)); padding:0; color:var(--fg);
                     background:var(--bg); border:1px solid var(--line); border-radius:6px; }
.annotation-dialog::backdrop { background:rgba(0,0,0,.45); }
.annotation-dialog form { padding:14px 16px; }
.annotation-dialog-target { margin:0 0 8px; color:var(--muted); }
.annotation-dialog .field { display:grid; grid-template-columns:72px minmax(0,1fr);
                            align-items:start; gap:8px; margin:8px 0; }
.annotation-dialog .field label { padding-top:6px; font-size:12px; }
.annotation-dialog .field input, .annotation-dialog .field textarea {
  border-radius:2px; padding:6px 8px;
}
.annotation-dialog .field textarea { min-height:60px; }
.annotation-dialog .actions { margin-top:12px; }
.anchor-block { border-radius:6px; }
.anchor-block:hover { background:var(--accent-soft); }
.markdown li[data-anchor]:hover { background:var(--accent-soft); }
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
button.link { background:none; color:var(--accent); border:0; padding:0; cursor:pointer; }
button.link:hover { text-decoration:underline; }
button.danger { background:transparent; color:#cf222e; border:1px solid #cf222e; padding:4px 10px; }
button.danger.solid { background:#cf222e; color:#fff; padding:6px 14px; font-weight:600; }
button.danger.solid:hover { background:#a40e26; border-color:#a40e26; }
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
main.wide { max-width:1500px; padding:24px 28px 40px; }
.ds-crumbs { margin:0 0 10px; }
.ds-head { display:flex; flex-wrap:wrap; align-items:flex-end; justify-content:space-between; gap:14px; }
.ds-head h1 { margin:0; }
.ds-head .sub { margin:4px 0 0; }
.ds-stats { display:flex; gap:8px; flex-wrap:wrap; }
.ds-stat { min-width:88px; padding:8px 12px; border:1px solid var(--line); border-radius:8px;
          background:var(--panel); }
.ds-stat b { display:block; font-size:19px; line-height:1.2; font-variant-numeric:tabular-nums; }
.ds-stat span { color:var(--muted); font-size:10px; letter-spacing:.06em; text-transform:uppercase; }
.ds-shell { display:grid; grid-template-columns:minmax(280px,380px) minmax(0,1fr);
            gap:16px; align-items:start; margin-top:18px; }
.ds-pane { background:var(--panel); border:1px solid var(--line); border-radius:10px;
           overflow:hidden; }
.ds-tree-pane { position:sticky; top:16px; height:calc(100vh - 32px);
                display:flex; flex-direction:column; }
.ds-tools { display:flex; flex-direction:column; gap:8px; padding:12px;
            border-bottom:1px solid var(--line); background:var(--bg); }
.ds-tools input { width:100%; font:inherit; font-size:13px; color:var(--fg);
                  background:var(--bg); border:1px solid var(--line); border-radius:6px;
                  padding:7px 9px; }
.ds-tools-row { display:flex; gap:6px; }
.ds-branch { display:flex; align-items:center; gap:8px; font-size:12px; color:var(--muted); }
.ds-branch label { flex:none; }
.ds-branch select { flex:1; min-width:0; font:inherit; color:var(--fg); background:var(--bg);
                    border:1px solid var(--line); border-radius:6px; padding:6px 8px; }
.ds-tools-row button { flex:1; padding:5px 8px; font-size:12px; }
.ds-tree { overflow:auto; padding:8px 10px 16px; font-size:13px; }
.ds-list { display:flex; flex-direction:column; gap:1px; }
.ds-row { display:flex; align-items:center; gap:5px; height:26px; min-height:26px;
          padding:0 6px; line-height:1.2; border-radius:6px; cursor:pointer;
          border:1px solid transparent; color:var(--fg); overflow:hidden; }
.ds-row:hover { background:var(--bg); }
.ds-row.active { background:var(--accent-soft); border-color:var(--accent); }
.ds-row .ds-name { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; min-width:0; }
.ds-caret { width:12px; flex:none; color:var(--muted); font-size:9px; line-height:1; }
.ds-caret::before { content:"\25B6"; }
.ds-row.open > .ds-caret::before { content:"\25BC"; }
.ds-caret.empty { visibility:hidden; }
.ds-folder, .ds-file { flex:none; display:inline-flex; align-items:center;
                       justify-content:center; width:16px; height:16px; }
.ds-folder { color:var(--accent); }
.ds-file { color:var(--muted); }
.ds-folder svg, .ds-file svg { display:block; width:14px; height:14px; }
.ds-chip { flex:none; display:inline-flex; align-items:center; justify-content:center;
           height:16px; min-width:34px; padding:0 4px; border-radius:4px;
           font:600 9px/1 ui-monospace, SFMono-Regular, Menlo, monospace;
           background:var(--bg); border:1px solid var(--line); color:var(--muted);
           letter-spacing:.03em; }
.ds-chip.file { min-width:24px; }
.ds-kind { flex:none; display:inline-flex; align-items:center; height:16px;
           padding:0 4px; border-radius:4px; border:1px solid var(--line);
           background:var(--bg); color:var(--accent); font-size:10px; line-height:1; }
.ds-dot { width:5px; height:5px; border-radius:50%; flex:none; }
.ds-dot.on { background:#2da44e; }
.ds-dot.off { background:transparent; border:1px solid var(--line); }
.ds-meta { margin-left:auto; color:var(--muted); min-width:0; overflow:hidden;
           text-overflow:ellipsis; white-space:nowrap;
           font:11px ui-monospace, SFMono-Regular, Menlo, monospace; }
.ds-right { margin-left:auto; display:flex; align-items:center; gap:6px; flex:none; }
.ds-count { margin-left:auto; color:var(--muted); font-size:11px;
            font-variant-numeric:tabular-nums; }
.ds-right .ds-count { margin-left:0; }
.ds-action { background:none; border:0; padding:0 2px; color:var(--accent);
             font:inherit; font-size:11px; cursor:pointer; }
.ds-action:hover { text-decoration:underline; }
.ds-children { display:flex; flex-direction:column; gap:1px; margin-left:12px;
               padding-left:8px; border-left:1px solid var(--line); }
.ds-empty, .ds-loading { color:var(--muted); padding:28px 16px; text-align:center; }
.ds-detail-pane { padding:0; min-height:64vh; }
.ds-detail { padding:22px 24px 32px; }
.ds-detail-head { display:flex; flex-direction:column; gap:6px; padding-bottom:14px;
                  border-bottom:1px solid var(--line); }
.ds-detail-head h2 { margin:0; font-size:20px; line-height:1.3; word-break:break-word; }
.ds-detail-kinds { display:flex; gap:6px; align-items:center; }
.ds-path { color:var(--muted); font:12px ui-monospace, SFMono-Regular, Menlo, monospace;
           word-break:break-all; }
.ds-tabs { display:flex; gap:4px; margin:16px 0 0; border-bottom:1px solid var(--line); }
.ds-tabs button { background:none; border:0; border-bottom:2px solid transparent;
                  color:var(--muted); padding:8px 12px; font:inherit; font-size:13px;
                  cursor:pointer; }
.ds-tabs button.active { color:var(--accent); border-bottom-color:var(--accent);
                         font-weight:600; }
.ds-panel { padding-top:16px; }
.ds-panel[hidden] { display:none; }
.ds-code { background:var(--bg); border:1px solid var(--line); border-radius:8px;
           overflow:auto; max-height:62vh; padding:10px 0;
           font:12.5px/1.55 ui-monospace, SFMono-Regular, Menlo, monospace; }
.ds-line { display:flex; gap:12px; padding:0 12px; white-space:pre; }
.ds-line.in { background:var(--accent-soft); box-shadow:inset 2px 0 0 var(--accent); }
.ds-line .ln { flex:none; width:44px; text-align:right; color:var(--muted);
               user-select:none; font-variant-numeric:tabular-nums; }
.ds-line .tx { white-space:pre; }
.ds-source-meta { color:var(--muted); margin-bottom:8px; display:flex; gap:12px;
                  flex-wrap:wrap; font:12px ui-monospace, SFMono-Regular, Menlo, monospace; }
.ds-source-error { color:var(--annotation-fg); background:var(--annotation-bg);
                   border:1px solid var(--annotation-border); border-radius:8px;
                   padding:12px 14px; }
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
  main.wide { padding:20px 14px 40px; }
  .two-col, .stats { grid-template-columns:1fr; }
  .grid { grid-template-columns:1fr; }
  .ds-shell { grid-template-columns:1fr; }
  .ds-tree-pane { position:static; height:52vh; }
  .ds-detail-pane { min-height:40vh; }
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

/// Show `ds://` addresses with percent escapes decoded; the stored value keeps
/// the encoded form.
fn display_percent_encoded(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn document_headings(input: &str) -> Vec<(usize, String, usize)> {
    let normalized = input.replace("\r\n", "\n");
    let mut headings = Vec::new();
    let mut in_code = false;
    let mut index = 0usize;
    for line in normalized.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if let Some((level, rest)) = markdown_heading(trimmed) {
            index += 1;
            headings.push((
                level,
                strip_doc_comment_markers(rest).trim_end().to_string(),
                index,
            ));
        }
    }
    headings
}

fn toc_label(text: &str) -> String {
    let text = text.trim();
    let contains_path = !text.contains(' ')
        && (text.starts_with("namespaces/")
            || text.starts_with("files/")
            || text.starts_with("documents/")
            || text.starts_with("design/")
            || text.starts_with("src/")
            || text.starts_with("tests/")
            || text.starts_with("docs/")
            || text.contains("::")
            || text.matches('/').count() >= 2);
    if contains_path {
        let local = text
            .rsplit_once("::")
            .map(|(_, local)| local)
            .unwrap_or(text);
        return local
            .rsplit_once('/')
            .map(|(_, local)| local)
            .unwrap_or(local)
            .to_string();
    }
    text.to_string()
}

fn document_view(input: &str) -> String {
    let headings = document_headings(input);
    let body = document_body_html(input);
    if headings.is_empty() {
        return format!(
            r#"<div class="doc-view"><div class="markdown doc-body">{body}</div></div>"#
        );
    }
    let items = headings
        .iter()
        .map(|(level, text, index)| {
            let indent = level.saturating_sub(1) * 14;
            format!(
                r##"<li style="padding-left:{indent}px"><a href="#md-h-{index}">{}</a></li>"##,
                inline_markdown(&toc_label(text), 0)
            )
        })
        .collect::<String>();
    format!(
        r#"<div class="doc-view">
<div class="doc-toc-head"><strong>目录</strong><button type="button" class="secondary" data-doc-fold-all="open">展开全部</button><button type="button" class="secondary" data-doc-fold-all="close">折叠全部</button><button type="button" class="secondary" onclick="var n=document.getElementById('doc-toc'); n.hidden=!n.hidden; this.textContent=n.hidden?'显示目录':'隐藏目录';">隐藏目录</button></div>
<nav class="doc-toc" id="doc-toc"><ul>{items}</ul></nav>
<div class="markdown doc-body">{body}</div>
</div>"#
    )
}

fn lazy_document_view(sections: &[DesignSpaceSection]) -> String {
    let mut toc_index = 0usize;
    let items = lazy_document_toc(sections, 0, &mut toc_index);
    let mut body_index = 0usize;
    let body = sections
        .iter()
        .map(|section| lazy_document_section(section, &mut body_index))
        .collect::<String>();
    format!(
        r#"<div class="doc-view lazy-doc-view">
<div class="doc-toc-head"><strong>目录</strong><button type="button" class="secondary" data-doc-open-all="open">展开全部</button><button type="button" class="secondary" data-doc-open-all="close">折叠全部</button><button type="button" class="secondary" onclick="var n=document.getElementById('doc-toc'); n.hidden=!n.hidden; this.textContent=n.hidden?'显示目录':'隐藏目录';">隐藏目录</button></div>
<nav class="doc-toc" id="doc-toc"><ul>{items}</ul></nav>
<div class="markdown doc-body lazy-doc-body">{body}</div>
</div>"#
    )
}

fn lazy_document_toc(sections: &[DesignSpaceSection], depth: usize, index: &mut usize) -> String {
    let mut out = String::new();
    for section in sections {
        let id = *index;
        *index += 1;
        out.push_str(&format!(
            r##"<li style="padding-left:{}px"><a href="#doc-section-{id}">{}</a></li>"##,
            depth * 14,
            inline_markdown(&toc_label(&section.title), 0)
        ));
        out.push_str(&lazy_document_toc(&section.children, depth + 1, index));
    }
    out
}

fn lazy_document_section(section: &DesignSpaceSection, index: &mut usize) -> String {
    let id = *index;
    *index += 1;
    let children = if section.children.is_empty() {
        String::new()
    } else {
        format!(
            r#"<div class="doc-section-children">{}</div>"#,
            section
                .children
                .iter()
                .map(|child| lazy_document_section(child, index))
                .collect::<String>()
        )
    };
    format!(
        r#"<details class="doc-section" data-section-qname="{qname}">
<summary id="doc-section-{id}">{title}</summary>
<div class="doc-section-body">
  <div class="doc-section-content markdown" data-section-content><p class="hint">展开后加载…</p></div>
  {children}
</div>
</details>"#,
        qname = escape(&section.qname),
        id = id,
        title = escape(&section.title),
        children = children,
    )
}

/// HTML fragment for one lazily loaded document section.
pub fn design_space_section(markdown: &str) -> String {
    markdown_to_html(markdown)
}

pub fn design_space_section_error(message: &str) -> String {
    format!(r#"<div class="ds-source-error">{}</div>"#, escape(message))
}

/// Render document body with one indentation wrapper per heading level.
/// The heading itself stays at its level; following content is indented so
/// nested sections are visually distinct from flat paragraphs.
fn document_body_html(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n");
    let mut out = String::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut chunk = String::new();
    let mut heading_index = 0usize;
    let mut in_code = false;
    for line in normalized.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            chunk.push_str(line);
            chunk.push('\n');
            continue;
        }
        if !in_code && let Some((level, rest)) = markdown_heading(trimmed) {
            flush_doc_chunk(&mut out, &mut chunk);
            while stack.last().is_some_and(|open| *open >= level) {
                out.push_str("</div>");
                stack.pop();
            }
            heading_index += 1;
            out.push_str(&format!(
                r#"<div class="doc-heading-row"><h{level} id="md-h-{heading_index}">{}</h{level}><button type="button" class="doc-fold" data-doc-fold aria-expanded="true" title="折叠此节">收起</button></div>"#,
                inline_markdown(rest.trim_end(), 0)
            ));
            out.push_str(&format!(
                r#"<div class="doc-level doc-section-body" style="margin-left:{}px">"#,
                (level - 1) * 18
            ));
            stack.push(level);
            continue;
        }
        chunk.push_str(line);
        chunk.push('\n');
    }
    flush_doc_chunk(&mut out, &mut chunk);
    while !stack.is_empty() {
        stack.pop();
        out.push_str("</div>");
    }
    out
}

fn flush_doc_chunk(out: &mut String, chunk: &mut String) {
    if !chunk.trim().is_empty() {
        out.push_str(&markdown_to_html(chunk));
    }
    chunk.clear();
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
    let mut section_open = false;
    let mut heading_index = 0usize;
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
            code = Some((
                trimmed.trim_start_matches('`').trim().to_string(),
                Vec::new(),
            ));
            index += 1;
            continue;
        }

        if let Some(key) = anchor_marker(trimmed) {
            flush_markdown_paragraph(&mut out, &mut paragraph);
            close_markdown_list(&mut out, &mut list);
            if section_open {
                out.push_str("</section>");
            }
            out.push_str(&format!(
                r#"<section class="anchor-block" data-anchor="{}">"#,
                escape(key)
            ));
            section_open = true;
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
            heading_index += 1;
            out.push_str(&format!(
                r#"<h{level} id="md-h-{heading_index}">{}</h{level}>"#,
                inline_markdown(rest, 0)
            ));
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
            let (anchor, item) = match list_item_anchor(item) {
                Some((key, item)) => (Some(key), item),
                None => (None, item),
            };
            out.push_str("<li");
            if let Some(key) = anchor {
                out.push_str(&format!(r#" data-anchor="{}""#, escape(key)));
            }
            out.push('>');
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
    if section_open {
        out.push_str("</section>");
    }
    out
}

fn anchor_marker(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("<!--anchor:")?;
    let key = rest.strip_suffix("-->")?;
    if key.is_empty() { None } else { Some(key) }
}

fn list_item_anchor(item: &str) -> Option<(&str, &str)> {
    let rest = item.strip_prefix("<!--anchor:")?;
    let end = rest.find("-->")?;
    let key = &rest[..end];
    if key.is_empty() {
        return None;
    }
    Some((key, rest[end + 3..].trim_start()))
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
    let level = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &line[level..];
    rest.starts_with(' ').then(|| (level, rest.trim_start()))
}

fn markdown_hr(line: &str) -> bool {
    let Some(first) = line.chars().next() else {
        return false;
    };
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    line.chars()
        .all(|character| character == first || character == ' ')
        && line.chars().filter(|character| *character == first).count() >= 3
}

fn markdown_list_item(line: &str) -> Option<(&'static str, &str)> {
    for prefix in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some(("ul", rest.trim_start()));
        }
    }
    let digits = line
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .count();
    if digits > 0
        && let Some(rest) = line[digits..].strip_prefix(". ")
    {
        return Some(("ol", rest.trim_start()));
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
            '{' => {
                if let Some((ids, end)) = doc_comment_marker(&characters, index) {
                    let count = ids.split(',').count();
                    let count_html = if count > 1 {
                        format!(r#"<span class="doc-comment-chip-count">{count}</span>"#)
                    } else {
                        String::new()
                    };
                    out.push_str(&format!(
                        r#"<button type="button" class="doc-comment-chip" data-comments="{ids}" aria-label="查看 {count} 条设计评论" title="查看评论"><svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true"><path d="M2 2.5h12v8.5H7.2L3.5 13.5V11H2z" fill="none" stroke="currentColor" stroke-linejoin="round"/><path d="M4.5 5.5h7M4.5 8h5" fill="none" stroke="currentColor" stroke-linecap="round"/></svg>{count_html}</button>"#,
                        ids = escape(&ids),
                    ));
                    index = end;
                } else {
                    out.push('{');
                    index += 1;
                }
            }
            _ => {
                let start = index;
                while index < characters.len()
                    && !matches!(characters[index], '`' | '[' | '*' | '~' | '\n' | '{')
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

fn doc_comment_marker(characters: &[char], start: usize) -> Option<(String, usize)> {
    const PREFIX: &str = "{{comments:";
    let prefix: Vec<char> = PREFIX.chars().collect();
    if !characters[start..].starts_with(&prefix) {
        return None;
    }
    let mut index = start + prefix.len();
    let mut ids = String::new();
    while index < characters.len() && characters[index] != '}' {
        let character = characters[index];
        if !(character.is_ascii_alphanumeric()
            || matches!(character, ',' | '#' | '_' | '.' | '/' | '-'))
        {
            return None;
        }
        ids.push(character);
        index += 1;
    }
    if index + 1 >= characters.len()
        || characters[index] != '}'
        || characters[index + 1] != '}'
        || ids.is_empty()
        || ids.starts_with(',')
        || ids.ends_with(',')
        || ids.contains(",,")
    {
        return None;
    }
    Some((ids, index + 2))
}

fn strip_doc_comment_markers(text: &str) -> String {
    const PREFIX: &str = "{{comments:";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(PREFIX) {
        out.push_str(&rest[..start]);
        let after = &rest[start + PREFIX.len()..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        rest = &after[end + 2..];
    }
    out.push_str(rest);
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
    Tasks,
    Projects,
    DesignSpace,
    MergeRequests,
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
    layout_page(ctx, active, title, body, false)
}

fn layout_wide(ctx: &ConsoleContext, active: Section, title: &str, body: &str) -> String {
    layout_page(ctx, active, title, body, true)
}

fn layout_page(
    ctx: &ConsoleContext,
    active: Section,
    title: &str,
    body: &str,
    wide: bool,
) -> String {
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

    let design_space_nav = ctx
        .current
        .as_ref()
        .map(|project| {
            nav_item(
                active,
                Section::DesignSpace,
                &format!("/projects/{}/design-space", project.id),
                "设计空间",
            )
        })
        .unwrap_or_default();

    let merge_requests_nav = ctx
        .current
        .as_ref()
        .map(|project| {
            nav_item(
                active,
                Section::MergeRequests,
                &format!("/projects/{}/merge-requests", project.id),
                "设计空间合并请求",
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
    {design_space_nav}
    {merge_requests_nav}
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
</div>
<div class="sidebar-user">
  <strong>{display_name}</strong>
  <span>@{handle} · {kind}{admin}</span>
  <form method="post" action="/logout"><button class="link" type="submit">退出登录</button></form>
</div>"#,
        nav_item(active, Section::Workbench, "/", "工作台"),
        nav_item(active, Section::Issues, "/issues", "议题"),
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
        design_space_nav = design_space_nav,
        merge_requests_nav = merge_requests_nav,
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

    let main_class = if wide { r#" class="wide""# } else { "" };
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
<main{main_class}>{body}</main>
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

pub fn workbench(ctx: &ConsoleContext, issues: &[Issue], tasks: &[Task]) -> String {
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
<p class="hint">当前已接入议题、需求文档和任务；设计文档、决策和发布尚未接入。</p>

<section>
  <h2>最近议题</h2>
  <div class="list">{issue_items}</div>
  <p><a href="/issues">查看全部议题 →</a></p>
</section>"#,
    );
    layout(ctx, Section::Workbench, "工作台", &body)
}

pub fn tasks_page(
    ctx: &ConsoleContext,
    tasks: &[Task],
    selected_state: Option<TaskState>,
) -> String {
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
        rows = if selected_state.is_some() {
            r#"<tr><td colspan="5" class="empty">该状态下没有任务。</td></tr>"#.to_string()
        } else {
            r#"<tr><td colspan="5" class="empty">还没有任务。项目创建后，流程脚本会创建第一个任务。</td></tr>"#.to_string()
        };
    }

    let mut options = String::new();
    options.push_str(&format!(
        r#"<option value=""{}>全部状态</option>"#,
        if selected_state.is_none() {
            " selected"
        } else {
            ""
        }
    ));
    for state in [
        TaskState::Pending,
        TaskState::InProgress,
        TaskState::Failed,
        TaskState::Completed,
    ] {
        options.push_str(&format!(
            r#"<option value="{value}"{selected}>{label}</option>"#,
            value = state.as_str(),
            selected = if selected_state == Some(state) {
                " selected"
            } else {
                ""
            },
            label = task_state_label(state),
        ));
    }
    let filter = format!(
        r#"<form class="inline-form" method="get" action="/tasks">
  <label for="task-state-filter">状态</label>
  <select id="task-state-filter" name="state" onchange="this.form.submit()">{options}</select>
  <noscript><button type="submit">筛选</button></noscript>
  <a href="/tasks">清除筛选</a>
</form>"#
    );
    let state_query = selected_state
        .map(|state| format!("?state={}", state.as_str()))
        .unwrap_or_default();
    let refresh = task_refresh_script(
        &format!("/tasks/state{state_query}"),
        &tasks_fingerprint(tasks),
    );
    let count = if selected_state.is_some() {
        format!("当前筛选：{} 个任务", tasks.len())
    } else {
        format!("共 {} 个任务", tasks.len())
    };
    let body = format!(
        r#"<h1>任务</h1>
<p class="sub">执行空间：谁在什么时候做什么。任务由流程脚本创建；声明 agent 角色的任务由任务池调度。</p>
{filter}
<p class="hint">{count}</p>
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

pub fn task_page(ctx: &ConsoleContext, task: &Task, links: &[TaskLink]) -> String {
    let actor_name = |actor: &foundry_domain::TaskActor| {
        let name = escape(&actor.display_name);
        match (ctx.viewer.is_admin(), actor.agent_definition_id) {
            (true, Some(agent_id)) => format!(r#"<a href="/agents/{agent_id}/logs">{name}</a>"#),
            _ => name,
        }
    };
    let active_agents = task
        .actors
        .iter()
        .filter(|actor| actor.active)
        .map(|actor| {
            format!(
                r#"<li><code>{role}</code> · {name}{work}</li>"#,
                role = escape(actor.role_key.as_str()),
                name = actor_name(actor),
                work = actor
                    .work
                    .as_deref()
                    .map(|work| format!(" · {}", escape(work)))
                    .unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    let active_agents = if active_agents.is_empty() {
        r#"<p class="hint">当前阶段没有活跃智能体。</p>"#.to_string()
    } else {
        format!("<ul>{}</ul>", active_agents.join(""))
    };
    let stage = task
        .stage
        .as_ref()
        .map(|stage| escape(stage.as_str()))
        .unwrap_or_else(|| "—".to_string());
    let task_operations = if task.allowed_operations.is_empty() {
        r#"<p class="hint">当前账号没有可执行的任务操作。</p>"#.to_string()
    } else {
        task.allowed_operations
            .iter()
            .map(|operation| {
                format!(
                    r#"<form class="inline-form" method="post" action="/tasks/{id}/operations">
<input type="hidden" name="operation" value="{operation}">
<button type="submit">{label}</button>
</form>"#,
                    id = task.id,
                    operation = escape(operation.name.as_str()),
                    label = escape(&operation.display_name),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
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
                            actor_name(actor),
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
                    r#"<tr><td><code>{name}</code></td><td>{kind}</td><td><code>{assignment}</code></td><td>{actors}</td></tr>"#,
                    name = escape(&role.name),
                    kind = role_kind_label(role.kind),
                    assignment = escape(&role.assignment),
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
    let recovery = if task.state == TaskState::Failed
        || task
            .stage
            .as_ref()
            .is_some_and(|stage| stage.as_str().ends_with("failed"))
    {
        format!(
            r#"<h2>失败处理</h2>
<div class="approval-actions">
<form class="inline-form" method="post" action="/tasks/{id}/retry"><button type="submit">重试</button></form>
<form class="inline-form" method="post" action="/tasks/{id}/abandon"><button class="secondary" type="submit">放弃</button></form>
</div>"#,
            id = task.id
        )
    } else {
        String::new()
    };
    let refresh = task_refresh_script(
        &format!("/tasks/{}/state", task.id),
        &task_fingerprint(task),
    );
    let description = if task.description.trim().is_empty() {
        r#"<p class="hint">未填写任务内容。</p>"#.to_string()
    } else {
        markdown_to_html(&task.description)
    };
    let input_block =
        if task.input.is_null() || task.input.as_object().is_some_and(|value| value.is_empty()) {
            String::new()
        } else {
            format!(
                r#"<details class="card"><summary>任务参数</summary><pre>{}</pre></details>"#,
                escape(
                    &serde_json::to_string_pretty(&task.input)
                        .unwrap_or_else(|_| task.input.to_string())
                )
            )
        };
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
<h2>当前阶段与活跃智能体</h2>
<div class="card">
  <p>当前阶段 <code>{stage}</code></p>
  {active_agents}
  <h3>可执行操作</h3>
  {task_operations}
  <p class="hint">有关联合并请求的审批请从 <a href="/projects/{project_id}/merge-requests">项目合并请求</a> 页面完成。</p>
</div>
<h2>任务内容</h2>
<div class="card markdown">{description}</div>
{input_block}
<h2>角色与成员</h2>
<table>
<thead><tr><th>角色</th><th>类型</th><th>指派</th><th>Actor</th></tr></thead>
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
</table>
{recovery}{refresh}"#,
        title = escape(&task.title),
        state = task_state_label(task.state),
        dispatch = task_dispatch_label(task.dispatch_state),
        workflow = escape(task.workflow.as_str()),
        version = task.workflow_version,
        stage = stage,
        active_agents = active_agents,
        task_operations = task_operations,
        project_id = task.project_id,
        parent = parent,
        updated = task
            .updated_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        refresh = refresh,
        recovery = recovery,
        description = description,
        input_block = input_block,
    );
    layout(ctx, Section::Tasks, &task.title, &body)
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

#[derive(Debug, Clone)]
pub struct IssueLinkedTask {
    pub id: String,
    pub title: String,
    pub workflow: String,
    pub stage: String,
    pub state: String,
    pub result: String,
    pub active: bool,
}

pub fn issue_page(ctx: &ConsoleContext, issue: &Issue, linked_tasks: &[IssueLinkedTask]) -> String {
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

    let triage = issue
        .triage
        .as_ref()
        .map(|triage| {
            format!(
                r#"<h2>分诊结论</h2><div class="card">
<div class="meta"><span class="badge">{classification}</span><span class="badge">置信度 {confidence}</span></div>
<p><strong>根因</strong>：{root_cause}</p>
<p><strong>决策</strong>：{decision}</p>
{notes}
</div>"#,
                classification = issue_classification_label(triage.classification),
                confidence = issue_confidence_label(triage.confidence),
                root_cause = escape(&triage.root_cause),
                decision = escape(&triage.decision),
                notes = triage
                    .notes
                    .as_deref()
                    .filter(|notes| !notes.trim().is_empty())
                    .map(|notes| format!("<p><strong>说明</strong>：{}</p>", escape(notes)))
                    .unwrap_or_default(),
            )
        })
        .unwrap_or_default();
    let render_task = |task: &IssueLinkedTask| {
        format!(
            r#"<div class="card"><div class="meta"><a href="/tasks/{id}"><strong>{title}</strong></a><span class="badge">{workflow}</span><span class="badge">{stage}</span><span class="badge">{state}</span>{result}</div></div>"#,
            id = escape(&task.id),
            title = escape(&task.title),
            workflow = escape(&task.workflow),
            stage = escape(&task.stage),
            state = escape(&task.state),
            result = if task.result.is_empty() {
                String::new()
            } else {
                format!("<span class=\"badge\">{}</span>", escape(&task.result))
            },
        )
    };
    let current = linked_tasks.iter().map(render_task).collect::<String>();
    let current = if current.is_empty() {
        r#"<p class="empty">尚未关联任务。</p>"#.to_string()
    } else {
        current
    };
    let linked_tasks = format!("<h2>当前任务</h2>{current}");

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
{triage}
{linked_tasks}
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
        triage = triage,
        linked_tasks = linked_tasks,
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

    // The platform has exactly one intended root workflow: the one that
    // directly creates the project clarification and the four continuous
    // iterations. Do not fall back to the alphabetically first workflow.
    let default_root = root_workflows
        .iter()
        .find(|workflow| {
            matches!(workflow.owner, WorkflowOwner::Platform)
                && workflow.id.as_str() == "software_development"
        })
        .or_else(|| {
            root_workflows.iter().find(|workflow| {
                matches!(workflow.owner, WorkflowOwner::Platform)
                    && workflow.id.as_str() == "project"
            })
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
    let git_field = input_field("git_repository", "Git 仓库 *", "", true);
    let git_branch_field = input_field("git_branch", "Git 分支 *", "main", true);

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
  {git_field}
  {git_branch_field}
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
    triggers: &[Trigger],
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
        let capsule_options = [
            (CapsuleProfile::Cli, "CLI / 命令行产物"),
            (CapsuleProfile::Api, "API / HTTP 服务产物"),
            (CapsuleProfile::Web, "Web / UI 产物"),
            (CapsuleProfile::Desktop, "桌面 GUI 产物"),
        ]
        .into_iter()
        .map(|(profile, label)| {
            format!(
                r#"<option value="{value}"{selected}>{label}</option>"#,
                value = profile.as_str(),
                selected = if project.test_capsule == profile {
                    " selected"
                } else {
                    ""
                },
                label = label,
            )
        })
        .collect::<String>();
        let capsule_field = format!(
            r#"<div class="field"><label for="test_capsule">最终产物测试胶囊</label><select id="test_capsule" name="test_capsule">{capsule_options}</select><p class="hint">测试 Agent 按产物类型选择胶囊：CLI/API 用 headless shell，Web 用浏览器胶囊，桌面用带截图/输入的桌面胶囊。</p></div>"#
        );
        format!(
            r#"<h2>项目设置</h2>
<form method="post" action="/projects/{id}/edit">
  <div class="grid">
    {name}
    {status}
  </div>
  {description}
  {git_branch}
  {capsule_field}
  <div class="field">
    <label><input type="checkbox" name="skip_human_review" value="true"{skip_checked}> 跳过所有人工审批</label>
    <p class="hint">开启后本项目所有人类审批自动通过；项目创建和澄清仍需人类参与，其余撰写、实现、评审、测试、合入、发布由智能体执行。</p>
  </div>
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
  <div class="actions"><button class="danger solid" type="submit">删除项目</button></div>
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
            git_branch = input_field("git_branch", "Git 分支", &project.git_branch, true),
            capsule_field = capsule_field,
            skip_checked = if project.skip_human_review {
                " checked"
            } else {
                ""
            },
            handle = input_field("handle", "账号 handle", "", true),
            role = role_field,
        )
    } else {
        String::new()
    };

    let trigger_rows = if triggers.is_empty() {
        r#"<tr><td colspan="6" class="empty">还没有触发器。创建一个设计检查、代码检查或一致性检查触发器。</td></tr>"#.to_string()
    } else {
        triggers
            .iter()
            .map(|trigger| {
                let run = if ctx.can_contribute() {
                    format!(
                        r#"<form class="inline-form" method="post" action="/projects/{project}/triggers/{id}/run"><button type="submit">立即运行</button></form>"#,
                        project = project.id,
                        id = trigger.id,
                    )
                } else {
                    String::new()
                };
                let manage = if can_manage {
                    format!(
                        r#"<form class="inline-form" method="post" action="/projects/{project}/triggers/{id}/toggle"><button class="secondary" type="submit">{toggle}</button></form>
<form class="inline-form" method="post" action="/projects/{project}/triggers/{id}/delete" onsubmit="return confirm('删除这个触发器？');"><button class="danger" type="submit">删除</button></form>"#,
                        project = project.id,
                        id = trigger.id,
                        toggle = if trigger.enabled { "停用" } else { "启用" },
                    )
                } else {
                    String::new()
                };
                format!(
                    r#"<tr><td><strong>{name}</strong><br><code>{workflow}</code></td><td><code>{schedule}</code></td><td>{status}</td><td>{last}</td><td>{next}</td><td class="ds-right">{run}{manage}</td></tr>"#,
                    name = escape(&trigger.name),
                    workflow = escape(&trigger.workflow_id),
                    schedule = escape(&trigger.schedule),
                    status = if trigger.enabled { "启用" } else { "停用" },
                    last = trigger
                        .last_run_at
                        .map(|at| at.date().to_string())
                        .unwrap_or_else(|| "—".to_string()),
                    next = trigger
                        .next_run_at
                        .map(|at| at.date().to_string())
                        .unwrap_or_else(|| "—".to_string()),
                    run = run,
                    manage = manage,
                )
            })
            .collect::<String>()
    };
    let trigger_form = if can_manage {
        format!(
            r#"<h3>创建触发器</h3>
<form method="post" action="/projects/{id}/triggers">
  <div class="grid">
    <div class="field"><label for="trigger-name">名称</label><input id="trigger-name" name="name" required placeholder="例如 每日设计检查"></div>
    <div class="field"><label for="trigger-workflow">流程</label><select id="trigger-workflow" name="workflow_id">
      <option value="daily_check">日常检查</option>
      <option value="design_inspection">设计检查</option>
      <option value="consistency_inspection">代码-设计一致性检查</option>
      <option value="style_inspection">代码风格检查</option>
      <option value="logic_inspection">代码逻辑检查</option>
      <option value="cohesion_inspection">逻辑聚合度检查</option>
      <option value="design_consistency_maintenance">技术设计一致性维护</option>
    </select></div>
    <div class="field"><label for="trigger-schedule">计划</label><input id="trigger-schedule" name="schedule" required placeholder="manual / continuous / every:60 / daily@09:00"></div>
    <div class="field"><label for="trigger-scope">范围（可选）</label><input id="trigger-scope" name="scope" placeholder="例如 src 或 design/app"></div>
  </div>
  <p class="hint">计划使用 UTC；manual 表示只手动运行。</p>
  <div class="actions"><button type="submit">创建触发器</button></div>
</form>"#,
            id = project.id,
        )
    } else {
        String::new()
    };
    let triggers_section = format!(
        r#"<h2>触发器</h2><p class="hint">触发器会创建任务；可以按计划运行，也可以点“立即运行”。</p>
<table><thead><tr><th>名称 / 流程</th><th>计划</th><th>状态</th><th>上次运行</th><th>下次运行</th><th></th></tr></thead><tbody>{rows}</tbody></table>
{form}"#,
        rows = trigger_rows,
        form = trigger_form,
    );
    let short_id = project.id.to_string()[..8].to_string();
    let design_space_display = escape(&display_percent_encoded(&project.design_space_url));
    let body = format!(
        r#"<p><a href="/projects">← 返回项目列表</a></p>
<h1>{name}</h1>
<p class="sub"><span class="badge">{status}</span></p>
{error_html}
<dl class="details">
  <dt>项目 ID</dt><dd><code title="{id}">{short_id}</code></dd>
  <dt>Git 仓库</dt><dd>{git_repository}</dd>
  <dt>Git 分支</dt><dd><code>{git_branch}</code></dd>
  <dt>设计空间</dt><dd>{design_space_display}</dd>
  <dt>设计空间分支</dt><dd><code>{design_space_branch}</code></dd>
  <dt>跳过所有人工审批</dt><dd>{skip_human_review}</dd>
  <dt>最终产物测试胶囊</dt><dd><code>{test_capsule}</code></dd>
  <dt>成员</dt><dd class="plain">{member_count}</dd>
  <dt>版本</dt><dd class="plain">{version}</dd>
</dl>
<h2>描述</h2><div class="card">{description}</div>
<p><a class="button secondary" href="/projects/{id}/design-space">设计空间</a> <a class="button secondary" href="/projects/{id}/merge-requests">合并请求</a></p>
{triggers_section}
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
        git_repository = escape(&project.git_repository),
        git_branch = escape(&project.git_branch),
        design_space_display = design_space_display,
        design_space_branch = escape(&project.design_space_branch),
        skip_human_review = if project.skip_human_review {
            "开启"
        } else {
            "关闭"
        },
        test_capsule = project.test_capsule.as_str(),
        short_id = short_id,
        member_count = members.len(),
        version = project.version,
        description = project
            .description
            .as_deref()
            .map(escape)
            .unwrap_or_else(|| "未填写".to_string()),
        triggers_section = triggers_section,
        member_rows = member_rows,
        manage = manage,
    );
    layout(ctx, Section::Projects, &project.name, &body)
}

fn short_id(value: &str) -> &str {
    &value[..value.len().min(12)]
}

fn language_chip(lang: &str) -> &str {
    match lang {
        "rust" => "RS",
        "cpp" => "C++",
        "c" => "C",
        "python" => "PY",
        "typescript" => "TS",
        "javascript" => "JS",
        "jsx" => "JSX",
        "tsx" => "TSX",
        "go" => "GO",
        "java" => "JAVA",
        "kotlin" => "KT",
        "csharp" => "C#",
        "ruby" => "RB",
        "php" => "PHP",
        "swift" => "SWIFT",
        "scala" => "SCALA",
        "shell" => "SH",
        "sql" => "SQL",
        "lua" => "LUA",
        "dart" => "DART",
        "elixir" => "EX",
        "haskell" => "HS",
        "text" => "TXT",
        other if !other.is_empty() => other,
        _ => "—",
    }
}

fn is_document_kind(kind: &str) -> bool {
    matches!(
        kind,
        "document-directory"
            | "document"
            | "document-section"
            | "document-list"
            | "document-text"
            | "document-topic"
    )
}

fn document_chip(kind: &str) -> &'static str {
    match kind {
        "document-directory" => "DOC DIR",
        "document" => "DOC",
        "document-section" => "SEC",
        "document-list" => "LIST",
        "document-text" => "TEXT",
        "document-topic" => "TOPIC",
        _ => "DOC",
    }
}

fn element_kind_label(kind: &str) -> &str {
    match kind {
        "function" => "函数",
        "method" => "方法",
        "class" => "类",
        "struct" => "结构体",
        "interface" => "接口",
        "enum" => "枚举",
        "trait" => "Trait",
        "module" => "模块",
        "layer" => "层",
        "namespace" => "命名空间",
        "concept" => "概念",
        "variable" => "变量",
        "constant" => "常量",
        "macro" => "宏",
        "file" => "文件",
        "binary_file" => "二进制文件",
        "text_file" => "文本文件",
        "document-directory" => "文档目录",
        "document" => "文档",
        "document-section" => "章节",
        "document-list" => "列表",
        "document-text" => "文本",
        "document-topic" => "标题块",
        "document-block" => "代码块",
        "document-mermaid" => "流程图",
        "document-html" => "HTML 预览",
        "requirement-user" => "需求用户",
        "requirement-story" => "需求故事",
        "requirement-system" => "需求系统",
        "requirement-module" => "需求模块",
        "requirement-function" => "需求功能",
        "requirement-interaction" => "交互",
        other => other,
    }
}

/// Full design-space page: header, tree shell and the client-side tree.
pub fn design_space_page(
    ctx: &ConsoleContext,
    project: &Project,
    overview: &DesignSpaceOverview,
    branches: &[String],
    selected_branch: &str,
) -> String {
    let tree_url = format!("/projects/{}/design-space/tree", project.id);
    let element_url = format!("/projects/{}/design-space/elements/", project.id);
    let branch_options = branches
        .iter()
        .map(|branch| {
            format!(
                r#"<option value="{branch}"{selected}>{branch}</option>"#,
                branch = escape(branch),
                selected = if branch == selected_branch {
                    " selected"
                } else {
                    ""
                },
            )
        })
        .collect::<String>();
    let branch_switch = if branches.is_empty() {
        String::new()
    } else {
        format!(
            r#"<form class="ds-branch" method="get" action="/projects/{id}/design-space">
<label for="ds-branch">分支</label>
<select id="ds-branch" name="branch" onchange="if(this.form.requestSubmit) this.form.requestSubmit(); else this.form.submit();">{options}</select>
<noscript><button class="secondary" type="submit">切换</button></noscript>
</form>"#,
            id = project.id,
            options = branch_options,
        )
    };
    let version = match &overview.version {
        Some(version) => format!(
            r#"分支 <code>{branch}</code> · 版本 <code>{version}</code> · 源码 <code>{commit}</code>"#,
            branch = escape(&overview.branch),
            version = escape(short_id(version)),
            commit = escape(short_id(&overview.source_commit)),
        ),
        None => format!(
            r#"分支 <code>{branch}</code> · 尚无版本"#,
            branch = escape(&overview.branch)
        ),
    };
    let coverage = if overview.element_count == 0 {
        0
    } else {
        overview.designed_count * 100 / overview.element_count
    };
    let body = format!(
        r#"<p class="ds-crumbs"><a href="/projects/{id}">← {name}</a></p>
<header class="ds-head">
  <div>
    <h1>设计空间</h1>
    <p class="sub">{version}</p>
  </div>
  <div class="ds-stats">
    <div class="ds-stat"><b>{files}</b><span>文件</span></div>
    <div class="ds-stat"><b>{elements}</b><span>元素</span></div>
    <div class="ds-stat"><b>{documents}</b><span>文档</span></div>
    <div class="ds-stat"><b>{designed}</b><span>已有设计</span></div>
    <div class="ds-stat"><b>{coverage}%</b><span>覆盖率</span></div>
  </div>
</header>
<div class="ds-shell" id="ds-shell"
     data-project-id="{id}"
     data-branch="{selected_branch}"
     data-tree-url="{tree_url}"
     data-element-url="{element_url}">
  <aside class="ds-pane ds-tree-pane">
    <div class="ds-tools">
      {branch_switch}
      <input id="ds-search" type="search" placeholder="搜索元素、文件或路径…" autocomplete="off" spellcheck="false">
      <div class="ds-tools-row">
        <button class="secondary" type="button" id="ds-expand">展开全部</button>
        <button class="secondary" type="button" id="ds-collapse">收起全部</button>
      </div>
    </div>
    <div class="ds-tree" id="ds-tree" aria-label="设计空间树">
      <div class="ds-loading">正在加载元素树…</div>
    </div>
  </aside>
  <section class="ds-pane ds-detail-pane" id="ds-detail" aria-live="polite">
    <div class="ds-empty">
      <p>从左侧选择一个元素。</p>
      <p class="hint">绿色圆点表示该元素已有设计；点击“源码”可以查看对应代码。</p>
    </div>
  </section>
</div>"#,
        id = project.id,
        name = escape(&project.name),
        version = version,
        files = overview.file_count,
        elements = overview.element_count,
        documents = overview.document_count,
        designed = overview.designed_count,
        coverage = coverage,
        tree_url = escape(&tree_url),
        element_url = escape(&element_url),
        selected_branch = escape(selected_branch),
        branch_switch = branch_switch,
    );
    let mut html = layout_wide(ctx, Section::DesignSpace, &project.name, &body);
    html = html.replace("</body>", &format!("{DESIGN_SPACE_SCRIPT}</body>"));
    html
}

/// Page shown when the console cannot read this project's design space.
pub fn design_space_unavailable_page(
    ctx: &ConsoleContext,
    project: &Project,
    message: &str,
) -> String {
    let body = format!(
        r#"<p class="ds-crumbs"><a href="/projects/{id}">← {name}</a></p>
<h1>设计空间</h1>
<div class="ds-shell" style="grid-template-columns:1fr">
  <section class="ds-pane ds-detail-pane">
    <div class="ds-detail">
      <div class="ds-source-error">{message}</div>
      <p class="hint">控制台无法读取设计空间仓库；检查本地设计空间目录和项目地址。</p>
    </div>
  </section>
</div>"#,
        id = project.id,
        name = escape(&project.name),
        message = escape(message),
    );
    layout_wide(ctx, Section::DesignSpace, &project.name, &body)
}

/// One element's detail fragment: design tab plus a lazily loaded source tab.
pub fn design_space_element(detail: &DesignSpaceDetail, source_url: &str) -> String {
    let is_document = is_document_kind(&detail.kind);
    let chip = if detail.kind == "file" {
        r#"<span class="ds-chip file">FILE</span>"#.to_string()
    } else if detail.kind == "directory" {
        r#"<span class="ds-chip">目录</span>"#.to_string()
    } else if is_document {
        format!(
            r#"<span class="ds-chip">{}</span>"#,
            escape(document_chip(&detail.kind))
        )
    } else if detail.kind.starts_with("requirement-") {
        r#"<span class="ds-chip">需求</span>"#.to_string()
    } else if detail.lang.is_empty() {
        r#"<span class="ds-chip">概念</span>"#.to_string()
    } else {
        format!(
            r#"<span class="ds-chip">{}</span>"#,
            escape(language_chip(&detail.lang))
        )
    };
    let lines_label = if detail.kind == "directory" {
        "目录规则".to_string()
    } else if is_document {
        "文档".to_string()
    } else if detail.kind.starts_with("requirement-") {
        "需求对象".to_string()
    } else if detail.path.is_empty() {
        "无源码锚点".to_string()
    } else if detail.start_line == 0 {
        "整个文件".to_string()
    } else {
        format!("L{}–L{}", detail.start_line, detail.end_line)
    };
    let location = if detail.path.is_empty() {
        escape(&lines_label)
    } else {
        format!("{} · {}", escape(&detail.path), lines_label)
    };
    let has_source = !is_document
        && !detail.path.is_empty()
        && detail.kind != "directory"
        && !matches!(
            detail.kind.as_str(),
            "system" | "module" | "layer" | "design-structure"
        );
    let source_tabs = if has_source {
        r#"<button type="button" data-ds-tab="source">源码</button>"#.to_string()
    } else {
        String::new()
    };
    let source_panel = if !has_source {
        String::new()
    } else {
        format!(
            r#"<div class="ds-panel" data-ds-panel="source" hidden data-source-url="{source_url}">
    <div class="ds-loading">打开“源码”标签后加载…</div>
  </div>"#,
            source_url = escape(source_url)
        )
    };
    let short_name = if detail.kind == "file" {
        detail.path.rsplit('/').next().unwrap_or(&detail.path)
    } else {
        detail.name.rsplit("::").next().unwrap_or(&detail.name)
    };
    let full_name_line = if detail.name != short_name {
        format!(
            r#"<div class="ds-path">全名 <code>{}</code></div>"#,
            escape(&detail.name)
        )
    } else {
        String::new()
    };
    let members = if detail.members.is_empty() {
        String::new()
    } else {
        let rows = detail
            .members
            .iter()
            .map(|member| {
                format!(
                    r#"<li><span class="badge">{kind}</span> <code>{qname}</code> {name}</li>"#,
                    kind = escape(&member.kind),
                    qname = escape(&member.qname),
                    name = escape(&member.name),
                )
            })
            .collect::<String>();
        format!(r#"<section class="structure-members"><h3>成员</h3><ul>{rows}</ul></section>"#)
    };
    let comments = if detail.comments.is_empty() {
        String::new()
    } else {
        let rows = detail
            .comments
            .iter()
            .map(|comment| {
                format!(
                    r#"<div class="doc-comment" data-comment="{id}" data-comment-key="{key}" data-resolved="{resolved}">
  <div class="doc-comment-head"><span class="badge">{status}</span><strong>评论 {id}</strong><code>{target}</code></div>
  <div class="doc-comment-body">{text}</div>
</div>"#,
                    resolved = if comment.resolved { "true" } else { "false" },
                    status = if comment.resolved { "已解决" } else { "未解决" },
                    id = comment.id,
                    key = escape(&format!("{}#{}", comment.target, comment.id)),
                    target = escape(&comment.target),
                    text = escape(&comment.text).replace("\n", "<br>"),
                )
            })
            .collect::<String>();
        format!(r#"<div id="doc-comment-store" hidden>{rows}</div>"#)
    };
    let has_design = detail.design.is_some() || !detail.sections.is_empty();
    let design = if detail.kind == "document" && !detail.sections.is_empty() {
        lazy_document_view(&detail.sections)
    } else {
        match &detail.design {
            Some(text) if !text.trim().is_empty() && detail.kind == "document" => document_view(text),
            Some(text) if !text.trim().is_empty() => format!(
                r#"<div class="markdown">{}</div>"#,
                markdown_to_html(text)
            ),
            Some(_) => r#"<div class="ds-empty">设计为空。</div>"#.to_string(),
            None if detail.kind == "file" => {
                r#"<div class="ds-empty">文件对象没有设计文本，可以查看源码。</div>"#.to_string()
            }
            None if detail.kind == "layer" => {
                r#"<div class="ds-empty">该层还没有关联设计文档。</div>"#.to_string()
            }
            None if matches!(detail.kind.as_str(), "system" | "module" | "design-structure") => {
                r#"<div class="ds-empty">该系统或模块还没有关联设计文档。</div>"#.to_string()
            }
            None => {
                r#"<div class="ds-empty">尚未为该元素编写设计。可在本地使用 <code>arche doc</code> 补充。</div>"#
                    .to_string()
            }
        }
    };
    format!(
        r#"<div class="ds-detail" data-hash="{hash}">
  <div class="ds-detail-head">
    <div class="ds-detail-kinds">{chip}<span class="badge">{kind}</span><span class="ds-dot {dot}" title="{dot_title}"></span></div>
    <h2 title="{full_name}">{name}</h2>
    {full_name_line}
    <div class="ds-path">{location}</div>
    <div class="ds-path">对象 <code title="{hash}">{short}</code> · 设计 <code>{design_state}</code></div>
  </div>
  <div class="ds-tabs" role="tablist">
    <button type="button" class="active" data-ds-tab="design">设计</button>
    {source_tabs}
  </div>
  <div class="ds-panel" data-ds-panel="design">{design}</div>
  {members}
  {comments}
  {source_panel}
</div>"#,
        hash = escape(&detail.hash),
        chip = chip,
        kind = escape(element_kind_label(&detail.kind)),
        dot = if has_design { "on" } else { "off" },
        dot_title = if has_design {
            "已有设计"
        } else {
            "尚无设计"
        },
        name = escape(short_name),
        full_name = escape(&detail.name),
        full_name_line = full_name_line,
        location = location,
        source_tabs = source_tabs,
        source_panel = source_panel,
        short = escape(short_id(&detail.hash)),
        design_state = if has_design { "已编写" } else { "缺失" },
        design = design,
        members = members,
        comments = comments,
    )
}

pub fn design_space_element_error(message: &str) -> String {
    format!(
        r#"<div class="ds-detail"><div class="ds-source-error">{}</div></div>"#,
        escape(message)
    )
}

/// Source lines around the element, with the element's own lines highlighted.
pub fn design_space_source(source: &DesignSpaceSource) -> String {
    let lines = source
        .lines
        .iter()
        .map(|line| {
            format!(
                r#"<div class="ds-line{in_element}"><span class="ln">{number}</span><span class="tx">{text}</span></div>"#,
                in_element = if line.in_element { " in" } else { "" },
                number = line.number,
                text = escape(&line.text),
            )
        })
        .collect::<String>();
    let element_span = if source.start_line == 0 {
        String::new()
    } else {
        format!(
            "<span>元素 {start}–{end}</span>",
            start = source.start_line,
            end = source.end_line
        )
    };
    format!(
        r#"<div class="ds-source">
  <div class="ds-source-meta">
    <span>commit {commit}</span>
    <span>{path}</span>
    <span>第 {first}–{last} 行 / 共 {total} 行</span>
    {element_span}
  </div>
  <div class="ds-code">{lines}</div>
</div>"#,
        commit = escape(short_id(&source.commit)),
        path = escape(&source.path),
        first = source.first_line,
        last = source.last_line,
        total = source.total_lines,
        element_span = element_span,
        lines = lines,
    )
}

pub fn design_space_source_error(message: &str) -> String {
    format!(r#"<div class="ds-source-error">{}</div>"#, escape(message))
}

// ---------------------------------------------------------------------------
// Design-space merge requests
// ---------------------------------------------------------------------------

pub fn merge_requests_page(
    ctx: &ConsoleContext,
    project: &Project,
    requests: &[MergeRequestReview],
    branches: &[String],
    error: Option<&str>,
) -> String {
    let rows = if requests.is_empty() {
        r#"<tr><td colspan="5" class="empty">还没有合并请求。</td></tr>"#.to_string()
    } else {
        requests
            .iter()
            .map(|review| {
                let request = &review.merge_request;
                format!(
                    r#"<tr>
<td><a href="/projects/{project}/merge-requests/{id}">{title}</a></td>
<td><code>{source}</code> → <code>{target}</code></td>
<td><span class="badge">{status}</span></td>
<td>{stage}</td>
<td>{created}</td>
</tr>"#,
                    project = project.id,
                    id = request.id,
                    title = escape(&request.title),
                    source = escape(&request.source_branch),
                    target = escape(&request.target_branch),
                    status = merge_request_status_label(review.status),
                    stage = review
                        .task
                        .stage
                        .as_ref()
                        .map(|stage| escape(stage.as_str()))
                        .unwrap_or_else(|| "—".to_string()),
                    created = request
                        .created_at
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let error = error
        .map(|message| format!(r#"<div class="card">{}</div>"#, escape(message)))
        .unwrap_or_default();
    let branch_options = branches
        .iter()
        .map(|branch| format!(r#"<option value="{0}">{0}</option>"#, escape(branch)))
        .collect::<String>();
    let branch_list = if branches.is_empty() {
        String::new()
    } else {
        format!(r#"<datalist id="mr-branches">{branch_options}</datalist>"#)
    };
    let create_form = if ctx.can_contribute() {
        format!(
            r#"<details class="card">
<summary>新建设计空间合并请求</summary>
<form method="post" action="/projects/{id}/merge-requests">
  <div class="field"><label for="mr-title">标题</label><input id="mr-title" name="title" required></div>
  <div class="field"><label for="mr-description">说明</label><textarea id="mr-description" name="description"></textarea></div>
  <div class="field"><label for="mr-source">源分支</label><input id="mr-source" name="source_branch" list="mr-branches" required></div>
  <div class="field"><label for="mr-target">目标分支</label><input id="mr-target" name="target_branch" list="mr-branches" value="{target}" required></div>
  {branch_list}
  <button type="submit">创建设计空间合并请求</button>
</form>
</details>"#,
            id = project.id,
            target = escape(&project.design_space_branch),
            branch_list = branch_list,
        )
    } else {
        String::new()
    };
    let body = format!(
        r#"<p><a href="/projects/{id}">← {name}</a></p>
<h1>设计空间合并请求</h1>
<p class="sub">设计空间合并请求的审批、评论和操作都在关联任务中完成。</p>
{error}
{create_form}
<table>
<thead><tr><th>标题</th><th>分支</th><th>状态</th><th>任务阶段</th><th>创建时间</th></tr></thead>
<tbody>{rows}</tbody>
</table>"#,
        id = project.id,
        name = escape(&project.name),
        create_form = create_form,
        error = error,
        rows = rows,
    );
    layout(ctx, Section::MergeRequests, &project.name, &body)
}

pub fn merge_request_changes(detail: &MergeRequestDetail) -> String {
    let document_objects = detail
        .documents
        .iter()
        .flat_map(|document| document.objects.iter().map(|object| object.qname.as_str()))
        .collect::<BTreeSet<_>>();
    let other_changes = detail
        .changes
        .iter()
        .filter(|change| !document_objects.contains(change.qname.as_str()))
        .collect::<Vec<_>>();
    let other_changes = if other_changes.is_empty() {
        r#"<p class="hint">没有文档以外的对象变更。</p>"#.to_string()
    } else {
        format!(
            r#"<table>
<thead><tr><th>状态</th><th>对象</th><th>类型</th></tr></thead>
<tbody>{}</tbody>
</table>"#,
            other_changes
                .iter()
                .map(|change| {
                    format!(
                        r#"<tr><td>{status}</td><td><code>{qname}</code></td><td>{kind}</td></tr>"#,
                        status = merge_request_change_label(change.status),
                        qname = escape(&change.qname),
                        kind = escape(&change.kind),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    let documents = if detail.documents.is_empty() {
        r#"<p class="hint">没有文档级差异。</p>"#.to_string()
    } else {
        detail
            .documents
            .iter()
            .map(|document| {
                let status = merge_request_change_label(document.status);
                let diff = merge_request_diff_html(
                    document.before.as_deref(),
                    document.after.as_deref(),
                );
                let objects = document
                    .objects
                    .iter()
                    .map(|object| {
                        let fields = if object.fields.is_empty() {
                            r#"<p class="hint">仅正文内容变化，见上方文档 diff。</p>"#
                                .to_string()
                        } else {
                            format!(
                                r#"<table class="field-diff"><tbody>{}</tbody></table>"#,
                                object
                                    .fields
                                    .iter()
                                    .map(|field| {
                                        format!(
                                            r#"<tr><th>{field}</th><td class="diff-before">{before}</td><td class="diff-after">{after}</td></tr>"#,
                                            field = escape(&field.field),
                                            before = field
                                                .before
                                                .as_deref()
                                                .map(escape)
                                                .unwrap_or_else(|| "—".to_string()),
                                            after = field
                                                .after
                                                .as_deref()
                                                .map(escape)
                                                .unwrap_or_else(|| "—".to_string()),
                                        )
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            )
                        };
                        format!(
                            r#"<details class="mr-object"><summary><span class="badge">{status}</span> <code>{qname}</code> <span class="muted">{kind}</span></summary>{fields}</details>"#,
                            status = merge_request_change_label(object.status),
                            qname = escape(&object.qname),
                            kind = escape(&object.kind),
                            fields = fields,
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    r#"<details class="mr-doc" open><summary><span class="badge">{status}</span> <strong>{title}</strong> <code>{qname}</code> <span class="muted">{count} 个对象</span></summary>
<pre class="diff">{diff}</pre>
<details class="mr-objects"><summary>对象变更（{count}）</summary>{objects}</details>
</details>"#,
                    status = status,
                    title = escape(&document.title),
                    qname = escape(&document.qname),
                    count = document.objects.len(),
                    diff = diff,
                    objects = objects,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let added = detail
        .changes
        .iter()
        .filter(|change| change.status == MergeRequestChangeStatus::Added)
        .count();
    let modified = detail
        .changes
        .iter()
        .filter(|change| change.status == MergeRequestChangeStatus::Modified)
        .count();
    let removed = detail
        .changes
        .iter()
        .filter(|change| change.status == MergeRequestChangeStatus::Removed)
        .count();
    format!(
        r#"<p class="hint">新增 {added}，修改 {modified}，删除 {removed}；变更文档 {} 个。</p>
{documents}
<h3>其他对象</h3>
{other_changes}"#,
        detail.documents.len()
    )
}

pub fn merge_request_page(
    ctx: &ConsoleContext,
    project: &Project,
    review: &MergeRequestReview,
) -> String {
    let request = &review.merge_request;
    let task = &review.task;
    let changes_url = format!(
        "/projects/{}/merge-requests/{}/changes",
        project.id, request.id
    );
    let changes_lazy = format!(
        r#"<details class="mr-changes" id="mr-changes" data-changes-url="{url}">
<summary>展开变更</summary>
<div class="mr-changes-body"><p class="hint">展开后加载文档和对象变更。</p></div>
</details>
<script>
(() => {{
  const details = document.getElementById('mr-changes');
  if (!details) return;
  const body = details.querySelector('.mr-changes-body');
  let loaded = false;
  let loading = false;
  details.addEventListener('toggle', async () => {{
    if (!details.open || loaded || loading) return;
    loading = true;
    body.innerHTML = '<p class="hint">正在加载变更…</p>';
    try {{
      const response = await fetch(details.dataset.changesUrl, {{ headers: {{ accept: 'text/html' }} }});
      const html = await response.text();
      if (!response.ok) throw new Error('HTTP ' + response.status);
      body.innerHTML = html;
      loaded = true;
    }} catch (error) {{
      const message = document.createElement('div');
      message.className = 'error';
      message.textContent = '无法加载变更：' + error.message;
      body.replaceChildren(message);
    }} finally {{
      loading = false;
    }}
  }});
}})();
</script>"#,
        url = escape(&changes_url)
    );
    let actions = if task.allowed_operations.is_empty() {
        r#"<p class="hint">当前账号没有可执行的审核操作。审核由关联任务中绑定的真人审核人完成。</p>"#
            .to_string()
    } else {
        task.allowed_operations
            .iter()
            .filter(|operation| !operation.name.as_str().ends_with(":conflict"))
            .map(|operation| {
                let name = operation.name.as_str();
                let mechanical_approval = name.ends_with(":merge")
                    && task.stage.as_ref().map(|stage| stage.as_str()) == Some("human_review");
                let approved = name.ends_with(":approved")
                    || name.ends_with(":review_approved")
                    || mechanical_approval;
                if approved {
                    format!(
                        r#"<form class="inline-form" method="post" action="/projects/{project}/merge-requests/{id}/approve">
<button type="submit">{label}</button>
</form>"#,
                        project = project.id,
                        id = request.id,
                        label = escape(&operation.display_name),
                    )
                } else {
                    format!(
                        r#"<form class="inline-form" method="post" action="/projects/{project}/merge-requests/{id}/operations">
<input type="hidden" name="operation" value="{operation}">
<input type="hidden" name="idempotency_key" value="design-space-mr:{id}:{operation}">
<button type="submit">{label}</button>
</form>"#,
                        project = project.id,
                        id = request.id,
                        operation = escape(name),
                        label = escape(&operation.display_name),
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let comments = if task.comments.is_empty() {
        r#"<p class="hint">还没有评论。</p>"#.to_string()
    } else {
        task.comments
            .iter()
            .map(|comment| {
                format!(
                    r#"<div class="card"><div class="meta"><strong>{author}</strong><span>{at}</span></div><div class="markdown">{body}</div></div>"#,
                    author = escape(&comment.author_display_name),
                    at = comment
                        .created_at
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                    body = markdown_to_html(&comment.body),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let description = if request.description.trim().is_empty() {
        String::new()
    } else {
        format!(
            r#"<div class="card markdown">{}</div>"#,
            markdown_to_html(&request.description)
        )
    };
    let body = format!(
        r#"<p><a href="/projects/{project}/merge-requests">← 返回设计空间合并请求</a></p>
<h1>{title}</h1>
<p class="sub"><span class="badge">{status}</span> 任务阶段 <code>{stage}</code></p>
<div class="details">
  <dt>源分支</dt><dd>{source} <code>{source_commit}</code></dd>
  <dt>目标分支</dt><dd>{target} <code>{target_commit}</code></dd>
  <dt>合并提交</dt><dd>{merge_commit}</dd>
  <dt>审核任务</dt><dd><a href="/tasks/{task_id}">{task_title}</a></dd>
  <dt>创建者</dt><dd>{created_by}</dd>
</div>
{description}
<h2>审核</h2>
<div class="approval-actions">{actions}</div>
<h2>变更</h2>
{changes_lazy}
<h2>评论</h2>
{comments}
<form class="card" method="post" action="/projects/{project}/merge-requests/{id}/comments">
  <div class="field"><label for="mr-comment">评论</label><textarea id="mr-comment" name="body" required></textarea></div>
  <button type="submit">发表评论</button>
</form>"#,
        project = project.id,
        id = request.id,
        title = escape(&request.title),
        status = merge_request_status_label(review.status),
        stage = task
            .stage
            .as_ref()
            .map(|stage| escape(stage.as_str()))
            .unwrap_or_else(|| "—".to_string()),
        source = escape(&request.source_branch),
        source_commit = escape(&request.source_commit),
        target = escape(&request.target_branch),
        target_commit = escape(&request.target_commit),
        merge_commit = request
            .merge_commit
            .as_deref()
            .map(escape)
            .unwrap_or_else(|| "—".to_string()),
        task_id = task.id,
        task_title = escape(&task.title),
        created_by = request.created_by,
        description = description,
        actions = actions,
        changes_lazy = changes_lazy,
        comments = comments,
    );
    layout(ctx, Section::MergeRequests, &request.title, &body)
}

fn merge_request_status_label(status: MergeRequestStatus) -> &'static str {
    match status {
        MergeRequestStatus::Reviewing => "审核中",
        MergeRequestStatus::Approved => "已批准",
        MergeRequestStatus::Merged => "已合并",
        MergeRequestStatus::Rejected => "已拒绝",
        MergeRequestStatus::Closed => "已关闭",
    }
}

fn merge_request_change_label(status: MergeRequestChangeStatus) -> &'static str {
    match status {
        MergeRequestChangeStatus::Added => "新增",
        MergeRequestChangeStatus::Modified => "修改",
        MergeRequestChangeStatus::Removed => "删除",
    }
}

fn merge_request_diff_html(before: Option<&str>, after: Option<&str>) -> String {
    let before_lines = before
        .map(|text| text.lines().collect::<Vec<_>>())
        .unwrap_or_default();
    let after_lines = after
        .map(|text| text.lines().collect::<Vec<_>>())
        .unwrap_or_default();
    if before.is_some() && before == after {
        return r#"<span class="diff-line diff-gap">（渲染内容无变化）</span>"#.to_string();
    }
    let mut rows: Vec<(&str, String)> = Vec::new();
    match (before, after) {
        (None, Some(_)) => {
            rows.extend(
                after_lines
                    .iter()
                    .map(|line| ("diff-add", format!("+ {line}"))),
            );
        }
        (Some(_), None) => {
            rows.extend(
                before_lines
                    .iter()
                    .map(|line| ("diff-del", format!("- {line}"))),
            );
        }
        (Some(_), Some(_)) => {
            let prefix = before_lines
                .iter()
                .zip(after_lines.iter())
                .take_while(|(left, right)| left == right)
                .count();
            let max_suffix = before_lines
                .len()
                .min(after_lines.len())
                .saturating_sub(prefix);
            let suffix = before_lines
                .iter()
                .rev()
                .zip(after_lines.iter().rev())
                .take(max_suffix)
                .take_while(|(left, right)| left == right)
                .count();
            let context = 4usize;
            if prefix > context {
                rows.push(("diff-gap", "…".to_string()));
            }
            let prefix_start = prefix.saturating_sub(context);
            rows.extend(
                before_lines[prefix_start..prefix]
                    .iter()
                    .map(|line| ("diff-context", format!("  {line}"))),
            );
            let before_middle = &before_lines[prefix..before_lines.len() - suffix];
            let after_middle = &after_lines[prefix..after_lines.len() - suffix];
            rows.extend(
                before_middle
                    .iter()
                    .map(|line| ("diff-del", format!("- {line}"))),
            );
            rows.extend(
                after_middle
                    .iter()
                    .map(|line| ("diff-add", format!("+ {line}"))),
            );
            let suffix_end = suffix.min(context);
            rows.extend(
                before_lines[before_lines.len() - suffix..before_lines.len() - suffix + suffix_end]
                    .iter()
                    .map(|line| ("diff-context", format!("  {line}"))),
            );
            if suffix > context {
                rows.push(("diff-gap", "…".to_string()));
            }
            if rows.is_empty() {
                rows.push(("diff-gap", "（渲染内容无变化）".to_string()));
            }
        }
        (None, None) => rows.push(("diff-gap", "（没有内容）".to_string())),
    }
    const MAX_LINES: usize = 3000;
    if rows.len() > MAX_LINES {
        rows.truncate(MAX_LINES);
        rows.push(("diff-gap", "…（diff 过长，已截断）".to_string()));
    }
    rows.into_iter()
        .map(|(class, line)| {
            format!(
                r#"<span class="diff-line {class}">{line}</span>"#,
                class = class,
                line = escape(&line)
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

const DESIGN_SPACE_SCRIPT: &str = r##"<script>
(() => {
  const shell = document.getElementById('ds-shell');
  if (!shell) return;
  const treeEl = document.getElementById('ds-tree');
  const detailEl = document.getElementById('ds-detail');
  const searchEl = document.getElementById('ds-search');
  const expandBtn = document.getElementById('ds-expand');
  const collapseBtn = document.getElementById('ds-collapse');
  const branch = shell.dataset.branch || '';
  const withBranch = (url) => {
    if (!branch) return url;
    return url + (url.includes('?') ? '&' : '?') + 'branch=' + encodeURIComponent(branch);
  };
  const treeUrl = withBranch(shell.dataset.treeUrl);
  const elementUrl = shell.dataset.elementUrl;
  const storeKey = 'foundry.ds.tree.v3.' + shell.dataset.projectId;
  const glyphs = {
    rust: 'RS', cpp: 'C++', c: 'C', python: 'PY', typescript: 'TS',
    javascript: 'JS', jsx: 'JSX', tsx: 'TSX', go: 'GO', java: 'JAVA',
    kotlin: 'KT', csharp: 'C#', ruby: 'RB', php: 'PHP', swift: 'SWIFT',
    scala: 'SCALA', shell: 'SH', sql: 'SQL', lua: 'LUA', dart: 'DART',
    elixir: 'EX', haskell: 'HS', text: 'TXT'
  };
  const folderIcon = '<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M1.5 3.5h4.2l1.3 1.6h7.5v7.4h-13z" fill="none" stroke="currentColor" stroke-linejoin="round"/></svg>';
  const langLabel = (lang) => glyphs[lang] || (lang ? lang.slice(0, 4).toUpperCase() : '');
  const codeKindLabels = {
    'class': '类', 'inner-class': '内部类',
    struct: '结构', 'inner-struct': '内部结构',
    enum: '枚举', 'inner-enum': '内部枚举',
    interface: '接口', 'inner-interface': '内部接口',
    method: '方法', function: '函数', mixin: '混入', part: '部件'
  };
  const requirementKindLabels = {
    'requirement-user': '用户',
    'requirement-story': '故事',
    'requirement-system': '系统',
    'requirement-module': '模块',
    'requirement-function': '功能',
    'requirement-interaction': '交互'
  };
  const shortName = (name) => { const parts = String(name).split(/::|\//); return parts[parts.length - 1] || String(name); };
  const skeletonQnames = new Set([
    'project',
    'technical', 'technical/directory', 'technical/file', 'technical/namespace', 'technical/system',
    'requirement', 'requirement/user', 'requirement/story', 'requirement/system',
    'concept', 'concept/concepts', 'concept/directories',
    'concept/domain', 'concept/domain/concepts', 'concept/domain/directories',
    'concept/requirement', 'concept/requirement/concepts', 'concept/requirement/directories',
    'concept/technical', 'concept/technical/concepts', 'concept/technical/directories',
  ]);
  const isSkeleton = (item) => skeletonQnames.has(String(item.qname));
  let expanded = new Set([
    'root', 'view:namespaces', 'view:concepts', 'view:structure', 'view:documents',
    'arche:project', 'arche:technical', 'arche:requirement', 'arche:concept',
    'ar:project', 'ar:technical', 'ar:requirement', 'ar:concept',
    'ar:concept/domain', 'ar:concept/requirement', 'ar:concept/technical',
  ]);
  try {
    const saved = localStorage.getItem(storeKey);
    if (saved) expanded = new Set(JSON.parse(saved));
  } catch (_) {}
  let data = null;
  let selected = null;
  let query = '';
  let timer = 0;
  const sourceCache = new Map();
  const save = () => { try { localStorage.setItem(storeKey, JSON.stringify([...expanded])); } catch (_) {} };
  const el = (tag, cls, text) => {
    const node = document.createElement(tag);
    if (cls) node.className = cls;
    if (text !== undefined) node.textContent = text;
    return node;
  };
  function chip(item) {
    let label;
    if (isSkeleton(item)) {
      label = item.kind === 'list' ? '列表' : item.kind === 'concept-directory' ? '概念目录' : '空间';
    }
    else if (item.isFile) label = 'FILE';
    else if (item.isStructure) label = item.kind === 'system' ? 'SYS' : item.kind === 'module' ? 'MOD' : item.kind === 'layer' ? 'LAY' : '结构';
    else if (item.kind === 'project-document') label = '说明书';
    else if (item.kind === 'concept') label = '概念';
    else if (item.kind === 'concept-directory') label = '概念目录';
    else if (requirementKindLabels[item.kind]) label = requirementKindLabels[item.kind];
    else if (item.kind === 'namespace') label = 'NS';
    else label = langLabel(item.lang) || '—';
    const node = el('span', 'ds-chip' + (item.isFile ? ' file' : ''), label);
    node.title = (item.lang ? item.lang + ' · ' : '') + item.kind;
    return node;
  }

  function kindBadge(item) {
    const label = codeKindLabels[item.kind];
    if (!label) return null;
    const node = el('span', 'ds-kind', label);
    node.title = item.kind;
    return node;
  }

  function toggle(key) {
    if (expanded.has(key)) expanded.delete(key);
    else expanded.add(key);
    save();
    renderTree();
  }

  function caret(hasChildren, key) {
    const node = el('span', 'ds-caret' + (hasChildren ? '' : ' empty'));
    if (hasChildren) {
      node.addEventListener('click', (event) => {
        event.stopPropagation();
        toggle(key);
      });
    }
    return node;
  }

  function countLabel(count, unit) {
    return count ? count + ' ' + unit : '';
  }

  function virtualRow(key, label, meta, icon, children) {
    const wrap = el('div');
    const open = expanded.has(key);
    const row = el('div', 'ds-row' + (open ? ' open' : ''));
    row.appendChild(caret(true, key));
    const glyph = el('span', 'ds-folder');
    glyph.innerHTML = icon;
    row.appendChild(glyph);
    const name = el('span', 'ds-name', label);
    row.appendChild(name);
    row.appendChild(el('span', 'ds-count', meta));
    row.addEventListener('click', () => toggle(key));
    wrap.appendChild(row);
    if (open) {
      const kids = children();
      kids.classList.add('ds-children');
      wrap.appendChild(kids);
    }
    return wrap;
  }

  function searchRow(item) {
    const row = el('div', 'ds-row');
    if (selected === item.hash) row.classList.add('active');
    row.dataset.hash = item.hash;
    row.appendChild(el('span', 'ds-caret empty'));
    row.appendChild(chip(item));
    const name = el('span', 'ds-name', item.name || shortName(item.qname));
    name.title = item.isFile ? item.path : item.name;
    row.appendChild(name);
    const kind = kindBadge(item);
    if (kind) row.appendChild(kind);
    row.appendChild(el('span', 'ds-dot ' + (item.hasDesign ? 'on' : 'off')));
    const meta = item.path + (item.startLine ? ':' + item.startLine : '');
    row.appendChild(el('span', 'ds-meta', meta));
    if (isSkeleton(item)) row.classList.add('ds-skeleton');
    else row.addEventListener('click', () => select(item));
    return row;
  }

  function renderSearch() {
    const matches = data.elements.filter((item) =>
      (item.name + ' ' + (item.qname || '') + ' ' + item.path).toLowerCase().includes(query));
    if (!matches.length) {
      treeEl.replaceChildren(el('div', 'ds-empty', '没有匹配的元素'));
      return;
    }
    const shown = matches.slice(0, 400);
    const list = el('div', 'ds-list');
    for (const item of shown) list.appendChild(searchRow(item));
    const note = matches.length > shown.length
      ? '显示前 ' + shown.length + ' / ' + matches.length + ' 个结果'
      : matches.length + ' 个结果';
    treeEl.replaceChildren(list, el('div', 'ds-empty', note));
  }

  function isArcheData(elements) {
    return elements.some((item) => /^(technical|requirement|concept)(\/|$)/.test(String(item.qname || '')));
  }

  function archeTree(elements) {
    const byQname = new Map();
    for (const item of elements) byQname.set(String(item.qname), item);
    const children = new Map();
    const hasParent = new Set();
    for (const item of elements) {
      const kids = (item.members || [])
        .map((qname) => byQname.get(String(qname)))
        .filter(Boolean);
      if (kids.length) children.set(String(item.qname), kids);
      for (const kid of kids) hasParent.add(String(kid.qname));
    }
    return {
      children,
      roots: elements.filter((item) => !hasParent.has(String(item.qname))),
    };
  }

  function archeRow(item, children) {
    const qname = String(item.qname);
    const kids = children.get(qname) || [];
    const key = 'ar:' + qname;
    const open = expanded.has(key);
    const wrap = el('div');
    const row = el('div', 'ds-row' + (open ? ' open' : ''));
    if (selected === item.hash) row.classList.add('active');
    row.dataset.hash = item.hash;
    row.appendChild(caret(kids.length > 0, key));
    row.appendChild(chip(item));
    const name = el('span', 'ds-name', item.name || shortName(qname));
    name.title = qname;
    row.appendChild(name);
    const badge = kindBadge(item);
    if (badge) row.appendChild(badge);
    row.appendChild(el('span', 'ds-dot ' + (item.hasDesign ? 'on' : 'off')));
    row.appendChild(el('span', 'ds-meta', item.startLine ? 'L' + item.startLine + '–' + item.endLine : ''));
    if (isSkeleton(item)) {
      row.classList.add('ds-skeleton');
      row.addEventListener('click', () => toggle(key));
    } else {
      row.addEventListener('click', () => select(item));
    }
    wrap.appendChild(row);
    if (open && kids.length) {
      const list = el('div', 'ds-children');
      for (const kid of kids) list.appendChild(archeRow(kid, children));
      wrap.appendChild(list);
    }
    return wrap;
  }

  function treeCount(items, children) {
    return items.reduce((count, item) =>
      count + 1 + treeCount(children.get(String(item.qname)) || [], children), 0);
  }

  function renderArcheTree(elements) {
    const { children, roots } = archeTree(elements);
    const groups = [
      { key: 'arche:project', label: '项目空间', prefix: 'project' },
      { key: 'arche:technical', label: '技术空间', prefix: 'technical' },
      { key: 'arche:requirement', label: '需求空间', prefix: 'requirement' },
      { key: 'arche:concept', label: '概念空间', prefix: 'concept' },
    ];
    const list = el('div', 'ds-list');
    for (const group of groups) {
      const root = roots.find((item) => String(item.qname) === group.prefix);
      const items = root
        ? (children.get(group.prefix) || [])
        : roots.filter((item) => {
            const qname = String(item.qname);
            return qname !== group.prefix && qname.startsWith(group.prefix + '/');
          });
      if (!items.length) continue;
      list.appendChild(virtualRow(
        group.key,
        group.label,
        countLabel(treeCount(items, children), '项'),
        folderIcon,
        () => {
          const kids = el('div', 'ds-list');
          for (const item of items) kids.appendChild(archeRow(item, children));
          return kids;
        }
      ));
    }
    treeEl.replaceChildren(list);
  }

  function renderTree() {
    if (!data) return;
    if (query) {
      renderSearch();
      return;
    }
    if (!data.elements.length) {
      treeEl.replaceChildren(el('div', 'ds-empty',
        '设计空间还没有元素。用 arche create 创建元素并 arche push 后刷新页面。'));
      return;
    }
    if (isArcheData(data.elements)) {
      renderArcheTree(data.elements);
      return;
    }
  }

  let mermaidLoader = null;
  function loadMermaid() {
    if (window.mermaid) return Promise.resolve(window.mermaid);
    if (!mermaidLoader) {
      mermaidLoader = new Promise((resolve, reject) => {
        const script = document.createElement('script');
        script.src = '/assets/mermaid.min.js';
        script.onload = () => window.mermaid ? resolve(window.mermaid) : reject(new Error('mermaid 未加载'));
        script.onerror = () => reject(new Error('mermaid 加载失败'));
        document.head.appendChild(script);
      });
    }
    return mermaidLoader;
  }

  function replaceMermaidPre(code, content, isError) {
    const pre = code.closest('pre');
    if (!pre) return;
    const node = document.createElement('div');
    if (isError) {
      node.className = 'mermaid-error';
      node.textContent = content;
    } else {
      node.className = 'mermaid-diagram';
      node.innerHTML = content;
    }
    pre.replaceWith(node);
  }

  function htmlPreviewDocument(source) {
    const csp = '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'; style-src \'unsafe-inline\'; img-src data:; font-src data:">';
    const doctype = source.match(/^\s*<!doctype[^>]*>/i);
    if (doctype) {
      const end = doctype[0].length;
      return source.slice(0, end) + csp + source.slice(end);
    }
    return '<!doctype html><meta charset="utf-8">' + csp + source;
  }

  function renderHtmlPreviews(root) {
    const codes = [...root.querySelectorAll('pre > code.language-html-preview')];
    for (const code of codes) {
      const pre = code.closest('pre');
      if (!pre) continue;
      const frame = document.createElement('iframe');
      frame.className = 'html-preview';
      frame.loading = 'lazy';
      frame.title = 'HTML 预览';
      frame.setAttribute('sandbox', '');
      frame.srcdoc = htmlPreviewDocument(code.textContent || '');
      pre.replaceWith(frame);
    }
  }

  async function renderMermaid(root) {
    const codes = [...root.querySelectorAll('pre > code.language-mermaid')];
    if (!codes.length) return;
    let mermaid;
    try {
      mermaid = await loadMermaid();
    } catch (error) {
      for (const code of codes) {
        replaceMermaidPre(code, '流程图加载失败：' + (error && error.message ? error.message : error), true);
      }
      return;
    }
    const dark = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
    mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', theme: dark ? 'dark' : 'default' });
    for (const [index, code] of codes.entries()) {
      const id = 'ds-mermaid-' + Date.now() + '-' + index;
      try {
        const result = await mermaid.render(id, code.textContent || '');
        replaceMermaidPre(code, result.svg, false);
      } catch (error) {
        replaceMermaidPre(code, '流程图渲染失败：' + (error && error.message ? error.message : error), true);
      }
    }
  }

  async function select(item) {
    if (!item || !item.hash) return;
    closeCommentPopover();
    selected = item.hash;
    for (const row of document.querySelectorAll('.ds-row.active')) row.classList.remove('active');
    for (const row of document.querySelectorAll('.ds-row[data-hash="' + CSS.escape(item.hash) + '"]')) {
      row.classList.add('active');
    }
    detailEl.replaceChildren(el('div', 'ds-loading', '加载元素…'));
    try {
      const response = await fetch(withBranch(elementUrl + encodeURIComponent(item.hash)), { headers: { accept: 'text/html' } });
      if (!response.ok) throw new Error('HTTP ' + response.status);
      detailEl.innerHTML = await response.text();
      bindTabs();
      bindElementLinks();
      bindLazySections();
      renderMermaid(detailEl).catch(() => {});
      renderHtmlPreviews(detailEl);
    } catch (error) {
      detailEl.replaceChildren(el('div', 'ds-source-error', '无法加载元素：' + error.message));
    }
  }

  function bindElementLinks() {
    for (const link of detailEl.querySelectorAll('a[href^="#ds-"]')) {
      link.addEventListener('click', (event) => {
        const hash = link.getAttribute('href').slice(4);
        const target = (data.elements || []).find((item) => String(item.hash) === hash);
        if (!target) return;
        event.preventDefault();
        select(target);
      });
    }
  }

  function bindTabs() {
    const buttons = detailEl.querySelectorAll('[data-ds-tab]');
    buttons.forEach((button) => {
      button.addEventListener('click', () => {
        for (const other of buttons) other.classList.toggle('active', other === button);
        for (const panel of detailEl.querySelectorAll('[data-ds-panel]')) {
          panel.hidden = panel.dataset.dsPanel !== button.dataset.dsTab;
        }
        if (button.dataset.dsTab === 'source') loadSource();
      });
    });
  }

  function bindLazySections() {
    const sections = [...detailEl.querySelectorAll('details.doc-section[data-section-qname]')];
    for (const details of sections) {
      if (details.dataset.bound === '1') continue;
      details.dataset.bound = '1';
      details.addEventListener('toggle', () => {
        if (!details.open || details.dataset.loaded === '1' || details.dataset.loading === '1') return;
        const body = details.querySelector('[data-section-content]');
        if (!body) return;
        const qname = details.dataset.sectionQname;
        const base = withBranch(elementUrl + encodeURIComponent(selected) + '/section');
        const url = base + (base.includes('?') ? '&' : '?') + 'qname=' + encodeURIComponent(qname);
        details.dataset.loading = '1';
        body.replaceChildren(el('p', 'hint', '加载章节…'));
        fetch(url, { headers: { accept: 'text/html' } })
          .then(async (response) => {
            const html = await response.text();
            if (!response.ok) throw new Error('HTTP ' + response.status);
            if (html.trim()) {
              body.innerHTML = html;
              renderMermaid(body).catch(() => {});
              renderHtmlPreviews(body);
            } else {
              body.replaceChildren(el('p', 'hint', '本节暂无正文。'));
            }
            details.dataset.loaded = '1';
          })
          .catch((error) => {
            body.replaceChildren(el('div', 'ds-source-error', '无法加载章节：' + error.message));
          })
          .finally(() => { delete details.dataset.loading; });
      });
    }
    for (const button of detailEl.querySelectorAll('[data-doc-open-all]')) {
      if (button.dataset.bound === '1') continue;
      button.dataset.bound = '1';
      button.addEventListener('click', () => {
        const open = button.dataset.docOpenAll === 'open';
        for (const details of sections) details.open = open;
      });
    }
    for (const link of detailEl.querySelectorAll('.doc-toc a')) {
      if (link.dataset.bound === '1') continue;
      link.dataset.bound = '1';
      link.addEventListener('click', () => {
        const id = String(link.getAttribute('href') || '').replace(/^#/, '');
        const target = id ? document.getElementById(id) : null;
        let node = target ? target.parentElement : null;
        while (node && node !== detailEl) {
          if (node.matches && node.matches('details.doc-section')) node.open = true;
          node = node.parentElement;
        }
      });
    }
  }

  let commentPopover = null;
  function closeCommentPopover() {
    if (commentPopover) commentPopover.hidden = true;
  }
  function ensureCommentPopover() {
    if (commentPopover) return commentPopover;
    commentPopover = document.createElement('div');
    commentPopover.className = 'doc-comment-popover';
    commentPopover.id = 'doc-comment-popover';
    commentPopover.hidden = true;
    commentPopover.addEventListener('click', (event) => event.stopPropagation());
    document.body.appendChild(commentPopover);
    return commentPopover;
  }
  function openCommentPopover(chip) {
    const ids = String(chip.dataset.comments || '').split(',').filter(Boolean);
    const store = detailEl.querySelector('#doc-comment-store');
    if (!ids.length || !store) return;
    const cards = [];
    let unresolved = 0;
    for (const id of ids) {
      const card = [...store.querySelectorAll('.doc-comment')].find((item) =>
        id.includes('#') ? item.dataset.commentKey === id : item.dataset.comment === id
      );
      if (!card) continue;
      cards.push(card.cloneNode(true));
      if (card.getAttribute('data-resolved') !== 'true') unresolved += 1;
    }
    if (!cards.length) return;
    const popover = ensureCommentPopover();
    const head = document.createElement('div');
    head.className = 'doc-comment-popover-head';
    const title = document.createElement('strong');
    title.textContent = '设计评论';
    const summary = document.createElement('span');
    summary.textContent = unresolved ? (unresolved + ' 条未解决 / 共 ' + cards.length + ' 条') : ('已全部解决 / 共 ' + cards.length + ' 条');
    const close = document.createElement('button');
    close.type = 'button';
    close.className = 'link';
    close.textContent = '关闭';
    close.addEventListener('click', closeCommentPopover);
    head.append(title, summary, close);
    popover.replaceChildren(head);
    for (const card of cards) popover.appendChild(card);
    popover.hidden = false;
    const rect = chip.getBoundingClientRect();
    const width = popover.offsetWidth;
    const height = popover.offsetHeight;
    let left = Math.min(Math.max(8, rect.right - width), window.innerWidth - width - 8);
    let top = rect.bottom + 6;
    if (top + height > window.innerHeight - 8) {
      top = Math.max(8, rect.top - height - 6);
    }
    popover.style.left = left + 'px';
    popover.style.top = top + 'px';
  }
  function setDocFold(button, open) {
    const row = button.closest('.doc-heading-row');
    const body = row ? row.nextElementSibling : null;
    if (!body || !body.classList.contains('doc-section-body')) return;
    body.hidden = !open;
    button.setAttribute('aria-expanded', String(open));
    button.textContent = open ? '收起' : '展开';
    button.title = open ? '折叠此节' : '展开此节';
  }
  function setAllDocFolds(open) {
    detailEl.querySelectorAll('[data-doc-fold]').forEach((button) => setDocFold(button, open));
  }
  function revealDocHeading(heading) {
    let node = heading.parentElement;
    while (node && node !== detailEl) {
      if (node.classList.contains('doc-section-body') && node.hidden) {
        const row = node.previousElementSibling;
        const button = row ? row.querySelector('[data-doc-fold]') : null;
        if (button) setDocFold(button, true);
        else node.hidden = false;
      }
      node = node.parentElement;
    }
  }
  detailEl.addEventListener('click', (event) => {
    const all = event.target.closest('[data-doc-fold-all]');
    if (all) {
      event.preventDefault();
      setAllDocFolds(all.dataset.docFoldAll === 'open');
      return;
    }
    const fold = event.target.closest('[data-doc-fold]');
    if (fold) {
      event.preventDefault();
      setDocFold(fold, fold.getAttribute('aria-expanded') !== 'true');
      return;
    }
    const link = event.target.closest('.doc-toc a');
    if (link) {
      const id = String(link.getAttribute('href') || '').replace(/^#/, '');
      const heading = id ? document.getElementById(id) : null;
      if (heading) revealDocHeading(heading);
    }
  });
  detailEl.addEventListener('click', (event) => {
    const chip = event.target.closest('.doc-comment-chip');
    if (!chip) return;
    event.preventDefault();
    event.stopPropagation();
    openCommentPopover(chip);
  });
  document.addEventListener('click', (event) => {
    if (!commentPopover || commentPopover.hidden) return;
    if (event.target.closest('.doc-comment-chip') || event.target.closest('#doc-comment-popover')) return;
    closeCommentPopover();
  });
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') closeCommentPopover();
  });
  window.addEventListener('resize', closeCommentPopover);

  function loadSource() {
    const panel = detailEl.querySelector('[data-ds-panel="source"]');
    if (!panel || panel.dataset.loaded === '1' || panel.dataset.loading === '1') return;
    const url = panel.dataset.sourceUrl;
    if (sourceCache.has(url)) {
      panel.innerHTML = sourceCache.get(url);
      panel.dataset.loaded = '1';
      return;
    }
    panel.dataset.loading = '1';
    panel.replaceChildren(el('div', 'ds-loading', '正在读取源码…'));
    fetch(url, { headers: { accept: 'text/html' } })
      .then(async (response) => {
        const html = await response.text();
        if (!response.ok) throw new Error('HTTP ' + response.status);
        sourceCache.set(url, html);
        panel.innerHTML = html;
        panel.dataset.loaded = '1';
      })
      .catch((error) => {
        panel.replaceChildren(el('div', 'ds-source-error', '无法读取源码：' + error.message));
      })
      .finally(() => { delete panel.dataset.loading; });
  }

  searchEl.addEventListener('input', () => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      query = searchEl.value.trim().toLowerCase();
      renderTree();
    }, 120);
  });
  expandBtn.addEventListener('click', () => {
    if (!data) return;
    for (const item of data.elements) expanded.add('ar:' + item.qname);
    expanded.add('arche:project');
    expanded.add('arche:technical');
    expanded.add('arche:requirement');
    expanded.add('arche:concept');
    save();
    renderTree();
  });
  collapseBtn.addEventListener('click', () => {
    expanded.clear();
    save();
    renderTree();
  });

  fetch(treeUrl, { headers: { accept: 'application/json' } })
    .then(async (response) => {
      if (!response.ok) throw new Error('HTTP ' + response.status);
      return response.json();
    })
    .then((payload) => {
      if (payload.error) throw new Error(payload.error);
      data = payload;
      renderTree();
    })
    .catch((error) => {
      treeEl.replaceChildren(el('div', 'ds-source-error', '无法加载设计空间：' + error.message));
    });
})();
</script>"##;

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
<p class="sub">这里只显示当前项目的智能体；智能体由平台定义，用户不能新建或修改。</p>
{error_html}
<p class="hint">项目文档撰写任务（<code>goal_clarification / clarifier / writer</code>）由 Task Dispatcher 分配智能体。</p>
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
    fingerprint: &str,
) -> String {
    let empty_hidden = if log.trim().is_empty() { "" } else { " hidden" };
    let content = format!(
        r#"<p class="empty"{empty_hidden} id="agent-log-empty">还没有日志。智能体启动后，进程的 stdout/stderr 会写入这里。</p>
<pre class="agent-log" id="agent-log">{log}</pre>"#,
        log = escape(log)
    );
    let script = r#"<script>
(() => {
  const id = "__ID__";
  const pre = document.getElementById('agent-log');
  const empty = document.getElementById('agent-log-empty');
  let known = "__FINGERPRINT__";
  let busy = false;
  let refreshing = true;
  const toggle = document.getElementById('agent-log-toggle');
  const refresh = document.getElementById('agent-log-refresh');
  const setRefreshing = (value) => {
    refreshing = value;
    if (toggle) toggle.textContent = value ? '停止自动刷新' : '开始自动刷新';
  };
  const atBottom = () => pre.scrollHeight - pre.scrollTop - pre.clientHeight < 40;
  const update = async () => {
    const state = await fetch('/agents/' + id + '/logs/state', {
      cache: 'no-store',
      credentials: 'same-origin'
    });
    if (!state.ok) return;
    const next = (await state.text()).trim();
    if (next === known) return;
    const response = await fetch('/agents/' + id + '/logs/content', {
      cache: 'no-store',
      credentials: 'same-origin'
    });
    if (!response.ok) return;
    known = next;
    const text = await response.text();
    const bottom = atBottom();
    pre.textContent = text;
    if (empty) empty.hidden = text.trim().length > 0;
    if (bottom) pre.scrollTop = pre.scrollHeight;
  };
  const tick = async () => {
    if (!refreshing || busy || document.hidden) return;
    busy = true;
    try { await update(); } catch (_) {}
    busy = false;
  };
  if (refresh) refresh.addEventListener('click', async () => {
    if (busy) return;
    busy = true;
    try { await update(); } catch (_) {}
    busy = false;
  });
  if (toggle) toggle.addEventListener('click', () => {
    setRefreshing(!refreshing);
    if (refreshing) tick();
  });
  setInterval(tick, 2000);
  document.addEventListener('visibilitychange', () => { if (!document.hidden) tick(); });
  pre.scrollTop = pre.scrollHeight;
})();
</script>"#
        .replace("__ID__", &definition.id.to_string())
        .replace("__FINGERPRINT__", fingerprint);
    let body = format!(
        r#"<p class="hint"><a href="/agents">← 返回智能体</a> · <button type="button" class="link" id="agent-log-refresh">刷新</button> · <button type="button" class="link" id="agent-log-toggle">停止自动刷新</button></p>
<h1>{name} 日志</h1>
<p class="sub"><code>@{account}</code> · <code>{role}</code> · 最近 256 KB</p>
{content}{script}"#,
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
        TaskState::Failed => "失败",
        TaskState::Completed => "已完成",
    }
}

fn task_dispatch_label(state: TaskDispatchState) -> &'static str {
    match state {
        TaskDispatchState::Blocked => "阻塞",
        TaskDispatchState::Ready => "就绪",
        TaskDispatchState::Running => "运行中",
        TaskDispatchState::Failed => "失败",
        TaskDispatchState::Closed => "已关闭",
    }
}

fn role_kind_label(kind: RoleKind) -> &'static str {
    match kind {
        RoleKind::Human => "人类",
        RoleKind::Agent => "智能体",
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
        Pooled => "在池中",
        InProgress => "处理中",
        Resolved => "待确认",
        Closed => "已关闭",
        Rejected => "不处理",
        Duplicate => "重复",
    }
}

fn issue_classification_label(classification: IssueClassification) -> &'static str {
    use IssueClassification::*;
    match classification {
        CodeDefect => "代码缺陷",
        DesignDefect => "设计缺陷",
        RequirementDefect => "需求缺陷",
        TestDefect => "测试缺陷",
        NotAProblem => "不是问题",
        Duplicate => "重复",
        NeedsInfo => "需要补充信息",
        Enhancement => "改进",
        Incident => "事故",
    }
}

fn issue_confidence_label(confidence: IssueConfidence) -> &'static str {
    use IssueConfidence::*;
    match confidence {
        High => "高",
        Medium => "中",
        Low => "低",
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

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;
    use uuid::Uuid;

    #[test]
    fn escapes_user_content() {
        assert_eq!(escape("<script>"), "&lt;script&gt;");
    }

    #[test]
    fn markdown_renders_basic_blocks_and_escapes_html() {
        let html = markdown_to_html(
            "## 标题\n\n**粗体** 和 `code`\n\n- 一\n- 二\n\n```js\nalert(1)\n```\n\n[链接](https://example.com)\n<script>",
        );
        assert!(html.contains(r#"<h2 id="md-h-1">标题</h2>"#));
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
    fn markdown_rejects_unsafe_links() {
        let html = markdown_to_html("[x](javascript:alert(1))");
        assert!(!html.contains(r#"href="javascript"#));
    }

    #[test]
    fn markdown_renders_anchor_sections_and_item_anchors() {
        let html = markdown_to_html(
            "<!--anchor:k:root-->\n# 标题\n\n正文\n\n- <!--anchor:k:item--> 列表项\n",
        );
        assert!(html.contains(r#"<section class="anchor-block" data-anchor="k:root">"#));
        assert!(html.contains(r#"<li data-anchor="k:item">列表项</li>"#));
        assert!(html.ends_with("</section>"));
    }

    #[test]
    fn projects_page_requires_a_git_repository_field() {
        let owner = Account::new(
            "owner".to_string(),
            "Owner".to_string(),
            AccountKind::Human,
            AccountRole::Member,
            None,
            Some("hash".to_string()),
        )
        .unwrap();
        let ctx = ConsoleContext::new(&owner, &[], None);
        let html = projects_page(&ctx, &[], &[], None);
        assert!(html.contains(r#"name="git_repository""#));
        assert!(html.contains(r#"name="git_branch""#));
        assert!(html.contains("Git 仓库"));
    }

    #[test]
    fn project_page_shows_repository_and_design_space() {
        let owner = Account::new(
            "owner".to_string(),
            "Owner".to_string(),
            AccountKind::Human,
            AccountRole::Member,
            None,
            Some("hash".to_string()),
        )
        .unwrap();
        let mut project =
            Project::new("Project", None, "git@example.com:test/repo.git", owner.id).unwrap();
        project.design_space_url = "ds://u@h/0b16bc99e483d732".to_string();
        let ctx = ConsoleContext::new(&owner, std::slice::from_ref(&project), Some(&project));
        let html = project_page(&ctx, &project, &[], &[], false, None);
        assert!(html.contains("git@example.com:test/repo.git"));
        assert!(html.contains("ds://u@h/0b16bc99e483d732"));
        assert!(html.contains(&project.id.to_string()[..8]));
        // Older stored addresses may still be percent-encoded.
        assert_eq!(
            display_percent_encoded("ds://u@h/%E8%BD%AF%E4%BB%B6"),
            "ds://u@h/软件"
        );
    }

    #[test]
    fn design_space_page_renders_tree_shell_and_escapes_content() {
        let owner = Account::new(
            "owner".to_string(),
            "Owner".to_string(),
            AccountKind::Human,
            AccountRole::Member,
            None,
            Some("hash".to_string()),
        )
        .unwrap();
        let mut project =
            Project::new("Project", None, "git@example.com:test/repo.git", owner.id).unwrap();
        project.design_space_url = "ds://u@h/0b16bc99e483d732".to_string();
        let ctx = ConsoleContext::new(&owner, std::slice::from_ref(&project), Some(&project));
        let overview = DesignSpaceOverview {
            branch: "main".to_string(),
            version: Some("ab".repeat(32)),
            source_commit: "cd".repeat(20),
            file_count: 4,
            element_count: 12,
            document_count: 5,
            designed_count: 3,
            elements: vec![DesignSpaceElement {
                hash: "ef".repeat(32),
                qname: "files/src/main.rs/fn".to_string(),
                name: "Fn <script>".to_string(),
                path: "src/<main>.rs".to_string(),
                lang: "rust".to_string(),
                kind: "function".to_string(),
                start_line: 3,
                end_line: 9,
                has_design: true,
                is_file: false,
                is_structure: false,
                members: Vec::new(),
            }],
        };
        let html = design_space_page(&ctx, &project, &overview, &["source".to_string()], "source");
        assert!(html.contains("id=\"ds-shell\""));
        assert!(html.contains(&format!(
            "data-tree-url=\"/projects/{}/design-space/tree\"",
            project.id
        )));
        assert!(html.contains("25%"));

        let detail = design_space_element(
            &DesignSpaceDetail {
                hash: "ef".repeat(32),
                name: "Fn <script>".to_string(),
                lang: "rust".to_string(),
                kind: "function".to_string(),
                path: "src/<main>.rs".to_string(),
                start_line: 3,
                end_line: 9,
                design: Some("<script>alert(1)</script>".to_string()),
                comments: Vec::new(),
                members: Vec::new(),
                sections: Vec::new(),
            },
            "/projects/x/design-space/elements/y/source",
        );
        assert!(detail.contains("&lt;script&gt;alert(1)"));
        assert!(!detail.contains("<script>alert(1)"));

        let source = design_space_source(&DesignSpaceSource {
            commit: "cd".repeat(20),
            path: "src/<main>.rs".to_string(),
            start_line: 3,
            end_line: 4,
            first_line: 3,
            last_line: 4,
            total_lines: 4,
            lines: vec![
                DesignSpaceLine {
                    number: 3,
                    text: "    let x = 1 < 2;".to_string(),
                    in_element: true,
                },
                DesignSpaceLine {
                    number: 4,
                    text: "</script>".to_string(),
                    in_element: false,
                },
            ],
        });
        assert!(source.contains("&lt; 2"));
        assert!(!source.contains("</script>"));
        assert!(source.contains("ds-line in"));
    }

    #[test]
    fn concept_detail_has_no_source_tab() {
        let html = design_space_element(
            &DesignSpaceDetail {
                hash: "ef".repeat(32),
                name: "Auth::Login".to_string(),
                lang: String::new(),
                kind: "concept".to_string(),
                path: String::new(),
                start_line: 0,
                end_line: 0,
                design: Some("登录流程".to_string()),
                comments: Vec::new(),
                members: Vec::new(),
                sections: Vec::new(),
            },
            "/projects/x/design-space/elements/y/source",
        );
        assert!(html.contains(">概念<"));
        assert!(html.contains("无源码锚点"));
        assert!(!html.contains(r#"data-ds-tab="source""#));
        assert!(!html.contains("data-source-url"));
    }

    #[test]
    fn document_detail_renders_content_without_a_source_tab() {
        let html = design_space_element(
            &DesignSpaceDetail {
                hash: "ef".repeat(32),
                name: "登录".to_string(),
                lang: String::new(),
                kind: "document-section".to_string(),
                path: "documents/design/technical/section1".to_string(),
                start_line: 0,
                end_line: 0,
                design: Some("## 登录\n\n登录使用 OAuth。".to_string()),
                comments: Vec::new(),
                members: Vec::new(),
                sections: Vec::new(),
            },
            "/projects/x/design-space/elements/y/source",
        );
        assert!(html.contains("SEC"));
        assert!(html.contains("章节"));
        assert!(html.contains("文档"));
        assert!(html.contains("登录使用 OAuth。"));
        assert!(html.contains("documents/design/technical/section1"));
        assert!(!html.contains(r#"data-ds-tab="source""#));
        assert!(!html.contains("data-source-url"));
    }

    #[test]
    fn toc_shortens_qualified_names_but_keeps_prose_titles() {
        assert_eq!(
            toc_label("namespaces/taskboard_utils/format_points"),
            "format_points"
        );
        assert_eq!(toc_label("src/domain/task.rs"), "task.rs");
        assert_eq!(toc_label("namespaces/app/Auth::login"), "login");
        assert_eq!(toc_label("用户 / 客户 / 账户"), "用户 / 客户 / 账户");
        assert_eq!(toc_label("登录流程"), "登录流程");
    }

    #[test]
    fn document_detail_loads_sections_on_demand() {
        let html = design_space_element(
            &DesignSpaceDetail {
                hash: "ef".repeat(32),
                name: "技术设计".to_string(),
                lang: String::new(),
                kind: "document".to_string(),
                path: "documents/design/technical".to_string(),
                start_line: 0,
                end_line: 0,
                design: None,
                comments: Vec::new(),
                members: Vec::new(),
                sections: vec![DesignSpaceSection {
                    qname: "documents/design/technical/section1".to_string(),
                    title: "登录".to_string(),
                    children: vec![DesignSpaceSection {
                        qname: "documents/design/technical/section1/section1".to_string(),
                        title: "失败".to_string(),
                        children: Vec::new(),
                    }],
                }],
            },
            "/projects/x/design-space/elements/y/source",
        );
        assert!(html.contains(r#"data-section-qname="documents/design/technical/section1""#));
        assert!(html.contains("展开后加载…"));
        assert!(html.contains(r#"id="doc-section-0""#));
        assert!(html.contains(r##"href="#doc-section-1""##));
        assert!(html.contains("已编写"));
        assert!(!html.contains("登录使用 OAuth。"));
    }

    #[test]
    fn document_detail_has_toc_and_renders_topic_as_bold_text() {
        let html = design_space_element(
            &DesignSpaceDetail {
                hash: "ef".repeat(32),
                name: "技术设计".to_string(),
                lang: String::new(),
                kind: "document".to_string(),
                path: "documents/design/technical".to_string(),
                start_line: 0,
                end_line: 0,
                design: Some("# 概览\n\n正文。\n\n## 类\n\n**职责**\n\n负责示例。".to_string()),
                comments: vec![DesignSpaceComment {
                    id: 1,
                    resolved: false,
                    target: "documents/design/technical/section1".to_string(),
                    text: "需要补充失败语义".to_string(),
                }],
                members: Vec::new(),
                sections: Vec::new(),
            },
            "/projects/x/design-space/elements/y/source",
        );
        assert!(html.contains(r#"class="doc-view""#));
        assert!(html.contains(r#"id="doc-toc""#));
        assert!(html.contains("需要补充失败语义"));
        assert!(html.contains("未解决"));
        assert!(html.contains(r##"href="#md-h-1""##));
        assert!(html.contains(r##"href="#md-h-2""##));
        assert!(html.contains(r##"id="md-h-1""##));
        assert!(html.contains(r#"class="doc-level doc-section-body" style="margin-left:18px""#));
        assert!(html.contains("data-doc-fold"));
        assert!(html.contains(r#"data-doc-fold-all="open""#));
        assert!(html.contains("<strong>职责</strong>"));
        assert!(!html.contains("TOPIC"));
        assert!(!html.contains("文档结构"));
    }

    #[test]
    fn module_detail_lists_members() {
        let html = design_space_element(
            &DesignSpaceDetail {
                hash: "ef".repeat(32),
                name: "核心".to_string(),
                lang: String::new(),
                kind: "module".to_string(),
                path: "design/app/core".to_string(),
                start_line: 0,
                end_line: 0,
                design: None,
                comments: Vec::new(),
                members: vec![
                    DesignSpaceMember {
                        hash: "aa".repeat(32),
                        qname: "namespaces/app/Auth".to_string(),
                        name: "Auth".to_string(),
                        kind: "class".to_string(),
                    },
                    DesignSpaceMember {
                        hash: "bb".repeat(32),
                        qname: "design/app/core/parser".to_string(),
                        name: "解析".to_string(),
                        kind: "module".to_string(),
                    },
                ],
                sections: Vec::new(),
            },
            "/projects/x/design-space/elements/y/source",
        );
        assert!(html.contains("成员"));
        assert!(html.contains("namespaces/app/Auth"));
        assert!(html.contains("design/app/core/parser"));
        assert!(html.contains("还没有关联设计文档"));
    }

    #[test]
    fn document_comments_render_as_collapsed_chips() {
        let html = design_space_element(
            &DesignSpaceDetail {
                hash: "ef".repeat(32),
                name: "技术设计".to_string(),
                lang: String::new(),
                kind: "document".to_string(),
                path: "documents/design/technical".to_string(),
                start_line: 0,
                end_line: 0,
                design: Some(
                    "# 概览{{comments:documents/design/technical/section1#1}}\n\n正文。{{comments:documents/design/technical/section1#2,documents/design/technical/section2/body1#3}}\n\n## 小节{{comments:documents/design/technical/section3#4}}"
                        .to_string(),
                ),
                comments: vec![
                    DesignSpaceComment {
                        id: 1,
                        resolved: false,
                        target: "documents/design/technical/section1".to_string(),
                        text: "需要补充失败语义".to_string(),
                    },
                    DesignSpaceComment {
                        id: 2,
                        resolved: true,
                        target: "documents/design/technical/section2/body1".to_string(),
                        text: "已处理的评论".to_string(),
                    },
                    DesignSpaceComment {
                        id: 3,
                        resolved: false,
                        target: "documents/design/technical/section2/body1".to_string(),
                        text: "第二条未解决评论".to_string(),
                    },
                    DesignSpaceComment {
                        id: 4,
                        resolved: false,
                        target: "documents/design/technical/section3".to_string(),
                        text: "小节评论".to_string(),
                    },
                ],
                members: Vec::new(),
                sections: Vec::new(),
            },
            "/projects/x/design-space/elements/y/source",
        );
        assert!(html.contains(
            r#"class="doc-comment-chip" data-comments="documents/design/technical/section1#1""#
        ));
        assert!(html.contains(
            r#"class="doc-comment-chip" data-comments="documents/design/technical/section1#2,documents/design/technical/section2/body1#3""#
        ));
        assert!(html.contains(
            r#"class="doc-comment-chip" data-comments="documents/design/technical/section3#4""#
        ));
        assert!(html.contains(r#"id="doc-comment-store" hidden"#));
        assert!(html.contains(r#"data-comment="1""#));
        assert!(html.contains(r#"data-comment-key="documents/design/technical/section1#1""#));
        assert!(html.contains("需要补充失败语义"));
        assert!(html.contains("第二条未解决评论"));
        assert!(!html.contains("{{comments:"));
        assert!(!html.contains("doc-comments"));
        assert!(html.contains(r##"href="#md-h-1">概览</a>"##));
        assert!(html.contains(r##"href="#md-h-2">小节</a>"##));
    }
    #[test]
    fn element_titles_use_short_names() {
        let html = design_space_element(
            &DesignSpaceDetail {
                hash: "ef".repeat(32),
                name: "crate::util::Helper".to_string(),
                lang: "rust".to_string(),
                kind: "function".to_string(),
                path: "src/util.rs".to_string(),
                start_line: 3,
                end_line: 9,
                design: None,
                comments: Vec::new(),
                members: Vec::new(),
                sections: Vec::new(),
            },
            "/projects/x/design-space/elements/y/source",
        );
        assert!(html.contains(r#"<h2 title="crate::util::Helper">Helper</h2>"#));
        assert!(html.contains("全名 <code>crate::util::Helper</code>"));
        assert!(!html.contains(">crate::util::Helper</h2>"));
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
        let project =
            Project::new("Project", None, "git@example.com:test/repo.git", owner.id).unwrap();
        let ctx = ConsoleContext::new(&owner, std::slice::from_ref(&project), Some(&project));

        let manager_html = project_page(&ctx, &project, &[], &[], true, None);
        assert!(manager_html.contains(&format!("/projects/{}/delete", project.id)));
        assert!(manager_html.contains("删除项目"));
        assert!(manager_html.contains(r#"class="danger solid""#));
        assert!(manager_html.contains(r#"name="skip_human_review""#));
        assert!(manager_html.contains(&format!("/projects/{}/edit", project.id)));

        let member_html = project_page(&ctx, &project, &[], &[], false, None);
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
            project_id: uuid::Uuid::now_v7(),
            model_base_url: "http://127.0.0.1:11434/v1".to_string(),
            model_name: "test-model".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            system_prompt: String::new(),
            tools: Vec::new(),
            shell: false,
            skills: Vec::new(),
            enabled: true,
            created_at: time::OffsetDateTime::now_utc(),
            updated_at: time::OffsetDateTime::now_utc(),
        };
        let log = "[info] tool <script>\n<b>bad</b>\n";

        let html = agent_logs_page(&ctx, &definition, log, "1:1");

        assert!(html.contains("Clarifier &lt;script&gt;"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("&lt;b&gt;bad&lt;/b&gt;"));
        assert!(html.contains("id=\"agent-log-refresh\""));
        assert!(html.contains("id=\"agent-log-toggle\""));
        assert!(html.contains("停止自动刷新"));
        assert!(html.contains("开始自动刷新"));
        // The log payload is escaped; only the refresh script is real.
        assert_eq!(html.matches("<script>").count(), 1);
    }

    #[test]
    fn merge_request_diff_marks_added_and_removed_lines() {
        let html = merge_request_diff_html(
            Some("# 标题\n\n旧正文。\n\n公共结尾。"),
            Some("# 标题\n\n新正文。\n\n公共结尾。"),
        );
        assert!(html.contains("diff-del"));
        assert!(html.contains("diff-add"));
        assert!(html.contains("- 旧正文。"));
        assert!(html.contains("+ 新正文。"));
        assert!(html.contains("公共结尾。"));
    }

    #[test]
    fn merge_request_page_uses_task_operations_and_escapes_content() {
        use foundry_domain::{
            AllowedOperation, DesignSpaceMergeRequest, MergeRequestDetail, OperationName, StageKey,
            Task, TaskDispatchState, WorkflowId,
        };

        let account = Account::new(
            "reviewer".to_string(),
            "Reviewer".to_string(),
            AccountKind::Human,
            AccountRole::Member,
            None,
            Some("hash".to_string()),
        )
        .unwrap();
        let project =
            Project::new("Project", None, "git@example.com:test/repo.git", account.id).unwrap();
        let ctx = ConsoleContext::new(&account, std::slice::from_ref(&project), Some(&project));
        let now = OffsetDateTime::now_utc();
        let task = Task {
            id: Uuid::now_v7(),
            project_id: project.id,
            parent_id: None,
            workflow: WorkflowId::new("design_space_merge"),
            workflow_version: 1,
            title: "Review <script>".to_string(),
            description: "Review task content".to_string(),
            state: TaskState::InProgress,
            dispatch_state: TaskDispatchState::Running,
            stage: Some(StageKey::new("merge")),
            result: None,
            input: Default::default(),
            created_at: now,
            updated_at: now,
            completed_at: None,
            dependencies: Vec::new(),
            role_specs: Vec::new(),
            actors: Vec::new(),
            allowed_operations: vec![AllowedOperation {
                name: OperationName::new("merge:merge"),
                display_name: "合并".to_string(),
                description: String::new(),
            }],
            comments: Vec::new(),
            operations: Vec::new(),
        };
        let request = DesignSpaceMergeRequest {
            id: Uuid::now_v7(),
            project_id: project.id,
            title: "Land <script>".to_string(),
            description: String::new(),
            source_branch: "feature".to_string(),
            target_branch: "main".to_string(),
            source_commit: "aa".to_string(),
            target_commit: "bb".to_string(),
            task_id: task.id,
            merge_commit: None,
            approved_content_digest: None,
            approved_source_commit: None,
            approved_by: None,
            approved_at: None,
            created_by: account.id,
            created_at: now,
            updated_at: now,
        };
        let html = merge_request_page(
            &ctx,
            &project,
            &MergeRequestReview {
                merge_request: request.clone(),
                task: task.clone(),
                status: MergeRequestStatus::Approved,
            },
        );
        assert!(html.contains("Land &lt;script&gt;"));
        assert!(!html.contains("Land <script>"));
        assert!(html.contains(&format!(
            "/projects/{}/merge-requests/{}/operations",
            project.id, request.id
        )));
        assert!(html.contains(r#"value="merge:merge""#));
        assert!(html.contains("审核任务"));
        assert!(html.contains("data-changes-url"));
        assert!(html.contains("展开变更"));
        assert!(!html.contains("技术设计"));
        assert!(!html.contains("+ 新正文"));

        let changes = merge_request_changes(&MergeRequestDetail {
            merge_request: request,
            task,
            status: MergeRequestStatus::Approved,
            changes: vec![foundry_domain::MergeRequestChange {
                status: MergeRequestChangeStatus::Modified,
                qname: "documents/design/technical/section1/body1".to_string(),
                kind: "document-text".to_string(),
                fields: vec![foundry_domain::MergeRequestFieldChange {
                    field: "text".to_string(),
                    before: Some("旧正文".to_string()),
                    after: Some("新正文".to_string()),
                }],
            }],
            documents: vec![foundry_domain::MergeRequestDocumentChange {
                status: MergeRequestChangeStatus::Modified,
                qname: "documents/design/technical".to_string(),
                title: "技术设计".to_string(),
                before: Some("# 技术设计\n\n旧正文".to_string()),
                after: Some("# 技术设计\n\n新正文".to_string()),
                objects: Vec::new(),
            }],
        });
        assert!(changes.contains("mr-doc"));
        assert!(changes.contains("diff-add"));
        assert!(changes.contains("diff-del"));
        assert!(changes.contains("+ 新正文"));
    }
}
