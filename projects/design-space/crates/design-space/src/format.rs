//! Canonical binary encoding for design-space records.
//!
//! The format mirrors `docs/storage-format.md`:
//!
//! - DSO1: design object record;
//! - CMT1: commit record;
//! - docs/symbols are raw UTF-8 text files and have no record header.
//!
//! All integers are little-endian. Hashes are raw 32-byte values; zero is the
//! None sentinel for optional hash fields.

use crate::model::*;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub const FORMAT_VERSION: u8 = 1;
pub const DSO_MAGIC: &[u8; 4] = b"DSO1";
pub const CMT_MAGIC: &[u8; 4] = b"CMT1";

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("unexpected end of input")]
    UnexpectedEof,
    #[error("trailing bytes after record")]
    TrailingBytes,
    #[error("invalid magic")]
    InvalidMagic,
    #[error("unsupported format version: {0}")]
    UnsupportedVersion(u8),
    #[error("invalid kind: {0:#04x}")]
    InvalidKind(u8),
    #[error("invalid {0} tag: {1:#04x}")]
    InvalidTag(&'static str, u8),
    #[error("invalid UTF-8 in {0}")]
    InvalidUtf8(&'static str),
    #[error("invalid value in {0}")]
    InvalidValue(&'static str),
    #[error("required hash is zero: {0}")]
    RequiredHashZero(&'static str),
    #[error("object kind and payload do not match")]
    KindMismatch,
    #[error("list is not sorted or contains duplicates: {0}")]
    NonCanonicalList(&'static str),
    #[error("hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
}

pub type Result<T> = std::result::Result<T, FormatError>;

// ---------------------------------------------------------------------------
// Hashing
// ---------------------------------------------------------------------------

pub fn hash_bytes(bytes: &[u8]) -> Hash32 {
    Hash32(Sha256::digest(bytes).into())
}

pub fn object_hash(object: &DesignObject) -> Result<ObjectHash> {
    validate_object(object)?;
    let payload = encode_payload(object)?;
    let mut hasher = Sha256::new();
    hasher.update([FORMAT_VERSION]);
    hasher.update([object.kind.as_byte()]);
    hasher.update(payload);
    Ok(ObjectHash(Hash32(hasher.finalize().into())))
}

/// Validate the local structural invariants of a design object.
///
/// Cross-object checks (parent/child consistency, symbol/doc existence, and
/// relationship closure) belong to the workspace layer.
pub fn validate_object(object: &DesignObject) -> Result<()> {
    if !object.check_kind() {
        return Err(FormatError::KindMismatch);
    }

    match &object.payload {
        ObjectPayload::View(p) => validate_view(p),
        ObjectPayload::Directory(p) => {
            require_hash(p.qualified_name, "directory qualified_name")?;
            validate_parent_ref(p.parent)?;
            validate_opt_path_source(p.source)?;
            validate_object_hashes(&p.children, "directory children")?;
            Ok(())
        }
        ObjectPayload::File(p) => {
            require_hash(p.qualified_name, "file qualified_name")?;
            require_hash(p.parent, "file parent")?;
            validate_opt_path_source(p.source)?;
            validate_content_hash(p.content_hash, p.source.is_some(), false, "file")?;
            validate_object_hashes(&p.children, "file children")?;
            Ok(())
        }
        ObjectPayload::Namespace(p) => {
            require_hash(p.qualified_name, "namespace qualified_name")?;
            validate_parent_ref(p.parent)?;
            validate_opt_span_source(p.source)?;
            validate_content_hash(p.content_hash, p.source.is_some(), false, "namespace")?;
            validate_object_hashes(&p.children, "namespace children")?;
            Ok(())
        }
        ObjectPayload::Class(p) => validate_class_payload(p, true),
        ObjectPayload::InnerClass(p) => validate_class_payload(p, false),
        ObjectPayload::Struct(p) => validate_struct_payload(p, true),
        ObjectPayload::InnerStruct(p) => validate_struct_payload(p, false),
        ObjectPayload::Enum(p) => validate_enum_payload(p, true),
        ObjectPayload::InnerEnum(p) => validate_enum_payload(p, false),
        ObjectPayload::Interface(p) => validate_interface_payload(p, true),
        ObjectPayload::InnerInterface(p) => validate_interface_payload(p, false),
        ObjectPayload::Method(p) => {
            require_hash(p.qualified_name, "method qualified_name")?;
            require_hash(p.parent, "method parent")?;
            validate_opt_span_source(p.source)?;
            validate_content_hash(p.content_hash, p.source.is_some(), false, "method")?;
            validate_dependencies(&p.explicit_dependencies, "method explicit dependencies")?;
            validate_dependencies(&p.implicit_dependencies, "method implicit dependencies")?;
            Ok(())
        }
        ObjectPayload::Function(p) => {
            require_hash(p.qualified_name, "function qualified_name")?;
            require_hash(p.namespace, "function namespace")?;
            validate_opt_span_source(p.source)?;
            validate_content_hash(p.content_hash, p.source.is_some(), false, "function")?;
            validate_dependencies(&p.explicit_dependencies, "function explicit dependencies")?;
            validate_dependencies(&p.implicit_dependencies, "function implicit dependencies")?;
            Ok(())
        }
        ObjectPayload::Part(p) => {
            require_hash(p.qualified_name, "part qualified_name")?;
            require_hash(p.parent, "part parent")?;
            validate_span_source(p.source, "part source")?;
            validate_content_hash(p.content_hash, true, true, "part")?;
            validate_dependencies(&p.explicit_dependencies, "part explicit dependencies")?;
            validate_dependencies(&p.implicit_dependencies, "part implicit dependencies")?;
            validate_object_hashes(&p.children, "part children")?;
            Ok(())
        }
        ObjectPayload::Mixin(p) => {
            require_hash(p.qualified_name, "mixin qualified_name")?;
            validate_class_parent(p.parent)?;
            validate_opt_span_source(p.source)?;
            validate_content_hash(p.content_hash, p.source.is_some(), false, "mixin")?;
            validate_symbol_list(&p.on, "mixin on", true)?;
            validate_symbol_list(&p.interfaces, "mixin interfaces", true)?;
            validate_symbol_list(&p.mixins, "mixin mixins", false)?;
            validate_dependencies(&p.explicit_dependencies, "mixin explicit dependencies")?;
            validate_dependencies(&p.implicit_dependencies, "mixin implicit dependencies")?;
            validate_object_hashes(&p.children, "mixin children")?;
            Ok(())
        }
        ObjectPayload::DocumentDirectory(p) => {
            require_hash(p.qualified_name, "document directory qualified_name")?;
            validate_parent_ref(p.parent)?;
            validate_object_hashes(&p.children, "document directory children")?;
            validate_comments(&p.comments, "document directory comments")?;
            Ok(())
        }
        ObjectPayload::Document(p) => {
            require_hash(p.qualified_name, "document qualified_name")?;
            require_hash(p.parent, "document parent")?;
            validate_text(&p.title, "document title")?;
            validate_ref_list(&p.sections, "document sections")?;
            validate_object_hashes(&p.children, "document children")?;
            validate_comments(&p.comments, "document comments")?;
            Ok(())
        }
        ObjectPayload::DocumentSection(p) => {
            require_hash(p.qualified_name, "document section qualified_name")?;
            require_hash(p.parent, "document section parent")?;
            validate_text(&p.title, "document section title")?;
            validate_ref_list(&p.body, "document section body")?;
            validate_ref_list(&p.sections, "document section sections")?;
            validate_object_hashes(&p.children, "document section children")?;
            validate_comments(&p.comments, "document section comments")?;
            Ok(())
        }
        ObjectPayload::DocumentList(p) => {
            require_hash(p.qualified_name, "document list qualified_name")?;
            require_hash(p.parent, "document list parent")?;
            validate_ref_list(&p.items, "document list items")?;
            validate_object_hashes(&p.children, "document list children")?;
            validate_comments(&p.comments, "document list comments")?;
            Ok(())
        }
        ObjectPayload::DocumentText(p) => {
            require_hash(p.qualified_name, "document text qualified_name")?;
            require_hash(p.parent, "document text parent")?;
            validate_text(&p.text, "document text")?;
            validate_comments(&p.comments, "document text comments")?;
            Ok(())
        }
        ObjectPayload::DocumentTopic(p) => {
            require_hash(p.qualified_name, "document topic qualified_name")?;
            require_hash(p.parent, "document topic parent")?;
            validate_text(&p.title, "document topic title")?;
            validate_ref_list(&p.paragraphs, "document topic paragraphs")?;
            validate_object_hashes(&p.children, "document topic children")?;
            validate_comments(&p.comments, "document topic comments")?;
            Ok(())
        }
        ObjectPayload::Block(p) => {
            require_hash(p.qualified_name, "block qualified_name")?;
            require_hash(p.parent, "block parent")?;
            validate_text_allow_empty(&p.language, "block language")?;
            validate_multiline_text(&p.text, "block text")?;
            validate_comments(&p.comments, "block comments")?;
            Ok(())
        }
        ObjectPayload::MermaidBlock(p) => {
            require_hash(p.qualified_name, "mermaid block qualified_name")?;
            require_hash(p.parent, "mermaid block parent")?;
            validate_multiline_text(&p.text, "mermaid block text")?;
            validate_comments(&p.comments, "mermaid block comments")?;
            Ok(())
        }
        ObjectPayload::HtmlBlock(p) => {
            require_hash(p.qualified_name, "html block qualified_name")?;
            require_hash(p.parent, "html block parent")?;
            validate_multiline_text(&p.text, "html block text")?;
            validate_comments(&p.comments, "html block comments")?;
            Ok(())
        }
        ObjectPayload::DesignStructure(p) => {
            require_hash(p.qualified_name, "design structure qualified_name")?;
            validate_parent_ref(p.parent)?;
            validate_text(&p.title, "design structure title")?;
            validate_object_hashes(&p.children, "design structure children")?;
            Ok(())
        }
        ObjectPayload::System(p) => {
            require_hash(p.qualified_name, "system qualified_name")?;
            require_hash(p.parent, "system parent")?;
            validate_text(&p.title, "system title")?;
            validate_symbol_list(&p.members, "system members", true)?;
            validate_object_hashes(&p.children, "system children")?;
            Ok(())
        }
        ObjectPayload::Module(p) => {
            require_hash(p.qualified_name, "module qualified_name")?;
            require_hash(p.parent, "module parent")?;
            validate_text(&p.title, "module title")?;
            validate_symbol_list(&p.members, "module members", true)?;
            validate_object_hashes(&p.children, "module children")?;
            Ok(())
        }
        ObjectPayload::ConceptDirectory(p) => {
            require_hash(p.qualified_name, "concept directory qualified_name")?;
            validate_parent_ref(p.parent)?;
            validate_object_hashes(p.children.as_slice(), "concept directory children")?;
            Ok(())
        }
        ObjectPayload::Concept(p) => {
            require_hash(p.qualified_name, "concept qualified_name")?;
            require_hash(p.parent, "concept parent")?;
            validate_text(&p.name, "concept name")?;
            validate_text(&p.summary, "concept summary")?;
            validate_text_allow_empty(&p.explanation, "concept explanation")?;
            validate_string_list(&p.characteristics, "concept characteristics")?;
            validate_string_list(&p.relations, "concept relations")?;
            validate_string_list(&p.aliases, "concept aliases")?;
            Ok(())
        }
        ObjectPayload::Layer(p) => {
            require_hash(p.qualified_name, "layer qualified_name")?;
            require_hash(p.parent, "layer parent")?;
            validate_text(&p.title, "layer title")?;
            if p.order == 0 {
                return Err(FormatError::InvalidValue("layer order"));
            }
            validate_symbol_list(&p.members, "layer members", true)?;
            validate_object_hashes(&p.children, "layer children")?;
            Ok(())
        }
    }
}

fn validate_view(p: &ViewPayload) -> Result<()> {
    require_hash(p.files, "view files")?;
    require_hash(p.namespaces, "view namespaces")?;
    // `documents` may be zero in views written before the document root
    // existed; `design` and `concepts` may be zero until those spaces exist.
    if p.documents.is_zero() && !p.concepts.is_zero() {
        return Err(FormatError::InvalidValue("view concepts without documents"));
    }
    Ok(())
}

fn validate_parent_ref(parent: ParentRef) -> Result<()> {
    match parent {
        ParentRef::View => Ok(()),
        ParentRef::Object(hash) => require_hash(hash, "parent"),
    }
}

fn validate_class_payload(p: &ClassPayload, top_level: bool) -> Result<()> {
    require_hash(p.qualified_name, "class qualified_name")?;
    validate_type_parent_form(p.parent, top_level, "class")?;
    validate_opt_span_source(p.source)?;
    validate_content_hash(p.content_hash, p.source.is_some(), false, "class")?;
    validate_symbol_list(&p.interfaces, "class interfaces", true)?;
    validate_symbol_list(&p.mixins, "class mixins", false)?;
    validate_dependencies(&p.explicit_dependencies, "class explicit dependencies")?;
    validate_dependencies(&p.implicit_dependencies, "class implicit dependencies")?;
    validate_object_hashes(&p.children, "class children")?;
    Ok(())
}

fn validate_struct_payload(p: &StructPayload, top_level: bool) -> Result<()> {
    require_hash(p.qualified_name, "struct qualified_name")?;
    validate_type_parent_form(p.parent, top_level, "struct")?;
    validate_opt_span_source(p.source)?;
    validate_content_hash(p.content_hash, p.source.is_some(), false, "struct")?;
    validate_dependencies(&p.explicit_dependencies, "struct explicit dependencies")?;
    validate_dependencies(&p.implicit_dependencies, "struct implicit dependencies")?;
    validate_object_hashes(&p.children, "struct children")?;
    Ok(())
}

fn validate_enum_payload(p: &EnumPayload, top_level: bool) -> Result<()> {
    require_hash(p.qualified_name, "enum qualified_name")?;
    validate_type_parent_form(p.parent, top_level, "enum")?;
    validate_opt_span_source(p.source)?;
    validate_content_hash(p.content_hash, p.source.is_some(), false, "enum")?;
    validate_dependencies(&p.explicit_dependencies, "enum explicit dependencies")?;
    validate_dependencies(&p.implicit_dependencies, "enum implicit dependencies")?;
    validate_object_hashes(&p.children, "enum children")?;
    Ok(())
}

fn validate_interface_payload(p: &InterfacePayload, top_level: bool) -> Result<()> {
    require_hash(p.qualified_name, "interface qualified_name")?;
    validate_type_parent_form(p.parent, top_level, "interface")?;
    validate_opt_span_source(p.source)?;
    validate_content_hash(p.content_hash, p.source.is_some(), false, "interface")?;
    validate_dependencies(&p.explicit_dependencies, "interface explicit dependencies")?;
    validate_dependencies(&p.implicit_dependencies, "interface implicit dependencies")?;
    validate_object_hashes(&p.children, "interface children")?;
    Ok(())
}

fn validate_type_parent_form(
    parent: ClassParent,
    top_level: bool,
    _what: &'static str,
) -> Result<()> {
    match (top_level, parent) {
        (true, ClassParent::TopLevel { namespace, .. }) => {
            require_hash(namespace, "top-level namespace parent")
        }
        (false, ClassParent::Nested { parent }) => require_hash(parent, "nested parent"),
        (true, ClassParent::Nested { .. }) => Err(FormatError::InvalidValue(
            "top-level type has a nested parent",
        )),
        (false, ClassParent::TopLevel { .. }) => Err(FormatError::InvalidValue(
            "inner type has a top-level parent",
        )),
    }
}

fn validate_class_parent(parent: ClassParent) -> Result<()> {
    match parent {
        ClassParent::TopLevel {
            file: _,
            namespace,
            module: _,
        } => require_hash(namespace, "top-level namespace parent"),
        ClassParent::Nested { parent } => require_hash(parent, "nested parent"),
    }
}

fn validate_opt_path_source(source: Option<PathSource>) -> Result<()> {
    if let Some(source) = source {
        require_hash(source.path, "source.path")?;
    }
    Ok(())
}

fn validate_opt_span_source(source: Option<SpanSource>) -> Result<()> {
    if let Some(source) = source {
        validate_span_source(source, "source")?;
    }
    Ok(())
}

fn validate_span_source(source: SpanSource, what: &'static str) -> Result<()> {
    require_hash(source.path, "source.path")?;
    if !source.span.is_valid() {
        return Err(FormatError::InvalidValue(what));
    }
    Ok(())
}

fn validate_content_hash(
    hash: Hash32,
    has_source: bool,
    required: bool,
    what: &'static str,
) -> Result<()> {
    if required && hash.is_zero() {
        return Err(FormatError::RequiredHashZero(what));
    }
    if !hash.is_zero() && !has_source {
        return Err(FormatError::InvalidValue(what));
    }
    if has_source && hash.is_zero() {
        return Err(FormatError::InvalidValue(what));
    }
    Ok(())
}

fn require_hash<T: IsZeroHash>(hash: T, what: &'static str) -> Result<()> {
    if hash.is_zero_hash() {
        Err(FormatError::RequiredHashZero(what))
    } else {
        Ok(())
    }
}

fn validate_symbol_list(hashes: &[SymbolHash], what: &'static str, sorted: bool) -> Result<()> {
    let mut previous: Option<SymbolHash> = None;
    let mut seen = HashSet::with_capacity(hashes.len());
    for hash in hashes {
        require_hash(*hash, what)?;
        if !seen.insert(hash.0.0) {
            return Err(FormatError::NonCanonicalList(what));
        }
        if let Some(previous) = previous
            && sorted
            && previous.0.0 >= hash.0.0
        {
            return Err(FormatError::NonCanonicalList(what));
        }
        previous = Some(*hash);
    }
    Ok(())
}

fn validate_object_hashes(hashes: &[ObjectHash], what: &'static str) -> Result<()> {
    let mut previous: Option<ObjectHash> = None;
    for hash in hashes {
        require_hash(*hash, what)?;
        if let Some(previous) = previous
            && previous.0.0 >= hash.0.0
        {
            return Err(FormatError::NonCanonicalList(what));
        }
        previous = Some(*hash);
    }
    Ok(())
}

fn validate_ref_list(hashes: &[SymbolHash], what: &'static str) -> Result<()> {
    let mut seen = HashSet::with_capacity(hashes.len());
    for hash in hashes {
        require_hash(*hash, what)?;
        if !seen.insert(hash.0.0) {
            return Err(FormatError::NonCanonicalList(what));
        }
    }
    Ok(())
}

fn validate_text(text: &str, what: &'static str) -> Result<()> {
    if text.is_empty() || text.contains('\n') || text.contains('\r') {
        return Err(FormatError::InvalidValue(what));
    }
    Ok(())
}

fn validate_text_allow_empty(text: &str, what: &'static str) -> Result<()> {
    if text.contains('\n') || text.contains('\r') {
        return Err(FormatError::InvalidValue(what));
    }
    Ok(())
}

fn validate_multiline_text(text: &str, what: &'static str) -> Result<()> {
    if text.is_empty() {
        return Err(FormatError::InvalidValue(what));
    }
    Ok(())
}

fn validate_string_list(items: &[String], what: &'static str) -> Result<()> {
    for item in items {
        validate_text(item, what)?;
    }
    Ok(())
}

fn validate_comments(comments: &[DocComment], what: &'static str) -> Result<()> {
    let mut previous = 0u32;
    for comment in comments {
        if comment.id == 0 || comment.id <= previous {
            return Err(FormatError::NonCanonicalList(what));
        }
        if comment.text.is_empty() {
            return Err(FormatError::InvalidValue(what));
        }
        previous = comment.id;
    }
    Ok(())
}

fn validate_dependencies(deps: &[Dependency], what: &'static str) -> Result<()> {
    let mut previous: Option<SymbolHash> = None;
    let mut seen = HashSet::with_capacity(deps.len());
    for dep in deps {
        require_hash(dep.target, what)?;
        if !seen.insert(dep.target.0.0) {
            return Err(FormatError::NonCanonicalList(what));
        }
        if let Some(evidence) = dep.evidence {
            require_hash(evidence.path, what)?;
            if !evidence.span.is_valid() {
                return Err(FormatError::InvalidValue(what));
            }
        }
        if let Some(previous) = previous
            && previous.0.0 >= dep.target.0.0
        {
            return Err(FormatError::NonCanonicalList(what));
        }
        previous = Some(dep.target);
    }
    Ok(())
}

trait IsZeroHash {
    fn is_zero_hash(&self) -> bool;
}

macro_rules! impl_zero_hash {
    ($($name:ident),+ $(,)?) => {
        $(
            impl IsZeroHash for $name {
                fn is_zero_hash(&self) -> bool {
                    self.is_zero()
                }
            }
        )+
    };
}

impl_zero_hash!(ObjectHash, CommitHash, DocHash, SymbolHash);

pub fn commit_hash(commit: &Commit) -> CommitHash {
    let payload = encode_commit_payload(commit);
    let mut hasher = Sha256::new();
    hasher.update([FORMAT_VERSION]);
    hasher.update(payload);
    CommitHash(Hash32(hasher.finalize().into()))
}

pub fn validate_commit(commit: &Commit) -> Result<()> {
    if commit.view.is_zero() {
        return Err(FormatError::RequiredHashZero("commit view"));
    }
    Ok(())
}

/// Content hash of an exact source fragment. No normalization is performed.
pub fn content_hash(bytes: &[u8]) -> Hash32 {
    hash_bytes(bytes)
}

pub fn doc_hash(text: &str) -> DocHash {
    DocHash(hash_bytes(text.as_bytes()))
}

pub fn symbol_hash(text: &str) -> SymbolHash {
    SymbolHash(hash_bytes(text.as_bytes()))
}

// ---------------------------------------------------------------------------
// DSO1
// ---------------------------------------------------------------------------

pub fn encode_object(object: &DesignObject) -> Result<Vec<u8>> {
    validate_object(object)?;
    let payload = encode_payload(object)?;
    let mut out = Vec::with_capacity(6 + payload.len());
    out.extend_from_slice(DSO_MAGIC);
    out.push(FORMAT_VERSION);
    out.push(object.kind.as_byte());
    out.extend_from_slice(&payload);
    Ok(out)
}

pub fn decode_object(bytes: &[u8]) -> Result<DesignObject> {
    let mut reader = Reader::new(bytes);
    if reader.read(4)? != DSO_MAGIC {
        return Err(FormatError::InvalidMagic);
    }
    let version = reader.read_u8()?;
    if version != FORMAT_VERSION {
        return Err(FormatError::UnsupportedVersion(version));
    }
    let kind_byte = reader.read_u8()?;
    let kind = Kind::from_byte(kind_byte).ok_or(FormatError::InvalidKind(kind_byte))?;
    let payload = decode_payload(kind, &mut reader)?;
    if !reader.is_empty() {
        return Err(FormatError::TrailingBytes);
    }
    let object = DesignObject { kind, payload };
    if !object.check_kind() {
        return Err(FormatError::KindMismatch);
    }
    validate_object(&object)?;
    Ok(object)
}

/// Verify an encoded DSO1 record against the expected object hash.
pub fn decode_object_checked(bytes: &[u8], expected: ObjectHash) -> Result<DesignObject> {
    let object = decode_object(bytes)?;
    let actual = object_hash(&object)?;
    if actual != expected {
        return Err(FormatError::HashMismatch {
            expected: expected.to_hex(),
            actual: actual.to_hex(),
        });
    }
    Ok(object)
}

fn encode_payload(object: &DesignObject) -> Result<Vec<u8>> {
    if !object.check_kind() {
        return Err(FormatError::KindMismatch);
    }
    let mut out = Vec::new();
    match &object.payload {
        ObjectPayload::View(p) => encode_view(p, &mut out),
        ObjectPayload::Directory(p) => encode_directory(p, &mut out),
        ObjectPayload::File(p) => encode_file(p, &mut out),
        ObjectPayload::Namespace(p) => encode_namespace(p, &mut out),
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => encode_class(p, &mut out),
        ObjectPayload::Struct(p) | ObjectPayload::InnerStruct(p) => encode_struct(p, &mut out),
        ObjectPayload::Enum(p) | ObjectPayload::InnerEnum(p) => encode_enum(p, &mut out),
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => {
            encode_interface(p, &mut out)
        }
        ObjectPayload::Method(p) => encode_method(p, &mut out),
        ObjectPayload::Function(p) => encode_function(p, &mut out),
        ObjectPayload::Part(p) => encode_part(p, &mut out),
        ObjectPayload::Mixin(p) => encode_mixin(p, &mut out),
        ObjectPayload::DocumentDirectory(p) => encode_document_directory(p, &mut out),
        ObjectPayload::Document(p) => encode_document(p, &mut out),
        ObjectPayload::DocumentSection(p) => encode_document_section(p, &mut out),
        ObjectPayload::DocumentList(p) => encode_document_list(p, &mut out),
        ObjectPayload::DocumentText(p) => encode_document_text(p, &mut out),
        ObjectPayload::DocumentTopic(p) => encode_document_topic(p, &mut out),
        ObjectPayload::Block(p) => encode_block(p, &mut out),
        ObjectPayload::MermaidBlock(p) => encode_mermaid_block(p, &mut out),
        ObjectPayload::HtmlBlock(p) => encode_html_block(p, &mut out),
        ObjectPayload::DesignStructure(p) => encode_design_structure(p, &mut out),
        ObjectPayload::System(p) => encode_system(p, &mut out),
        ObjectPayload::Module(p) => encode_module(p, &mut out),
        ObjectPayload::ConceptDirectory(p) => encode_concept_directory(p, &mut out),
        ObjectPayload::Concept(p) => encode_concept(p, &mut out),
        ObjectPayload::Layer(p) => encode_layer(p, &mut out),
    }
    Ok(out)
}

fn decode_payload(kind: Kind, reader: &mut Reader<'_>) -> Result<ObjectPayload> {
    Ok(match kind {
        Kind::View => ObjectPayload::View(decode_view(reader)?),
        Kind::Directory => ObjectPayload::Directory(decode_directory(reader)?),
        Kind::File => ObjectPayload::File(decode_file(reader)?),
        Kind::Namespace => ObjectPayload::Namespace(decode_namespace(reader)?),
        Kind::Class => ObjectPayload::Class(decode_class(reader)?),
        Kind::InnerClass => ObjectPayload::InnerClass(decode_class(reader)?),
        Kind::Struct => ObjectPayload::Struct(decode_struct(reader)?),
        Kind::InnerStruct => ObjectPayload::InnerStruct(decode_struct(reader)?),
        Kind::Enum => ObjectPayload::Enum(decode_enum(reader)?),
        Kind::InnerEnum => ObjectPayload::InnerEnum(decode_enum(reader)?),
        Kind::Interface => ObjectPayload::Interface(decode_interface(reader)?),
        Kind::InnerInterface => ObjectPayload::InnerInterface(decode_interface(reader)?),
        Kind::Method => ObjectPayload::Method(decode_method(reader)?),
        Kind::Function => ObjectPayload::Function(decode_function(reader)?),
        Kind::Part => ObjectPayload::Part(decode_part(reader)?),
        Kind::Mixin => ObjectPayload::Mixin(decode_mixin(reader)?),
        Kind::DocumentDirectory => {
            ObjectPayload::DocumentDirectory(decode_document_directory(reader)?)
        }
        Kind::Document => ObjectPayload::Document(decode_document(reader)?),
        Kind::DocumentSection => ObjectPayload::DocumentSection(decode_document_section(reader)?),
        Kind::DocumentList => ObjectPayload::DocumentList(decode_document_list(reader)?),
        Kind::DocumentText => ObjectPayload::DocumentText(decode_document_text(reader)?),
        Kind::DocumentTopic => ObjectPayload::DocumentTopic(decode_document_topic(reader)?),
        Kind::Block => ObjectPayload::Block(decode_block(reader)?),
        Kind::MermaidBlock => ObjectPayload::MermaidBlock(decode_mermaid_block(reader)?),
        Kind::HtmlBlock => ObjectPayload::HtmlBlock(decode_html_block(reader)?),
        Kind::DesignStructure => ObjectPayload::DesignStructure(decode_design_structure(reader)?),
        Kind::System => ObjectPayload::System(decode_system(reader)?),
        Kind::Module => ObjectPayload::Module(decode_module(reader)?),
        Kind::ConceptDirectory => {
            ObjectPayload::ConceptDirectory(decode_concept_directory(reader)?)
        }
        Kind::Concept => ObjectPayload::Concept(decode_concept(reader)?),
        Kind::Layer => ObjectPayload::Layer(decode_layer(reader)?),
    })
}

fn encode_view(p: &ViewPayload, out: &mut Vec<u8>) {
    out.extend_from_slice(&p.source_commit.0);
    put_hash(out, p.files);
    put_hash(out, p.namespaces);
    put_hash(out, p.design);
    // Views written before the document root existed end here. Omit the
    // optional tail so their canonical hash stays stable.
    if !p.documents.is_zero() {
        put_hash(out, p.documents);
        if !p.concepts.is_zero() {
            put_hash(out, p.concepts);
        }
    }
}

fn decode_view(r: &mut Reader<'_>) -> Result<ViewPayload> {
    let source_commit = GitOid(read_32(r)?);
    let files = ObjectHash(Hash32(read_32(r)?));
    let namespaces = ObjectHash(Hash32(read_32(r)?));
    let design = ObjectHash(Hash32(read_32(r)?));
    // Older views predate the document root and the concept space.
    let documents = if r.is_empty() {
        ObjectHash::ZERO
    } else {
        ObjectHash(Hash32(read_32(r)?))
    };
    let concepts = if r.is_empty() {
        ObjectHash::ZERO
    } else {
        ObjectHash(Hash32(read_32(r)?))
    };
    Ok(ViewPayload {
        source_commit,
        files,
        namespaces,
        design,
        documents,
        concepts,
    })
}

fn encode_directory(p: &DirectoryPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    encode_parent_ref(out, p.parent);
    encode_opt_path_source(out, p.source);
    put_hash(out, p.document);
    encode_object_hashes(out, &p.children);
}

fn decode_directory(r: &mut Reader<'_>) -> Result<DirectoryPayload> {
    Ok(DirectoryPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: decode_parent_ref(r)?,
        source: decode_opt_path_source(r)?,
        document: SymbolHash(Hash32(read_32(r)?)),
        children: decode_object_hashes(r)?,
    })
}

fn encode_file(p: &FilePayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.parent);
    encode_opt_path_source(out, p.source);
    put_raw_hash(out, p.content_hash);
    put_hash(out, p.document);
    encode_object_hashes(out, &p.children);
}

fn decode_file(r: &mut Reader<'_>) -> Result<FilePayload> {
    Ok(FilePayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: SymbolHash(Hash32(read_32(r)?)),
        source: decode_opt_path_source(r)?,
        content_hash: Hash32(read_32(r)?),
        document: SymbolHash(Hash32(read_32(r)?)),
        children: decode_object_hashes(r)?,
    })
}

fn encode_namespace(p: &NamespacePayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    encode_parent_ref(out, p.parent);
    encode_opt_span_source(out, p.source);
    put_raw_hash(out, p.content_hash);
    put_hash(out, p.document);
    encode_object_hashes(out, &p.children);
}

fn decode_namespace(r: &mut Reader<'_>) -> Result<NamespacePayload> {
    Ok(NamespacePayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: decode_parent_ref(r)?,
        source: decode_opt_span_source(r)?,
        content_hash: Hash32(read_32(r)?),
        document: SymbolHash(Hash32(read_32(r)?)),
        children: decode_object_hashes(r)?,
    })
}

fn encode_class(p: &ClassPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    encode_class_parent(out, p.parent);
    encode_opt_span_source(out, p.source);
    put_raw_hash(out, p.content_hash);
    put_hash(out, p.document);
    put_hash(out, p.superclass);
    encode_symbols(out, &p.interfaces);
    encode_symbols(out, &p.mixins);
    encode_dependencies(out, &p.explicit_dependencies);
    encode_dependencies(out, &p.implicit_dependencies);
    encode_object_hashes(out, &p.children);
}

fn decode_class(r: &mut Reader<'_>) -> Result<ClassPayload> {
    Ok(ClassPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: decode_class_parent(r)?,
        source: decode_opt_span_source(r)?,
        content_hash: Hash32(read_32(r)?),
        document: SymbolHash(Hash32(read_32(r)?)),
        superclass: SymbolHash(Hash32(read_32(r)?)),
        interfaces: decode_symbols(r)?,
        mixins: decode_symbols(r)?,
        explicit_dependencies: decode_dependencies(r)?,
        implicit_dependencies: decode_dependencies(r)?,
        children: decode_object_hashes(r)?,
    })
}

fn encode_struct(p: &StructPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    encode_class_parent(out, p.parent);
    encode_opt_span_source(out, p.source);
    put_raw_hash(out, p.content_hash);
    put_hash(out, p.document);
    encode_dependencies(out, &p.explicit_dependencies);
    encode_dependencies(out, &p.implicit_dependencies);
    encode_object_hashes(out, &p.children);
}

fn decode_struct(r: &mut Reader<'_>) -> Result<StructPayload> {
    Ok(StructPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: decode_class_parent(r)?,
        source: decode_opt_span_source(r)?,
        content_hash: Hash32(read_32(r)?),
        document: SymbolHash(Hash32(read_32(r)?)),
        explicit_dependencies: decode_dependencies(r)?,
        implicit_dependencies: decode_dependencies(r)?,
        children: decode_object_hashes(r)?,
    })
}

fn encode_enum(p: &EnumPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    encode_class_parent(out, p.parent);
    encode_opt_span_source(out, p.source);
    put_raw_hash(out, p.content_hash);
    put_hash(out, p.document);
    encode_dependencies(out, &p.explicit_dependencies);
    encode_dependencies(out, &p.implicit_dependencies);
    encode_object_hashes(out, &p.children);
}

fn decode_enum(r: &mut Reader<'_>) -> Result<EnumPayload> {
    Ok(EnumPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: decode_class_parent(r)?,
        source: decode_opt_span_source(r)?,
        content_hash: Hash32(read_32(r)?),
        document: SymbolHash(Hash32(read_32(r)?)),
        explicit_dependencies: decode_dependencies(r)?,
        implicit_dependencies: decode_dependencies(r)?,
        children: decode_object_hashes(r)?,
    })
}

fn encode_interface(p: &InterfacePayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    encode_class_parent(out, p.parent);
    encode_opt_span_source(out, p.source);
    put_raw_hash(out, p.content_hash);
    put_hash(out, p.document);
    put_hash(out, p.extends);
    encode_dependencies(out, &p.explicit_dependencies);
    encode_dependencies(out, &p.implicit_dependencies);
    encode_object_hashes(out, &p.children);
}

fn decode_interface(r: &mut Reader<'_>) -> Result<InterfacePayload> {
    Ok(InterfacePayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: decode_class_parent(r)?,
        source: decode_opt_span_source(r)?,
        content_hash: Hash32(read_32(r)?),
        document: SymbolHash(Hash32(read_32(r)?)),
        extends: SymbolHash(Hash32(read_32(r)?)),
        explicit_dependencies: decode_dependencies(r)?,
        implicit_dependencies: decode_dependencies(r)?,
        children: decode_object_hashes(r)?,
    })
}

fn encode_method(p: &MethodPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.parent);
    encode_opt_span_source(out, p.source);
    put_raw_hash(out, p.content_hash);
    put_hash(out, p.document);
    put_hash(out, p.overrides);
    encode_dependencies(out, &p.explicit_dependencies);
    encode_dependencies(out, &p.implicit_dependencies);
}

fn decode_method(r: &mut Reader<'_>) -> Result<MethodPayload> {
    Ok(MethodPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: SymbolHash(Hash32(read_32(r)?)),
        source: decode_opt_span_source(r)?,
        content_hash: Hash32(read_32(r)?),
        document: SymbolHash(Hash32(read_32(r)?)),
        overrides: SymbolHash(Hash32(read_32(r)?)),
        explicit_dependencies: decode_dependencies(r)?,
        implicit_dependencies: decode_dependencies(r)?,
    })
}

fn encode_function(p: &FunctionPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.file);
    put_hash(out, p.namespace);
    put_hash(out, p.module);
    encode_opt_span_source(out, p.source);
    put_raw_hash(out, p.content_hash);
    put_hash(out, p.document);
    encode_dependencies(out, &p.explicit_dependencies);
    encode_dependencies(out, &p.implicit_dependencies);
}

fn decode_function(r: &mut Reader<'_>) -> Result<FunctionPayload> {
    Ok(FunctionPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        file: SymbolHash(Hash32(read_32(r)?)),
        namespace: SymbolHash(Hash32(read_32(r)?)),
        module: SymbolHash(Hash32(read_32(r)?)),
        source: decode_opt_span_source(r)?,
        content_hash: Hash32(read_32(r)?),
        document: SymbolHash(Hash32(read_32(r)?)),
        explicit_dependencies: decode_dependencies(r)?,
        implicit_dependencies: decode_dependencies(r)?,
    })
}

fn encode_part(p: &PartPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.parent);
    encode_span_source(out, p.source);
    put_raw_hash(out, p.content_hash);
    encode_dependencies(out, &p.explicit_dependencies);
    encode_dependencies(out, &p.implicit_dependencies);
    encode_object_hashes(out, &p.children);
}

fn decode_part(r: &mut Reader<'_>) -> Result<PartPayload> {
    Ok(PartPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: SymbolHash(Hash32(read_32(r)?)),
        source: decode_span_source(r)?,
        content_hash: Hash32(read_32(r)?),
        explicit_dependencies: decode_dependencies(r)?,
        implicit_dependencies: decode_dependencies(r)?,
        children: decode_object_hashes(r)?,
    })
}

fn encode_mixin(p: &MixinPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    encode_class_parent(out, p.parent);
    encode_opt_span_source(out, p.source);
    put_raw_hash(out, p.content_hash);
    put_hash(out, p.document);
    encode_symbols(out, &p.on);
    put_hash(out, p.superclass);
    encode_symbols(out, &p.interfaces);
    encode_symbols(out, &p.mixins);
    encode_dependencies(out, &p.explicit_dependencies);
    encode_dependencies(out, &p.implicit_dependencies);
    encode_object_hashes(out, &p.children);
}

fn decode_mixin(r: &mut Reader<'_>) -> Result<MixinPayload> {
    Ok(MixinPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: decode_class_parent(r)?,
        source: decode_opt_span_source(r)?,
        content_hash: Hash32(read_32(r)?),
        document: SymbolHash(Hash32(read_32(r)?)),
        on: decode_symbols(r)?,
        superclass: SymbolHash(Hash32(read_32(r)?)),
        interfaces: decode_symbols(r)?,
        mixins: decode_symbols(r)?,
        explicit_dependencies: decode_dependencies(r)?,
        implicit_dependencies: decode_dependencies(r)?,
        children: decode_object_hashes(r)?,
    })
}

fn encode_document_directory(p: &DocumentDirectoryPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    encode_parent_ref(out, p.parent);
    encode_object_hashes(out, &p.children);
    encode_comments(out, &p.comments);
}

fn decode_document_directory(r: &mut Reader<'_>) -> Result<DocumentDirectoryPayload> {
    Ok(DocumentDirectoryPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: decode_parent_ref(r)?,
        children: decode_object_hashes(r)?,
        comments: decode_comments(r)?,
    })
}

fn encode_document(p: &DocumentPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.parent);
    put_string(out, &p.title);
    encode_symbols(out, &p.sections);
    encode_object_hashes(out, &p.children);
    encode_comments(out, &p.comments);
}

fn decode_document(r: &mut Reader<'_>) -> Result<DocumentPayload> {
    Ok(DocumentPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: SymbolHash(Hash32(read_32(r)?)),
        title: read_string(r, "document title")?,
        sections: decode_symbols(r)?,
        children: decode_object_hashes(r)?,
        comments: decode_comments(r)?,
    })
}

fn encode_document_section(p: &DocumentSectionPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.parent);
    put_string(out, &p.title);
    encode_symbols(out, &p.body);
    encode_symbols(out, &p.sections);
    encode_object_hashes(out, &p.children);
    encode_comments(out, &p.comments);
}

fn decode_document_section(r: &mut Reader<'_>) -> Result<DocumentSectionPayload> {
    Ok(DocumentSectionPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: SymbolHash(Hash32(read_32(r)?)),
        title: read_string(r, "document section title")?,
        body: decode_symbols(r)?,
        sections: decode_symbols(r)?,
        children: decode_object_hashes(r)?,
        comments: decode_comments(r)?,
    })
}

fn encode_document_list(p: &DocumentListPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.parent);
    encode_symbols(out, &p.items);
    encode_object_hashes(out, &p.children);
    encode_comments(out, &p.comments);
}

fn decode_document_list(r: &mut Reader<'_>) -> Result<DocumentListPayload> {
    Ok(DocumentListPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: SymbolHash(Hash32(read_32(r)?)),
        items: decode_symbols(r)?,
        children: decode_object_hashes(r)?,
        comments: decode_comments(r)?,
    })
}

fn encode_document_text(p: &DocumentTextPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.parent);
    put_string(out, &p.text);
    encode_comments(out, &p.comments);
}

fn decode_document_text(r: &mut Reader<'_>) -> Result<DocumentTextPayload> {
    Ok(DocumentTextPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: SymbolHash(Hash32(read_32(r)?)),
        text: read_string(r, "document text")?,
        comments: decode_comments(r)?,
    })
}

fn encode_document_topic(p: &DocumentTopicPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.parent);
    put_string(out, &p.title);
    encode_symbols(out, &p.paragraphs);
    encode_object_hashes(out, &p.children);
    encode_comments(out, &p.comments);
}

fn decode_document_topic(r: &mut Reader<'_>) -> Result<DocumentTopicPayload> {
    Ok(DocumentTopicPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: SymbolHash(Hash32(read_32(r)?)),
        title: read_string(r, "document topic title")?,
        paragraphs: decode_symbols(r)?,
        children: decode_object_hashes(r)?,
        comments: decode_comments(r)?,
    })
}

fn encode_block(p: &BlockPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.parent);
    put_string(out, &p.language);
    put_string(out, &p.text);
    encode_comments(out, &p.comments);
}

fn decode_block(r: &mut Reader<'_>) -> Result<BlockPayload> {
    Ok(BlockPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: SymbolHash(Hash32(read_32(r)?)),
        language: read_string(r, "block language")?,
        text: read_string(r, "block text")?,
        comments: decode_comments(r)?,
    })
}

fn encode_mermaid_block(p: &MermaidBlockPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.parent);
    put_string(out, &p.text);
    encode_comments(out, &p.comments);
}

fn decode_mermaid_block(r: &mut Reader<'_>) -> Result<MermaidBlockPayload> {
    Ok(MermaidBlockPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: SymbolHash(Hash32(read_32(r)?)),
        text: read_string(r, "mermaid block text")?,
        comments: decode_comments(r)?,
    })
}

fn encode_html_block(p: &HtmlBlockPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.parent);
    put_string(out, &p.text);
    encode_comments(out, &p.comments);
}

fn decode_html_block(r: &mut Reader<'_>) -> Result<HtmlBlockPayload> {
    Ok(HtmlBlockPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: SymbolHash(Hash32(read_32(r)?)),
        text: read_string(r, "html block text")?,
        comments: decode_comments(r)?,
    })
}

fn encode_design_structure(p: &DesignStructurePayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    encode_parent_ref(out, p.parent);
    put_string(out, &p.title);
    put_hash(out, p.document);
    encode_object_hashes(out, &p.children);
}

fn decode_design_structure(r: &mut Reader<'_>) -> Result<DesignStructurePayload> {
    Ok(DesignStructurePayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: decode_parent_ref(r)?,
        title: read_string(r, "design structure title")?,
        document: SymbolHash(Hash32(read_32(r)?)),
        children: decode_object_hashes(r)?,
    })
}

fn encode_system(p: &SystemPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.parent);
    put_string(out, &p.title);
    put_hash(out, p.document);
    encode_object_hashes(out, &p.children);
    // Appended optional field: objects written before systems could own
    // members have no trailing bytes and decode with an empty member list.
    if !p.members.is_empty() {
        encode_symbols(out, &p.members);
    }
}

fn decode_system(r: &mut Reader<'_>) -> Result<SystemPayload> {
    let qualified_name = SymbolHash(Hash32(read_32(r)?));
    let parent = SymbolHash(Hash32(read_32(r)?));
    let title = read_string(r, "system title")?;
    let document = SymbolHash(Hash32(read_32(r)?));
    let children = decode_object_hashes(r)?;
    let members = if r.is_empty() {
        Vec::new()
    } else {
        decode_symbols(r)?
    };
    Ok(SystemPayload {
        qualified_name,
        parent,
        title,
        document,
        members,
        children,
    })
}

fn encode_module(p: &ModulePayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.parent);
    put_string(out, &p.title);
    put_hash(out, p.document);
    encode_symbols(out, &p.members);
    encode_object_hashes(out, &p.children);
}

fn decode_module(r: &mut Reader<'_>) -> Result<ModulePayload> {
    Ok(ModulePayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: SymbolHash(Hash32(read_32(r)?)),
        title: read_string(r, "module title")?,
        document: SymbolHash(Hash32(read_32(r)?)),
        members: decode_symbols(r)?,
        children: decode_object_hashes(r)?,
    })
}

fn encode_concept_directory(p: &ConceptDirectoryPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    encode_parent_ref(out, p.parent);
    encode_object_hashes(out, &p.children);
}

fn decode_concept_directory(r: &mut Reader<'_>) -> Result<ConceptDirectoryPayload> {
    Ok(ConceptDirectoryPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: decode_parent_ref(r)?,
        children: decode_object_hashes(r)?,
    })
}

fn encode_concept(p: &ConceptPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.parent);
    put_string(out, &p.name);
    put_string(out, &p.summary);
    put_string(out, &p.explanation);
    encode_strings(out, &p.characteristics);
    encode_strings(out, &p.relations);
    encode_strings(out, &p.aliases);
}

fn decode_concept(r: &mut Reader<'_>) -> Result<ConceptPayload> {
    Ok(ConceptPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: SymbolHash(Hash32(read_32(r)?)),
        name: read_string(r, "concept name")?,
        summary: read_string(r, "concept summary")?,
        explanation: read_string(r, "concept explanation")?,
        characteristics: decode_strings(r)?,
        relations: decode_strings(r)?,
        aliases: decode_strings(r)?,
    })
}

fn encode_layer(p: &LayerPayload, out: &mut Vec<u8>) {
    put_hash(out, p.qualified_name);
    put_hash(out, p.parent);
    put_string(out, &p.title);
    out.extend_from_slice(&p.order.to_le_bytes());
    put_hash(out, p.document);
    encode_symbols(out, &p.members);
    encode_object_hashes(out, &p.children);
}

fn decode_layer(r: &mut Reader<'_>) -> Result<LayerPayload> {
    Ok(LayerPayload {
        qualified_name: SymbolHash(Hash32(read_32(r)?)),
        parent: SymbolHash(Hash32(read_32(r)?)),
        title: read_string(r, "layer title")?,
        order: r.read_u32()?,
        document: SymbolHash(Hash32(read_32(r)?)),
        members: decode_symbols(r)?,
        children: decode_object_hashes(r)?,
    })
}

fn encode_strings(out: &mut Vec<u8>, items: &[String]) {
    put_len(out, items.len());
    for item in items {
        put_string(out, item);
    }
}

fn decode_strings(r: &mut Reader<'_>) -> Result<Vec<String>> {
    decode_vec(r, |r| read_string(r, "concept text"))
}

// ---------------------------------------------------------------------------
// CMT1
// ---------------------------------------------------------------------------

pub fn encode_commit(commit: &Commit) -> Vec<u8> {
    let payload = encode_commit_payload(commit);
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(CMT_MAGIC);
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&[0, 0, 0]);
    out.extend_from_slice(&payload);
    out
}

pub fn decode_commit(bytes: &[u8]) -> Result<Commit> {
    let mut reader = Reader::new(bytes);
    if reader.read(4)? != CMT_MAGIC {
        return Err(FormatError::InvalidMagic);
    }
    let version = reader.read_u8()?;
    if version != FORMAT_VERSION {
        return Err(FormatError::UnsupportedVersion(version));
    }
    let reserved = reader.read(3)?;
    if reserved != [0, 0, 0] {
        return Err(FormatError::InvalidValue("CMT1 reserved"));
    }
    let commit = decode_commit_payload(&mut reader)?;
    if !reader.is_empty() {
        return Err(FormatError::TrailingBytes);
    }
    validate_commit(&commit)?;
    Ok(commit)
}

pub fn decode_commit_checked(bytes: &[u8], expected: CommitHash) -> Result<Commit> {
    let commit = decode_commit(bytes)?;
    let actual = commit_hash(&commit);
    if actual != expected {
        return Err(FormatError::HashMismatch {
            expected: expected.to_hex(),
            actual: actual.to_hex(),
        });
    }
    Ok(commit)
}

fn encode_commit_payload(commit: &Commit) -> Vec<u8> {
    let mut out = Vec::new();
    put_hash(&mut out, commit.view);
    put_hash(&mut out, commit.parent);
    put_string(&mut out, &commit.message);
    put_string(&mut out, &commit.author);
    out.extend_from_slice(&commit.timestamp.to_le_bytes());
    out
}

fn decode_commit_payload(r: &mut Reader<'_>) -> Result<Commit> {
    Ok(Commit {
        view: ObjectHash(Hash32(read_32(r)?)),
        parent: CommitHash(Hash32(read_32(r)?)),
        message: read_string(r, "commit message")?,
        author: read_string(r, "commit author")?,
        timestamp: i64::from_le_bytes(read_array(r)?),
    })
}

// ---------------------------------------------------------------------------
// Primitive encoding
// ---------------------------------------------------------------------------

fn put_raw_hash(out: &mut Vec<u8>, hash: Hash32) {
    out.extend_from_slice(&hash.0);
}

fn put_hash<T: Copy + Into<Hash32>>(out: &mut Vec<u8>, hash: T) {
    out.extend_from_slice(&hash.into().0);
}

// Typed hashes are distinct newtypes; these conversions keep the encoder
// explicit without allowing accidental cross-type assignment in the model.
macro_rules! impl_hash_into {
    ($($name:ident),+ $(,)?) => {
        $(
            impl From<$name> for Hash32 {
                fn from(value: $name) -> Self {
                    value.0
                }
            }
        )+
    };
}

impl_hash_into!(ObjectHash, CommitHash, DocHash, SymbolHash);

fn encode_object_hashes(out: &mut Vec<u8>, hashes: &[ObjectHash]) {
    put_len(out, hashes.len());
    for hash in hashes {
        out.extend_from_slice(&hash.0.0);
    }
}

fn decode_object_hashes(r: &mut Reader<'_>) -> Result<Vec<ObjectHash>> {
    decode_vec(r, |r| Ok(ObjectHash(Hash32(read_32(r)?))))
}

fn encode_comments(out: &mut Vec<u8>, comments: &[DocComment]) {
    put_len(out, comments.len());
    for comment in comments {
        out.extend_from_slice(&comment.id.to_le_bytes());
        out.push(if comment.resolved { 1 } else { 0 });
        put_string(out, &comment.text);
    }
}

fn decode_comments(r: &mut Reader<'_>) -> Result<Vec<DocComment>> {
    decode_vec(r, |r| {
        let id = r.read_u32()?;
        let resolved = match r.read_u8()? {
            0 => false,
            1 => true,
            tag => return Err(FormatError::InvalidTag("comment status", tag)),
        };
        let text = read_string(r, "comment text")?;
        Ok(DocComment { id, text, resolved })
    })
}

fn encode_symbols(out: &mut Vec<u8>, hashes: &[SymbolHash]) {
    put_len(out, hashes.len());
    for hash in hashes {
        out.extend_from_slice(&hash.0.0);
    }
}

fn decode_symbols(r: &mut Reader<'_>) -> Result<Vec<SymbolHash>> {
    decode_vec(r, |r| Ok(SymbolHash(Hash32(read_32(r)?))))
}

fn encode_dependencies(out: &mut Vec<u8>, deps: &[Dependency]) {
    put_len(out, deps.len());
    for dep in deps {
        out.extend_from_slice(&dep.target.0.0);
        match dep.evidence {
            Some(evidence) => {
                out.push(1);
                out.extend_from_slice(&evidence.path.0.0);
                put_span(out, evidence.span);
            }
            None => out.push(0),
        }
        out.extend_from_slice(&dep.rationale.0.0);
    }
}

fn decode_dependencies(r: &mut Reader<'_>) -> Result<Vec<Dependency>> {
    decode_vec(r, |r| {
        let target = SymbolHash(Hash32(read_32(r)?));
        let evidence = match r.read_u8()? {
            0 => None,
            1 => Some(DependencyEvidence {
                path: SymbolHash(Hash32(read_32(r)?)),
                span: read_span(r)?,
            }),
            tag => return Err(FormatError::InvalidTag("dependency evidence", tag)),
        };
        let rationale = DocHash(Hash32(read_32(r)?));
        Ok(Dependency {
            target,
            evidence,
            rationale,
        })
    })
}

fn encode_parent_ref(out: &mut Vec<u8>, parent: ParentRef) {
    match parent {
        ParentRef::View => out.push(1),
        ParentRef::Object(hash) => {
            out.push(2);
            out.extend_from_slice(&hash.0.0);
        }
    }
}

fn decode_parent_ref(r: &mut Reader<'_>) -> Result<ParentRef> {
    match r.read_u8()? {
        1 => Ok(ParentRef::View),
        2 => Ok(ParentRef::Object(SymbolHash(Hash32(read_32(r)?)))),
        tag => Err(FormatError::InvalidTag("parent ref", tag)),
    }
}

fn encode_class_parent(out: &mut Vec<u8>, parent: ClassParent) {
    match parent {
        ClassParent::TopLevel {
            file,
            namespace,
            module,
        } => {
            out.push(1);
            out.extend_from_slice(&file.0.0);
            out.extend_from_slice(&namespace.0.0);
            out.extend_from_slice(&module.0.0);
        }
        ClassParent::Nested { parent } => {
            out.push(2);
            out.extend_from_slice(&parent.0.0);
        }
    }
}

fn decode_class_parent(r: &mut Reader<'_>) -> Result<ClassParent> {
    match r.read_u8()? {
        1 => Ok(ClassParent::TopLevel {
            file: SymbolHash(Hash32(read_32(r)?)),
            namespace: SymbolHash(Hash32(read_32(r)?)),
            module: SymbolHash(Hash32(read_32(r)?)),
        }),
        2 => Ok(ClassParent::Nested {
            parent: SymbolHash(Hash32(read_32(r)?)),
        }),
        tag => Err(FormatError::InvalidTag("class parent", tag)),
    }
}

fn encode_opt_path_source(out: &mut Vec<u8>, source: Option<PathSource>) {
    match source {
        Some(source) => {
            out.push(1);
            out.extend_from_slice(&source.path.0.0);
        }
        None => out.push(0),
    }
}

fn decode_opt_path_source(r: &mut Reader<'_>) -> Result<Option<PathSource>> {
    match r.read_u8()? {
        0 => Ok(None),
        1 => Ok(Some(PathSource {
            path: SymbolHash(Hash32(read_32(r)?)),
        })),
        tag => Err(FormatError::InvalidTag("path source", tag)),
    }
}

fn encode_opt_span_source(out: &mut Vec<u8>, source: Option<SpanSource>) {
    match source {
        Some(source) => {
            out.push(1);
            encode_span_source(out, source);
        }
        None => out.push(0),
    }
}

fn decode_opt_span_source(r: &mut Reader<'_>) -> Result<Option<SpanSource>> {
    match r.read_u8()? {
        0 => Ok(None),
        1 => Ok(Some(decode_span_source(r)?)),
        tag => Err(FormatError::InvalidTag("span source", tag)),
    }
}

fn encode_span_source(out: &mut Vec<u8>, source: SpanSource) {
    out.extend_from_slice(&source.path.0.0);
    put_span(out, source.span);
}

fn decode_span_source(r: &mut Reader<'_>) -> Result<SpanSource> {
    Ok(SpanSource {
        path: SymbolHash(Hash32(read_32(r)?)),
        span: read_span(r)?,
    })
}

fn put_span(out: &mut Vec<u8>, span: Span) {
    out.extend_from_slice(&span.start_byte.to_le_bytes());
    out.extend_from_slice(&span.end_byte.to_le_bytes());
    out.extend_from_slice(&span.start_line.to_le_bytes());
    out.extend_from_slice(&span.end_line.to_le_bytes());
}

fn read_span(r: &mut Reader<'_>) -> Result<Span> {
    Ok(Span {
        start_byte: u64::from_le_bytes(read_array(r)?),
        end_byte: u64::from_le_bytes(read_array(r)?),
        start_line: u32::from_le_bytes(read_array(r)?),
        end_line: u32::from_le_bytes(read_array(r)?),
    })
}

fn put_len(out: &mut Vec<u8>, len: usize) {
    out.extend_from_slice(&(len as u32).to_le_bytes());
}

fn put_string(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
}

fn read_string(r: &mut Reader<'_>, what: &'static str) -> Result<String> {
    let len = r.read_u32()? as usize;
    let bytes = r.read(len)?;
    String::from_utf8(bytes.to_vec()).map_err(|_| FormatError::InvalidUtf8(what))
}

fn decode_vec<T>(
    r: &mut Reader<'_>,
    mut item: impl FnMut(&mut Reader<'_>) -> Result<T>,
) -> Result<Vec<T>> {
    let count = r.read_u32()? as usize;
    // Avoid preallocating an attacker-controlled huge Vec.
    let mut out = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        out.push(item(r)?);
    }
    Ok(out)
}

fn read_32(r: &mut Reader<'_>) -> Result<[u8; 32]> {
    let bytes = r.read(32)?;
    let mut out = [0u8; 32];
    out.copy_from_slice(bytes);
    Ok(out)
}

fn read_array<const N: usize>(r: &mut Reader<'_>) -> Result<[u8; N]> {
    let bytes = r.read(N)?;
    let mut out = [0u8; N];
    out.copy_from_slice(bytes);
    Ok(out)
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn is_empty(&self) -> bool {
        self.pos == self.data.len()
    }

    fn read(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or(FormatError::UnexpectedEof)?;
        if end > self.data.len() {
            return Err(FormatError::UnexpectedEof);
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read(1)?[0])
    }

    fn read_u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(read_array(self)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(byte: u8) -> SymbolHash {
        SymbolHash(Hash32([byte; 32]))
    }

    fn o(byte: u8) -> ObjectHash {
        ObjectHash(Hash32([byte; 32]))
    }

    fn d(byte: u8) -> DocHash {
        DocHash(Hash32([byte; 32]))
    }

    fn roundtrip(object: DesignObject) {
        let bytes = encode_object(&object).unwrap();
        let decoded = decode_object(&bytes).unwrap();
        assert_eq!(object, decoded);
        let hash = object_hash(&object).unwrap();
        assert_eq!(hash, object_hash(&decoded).unwrap());
        decode_object_checked(&bytes, hash).unwrap();
    }

    #[test]
    fn roundtrips_view() {
        roundtrip(DesignObject::new(
            Kind::View,
            ObjectPayload::View(ViewPayload {
                source_commit: GitOid([1; 32]),
                files: o(2),
                namespaces: o(3),
                design: ObjectHash::ZERO,
                documents: o(4),
                concepts: o(5),
            }),
        ));
    }

    fn view_hash(files: &str, namespaces: &str, design: &str, documents: &str) -> String {
        let object = DesignObject::new(
            Kind::View,
            ObjectPayload::View(ViewPayload {
                source_commit: GitOid([0; 32]),
                files: ObjectHash(Hash32::from_hex(files).unwrap()),
                namespaces: ObjectHash(Hash32::from_hex(namespaces).unwrap()),
                design: ObjectHash(Hash32::from_hex(design).unwrap()),
                documents: ObjectHash(Hash32::from_hex(documents).unwrap()),
                concepts: ObjectHash::ZERO,
            }),
        );
        object_hash(&object).unwrap().0.to_hex()
    }

    #[test]
    fn keeps_the_hash_of_a_view_without_documents() {
        assert_eq!(
            view_hash(
                "efadf253f5b3e52c54b2e0b4f580d9b7eb1e1aff00d67548200fdc0967289cf7",
                "bcd909463a4f037771954f489d8aa471d3940c7895d30e5636a56ca724575f52",
                "0000000000000000000000000000000000000000000000000000000000000000",
                "0000000000000000000000000000000000000000000000000000000000000000",
            ),
            "e67ef81f8bddd49877ff35d4998043efdd3f8f38961976488a1dac5007e78345"
        );
    }

    #[test]
    fn keeps_the_hash_of_a_view_without_concepts() {
        assert_eq!(
            view_hash(
                "efadf253f5b3e52c54b2e0b4f580d9b7eb1e1aff00d67548200fdc0967289cf7",
                "bcd909463a4f037771954f489d8aa471d3940c7895d30e5636a56ca724575f52",
                "9a4492342fb2250696621676df648c37f393fe5a393f6352045c390f6159c567",
                "a705a3e3ee8a439751c4ea9d902429caa45a87dd40b375b0b20f9651b522d924",
            ),
            "a50c5e057576ceb798aee79c006f69994488f9019a38efd91988190ed5df1d6d"
        );
    }

    #[test]
    fn decodes_a_legacy_view_without_documents() {
        let object = DesignObject::new(
            Kind::View,
            ObjectPayload::View(ViewPayload {
                source_commit: GitOid([0; 32]),
                files: ObjectHash(
                    Hash32::from_hex(
                        "efadf253f5b3e52c54b2e0b4f580d9b7eb1e1aff00d67548200fdc0967289cf7",
                    )
                    .unwrap(),
                ),
                namespaces: ObjectHash(
                    Hash32::from_hex(
                        "bcd909463a4f037771954f489d8aa471d3940c7895d30e5636a56ca724575f52",
                    )
                    .unwrap(),
                ),
                design: ObjectHash::ZERO,
                documents: ObjectHash::ZERO,
                concepts: ObjectHash::ZERO,
            }),
        );
        let encoded = encode_object(&object).unwrap();
        assert_eq!(encoded.len(), 134);
        assert_eq!(decode_object(&encoded).unwrap(), object);
    }

    #[test]
    fn roundtrips_directory() {
        roundtrip(DesignObject::new(
            Kind::Directory,
            ObjectPayload::Directory(DirectoryPayload {
                qualified_name: s(1),
                parent: ParentRef::View,
                source: Some(PathSource { path: s(2) }),
                document: SymbolHash::ZERO,
                children: vec![o(3), o(4)],
            }),
        ));
    }

    #[test]
    fn roundtrips_class() {
        roundtrip(DesignObject::new(
            Kind::Class,
            ObjectPayload::Class(ClassPayload {
                qualified_name: s(1),
                parent: ClassParent::TopLevel {
                    file: s(2),
                    namespace: s(3),
                    module: SymbolHash::ZERO,
                },
                source: Some(SpanSource {
                    path: s(4),
                    span: Span {
                        start_byte: 1,
                        end_byte: 9,
                        start_line: 1,
                        end_line: 2,
                    },
                }),
                content_hash: Hash32([5; 32]),
                document: s(6),
                superclass: s(7),
                interfaces: vec![s(8)],
                mixins: vec![s(9)],
                explicit_dependencies: vec![Dependency {
                    target: s(10),
                    evidence: Some(DependencyEvidence {
                        path: s(11),
                        span: Span {
                            start_byte: 2,
                            end_byte: 4,
                            start_line: 2,
                            end_line: 2,
                        },
                    }),
                    rationale: DocHash::ZERO,
                }],
                implicit_dependencies: vec![Dependency {
                    target: s(12),
                    evidence: None,
                    rationale: d(13),
                }],
                children: vec![o(14)],
            }),
        ));
    }

    #[test]
    fn roundtrips_inner_class() {
        roundtrip(DesignObject::new(
            Kind::InnerClass,
            ObjectPayload::InnerClass(ClassPayload {
                qualified_name: s(1),
                parent: ClassParent::Nested { parent: s(2) },
                source: None,
                content_hash: Hash32::ZERO,
                document: SymbolHash::ZERO,
                superclass: s(3),
                interfaces: vec![s(4)],
                mixins: vec![s(5)],
                explicit_dependencies: vec![],
                implicit_dependencies: vec![],
                children: vec![o(6)],
            }),
        ));
    }

    #[test]
    fn roundtrips_interface_extends() {
        roundtrip(DesignObject::new(
            Kind::InnerInterface,
            ObjectPayload::InnerInterface(InterfacePayload {
                qualified_name: s(1),
                parent: ClassParent::Nested { parent: s(2) },
                source: None,
                content_hash: Hash32::ZERO,
                document: SymbolHash::ZERO,
                extends: s(3),
                explicit_dependencies: vec![],
                implicit_dependencies: vec![],
                children: vec![],
            }),
        ));
    }

    #[test]
    fn roundtrips_method_and_function() {
        roundtrip(DesignObject::new(
            Kind::Method,
            ObjectPayload::Method(MethodPayload {
                qualified_name: s(1),
                parent: s(2),
                source: None,
                content_hash: Hash32::ZERO,
                document: SymbolHash::ZERO,
                overrides: s(3),
                explicit_dependencies: vec![],
                implicit_dependencies: vec![],
            }),
        ));
        roundtrip(DesignObject::new(
            Kind::Function,
            ObjectPayload::Function(FunctionPayload {
                qualified_name: s(1),
                file: s(2),
                namespace: s(3),
                module: SymbolHash::ZERO,
                source: None,
                content_hash: Hash32::ZERO,
                document: SymbolHash::ZERO,
                explicit_dependencies: vec![],
                implicit_dependencies: vec![],
            }),
        ));
    }

    #[test]
    fn roundtrips_part_and_mixin() {
        roundtrip(DesignObject::new(
            Kind::Part,
            ObjectPayload::Part(PartPayload {
                qualified_name: s(1),
                parent: s(2),
                source: SpanSource {
                    path: s(3),
                    span: Span {
                        start_byte: 0,
                        end_byte: 1,
                        start_line: 1,
                        end_line: 1,
                    },
                },
                content_hash: Hash32([4; 32]),
                explicit_dependencies: vec![],
                implicit_dependencies: vec![],
                children: vec![],
            }),
        ));
        roundtrip(DesignObject::new(
            Kind::Mixin,
            ObjectPayload::Mixin(MixinPayload {
                qualified_name: s(1),
                parent: ClassParent::TopLevel {
                    file: SymbolHash::ZERO,
                    namespace: s(2),
                    module: SymbolHash::ZERO,
                },
                source: None,
                content_hash: Hash32::ZERO,
                document: SymbolHash::ZERO,
                on: vec![s(3), s(4)],
                superclass: SymbolHash::ZERO,
                interfaces: vec![s(5)],
                mixins: vec![s(6)],
                explicit_dependencies: vec![],
                implicit_dependencies: vec![],
                children: vec![],
            }),
        ));
    }

    #[test]
    fn roundtrips_document_objects() {
        roundtrip(DesignObject::new(
            Kind::DocumentDirectory,
            ObjectPayload::DocumentDirectory(DocumentDirectoryPayload {
                qualified_name: s(1),
                parent: ParentRef::View,
                children: vec![],
                comments: vec![],
            }),
        ));
        roundtrip(DesignObject::new(
            Kind::Document,
            ObjectPayload::Document(DocumentPayload {
                qualified_name: s(1),
                parent: s(2),
                title: "Auth".into(),
                sections: vec![s(3), s(4)],
                children: vec![o(5), o(6)],
                comments: vec![DocComment {
                    id: 1,
                    text: "needs examples".into(),
                    resolved: false,
                }],
            }),
        ));
        roundtrip(DesignObject::new(
            Kind::DocumentSection,
            ObjectPayload::DocumentSection(DocumentSectionPayload {
                qualified_name: s(1),
                parent: s(2),
                title: "Login".into(),
                body: vec![s(3), s(4)],
                sections: vec![s(5)],
                children: vec![o(6), o(7), o(8)],
                comments: vec![],
            }),
        ));
        roundtrip(DesignObject::new(
            Kind::DocumentList,
            ObjectPayload::DocumentList(DocumentListPayload {
                qualified_name: s(1),
                parent: s(2),
                items: vec![s(3), s(4)],
                children: vec![o(5), o(6)],
                comments: vec![DocComment {
                    id: 1,
                    text: "keep sorted".into(),
                    resolved: true,
                }],
            }),
        ));
        roundtrip(DesignObject::new(
            Kind::DocumentText,
            ObjectPayload::DocumentText(DocumentTextPayload {
                qualified_name: s(1),
                parent: s(2),
                text: "one line".into(),
                comments: vec![],
            }),
        ));
        roundtrip(DesignObject::new(
            Kind::DocumentTopic,
            ObjectPayload::DocumentTopic(DocumentTopicPayload {
                qualified_name: s(1),
                parent: s(2),
                title: "Responsibilities".into(),
                paragraphs: vec![s(3), s(4)],
                children: vec![o(5), o(6)],
                comments: vec![],
            }),
        ));
    }

    #[test]
    fn rejects_invalid_document_text() {
        let object = DesignObject::new(
            Kind::DocumentText,
            ObjectPayload::DocumentText(DocumentTextPayload {
                qualified_name: s(1),
                parent: s(2),
                text: "two\nlines".into(),
                comments: vec![],
            }),
        );
        assert!(matches!(
            validate_object(&object),
            Err(FormatError::InvalidValue("document text"))
        ));
    }

    #[test]
    fn roundtrips_document_block() {
        roundtrip(DesignObject::new(
            Kind::Block,
            ObjectPayload::Block(BlockPayload {
                qualified_name: s(1),
                parent: s(2),
                language: "rust".into(),
                text: "let token = login();\nnext_step();".into(),
                comments: vec![],
            }),
        ));
    }

    #[test]
    fn rejects_empty_block_text() {
        let object = DesignObject::new(
            Kind::Block,
            ObjectPayload::Block(BlockPayload {
                qualified_name: s(1),
                parent: s(2),
                language: String::new(),
                text: String::new(),
                comments: vec![],
            }),
        );
        assert!(matches!(
            validate_object(&object),
            Err(FormatError::InvalidValue("block text"))
        ));
    }

    #[test]
    fn roundtrips_mermaid_block() {
        roundtrip(DesignObject::new(
            Kind::MermaidBlock,
            ObjectPayload::MermaidBlock(MermaidBlockPayload {
                qualified_name: s(1),
                parent: s(2),
                text: "flowchart TD\n  A --> B".into(),
                comments: vec![],
            }),
        ));
    }

    #[test]
    fn rejects_empty_mermaid_text() {
        let object = DesignObject::new(
            Kind::MermaidBlock,
            ObjectPayload::MermaidBlock(MermaidBlockPayload {
                qualified_name: s(1),
                parent: s(2),
                text: String::new(),
                comments: vec![],
            }),
        );
        assert!(matches!(
            validate_object(&object),
            Err(FormatError::InvalidValue("mermaid block text"))
        ));
    }

    #[test]
    fn roundtrips_html_block() {
        roundtrip(DesignObject::new(
            Kind::HtmlBlock,
            ObjectPayload::HtmlBlock(HtmlBlockPayload {
                qualified_name: s(1),
                parent: s(2),
                text: "<svg><circle cx=\"10\" cy=\"10\" r=\"8\"/></svg>".into(),
                comments: vec![],
            }),
        ));
    }

    #[test]
    fn rejects_empty_html_text() {
        let object = DesignObject::new(
            Kind::HtmlBlock,
            ObjectPayload::HtmlBlock(HtmlBlockPayload {
                qualified_name: s(1),
                parent: s(2),
                text: String::new(),
                comments: vec![],
            }),
        );
        assert!(matches!(
            validate_object(&object),
            Err(FormatError::InvalidValue("html block text"))
        ));
    }

    #[test]
    fn roundtrips_structure_objects() {
        roundtrip(DesignObject::new(
            Kind::DesignStructure,
            ObjectPayload::DesignStructure(DesignStructurePayload {
                qualified_name: s(1),
                parent: ParentRef::View,
                title: "设计结构".to_owned(),
                document: SymbolHash::ZERO,
                children: vec![],
            }),
        ));
        roundtrip(DesignObject::new(
            Kind::System,
            ObjectPayload::System(SystemPayload {
                qualified_name: s(2),
                parent: s(1),
                title: "应用".to_owned(),
                document: s(3),
                members: vec![s(5), s(6)],
                children: vec![],
            }),
        ));
        roundtrip(DesignObject::new(
            Kind::System,
            ObjectPayload::System(SystemPayload {
                qualified_name: s(9),
                parent: s(1),
                title: "空成员系统".to_owned(),
                document: s(3),
                members: vec![],
                children: vec![],
            }),
        ));
        roundtrip(DesignObject::new(
            Kind::Module,
            ObjectPayload::Module(ModulePayload {
                qualified_name: s(4),
                parent: s(2),
                title: "核心".to_owned(),
                document: s(3),
                members: vec![s(7), s(8)],
                children: vec![],
            }),
        ));
    }

    #[test]
    fn roundtrips_commit() {
        let commit = Commit {
            view: o(1),
            parent: CommitHash::ZERO,
            message: "hello".into(),
            author: "agent".into(),
            timestamp: 42,
        };
        let bytes = encode_commit(&commit);
        let decoded = decode_commit(&bytes).unwrap();
        assert_eq!(commit, decoded);
        decode_commit_checked(&bytes, commit_hash(&commit)).unwrap();
    }

    #[test]
    fn rejects_trailing_bytes() {
        let object = DesignObject::new(
            Kind::View,
            ObjectPayload::View(ViewPayload {
                source_commit: GitOid::ZERO,
                files: o(1),
                namespaces: o(2),
                design: ObjectHash::ZERO,
                documents: o(3),
                concepts: ObjectHash::ZERO,
            }),
        );
        let mut bytes = encode_object(&object).unwrap();
        // 32 bytes are read as the optional concept root; the 33rd is trailing.
        bytes.extend(std::iter::repeat_n(0, 33));
        assert!(matches!(
            decode_object(&bytes),
            Err(FormatError::TrailingBytes)
        ));
    }

    #[test]
    fn rejects_bad_magic() {
        assert!(matches!(
            decode_object(b"NOPE"),
            Err(FormatError::InvalidMagic)
        ));
        assert!(matches!(
            decode_commit(b"NOPE"),
            Err(FormatError::InvalidMagic)
        ));
    }

    #[test]
    fn docs_and_symbols_are_raw_text_hashes() {
        assert_eq!(doc_hash("hello"), DocHash(hash_bytes(b"hello")));
        assert_eq!(symbol_hash("hello"), SymbolHash(hash_bytes(b"hello")));
        assert_eq!(doc_hash(""), DocHash(hash_bytes(b"")));
    }

    #[test]
    fn content_hash_hashes_exact_bytes() {
        assert_eq!(content_hash(b"a\nb"), hash_bytes(b"a\nb"));
        assert_ne!(content_hash(b"a\nb"), content_hash(b"a\r\nb"));
        assert_ne!(content_hash(b""), Hash32::ZERO);
    }

    #[test]
    fn rejects_invalid_objects() {
        let zero_name = DesignObject::new(
            Kind::Directory,
            ObjectPayload::Directory(DirectoryPayload {
                qualified_name: SymbolHash::ZERO,
                parent: ParentRef::View,
                source: None,
                document: SymbolHash::ZERO,
                children: vec![],
            }),
        );
        assert!(matches!(
            object_hash(&zero_name),
            Err(FormatError::RequiredHashZero(_))
        ));

        let hash_without_source = DesignObject::new(
            Kind::File,
            ObjectPayload::File(FilePayload {
                qualified_name: s(1),
                parent: s(2),
                source: None,
                content_hash: Hash32([3; 32]),
                document: SymbolHash::ZERO,
                children: vec![],
            }),
        );
        assert!(matches!(
            object_hash(&hash_without_source),
            Err(FormatError::InvalidValue("file"))
        ));

        let source_without_hash = DesignObject::new(
            Kind::File,
            ObjectPayload::File(FilePayload {
                qualified_name: s(1),
                parent: s(2),
                source: Some(PathSource { path: s(3) }),
                content_hash: Hash32::ZERO,
                document: SymbolHash::ZERO,
                children: vec![],
            }),
        );
        assert!(matches!(
            object_hash(&source_without_hash),
            Err(FormatError::InvalidValue("file"))
        ));

        let unsorted_interfaces = DesignObject::new(
            Kind::Class,
            ObjectPayload::Class(ClassPayload {
                qualified_name: s(1),
                parent: ClassParent::TopLevel {
                    file: SymbolHash::ZERO,
                    namespace: s(2),
                    module: SymbolHash::ZERO,
                },
                source: None,
                content_hash: Hash32::ZERO,
                document: SymbolHash::ZERO,
                superclass: SymbolHash::ZERO,
                interfaces: vec![s(3), s(2)],
                mixins: vec![],
                explicit_dependencies: vec![],
                implicit_dependencies: vec![],
                children: vec![],
            }),
        );
        assert!(matches!(
            object_hash(&unsorted_interfaces),
            Err(FormatError::NonCanonicalList(_))
        ));

        let zero_part_hash = DesignObject::new(
            Kind::Part,
            ObjectPayload::Part(PartPayload {
                qualified_name: s(1),
                parent: s(2),
                source: SpanSource {
                    path: s(3),
                    span: Span {
                        start_byte: 0,
                        end_byte: 1,
                        start_line: 1,
                        end_line: 1,
                    },
                },
                content_hash: Hash32::ZERO,
                explicit_dependencies: vec![],
                implicit_dependencies: vec![],
                children: vec![],
            }),
        );
        assert!(matches!(
            object_hash(&zero_part_hash),
            Err(FormatError::RequiredHashZero("part"))
        ));
    }

    #[test]
    fn rejects_invalid_commits() {
        let commit = Commit {
            view: ObjectHash::ZERO,
            parent: CommitHash::ZERO,
            message: String::new(),
            author: String::new(),
            timestamp: 0,
        };
        assert!(matches!(
            validate_commit(&commit),
            Err(FormatError::RequiredHashZero("commit view"))
        ));
        assert!(matches!(
            decode_commit(&encode_commit(&commit)),
            Err(FormatError::RequiredHashZero("commit view"))
        ));
    }
}
