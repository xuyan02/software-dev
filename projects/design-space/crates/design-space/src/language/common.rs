//! Shared entity tree and naming helpers.

use std::collections::BTreeMap;

use tree_sitter::Node;

use crate::element::Element;

#[derive(Clone)]
pub(crate) struct Entity {
    pub(crate) name: String,
    pub(crate) full: Option<String>,
    pub(crate) ty: u32,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) children: Vec<Entity>,
}

impl Entity {
    pub(crate) fn new(name: String, ty: u32, node: Node, children: Vec<Entity>) -> Self {
        Self {
            name,
            full: None,
            ty,
            start: node.start_byte(),
            end: node.end_byte(),
            children,
        }
    }
}

/// Assign final names, flatten the tree and apply the language's root prefix.
pub(crate) fn finish(
    mut entities: Vec<Entity>,
    path: &str,
    commit_hex: &str,
    source: &str,
    prefix: Option<&str>,
) -> Vec<Element> {
    let mut used = BTreeMap::new();
    assign_names(&mut entities, None, source, &mut used);

    let mut out = Vec::new();
    flatten(entities, None, path, commit_hex, source, &mut out);

    if let Some(prefix) = prefix {
        prefix_roots(&mut out, prefix);
    }
    out
}

pub(crate) fn named_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

pub(crate) fn text(node: Node, source: &str) -> String {
    node.utf8_text(source.as_bytes())
        .unwrap_or("")
        .trim()
        .to_owned()
}

pub(crate) fn field_text(node: Node, field: &str, source: &str) -> Option<String> {
    node.child_by_field_name(field)
        .map(|child| text(child, source))
        .filter(|value| !value.is_empty())
}

pub(crate) fn anonymous(node: Node, kind: &str, source: &str) -> String {
    format!("({kind})@{}", line_of(source, node.start_byte()))
}

pub(crate) fn line_of(source: &str, offset: usize) -> u32 {
    let limit = offset.min(source.len());
    source.as_bytes()[..limit]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count() as u32
        + 1
}

pub(crate) fn file_stem(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?;
    let stem = name.split('.').next()?;
    (!stem.is_empty()).then(|| stem.to_owned())
}

fn assign_names(
    entities: &mut [Entity],
    parent: Option<&str>,
    source: &str,
    used: &mut BTreeMap<String, usize>,
) {
    for entity in entities {
        let mut name = match parent {
            Some(parent) => format!("{parent}::{}", entity.name),
            None => entity.name.clone(),
        };
        if used.contains_key(&name) {
            name = format!("{name}@{}", line_of(source, entity.start));
        }
        used.insert(name.clone(), 1);
        entity.full = Some(name.clone());
        assign_names(&mut entity.children, Some(&name), source, used);
    }
}

fn flatten(
    entities: Vec<Entity>,
    parent: Option<&str>,
    path: &str,
    commit_hex: &str,
    source: &str,
    out: &mut Vec<Element>,
) {
    for entity in entities {
        let name = entity
            .full
            .clone()
            .expect("assign_names runs before flatten");
        let children: Vec<String> = entity
            .children
            .iter()
            .map(|child| {
                child
                    .full
                    .clone()
                    .expect("assign_names runs before flatten")
            })
            .collect();
        out.push(Element {
            name: name.clone(),
            ty: entity.ty,
            path: path.to_owned(),
            commit: commit_hex.to_owned(),
            start_byte: entity.start as u64,
            end_byte: entity.end as u64,
            start_line: line_of(source, entity.start),
            end_line: line_of(source, entity.end.saturating_sub(1)),
            own_text: own_text(source, entity.start, entity.end, &entity.children),
            parent: parent.map(str::to_owned),
            children,
            design: None,
            content_hash: None,
        });
        flatten(entity.children, Some(&name), path, commit_hex, source, out);
    }
}

fn own_text(source: &str, start: usize, end: usize, children: &[Entity]) -> String {
    let mut ranges: Vec<(usize, usize)> = children
        .iter()
        .map(|child| (child.start.max(start), child.end.min(end)))
        .filter(|(child_start, child_end)| child_start < child_end)
        .collect();
    ranges.sort_unstable();

    let mut out = String::new();
    let mut cursor = start;
    for (child_start, child_end) in ranges {
        if child_start > cursor {
            out.push_str(source.get(cursor..child_start).unwrap_or(""));
        }
        cursor = cursor.max(child_end);
    }
    if cursor < end {
        out.push_str(source.get(cursor..end).unwrap_or(""));
    }
    out
}

fn prefix_roots(elements: &mut [Element], prefix: &str) {
    let roots: Vec<String> = elements
        .iter()
        .filter(|element| element.parent.is_none())
        .map(|element| element.name.clone())
        .collect();
    for root in roots {
        if root.starts_with(&format!("{prefix}::")) {
            continue;
        }
        let renamed = format!("{prefix}::{root}");
        rename_subtree(elements, &root, &renamed);
    }
}

fn rename_subtree(elements: &mut [Element], old: &str, new: &str) {
    let old_prefix = format!("{old}::");
    let new_prefix = format!("{new}::");

    for element in elements.iter_mut() {
        if element.name == old {
            element.name = new.to_owned();
        } else if let Some(rest) = element.name.strip_prefix(&old_prefix) {
            element.name = format!("{new_prefix}{rest}");
        }

        if element.parent.as_deref() == Some(old) {
            element.parent = Some(new.to_owned());
        } else {
            let parent = element.parent.clone();
            if let Some(parent) = parent
                && let Some(rest) = parent.strip_prefix(&old_prefix)
            {
                element.parent = Some(format!("{new_prefix}{rest}"));
            }
        }

        for child in &mut element.children {
            if child == old {
                *child = new.to_owned();
            } else if let Some(rest) = child.strip_prefix(&old_prefix) {
                *child = format!("{new_prefix}{rest}");
            }
        }
    }
}
