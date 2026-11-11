//! Schema-described documents stored as content-addressed anchor trees.
//!
//! A document is a root anchor. Chapters and list items are anchors too. The
//! schema describes the chapter tree, cardinality and content kind; the runtime
//! tree carries the actual content. Every anchor has a stable key for addressing
//! and a SHA-256 content hash as its ID.

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Validation, ValidationIssue};

pub const ANCHOR_DOCUMENT_SCHEMA_VERSION: u32 = 1;
pub const MAX_ANCHOR_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;

fn hash_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hash_hex(bytes))
}

/// Stable location key derived from the parent key, schema key and occurrence.
/// It does not depend on content, so edits and reordering keep the key.
fn location_key(parent_key: &str, schema_key: &str, occurrence_key: &str) -> String {
    let payload = json!({
        "parent": parent_key,
        "schema": schema_key,
        "occurrence": occurrence_key,
    });
    let bytes = serde_json::to_vec(&payload).expect("location payload is serializable");
    format!("k:{}", hash_hex(&bytes))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Occurrence {
    Single,
    Multiple,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemSchema {
    pub key: String,
    #[serde(default)]
    pub text_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentSchema {
    Text,
    List {
        item: ItemSchema,
        #[serde(default)]
        min: usize,
        #[serde(default)]
        max: Option<usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChapterSchema {
    pub key: String,
    pub title: String,
    pub occurrence: Occurrence,
    pub content: ContentSchema,
    #[serde(default)]
    pub children: Vec<ChapterSchema>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub min: usize,
    #[serde(default)]
    pub max: Option<usize>,
    #[serde(default)]
    pub text_required: bool,
}

impl ChapterSchema {
    fn required(mut self, value: bool) -> Self {
        self.required = value;
        self
    }

    fn text_required(mut self, value: bool) -> Self {
        self.text_required = value;
        self
    }

    #[allow(dead_code)]
    fn min(mut self, value: usize) -> Self {
        self.min = value;
        self
    }

    #[allow(dead_code)]
    fn max(mut self, value: usize) -> Self {
        self.max = Some(value);
        self
    }

    fn children(mut self, children: Vec<ChapterSchema>) -> Self {
        self.children = children;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentSchema {
    pub kind: String,
    pub schema_version: u32,
    pub root: ChapterSchema,
}

impl DocumentSchema {
    pub fn chapter(&self, key: &str) -> Option<&ChapterSchema> {
        fn walk<'a>(node: &'a ChapterSchema, key: &str) -> Option<&'a ChapterSchema> {
            if node.key == key {
                return Some(node);
            }
            node.children.iter().find_map(|child| walk(child, key))
        }
        walk(&self.root, key)
    }

    pub fn item(&self, key: &str) -> Option<&ItemSchema> {
        fn walk<'a>(node: &'a ChapterSchema, key: &str) -> Option<&'a ItemSchema> {
            if let ContentSchema::List { item, .. } = &node.content
                && item.key == key
            {
                return Some(item);
            }
            node.children.iter().find_map(|child| walk(child, key))
        }
        walk(&self.root, key)
    }

    pub fn validate(&self) -> Validation {
        let mut validation = Validation::default();
        if self.root.occurrence != Occurrence::Single {
            validation.errors.push(ValidationIssue::error(
                "root.occurrence",
                "invalid_root_occurrence",
                "the document root must be single",
            ));
        }
        let mut keys = HashSet::new();
        fn walk(node: &ChapterSchema, keys: &mut HashSet<String>, validation: &mut Validation) {
            if !keys.insert(node.key.clone()) {
                validation.errors.push(ValidationIssue::error(
                    format!("{}.key", node.key),
                    "duplicate_key",
                    format!("duplicate schema key {}", node.key),
                ));
            }
            if let ContentSchema::List { item, min, max } = &node.content {
                if !node.children.is_empty() {
                    validation.errors.push(ValidationIssue::error(
                        format!("{}.children", node.key),
                        "list_has_children",
                        "list chapters cannot have subchapters",
                    ));
                }
                if !keys.insert(item.key.clone()) {
                    validation.errors.push(ValidationIssue::error(
                        format!("{}.item.key", node.key),
                        "duplicate_key",
                        format!("duplicate item schema key {}", item.key),
                    ));
                }
                if let Some(max) = max
                    && min > max
                {
                    validation.errors.push(ValidationIssue::error(
                        format!("{}.content", node.key),
                        "invalid_range",
                        "min must not exceed max",
                    ));
                }
            }
            let multiple_children = node
                .children
                .iter()
                .filter(|child| child.occurrence == Occurrence::Multiple)
                .count();
            if multiple_children > 0 && node.children.len() > 1 {
                validation.errors.push(ValidationIssue::error(
                    format!("{}.children", node.key),
                    "multiple_child_not_alone",
                    "a multiple chapter must be the only child of its parent",
                ));
            }
            let mut child_keys = HashSet::new();
            for child in &node.children {
                if !child_keys.insert(child.key.clone()) {
                    validation.errors.push(ValidationIssue::error(
                        format!("{}.children", node.key),
                        "duplicate_child",
                        format!("duplicate child schema key {}", child.key),
                    ));
                }
                if let Some(max) = child.max
                    && child.min > max
                {
                    validation.errors.push(ValidationIssue::error(
                        format!("{}.min", child.key),
                        "invalid_range",
                        "min must not exceed max",
                    ));
                }
                walk(child, keys, validation);
            }
        }
        walk(&self.root, &mut keys, &mut validation);
        validation
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorKind {
    Document,
    Chapter,
    Item,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Text,
    List,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorNode {
    #[serde(default)]
    pub id: String,
    pub key: String,
    /// Human-readable path, used for debugging and path aliases. It is not
    /// part of the public structure API.
    #[serde(default)]
    pub path: String,
    pub schema_key: String,
    pub kind: AnchorKind,
    pub title: String,
    #[serde(default)]
    pub occurrence_key: String,
    pub content_kind: ContentKind,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub items: Vec<AnchorNode>,
    #[serde(default)]
    pub children: Vec<AnchorNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentSnapshot {
    pub kind: String,
    pub schema_version: u32,
    pub root: AnchorNode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnchorStructure {
    pub id: String,
    pub key: String,
    pub kind: AnchorKind,
    pub title: String,
    #[serde(default)]
    pub occurrence_key: String,
    pub content_kind: ContentKind,
    pub text_preview: String,
    pub items: Vec<AnchorStructure>,
    pub children: Vec<AnchorStructure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnchorAnnotationStatus {
    #[default]
    Open,
    Resolved,
    Stale,
    Orphaned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorAnnotation {
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: String,
    pub anchor_key: String,
    pub anchor_id: String,
    #[serde(default)]
    pub quote: Option<String>,
    pub body: String,
    pub author_id: Uuid,
    pub author_handle: String,
    pub author_name: String,
    #[serde(default)]
    pub status: AnchorAnnotationStatus,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub resolved_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub resolved_by: Option<Uuid>,
}

impl AnchorAnnotation {
    pub fn effective_status(&self, snapshot: &DocumentSnapshot) -> AnchorAnnotationStatus {
        match snapshot.find(&self.anchor_key) {
            None => AnchorAnnotationStatus::Orphaned,
            Some(node) if node.id != self.anchor_id => AnchorAnnotationStatus::Stale,
            Some(_) => self.status,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorOperation {
    pub op_id: Uuid,
    #[serde(flatten)]
    pub op: AnchorOp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnchorOp {
    Add {
        parent: String,
        #[serde(default)]
        occurrence_key: Option<String>,
        #[serde(default)]
        text: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        after: Option<String>,
    },
    UpdateText {
        anchor: String,
        text: String,
    },
    UpdateTitle {
        anchor: String,
        title: String,
    },
    Delete {
        anchor: String,
        #[serde(default)]
        cascade: bool,
    },
    Move {
        anchor: String,
        #[serde(default)]
        after: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnchorOpError {
    #[error("{0}")]
    Invalid(String),
}

fn invalid(message: impl Into<String>) -> AnchorOpError {
    AnchorOpError::Invalid(message.into())
}

impl AnchorNode {
    pub fn hash(&self) -> String {
        let payload = json!({
            "key": self.key,
            "schema_key": self.schema_key,
            "kind": self.kind,
            "occurrence_key": self.occurrence_key,
            "content_kind": self.content_kind,
            "text": self.text,
            "items": self.items.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
            "children": self.children.iter().map(|child| child.id.as_str()).collect::<Vec<_>>(),
        });
        let bytes = serde_json::to_vec(&payload).expect("hash payload is serializable");
        hash_bytes(&bytes)
    }

    fn rehash(&mut self) {
        for item in &mut self.items {
            item.rehash();
        }
        for child in &mut self.children {
            child.rehash();
        }
        self.id = self.hash();
    }

    fn find(&self, key: &str) -> Option<&AnchorNode> {
        if self.key == key {
            return Some(self);
        }
        self.items
            .iter()
            .find_map(|item| item.find(key))
            .or_else(|| self.children.iter().find_map(|child| child.find(key)))
    }

    fn find_path(&self, path: &str) -> Option<&AnchorNode> {
        if self.path == path {
            return Some(self);
        }
        self.items
            .iter()
            .find_map(|item| item.find_path(path))
            .or_else(|| self.children.iter().find_map(|child| child.find_path(path)))
    }

    fn find_by_id(&self, id: &str) -> Option<&AnchorNode> {
        if self.id == id {
            return Some(self);
        }
        self.items
            .iter()
            .find_map(|item| item.find_by_id(id))
            .or_else(|| self.children.iter().find_map(|child| child.find_by_id(id)))
    }

    fn find_mut(&mut self, key: &str) -> Option<&mut AnchorNode> {
        if self.key == key {
            return Some(self);
        }
        for item in &mut self.items {
            if let Some(found) = item.find_mut(key) {
                return Some(found);
            }
        }
        for child in &mut self.children {
            if let Some(found) = child.find_mut(key) {
                return Some(found);
            }
        }
        None
    }

    fn find_parent_mut(&mut self, key: &str) -> Option<&mut AnchorNode> {
        if self.items.iter().any(|item| item.key == key)
            || self.children.iter().any(|child| child.key == key)
        {
            return Some(self);
        }
        for child in &mut self.children {
            if let Some(found) = child.find_parent_mut(key) {
                return Some(found);
            }
        }
        None
    }

    fn descendants_empty(&self) -> bool {
        self.items.is_empty() && self.children.is_empty()
    }

    fn structure(&self) -> AnchorStructure {
        AnchorStructure {
            id: self.id.clone(),
            key: self.key.clone(),
            kind: self.kind,
            title: self.title.clone(),
            occurrence_key: self.occurrence_key.clone(),
            content_kind: self.content_kind,
            text_preview: preview(&self.text),
            items: self.items.iter().map(AnchorNode::structure).collect(),
            children: self.children.iter().map(AnchorNode::structure).collect(),
        }
    }

    fn render_toc(&self, depth: usize, out: &mut String) {
        if self.kind != AnchorKind::Chapter {
            return;
        }
        out.push_str(&"  ".repeat(depth));
        out.push_str("- ");
        out.push_str(&self.title);
        out.push_str(" (`");
        out.push_str(&self.key);
        out.push_str("`)\n");
        for child in &self.children {
            child.render_toc(depth + 1, out);
        }
    }

    fn render_markdown(&self, depth: usize, out: &mut String) {
        match self.kind {
            AnchorKind::Document => {
                for child in &self.children {
                    child.render_markdown(1, out);
                }
            }
            AnchorKind::Item => {
                out.push_str(&self.text);
                out.push('\n');
            }
            AnchorKind::Chapter => {
                let level = depth.min(6);
                out.push_str(&"#".repeat(level));
                out.push(' ');
                out.push_str(&self.title);
                out.push_str("\n\n");
                if !self.text.trim().is_empty() {
                    out.push_str(self.text.trim());
                    out.push_str("\n\n");
                }
                for item in &self.items {
                    out.push_str("- ");
                    out.push_str(item.text.trim());
                    out.push('\n');
                }
                if !self.items.is_empty() {
                    out.push('\n');
                }
                for child in &self.children {
                    child.render_markdown(depth + 1, out);
                }
            }
        }
    }

    /// Markdown with invisible anchor markers used by the web console to
    /// attach annotations to the content they belong to. The plain Markdown
    /// API used by agents never includes these markers.
    fn render_markdown_with_anchors(&self, depth: usize, out: &mut String) {
        match self.kind {
            AnchorKind::Document => {
                for child in &self.children {
                    child.render_markdown_with_anchors(1, out);
                }
            }
            AnchorKind::Item => {
                render_anchor_marker(&self.key, out);
                out.push(' ');
                out.push_str(&self.text);
                out.push('\n');
            }
            AnchorKind::Chapter => {
                render_anchor_marker(&self.key, out);
                out.push('\n');
                let level = depth.min(6);
                out.push_str(&"#".repeat(level));
                out.push(' ');
                out.push_str(&self.title);
                out.push_str("\n\n");
                if !self.text.trim().is_empty() {
                    out.push_str(self.text.trim());
                    out.push_str("\n\n");
                }
                for item in &self.items {
                    out.push_str("- ");
                    render_anchor_marker(&item.key, out);
                    out.push(' ');
                    out.push_str(item.text.trim());
                    out.push('\n');
                }
                if !self.items.is_empty() {
                    out.push('\n');
                }
                for child in &self.children {
                    child.render_markdown_with_anchors(depth + 1, out);
                }
            }
        }
    }
}

fn render_anchor_marker(key: &str, out: &mut String) {
    out.push_str("<!--anchor:");
    out.push_str(key);
    out.push_str("-->");
}

fn preview(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= 120 {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(120).collect();
    out.push('…');
    out
}

impl DocumentSnapshot {
    pub fn empty(schema: &DocumentSchema) -> Self {
        let root_key = location_key("", &schema.root.key, &schema.root.key);
        let mut root = AnchorNode {
            id: String::new(),
            key: root_key.clone(),
            path: schema.root.key.clone(),
            schema_key: schema.root.key.clone(),
            kind: AnchorKind::Document,
            title: schema.root.title.clone(),
            occurrence_key: schema.root.key.clone(),
            content_kind: ContentKind::Text,
            text: String::new(),
            items: Vec::new(),
            children: Vec::new(),
        };
        root.children = skeleton_children(
            &root.key,
            &root.path,
            AnchorKind::Document,
            &schema.root,
            schema,
        );
        root.rehash();
        Self {
            kind: schema.kind.clone(),
            schema_version: schema.schema_version,
            root,
        }
    }

    pub fn root_id(&self) -> &str {
        &self.root.id
    }

    pub fn find(&self, key: &str) -> Option<&AnchorNode> {
        self.root.find(key)
    }

    pub fn find_by_id(&self, id: &str) -> Option<&AnchorNode> {
        self.root.find_by_id(id)
    }

    pub fn find_path(&self, path: &str) -> Option<&AnchorNode> {
        self.root.find_path(path)
    }

    pub fn structure(&self, key: &str) -> Option<AnchorStructure> {
        self.find(key).map(AnchorNode::structure)
    }

    pub fn structure_by_id(&self, id: &str) -> Option<AnchorStructure> {
        self.find_by_id(id).map(AnchorNode::structure)
    }

    pub fn toc(&self, key: &str) -> Option<String> {
        let node = self.find(key)?;
        let mut out = String::new();
        if node.kind == AnchorKind::Document {
            for child in &node.children {
                child.render_toc(0, &mut out);
            }
        } else {
            node.render_toc(0, &mut out);
        }
        Some(out.trim_end().to_string())
    }

    pub fn markdown(&self, key: &str) -> Option<String> {
        let node = self.find(key)?;
        let mut out = String::new();
        node.render_markdown(1, &mut out);
        Some(out.trim_end().to_string())
    }

    /// Markdown with anchor markers for the web console. Callers that only
    /// need content must use [`Self::markdown`] instead.
    pub fn markdown_with_anchors(&self, key: &str) -> Option<String> {
        let node = self.find(key)?;
        let mut out = String::new();
        node.render_markdown_with_anchors(1, &mut out);
        Some(out.trim_end().to_string())
    }

    pub fn markdown_by_id(&self, id: &str) -> Option<String> {
        let node = self.find_by_id(id)?;
        let mut out = String::new();
        node.render_markdown(1, &mut out);
        Some(out.trim_end().to_string())
    }

    pub fn apply(
        &self,
        schema: &DocumentSchema,
        op: &AnchorOp,
    ) -> Result<(DocumentSnapshot, Option<String>), AnchorOpError> {
        let mut next = self.clone();
        let created = apply_op(&mut next.root, schema, op)?;
        retitle(&mut next.root, schema);
        next.root.rehash();
        Ok((next, created))
    }

    pub fn validate(&self, schema: &DocumentSchema) -> Validation {
        validate_document(self, schema)
    }
}

/// Pre-create every single-occurrence chapter under a parent. Multiple
/// chapters are data, not skeleton, so they are created by `add`.
fn skeleton_children(
    parent_key: &str,
    parent_path: &str,
    parent_kind: AnchorKind,
    parent_schema: &ChapterSchema,
    schema: &DocumentSchema,
) -> Vec<AnchorNode> {
    parent_schema
        .children
        .iter()
        .filter(|child| child.occurrence == Occurrence::Single)
        .map(|child| skeleton_chapter(parent_key, parent_path, parent_kind, child, schema))
        .collect()
}

fn skeleton_chapter(
    parent_key: &str,
    parent_path: &str,
    parent_kind: AnchorKind,
    chapter: &ChapterSchema,
    schema: &DocumentSchema,
) -> AnchorNode {
    let key = location_key(parent_key, &chapter.key, &chapter.key);
    let path = if parent_kind == AnchorKind::Document {
        chapter.key.clone()
    } else {
        format!("{parent_path}.{}", chapter.key)
    };
    let mut node = AnchorNode {
        id: String::new(),
        key: key.clone(),
        path: path.clone(),
        schema_key: chapter.key.clone(),
        kind: AnchorKind::Chapter,
        title: chapter.title.clone(),
        occurrence_key: chapter.key.clone(),
        content_kind: match chapter.content {
            ContentSchema::Text => ContentKind::Text,
            ContentSchema::List { .. } => ContentKind::List,
        },
        text: String::new(),
        items: Vec::new(),
        children: Vec::new(),
    };
    node.children = skeleton_children(&key, &path, AnchorKind::Chapter, chapter, schema);
    node.rehash();
    node
}

fn resolve_key<'a>(root: &'a AnchorNode, value: &str) -> Option<&'a str> {
    let node = if let Some(id) = value.strip_prefix("id:") {
        root.find_by_id(id)
    } else if let Some(key) = value.strip_prefix("key:") {
        root.find(key)
    } else if let Some(path) = value.strip_prefix("path:") {
        root.find_path(path)
    } else {
        root.find(value).or_else(|| root.find_path(value))
    };
    node.map(|node| node.key.as_str())
}

fn apply_op(
    root: &mut AnchorNode,
    schema: &DocumentSchema,
    op: &AnchorOp,
) -> Result<Option<String>, AnchorOpError> {
    match op {
        AnchorOp::Add {
            parent,
            occurrence_key,
            text,
            title,
            after,
        } => {
            let parent_key = resolve_key(root, parent)
                .map(str::to_string)
                .ok_or_else(|| invalid(format!("unknown parent {parent}")))?;
            let after_key = match after {
                Some(value) => Some(
                    resolve_key(root, value)
                        .map(str::to_string)
                        .ok_or_else(|| invalid(format!("unknown sibling {value}")))?,
                ),
                None => None,
            };
            add_anchor(
                root,
                schema,
                &parent_key,
                occurrence_key.as_deref(),
                text,
                title.as_deref(),
                after_key.as_deref(),
            )
        }
        AnchorOp::UpdateText { anchor, text } => {
            let anchor_key = resolve_key(root, anchor)
                .map(str::to_string)
                .ok_or_else(|| invalid(format!("unknown anchor {anchor}")))?;
            let node = root
                .find_mut(&anchor_key)
                .ok_or_else(|| invalid(format!("unknown anchor {anchor}")))?;
            if node.kind == AnchorKind::Document {
                return Err(invalid("the document root has no text"));
            }
            node.text = text.clone();
            Ok(None)
        }
        AnchorOp::UpdateTitle { anchor, title } => {
            let anchor_key = resolve_key(root, anchor)
                .map(str::to_string)
                .ok_or_else(|| invalid(format!("unknown anchor {anchor}")))?;
            let node = root
                .find_mut(&anchor_key)
                .ok_or_else(|| invalid(format!("unknown anchor {anchor}")))?;
            if node.kind != AnchorKind::Chapter {
                return Err(invalid("only multiple chapter titles can be changed"));
            }
            let chapter = schema
                .chapter(&node.schema_key)
                .ok_or_else(|| invalid(format!("unknown schema {}", node.schema_key)))?;
            if chapter.occurrence != Occurrence::Multiple {
                return Err(invalid("only multiple chapter titles can be changed"));
            }
            let title = title.trim();
            if title.is_empty() {
                return Err(invalid("title cannot be empty"));
            }
            node.title = title.to_string();
            Ok(None)
        }
        AnchorOp::Delete { anchor, cascade } => {
            let anchor_key = resolve_key(root, anchor)
                .map(str::to_string)
                .ok_or_else(|| invalid(format!("unknown anchor {anchor}")))?;
            if anchor_key == root.key {
                return Err(invalid("the document root cannot be deleted"));
            }
            let target = root
                .find(&anchor_key)
                .ok_or_else(|| invalid(format!("unknown anchor {anchor}")))?;
            if target.kind == AnchorKind::Chapter {
                let chapter = schema
                    .chapter(&target.schema_key)
                    .ok_or_else(|| invalid(format!("unknown schema {}", target.schema_key)))?;
                if chapter.occurrence == Occurrence::Single {
                    return Err(invalid(
                        "single chapters cannot be deleted; update their text instead",
                    ));
                }
            }
            let parent = root
                .find_parent_mut(&anchor_key)
                .ok_or_else(|| invalid(format!("unknown anchor {anchor}")))?;
            if let Some(index) = parent.items.iter().position(|item| item.key == anchor_key) {
                parent.items.remove(index);
                return Ok(None);
            }
            let Some(index) = parent
                .children
                .iter()
                .position(|child| child.key == anchor_key)
            else {
                return Err(invalid(format!("unknown anchor {anchor}")));
            };
            if !cascade && !parent.children[index].descendants_empty() {
                return Err(invalid(format!(
                    "anchor {anchor} has descendants; pass cascade=true to delete"
                )));
            }
            parent.children.remove(index);
            Ok(None)
        }
        AnchorOp::Move { anchor, after } => {
            let anchor_key = resolve_key(root, anchor)
                .map(str::to_string)
                .ok_or_else(|| invalid(format!("unknown anchor {anchor}")))?;
            if anchor_key == root.key {
                return Err(invalid("the document root cannot be moved"));
            }
            let after_key = match after {
                Some(value) => Some(
                    resolve_key(root, value)
                        .map(str::to_string)
                        .ok_or_else(|| invalid(format!("unknown sibling {value}")))?,
                ),
                None => None,
            };
            if after_key.as_deref() == Some(anchor_key.as_str()) {
                return Err(invalid("an anchor cannot be moved after itself"));
            }
            let parent = root
                .find_parent_mut(&anchor_key)
                .ok_or_else(|| invalid(format!("unknown anchor {anchor}")))?;
            if let Some(index) = parent.items.iter().position(|item| item.key == anchor_key) {
                let node = parent.items.remove(index);
                let position = match &after_key {
                    None => 0,
                    Some(after) => parent
                        .items
                        .iter()
                        .position(|item| item.key == *after)
                        .map(|index| index + 1)
                        .ok_or_else(|| invalid(format!("unknown sibling {after}")))?,
                };
                parent.items.insert(position, node);
                return Ok(None);
            }
            let Some(index) = parent
                .children
                .iter()
                .position(|child| child.key == anchor_key)
            else {
                return Err(invalid(format!("unknown anchor {anchor}")));
            };
            let node = parent.children.remove(index);
            let position = match &after_key {
                None => 0,
                Some(after) => parent
                    .children
                    .iter()
                    .position(|child| child.key == *after)
                    .map(|index| index + 1)
                    .ok_or_else(|| invalid(format!("unknown sibling {after}")))?,
            };
            parent.children.insert(position, node);
            Ok(None)
        }
    }
}

fn add_anchor(
    root: &mut AnchorNode,
    schema: &DocumentSchema,
    parent_key: &str,
    occurrence_key: Option<&str>,
    text: &str,
    title: Option<&str>,
    after: Option<&str>,
) -> Result<Option<String>, AnchorOpError> {
    let parent = root
        .find_mut(parent_key)
        .ok_or_else(|| invalid(format!("unknown parent {parent_key}")))?;
    if parent.kind == AnchorKind::Item {
        return Err(invalid("list items cannot have children"));
    }
    let parent_schema = if parent.kind == AnchorKind::Document {
        &schema.root
    } else {
        schema
            .chapter(&parent.schema_key)
            .ok_or_else(|| invalid(format!("unknown schema {}", parent.schema_key)))?
    };

    let schema_key = match &parent_schema.content {
        ContentSchema::List { item, .. } => item.key.as_str(),
        ContentSchema::Text => match parent_schema.children.as_slice() {
            [only] => only.key.as_str(),
            _ => {
                return Err(invalid(format!(
                    "parent {parent_key} must have exactly one child schema"
                )));
            }
        },
    };

    if let ContentSchema::List { item, .. } = &parent_schema.content {
        let occurrence_key = occurrence_key
            .map(str::to_string)
            .unwrap_or_else(|| Uuid::now_v7().to_string());
        let key = location_key(&parent.key, &item.key, &occurrence_key);
        let path = format!("{}/{}", parent.path, occurrence_key);
        if parent.items.iter().any(|child| child.key == key) {
            return Err(invalid(format!("duplicate anchor key {key}")));
        }
        let node = AnchorNode {
            id: String::new(),
            key: key.clone(),
            path,
            schema_key: item.key.clone(),
            kind: AnchorKind::Item,
            title: String::new(),
            occurrence_key: occurrence_key.clone(),
            content_kind: ContentKind::Text,
            text: text.to_string(),
            items: Vec::new(),
            children: Vec::new(),
        };
        let position = insertion_index(&parent.items, after)?;
        parent.items.insert(position, node);
        return Ok(Some(key));
    }

    let child_schema = parent_schema
        .children
        .first()
        .ok_or_else(|| invalid(format!("parent {parent_key} has no child schema")))?;

    let (occurrence_key, key, path, node_title) = match child_schema.occurrence {
        Occurrence::Single => {
            if parent
                .children
                .iter()
                .any(|child| child.schema_key == schema_key)
            {
                return Err(invalid(format!(
                    "chapter {schema_key} already exists under {parent_key}"
                )));
            }
            let occurrence_key = schema_key.to_string();
            let key = location_key(&parent.key, schema_key, &occurrence_key);
            let path = if parent.kind == AnchorKind::Document {
                schema_key.to_string()
            } else {
                format!("{}.{}", parent.path, schema_key)
            };
            (occurrence_key, key, path, child_schema.title.clone())
        }
        Occurrence::Multiple => {
            let title = title
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .ok_or_else(|| invalid(format!("chapter {schema_key} requires a title")))?;
            let occurrence_key = occurrence_key
                .map(str::to_string)
                .unwrap_or_else(|| Uuid::now_v7().to_string());
            let key = location_key(&parent.key, schema_key, &occurrence_key);
            let path = if parent.kind == AnchorKind::Document {
                format!("{}/{}", schema_key, occurrence_key)
            } else {
                format!("{}/{}/{}", parent.path, schema_key, occurrence_key)
            };
            if parent.children.iter().any(|child| child.key == key) {
                return Err(invalid(format!("duplicate anchor key {key}")));
            }
            (occurrence_key, key, path, title.to_string())
        }
    };

    let node = AnchorNode {
        id: String::new(),
        key: key.clone(),
        path: path.clone(),
        schema_key: child_schema.key.clone(),
        kind: AnchorKind::Chapter,
        title: node_title,
        occurrence_key,
        content_kind: match child_schema.content {
            ContentSchema::Text => ContentKind::Text,
            ContentSchema::List { .. } => ContentKind::List,
        },
        text: text.to_string(),
        items: Vec::new(),
        children: skeleton_children(&key, &path, AnchorKind::Chapter, child_schema, schema),
    };
    let position = insertion_index(&parent.children, after)?;
    parent.children.insert(position, node);
    Ok(Some(key))
}

fn insertion_index(list: &[AnchorNode], after: Option<&str>) -> Result<usize, AnchorOpError> {
    match after {
        None => Ok(list.len()),
        Some(after) => list
            .iter()
            .position(|item| item.key == after)
            .map(|index| index + 1)
            .ok_or_else(|| invalid(format!("unknown sibling {after}"))),
    }
}

fn retitle(node: &mut AnchorNode, schema: &DocumentSchema) {
    retitle_node(node, schema);
}

fn retitle_node(node: &mut AnchorNode, schema: &DocumentSchema) {
    match node.kind {
        AnchorKind::Document => {
            node.title = schema.root.title.clone();
        }
        AnchorKind::Chapter => {
            if let Some(chapter) = schema.chapter(&node.schema_key)
                && chapter.occurrence == Occurrence::Single
            {
                node.title = chapter.title.clone();
            }
        }
        AnchorKind::Item => {
            node.title.clear();
        }
    }
    for child in &mut node.children {
        retitle_node(child, schema);
    }
    for item in &mut node.items {
        retitle_node(item, schema);
    }
}

fn validate_document(snapshot: &DocumentSnapshot, schema: &DocumentSchema) -> Validation {
    let mut validation = Validation::default();
    if snapshot.kind != schema.kind {
        validation.errors.push(ValidationIssue::error(
            "kind",
            "kind_mismatch",
            format!(
                "document kind {} does not match schema {}",
                snapshot.kind, schema.kind
            ),
        ));
    }
    if snapshot.schema_version != schema.schema_version {
        validation.errors.push(ValidationIssue::error(
            "schema_version",
            "schema_version_mismatch",
            format!(
                "document schema_version {} does not match schema {}",
                snapshot.schema_version, schema.schema_version
            ),
        ));
    }
    if snapshot.root.kind != AnchorKind::Document {
        validation.errors.push(ValidationIssue::error(
            "root.kind",
            "invalid_root_kind",
            "the root anchor must be a document",
        ));
    }
    let mut keys = HashSet::new();
    validate_node(
        &snapshot.root,
        Some(&schema.root),
        schema,
        &mut keys,
        &mut validation,
    );
    if let Ok(bytes) = serde_json::to_vec(snapshot)
        && bytes.len() > MAX_ANCHOR_DOCUMENT_BYTES
    {
        validation.errors.push(ValidationIssue::error(
            "document",
            "document_too_large",
            format!("document exceeds {MAX_ANCHOR_DOCUMENT_BYTES} bytes"),
        ));
    }
    validation
}

fn validate_node(
    node: &AnchorNode,
    expected: Option<&ChapterSchema>,
    schema: &DocumentSchema,
    keys: &mut HashSet<String>,
    validation: &mut Validation,
) {
    if !keys.insert(node.key.clone()) {
        validation.errors.push(ValidationIssue::error(
            format!("{}.key", node.key),
            "duplicate_key",
            format!("duplicate anchor key {}", node.key),
        ));
    }
    if node.id != node.hash() {
        validation.errors.push(ValidationIssue::error(
            format!("{}.id", node.key),
            "hash_mismatch",
            "anchor id does not match its content",
        ));
    }

    if node.kind == AnchorKind::Item {
        if !node.items.is_empty() || !node.children.is_empty() {
            validation.errors.push(ValidationIssue::error(
                format!("{}.children", node.key),
                "item_has_children",
                "list items are text-only",
            ));
        }
        if let Some(item) = schema.item(&node.schema_key) {
            if item.text_required && node.text.trim().is_empty() {
                validation.errors.push(ValidationIssue::error(
                    format!("{}.text", node.key),
                    "empty_text",
                    "list item text must not be empty",
                ));
            }
        } else {
            validation.errors.push(ValidationIssue::error(
                format!("{}.schema_key", node.key),
                "unknown_item_schema",
                format!("unknown item schema {}", node.schema_key),
            ));
        }
        return;
    }

    let Some(chapter) = expected else {
        validation.errors.push(ValidationIssue::error(
            format!("{}.schema_key", node.key),
            "unknown_schema",
            format!("unknown chapter schema {}", node.schema_key),
        ));
        return;
    };
    if node.kind == AnchorKind::Document && chapter.key != schema.root.key {
        validation.errors.push(ValidationIssue::error(
            "root.kind",
            "invalid_root",
            "the root anchor must use the root schema",
        ));
    }
    let expected_content = match chapter.content {
        ContentSchema::Text => ContentKind::Text,
        ContentSchema::List { .. } => ContentKind::List,
    };
    if node.content_kind != expected_content {
        validation.errors.push(ValidationIssue::error(
            format!("{}.content_kind", node.key),
            "content_kind_mismatch",
            "anchor content kind does not match its schema",
        ));
    }
    if node.content_kind == ContentKind::Text && !node.items.is_empty() {
        validation.errors.push(ValidationIssue::error(
            format!("{}.items", node.key),
            "text_has_items",
            "a text chapter cannot have list items",
        ));
    }
    if chapter.text_required && node.text.trim().is_empty() {
        validation.errors.push(ValidationIssue::error(
            format!("{}.text", node.key),
            "empty_text",
            "chapter text must not be empty",
        ));
    }
    if let ContentSchema::List { item, min, max } = &chapter.content {
        if node.items.len() < *min {
            validation.errors.push(ValidationIssue::error(
                format!("{}.items", node.key),
                "too_few_items",
                format!("expected at least {min} list items"),
            ));
        }
        if let Some(max) = max
            && node.items.len() > *max
        {
            validation.errors.push(ValidationIssue::error(
                format!("{}.items", node.key),
                "too_many_items",
                format!("expected at most {max} list items"),
            ));
        }
        for item_node in &node.items {
            if item_node.kind != AnchorKind::Item || item_node.schema_key != item.key {
                validation.errors.push(ValidationIssue::error(
                    format!("{}.items.{}", node.key, item_node.key),
                    "invalid_item",
                    format!("expected item schema {}", item.key),
                ));
            }
            validate_item(item_node, item, keys, validation);
        }
    }

    let mut counts: HashMap<&str, usize> = HashMap::new();
    for child in &node.children {
        *counts.entry(child.schema_key.as_str()).or_insert(0) += 1;
        let child_schema = chapter
            .children
            .iter()
            .find(|candidate| candidate.key == child.schema_key);
        validate_node(child, child_schema, schema, keys, validation);
    }
    for child_schema in &chapter.children {
        let count = counts.get(child_schema.key.as_str()).copied().unwrap_or(0);
        if child_schema.required && count == 0 {
            validation.errors.push(ValidationIssue::error(
                format!("{}.children.{}", node.key, child_schema.key),
                "missing_required",
                format!("required chapter {} is missing", child_schema.key),
            ));
        }
        match child_schema.occurrence {
            Occurrence::Single => {
                if count > 1 {
                    validation.errors.push(ValidationIssue::error(
                        format!("{}.children.{}", node.key, child_schema.key),
                        "too_many_occurrences",
                        format!("chapter {} may appear only once", child_schema.key),
                    ));
                }
            }
            Occurrence::Multiple => {
                if count < child_schema.min {
                    validation.errors.push(ValidationIssue::error(
                        format!("{}.children.{}", node.key, child_schema.key),
                        "too_few_occurrences",
                        format!(
                            "chapter {} needs at least {} occurrences",
                            child_schema.key, child_schema.min
                        ),
                    ));
                }
                if let Some(max) = child_schema.max
                    && count > max
                {
                    validation.errors.push(ValidationIssue::error(
                        format!("{}.children.{}", node.key, child_schema.key),
                        "too_many_occurrences",
                        format!(
                            "chapter {} allows at most {} occurrences",
                            child_schema.key, max
                        ),
                    ));
                }
            }
        }
    }
}

fn validate_item(
    node: &AnchorNode,
    item: &ItemSchema,
    keys: &mut HashSet<String>,
    validation: &mut Validation,
) {
    if !keys.insert(node.key.clone()) {
        validation.errors.push(ValidationIssue::error(
            format!("{}.key", node.key),
            "duplicate_key",
            format!("duplicate anchor key {}", node.key),
        ));
    }
    if node.kind != AnchorKind::Item {
        validation.errors.push(ValidationIssue::error(
            format!("{}.kind", node.key),
            "invalid_item_kind",
            "list content must contain item anchors",
        ));
    }
    if node.schema_key != item.key {
        validation.errors.push(ValidationIssue::error(
            format!("{}.schema_key", node.key),
            "invalid_item_schema",
            format!("expected item schema {}", item.key),
        ));
    }
    if !node.items.is_empty() || !node.children.is_empty() {
        validation.errors.push(ValidationIssue::error(
            format!("{}.children", node.key),
            "item_has_children",
            "list items are text-only",
        ));
    }
    if item.text_required && node.text.trim().is_empty() {
        validation.errors.push(ValidationIssue::error(
            format!("{}.text", node.key),
            "empty_text",
            "list item text must not be empty",
        ));
    }
    if node.id != node.hash() {
        validation.errors.push(ValidationIssue::error(
            format!("{}.id", node.key),
            "hash_mismatch",
            "anchor id does not match its content",
        ));
    }
}

/// The project document schema.
pub fn project_document_schema() -> DocumentSchema {
    fn chapter(
        key: &str,
        title: &str,
        occurrence: Occurrence,
        content: ContentSchema,
    ) -> ChapterSchema {
        ChapterSchema {
            key: key.to_string(),
            title: title.to_string(),
            occurrence,
            content,
            children: Vec::new(),
            required: false,
            min: 0,
            max: None,
            text_required: false,
        }
    }

    fn text(key: &str, title: &str) -> ChapterSchema {
        chapter(key, title, Occurrence::Single, ContentSchema::Text)
    }

    fn list(key: &str, title: &str, item_key: &str) -> ChapterSchema {
        chapter(
            key,
            title,
            Occurrence::Single,
            ContentSchema::List {
                item: ItemSchema {
                    key: item_key.to_string(),
                    text_required: true,
                },
                min: 0,
                max: None,
            },
        )
    }

    fn required_list(key: &str, title: &str, item_key: &str, min: usize) -> ChapterSchema {
        chapter(
            key,
            title,
            Occurrence::Single,
            ContentSchema::List {
                item: ItemSchema {
                    key: item_key.to_string(),
                    text_required: true,
                },
                min,
                max: None,
            },
        )
        .required(true)
    }

    DocumentSchema {
        kind: "project".to_string(),
        schema_version: ANCHOR_DOCUMENT_SCHEMA_VERSION,
        root: chapter(
            "document",
            "项目文档",
            Occurrence::Single,
            ContentSchema::Text,
        )
        .children(vec![
            text("positioning", "定位").required(true).children(vec![
                text("what_is", "是什么").required(true).text_required(true),
                list("what_is_not", "不是什么", "exclusion").required(true),
            ]),
            text("background", "背景").required(true).children(vec![
                text("context", "现状").required(true).text_required(true),
                required_list("problems", "问题", "problem", 1),
            ]),
            required_list("goals", "目标", "goal", 1),
            list("non_goals", "非目标", "non_goal"),
            list("phasing_plan", "分期计划", "phase"),
            list("constraints", "约束", "constraint"),
            list("risks", "风险", "risk"),
            list("glossary", "术语表", "term"),
        ]),
    }
}

/// The requirements document uses the same anchor engine as the project
/// document. Users live under 用户; each subsystem owns function sets, and
/// each function set owns stories and functions.
pub fn requirements_document_schema() -> DocumentSchema {
    fn chapter(
        key: &str,
        title: &str,
        occurrence: Occurrence,
        content: ContentSchema,
    ) -> ChapterSchema {
        ChapterSchema {
            key: key.to_string(),
            title: title.to_string(),
            occurrence,
            content,
            children: Vec::new(),
            required: false,
            min: 0,
            max: None,
            text_required: false,
        }
    }

    fn text(key: &str, title: &str) -> ChapterSchema {
        chapter(key, title, Occurrence::Single, ContentSchema::Text)
    }

    fn multiple(key: &str, title: &str) -> ChapterSchema {
        chapter(key, title, Occurrence::Multiple, ContentSchema::Text)
    }

    let user = multiple("user", "用户").text_required(true);

    let story = multiple("story", "故事").text_required(true);

    let ui_image = multiple("ui_image", "图片").text_required(true);

    let function = multiple("function", "功能")
        .text_required(true)
        .children(vec![ui_image]);

    let function_set = multiple("function_set", "功能集")
        .text_required(true)
        .children(vec![
            text("stories", "故事").required(true).children(vec![story]),
            text("functions", "功能")
                .required(true)
                .children(vec![function]),
        ]);

    let subsystem = multiple("subsystem", "子系统")
        .text_required(true)
        .children(vec![function_set]);

    DocumentSchema {
        kind: "requirements".to_string(),
        schema_version: ANCHOR_DOCUMENT_SCHEMA_VERSION,
        root: text("document", "需求文档").children(vec![
            text("users", "用户").required(true).children(vec![user]),
            text("subsystems", "子系统")
                .required(true)
                .children(vec![subsystem]),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(
        snapshot: &DocumentSnapshot,
        schema: &DocumentSchema,
        parent: &str,
        occurrence_key: Option<&str>,
        text: &str,
    ) -> DocumentSnapshot {
        add_with_title(snapshot, schema, parent, occurrence_key, text, None)
    }

    fn add_titled(
        snapshot: &DocumentSnapshot,
        schema: &DocumentSchema,
        parent: &str,
        occurrence_key: Option<&str>,
        text: &str,
        title: &str,
    ) -> DocumentSnapshot {
        add_with_title(snapshot, schema, parent, occurrence_key, text, Some(title))
    }

    fn add_with_title(
        snapshot: &DocumentSnapshot,
        schema: &DocumentSchema,
        parent: &str,
        occurrence_key: Option<&str>,
        text: &str,
        title: Option<&str>,
    ) -> DocumentSnapshot {
        let (next, _) = snapshot
            .apply(
                schema,
                &AnchorOp::Add {
                    parent: parent.to_string(),
                    occurrence_key: occurrence_key.map(str::to_string),
                    text: text.to_string(),
                    title: title.map(str::to_string),
                    after: None,
                },
            )
            .unwrap();
        next
    }

    fn update(
        snapshot: &DocumentSnapshot,
        schema: &DocumentSchema,
        anchor: &str,
        text: &str,
    ) -> DocumentSnapshot {
        let (next, _) = snapshot
            .apply(
                schema,
                &AnchorOp::UpdateText {
                    anchor: anchor.to_string(),
                    text: text.to_string(),
                },
            )
            .unwrap();
        next
    }

    fn sample(schema: &DocumentSchema) -> DocumentSnapshot {
        let mut snapshot = DocumentSnapshot::empty(schema);
        snapshot = update(&snapshot, schema, "positioning.what_is", "企业费用报销系统");
        snapshot = add(
            &snapshot,
            schema,
            "positioning.what_is_not",
            Some("e1"),
            "财务总账系统",
        );
        snapshot = update(
            &snapshot,
            schema,
            "background.context",
            "报销目前由员工填写纸质单。",
        );
        snapshot = add(
            &snapshot,
            schema,
            "background.problems",
            Some("p1"),
            "员工无法查询报销进度，平均催问两次",
        );
        snapshot = add(
            &snapshot,
            schema,
            "goals",
            Some("g1"),
            "员工可自助查询报销进度",
        );
        snapshot = add(&snapshot, schema, "non_goals", Some("n1"), "不做移动端应用");
        snapshot
    }

    #[test]
    fn project_schema_is_valid() {
        let schema = project_document_schema();
        let validation = schema.validate();
        assert!(validation.is_valid(), "{:?}", validation.errors);
    }

    #[test]
    fn project_schema_has_a_phasing_plan_list() {
        let schema = project_document_schema();
        let phasing = schema
            .root
            .children
            .iter()
            .find(|child| child.key == "phasing_plan")
            .expect("phasing plan chapter");
        assert_eq!(phasing.title, "分期计划");
        assert!(matches!(phasing.content, ContentSchema::List { .. }));
    }

    #[test]
    fn requirements_schema_is_valid() {
        let schema = requirements_document_schema();
        let validation = schema.validate();
        assert!(validation.is_valid(), "{:?}", validation.errors);
        let snapshot = DocumentSnapshot::empty(&schema);
        assert!(snapshot.find_path("users").is_some());
        assert!(snapshot.find_path("subsystems").is_some());
    }

    #[test]
    fn user_title_is_the_added_title() {
        let schema = requirements_document_schema();
        let mut snapshot = DocumentSnapshot::empty(&schema);
        let text = "姓名：陈铭\n年龄：34 岁\n基本状况：城市白领\n爱好：徒步\n职业：产品经理\n典型场景：在通勤路上规划周末采购\n诉求：快速找到符合预算的商品\n主次：主要";
        snapshot = add_titled(&snapshot, &schema, "users", Some("u1"), text, "陈铭");
        let markdown = snapshot.markdown(&snapshot.root.key).unwrap();
        assert!(markdown.contains("## 陈铭"));
        assert!(!markdown.contains("用户 1"));
        assert!(markdown.contains("姓名：陈铭"));
        assert!(markdown.contains("年龄：34 岁"));
    }

    #[test]
    fn add_requires_a_title_for_multiple_chapters() {
        let schema = requirements_document_schema();
        let snapshot = DocumentSnapshot::empty(&schema);
        let error = snapshot
            .apply(
                &schema,
                &AnchorOp::Add {
                    parent: "users".to_string(),
                    occurrence_key: Some("u1".to_string()),
                    text: "姓名：陈铭".to_string(),
                    title: None,
                    after: None,
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("requires a title"));
    }

    #[test]
    fn update_title_changes_only_multiple_chapters() {
        let schema = requirements_document_schema();
        let mut snapshot = DocumentSnapshot::empty(&schema);
        snapshot = add_titled(
            &snapshot,
            &schema,
            "users",
            Some("u1"),
            "姓名：陈铭\n年龄：34 岁",
            "陈铭",
        );
        snapshot = update(
            &snapshot,
            &schema,
            "users/user/u1",
            "姓名：李明\n年龄：34 岁",
        );
        assert_eq!(snapshot.find_path("users/user/u1").unwrap().title, "陈铭");
        let (snapshot, _) = snapshot
            .apply(
                &schema,
                &AnchorOp::UpdateTitle {
                    anchor: "users/user/u1".to_string(),
                    title: "李明".to_string(),
                },
            )
            .unwrap();
        assert_eq!(snapshot.find_path("users/user/u1").unwrap().title, "李明");
        assert!(
            snapshot
                .markdown(&snapshot.root.key)
                .unwrap()
                .contains("## 李明")
        );
        let error = snapshot
            .apply(
                &schema,
                &AnchorOp::UpdateTitle {
                    anchor: "users".to_string(),
                    title: "所有人".to_string(),
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("only multiple"));
        let error = snapshot
            .apply(
                &schema,
                &AnchorOp::UpdateTitle {
                    anchor: "users/user/u1".to_string(),
                    title: "  ".to_string(),
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("empty"));
    }

    #[test]
    fn requirements_user_is_a_single_text_item() {
        let schema = requirements_document_schema();
        let users = schema
            .root
            .children
            .iter()
            .find(|child| child.key == "users")
            .expect("users chapter");
        let user = users
            .children
            .iter()
            .find(|child| child.key == "user")
            .expect("user item");
        assert!(user.children.is_empty());
        assert!(user.text_required);
        assert_eq!(user.occurrence, Occurrence::Multiple);
        assert_eq!(user.content, ContentSchema::Text);
    }

    #[test]
    fn requirements_sections_keep_fields_in_item_text() {
        let schema = requirements_document_schema();
        fn keys(chapter: &ChapterSchema) -> Vec<&str> {
            chapter
                .children
                .iter()
                .map(|child| child.key.as_str())
                .collect()
        }
        let subsystem = schema
            .root
            .children
            .iter()
            .find(|child| child.key == "subsystems")
            .and_then(|subsystems| subsystems.children.iter().find(|c| c.key == "subsystem"))
            .expect("subsystem item");
        assert_eq!(keys(subsystem), ["function_set"]);
        assert!(subsystem.text_required);

        let function_set = subsystem
            .children
            .iter()
            .find(|child| child.key == "function_set")
            .expect("function set item");
        assert_eq!(keys(function_set), ["stories", "functions"]);
        assert!(function_set.text_required);

        let story = function_set
            .children
            .iter()
            .find(|child| child.key == "stories")
            .and_then(|stories| stories.children.iter().find(|child| child.key == "story"))
            .expect("story item");
        assert!(story.children.is_empty());
        assert!(story.text_required);

        let function = function_set
            .children
            .iter()
            .find(|child| child.key == "functions")
            .and_then(|functions| {
                functions
                    .children
                    .iter()
                    .find(|child| child.key == "function")
            })
            .expect("function item");
        assert_eq!(keys(function), ["ui_image"]);
        assert!(function.text_required);

        let image = function
            .children
            .iter()
            .find(|child| child.key == "ui_image")
            .expect("ui image item");
        assert!(image.children.is_empty());
        assert!(image.text_required);
    }

    #[test]
    fn subsystem_title_is_the_added_title() {
        let schema = requirements_document_schema();
        let mut snapshot = DocumentSnapshot::empty(&schema);
        let text = "软件名称：订单系统移动端\n软件形态：移动应用\n覆盖用户：买家\n闭环目标：买家查看订单状态\n与其他子系统的关系：无\n边界：不含下单和支付";
        snapshot = add_titled(
            &snapshot,
            &schema,
            "subsystems",
            Some("orders"),
            text,
            "订单系统移动端",
        );
        let markdown = snapshot.markdown(&snapshot.root.key).unwrap();
        assert!(markdown.contains("## 订单系统移动端"));
        assert!(markdown.contains("软件形态：移动应用"));
        assert!(markdown.contains("边界：不含下单和支付"));
    }

    #[test]
    fn rejects_multiple_chapter_with_siblings() {
        let mut schema = project_document_schema();
        let mut extra = schema.root.children[0].clone();
        extra.key = "extra".to_string();
        extra.occurrence = Occurrence::Multiple;
        schema.root.children.push(extra);
        let validation = schema.validate();
        assert!(
            validation
                .errors
                .iter()
                .any(|issue| issue.code == "multiple_child_not_alone"),
            "{:?}",
            validation.errors
        );
    }

    #[test]
    fn infers_child_type_from_parent() {
        let schema = project_document_schema();
        let snapshot = DocumentSnapshot::empty(&schema);
        let (next, created) = snapshot
            .apply(
                &schema,
                &AnchorOp::Add {
                    parent: "background.problems".to_string(),
                    occurrence_key: Some("p1".to_string()),
                    text: "员工无法查询报销进度".to_string(),
                    title: None,
                    after: None,
                },
            )
            .unwrap();
        assert!(created.is_some());
        assert!(next.find_path("background.problems/p1").is_some());
        let (next, created) = next
            .apply(
                &schema,
                &AnchorOp::Add {
                    parent: "positioning.what_is_not".to_string(),
                    occurrence_key: Some("e1".to_string()),
                    text: "财务总账系统".to_string(),
                    title: None,
                    after: None,
                },
            )
            .unwrap();
        assert!(created.is_some());
        assert!(next.find_path("positioning.what_is_not/e1").is_some());
        let error = next
            .apply(
                &schema,
                &AnchorOp::Add {
                    parent: "document".to_string(),
                    occurrence_key: None,
                    text: "ambiguous".to_string(),
                    title: None,
                    after: None,
                },
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("must have exactly one child schema")
        );
    }

    #[test]
    fn precreates_single_chapters_but_not_multiple_instances() {
        let schema = project_document_schema();
        let snapshot = DocumentSnapshot::empty(&schema);
        assert!(snapshot.find_path("positioning").is_some());
        assert!(snapshot.find_path("positioning.what_is").is_some());
        assert!(snapshot.find_path("positioning.what_is_not").is_some());
        assert!(snapshot.find_path("background").is_some());
        assert!(snapshot.find_path("background.context").is_some());
        assert!(snapshot.find_path("background.problems").is_some());
        assert!(snapshot.find_path("goals").is_some());
        assert!(snapshot.find_path("background.problems/p1").is_none());
    }

    #[test]
    fn builds_a_valid_document_and_renders_markdown() {
        let schema = project_document_schema();
        let snapshot = sample(&schema);
        let validation = snapshot.validate(&schema);
        assert!(validation.is_valid(), "{:?}", validation.errors);
        assert!(!snapshot.root_id().is_empty());
        assert!(snapshot.find_path("goals/g1").is_some());
        let markdown = snapshot.markdown(&snapshot.root.key).unwrap();
        assert!(markdown.contains("# 定位"));
        assert!(markdown.contains("# 目标"));
        assert!(markdown.contains("- 员工可自助查询报销进度"));
    }

    #[test]
    fn flat_list_items_render_as_bullets_not_titles() {
        let schema = project_document_schema();
        let snapshot = sample(&schema);
        let markdown = snapshot.markdown(&snapshot.root.key).unwrap();
        assert!(markdown.contains("- 不做移动端应用"));
        assert!(!markdown.contains("非目标 1"));
    }

    #[test]
    fn markdown_with_anchors_marks_chapters_and_items() {
        let schema = project_document_schema();
        let snapshot = sample(&schema);
        let markdown = snapshot
            .markdown_with_anchors(&snapshot.root.key)
            .expect("anchored markdown");
        assert!(markdown.contains("<!--anchor:"));
        assert!(markdown.contains("-->\n# 定位"));
        assert!(markdown.contains("- <!--anchor:"));
        assert!(
            !snapshot
                .markdown(&snapshot.root.key)
                .unwrap()
                .contains("<!--anchor:")
        );
    }

    #[test]
    fn update_changes_hashes_but_keeps_keys() {
        let schema = project_document_schema();
        let snapshot = sample(&schema);
        let old_root = snapshot.root_id().to_string();
        let old_goal = snapshot.find_path("goals/g1").unwrap().id.clone();
        let (next, _) = snapshot
            .apply(
                &schema,
                &AnchorOp::UpdateText {
                    anchor: "goals/g1".to_string(),
                    text: "员工可自助提交并查询报销进度".to_string(),
                },
            )
            .unwrap();
        assert_ne!(old_root, next.root_id());
        assert_ne!(old_goal, next.find_path("goals/g1").unwrap().id);
        assert!(next.find_path("goals/g1").is_some());
        assert!(next.validate(&schema).is_valid());
    }

    #[test]
    fn list_items_are_text_only() {
        let schema = project_document_schema();
        let snapshot = sample(&schema);
        let error = snapshot
            .apply(
                &schema,
                &AnchorOp::Add {
                    parent: "positioning.what_is_not/e1".to_string(),
                    occurrence_key: Some("e2".to_string()),
                    text: "another".to_string(),
                    title: None,
                    after: None,
                },
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("list items cannot have children")
        );
    }

    #[test]
    fn single_chapters_cannot_be_deleted() {
        let schema = project_document_schema();
        let snapshot = sample(&schema);
        for anchor in ["positioning.what_is", "background.context", "goals"] {
            let error = snapshot
                .apply(
                    &schema,
                    &AnchorOp::Delete {
                        anchor: anchor.to_string(),
                        cascade: true,
                    },
                )
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("single chapters cannot be deleted"),
                "{anchor}: {error}"
            );
        }
    }

    #[test]
    fn delete_requires_cascade_for_chapters_with_children() {
        let schema = requirements_document_schema();
        let snapshot = DocumentSnapshot::empty(&schema);
        let snapshot = add_titled(
            &snapshot,
            &schema,
            "subsystems",
            Some("s1"),
            "软件名称：订单系统\n软件形态：移动应用",
            "订单系统",
        );
        let snapshot = add_titled(
            &snapshot,
            &schema,
            "subsystems/subsystem/s1",
            Some("f1"),
            "名称：订单功能集",
            "订单功能集",
        );
        let error = snapshot
            .apply(
                &schema,
                &AnchorOp::Delete {
                    anchor: "subsystems/subsystem/s1/function_set/f1".to_string(),
                    cascade: false,
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("cascade"));
        let (next, _) = snapshot
            .apply(
                &schema,
                &AnchorOp::Delete {
                    anchor: "subsystems/subsystem/s1/function_set/f1".to_string(),
                    cascade: true,
                },
            )
            .unwrap();
        assert!(
            next.find_path("subsystems/subsystem/s1/function_set/f1")
                .is_none()
        );
    }

    #[test]
    fn move_reorders_list_items() {
        let schema = project_document_schema();
        let mut snapshot = sample(&schema);
        snapshot = add(
            &snapshot,
            &schema,
            "positioning.what_is_not",
            Some("e2"),
            "差旅预订平台",
        );
        let (next, _) = snapshot
            .apply(
                &schema,
                &AnchorOp::Move {
                    anchor: "positioning.what_is_not/e2".to_string(),
                    after: Some("positioning.what_is_not/e1".to_string()),
                },
            )
            .unwrap();
        let items = &next.find_path("positioning.what_is_not").unwrap().items;
        assert_eq!(items[0].path, "positioning.what_is_not/e1");
        assert_eq!(items[1].path, "positioning.what_is_not/e2");
    }
}
