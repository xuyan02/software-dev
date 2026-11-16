//! Design-object graph loading and View rebuilding.
//!
//! The content-addressed store can hash one object at a time, but editing a
//! child changes every parent that contains it. This module reconstructs the
//! containment graph from a View, lets callers mutate a qualified-name
//! catalog, and rebuilds the full View bottom-up.

use crate::format::{object_hash, symbol_hash, validate_object};
use crate::model::*;
use crate::store::{Store, StoreError};
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("format: {0}")]
    Format(#[from] crate::format::FormatError),
    #[error("View does not contain kind=view")]
    NotAView,
    #[error("duplicate qualified name: {0}")]
    DuplicateName(String),
    #[error("object qualified_name does not match catalog key")]
    NameMismatch,
    #[error("missing parent: {0}")]
    MissingParent(String),
    #[error("object is not reachable from a root: {0}")]
    Orphan(String),
    #[error("parent graph contains a cycle: {0}")]
    Cycle(String),
    #[error("object kind {0:?} cannot contain children")]
    NotAContainer(Kind),
    #[error("root files is missing or invalid")]
    MissingFilesRoot,
    #[error("root namespaces is missing or invalid")]
    MissingNamespacesRoot,
    #[error("object {0} has an invalid parent reference")]
    InvalidParent(String),
    #[error("object {0} is used as a parent but cannot contain children")]
    InvalidParentKind(String),
    #[error("object {object} has invalid parent: {message}")]
    InvalidParentDetail { object: String, message: String },
    #[error("object {object} references missing object {target}")]
    MissingReference { object: String, target: String },
    #[error("object {object} has relation target {target} of kind {actual:?}; expected {expected}")]
    InvalidRelationKind {
        object: String,
        target: String,
        actual: Kind,
        expected: &'static str,
    },
    #[error("relation graph contains a cycle: {0}")]
    RelationCycle(String),
    #[error("method {method} has invalid overrides: {message}")]
    InvalidOverride { method: String, message: String },
    #[error("object {object} has invalid dependency: {message}")]
    InvalidDependency { object: String, message: String },
}

pub type Result<T> = std::result::Result<T, CatalogError>;

/// A mutable catalog keyed by qualified-name symbol.
///
/// Qualified names must be unique. Part display names are allowed to collide
/// in the wire format, but a graph cannot resolve two different parent objects
/// with the same name, so the graph builder rejects duplicates.
#[derive(Debug, Clone)]
pub struct Catalog {
    source_commit: GitOid,
    entries: BTreeMap<SymbolHash, DesignObject>,
}

impl Catalog {
    pub fn new(source_commit: GitOid) -> Self {
        Self {
            source_commit,
            entries: BTreeMap::new(),
        }
    }

    pub fn source_commit(&self) -> GitOid {
        self.source_commit
    }

    pub fn set_source_commit(&mut self, source_commit: GitOid) {
        self.source_commit = source_commit;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, qname: SymbolHash) -> Option<&DesignObject> {
        self.entries.get(&qname)
    }

    pub fn get_mut(&mut self, qname: SymbolHash) -> Option<&mut DesignObject> {
        self.entries.get_mut(&qname)
    }

    pub fn iter(&self) -> impl Iterator<Item = (SymbolHash, &DesignObject)> {
        self.entries.iter().map(|(name, object)| (*name, object))
    }

    pub fn insert(&mut self, qname: SymbolHash, object: DesignObject) -> Result<()> {
        if object_name(&object) != Some(qname) {
            return Err(CatalogError::NameMismatch);
        }
        if self.entries.contains_key(&qname) {
            return Err(CatalogError::DuplicateName(normalize_name(qname)));
        }
        self.entries.insert(qname, object);
        Ok(())
    }

    pub fn replace(&mut self, qname: SymbolHash, object: DesignObject) -> Result<()> {
        if object_name(&object) != Some(qname) {
            return Err(CatalogError::NameMismatch);
        }
        self.entries.insert(qname, object);
        Ok(())
    }

    pub fn remove(&mut self, qname: SymbolHash) -> Option<DesignObject> {
        self.entries.remove(&qname)
    }

    /// Direct containment parents of one object.
    pub fn parents(&self, qname: SymbolHash) -> Vec<SymbolHash> {
        self.entries.get(&qname).map(parents_of).unwrap_or_default()
    }

    /// Direct containment children of one object.
    pub fn children(&self, qname: SymbolHash) -> Vec<SymbolHash> {
        let mut children = self
            .entries
            .iter()
            .filter(|(_, object)| parents_of(object).contains(&qname))
            .map(|(name, _)| *name)
            .collect::<Vec<_>>();
        children.sort();
        children
    }

    /// Objects that refer to `qname` through containment, relations, or
    /// dependencies.
    pub fn references_to(&self, qname: SymbolHash) -> Vec<SymbolHash> {
        let mut references = self
            .entries
            .iter()
            .filter(|(name, object)| {
                **name != qname
                    && (parents_of(object).contains(&qname)
                        || relation_targets(object).contains(&qname)
                        || relation_override(object) == Some(qname)
                        || dependency_lists(object).is_some_and(|(explicit, implicit)| {
                            explicit.iter().any(|dep| dep.target == qname)
                                || implicit.iter().any(|dep| dep.target == qname)
                        }))
            })
            .map(|(name, _)| *name)
            .collect::<Vec<_>>();
        references.sort();
        references
    }

    /// Rewrite qualified names and references according to a rename mapping.
    pub fn rename_symbols(&mut self, mapping: &BTreeMap<SymbolHash, SymbolHash>) -> Result<()> {
        if mapping.is_empty() {
            return Ok(());
        }
        let mut entries = BTreeMap::new();
        for (qname, object) in &self.entries {
            let mut object = object.clone();
            rewrite_object(&mut object, mapping);
            let new_qname = mapping.get(qname).copied().unwrap_or(*qname);
            if object_name(&object) != Some(new_qname) {
                return Err(CatalogError::NameMismatch);
            }
            if entries.insert(new_qname, object).is_some() {
                return Err(CatalogError::DuplicateName(normalize_name(new_qname)));
            }
        }
        self.entries = entries;
        Ok(())
    }

    /// Load every object reachable from a View root.
    pub fn load(store: &Store, view: ObjectHash) -> Result<Self> {
        if view.is_zero() {
            return Err(CatalogError::NotAView);
        }
        let view_object = store.get_object(view)?;
        let ObjectPayload::View(view_payload) = &view_object.payload else {
            return Err(CatalogError::NotAView);
        };
        let mut catalog = Self::new(view_payload.source_commit);
        let mut seen = HashSet::new();
        visit(store, view_payload.files, &mut catalog, &mut seen)?;
        visit(store, view_payload.namespaces, &mut catalog, &mut seen)?;
        if !view_payload.design.is_zero() {
            visit(store, view_payload.design, &mut catalog, &mut seen)?;
        }
        Ok(catalog)
    }

    /// Rebuild all object hashes and the View root.
    ///
    /// The catalog is used as the source of truth. Parent/child containers are
    /// reconstructed from each object's parent fields; the caller is not
    /// allowed to supply children directly.
    pub fn rebuild(&self, store: &Store) -> Result<ObjectHash> {
        let mut edges: BTreeMap<SymbolHash, Vec<SymbolHash>> = BTreeMap::new();
        let mut rooted = BTreeSet::new();
        let files = symbol_hash("files");
        let namespaces = symbol_hash("namespaces");

        for (qname, object) in &self.entries {
            if object_name(object) != Some(*qname) {
                return Err(CatalogError::NameMismatch);
            }
            validate_object(object)?;
            let parents = parents_of(object);
            if parents.is_empty() {
                if has_view_parent(object) {
                    if *qname != files && *qname != namespaces {
                        return Err(CatalogError::InvalidParent(normalize_name(*qname)));
                    }
                    rooted.insert(*qname);
                } else if *qname != files && *qname != namespaces {
                    return Err(CatalogError::Orphan(normalize_name(*qname)));
                }
            }
            for parent in parents {
                if !self.entries.contains_key(&parent) {
                    return Err(CatalogError::MissingParent(normalize_name(parent)));
                }
                let parent_object = &self.entries[&parent];
                if !can_contain(parent_object) {
                    return Err(CatalogError::InvalidParentKind(normalize_name(parent)));
                }
                edges.entry(parent).or_default().push(*qname);
            }
        }

        let files_object = self
            .entries
            .get(&files)
            .ok_or(CatalogError::MissingFilesRoot)?;
        if files_object.kind != Kind::Directory || !has_view_parent(files_object) {
            return Err(CatalogError::MissingFilesRoot);
        }
        let namespaces_object = self
            .entries
            .get(&namespaces)
            .ok_or(CatalogError::MissingNamespacesRoot)?;
        if namespaces_object.kind != Kind::Namespace || !has_view_parent(namespaces_object) {
            return Err(CatalogError::MissingNamespacesRoot);
        }
        if rooted != BTreeSet::from([files, namespaces]) {
            return Err(CatalogError::InvalidParent("root set".into()));
        }

        // Each object must be reachable from one of the two roots. The edge
        // construction above already rejects orphans; this catches entries
        // that are only connected to other orphans.
        let mut reachable = BTreeSet::new();
        let mut queue = vec![files, namespaces];
        while let Some(qname) = queue.pop() {
            if !reachable.insert(qname) {
                continue;
            }
            if let Some(children) = edges.get(&qname) {
                queue.extend(children.iter().copied());
            }
        }
        for qname in self.entries.keys() {
            if !reachable.contains(qname) {
                return Err(CatalogError::Orphan(normalize_name(*qname)));
            }
        }

        validate_semantics(store, &self.entries)?;

        let mut memo = BTreeMap::new();
        let mut visiting = BTreeSet::new();
        let files_hash = compute(
            store,
            &self.entries,
            &edges,
            files,
            &mut memo,
            &mut visiting,
        )?;
        let namespaces_hash = compute(
            store,
            &self.entries,
            &edges,
            namespaces,
            &mut memo,
            &mut visiting,
        )?;

        let view = DesignObject::new(
            Kind::View,
            ObjectPayload::View(ViewPayload {
                source_commit: self.source_commit,
                files: files_hash,
                namespaces: namespaces_hash,
                design: ObjectHash::ZERO,
            }),
        );
        let view_hash = object_hash(&view)?;
        store.put_object(&view)?;
        Ok(view_hash)
    }
}

fn visit(
    store: &Store,
    hash: ObjectHash,
    catalog: &mut Catalog,
    seen: &mut HashSet<ObjectHash>,
) -> Result<()> {
    if !seen.insert(hash) {
        return Ok(());
    }
    let object = store.get_object(hash)?;
    if let Some(qname) = object_name(&object) {
        if catalog.entries.contains_key(&qname) {
            return Err(CatalogError::DuplicateName(normalize_name(qname)));
        }
        catalog.entries.insert(qname, object.clone());
    }
    for child in children_of(&object) {
        visit(store, *child, catalog, seen)?;
    }
    Ok(())
}

fn compute(
    store: &Store,
    entries: &BTreeMap<SymbolHash, DesignObject>,
    edges: &BTreeMap<SymbolHash, Vec<SymbolHash>>,
    qname: SymbolHash,
    memo: &mut BTreeMap<SymbolHash, ObjectHash>,
    visiting: &mut BTreeSet<SymbolHash>,
) -> Result<ObjectHash> {
    if let Some(hash) = memo.get(&qname) {
        return Ok(*hash);
    }
    if !visiting.insert(qname) {
        return Err(CatalogError::Cycle(normalize_name(qname)));
    }

    let object = entries
        .get(&qname)
        .ok_or_else(|| CatalogError::MissingParent(normalize_name(qname)))?;
    let mut child_hashes = Vec::new();
    if let Some(children) = edges.get(&qname) {
        for child in children {
            child_hashes.push(compute(store, entries, edges, *child, memo, visiting)?);
        }
    }
    child_hashes.sort();
    let mut object = object.clone();
    set_children(&mut object, child_hashes)?;
    let hash = object_hash(&object)?;
    store.put_object(&object)?;
    visiting.remove(&qname);
    memo.insert(qname, hash);
    Ok(hash)
}

fn object_name(object: &DesignObject) -> Option<SymbolHash> {
    match &object.payload {
        ObjectPayload::View(_) => None,
        ObjectPayload::Directory(p) => Some(p.qualified_name),
        ObjectPayload::File(p) => Some(p.qualified_name),
        ObjectPayload::Namespace(p) => Some(p.qualified_name),
        ObjectPayload::Class(p) => Some(p.qualified_name),
        ObjectPayload::Interface(p) => Some(p.qualified_name),
        ObjectPayload::Method(p) => Some(p.qualified_name),
        ObjectPayload::Function(p) => Some(p.qualified_name),
        ObjectPayload::Part(p) => Some(p.qualified_name),
        ObjectPayload::Mixin(p) => Some(p.qualified_name),
    }
}

fn children_of(object: &DesignObject) -> &[ObjectHash] {
    match &object.payload {
        ObjectPayload::Directory(p) => &p.children,
        ObjectPayload::File(p) => &p.children,
        ObjectPayload::Namespace(p) => &p.children,
        ObjectPayload::Class(p) => &p.children,
        ObjectPayload::Interface(p) => &p.children,
        ObjectPayload::Part(p) => &p.children,
        ObjectPayload::Mixin(p) => &p.children,
        ObjectPayload::View(_) | ObjectPayload::Method(_) | ObjectPayload::Function(_) => &[],
    }
}

fn parents_of(object: &DesignObject) -> Vec<SymbolHash> {
    let mut out = Vec::new();
    match &object.payload {
        ObjectPayload::Directory(p) => {
            if let ParentRef::Object(parent) = p.parent {
                out.push(parent);
            }
        }
        ObjectPayload::File(p) => out.push(p.parent),
        ObjectPayload::Namespace(p) => {
            if let ParentRef::Object(parent) = p.parent {
                out.push(parent);
            }
        }
        ObjectPayload::Class(p) => push_class_parents(&p.parent, &mut out),
        ObjectPayload::Interface(p) => push_class_parents(&p.parent, &mut out),
        ObjectPayload::Method(p) => out.push(p.parent),
        ObjectPayload::Function(p) => {
            push_nonzero(p.file, &mut out);
            push_nonzero(p.namespace, &mut out);
            push_nonzero(p.module, &mut out);
        }
        ObjectPayload::Part(p) => out.push(p.parent),
        ObjectPayload::Mixin(p) => push_class_parents(&p.parent, &mut out),
        ObjectPayload::View(_) => {}
    }
    out.retain(|hash| !hash.is_zero());
    out.sort();
    out.dedup();
    out
}

fn push_class_parents(parent: &ClassParent, out: &mut Vec<SymbolHash>) {
    match parent {
        ClassParent::TopLevel {
            file,
            namespace,
            module,
        } => {
            push_nonzero(*file, out);
            push_nonzero(*namespace, out);
            push_nonzero(*module, out);
        }
        ClassParent::Nested { parent } => push_nonzero(*parent, out),
    }
}

fn push_nonzero(hash: SymbolHash, out: &mut Vec<SymbolHash>) {
    if !hash.is_zero() {
        out.push(hash);
    }
}

fn has_view_parent(object: &DesignObject) -> bool {
    match &object.payload {
        ObjectPayload::Directory(p) => matches!(p.parent, ParentRef::View),
        ObjectPayload::Namespace(p) => matches!(p.parent, ParentRef::View),
        _ => false,
    }
}

fn can_contain(object: &DesignObject) -> bool {
    matches!(
        object.kind,
        Kind::Directory
            | Kind::File
            | Kind::Namespace
            | Kind::Class
            | Kind::Interface
            | Kind::Part
            | Kind::Mixin
    )
}

fn set_children(object: &mut DesignObject, children: Vec<ObjectHash>) -> Result<()> {
    match &mut object.payload {
        ObjectPayload::Directory(p) => p.children = children,
        ObjectPayload::File(p) => p.children = children,
        ObjectPayload::Namespace(p) => p.children = children,
        ObjectPayload::Class(p) => p.children = children,
        ObjectPayload::Interface(p) => p.children = children,
        ObjectPayload::Part(p) => p.children = children,
        ObjectPayload::Mixin(p) => p.children = children,
        ObjectPayload::View(_) => return Err(CatalogError::NotAContainer(Kind::View)),
        ObjectPayload::Method(_) => {
            if !children.is_empty() {
                return Err(CatalogError::NotAContainer(Kind::Method));
            }
        }
        ObjectPayload::Function(_) => {
            if !children.is_empty() {
                return Err(CatalogError::NotAContainer(Kind::Function));
            }
        }
    }
    Ok(())
}

fn normalize_name(hash: SymbolHash) -> String {
    hash.to_hex()
}

fn object_label(store: &Store, qname: SymbolHash) -> String {
    store.get_symbol(qname).unwrap_or_else(|_| qname.to_hex())
}

fn rewrite_object(object: &mut DesignObject, mapping: &BTreeMap<SymbolHash, SymbolHash>) {
    let map = |hash: SymbolHash| mapping.get(&hash).copied().unwrap_or(hash);
    match &mut object.payload {
        ObjectPayload::View(_) => {}
        ObjectPayload::Directory(p) => {
            p.qualified_name = map(p.qualified_name);
            if let ParentRef::Object(parent) = p.parent {
                p.parent = ParentRef::Object(map(parent));
            }
        }
        ObjectPayload::File(p) => {
            p.qualified_name = map(p.qualified_name);
            p.parent = map(p.parent);
        }
        ObjectPayload::Namespace(p) => {
            p.qualified_name = map(p.qualified_name);
            if let ParentRef::Object(parent) = p.parent {
                p.parent = ParentRef::Object(map(parent));
            }
        }
        ObjectPayload::Class(p) => {
            p.qualified_name = map(p.qualified_name);
            rewrite_class_parent(&mut p.parent, mapping);
            p.superclass = map(p.superclass);
            rewrite_symbols(&mut p.interfaces, mapping);
            rewrite_symbols(&mut p.mixins, mapping);
            rewrite_dependencies(&mut p.explicit_dependencies, mapping);
            rewrite_dependencies(&mut p.implicit_dependencies, mapping);
        }
        ObjectPayload::Interface(p) => {
            p.qualified_name = map(p.qualified_name);
            rewrite_class_parent(&mut p.parent, mapping);
            p.extends = map(p.extends);
            rewrite_dependencies(&mut p.explicit_dependencies, mapping);
            rewrite_dependencies(&mut p.implicit_dependencies, mapping);
        }
        ObjectPayload::Method(p) => {
            p.qualified_name = map(p.qualified_name);
            p.parent = map(p.parent);
            p.overrides = map(p.overrides);
            rewrite_dependencies(&mut p.explicit_dependencies, mapping);
            rewrite_dependencies(&mut p.implicit_dependencies, mapping);
        }
        ObjectPayload::Function(p) => {
            p.qualified_name = map(p.qualified_name);
            p.file = map(p.file);
            p.namespace = map(p.namespace);
            p.module = map(p.module);
            rewrite_dependencies(&mut p.explicit_dependencies, mapping);
            rewrite_dependencies(&mut p.implicit_dependencies, mapping);
        }
        ObjectPayload::Part(p) => {
            p.qualified_name = map(p.qualified_name);
            p.parent = map(p.parent);
            rewrite_dependencies(&mut p.explicit_dependencies, mapping);
            rewrite_dependencies(&mut p.implicit_dependencies, mapping);
        }
        ObjectPayload::Mixin(p) => {
            p.qualified_name = map(p.qualified_name);
            rewrite_class_parent(&mut p.parent, mapping);
            rewrite_symbols(&mut p.on, mapping);
            p.superclass = map(p.superclass);
            rewrite_symbols(&mut p.interfaces, mapping);
            rewrite_symbols(&mut p.mixins, mapping);
            rewrite_dependencies(&mut p.explicit_dependencies, mapping);
            rewrite_dependencies(&mut p.implicit_dependencies, mapping);
        }
    }
}

fn rewrite_class_parent(parent: &mut ClassParent, mapping: &BTreeMap<SymbolHash, SymbolHash>) {
    match parent {
        ClassParent::TopLevel {
            file,
            namespace,
            module,
        } => {
            *file = mapping.get(file).copied().unwrap_or(*file);
            *namespace = mapping.get(namespace).copied().unwrap_or(*namespace);
            *module = mapping.get(module).copied().unwrap_or(*module);
        }
        ClassParent::Nested { parent } => {
            *parent = mapping.get(parent).copied().unwrap_or(*parent);
        }
    }
}

fn rewrite_symbols(list: &mut [SymbolHash], mapping: &BTreeMap<SymbolHash, SymbolHash>) {
    for symbol in list {
        *symbol = mapping.get(symbol).copied().unwrap_or(*symbol);
    }
}

fn rewrite_dependencies(
    dependencies: &mut [Dependency],
    mapping: &BTreeMap<SymbolHash, SymbolHash>,
) {
    for dependency in dependencies {
        dependency.target = mapping
            .get(&dependency.target)
            .copied()
            .unwrap_or(dependency.target);
    }
}

fn require_target<'a>(
    entries: &'a BTreeMap<SymbolHash, DesignObject>,
    store: &Store,
    object: SymbolHash,
    target: SymbolHash,
    allowed: &[Kind],
    expected: &'static str,
) -> Result<&'a DesignObject> {
    let found = entries
        .get(&target)
        .ok_or_else(|| CatalogError::MissingReference {
            object: object_label(store, object),
            target: object_label(store, target),
        })?;
    if !allowed.contains(&found.kind) {
        return Err(CatalogError::InvalidRelationKind {
            object: object_label(store, object),
            target: object_label(store, target),
            actual: found.kind,
            expected,
        });
    }
    Ok(found)
}

fn require_optional_target(
    entries: &BTreeMap<SymbolHash, DesignObject>,
    store: &Store,
    object: SymbolHash,
    target: SymbolHash,
    allowed: &[Kind],
    expected: &'static str,
) -> Result<()> {
    if !target.is_zero() {
        require_target(entries, store, object, target, allowed, expected)?;
    }
    Ok(())
}

fn require_existing_target(
    entries: &BTreeMap<SymbolHash, DesignObject>,
    store: &Store,
    object: SymbolHash,
    target: SymbolHash,
) -> Result<()> {
    if !target.is_zero() && !entries.contains_key(&target) {
        return Err(CatalogError::MissingReference {
            object: object_label(store, object),
            target: object_label(store, target),
        });
    }
    Ok(())
}

fn validate_semantics(store: &Store, entries: &BTreeMap<SymbolHash, DesignObject>) -> Result<()> {
    for (qname, object) in entries {
        validate_parents(store, entries, *qname, object)?;
        validate_relation_kinds(store, entries, *qname, object)?;
        validate_dependencies(store, entries, *qname, object)?;
    }
    validate_relation_cycles(store, entries)?;
    validate_overrides(store, entries)?;
    Ok(())
}

fn validate_parents(
    store: &Store,
    entries: &BTreeMap<SymbolHash, DesignObject>,
    qname: SymbolHash,
    object: &DesignObject,
) -> Result<()> {
    match &object.payload {
        ObjectPayload::Directory(p) => {
            if let ParentRef::Object(parent) = p.parent {
                require_target(
                    entries,
                    store,
                    qname,
                    parent,
                    &[Kind::Directory],
                    "a directory",
                )?;
            }
        }
        ObjectPayload::File(p) => {
            require_target(
                entries,
                store,
                qname,
                p.parent,
                &[Kind::Directory],
                "a directory",
            )?;
        }
        ObjectPayload::Namespace(p) => {
            if let ParentRef::Object(parent) = p.parent {
                require_target(
                    entries,
                    store,
                    qname,
                    parent,
                    &[Kind::Namespace],
                    "a namespace",
                )?;
            }
        }
        ObjectPayload::Class(p) => {
            validate_type_parents(store, entries, qname, &p.parent)?;
        }
        ObjectPayload::Interface(p) => {
            validate_type_parents(store, entries, qname, &p.parent)?;
        }
        ObjectPayload::Mixin(p) => {
            validate_type_parents(store, entries, qname, &p.parent)?;
        }
        ObjectPayload::Method(p) => {
            require_target(
                entries,
                store,
                qname,
                p.parent,
                &[Kind::Class, Kind::Interface, Kind::Part],
                "a class, interface, or part",
            )?;
        }
        ObjectPayload::Function(p) => {
            require_target(
                entries,
                store,
                qname,
                p.namespace,
                &[Kind::Namespace],
                "a namespace",
            )?;
            require_optional_target(entries, store, qname, p.file, &[Kind::File], "a file")?;
            require_existing_target(entries, store, qname, p.module)?;
        }
        ObjectPayload::Part(p) => {
            require_target(
                entries,
                store,
                qname,
                p.parent,
                &[Kind::Class, Kind::Interface],
                "a class or interface",
            )?;
        }
        ObjectPayload::View(_) => {}
    }
    Ok(())
}

fn validate_type_parents(
    store: &Store,
    entries: &BTreeMap<SymbolHash, DesignObject>,
    qname: SymbolHash,
    parent: &ClassParent,
) -> Result<()> {
    match parent {
        ClassParent::Nested { parent } => {
            require_target(
                entries,
                store,
                qname,
                *parent,
                &[Kind::Class, Kind::Interface],
                "a class or interface",
            )?;
        }
        ClassParent::TopLevel {
            file,
            namespace,
            module,
        } => {
            if namespace.is_zero() {
                return Err(CatalogError::InvalidParentDetail {
                    object: object_label(store, qname),
                    message: "top-level type has no namespace".to_owned(),
                });
            }
            require_target(
                entries,
                store,
                qname,
                *namespace,
                &[Kind::Namespace],
                "a namespace",
            )?;
            require_optional_target(entries, store, qname, *file, &[Kind::File], "a file")?;
            require_existing_target(entries, store, qname, *module)?;
        }
    }
    Ok(())
}

fn validate_relation_kinds(
    store: &Store,
    entries: &BTreeMap<SymbolHash, DesignObject>,
    qname: SymbolHash,
    object: &DesignObject,
) -> Result<()> {
    match &object.payload {
        ObjectPayload::Class(p) => {
            require_optional_target(
                entries,
                store,
                qname,
                p.superclass,
                &[Kind::Class],
                "a class",
            )?;
            for target in &p.interfaces {
                require_target(
                    entries,
                    store,
                    qname,
                    *target,
                    &[Kind::Interface],
                    "an interface",
                )?;
            }
            for target in &p.mixins {
                require_target(
                    entries,
                    store,
                    qname,
                    *target,
                    &[Kind::Class, Kind::Mixin],
                    "a class or mixin",
                )?;
            }
        }
        ObjectPayload::Interface(p) => {
            require_optional_target(
                entries,
                store,
                qname,
                p.extends,
                &[Kind::Interface],
                "an interface",
            )?;
        }
        ObjectPayload::Mixin(p) => {
            for target in &p.on {
                require_target(
                    entries,
                    store,
                    qname,
                    *target,
                    &[Kind::Class, Kind::Mixin],
                    "a class or mixin",
                )?;
            }
            require_optional_target(
                entries,
                store,
                qname,
                p.superclass,
                &[Kind::Class],
                "a class",
            )?;
            for target in &p.interfaces {
                require_target(
                    entries,
                    store,
                    qname,
                    *target,
                    &[Kind::Interface],
                    "an interface",
                )?;
            }
            for target in &p.mixins {
                require_target(
                    entries,
                    store,
                    qname,
                    *target,
                    &[Kind::Class, Kind::Mixin],
                    "a class or mixin",
                )?;
            }
        }
        ObjectPayload::Method(p) => {
            require_optional_target(
                entries,
                store,
                qname,
                p.overrides,
                &[Kind::Method],
                "a method",
            )?;
        }
        _ => {}
    }
    Ok(())
}

fn validate_dependencies(
    store: &Store,
    entries: &BTreeMap<SymbolHash, DesignObject>,
    qname: SymbolHash,
    object: &DesignObject,
) -> Result<()> {
    let Some((explicit, implicit)) = dependency_lists(object) else {
        return Ok(());
    };
    let structural = relation_targets(object)
        .into_iter()
        .collect::<BTreeSet<_>>();
    for dependency in explicit {
        validate_dependency_target(store, entries, qname, dependency.target)?;
        if structural.contains(&dependency.target) {
            return Err(CatalogError::InvalidDependency {
                object: object_label(store, qname),
                message: "structural relation must not be duplicated as an explicit dependency"
                    .to_owned(),
            });
        }
    }
    for dependency in implicit {
        validate_dependency_target(store, entries, qname, dependency.target)?;
    }
    Ok(())
}

fn validate_dependency_target(
    store: &Store,
    entries: &BTreeMap<SymbolHash, DesignObject>,
    qname: SymbolHash,
    target: SymbolHash,
) -> Result<()> {
    if target == qname {
        return Err(CatalogError::InvalidDependency {
            object: object_label(store, qname),
            message: "dependency cannot refer to itself".to_owned(),
        });
    }
    if !entries.contains_key(&target) {
        return Err(CatalogError::MissingReference {
            object: object_label(store, qname),
            target: object_label(store, target),
        });
    }
    Ok(())
}

fn relation_targets(object: &DesignObject) -> Vec<SymbolHash> {
    let mut targets = Vec::new();
    match &object.payload {
        ObjectPayload::Class(p) => {
            targets.push(p.superclass);
            targets.extend(p.interfaces.iter().copied());
            targets.extend(p.mixins.iter().copied());
        }
        ObjectPayload::Interface(p) => targets.push(p.extends),
        ObjectPayload::Mixin(p) => {
            targets.extend(p.on.iter().copied());
            targets.push(p.superclass);
            targets.extend(p.interfaces.iter().copied());
            targets.extend(p.mixins.iter().copied());
        }
        _ => {}
    }
    targets.retain(|target| !target.is_zero());
    targets
}

fn relation_override(object: &DesignObject) -> Option<SymbolHash> {
    match &object.payload {
        ObjectPayload::Method(p) if !p.overrides.is_zero() => Some(p.overrides),
        _ => None,
    }
}

/// Relations that automatically count as explicit dependencies.
pub fn structural_dependencies(object: &DesignObject) -> Vec<SymbolHash> {
    relation_targets(object)
}

fn dependency_lists(object: &DesignObject) -> Option<(&[Dependency], &[Dependency])> {
    match &object.payload {
        ObjectPayload::Class(p) => Some((&p.explicit_dependencies, &p.implicit_dependencies)),
        ObjectPayload::Interface(p) => Some((&p.explicit_dependencies, &p.implicit_dependencies)),
        ObjectPayload::Method(p) => Some((&p.explicit_dependencies, &p.implicit_dependencies)),
        ObjectPayload::Function(p) => Some((&p.explicit_dependencies, &p.implicit_dependencies)),
        ObjectPayload::Part(p) => Some((&p.explicit_dependencies, &p.implicit_dependencies)),
        ObjectPayload::Mixin(p) => Some((&p.explicit_dependencies, &p.implicit_dependencies)),
        _ => None,
    }
}

fn validate_relation_cycles(
    store: &Store,
    entries: &BTreeMap<SymbolHash, DesignObject>,
) -> Result<()> {
    let mut state: BTreeMap<SymbolHash, u8> = BTreeMap::new();
    let mut stack = Vec::new();
    for qname in entries.keys() {
        visit_relation(*qname, entries, store, &mut state, &mut stack)?;
    }
    Ok(())
}

fn visit_relation(
    qname: SymbolHash,
    entries: &BTreeMap<SymbolHash, DesignObject>,
    store: &Store,
    state: &mut BTreeMap<SymbolHash, u8>,
    stack: &mut Vec<SymbolHash>,
) -> Result<()> {
    match state.get(&qname) {
        Some(2) => return Ok(()),
        Some(1) => {
            let start = stack.iter().position(|item| *item == qname).unwrap_or(0);
            let mut names = stack[start..]
                .iter()
                .map(|item| object_label(store, *item))
                .collect::<Vec<_>>();
            names.push(object_label(store, qname));
            return Err(CatalogError::RelationCycle(names.join(" -> ")));
        }
        _ => {}
    }
    state.insert(qname, 1);
    stack.push(qname);
    if let Some(object) = entries.get(&qname) {
        for target in relation_targets(object) {
            if entries.contains_key(&target) {
                visit_relation(target, entries, store, state, stack)?;
            }
        }
    }
    stack.pop();
    state.insert(qname, 2);
    Ok(())
}

fn validate_overrides(store: &Store, entries: &BTreeMap<SymbolHash, DesignObject>) -> Result<()> {
    for (qname, object) in entries {
        let ObjectPayload::Method(method) = &object.payload else {
            continue;
        };
        if method.overrides.is_zero() {
            continue;
        }
        let method_name = object_label(store, *qname);
        let target =
            entries
                .get(&method.overrides)
                .ok_or_else(|| CatalogError::MissingReference {
                    object: method_name.clone(),
                    target: object_label(store, method.overrides),
                })?;
        let ObjectPayload::Method(target_method) = &target.payload else {
            return Err(CatalogError::InvalidOverride {
                method: method_name,
                message: "overrides target is not a method".to_owned(),
            });
        };
        let owner = method_owner(entries, method.parent)?;
        let target_owner = method_owner(entries, target_method.parent)?;
        let closure = relation_closure(entries, owner);
        if !closure.contains(&target_owner) {
            return Err(CatalogError::InvalidOverride {
                method: method_name,
                message: "overrides target is not in the relation closure".to_owned(),
            });
        }
        let Some(signature) = signature_of(store, *qname) else {
            return Err(CatalogError::InvalidOverride {
                method: method_name,
                message: "method qualified name has no signature hash".to_owned(),
            });
        };
        let Some(target_signature) = signature_of(store, method.overrides) else {
            return Err(CatalogError::InvalidOverride {
                method: method_name,
                message: "overrides target has no signature hash".to_owned(),
            });
        };
        if signature != target_signature {
            return Err(CatalogError::InvalidOverride {
                method: method_name,
                message: "overrides target signature does not match".to_owned(),
            });
        }
    }
    Ok(())
}

fn method_owner(
    entries: &BTreeMap<SymbolHash, DesignObject>,
    parent: SymbolHash,
) -> Result<SymbolHash> {
    let Some(object) = entries.get(&parent) else {
        return Ok(parent);
    };
    if let ObjectPayload::Part(part) = &object.payload {
        return Ok(part.parent);
    }
    Ok(parent)
}

fn relation_closure(
    entries: &BTreeMap<SymbolHash, DesignObject>,
    owner: SymbolHash,
) -> BTreeSet<SymbolHash> {
    let mut seen = BTreeSet::new();
    let mut queue = entries
        .get(&owner)
        .map(relation_targets)
        .unwrap_or_default();
    while let Some(target) = queue.pop() {
        if !seen.insert(target) {
            continue;
        }
        if let Some(object) = entries.get(&target) {
            queue.extend(relation_targets(object));
        }
    }
    seen
}

fn signature_of(store: &Store, qname: SymbolHash) -> Option<String> {
    let text = store.get_symbol(qname).ok()?;
    let (_, signature) = text.rsplit_once('#')?;
    Some(signature.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::symbol_hash;
    use crate::store::Store;
    use tempfile::tempdir;

    fn sym(text: &str) -> SymbolHash {
        symbol_hash(text)
    }

    fn directory(qname: &str, parent: ParentRef) -> DesignObject {
        DesignObject::new(
            Kind::Directory,
            ObjectPayload::Directory(DirectoryPayload {
                qualified_name: sym(qname),
                parent,
                source: None,
                design_doc: DocHash::ZERO,
                children: vec![],
            }),
        )
    }

    fn ns(qname: &str, parent: ParentRef) -> DesignObject {
        DesignObject::new(
            Kind::Namespace,
            ObjectPayload::Namespace(NamespacePayload {
                qualified_name: sym(qname),
                parent,
                source: None,
                content_hash: Hash32::ZERO,
                design_doc: DocHash::ZERO,
                children: vec![],
            }),
        )
    }

    fn file(qname: &str, parent: &str) -> DesignObject {
        DesignObject::new(
            Kind::File,
            ObjectPayload::File(FilePayload {
                qualified_name: sym(qname),
                parent: sym(parent),
                source: None,
                content_hash: Hash32::ZERO,
                design_doc: DocHash::ZERO,
                children: vec![],
            }),
        )
    }

    fn class(qname: &str, namespace: &str, file: Option<&str>) -> DesignObject {
        DesignObject::new(
            Kind::Class,
            ObjectPayload::Class(ClassPayload {
                qualified_name: sym(qname),
                parent: ClassParent::TopLevel {
                    file: file.map(sym).unwrap_or(SymbolHash::ZERO),
                    namespace: sym(namespace),
                    module: SymbolHash::ZERO,
                },
                source: None,
                content_hash: Hash32::ZERO,
                design_doc: DocHash::ZERO,
                superclass: SymbolHash::ZERO,
                interfaces: vec![],
                mixins: vec![],
                explicit_dependencies: vec![],
                implicit_dependencies: vec![],
                children: vec![],
            }),
        )
    }

    fn method(qname: &str, parent: &str) -> DesignObject {
        DesignObject::new(
            Kind::Method,
            ObjectPayload::Method(MethodPayload {
                qualified_name: sym(qname),
                parent: sym(parent),
                source: None,
                content_hash: Hash32::ZERO,
                design_doc: DocHash::ZERO,
                overrides: SymbolHash::ZERO,
                explicit_dependencies: vec![],
                implicit_dependencies: vec![],
            }),
        )
    }

    fn base_catalog(store: &Store) -> Catalog {
        // Store symbols so the graph can be displayed later; the builder itself
        // only uses hashes.
        for name in [
            "files",
            "namespaces",
            "namespaces/app",
            "namespaces/app/Auth",
        ] {
            store.put_symbol(name).unwrap();
        }
        let mut catalog = Catalog::new(GitOid::ZERO);
        catalog
            .insert(sym("files"), directory("files", ParentRef::View))
            .unwrap();
        catalog
            .insert(sym("namespaces"), ns("namespaces", ParentRef::View))
            .unwrap();
        catalog
            .insert(
                sym("namespaces/app"),
                ns("namespaces/app", ParentRef::Object(sym("namespaces"))),
            )
            .unwrap();
        catalog
            .insert(
                sym("namespaces/app/Auth"),
                class("namespaces/app/Auth", "namespaces/app", None),
            )
            .unwrap();
        catalog
    }

    #[test]
    fn rebuilds_view_and_parent_children() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join(".ds")).unwrap();
        let catalog = base_catalog(&store);
        let view = catalog.rebuild(&store).unwrap();

        let loaded = Catalog::load(&store, view).unwrap();
        assert_eq!(loaded.len(), catalog.len());
        assert_eq!(loaded.source_commit(), GitOid::ZERO);

        let view_object = store.get_object(view).unwrap();
        let ObjectPayload::View(p) = view_object.payload else {
            panic!("not a view");
        };
        let files = store.get_object(p.files).unwrap();
        let namespaces = store.get_object(p.namespaces).unwrap();
        let ObjectPayload::Directory(files_p) = files.payload else {
            panic!("not a dir");
        };
        let ObjectPayload::Namespace(ns_p) = namespaces.payload else {
            panic!("not a ns");
        };
        assert_eq!(files_p.children.len(), 0);
        assert_eq!(ns_p.children.len(), 1);
    }

    #[test]
    fn changing_child_changes_ancestors() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join(".ds")).unwrap();
        let mut catalog = base_catalog(&store);
        let view1 = catalog.rebuild(&store).unwrap();

        let doc = store.put_doc("class design").unwrap();
        let auth = catalog.get_mut(sym("namespaces/app/Auth")).unwrap();
        let ObjectPayload::Class(p) = &mut auth.payload else {
            panic!("not a class");
        };
        p.design_doc = doc;
        let view2 = catalog.rebuild(&store).unwrap();
        assert_ne!(view1, view2);
    }

    #[test]
    fn rejects_missing_parent() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join(".ds")).unwrap();
        let mut catalog = base_catalog(&store);
        catalog
            .insert(
                sym("namespaces/missing/C"),
                class("namespaces/missing/C", "namespaces/missing", None),
            )
            .unwrap();
        assert!(matches!(
            catalog.rebuild(&store),
            Err(CatalogError::MissingParent(_))
        ));
    }

    #[test]
    fn rejects_invalid_parent_kinds_in_cycles() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join(".ds")).unwrap();
        let mut catalog = Catalog::new(GitOid::ZERO);
        catalog
            .insert(sym("files"), directory("files", ParentRef::View))
            .unwrap();
        catalog
            .insert(sym("namespaces"), ns("namespaces", ParentRef::View))
            .unwrap();
        // Two top-level classes reference each other through their file
        // parent fields. This is malformed input: a class cannot be a file
        // parent.
        let mut a = class("namespaces/A", "namespaces", None);
        let mut b = class("namespaces/B", "namespaces", None);
        let ObjectPayload::Class(a_p) = &mut a.payload else {
            unreachable!()
        };
        a_p.parent = ClassParent::TopLevel {
            file: sym("namespaces/B"),
            namespace: sym("namespaces"),
            module: SymbolHash::ZERO,
        };
        let ObjectPayload::Class(b_p) = &mut b.payload else {
            unreachable!()
        };
        b_p.parent = ClassParent::TopLevel {
            file: sym("namespaces/A"),
            namespace: sym("namespaces"),
            module: SymbolHash::ZERO,
        };
        catalog.insert(sym("namespaces/A"), a).unwrap();
        catalog.insert(sym("namespaces/B"), b).unwrap();
        assert!(matches!(
            catalog.rebuild(&store),
            Err(CatalogError::InvalidRelationKind { .. })
        ));
    }

    #[test]
    fn rejects_inheritance_cycles() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join(".ds")).unwrap();
        let mut catalog = base_catalog(&store);
        let a = catalog.get_mut(sym("namespaces/app/Auth")).unwrap();
        let ObjectPayload::Class(p) = &mut a.payload else {
            unreachable!()
        };
        p.superclass = sym("namespaces/app/Auth");
        assert!(matches!(
            catalog.rebuild(&store),
            Err(CatalogError::RelationCycle(_))
        ));
    }

    #[test]
    fn supports_shared_child_under_two_parents() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join(".ds")).unwrap();
        let mut catalog = base_catalog(&store);
        catalog
            .insert(
                sym("files/src"),
                directory("files/src", ParentRef::Object(sym("files"))),
            )
            .unwrap();
        catalog
            .insert(
                sym("files/src/auth.rs"),
                file("files/src/auth.rs", "files/src"),
            )
            .unwrap();
        let auth = catalog.get_mut(sym("namespaces/app/Auth")).unwrap();
        let ObjectPayload::Class(p) = &mut auth.payload else {
            unreachable!()
        };
        p.parent = ClassParent::TopLevel {
            file: sym("files/src/auth.rs"),
            namespace: sym("namespaces/app"),
            module: SymbolHash::ZERO,
        };
        let view = catalog.rebuild(&store).unwrap();
        let loaded = Catalog::load(&store, view).unwrap();
        assert_eq!(loaded.len(), catalog.len());
    }

    #[test]
    fn renames_descendants_and_references() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join(".ds")).unwrap();
        let mut catalog = base_catalog(&store);
        catalog
            .insert(
                sym("namespaces/app/Base"),
                class("namespaces/app/Base", "namespaces/app", None),
            )
            .unwrap();
        let derived = class("namespaces/app/Derived", "namespaces/app", None);
        let ObjectPayload::Class(mut derived_p) = derived.payload else {
            unreachable!()
        };
        derived_p.superclass = sym("namespaces/app/Base");
        catalog
            .insert(
                sym("namespaces/app/Derived"),
                DesignObject::new(Kind::Class, ObjectPayload::Class(derived_p)),
            )
            .unwrap();
        catalog
            .insert(
                sym("namespaces/app/Base/login#sig"),
                method("namespaces/app/Base/login#sig", "namespaces/app/Base"),
            )
            .unwrap();

        let mapping = BTreeMap::from([
            (sym("namespaces/app/Base"), sym("namespaces/app/Base2")),
            (
                sym("namespaces/app/Base/login#sig"),
                sym("namespaces/app/Base2/login#sig"),
            ),
        ]);
        catalog.rename_symbols(&mapping).unwrap();

        assert!(catalog.get(sym("namespaces/app/Base2")).is_some());
        assert!(catalog.get(sym("namespaces/app/Base2/login#sig")).is_some());
        let derived = catalog.get(sym("namespaces/app/Derived")).unwrap();
        let ObjectPayload::Class(p) = &derived.payload else {
            unreachable!()
        };
        assert_eq!(p.superclass, sym("namespaces/app/Base2"));
        let method = catalog.get(sym("namespaces/app/Base2/login#sig")).unwrap();
        let ObjectPayload::Method(p) = &method.payload else {
            unreachable!()
        };
        assert_eq!(p.parent, sym("namespaces/app/Base2"));
        catalog.rebuild(&store).unwrap();
    }
}
