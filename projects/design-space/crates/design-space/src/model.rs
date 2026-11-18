//! The design-space object model.
//!
//! This module implements the object shapes described in
//! `docs/storage-format.md` and is used by the canonical binary format in
//! [`crate::format`].

use std::fmt;

/// A 32-byte SHA-256 value.
///
/// Zero is the None sentinel for every optional hash field. It is never a
/// valid computed hash.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash32(pub [u8; 32]);

impl Hash32 {
    pub const ZERO: Self = Self([0; 32]);

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn is_zero(&self) -> bool {
        self.0 == [0; 32]
    }

    pub fn to_hex(self) -> String {
        let mut out = String::with_capacity(64);
        for byte in self.0 {
            use std::fmt::Write as _;
            write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
        }
        out
    }

    pub fn from_hex(text: &str) -> Option<Self> {
        if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (index, chunk) in text.as_bytes().chunks(2).enumerate() {
            bytes[index] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
        }
        Some(Self(bytes))
    }
}

impl fmt::Debug for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

macro_rules! typed_hash {
    ($name:ident) => {
        #[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub Hash32);

        impl $name {
            pub const ZERO: Self = Self(Hash32::ZERO);

            pub const fn from_bytes(bytes: [u8; 32]) -> Self {
                Self(Hash32(bytes))
            }

            pub const fn from_hash(hash: Hash32) -> Self {
                Self(hash)
            }

            pub fn is_zero(&self) -> bool {
                self.0.is_zero()
            }

            pub fn to_hex(self) -> String {
                self.0.to_hex()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.to_hex())
            }
        }
    };
}

typed_hash!(ObjectHash);
typed_hash!(CommitHash);
typed_hash!(DocHash);
typed_hash!(SymbolHash);

/// A Git commit OID. This is an external identifier, not an internal hash.
///
/// SHA-1 uses the first 20 bytes and zero-pads the remaining 12. The actual
/// interpretation is selected by the workspace-level `source.object_format`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct GitOid(pub [u8; 32]);

impl GitOid {
    pub const ZERO: Self = Self([0; 32]);

    pub fn is_zero(&self) -> bool {
        self.0 == [0; 32]
    }

    pub fn to_hex(self) -> String {
        Hash32(self.0).to_hex()
    }
}

impl fmt::Debug for GitOid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// The kind byte stored in a DSO1 record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Kind {
    View = 0x01,
    Directory = 0x02,
    File = 0x03,
    Namespace = 0x04,
    Class = 0x05,
    Interface = 0x06,
    Method = 0x07,
    Function = 0x08,
    Part = 0x09,
    Mixin = 0x0A,
    InnerClass = 0x0C,
    InnerInterface = 0x0D,
    DocumentDirectory = 0x0E,
    Document = 0x0F,
    DocumentSection = 0x10,
    DocumentList = 0x11,
    DocumentText = 0x12,
    DocumentTopic = 0x13,
    DesignStructure = 0x14,
    System = 0x15,
    Module = 0x16,
}

impl Kind {
    pub const fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            0x01 => Self::View,
            0x02 => Self::Directory,
            0x03 => Self::File,
            0x04 => Self::Namespace,
            0x05 => Self::Class,
            0x06 => Self::Interface,
            0x07 => Self::Method,
            0x08 => Self::Function,
            0x09 => Self::Part,
            0x0A => Self::Mixin,
            0x0C => Self::InnerClass,
            0x0D => Self::InnerInterface,
            0x0E => Self::DocumentDirectory,
            0x0F => Self::Document,
            0x10 => Self::DocumentSection,
            0x11 => Self::DocumentList,
            0x12 => Self::DocumentText,
            0x13 => Self::DocumentTopic,
            0x14 => Self::DesignStructure,
            0x15 => Self::System,
            0x16 => Self::Module,
            _ => return None,
        })
    }

    pub const fn as_byte(self) -> u8 {
        self as u8
    }
}

/// A source path, used by directory and file objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathSource {
    pub path: SymbolHash,
}

/// A byte/line range in a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start_byte: u64,
    pub end_byte: u64,
    pub start_line: u32,
    pub end_line: u32,
}

impl Span {
    pub fn is_valid(&self) -> bool {
        self.end_byte >= self.start_byte && self.end_line >= self.start_line
    }
}

/// A span in a source file, used by code objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpanSource {
    pub path: SymbolHash,
    pub span: Span,
}

/// A parent edge. View is a special parent; all other parents use a qualified
/// name symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentRef {
    View,
    Object(SymbolHash),
}

/// The parent shape of a class, interface, or mixin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassParent {
    TopLevel {
        file: SymbolHash,
        namespace: SymbolHash,
        module: SymbolHash,
    },
    Nested {
        parent: SymbolHash,
    },
}

/// A typed dependency target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DependencyEvidence {
    pub path: SymbolHash,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dependency {
    pub target: SymbolHash,
    pub evidence: Option<DependencyEvidence>,
    pub rationale: DocHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewPayload {
    pub source_commit: GitOid,
    pub files: ObjectHash,
    pub namespaces: ObjectHash,
    pub design: ObjectHash,
    pub documents: ObjectHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryPayload {
    pub qualified_name: SymbolHash,
    pub parent: ParentRef,
    pub source: Option<PathSource>,
    pub document: SymbolHash,
    pub children: Vec<ObjectHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePayload {
    pub qualified_name: SymbolHash,
    pub parent: SymbolHash,
    pub source: Option<PathSource>,
    pub content_hash: Hash32,
    pub document: SymbolHash,
    pub children: Vec<ObjectHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespacePayload {
    pub qualified_name: SymbolHash,
    pub parent: ParentRef,
    pub source: Option<SpanSource>,
    pub content_hash: Hash32,
    pub document: SymbolHash,
    pub children: Vec<ObjectHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassPayload {
    pub qualified_name: SymbolHash,
    pub parent: ClassParent,
    pub source: Option<SpanSource>,
    pub content_hash: Hash32,
    pub document: SymbolHash,
    pub superclass: SymbolHash,
    pub interfaces: Vec<SymbolHash>,
    pub mixins: Vec<SymbolHash>,
    pub explicit_dependencies: Vec<Dependency>,
    pub implicit_dependencies: Vec<Dependency>,
    pub children: Vec<ObjectHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfacePayload {
    pub qualified_name: SymbolHash,
    pub parent: ClassParent,
    pub source: Option<SpanSource>,
    pub content_hash: Hash32,
    pub document: SymbolHash,
    pub extends: SymbolHash,
    pub explicit_dependencies: Vec<Dependency>,
    pub implicit_dependencies: Vec<Dependency>,
    pub children: Vec<ObjectHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodPayload {
    pub qualified_name: SymbolHash,
    pub parent: SymbolHash,
    pub source: Option<SpanSource>,
    pub content_hash: Hash32,
    pub document: SymbolHash,
    pub overrides: SymbolHash,
    pub explicit_dependencies: Vec<Dependency>,
    pub implicit_dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionPayload {
    pub qualified_name: SymbolHash,
    pub file: SymbolHash,
    pub namespace: SymbolHash,
    pub module: SymbolHash,
    pub source: Option<SpanSource>,
    pub content_hash: Hash32,
    pub document: SymbolHash,
    pub explicit_dependencies: Vec<Dependency>,
    pub implicit_dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartPayload {
    pub qualified_name: SymbolHash,
    pub parent: SymbolHash,
    pub source: SpanSource,
    pub content_hash: Hash32,
    pub explicit_dependencies: Vec<Dependency>,
    pub implicit_dependencies: Vec<Dependency>,
    pub children: Vec<ObjectHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixinPayload {
    pub qualified_name: SymbolHash,
    pub parent: ClassParent,
    pub source: Option<SpanSource>,
    pub content_hash: Hash32,
    pub document: SymbolHash,
    pub on: Vec<SymbolHash>,
    pub superclass: SymbolHash,
    pub interfaces: Vec<SymbolHash>,
    pub mixins: Vec<SymbolHash>,
    pub explicit_dependencies: Vec<Dependency>,
    pub implicit_dependencies: Vec<Dependency>,
    pub children: Vec<ObjectHash>,
}

/// A comment attached to one document object. Comment ids are local to the
/// object and increase monotonically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocComment {
    pub id: u32,
    pub text: String,
    pub resolved: bool,
}

/// A document directory. `documents` is the root; other directories nest
/// under it. Document directories are ordered by name, not by a fragment
/// order list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentDirectoryPayload {
    pub qualified_name: SymbolHash,
    pub parent: ParentRef,
    pub children: Vec<ObjectHash>,
    pub comments: Vec<DocComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentPayload {
    pub qualified_name: SymbolHash,
    pub parent: SymbolHash,
    pub title: String,
    /// Ordered section references by qualified name.
    pub sections: Vec<SymbolHash>,
    pub children: Vec<ObjectHash>,
    pub comments: Vec<DocComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSectionPayload {
    pub qualified_name: SymbolHash,
    pub parent: SymbolHash,
    pub title: String,
    /// Ordered body element references by qualified name.
    pub body: Vec<SymbolHash>,
    /// Ordered child section references by qualified name.
    pub sections: Vec<SymbolHash>,
    pub children: Vec<ObjectHash>,
    pub comments: Vec<DocComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentListPayload {
    pub qualified_name: SymbolHash,
    pub parent: SymbolHash,
    /// Ordered item references by qualified name.
    pub items: Vec<SymbolHash>,
    pub children: Vec<ObjectHash>,
    pub comments: Vec<DocComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentTextPayload {
    pub qualified_name: SymbolHash,
    pub parent: SymbolHash,
    pub text: String,
    pub comments: Vec<DocComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentTopicPayload {
    pub qualified_name: SymbolHash,
    pub parent: SymbolHash,
    pub title: String,
    /// Ordered body paragraph references by qualified name.
    pub paragraphs: Vec<SymbolHash>,
    pub children: Vec<ObjectHash>,
    pub comments: Vec<DocComment>,
}

/// Root of the logical design structure. Its children are systems.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignStructurePayload {
    pub qualified_name: SymbolHash,
    pub parent: ParentRef,
    pub title: String,
    pub document: SymbolHash,
    pub children: Vec<ObjectHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemPayload {
    pub qualified_name: SymbolHash,
    pub parent: SymbolHash,
    pub title: String,
    pub document: SymbolHash,
    pub children: Vec<ObjectHash>,
}

/// A logical module. `members` are references to top-level code elements;
/// child modules are containment children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModulePayload {
    pub qualified_name: SymbolHash,
    pub parent: SymbolHash,
    pub title: String,
    pub document: SymbolHash,
    pub members: Vec<SymbolHash>,
    pub children: Vec<ObjectHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectPayload {
    View(ViewPayload),
    Directory(DirectoryPayload),
    File(FilePayload),
    Namespace(NamespacePayload),
    Class(ClassPayload),
    InnerClass(ClassPayload),
    Interface(InterfacePayload),
    InnerInterface(InterfacePayload),
    Method(MethodPayload),
    Function(FunctionPayload),
    Part(PartPayload),
    Mixin(MixinPayload),
    DocumentDirectory(DocumentDirectoryPayload),
    Document(DocumentPayload),
    DocumentSection(DocumentSectionPayload),
    DocumentList(DocumentListPayload),
    DocumentText(DocumentTextPayload),
    DocumentTopic(DocumentTopicPayload),
    DesignStructure(DesignStructurePayload),
    System(SystemPayload),
    Module(ModulePayload),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignObject {
    pub kind: Kind,
    pub payload: ObjectPayload,
}

impl DesignObject {
    pub fn new(kind: Kind, payload: ObjectPayload) -> Self {
        Self { kind, payload }
    }

    pub fn kind_from_payload(payload: &ObjectPayload) -> Kind {
        match payload {
            ObjectPayload::View(_) => Kind::View,
            ObjectPayload::Directory(_) => Kind::Directory,
            ObjectPayload::File(_) => Kind::File,
            ObjectPayload::Namespace(_) => Kind::Namespace,
            ObjectPayload::Class(_) => Kind::Class,
            ObjectPayload::InnerClass(_) => Kind::InnerClass,
            ObjectPayload::Interface(_) => Kind::Interface,
            ObjectPayload::InnerInterface(_) => Kind::InnerInterface,
            ObjectPayload::Method(_) => Kind::Method,
            ObjectPayload::Function(_) => Kind::Function,
            ObjectPayload::Part(_) => Kind::Part,
            ObjectPayload::Mixin(_) => Kind::Mixin,
            ObjectPayload::DocumentDirectory(_) => Kind::DocumentDirectory,
            ObjectPayload::Document(_) => Kind::Document,
            ObjectPayload::DocumentSection(_) => Kind::DocumentSection,
            ObjectPayload::DocumentList(_) => Kind::DocumentList,
            ObjectPayload::DocumentText(_) => Kind::DocumentText,
            ObjectPayload::DocumentTopic(_) => Kind::DocumentTopic,
            ObjectPayload::DesignStructure(_) => Kind::DesignStructure,
            ObjectPayload::System(_) => Kind::System,
            ObjectPayload::Module(_) => Kind::Module,
        }
    }

    pub fn check_kind(&self) -> bool {
        self.kind == Self::kind_from_payload(&self.payload)
    }
}

/// A commit record. The source commit belongs to the View, not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub view: ObjectHash,
    pub parent: CommitHash,
    pub message: String,
    pub author: String,
    pub timestamp: i64,
}

/// A design document is raw UTF-8 text.
pub type DesignDoc = String;

/// A symbol is raw UTF-8 text.
pub type SymbolText = String;
