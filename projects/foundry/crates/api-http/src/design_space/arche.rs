//! Read-only access to an Arche v2 design repository for the console.
//!
//! Arche stores documents as object fields and keeps comments outside the
//! versioned graph. This module maps that model onto the console's existing
//! read types so the v1 and v2 repositories can be served by the same
//! `DesignSpaceAccess` surface while projects migrate.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use arche_core::{
    Commit, CommitHash, Object, ObjectHash, PropertyAccess, PropertyValue, Repository, SymbolHash,
    priority_display,
};
use foundry_domain::{
    MergeRequestChange, MergeRequestChangeStatus, MergeRequestDiff, MergeRequestFieldChange,
};
use foundry_web::{
    DesignSpaceComment, DesignSpaceDetail, DesignSpaceElement, DesignSpaceMember,
    DesignSpaceOverview, DesignSpaceSource,
};
use sha2::{Digest, Sha256};

use super::{DesignSpaceAccess, language_for_path, read_blob, source_lines};

const ZERO_HEX: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn open(path: &Path) -> Result<Repository, String> {
    Repository::open(path).map_err(|error| error.to_string())
}

fn commit_of(repo: &Repository, branch: &str) -> Result<Option<(CommitHash, Commit)>, String> {
    let name = format!("refs/heads/{branch}");
    let Some(hash) = repo.ref_hash(&name).map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    let commit = repo.read_commit(hash).map_err(|error| error.to_string())?;
    Ok(Some((hash, commit)))
}

fn symbol(repo: &Repository, value: SymbolHash) -> String {
    if value.is_zero() {
        String::new()
    } else {
        repo.resolve(value).unwrap_or_default()
    }
}

fn local_part(qname: &str) -> String {
    let tail = qname.rsplit('/').next().unwrap_or(qname);
    tail.split('#').next().unwrap_or(tail).to_owned()
}

fn name_of(repo: &Repository, object: &Object, qname: &str) -> String {
    if let Ok(PropertyValue::String(display_name)) = object.get_property("display_name")
        && !display_name.is_empty()
    {
        return display_name;
    }
    match qname {
        "requirement/user" => return "用户".to_string(),
        "requirement/story" => return "故事".to_string(),
        "requirement/system" => return "系统".to_string(),
        _ => {}
    }
    let name = object.name();
    if name.is_zero() {
        local_part(qname)
    } else {
        symbol(repo, name)
    }
}

fn source_info(repo: &Repository, object: &Object) -> (String, u32, u32) {
    match object {
        Object::TechnicalDirectory(value) => (symbol(repo, value.source().path), 0, 0),
        Object::TechnicalFile(value) => (symbol(repo, value.source().path), 0, 0),
        Object::TechnicalClass(value) => span(repo, value.source()),
        Object::TechnicalStruct(value) => span(repo, value.source()),
        Object::TechnicalEnum(value) => span(repo, value.source()),
        Object::TechnicalInterface(value) => span(repo, value.source()),
        Object::TechnicalMixin(value) => span(repo, value.source()),
        Object::TechnicalMethod(value) => span(repo, value.source()),
        Object::TechnicalFunction(value) => span(repo, value.source()),
        Object::TechnicalPart(value) => span(repo, value.source()),
        Object::TechnicalVariable(value) => span(repo, value.source()),
        Object::TechnicalConstant(value) => span(repo, value.source()),
        Object::TechnicalMember(value) => span(repo, value.source()),
        Object::TechnicalEnumValue(value) => span(repo, value.source()),
        Object::TechnicalArgument(value) => span(repo, value.source()),
        Object::TechnicalReturnValue(value) => span(repo, value.source()),
        Object::TechnicalProperty(value) => {
            let getter = value.getter();
            if getter.path.is_zero() {
                span(repo, value.setter())
            } else {
                span(repo, getter)
            }
        }
        _ => (String::new(), 0, 0),
    }
}

fn span(repo: &Repository, value: arche_core::SourceSpan) -> (String, u32, u32) {
    (symbol(repo, value.path), value.start, value.end)
}

fn is_root(object: &Object) -> bool {
    matches!(
        object,
        Object::Root(_)
            | Object::TechnicalRoot(_)
            | Object::RequirementRoot(_)
            | Object::ProjectRoot(_)
    )
}

fn is_element(object: &Object) -> bool {
    if is_root(object)
        || matches!(
            object,
            Object::ObjectList(_)
                | Object::DocumentText(_)
                | Object::DocumentList(_)
                | Object::DocumentParagraphList(_)
                | Object::DocumentTopic(_)
                | Object::DocumentCode(_)
                | Object::DocumentMermaid(_)
                | Object::DocumentHtml(_)
        )
    {
        return false;
    }
    !(matches!(object, Object::ConceptDirectory(_)) && object.parent().is_zero())
}

/// The fixed objects created by `arche init`. Skeleton membership is by qname,
/// not by object kind: user-created roots, lists and concept directories are
/// regular elements. `root` itself is the commit root and stays hidden.
const SKELETON_QNAMES: &[&str] = &[
    "project",
    "technical",
    "technical/directory",
    "technical/file",
    "technical/namespace",
    "technical/system",
    "requirement",
    "requirement/user",
    "requirement/story",
    "requirement/system",
    "concept",
    "concept/concepts",
    "concept/directories",
    "concept/domain",
    "concept/domain/concepts",
    "concept/domain/directories",
    "concept/requirement",
    "concept/requirement/concepts",
    "concept/requirement/directories",
    "concept/technical",
    "concept/technical/concepts",
    "concept/technical/directories",
];

fn is_skeleton_qname(qname: &str) -> bool {
    SKELETON_QNAMES.contains(&qname)
}

fn is_tree_object(qname: &str, object: &Object) -> bool {
    is_skeleton_qname(qname) || is_element(object)
}

fn kind_label(object: &Object) -> &'static str {
    match object {
        Object::Root(_)
        | Object::TechnicalRoot(_)
        | Object::RequirementRoot(_)
        | Object::ProjectRoot(_) => "design-structure",
        Object::ProjectDocument(_) => "project-document",
        Object::ObjectList(_) => "list",
        Object::ConceptDirectory(_) => "concept-directory",
        Object::TechnicalSystem(_) => "system",
        Object::TechnicalModule(_) => "module",
        Object::TechnicalLayer(_) => "layer",
        Object::TechnicalDirectory(_) => "directory",
        Object::TechnicalFile(_) => "file",
        Object::TechnicalNamespace(_) => "namespace",
        Object::TechnicalClass(_) => "class",
        Object::TechnicalStruct(_) => "struct",
        Object::TechnicalEnum(_) => "enum",
        Object::TechnicalInterface(_) => "interface",
        Object::TechnicalMethod(_) => "method",
        Object::TechnicalFunction(_) => "function",
        Object::TechnicalMixin(_) => "mixin",
        Object::TechnicalPart(_) => "part",
        Object::TechnicalVariable(_) => "variable",
        Object::TechnicalConstant(_) => "constant",
        Object::TechnicalMember(_) => "member",
        Object::TechnicalProperty(_) => "property",
        Object::TechnicalEnumValue(_) => "enum-value",
        Object::TechnicalArgument(_) => "argument",
        Object::TechnicalReturnValue(_) => "return-value",
        Object::Concept(_) => "concept",
        Object::RequirementUser(_) => "requirement-user",
        Object::RequirementStory(_) => "requirement-story",
        Object::RequirementSystem(_) => "requirement-system",
        Object::RequirementModule(_) => "requirement-module",
        Object::RequirementFunction(_) => "requirement-function",
        Object::RequirementInteraction(_) => "requirement-interaction",
        _ => "object",
    }
}

fn is_structure(object: &Object) -> bool {
    matches!(
        object,
        Object::TechnicalSystem(_) | Object::TechnicalModule(_) | Object::TechnicalLayer(_)
    )
}

fn has_design(object: &Object) -> bool {
    arche_core::repo::doc_fields(object)
        .into_iter()
        .any(|(_, value)| !value.is_zero())
}

struct Element {
    hash: ObjectHash,
    qname: String,
    name: String,
    kind: &'static str,
    path: String,
    start_line: u32,
    end_line: u32,
    has_design: bool,
    is_file: bool,
    is_structure: bool,
}

fn collect_elements(repo: &Repository, commit: &Commit) -> Result<Vec<Element>, String> {
    let mut elements = Vec::new();
    for (qname, hash) in &commit.objects {
        let object = repo.read(*hash).map_err(|error| error.to_string())?;
        let qname = symbol(repo, *qname);
        if is_skeleton_qname(&qname) || !is_element(&object) {
            continue;
        }
        let (path, start_line, end_line) = source_info(repo, &object);
        elements.push(Element {
            hash: *hash,
            qname: qname.clone(),
            name: name_of(repo, &object, &qname),
            kind: kind_label(&object),
            path,
            start_line,
            end_line,
            has_design: has_design(&object),
            is_file: matches!(object, Object::TechnicalFile(_)),
            is_structure: is_structure(&object),
        });
    }
    Ok(elements)
}

/// Skeleton nodes are not design elements, but the console still renders them
/// so an initialized repository shows its full shape instead of only the
/// concept space.
fn collect_skeleton(repo: &Repository, commit: &Commit) -> Result<Vec<DesignSpaceElement>, String> {
    let mut elements = Vec::new();
    for (qname, hash) in &commit.objects {
        let object = repo.read(*hash).map_err(|error| error.to_string())?;
        let qname = symbol(repo, *qname);
        if !is_skeleton_qname(&qname) {
            continue;
        }
        elements.push(DesignSpaceElement {
            hash: hash.to_hex(),
            qname: qname.clone(),
            name: name_of(repo, &object, &qname),
            path: String::new(),
            lang: String::new(),
            kind: kind_label(&object).to_owned(),
            start_line: 0,
            end_line: 0,
            has_design: false,
            is_file: false,
            is_structure: false,
            members: Vec::new(),
        });
    }
    Ok(elements)
}

type ChildIndex = BTreeMap<String, Vec<(ObjectHash, String, String, &'static str)>>;

fn children(repo: &Repository, commit: &Commit) -> Result<ChildIndex, String> {
    let mut entries = Vec::new();
    for (qname, hash) in &commit.objects {
        let object = repo.read(*hash).map_err(|error| error.to_string())?;
        let qname = symbol(repo, *qname);
        if !is_tree_object(&qname, &object) {
            continue;
        }
        let name = name_of(repo, &object, &qname);
        let kind = kind_label(&object);
        entries.push((qname, *hash, name, kind, object));
    }

    // Root spaces such as `requirement` own their items through ObjectLists
    // (`requirement/user`, `requirement/story`, `requirement/system`). The
    // item's `parent` field points at the root, so using parent alone would
    // render every user/story/system as a sibling of those list nodes. Use the
    // list membership for items held by a visible skeleton list instead.
    let by_qname = entries
        .iter()
        .map(|(qname, _, _, _, _)| qname.clone())
        .collect::<BTreeSet<_>>();
    let by_hash = entries
        .iter()
        .map(|(qname, hash, _, _, _)| (*hash, qname.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut list_parent: BTreeMap<String, String> = BTreeMap::new();
    for (qname, _, _, _, object) in &entries {
        if !is_skeleton_qname(qname) || !qname.starts_with("requirement/") {
            continue;
        }
        let Object::ObjectList(list) = object else {
            continue;
        };
        for item in &list.items() {
            if let Some(item_qname) = by_hash.get(item)
                && by_qname.contains(item_qname)
            {
                list_parent.insert(item_qname.clone(), qname.clone());
            }
        }
    }

    let mut index: ChildIndex = BTreeMap::new();
    for (qname, hash, name, kind, object) in entries {
        let parent = list_parent
            .get(&qname)
            .cloned()
            .unwrap_or_else(|| symbol(repo, object.parent()));
        if parent.is_empty() {
            continue;
        }
        index
            .entry(parent)
            .or_default()
            .push((hash, qname, name, kind));
    }
    for values in index.values_mut() {
        values.sort_by(|left, right| left.1.cmp(&right.1));
    }
    Ok(index)
}

fn element<'a>(elements: &'a [Element], hash: &str) -> Option<&'a Element> {
    elements
        .iter()
        .find(|element| element.hash.to_hex() == hash)
}

pub(super) fn overview(path: &Path, branch: &str) -> Result<DesignSpaceOverview, String> {
    let repo = open(path)?;
    let mut overview = DesignSpaceOverview {
        branch: branch.to_owned(),
        version: None,
        source_commit: String::new(),
        file_count: 0,
        element_count: 0,
        document_count: 0,
        designed_count: 0,
        elements: Vec::new(),
    };
    let Some((hash, commit)) = commit_of(&repo, branch)? else {
        return Ok(overview);
    };
    overview.version = Some(hash.to_hex());
    overview.source_commit = source_commit(&repo, &commit);
    let elements = collect_elements(&repo, &commit)?;
    for element in &elements {
        if element.is_file {
            overview.file_count += 1;
        } else if element.is_structure {
            // Structures are counted by the tree, not as design elements.
        } else {
            overview.element_count += 1;
            if element.has_design {
                overview.designed_count += 1;
            }
        }
        overview.elements.push(DesignSpaceElement {
            hash: element.hash.to_hex(),
            qname: element.qname.clone(),
            name: element.name.clone(),
            path: element.path.clone(),
            lang: language_for_path(&element.path),
            kind: element.kind.to_owned(),
            start_line: element.start_line,
            end_line: element.end_line,
            has_design: element.has_design,
            is_file: element.is_file,
            is_structure: element.is_structure,
            members: Vec::new(),
        });
    }
    overview.elements.extend(collect_skeleton(&repo, &commit)?);
    let index = children(&repo, &commit)?;
    for element in &mut overview.elements {
        element.members = index
            .get(&element.qname)
            .map(|values| values.iter().map(|value| value.1.clone()).collect())
            .unwrap_or_default();
    }
    overview.elements.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.start_line.cmp(&right.start_line))
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(overview)
}

fn source_commit(repo: &Repository, commit: &Commit) -> String {
    for (qname, hash) in &commit.objects {
        let qname = symbol(repo, *qname);
        if qname != "root" {
            continue;
        }
        let Ok(object) = repo.read(*hash) else {
            return String::new();
        };
        if let Object::Root(root) = object {
            return root.source_commit().to_hex();
        }
    }
    String::new()
}

pub(super) fn detail(path: &Path, hash: &str, branch: &str) -> Result<DesignSpaceDetail, String> {
    let repo = open(path)?;
    let Some((_, commit)) = commit_of(&repo, branch)? else {
        return Err("设计空间分支还没有提交".to_string());
    };
    let elements = collect_elements(&repo, &commit)?;
    let found = element(&elements, hash).ok_or_else(|| format!("对象不存在: {hash}"))?;
    let object = repo.read(found.hash).map_err(|error| error.to_string())?;
    let design = design_markdown(&repo, &object, &elements)?;
    let comments = repo
        .comment_list_on(branch, Some(&found.qname), false)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|comment| DesignSpaceComment {
            id: comment.id as u32,
            resolved: comment.resolved,
            target: comment.target,
            text: comment.body,
        })
        .collect();
    let index = children(&repo, &commit)?;
    let members = if matches!(object, Object::RequirementSystem(_)) {
        // Modules stay in the tree, not in the system detail pane.
        Vec::new()
    } else {
        index
            .get(&found.qname)
            .map(|values| {
                values
                    .iter()
                    .map(|(hash, qname, name, kind)| DesignSpaceMember {
                        hash: hash.to_hex(),
                        qname: qname.clone(),
                        name: name.clone(),
                        kind: kind.to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok(DesignSpaceDetail {
        hash: found.hash.to_hex(),
        name: found.name.clone(),
        lang: language_for_path(&found.path),
        kind: found.kind.to_owned(),
        path: found.path.clone(),
        start_line: found.start_line,
        end_line: found.end_line,
        design,
        comments,
        members,
        sections: Vec::new(),
    })
}

pub(super) fn section(
    path: &Path,
    hash: &str,
    branch: &str,
    qname: &str,
) -> Result<String, String> {
    let repo = open(path)?;
    let Some((_, commit)) = commit_of(&repo, branch)? else {
        return Err("设计空间分支还没有提交".to_string());
    };
    let elements = collect_elements(&repo, &commit)?;
    let found = element(&elements, hash).ok_or_else(|| format!("对象不存在: {hash}"))?;
    if !qname.is_empty() && !qname.starts_with(&format!("{}/", found.qname)) {
        return Err(format!("章节不属于该对象: {qname}"));
    }
    let object = repo.read(found.hash).map_err(|error| error.to_string())?;
    design_markdown(&repo, &object, &elements)?.ok_or_else(|| "对象没有文档字段".to_string())
}

pub(super) fn source(
    access: &DesignSpaceAccess,
    path: &Path,
    project: &foundry_domain::Project,
    hash: &str,
    branch: &str,
) -> Result<DesignSpaceSource, String> {
    let repo = open(path)?;
    let Some((_, commit)) = commit_of(&repo, branch)? else {
        return Err("设计空间分支还没有提交".to_string());
    };
    let elements = collect_elements(&repo, &commit)?;
    let found = element(&elements, hash).ok_or_else(|| format!("对象不存在: {hash}"))?;
    if found.path.is_empty() {
        return Err("该对象没有源码锚点".to_string());
    }
    let git_commit = source_commit(&repo, &commit);
    if git_commit.is_empty() || git_commit == ZERO_HEX {
        return Err("该设计空间还没有绑定源码提交".to_string());
    }
    let worktree = access.ensure_source(project, &git_commit)?;
    let text = read_blob(&worktree, &git_commit, &found.path)?;
    source_lines(
        &text,
        &git_commit,
        &found.path,
        found.start_line,
        found.end_line,
    )
}

pub(super) fn branches(path: &Path) -> Result<Vec<String>, String> {
    let repo = open(path)?;
    let mut branches = repo.branch_list().map_err(|error| error.to_string())?;
    branches.sort();
    Ok(branches)
}

pub(super) fn branch_head(path: &Path, branch: &str) -> Result<Option<String>, String> {
    let repo = open(path)?;
    let name = format!("refs/heads/{branch}");
    Ok(repo
        .ref_hash(&name)
        .map_err(|error| error.to_string())?
        .map(CommitHash::to_hex))
}

pub(super) fn is_ancestor(path: &Path, ancestor: &str, descendant: &str) -> Result<bool, String> {
    let repo = open(path)?;
    let ancestor = CommitHash::from_hex(ancestor).map_err(|error| error.to_string())?;
    let descendant = CommitHash::from_hex(descendant).map_err(|error| error.to_string())?;
    repo.is_ancestor(ancestor, descendant)
        .map_err(|error| error.to_string())
}

pub(super) fn delete_branch(path: &Path, branch: &str) -> Result<(), String> {
    let repo = open(path)?;
    repo.branch_delete(branch)
        .map_err(|error| error.to_string())
}

pub(super) fn fast_forward(
    path: &Path,
    target_branch: &str,
    expected_target: &str,
    source_commit: &str,
) -> Result<(), String> {
    let repo = open(path)?;
    let name = format!("refs/heads/{target_branch}");
    let current = repo
        .ref_hash(&name)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("分支不存在: {target_branch}"))?;
    if current.to_hex() == source_commit {
        return Ok(());
    }
    if current.to_hex() != expected_target {
        return Err("目标分支在合并请求创建后已经前进，请重新创建合并请求".to_string());
    }
    let expected = CommitHash::from_hex(expected_target).map_err(|error| error.to_string())?;
    let source = CommitHash::from_hex(source_commit).map_err(|error| error.to_string())?;
    if !repo
        .is_ancestor(expected, source)
        .map_err(|error| error.to_string())?
    {
        return Err("源分支不是目标分支的后代，无法快进合并".to_string());
    }
    repo.update_ref_checked(&name, Some(expected), source, false)
        .map_err(|error| error.to_string())
}

pub(super) fn comment_count(path: &Path, branch: &str, commit: &str) -> Result<usize, String> {
    let repo = open(path)?;
    let hash = CommitHash::from_hex(commit).map_err(|error| error.to_string())?;
    let commit = repo.read_commit(hash).map_err(|error| error.to_string())?;
    let qnames = commit
        .objects
        .iter()
        .map(|(qname, _)| symbol(&repo, *qname))
        .collect::<BTreeSet<_>>();
    let comments = repo
        .comment_list_on(branch, None, false)
        .map_err(|error| error.to_string())?;
    Ok(comments
        .into_iter()
        .filter(|comment| qnames.contains(&comment.target))
        .count())
}

pub(super) fn content_digest(path: &Path, commit: &str) -> Result<String, String> {
    let repo = open(path)?;
    let hash = CommitHash::from_hex(commit).map_err(|error| error.to_string())?;
    let commit = repo.read_commit(hash).map_err(|error| error.to_string())?;
    let mut entries = commit
        .objects
        .iter()
        .map(|(qname, object)| (symbol(&repo, *qname), object.to_hex()))
        .collect::<Vec<_>>();
    entries.sort();
    let mut hasher = Sha256::new();
    for (qname, object) in entries {
        hasher.update(qname.as_bytes());
        hasher.update([0]);
        hasher.update(object.as_bytes());
        hasher.update([b'\n']);
    }
    Ok(super::to_hex_hash(&hasher.finalize()))
}

pub(super) fn diff(path: &Path, from: &str, to: &str) -> Result<MergeRequestDiff, String> {
    let repo = open(path)?;
    let from = CommitHash::from_hex(from).map_err(|error| error.to_string())?;
    let to = CommitHash::from_hex(to).map_err(|error| error.to_string())?;
    let from = repo.read_commit(from).map_err(|error| error.to_string())?;
    let to = repo.read_commit(to).map_err(|error| error.to_string())?;
    let before = snapshot(&repo, &from)?;
    let after = snapshot(&repo, &to)?;
    let names = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut changes = Vec::new();
    for qname in names {
        let old = before.get(&qname);
        let new = after.get(&qname);
        let (status, hash) = match (old, new) {
            (None, Some(hash)) => (MergeRequestChangeStatus::Added, *hash),
            (Some(hash), None) => (MergeRequestChangeStatus::Removed, *hash),
            (Some(old), Some(new)) if old != new => (MergeRequestChangeStatus::Modified, *new),
            _ => continue,
        };
        let object = repo.read(hash).map_err(|error| error.to_string())?;
        let fields = match (old, new) {
            (Some(old), Some(new)) => field_changes(&repo, *old, *new)?,
            (None, Some(new)) => field_changes(&repo, ObjectHash::ZERO, *new)?,
            (Some(old), None) => field_changes(&repo, *old, ObjectHash::ZERO)?,
            (None, None) => Vec::new(),
        };
        changes.push(MergeRequestChange {
            status,
            qname,
            kind: kind_label(&object).to_string(),
            fields,
        });
    }
    changes.sort_by(|left, right| left.qname.cmp(&right.qname));
    Ok(MergeRequestDiff {
        changes,
        documents: Vec::new(),
    })
}

fn snapshot(repo: &Repository, commit: &Commit) -> Result<BTreeMap<String, ObjectHash>, String> {
    Ok(commit
        .objects
        .iter()
        .map(|(qname, hash)| (symbol(repo, *qname), *hash))
        .collect())
}

fn field_changes(
    repo: &Repository,
    before: ObjectHash,
    after: ObjectHash,
) -> Result<Vec<MergeRequestFieldChange>, String> {
    let before = if before.is_zero() {
        BTreeMap::new()
    } else {
        field_map(repo, before)?
    };
    let after = if after.is_zero() {
        BTreeMap::new()
    } else {
        field_map(repo, after)?
    };
    let names = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut changes = Vec::new();
    for name in names {
        if before.get(&name) == after.get(&name) {
            continue;
        }
        changes.push(MergeRequestFieldChange {
            field: name.clone(),
            before: before.get(&name).cloned(),
            after: after.get(&name).cloned(),
        });
    }
    Ok(changes)
}

fn field_map(repo: &Repository, hash: ObjectHash) -> Result<BTreeMap<String, String>, String> {
    let object = repo.read(hash).map_err(|error| error.to_string())?;
    let mut map = BTreeMap::new();
    for (field, value) in arche_core::repo::doc_fields(&object) {
        if value.is_zero() {
            continue;
        }
        let field_object = repo.read(value).map_err(|error| error.to_string())?;
        let mut text = String::new();
        render_block(repo, &field_object, 3, &mut text)?;
        map.insert(field.to_owned(), text.trim_end().to_owned());
    }
    Ok(map)
}

fn attribute_label(object: &Object, name: &str) -> String {
    match name {
        "display_name" if matches!(object, Object::RequirementUser(_)) => "姓名".to_string(),
        "display_name" => "名称".to_string(),
        "age" => "年龄".to_string(),
        "occupation" => "职业".to_string(),
        "priority" => "优先级".to_string(),
        "stories" => "关联故事".to_string(),
        "frequency" => "频率".to_string(),
        "users" => "参与用户".to_string(),
        "protagonist" => "主角".to_string(),
        "functions" => "涉及功能".to_string(),
        other => other.to_string(),
    }
}

fn doc_field_label(object: &Object, field: &str) -> String {
    let label = match (object, field) {
        (Object::RequirementUser(_), "situations") => "处境",
        (Object::RequirementUser(_), "hobbies") => "爱好",
        (Object::RequirementUser(_), "needs") => "需求",
        (Object::RequirementStory(_), "preconditions") => "前置条件",
        (Object::RequirementStory(_), "description") => "用户旅程",
        (Object::RequirementStory(_), "requirement") => "需求",
        (Object::RequirementStory(_), "flow") => "流程图",
        (Object::RequirementSystem(_), "position") => "定位",
        (Object::RequirementSystem(_), "app_form") => "应用形态",
        (Object::RequirementModule(_), "position") => "定位",
        (Object::RequirementFunction(_), "initiator") => "发起者",
        (Object::RequirementFunction(_), "system_change") => "系统变化",
        (Object::RequirementFunction(_), "preconditions") => "前置条件",
        (Object::RequirementFunction(_), "feedback") => "反馈",
        (Object::RequirementFunction(_), "description") => "描述",
        (Object::RequirementFunction(_), "flow") => "流程图",
        (Object::RequirementInteraction(_), "description") => "描述",
        (Object::RequirementInteraction(_), "ui") => "界面",
        _ => field,
    };
    label.to_string()
}

fn qname_link(qname: &str, elements: &[Element]) -> String {
    if let Some(element) = elements.iter().find(|element| element.qname == qname) {
        let label = if element.name.is_empty() {
            qname
        } else {
            element.name.as_str()
        };
        format!("[{label}](#ds-{})", element.hash.to_hex())
    } else {
        qname.to_string()
    }
}

fn attribute_value(
    repo: &Repository,
    object: &Object,
    name: &str,
    value: String,
    elements: &[Element],
) -> String {
    if name == "priority" {
        if let Ok(priority) = value.parse::<u8>() {
            return priority_display(priority);
        }
    }
    match object.get_property(name) {
        Ok(PropertyValue::StringRef(hash)) if !hash.is_zero() => repo
            .resolve(hash)
            .map(|qname| qname_link(&qname, elements))
            .unwrap_or(value),
        Ok(PropertyValue::StringRefList(values)) => values
            .as_slice()
            .iter()
            .map(|hash| {
                repo.resolve(*hash)
                    .map(|qname| qname_link(&qname, elements))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|values| values.join("、"))
            .unwrap_or(value),
        _ => value,
    }
}

fn design_markdown(
    repo: &Repository,
    object: &Object,
    elements: &[Element],
) -> Result<Option<String>, String> {
    let mut out = String::new();
    let attributes = repo
        .object_attributes(object)
        .map_err(|error| error.to_string())?;
    let is_user = matches!(object, Object::RequirementUser(_));
    if is_user {
        let attributes = attributes.iter().cloned().collect::<BTreeMap<_, _>>();
        for (name, label) in [
            ("display_name", "姓名"),
            ("age", "年龄"),
            ("occupation", "职业"),
            ("priority", "优先级"),
        ] {
            let Some(value) = attributes.get(name) else {
                continue;
            };
            let value = attribute_value(repo, object, name, value.clone(), elements);
            if value.trim().is_empty() || value == "未标定" {
                continue;
            }
            out.push_str(&format!("## {label}\n\n{value}\n\n"));
        }
    } else if !attributes.is_empty() {
        out.push_str("## 属性\n\n");
        for (name, value) in attributes {
            let value = attribute_value(repo, object, &name, value, elements);
            out.push_str(&format!(
                "- {}：{}\n",
                attribute_label(object, &name),
                value
            ));
        }
        out.push('\n');
    }
    for (field, value) in arche_core::repo::doc_fields(object) {
        if value.is_zero() {
            continue;
        }
        let field_object = repo.read(value).map_err(|error| error.to_string())?;
        out.push_str(&format!("## {}\n\n", doc_field_label(object, field)));
        render_block(repo, &field_object, 3, &mut out)?;
        out.push('\n');
    }
    if is_user {
        out.push_str("## 关联故事\n\n");
        match object.get_property("stories") {
            Ok(PropertyValue::StringRefList(values)) if !values.as_slice().is_empty() => {
                for hash in values.as_slice() {
                    if let Ok(qname) = repo.resolve(*hash) {
                        out.push_str(&format!("- {}\n", qname_link(&qname, elements)));
                    }
                }
            }
            _ => out.push_str("- 暂无\n"),
        }
        out.push('\n');
    }
    if out.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}

fn render_block(
    repo: &Repository,
    object: &Object,
    level: usize,
    out: &mut String,
) -> Result<(), String> {
    match object {
        Object::DocumentText(value) => {
            out.push_str(value.content());
            out.push_str("\n\n");
        }
        Object::DocumentList(value) => {
            for hash in value.items().as_slice() {
                let item = repo.read(*hash).map_err(|error| error.to_string())?;
                match item {
                    Object::DocumentText(text) => {
                        out.push_str(&format!("- {}\n", text.content()));
                    }
                    other => {
                        out.push_str(&format!(
                            "- {}: {}\n",
                            kind_label(&other),
                            symbol(repo, other.qname())
                        ));
                    }
                }
            }
            out.push('\n');
        }
        Object::DocumentParagraphList(value) => {
            for hash in value.items().as_slice() {
                let item = repo.read(*hash).map_err(|error| error.to_string())?;
                render_block(repo, &item, level + 1, out)?;
            }
        }
        Object::DocumentTopic(value) => {
            out.push_str(&format!("{} {}\n\n", "#".repeat(level), value.title()));
            let body = repo
                .read(value.body().get())
                .map_err(|error| error.to_string())?;
            render_block(repo, &body, level + 1, out)?;
        }
        Object::DocumentCode(value) => {
            out.push_str(&format!(
                "```{}\n{}\n```\n\n",
                value.language(),
                value.source()
            ));
        }
        Object::DocumentMermaid(value) => {
            out.push_str(&format!("```mermaid\n{}\n```\n\n", value.source()));
        }
        Object::DocumentHtml(value) => {
            out.push_str(&format!("```html\n{}\n```\n\n", value.source()));
        }
        other => {
            out.push_str(&format!(
                "- {}: {}\n\n",
                kind_label(other),
                symbol(repo, other.qname())
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn local_part_strips_hash_and_parent() {
        assert_eq!(local_part("technical/namespace/app/run#abcd1234"), "run");
        assert_eq!(local_part("technical/file/src/main.rs"), "main.rs");
    }

    #[test]
    fn reads_an_arche_repository() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        repo.create_technical_system("app").unwrap();
        repo.set_document_text("technical/app", "position", "应用入口。")
            .unwrap();
        repo.add_all().unwrap();
        repo.commit_index("init", true).unwrap();
        let branch = repo.current_branch().unwrap();

        let overview = overview(temp.path(), &branch).unwrap();
        assert!(overview.version.is_some());
        let element = overview
            .elements
            .iter()
            .find(|element| element.qname == "technical/app")
            .expect("system element");
        assert_eq!(element.kind, "system");
        assert!(element.has_design);

        let detail = detail(temp.path(), &element.hash, &branch).unwrap();
        assert!(detail.design.unwrap_or_default().contains("应用入口"));

        let head = branch_head(temp.path(), &branch).unwrap().unwrap();
        assert!(branches(temp.path()).unwrap().contains(&branch));
        assert_eq!(content_digest(temp.path(), &head).unwrap().len(), 64);
        assert_eq!(comment_count(temp.path(), &branch, &head).unwrap(), 0);
    }

    #[test]
    fn requirement_items_nest_under_their_list_nodes() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let (user, _) = repo.create_requirement_user("protagonist", "主角").unwrap();
        let user = repo.resolve(user).unwrap();
        repo.set_scalar(&user, "age", "35").unwrap();
        repo.set_scalar(&user, "occupation", "产品设计师").unwrap();
        repo.set_scalar(&user, "priority", "2").unwrap();
        let (story, _) = repo.create_requirement_story("journey").unwrap();
        let story = repo.resolve(story).unwrap();
        repo.set_refs(&user, "stories", &[story]).unwrap();
        let (system, _) = repo.create_requirement_system("sys").unwrap();
        let system = repo.resolve(system).unwrap();
        repo.set_document_text(&system, "position", "仓库管理平台")
            .unwrap();
        repo.set_document_text(&system, "app_form", "Web 应用")
            .unwrap();
        repo.add_all().unwrap();
        repo.commit_index("requirements", true).unwrap();
        let branch = repo.current_branch().unwrap();

        let overview = overview(temp.path(), &branch).unwrap();
        let members = |qname: &str| {
            overview
                .elements
                .iter()
                .find(|element| element.qname == qname)
                .map(|element| element.members.clone())
                .unwrap_or_default()
        };
        let root = members("requirement");
        assert!(root.contains(&"requirement/user".to_string()));
        assert!(root.contains(&"requirement/story".to_string()));
        assert!(root.contains(&"requirement/system".to_string()));
        assert!(!root.contains(&user));
        assert!(!root.contains(&"requirement/story/journey".to_string()));
        assert!(members("requirement/user").contains(&user));
        assert!(members("requirement/story").contains(&"requirement/story/journey".to_string()));
        assert!(members("requirement/system").contains(&"requirement/system/sys".to_string()));
        for (qname, label) in [
            ("requirement/user", "用户"),
            ("requirement/story", "故事"),
            ("requirement/system", "系统"),
        ] {
            let element = overview
                .elements
                .iter()
                .find(|element| element.qname == qname)
                .expect("skeleton list");
            assert_eq!(element.name, label);
        }

        let user_element = overview
            .elements
            .iter()
            .find(|element| element.qname == user)
            .expect("user element");
        assert_eq!(user_element.name, "主角");
        let user_hash = repo.find_by_qname(&user).unwrap();
        let user_detail = detail(temp.path(), &user_hash.to_hex(), &branch).unwrap();
        let design = user_detail.design.unwrap_or_default();
        assert!(!design.contains("## 属性"), "{design}");
        assert!(design.contains("## 姓名\n\n主角"), "{design}");
        assert!(design.contains("## 年龄\n\n35"), "{design}");
        assert!(design.contains("## 职业\n\n产品设计师"), "{design}");
        assert!(
            design.contains("## 优先级\n\n主要用户（primary）"),
            "{design}"
        );
        assert!(design.contains("[journey](#ds-"), "{design}");
        assert!(!design.contains("[requirement/story/journey]"), "{design}");
        let stories = design.find("## 关联故事").expect("stories heading");
        assert!(stories > design.find("## 优先级").expect("priority heading"));
        assert_eq!(design.rfind("## "), Some(stories), "{design}");
        assert!(design[stories..].contains("- [journey](#ds-"), "{design}");

        let system_hash = repo.find_by_qname(&system).unwrap();
        let system_detail = detail(temp.path(), &system_hash.to_hex(), &branch).unwrap();
        let design = system_detail.design.unwrap_or_default();
        assert!(design.contains("## 定位\n\n仓库管理平台"), "{design}");
        assert!(design.contains("## 应用形态\n\nWeb 应用"), "{design}");
        assert!(system_detail.members.is_empty());
    }

    #[test]
    fn init_skeleton_is_visible_in_the_tree() {
        let temp = TempDir::new().unwrap();
        Repository::init(temp.path()).unwrap();
        let overview = overview(temp.path(), "main").unwrap();
        let by_qname: std::collections::HashMap<_, _> = overview
            .elements
            .iter()
            .map(|element| (element.qname.as_str(), element))
            .collect();

        for (qname, kind) in [
            ("project", "design-structure"),
            ("project/document", "project-document"),
            ("technical", "design-structure"),
            ("technical/directory", "list"),
            ("technical/file", "list"),
            ("technical/namespace", "list"),
            ("technical/system", "list"),
            ("requirement", "design-structure"),
            ("requirement/user", "list"),
            ("requirement/story", "list"),
            ("requirement/system", "list"),
            ("concept", "concept-directory"),
            ("concept/directories", "list"),
            ("concept/concepts", "list"),
            ("concept/domain", "concept-directory"),
            ("concept/domain/directories", "list"),
            ("concept/domain/concepts", "list"),
            ("concept/requirement", "concept-directory"),
            ("concept/requirement/directories", "list"),
            ("concept/requirement/concepts", "list"),
            ("concept/technical", "concept-directory"),
            ("concept/technical/directories", "list"),
            ("concept/technical/concepts", "list"),
        ] {
            let element = by_qname
                .get(qname)
                .unwrap_or_else(|| panic!("missing skeleton element {qname}"));
            assert_eq!(element.kind, kind, "{qname} kind");
        }
        assert!(!by_qname.contains_key("root"), "commit root stays hidden");
        assert!(is_skeleton_qname("project"));
        assert!(!is_skeleton_qname("project/document"));
        assert!(by_qname["project/document"].has_design);
        assert!(
            by_qname["project"]
                .members
                .contains(&"project/document".to_string())
        );
        assert!(
            by_qname["technical"]
                .members
                .contains(&"technical/system".to_string())
        );
        assert!(
            by_qname["concept"]
                .members
                .contains(&"concept/domain".to_string())
        );
    }
}
