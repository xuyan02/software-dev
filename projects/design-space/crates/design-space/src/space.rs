//! Building versions, refs, and reading a design space back.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::branch;
use crate::element::Element;
use crate::error::{Error, Result};
use crate::hash::{Hash, ZERO_HASH, from_hex, hash, is_zero, to_commit_hex, to_hex};
use crate::node::{DesignNode, IndexEntry, IndexNode, Node, ObjectNode, VersionNode};
use crate::store::Store;
use crate::types::{T_DIRECTORY, T_FILE, T_VIRTUAL_ROOT};

/// Target number of entries per index node.
pub const FANOUT: usize = 32;

pub struct CommitOptions {
    pub message: String,
    pub author: String,
    pub source_commit: [u8; 32],
    pub pipeline: String,
    pub ref_name: String,
}

#[derive(Debug)]
pub struct CommitResult {
    pub version: Hash,
    pub root: Hash,
    pub objects: usize,
    pub missing: usize,
}

/// Build objects, views, a root and a version from an element set, then advance
/// the branch ref. Missing designs are allowed; their `design_hash` is 0.
pub fn commit(
    store: &Store,
    dir: &Path,
    elements: &[Element],
    options: &CommitOptions,
) -> Result<CommitResult> {
    let index = index_elements(elements)?;
    validate_tree(elements, &index)?;

    let order = topological_order(elements, &index)?;
    let mut computed: BTreeMap<String, (Hash, Hash, Hash)> = BTreeMap::new();
    let mut objects = 0usize;
    let mut missing = 0usize;

    for name in order {
        let element = &elements[index[name.as_str()]];
        let is_file = element.ty == T_FILE;
        if !is_file && element.design.is_none() {
            missing += 1;
        }
        let design = if is_file {
            None
        } else {
            element.design.as_ref()
        };

        let child_content: Vec<Hash> = element
            .children
            .iter()
            .map(|child| computed[child.as_str()].0)
            .collect();
        let child_design: Vec<Hash> = element
            .children
            .iter()
            .map(|child| computed[child.as_str()].1)
            .collect();

        let content = match &element.content_hash {
            Some(hex) => from_hex(hex)?,
            None => content_hash(&element.own_text, &child_content),
        };
        let design_hash = match design {
            Some(text) => store.put(&Node::Design(DesignNode { text: text.clone() }))?,
            None => ZERO_HASH,
        };
        let input = input_hash(&content, &child_design);

        let object = Node::Object(ObjectNode {
            ty: element.ty,
            commit: parse_commit_hex(&element.commit)?,
            start_byte: element.start_byte,
            end_byte: element.end_byte,
            start_line: element.start_line,
            end_line: element.end_line,
            content_hash: content,
            input_hash: input,
            design_hash,
            name: element.name.clone(),
            path: element.path.clone(),
        });
        let object_hash = store.put(&object)?;
        computed.insert(name, (content, design_hash, object_hash));
        objects += 1;
    }

    let namespaces_root = build_view(
        store,
        elements
            .iter()
            .filter(|element| element.ty != T_FILE)
            .map(|element| IndexEntry {
                key: element.name.as_bytes().to_vec(),
                hash: computed[element.name.as_str()].2,
            })
            .collect(),
    )?;
    let files_root = build_view(
        store,
        elements
            .iter()
            .map(|element| IndexEntry {
                key: file_key(element),
                hash: computed[element.name.as_str()].2,
            })
            .collect(),
    )?;

    let virtual_root = IndexNode::new(
        T_VIRTUAL_ROOT,
        false,
        vec![
            IndexEntry {
                key: b"files".to_vec(),
                hash: files_root,
            },
            IndexEntry {
                key: b"namespaces".to_vec(),
                hash: namespaces_root,
            },
        ],
    )?;
    let root = store.put(&Node::Index(virtual_root))?;

    let parent = read_ref_optional(dir, &options.ref_name)?.unwrap_or(ZERO_HASH);
    let version = Node::Version(VersionNode {
        parent,
        root,
        source_commit: options.source_commit,
        pipeline: options.pipeline.clone(),
        author: options.author.clone(),
        message: options.message.clone(),
        timestamp: now(),
    });
    let version_hash = store.put(&version)?;

    write_tree_cache(dir, &version_hash, elements)?;
    write_ref(dir, &options.ref_name, &version_hash)?;

    Ok(CommitResult {
        version: version_hash,
        root,
        objects,
        missing,
    })
}

struct PreviousDesign {
    input_hash: Hash,
    design: String,
}

fn previous_from_version(store: &Store, base: &Hash) -> Result<BTreeMap<String, PreviousDesign>> {
    let version = load_version(store, base)?;
    let Some(view) = view_root(store, &version, b"namespaces")? else {
        return Ok(BTreeMap::new());
    };
    let mut entries = Vec::new();
    collect_view(store, &view, &mut entries)?;

    let mut previous = BTreeMap::new();
    for (_, object_hash) in entries {
        let Node::Object(object) = store.get(&object_hash)? else {
            continue;
        };
        if is_zero(&object.design_hash) {
            continue;
        }
        let Node::Design(design) = store.get(&object.design_hash)? else {
            continue;
        };
        previous.insert(
            object.name.clone(),
            PreviousDesign {
                input_hash: object.input_hash,
                design: design.text,
            },
        );
    }
    Ok(previous)
}

/// Designs already written in a branch's working state, keyed by element name.
fn previous_from_elements(elements: &[Element]) -> Result<BTreeMap<String, PreviousDesign>> {
    let index = index_elements(elements)?;
    let order = topological_order(elements, &index)?;
    let mut contents: BTreeMap<String, Hash> = BTreeMap::new();
    let mut designs: BTreeMap<String, Hash> = BTreeMap::new();
    let mut previous = BTreeMap::new();

    for name in order {
        let element = &elements[index[&name]];
        let child_contents: Vec<Hash> = element
            .children
            .iter()
            .map(|child| contents[child.as_str()])
            .collect();
        let content = match &element.content_hash {
            Some(hex) => from_hex(hex)?,
            None => content_hash(&element.own_text, &child_contents),
        };
        let child_designs: Option<Vec<Hash>> = element
            .children
            .iter()
            .map(|child| designs.get(child.as_str()).copied())
            .collect();

        if let Some(text) = &element.design {
            let design_hash = Node::Design(DesignNode { text: text.clone() }).hash();
            designs.insert(name.clone(), design_hash);
            if let Some(child_designs) = child_designs {
                previous.insert(
                    name.clone(),
                    PreviousDesign {
                        input_hash: input_hash(&content, &child_designs),
                        design: text.clone(),
                    },
                );
            }
        }
        contents.insert(name, content);
    }
    Ok(previous)
}

fn apply_previous(
    elements: &mut [Element],
    previous: &BTreeMap<String, PreviousDesign>,
) -> Result<usize> {
    let index = index_elements(elements)?;
    let order = topological_order(elements, &index)?;
    let mut contents: BTreeMap<String, Hash> = BTreeMap::new();
    let mut designs: BTreeMap<String, Hash> = BTreeMap::new();
    let mut reused = 0;

    for name in order {
        let position = index[&name];
        let (content, child_designs, own_design) = {
            let element = &elements[position];
            let child_contents: Vec<Hash> = element
                .children
                .iter()
                .map(|child| contents[child.as_str()])
                .collect();
            let content = match &element.content_hash {
                Some(hex) => from_hex(hex)?,
                None => content_hash(&element.own_text, &child_contents),
            };
            let child_designs: Option<Vec<Hash>> = element
                .children
                .iter()
                .map(|child| designs.get(child.as_str()).copied())
                .collect();
            (content, child_designs, element.design.clone())
        };

        if let Some(text) = own_design {
            let design_hash = Node::Design(DesignNode { text }).hash();
            designs.insert(name.clone(), design_hash);
        } else if let (Some(child_designs), Some(old)) = (child_designs, previous.get(&name)) {
            let input = input_hash(&content, &child_designs);
            if input == old.input_hash {
                elements[position].design = Some(old.design.clone());
                designs.insert(
                    name.clone(),
                    Node::Design(DesignNode {
                        text: old.design.clone(),
                    })
                    .hash(),
                );
                reused += 1;
            }
        }

        contents.insert(name, content);
    }
    Ok(reused)
}

/// Copy designs from a committed version where the input hash is unchanged.
pub fn reuse_designs(store: &Store, base: &Hash, elements: &mut [Element]) -> Result<usize> {
    let previous = previous_from_version(store, base)?;
    if previous.is_empty() {
        return Ok(0);
    }
    apply_previous(elements, &previous)
}

/// Copy designs the agent already wrote in this branch's working state.
pub fn reuse_working_designs(elements: &mut [Element], work: &[Element]) -> Result<usize> {
    let previous = previous_from_elements(work)?;
    if previous.is_empty() {
        return Ok(0);
    }
    apply_previous(elements, &previous)
}

/// Parent/child links of one committed element, kept in a local cache.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CachedElement {
    name: String,
    parent: Option<String>,
    children: Vec<String>,
}

fn tree_cache_path(dir: &Path, version: &Hash) -> PathBuf {
    dir.join("cache")
        .join("trees")
        .join(format!("{}.json", to_hex(version)))
}

/// Persist the element hierarchy of a committed version.
///
/// Versions store hashes and spans but not parent/child links. Incremental
/// scan needs the links to rebuild unchanged files without parsing; this local
/// cache provides them. A missing cache only costs a full re-parse, never
/// correctness.
fn write_tree_cache(dir: &Path, version: &Hash, elements: &[Element]) -> Result<()> {
    let entries: Vec<CachedElement> = elements
        .iter()
        .map(|element| CachedElement {
            name: element.name.clone(),
            parent: element.parent.clone(),
            children: element.children.clone(),
        })
        .collect();
    let path = tree_cache_path(dir, version);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(&entries)?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes)?;
    fs::rename(temp, path)?;
    Ok(())
}

fn read_tree_cache(dir: &Path, version: &Hash) -> Option<Vec<CachedElement>> {
    let bytes = fs::read(tree_cache_path(dir, version)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Load the element set of a committed version.
///
/// `hierarchy` also restores parent/child links from the local tree cache;
/// that is required to rebuild ancestor hashes after a design edit.
pub fn elements_at(
    store: &Store,
    dir: &Path,
    version: &Hash,
    hierarchy: bool,
) -> Result<Vec<Element>> {
    let version_node = load_version(store, version)?;
    let Some(view) = view_root(store, &version_node, b"files")? else {
        return Ok(Vec::new());
    };
    let mut entries = Vec::new();
    collect_view(store, &view, &mut entries)?;

    let cached = if hierarchy {
        Some(read_tree_cache(dir, version).ok_or_else(|| {
            Error::Config("tree cache is missing for this version; run `ds scan`".into())
        })?)
    } else {
        None
    };
    let links: BTreeMap<String, (Option<String>, Vec<String>)> = cached
        .iter()
        .flatten()
        .map(|entry| {
            (
                entry.name.clone(),
                (entry.parent.clone(), entry.children.clone()),
            )
        })
        .collect();

    let mut elements = Vec::new();
    for (_, object_hash) in entries {
        let Node::Object(object) = store.get(&object_hash)? else {
            continue;
        };
        let design = if is_zero(&object.design_hash) {
            None
        } else {
            match store.get(&object.design_hash)? {
                Node::Design(design) => Some(design.text),
                _ => None,
            }
        };
        let (parent, children) = match links.get(&object.name) {
            Some((parent, children)) => (parent.clone(), children.clone()),
            None if hierarchy => {
                return Err(Error::Config(format!(
                    "tree cache has no entry for {}",
                    object.name
                )));
            }
            None => (None, Vec::new()),
        };
        elements.push(Element {
            name: object.name.clone(),
            ty: object.ty,
            path: object.path.clone(),
            commit: to_commit_hex(&object.commit),
            start_byte: object.start_byte,
            end_byte: object.end_byte,
            start_line: object.start_line,
            end_line: object.end_line,
            own_text: String::new(),
            parent,
            children,
            design,
            content_hash: Some(to_hex(&object.content_hash)),
        });
    }
    Ok(elements)
}

/// Rebuild work elements for files whose blob did not change.
///
/// Elements get their old content hash and the hierarchy from the tree cache;
/// designs are refilled by [`reuse_designs`]. Files missing from the cache are
/// returned in the second set so the caller parses them instead.
pub fn reuse_file_elements(
    store: &Store,
    dir: &Path,
    base: &Hash,
    paths: &BTreeSet<String>,
    commit_hex: &str,
) -> Result<(Vec<Element>, BTreeSet<String>)> {
    if paths.is_empty() {
        return Ok((Vec::new(), BTreeSet::new()));
    }
    let Some(cached) = read_tree_cache(dir, base) else {
        return Ok((Vec::new(), paths.clone()));
    };
    let hierarchy: BTreeMap<&str, &CachedElement> = cached
        .iter()
        .map(|entry| (entry.name.as_str(), entry))
        .collect();

    let version = load_version(store, base)?;
    let Some(view) = view_root(store, &version, b"namespaces")? else {
        return Ok((Vec::new(), paths.clone()));
    };
    let mut entries = Vec::new();
    collect_view(store, &view, &mut entries)?;

    let mut by_path: BTreeMap<String, Vec<ObjectNode>> = BTreeMap::new();
    for (_, object_hash) in entries {
        if let Node::Object(object) = store.get(&object_hash)?
            && paths.contains(&object.path)
        {
            by_path.entry(object.path.clone()).or_default().push(object);
        }
    }

    let mut elements = Vec::new();
    let mut failed = BTreeSet::new();
    for (path, objects) in by_path {
        let names: BTreeSet<&str> = objects.iter().map(|object| object.name.as_str()).collect();
        let mut complete = true;
        let mut path_elements = Vec::with_capacity(objects.len());

        for object in &objects {
            let Some(entry) = hierarchy.get(object.name.as_str()) else {
                complete = false;
                break;
            };
            if let Some(parent) = &entry.parent
                && !names.contains(parent.as_str())
            {
                complete = false;
                break;
            }
            if entry
                .children
                .iter()
                .any(|child| !names.contains(child.as_str()))
            {
                complete = false;
                break;
            }
            path_elements.push(Element {
                name: object.name.clone(),
                ty: object.ty,
                path: path.clone(),
                commit: commit_hex.to_owned(),
                start_byte: object.start_byte,
                end_byte: object.end_byte,
                start_line: object.start_line,
                end_line: object.end_line,
                own_text: String::new(),
                parent: entry.parent.clone(),
                children: entry.children.clone(),
                design: None,
                content_hash: Some(to_hex(&object.content_hash)),
            });
        }

        if complete {
            elements.extend(path_elements);
        } else {
            failed.insert(path);
        }
    }
    Ok((elements, failed))
}

fn index_elements(elements: &[Element]) -> Result<BTreeMap<String, usize>> {
    let mut index = BTreeMap::new();
    for (position, element) in elements.iter().enumerate() {
        if index.insert(element.name.clone(), position).is_some() {
            return Err(Error::Config(format!(
                "duplicate element name {}",
                element.name
            )));
        }
    }
    Ok(index)
}

fn validate_tree(elements: &[Element], index: &BTreeMap<String, usize>) -> Result<()> {
    for element in elements {
        if let Some(parent) = &element.parent {
            let parent_element = index
                .get(parent.as_str())
                .map(|position| &elements[*position])
                .ok_or_else(|| Error::Config(format!("unknown parent {parent}")))?;
            if !parent_element
                .children
                .iter()
                .any(|child| child == &element.name)
            {
                return Err(Error::Config(format!(
                    "{} lists {} as parent but does not include it as a child",
                    parent, element.name
                )));
            }
        }
        for child in &element.children {
            let child_element = index
                .get(child.as_str())
                .map(|position| &elements[*position])
                .ok_or_else(|| Error::Config(format!("unknown child {child}")))?;
            if child_element.parent.as_deref() != Some(element.name.as_str()) {
                return Err(Error::Config(format!(
                    "{} lists {} as child but its parent is different",
                    element.name, child
                )));
            }
        }
    }
    Ok(())
}

fn topological_order(elements: &[Element], index: &BTreeMap<String, usize>) -> Result<Vec<String>> {
    let mut state: BTreeMap<String, u8> = BTreeMap::new();
    let mut order = Vec::with_capacity(elements.len());

    fn visit(
        name: &str,
        elements: &[Element],
        index: &BTreeMap<String, usize>,
        state: &mut BTreeMap<String, u8>,
        order: &mut Vec<String>,
    ) -> Result<()> {
        match state.get(name).copied() {
            Some(2) => return Ok(()),
            Some(1) => {
                return Err(Error::Config(format!("cycle in element tree at {name}")));
            }
            _ => {}
        }
        state.insert(name.to_owned(), 1);
        let position = *index
            .get(name)
            .ok_or_else(|| Error::Config(format!("unknown element {name}")))?;
        let element = &elements[position];
        for child in &element.children {
            visit(child, elements, index, state, order)?;
        }
        state.insert(name.to_owned(), 2);
        order.push(name.to_owned());
        Ok(())
    }

    for element in elements {
        visit(&element.name, elements, index, &mut state, &mut order)?;
    }
    Ok(order)
}

fn file_key(element: &Element) -> Vec<u8> {
    if element.name == element.path {
        return element.path.as_bytes().to_vec();
    }
    let mut key = element.path.as_bytes().to_vec();
    key.push(0);
    key.extend_from_slice(element.name.as_bytes());
    key
}

fn content_hash(own_text: &str, children: &[Hash]) -> Hash {
    let mut buffer = Vec::with_capacity(1 + 8 + own_text.len() + children.len() * 32);
    buffer.push(if children.is_empty() { 0x01 } else { 0x02 });
    buffer.extend_from_slice(&(own_text.len() as u64).to_le_bytes());
    buffer.extend_from_slice(own_text.as_bytes());
    for child in children {
        buffer.extend_from_slice(child);
    }
    hash(&buffer)
}

fn input_hash(content: &Hash, children: &[Hash]) -> Hash {
    if children.is_empty() {
        return *content;
    }
    let mut buffer = Vec::with_capacity(1 + 32 + children.len() * 32);
    buffer.push(0x03);
    buffer.extend_from_slice(content);
    for child in children {
        buffer.extend_from_slice(child);
    }
    hash(&buffer)
}

fn build_view(store: &Store, mut entries: Vec<IndexEntry>) -> Result<Hash> {
    entries.sort_by(|a, b| a.key.cmp(&b.key));

    if entries.len() <= FANOUT {
        return store.put(&Node::Index(IndexNode::new(T_DIRECTORY, true, entries)?));
    }

    let mut level = Vec::new();
    for chunk in entries.chunks(FANOUT) {
        let key = chunk.last().expect("chunks are never empty").key.clone();
        let node = IndexNode::new(T_DIRECTORY, true, chunk.to_vec())?;
        let child = store.put(&Node::Index(node))?;
        level.push(IndexEntry { key, hash: child });
    }

    while level.len() > 1 {
        if level.len() <= FANOUT {
            return store.put(&Node::Index(IndexNode::new(T_DIRECTORY, false, level)?));
        }
        let mut next = Vec::new();
        for chunk in level.chunks(FANOUT) {
            let key = chunk.last().expect("chunks are never empty").key.clone();
            let node = IndexNode::new(T_DIRECTORY, false, chunk.to_vec())?;
            let child = store.put(&Node::Index(node))?;
            next.push(IndexEntry { key, hash: child });
        }
        level = next;
    }

    Ok(level[0].hash)
}

pub fn parse_commit_hex(input: &str) -> Result<[u8; 32]> {
    let input = input.trim();
    match input.len() {
        40 => {
            let bytes = bytes_from_hex(input)?;
            let mut out = ZERO_HASH;
            out[12..].copy_from_slice(&bytes);
            Ok(out)
        }
        64 => from_hex(input),
        other => Err(Error::Config(format!(
            "commit must be 40 or 64 hex characters, got {other}"
        ))),
    }
}

fn bytes_from_hex(input: &str) -> Result<Vec<u8>> {
    if !input.len().is_multiple_of(2) {
        return Err(Error::Hex("odd number of hex characters".into()));
    }
    let mut out = Vec::with_capacity(input.len() / 2);
    for chunk in input.as_bytes().chunks_exact(2) {
        let text = std::str::from_utf8(chunk).map_err(|error| Error::Hex(error.to_string()))?;
        out.push(
            u8::from_str_radix(text, 16).map_err(|error| Error::Hex(format!("{text}: {error}")))?,
        );
    }
    Ok(out)
}

pub fn resolve_version(store: &Store, dir: &Path, at: Option<&str>) -> Result<Hash> {
    let address = match at {
        Some(value) if value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()) => {
            from_hex(value)?
        }
        Some(reference) => read_ref(dir, reference)?,
        None => read_ref(dir, &branch::current(dir)?)?,
    };
    store.get(&address)?;
    Ok(address)
}

pub fn load_version(store: &Store, version: &Hash) -> Result<VersionNode> {
    match store.get(version)? {
        Node::Version(version) => Ok(version),
        _ => Err(Error::InvalidNode("not a version node".into())),
    }
}

pub fn view_root(store: &Store, version: &VersionNode, name: &[u8]) -> Result<Option<Hash>> {
    match store.get(&version.root)? {
        Node::Index(root) => Ok(root.get(name).copied()),
        _ => Err(Error::InvalidNode("root is not an index node".into())),
    }
}

pub fn lookup_object(
    store: &Store,
    version: &Hash,
    name: &str,
) -> Result<(ObjectNode, DesignNode)> {
    let version = load_version(store, version)?;
    let mut object_hash = None;
    for view_name in [&b"namespaces"[..], &b"files"[..]] {
        if let Some(view) = view_root(store, &version, view_name)?
            && let Some(hash) = find_in_view(store, &view, name.as_bytes())?
        {
            object_hash = Some(hash);
            break;
        }
    }
    let object_hash = object_hash.ok_or_else(|| Error::NotFound(name.to_owned()))?;
    let object = match store.get(&object_hash)? {
        Node::Object(object) => object,
        _ => return Err(Error::InvalidNode("view leaf is not an object".into())),
    };
    let design = if is_zero(&object.design_hash) {
        DesignNode {
            text: String::new(),
        }
    } else {
        match store.get(&object.design_hash)? {
            Node::Design(design) => design,
            _ => {
                return Err(Error::InvalidNode(
                    "object does not point at a design".into(),
                ));
            }
        }
    };
    Ok((object, design))
}

pub fn find_in_view(store: &Store, root: &Hash, key: &[u8]) -> Result<Option<Hash>> {
    let mut current = *root;
    loop {
        match store.get(&current)? {
            Node::Index(node) => {
                if node.leaf {
                    return Ok(node.get(key).copied());
                }
                match node.child_for(key) {
                    Some(next) => current = *next,
                    None => return Ok(None),
                }
            }
            _ => return Err(Error::InvalidNode("view node is not an index".into())),
        }
    }
}

pub fn collect_view(store: &Store, root: &Hash, out: &mut Vec<(Vec<u8>, Hash)>) -> Result<()> {
    match store.get(root)? {
        Node::Index(node) => {
            if node.leaf {
                for entry in node.entries {
                    out.push((entry.key, entry.hash));
                }
            } else {
                for entry in node.entries {
                    collect_view(store, &entry.hash, out)?;
                }
            }
            Ok(())
        }
        _ => Err(Error::InvalidNode("view node is not an index".into())),
    }
}

pub fn read_ref(dir: &Path, name: &str) -> Result<Hash> {
    read_ref_optional(dir, name)?.ok_or_else(|| Error::NotFound(format!("ref {name}")))
}

pub fn read_ref_optional(dir: &Path, name: &str) -> Result<Option<Hash>> {
    let path = ref_path(dir, name)?;
    match fs::read_to_string(&path) {
        Ok(text) => Ok(Some(from_hex(text.trim())?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Io(error)),
    }
}

pub fn write_ref(dir: &Path, name: &str, version: &Hash) -> Result<()> {
    let path = ref_path(dir, name)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_dir = dir.join("tmp");
    fs::create_dir_all(&tmp_dir)?;
    let temp = tmp_dir.join(format!("ref-{}", name.replace('/', "_")));
    fs::write(&temp, format!("{}\n", to_hex(version)))?;
    fs::rename(temp, path)?;
    Ok(())
}

fn ref_path(dir: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
    {
        return Err(Error::Config(format!("invalid ref name {name}")));
    }
    Ok(dir.join("refs/heads").join(name))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// True when a parent field means "no parent".
pub fn is_initial(version: &VersionNode) -> bool {
    is_zero(&version.parent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::Element;
    use crate::types::{CPP_CLASS, CPP_METHOD};

    fn element(name: &str, parent: Option<&str>, children: Vec<&str>, design: &str) -> Element {
        Element {
            name: name.into(),
            ty: if children.is_empty() {
                CPP_METHOD
            } else {
                CPP_CLASS
            },
            path: "src/a.cpp".into(),
            commit: "00".repeat(32),
            start_byte: 0,
            end_byte: 10,
            start_line: 1,
            end_line: 2,
            own_text: format!("text of {name}"),
            parent: parent.map(str::to_owned),
            children: children.into_iter().map(str::to_owned).collect(),
            design: Some(design.into()),
            content_hash: None,
        }
    }

    fn file_element(path: &str) -> Element {
        Element {
            name: path.into(),
            ty: T_FILE,
            path: path.into(),
            commit: "00".repeat(32),
            start_byte: 0,
            end_byte: 0,
            start_line: 0,
            end_line: 0,
            own_text: String::new(),
            parent: None,
            children: vec![],
            design: None,
            content_hash: Some(to_hex(&hash(b"blob"))),
        }
    }

    fn setup() -> (tempfile::TempDir, Store, PathBuf, Vec<Element>) {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join(".ds");
        fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        let elements = vec![
            element("acme::Foo", None, vec!["acme::Foo::count"], "the class"),
            element("acme::Foo::count", Some("acme::Foo"), vec![], "the method"),
        ];
        (temp, store, dir, elements)
    }

    fn options() -> CommitOptions {
        CommitOptions {
            message: "first".into(),
            author: "agent".into(),
            source_commit: [0u8; 32],
            pipeline: "test".into(),
            ref_name: "main".into(),
        }
    }

    #[test]
    fn commits_and_reads_back() {
        let (_temp, store, dir, elements) = setup();
        let result = commit(&store, &dir, &elements, &options()).unwrap();
        assert_eq!(result.objects, 2);
        assert_eq!(read_ref(&dir, "main").unwrap(), result.version);

        let (object, design) = lookup_object(&store, &result.version, "acme::Foo::count").unwrap();
        assert_eq!(object.name, "acme::Foo::count");
        assert_eq!(design.text, "the method");

        let version = load_version(&store, &result.version).unwrap();
        let view = view_root(&store, &version, b"namespaces").unwrap().unwrap();
        let mut entries = Vec::new();
        collect_view(&store, &view, &mut entries).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn file_objects_have_no_design_and_live_in_the_files_view() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join(".ds");
        fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        let elements = vec![
            element("acme::Foo", None, vec!["acme::Foo::count"], "the class"),
            element("acme::Foo::count", Some("acme::Foo"), vec![], "the method"),
            file_element("src/a.cpp"),
        ];

        let result = commit(&store, &dir, &elements, &options()).unwrap();
        assert_eq!(result.objects, 3);

        let version = load_version(&store, &result.version).unwrap();
        let namespaces = view_root(&store, &version, b"namespaces").unwrap().unwrap();
        let mut entries = Vec::new();
        collect_view(&store, &namespaces, &mut entries).unwrap();
        assert_eq!(entries.len(), 2);

        let files = view_root(&store, &version, b"files").unwrap().unwrap();
        let mut entries = Vec::new();
        collect_view(&store, &files, &mut entries).unwrap();
        assert_eq!(entries.len(), 3);

        let (object, design) = lookup_object(&store, &result.version, "src/a.cpp").unwrap();
        assert_eq!(object.ty, T_FILE);
        assert_eq!(object.design_hash, ZERO_HASH);
        assert!(design.text.is_empty());
    }

    #[test]
    fn reuses_designs_when_input_hash_is_unchanged() {
        let (_temp, store, dir, elements) = setup();
        let first = commit(&store, &dir, &elements, &options()).unwrap();

        let mut elements = vec![
            element("acme::Foo", None, vec!["acme::Foo::count"], ""),
            element("acme::Foo::count", Some("acme::Foo"), vec![], ""),
        ];
        for element in &mut elements {
            element.design = None;
        }
        let reused = reuse_designs(&store, &first.version, &mut elements).unwrap();
        assert_eq!(reused, 2);
        assert_eq!(elements[0].design.as_deref(), Some("the class"));
        assert_eq!(elements[1].design.as_deref(), Some("the method"));
    }

    #[test]
    fn reuses_unchanged_children_but_not_a_changed_parent() {
        let (_temp, store, dir, elements) = setup();
        let first = commit(&store, &dir, &elements, &options()).unwrap();

        let mut elements = vec![
            element("acme::Foo", None, vec!["acme::Foo::count"], ""),
            element("acme::Foo::count", Some("acme::Foo"), vec![], ""),
        ];
        for element in &mut elements {
            element.design = None;
        }
        elements[0].own_text = "changed class text".into();

        let reused = reuse_designs(&store, &first.version, &mut elements).unwrap();
        assert_eq!(reused, 1);
        assert_eq!(elements[0].design, None);
        assert_eq!(elements[1].design.as_deref(), Some("the method"));
    }

    #[test]
    fn reuses_unchanged_files_without_parsing() {
        let (_temp, store, dir, elements) = setup();
        let first = commit(&store, &dir, &elements, &options()).unwrap();

        let paths: BTreeSet<String> = ["src/a.cpp".to_owned()].into_iter().collect();
        let new_commit = "11".repeat(20);
        let (mut elements, failed) =
            reuse_file_elements(&store, &dir, &first.version, &paths, &new_commit).unwrap();
        assert!(failed.is_empty());
        assert_eq!(elements.len(), 2);
        assert!(elements.iter().all(|element| element.own_text.is_empty()));
        assert!(elements.iter().all(|element| element.design.is_none()));
        assert!(
            elements
                .iter()
                .all(|element| element.content_hash.is_some())
        );

        let class = elements
            .iter()
            .find(|element| element.name == "acme::Foo")
            .unwrap();
        assert_eq!(class.children, vec!["acme::Foo::count".to_owned()]);

        let reused = reuse_designs(&store, &first.version, &mut elements).unwrap();
        assert_eq!(reused, 2);
        assert!(elements.iter().all(|element| element.design.is_some()));

        let second = commit(&store, &dir, &elements, &options()).unwrap();

        for name in ["acme::Foo", "acme::Foo::count"] {
            let (before, _) = lookup_object(&store, &first.version, name).unwrap();
            let (after, design) = lookup_object(&store, &second.version, name).unwrap();
            assert_eq!(before.content_hash, after.content_hash);
            assert_eq!(before.input_hash, after.input_hash);
            assert_eq!(
                design.text,
                format!(
                    "the {}",
                    if name.ends_with("count") {
                        "method"
                    } else {
                        "class"
                    }
                )
            );
        }
    }

    #[test]
    fn editing_a_child_design_changes_the_parent_input() {
        let (_temp, store, dir, elements) = setup();
        let first = commit(&store, &dir, &elements, &options()).unwrap();

        let paths: BTreeSet<String> = ["src/a.cpp".to_owned()].into_iter().collect();
        let new_commit = "22".repeat(20);
        let (mut elements, failed) =
            reuse_file_elements(&store, &dir, &first.version, &paths, &new_commit).unwrap();
        assert!(failed.is_empty());
        for element in &mut elements {
            element.design = Some(if element.name.ends_with("count") {
                "the method v2".to_owned()
            } else {
                "the class v2".to_owned()
            });
        }

        let second = commit(&store, &dir, &elements, &options()).unwrap();

        let (before_class, _) = lookup_object(&store, &first.version, "acme::Foo").unwrap();
        let (after_class, _) = lookup_object(&store, &second.version, "acme::Foo").unwrap();
        assert_eq!(before_class.content_hash, after_class.content_hash);
        assert_ne!(before_class.input_hash, after_class.input_hash);

        let (before_method, _) = lookup_object(&store, &first.version, "acme::Foo::count").unwrap();
        let (after_method, _) = lookup_object(&store, &second.version, "acme::Foo::count").unwrap();
        assert_eq!(before_method.input_hash, after_method.input_hash);
    }

    #[test]
    fn missing_tree_cache_marks_files_for_reparse() {
        let (_temp, store, dir, elements) = setup();
        let first = commit(&store, &dir, &elements, &options()).unwrap();
        fs::remove_dir_all(dir.join("cache")).unwrap();

        let paths: BTreeSet<String> = ["src/a.cpp".to_owned()].into_iter().collect();
        let (elements, failed) =
            reuse_file_elements(&store, &dir, &first.version, &paths, &"33".repeat(20)).unwrap();
        assert!(elements.is_empty());
        assert_eq!(failed, paths);
    }

    #[test]
    fn allows_missing_designs() {
        let (_temp, store, dir, mut elements) = setup();
        elements[0].design = None;
        let result = commit(&store, &dir, &elements, &options()).unwrap();
        assert_eq!(result.missing, 1);
        let (object, _) = lookup_object(&store, &result.version, "acme::Foo").unwrap();
        assert_eq!(object.design_hash, ZERO_HASH);
    }

    #[test]
    fn refuses_inconsistent_parent() {
        let (_temp, store, dir, mut elements) = setup();
        elements[1].parent = Some("acme::Foo".into());
        elements[0].children.clear();
        let error = commit(&store, &dir, &elements, &options()).unwrap_err();
        assert!(error.to_string().contains("parent"));
    }

    #[test]
    fn detects_cycles() {
        let (_temp, store, dir, mut elements) = setup();
        elements[0].parent = Some("acme::Foo::count".into());
        elements[1].children = vec!["acme::Foo".into()];
        let error = commit(&store, &dir, &elements, &options()).unwrap_err();
        assert!(error.to_string().contains("cycle"));
    }

    #[test]
    fn version_parent_is_set_on_second_commit() {
        let (_temp, store, dir, elements) = setup();
        let first = commit(&store, &dir, &elements, &options()).unwrap();

        let second_elements = vec![
            element("acme::Foo", None, vec!["acme::Foo::count"], "the class v2"),
            element("acme::Foo::count", Some("acme::Foo"), vec![], "the method"),
        ];
        let second = commit(&store, &dir, &second_elements, &options()).unwrap();
        let version = load_version(&store, &second.version).unwrap();
        assert_eq!(version.parent, first.version);
    }

    #[test]
    fn commit_hex_accepts_sha1_and_sha256() {
        let sha1 = parse_commit_hex(&"a".repeat(40)).unwrap();
        assert_eq!(&sha1[..12], &[0u8; 12]);
        assert_eq!(sha1[12], 0xaa);
        let sha256 = parse_commit_hex(&"b".repeat(64)).unwrap();
        assert_eq!(sha256, [0xbbu8; 32]);
    }
}
