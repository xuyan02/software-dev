use anyhow::{Result, bail};

use crate::hash::{GitCommit, ObjectHash, SymbolHash};
use crate::source::{SourcePath, SourceSpan};

mod accessors;

#[path = "construct.rs"]
pub mod construct;
#[path = "io.rs"]
pub mod io;
#[path = "property.rs"]
pub mod property;
#[path = "repo.rs"]
pub mod repo;

pub(crate) const OBJECT_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    TechnicalRoot,
    RequirementRoot,
    ConceptDirectory,
    TechnicalSystem,
    TechnicalModule,
    Root,
    ObjectList,
    DocumentText,
    DocumentList,
    DocumentParagraphList,
    DocumentTopic,
    DocumentCode,
    DocumentMermaid,
    DocumentHtml,
    Concept,
    RequirementUser,
    RequirementStory,
    RequirementSystem,
    RequirementModule,
    RequirementFunction,
    RequirementInteraction,
    TechnicalDirectory,
    TechnicalFile,
    TechnicalLayer,
    TechnicalNamespace,
    TechnicalClass,
    TechnicalStruct,
    TechnicalEnum,
    TechnicalInterface,
    TechnicalMethod,
    TechnicalFunction,
    TechnicalMixin,
    TechnicalPart,
    TechnicalVariable,
    TechnicalConstant,
    TechnicalMember,
    TechnicalProperty,
    TechnicalEnumValue,
    TechnicalArgument,
    TechnicalReturnValue,
    ProjectRoot,
    ProjectDocument,
}

impl ObjectKind {
    pub fn tag(self) -> u8 {
        match self {
            Self::TechnicalRoot => 0,
            Self::RequirementRoot => 1,
            Self::ConceptDirectory => 2,
            Self::TechnicalSystem => 3,
            Self::TechnicalModule => 4,
            Self::Root => 5,
            Self::ObjectList => 6,
            Self::DocumentText => 7,
            Self::DocumentList => 8,
            Self::DocumentParagraphList => 9,
            Self::DocumentTopic => 10,
            Self::DocumentCode => 11,
            Self::DocumentMermaid => 12,
            Self::DocumentHtml => 13,
            Self::Concept => 14,
            Self::RequirementUser => 16,
            Self::RequirementStory => 17,
            Self::RequirementSystem => 18,
            Self::RequirementModule => 19,
            Self::RequirementFunction => 20,
            Self::RequirementInteraction => 21,
            Self::TechnicalDirectory => 22,
            Self::TechnicalFile => 23,
            Self::TechnicalLayer => 24,
            Self::TechnicalNamespace => 25,
            Self::TechnicalClass => 26,
            Self::TechnicalStruct => 27,
            Self::TechnicalEnum => 28,
            Self::TechnicalInterface => 29,
            Self::TechnicalMethod => 30,
            Self::TechnicalFunction => 31,
            Self::TechnicalMixin => 32,
            Self::TechnicalPart => 33,
            Self::TechnicalVariable => 34,
            Self::TechnicalConstant => 35,
            Self::TechnicalMember => 36,
            Self::TechnicalProperty => 37,
            Self::TechnicalEnumValue => 38,
            Self::TechnicalArgument => 39,
            Self::TechnicalReturnValue => 40,
            Self::ProjectRoot => 41,
            Self::ProjectDocument => 42,
        }
    }

    pub fn from_tag(tag: u8) -> Result<Self> {
        Ok(match tag {
            0 => Self::TechnicalRoot,
            1 => Self::RequirementRoot,
            2 => Self::ConceptDirectory,
            3 => Self::TechnicalSystem,
            4 => Self::TechnicalModule,
            5 => Self::Root,
            6 => Self::ObjectList,
            7 => Self::DocumentText,
            8 => Self::DocumentList,
            9 => Self::DocumentParagraphList,
            10 => Self::DocumentTopic,
            11 => Self::DocumentCode,
            12 => Self::DocumentMermaid,
            13 => Self::DocumentHtml,
            14 => Self::Concept,
            16 => Self::RequirementUser,
            17 => Self::RequirementStory,
            18 => Self::RequirementSystem,
            19 => Self::RequirementModule,
            20 => Self::RequirementFunction,
            21 => Self::RequirementInteraction,
            22 => Self::TechnicalDirectory,
            23 => Self::TechnicalFile,
            24 => Self::TechnicalLayer,
            25 => Self::TechnicalNamespace,
            26 => Self::TechnicalClass,
            27 => Self::TechnicalStruct,
            28 => Self::TechnicalEnum,
            29 => Self::TechnicalInterface,
            30 => Self::TechnicalMethod,
            31 => Self::TechnicalFunction,
            32 => Self::TechnicalMixin,
            33 => Self::TechnicalPart,
            34 => Self::TechnicalVariable,
            35 => Self::TechnicalConstant,
            36 => Self::TechnicalMember,
            37 => Self::TechnicalProperty,
            38 => Self::TechnicalEnumValue,
            39 => Self::TechnicalArgument,
            40 => Self::TechnicalReturnValue,
            41 => Self::ProjectRoot,
            42 => Self::ProjectDocument,
            _ => bail!("unknown object kind tag {tag}"),
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::TechnicalRoot => "TechnicalRoot",
            Self::RequirementRoot => "RequirementRoot",
            Self::ConceptDirectory => "ConceptDirectory",
            Self::TechnicalSystem => "TechnicalSystem",
            Self::TechnicalModule => "TechnicalModule",
            Self::Root => "Root",
            Self::ObjectList => "ObjectList",
            Self::DocumentText => "DocumentText",
            Self::DocumentList => "DocumentList",
            Self::DocumentParagraphList => "DocumentParagraphList",
            Self::DocumentTopic => "DocumentTopic",
            Self::DocumentCode => "DocumentCode",
            Self::DocumentMermaid => "DocumentMermaid",
            Self::DocumentHtml => "DocumentHtml",
            Self::Concept => "Concept",
            Self::RequirementUser => "RequirementUser",
            Self::RequirementStory => "RequirementStory",
            Self::RequirementSystem => "RequirementSystem",
            Self::RequirementModule => "RequirementModule",
            Self::RequirementFunction => "RequirementFunction",
            Self::RequirementInteraction => "RequirementInteraction",
            Self::TechnicalDirectory => "TechnicalDirectory",
            Self::TechnicalFile => "TechnicalFile",
            Self::TechnicalLayer => "TechnicalLayer",
            Self::TechnicalNamespace => "TechnicalNamespace",
            Self::TechnicalClass => "TechnicalClass",
            Self::TechnicalStruct => "TechnicalStruct",
            Self::TechnicalEnum => "TechnicalEnum",
            Self::TechnicalInterface => "TechnicalInterface",
            Self::TechnicalMethod => "TechnicalMethod",
            Self::TechnicalFunction => "TechnicalFunction",
            Self::TechnicalMixin => "TechnicalMixin",
            Self::TechnicalPart => "TechnicalPart",
            Self::TechnicalVariable => "TechnicalVariable",
            Self::TechnicalConstant => "TechnicalConstant",
            Self::TechnicalMember => "TechnicalMember",
            Self::TechnicalProperty => "TechnicalProperty",
            Self::TechnicalEnumValue => "TechnicalEnumValue",
            Self::TechnicalArgument => "TechnicalArgument",
            Self::TechnicalReturnValue => "TechnicalReturnValue",
            Self::ProjectRoot => "ProjectRoot",
            Self::ProjectDocument => "ProjectDocument",
        }
    }
}

// `qname` and `name` are the object's own identity, not cross-object references.
// Every other SymbolHash/ObjectHash field carries an inline comment naming the
// concrete target kind(s); these comments are the source for dynamic validation.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalRoot {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,    // Root qname
    file: ObjectHash,      // ObjectList<TechnicalFile>
    directory: ObjectHash, // ObjectList<TechnicalDirectory>
    namespace: ObjectHash, // ObjectList<TechnicalNamespace>
    system: ObjectHash,    // ObjectList<TechnicalSystem>
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequirementRoot {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // Root qname
    user: ObjectHash,   // ObjectList<RequirementUser>
    story: ObjectHash,  // ObjectList<RequirementStory>
    system: ObjectHash, // ObjectList<RequirementSystem>
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConceptDirectory {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,      // ConceptDirectory or Root qname
    directories: ObjectHash, // ObjectList<ConceptDirectory>
    concepts: ObjectHash,    // ObjectList<Concept>
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectRoot {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,   // Root qname
    document: ObjectHash, // ProjectDocument
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectDocument {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,   // ProjectRoot qname
    document: ObjectHash, // DocumentParagraphList (the project document tree)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalSystem {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,                // TechnicalRoot qname
    members: ObjectHash,               // ObjectList<code type | TechnicalFunction>
    modules: ObjectHash,               // ObjectList<TechnicalModule>
    layers: ObjectHash,                // ObjectList<TechnicalLayer>
    constants: ObjectHash,             // ObjectList<TechnicalConstant>
    variables: ObjectHash,             // ObjectList<TechnicalVariable>
    position: ObjectHash,              // DocumentText
    responsibilities: ObjectHash,      // DocumentList
    non_responsibilities: ObjectHash,  // DocumentList
    features: ObjectHash,              // DocumentList
    non_features: ObjectHash,          // DocumentList
    invariants: ObjectHash,            // DocumentList
    constraints: ObjectHash,           // DocumentList
    design_principles: ObjectHash,     // DocumentList
    external_dependencies: ObjectHash, // DocumentList
    usage: ObjectHash,                 // DocumentParagraphList
    design_points: ObjectHash,         // DocumentParagraphList
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalModule {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // TechnicalSystem | TechnicalModule | TechnicalLayer qname
    members: ObjectHash, // ObjectList<code type | TechnicalFunction>
    dependencies: Vec<SymbolHash>, // Vec<technical type qname>
    constants: ObjectHash, // ObjectList<TechnicalConstant>
    variables: ObjectHash, // ObjectList<TechnicalVariable>
    position: ObjectHash, // DocumentText
    responsibilities: ObjectHash, // DocumentList
    non_responsibilities: ObjectHash, // DocumentList
    features: ObjectHash, // DocumentList
    non_features: ObjectHash, // DocumentList
    invariants: ObjectHash, // DocumentList
    constraints: ObjectHash, // DocumentList
    design_principles: ObjectHash, // DocumentList
    usage: ObjectHash,  // DocumentParagraphList
    design_points: ObjectHash, // DocumentParagraphList
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Root {
    qname: SymbolHash,
    name: SymbolHash,
    source_commit: GitCommit,
    technical: ObjectHash,   // TechnicalRoot
    requirement: ObjectHash, // RequirementRoot
    concept: ObjectHash,     // ConceptDirectory
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectList {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,     // owner object qname
    items: Vec<ObjectHash>, // element type is fixed by the owning field
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentText {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // owner object qname
    content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentList {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,     // owner object qname
    items: Vec<ObjectHash>, // DocumentText
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentParagraphList {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,     // owner object qname
    items: Vec<ObjectHash>, // DocumentText | DocumentList | DocumentTopic | DocumentCode | DocumentMermaid | DocumentHtml
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentTopic {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // DocumentParagraphList qname
    title: String,
    body: ObjectHash, // DocumentParagraphList
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentCode {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // owner object qname
    language: String,
    source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentMermaid {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // owner object qname
    source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentHtml {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // owner object qname
    source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Concept {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // ConceptDirectory qname
    display_name: String,
    explanation: ObjectHash, // DocumentParagraphList
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequirementUser {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // RequirementRoot qname
    real_name: String,
    age: String,
    occupation: String,
    situations: ObjectHash, // DocumentList
    hobbies: ObjectHash,    // DocumentList
    needs: ObjectHash,      // DocumentList
    priority: u8,
    stories: Vec<SymbolHash>, // Vec<RequirementStory qname>
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequirementStory {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,        // RequirementRoot qname
    preconditions: ObjectHash, // DocumentList
    frequency: u8,
    description: ObjectHash,    // DocumentParagraphList
    requirement: ObjectHash,    // DocumentList
    users: Vec<SymbolHash>,     // Vec<RequirementUser qname>
    functions: Vec<SymbolHash>, // Vec<RequirementFunction qname>
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequirementSystem {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,           // RequirementRoot qname
    modules: ObjectHash,          // ObjectList<RequirementModule>
    form: ObjectHash,             // DocumentText
    covered_users: ObjectHash,    // DocumentList
    closed_loop_goal: ObjectHash, // DocumentParagraphList
    relationships: ObjectHash,    // DocumentParagraphList
    boundary: ObjectHash,         // DocumentList
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequirementModule {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,            // RequirementSystem qname
    functions: ObjectHash,         // ObjectList<RequirementFunction>
    core_concept: ObjectHash,      // DocumentText
    included_concepts: ObjectHash, // DocumentList
    concept_relations: ObjectHash, // DocumentParagraphList
    boundary: ObjectHash,          // DocumentList
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequirementFunction {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,        // RequirementModule qname
    flow: ObjectHash,          // DocumentMermaid
    interactions: ObjectHash,  // ObjectList<RequirementInteraction>
    initiator: ObjectHash,     // DocumentText
    system_change: ObjectHash, // DocumentParagraphList
    preconditions: ObjectHash, // DocumentParagraphList
    feedback: ObjectHash,      // DocumentText
    description: ObjectHash,   // DocumentParagraphList
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequirementInteraction {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,      // RequirementFunction qname
    description: ObjectHash, // DocumentParagraphList
    ui: ObjectHash,          // DocumentParagraphList
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalDirectory {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // TechnicalRoot | TechnicalDirectory qname
    source: SourcePath,
    children: ObjectHash, // ObjectList<TechnicalDirectory | TechnicalFile>
    position: ObjectHash, // DocumentText
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalFile {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // TechnicalDirectory qname
    source: SourcePath,
    entities: ObjectHash, // ObjectList<code type | TechnicalFunction>
    position: ObjectHash, // DocumentText
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalLayer {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,            // TechnicalSystem | TechnicalModule qname
    members: ObjectHash,           // ObjectList<code type | TechnicalFunction>
    dependencies: Vec<SymbolHash>, // Vec<technical type qname>
    position: ObjectHash,          // DocumentText
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalNamespace {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,    // TechnicalRoot | TechnicalNamespace qname
    children: ObjectHash,  // ObjectList<TechnicalNamespace>
    entities: ObjectHash,  // ObjectList<code type | TechnicalFunction>
    constants: ObjectHash, // ObjectList<TechnicalConstant>
    variables: ObjectHash, // ObjectList<TechnicalVariable>
    position: ObjectHash,  // DocumentText
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalClass {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,    // owner qname
    namespace: SymbolHash, // TechnicalNamespace qname
    file: SymbolHash,      // TechnicalFile qname
    module: SymbolHash,    // TechnicalModule qname
    source: SourceSpan,
    superclass: SymbolHash,                 // TechnicalClass qname
    interfaces: Vec<SymbolHash>,            // Vec<TechnicalInterface qname>
    mixins: Vec<SymbolHash>,                // Vec<TechnicalMixin qname>
    explicit_dependencies: Vec<SymbolHash>, // Vec<technical type qname>
    implicit_dependencies: Vec<SymbolHash>, // Vec<technical type qname>
    method: ObjectHash,                     // ObjectList<TechnicalMethod>
    types: ObjectHash,                      // ObjectList<code type>
    parts: ObjectHash,                      // ObjectList<TechnicalPart>
    constants: ObjectHash,                  // ObjectList<TechnicalConstant>
    members: ObjectHash,                    // ObjectList<TechnicalMember>
    properties: ObjectHash,                 // ObjectList<TechnicalProperty>
    position: ObjectHash,                   // DocumentText
    responsibilities: ObjectHash,           // DocumentList
    non_responsibilities: ObjectHash,       // DocumentList
    features: ObjectHash,                   // DocumentList
    non_features: ObjectHash,               // DocumentList
    invariants: ObjectHash,                 // DocumentList
    constraints: ObjectHash,                // DocumentList
    design_principles: ObjectHash,          // DocumentList
    usage: ObjectHash,                      // DocumentParagraphList
    design_points: ObjectHash,              // DocumentParagraphList
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalStruct {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,    // owner qname
    namespace: SymbolHash, // TechnicalNamespace qname
    file: SymbolHash,      // TechnicalFile qname
    module: SymbolHash,    // TechnicalModule qname
    source: SourceSpan,
    dependencies: Vec<SymbolHash>, // Vec<technical type qname>
    method: ObjectHash,            // ObjectList<TechnicalMethod>
    types: ObjectHash,             // ObjectList<code type>
    constants: ObjectHash,         // ObjectList<TechnicalConstant>
    members: ObjectHash,           // ObjectList<TechnicalMember>
    position: ObjectHash,          // DocumentText
    invariants: ObjectHash,        // DocumentList
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalEnum {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,    // owner qname
    namespace: SymbolHash, // TechnicalNamespace qname
    file: SymbolHash,      // TechnicalFile qname
    module: SymbolHash,    // TechnicalModule qname
    source: SourceSpan,
    dependencies: Vec<SymbolHash>, // Vec<technical type qname>
    values: ObjectHash,            // ObjectList<TechnicalEnumValue>
    position: ObjectHash,          // DocumentText
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalInterface {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,    // owner qname
    namespace: SymbolHash, // TechnicalNamespace qname
    file: SymbolHash,      // TechnicalFile qname
    module: SymbolHash,    // TechnicalModule qname
    source: SourceSpan,
    extends: SymbolHash,              // TechnicalInterface qname
    dependencies: Vec<SymbolHash>,    // Vec<technical type qname>
    methods: ObjectHash,              // ObjectList<TechnicalMethod>
    position: ObjectHash,             // DocumentText
    responsibilities: ObjectHash,     // DocumentList
    non_responsibilities: ObjectHash, // DocumentList
    features: ObjectHash,             // DocumentList
    non_features: ObjectHash,         // DocumentList
    invariants: ObjectHash,           // DocumentList
    usage: ObjectHash,                // DocumentParagraphList
    design_points: ObjectHash,        // DocumentParagraphList
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalMethod {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // TechnicalClass | TechnicalInterface | TechnicalMixin qname
    source: SourceSpan,
    is_static: bool,
    is_async: bool,
    is_abstract: bool,
    is_const: bool,
    overrides: SymbolHash,                  // TechnicalMethod qname
    explicit_dependencies: Vec<SymbolHash>, // Vec<technical type qname>
    implicit_dependencies: Vec<SymbolHash>, // Vec<technical type qname>
    position: ObjectHash,                   // DocumentText
    arguments: ObjectHash,                  // ObjectList<TechnicalArgument>
    return_value: ObjectHash,               // TechnicalReturnValue
    usage: ObjectHash,                      // DocumentParagraphList
    design_points: ObjectHash,              // DocumentParagraphList
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalFunction {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,    // TechnicalNamespace qname
    namespace: SymbolHash, // TechnicalNamespace qname
    file: SymbolHash,      // TechnicalFile qname
    module: SymbolHash,    // TechnicalModule qname
    source: SourceSpan,
    is_async: bool,
    explicit_dependencies: Vec<SymbolHash>, // Vec<technical type qname>
    implicit_dependencies: Vec<SymbolHash>, // Vec<technical type qname>
    position: ObjectHash,                   // DocumentText
    arguments: ObjectHash,                  // ObjectList<TechnicalArgument>
    return_value: ObjectHash,               // TechnicalReturnValue
    usage: ObjectHash,                      // DocumentParagraphList
    design_points: ObjectHash,              // DocumentParagraphList
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalMixin {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash,    // owner qname
    namespace: SymbolHash, // TechnicalNamespace qname
    file: SymbolHash,      // TechnicalFile qname
    module: SymbolHash,    // TechnicalModule qname
    source: SourceSpan,
    on: Vec<SymbolHash>,                    // Vec<technical type qname>
    superclass: SymbolHash,                 // TechnicalClass qname
    interfaces: Vec<SymbolHash>,            // Vec<TechnicalInterface qname>
    mixins: Vec<SymbolHash>,                // Vec<TechnicalMixin qname>
    explicit_dependencies: Vec<SymbolHash>, // Vec<technical type qname>
    implicit_dependencies: Vec<SymbolHash>, // Vec<technical type qname>
    methods: ObjectHash,                    // ObjectList<TechnicalMethod>
    types: ObjectHash,                      // ObjectList<code type>
    position: ObjectHash,                   // DocumentText
    responsibilities: ObjectHash,           // DocumentList
    non_responsibilities: ObjectHash,       // DocumentList
    features: ObjectHash,                   // DocumentList
    non_features: ObjectHash,               // DocumentList
    invariants: ObjectHash,                 // DocumentList
    usage: ObjectHash,                      // DocumentParagraphList
    design_points: ObjectHash,              // DocumentParagraphList
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalPart {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // TechnicalClass qname
    source: SourceSpan,
    explicit_dependencies: Vec<SymbolHash>, // Vec<technical type qname>
    implicit_dependencies: Vec<SymbolHash>, // Vec<technical type qname>
    members: ObjectHash,                    // ObjectList<TechnicalMember>
    methods: ObjectHash,                    // ObjectList<TechnicalMethod>
    types: ObjectHash,                      // ObjectList<code type>
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalVariable {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // TechnicalSystem | TechnicalModule | TechnicalNamespace qname
    source: SourceSpan,
    ty: SymbolHash,       // type qname
    position: ObjectHash, // DocumentText
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalConstant {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // TechnicalSystem | TechnicalModule | TechnicalNamespace | TechnicalClass | TechnicalStruct qname
    source: SourceSpan,
    ty: SymbolHash,       // type qname
    position: ObjectHash, // DocumentText
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalMember {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // TechnicalClass | TechnicalStruct qname
    source: SourceSpan,
    ty: SymbolHash, // type qname
    is_static: bool,
    is_const: bool,
    position: ObjectHash, // DocumentText
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalProperty {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // TechnicalClass qname
    ty: SymbolHash,     // type qname
    getter: SourceSpan,
    setter: SourceSpan,
    position: ObjectHash, // DocumentText
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalEnumValue {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // TechnicalEnum qname
    source: SourceSpan,
    position: ObjectHash, // DocumentText
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalArgument {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // TechnicalMethod | TechnicalFunction qname
    source: SourceSpan,
    ty: SymbolHash,       // type qname
    position: ObjectHash, // DocumentText
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnicalReturnValue {
    qname: SymbolHash,
    name: SymbolHash,
    parent: SymbolHash, // TechnicalMethod | TechnicalFunction qname
    source: SourceSpan,
    ty: SymbolHash,       // type qname
    position: ObjectHash, // DocumentText
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Object {
    TechnicalRoot(TechnicalRoot),
    RequirementRoot(RequirementRoot),
    ConceptDirectory(ConceptDirectory),
    TechnicalSystem(TechnicalSystem),
    TechnicalModule(TechnicalModule),
    Root(Root),
    ObjectList(ObjectList),
    DocumentText(DocumentText),
    DocumentList(DocumentList),
    DocumentParagraphList(DocumentParagraphList),
    DocumentTopic(DocumentTopic),
    DocumentCode(DocumentCode),
    DocumentMermaid(DocumentMermaid),
    DocumentHtml(DocumentHtml),
    Concept(Concept),
    RequirementUser(RequirementUser),
    RequirementStory(RequirementStory),
    RequirementSystem(RequirementSystem),
    RequirementModule(RequirementModule),
    RequirementFunction(RequirementFunction),
    RequirementInteraction(RequirementInteraction),
    TechnicalDirectory(TechnicalDirectory),
    TechnicalFile(TechnicalFile),
    TechnicalLayer(TechnicalLayer),
    TechnicalNamespace(TechnicalNamespace),
    TechnicalClass(TechnicalClass),
    TechnicalStruct(TechnicalStruct),
    TechnicalEnum(TechnicalEnum),
    TechnicalInterface(TechnicalInterface),
    TechnicalMethod(TechnicalMethod),
    TechnicalFunction(TechnicalFunction),
    TechnicalMixin(TechnicalMixin),
    TechnicalPart(TechnicalPart),
    TechnicalVariable(TechnicalVariable),
    TechnicalConstant(TechnicalConstant),
    TechnicalMember(TechnicalMember),
    TechnicalProperty(TechnicalProperty),
    TechnicalEnumValue(TechnicalEnumValue),
    TechnicalArgument(TechnicalArgument),
    TechnicalReturnValue(TechnicalReturnValue),
    ProjectRoot(ProjectRoot),
    ProjectDocument(ProjectDocument),
}

impl Object {
    pub fn kind(&self) -> ObjectKind {
        match self {
            Self::TechnicalRoot(_) => ObjectKind::TechnicalRoot,
            Self::RequirementRoot(_) => ObjectKind::RequirementRoot,
            Self::ConceptDirectory(_) => ObjectKind::ConceptDirectory,
            Self::TechnicalSystem(_) => ObjectKind::TechnicalSystem,
            Self::TechnicalModule(_) => ObjectKind::TechnicalModule,
            Self::Root(_) => ObjectKind::Root,
            Self::ObjectList(_) => ObjectKind::ObjectList,
            Self::DocumentText(_) => ObjectKind::DocumentText,
            Self::DocumentList(_) => ObjectKind::DocumentList,
            Self::DocumentParagraphList(_) => ObjectKind::DocumentParagraphList,
            Self::DocumentTopic(_) => ObjectKind::DocumentTopic,
            Self::DocumentCode(_) => ObjectKind::DocumentCode,
            Self::DocumentMermaid(_) => ObjectKind::DocumentMermaid,
            Self::DocumentHtml(_) => ObjectKind::DocumentHtml,
            Self::Concept(_) => ObjectKind::Concept,
            Self::RequirementUser(_) => ObjectKind::RequirementUser,
            Self::RequirementStory(_) => ObjectKind::RequirementStory,
            Self::RequirementSystem(_) => ObjectKind::RequirementSystem,
            Self::RequirementModule(_) => ObjectKind::RequirementModule,
            Self::RequirementFunction(_) => ObjectKind::RequirementFunction,
            Self::RequirementInteraction(_) => ObjectKind::RequirementInteraction,
            Self::TechnicalDirectory(_) => ObjectKind::TechnicalDirectory,
            Self::TechnicalFile(_) => ObjectKind::TechnicalFile,
            Self::TechnicalLayer(_) => ObjectKind::TechnicalLayer,
            Self::TechnicalNamespace(_) => ObjectKind::TechnicalNamespace,
            Self::TechnicalClass(_) => ObjectKind::TechnicalClass,
            Self::TechnicalStruct(_) => ObjectKind::TechnicalStruct,
            Self::TechnicalEnum(_) => ObjectKind::TechnicalEnum,
            Self::TechnicalInterface(_) => ObjectKind::TechnicalInterface,
            Self::TechnicalMethod(_) => ObjectKind::TechnicalMethod,
            Self::TechnicalFunction(_) => ObjectKind::TechnicalFunction,
            Self::TechnicalMixin(_) => ObjectKind::TechnicalMixin,
            Self::TechnicalPart(_) => ObjectKind::TechnicalPart,
            Self::TechnicalVariable(_) => ObjectKind::TechnicalVariable,
            Self::TechnicalConstant(_) => ObjectKind::TechnicalConstant,
            Self::TechnicalMember(_) => ObjectKind::TechnicalMember,
            Self::TechnicalProperty(_) => ObjectKind::TechnicalProperty,
            Self::TechnicalEnumValue(_) => ObjectKind::TechnicalEnumValue,
            Self::TechnicalArgument(_) => ObjectKind::TechnicalArgument,
            Self::TechnicalReturnValue(_) => ObjectKind::TechnicalReturnValue,
            Self::ProjectRoot(_) => ObjectKind::ProjectRoot,
            Self::ProjectDocument(_) => ObjectKind::ProjectDocument,
        }
    }

    pub fn qname(&self) -> SymbolHash {
        match self {
            Self::TechnicalRoot(value) => value.qname,
            Self::RequirementRoot(value) => value.qname,
            Self::ConceptDirectory(value) => value.qname,
            Self::TechnicalSystem(value) => value.qname,
            Self::TechnicalModule(value) => value.qname,
            Self::Root(value) => value.qname,
            Self::ObjectList(value) => value.qname,
            Self::DocumentText(value) => value.qname,
            Self::DocumentList(value) => value.qname,
            Self::DocumentParagraphList(value) => value.qname,
            Self::DocumentTopic(value) => value.qname,
            Self::DocumentCode(value) => value.qname,
            Self::DocumentMermaid(value) => value.qname,
            Self::DocumentHtml(value) => value.qname,
            Self::Concept(value) => value.qname,
            Self::RequirementUser(value) => value.qname,
            Self::RequirementStory(value) => value.qname,
            Self::RequirementSystem(value) => value.qname,
            Self::RequirementModule(value) => value.qname,
            Self::RequirementFunction(value) => value.qname,
            Self::RequirementInteraction(value) => value.qname,
            Self::TechnicalDirectory(value) => value.qname,
            Self::TechnicalFile(value) => value.qname,
            Self::TechnicalLayer(value) => value.qname,
            Self::TechnicalNamespace(value) => value.qname,
            Self::TechnicalClass(value) => value.qname,
            Self::TechnicalStruct(value) => value.qname,
            Self::TechnicalEnum(value) => value.qname,
            Self::TechnicalInterface(value) => value.qname,
            Self::TechnicalMethod(value) => value.qname,
            Self::TechnicalFunction(value) => value.qname,
            Self::TechnicalMixin(value) => value.qname,
            Self::TechnicalPart(value) => value.qname,
            Self::TechnicalVariable(value) => value.qname,
            Self::TechnicalConstant(value) => value.qname,
            Self::TechnicalMember(value) => value.qname,
            Self::TechnicalProperty(value) => value.qname,
            Self::TechnicalEnumValue(value) => value.qname,
            Self::TechnicalArgument(value) => value.qname,
            Self::TechnicalReturnValue(value) => value.qname,
            Self::ProjectRoot(value) => value.qname,
            Self::ProjectDocument(value) => value.qname,
        }
    }

    pub fn for_each_reference_mut(&mut self, mut f: impl FnMut(&mut ObjectHash)) {
        let f = &mut f;
        match self {
            Self::TechnicalRoot(value) => {
                opt_ref(f, &mut value.file);
                opt_ref(f, &mut value.directory);
                opt_ref(f, &mut value.namespace);
                opt_ref(f, &mut value.system);
            }
            Self::RequirementRoot(value) => {
                opt_ref(f, &mut value.user);
                opt_ref(f, &mut value.story);
                opt_ref(f, &mut value.system);
            }
            Self::ConceptDirectory(value) => {
                opt_ref(f, &mut value.directories);
                opt_ref(f, &mut value.concepts);
            }
            Self::TechnicalSystem(value) => {
                opt_ref(f, &mut value.members);
                opt_ref(f, &mut value.modules);
                opt_ref(f, &mut value.layers);
                opt_ref(f, &mut value.constants);
                opt_ref(f, &mut value.variables);
                opt_ref(f, &mut value.position);
                opt_ref(f, &mut value.responsibilities);
                opt_ref(f, &mut value.non_responsibilities);
                opt_ref(f, &mut value.features);
                opt_ref(f, &mut value.non_features);
                opt_ref(f, &mut value.invariants);
                opt_ref(f, &mut value.constraints);
                opt_ref(f, &mut value.design_principles);
                opt_ref(f, &mut value.external_dependencies);
                opt_ref(f, &mut value.usage);
                opt_ref(f, &mut value.design_points);
            }
            Self::TechnicalModule(value) => {
                opt_ref(f, &mut value.members);
                opt_ref(f, &mut value.constants);
                opt_ref(f, &mut value.variables);
                opt_ref(f, &mut value.position);
                opt_ref(f, &mut value.responsibilities);
                opt_ref(f, &mut value.non_responsibilities);
                opt_ref(f, &mut value.features);
                opt_ref(f, &mut value.non_features);
                opt_ref(f, &mut value.invariants);
                opt_ref(f, &mut value.constraints);
                opt_ref(f, &mut value.design_principles);
                opt_ref(f, &mut value.usage);
                opt_ref(f, &mut value.design_points);
            }
            Self::Root(value) => {
                f(&mut value.technical);
                f(&mut value.requirement);
                f(&mut value.concept);
            }
            Self::ObjectList(value) => list_ref(f, &mut value.items),
            Self::DocumentList(value) => list_ref(f, &mut value.items),
            Self::DocumentParagraphList(value) => list_ref(f, &mut value.items),
            Self::DocumentTopic(value) => f(&mut value.body),
            Self::Concept(value) => opt_ref(f, &mut value.explanation),
            Self::RequirementUser(value) => {
                opt_ref(f, &mut value.situations);
                opt_ref(f, &mut value.hobbies);
                opt_ref(f, &mut value.needs);
            }
            Self::RequirementStory(value) => {
                opt_ref(f, &mut value.preconditions);
                opt_ref(f, &mut value.description);
                opt_ref(f, &mut value.requirement);
            }
            Self::RequirementSystem(value) => {
                opt_ref(f, &mut value.modules);
                opt_ref(f, &mut value.form);
                opt_ref(f, &mut value.covered_users);
                opt_ref(f, &mut value.closed_loop_goal);
                opt_ref(f, &mut value.relationships);
                opt_ref(f, &mut value.boundary);
            }
            Self::RequirementModule(value) => {
                opt_ref(f, &mut value.functions);
                opt_ref(f, &mut value.core_concept);
                opt_ref(f, &mut value.included_concepts);
                opt_ref(f, &mut value.concept_relations);
                opt_ref(f, &mut value.boundary);
            }
            Self::RequirementFunction(value) => {
                opt_ref(f, &mut value.flow);
                opt_ref(f, &mut value.interactions);
                opt_ref(f, &mut value.initiator);
                opt_ref(f, &mut value.system_change);
                opt_ref(f, &mut value.preconditions);
                opt_ref(f, &mut value.feedback);
                opt_ref(f, &mut value.description);
            }
            Self::RequirementInteraction(value) => {
                opt_ref(f, &mut value.description);
                opt_ref(f, &mut value.ui);
            }
            Self::TechnicalDirectory(value) => {
                opt_ref(f, &mut value.children);
                opt_ref(f, &mut value.position);
            }
            Self::TechnicalFile(value) => {
                opt_ref(f, &mut value.entities);
                opt_ref(f, &mut value.position);
            }
            Self::TechnicalLayer(value) => {
                opt_ref(f, &mut value.members);
                opt_ref(f, &mut value.position);
            }
            Self::TechnicalNamespace(value) => {
                opt_ref(f, &mut value.children);
                opt_ref(f, &mut value.entities);
                opt_ref(f, &mut value.constants);
                opt_ref(f, &mut value.variables);
                opt_ref(f, &mut value.position);
            }
            Self::TechnicalClass(value) => {
                opt_ref(f, &mut value.method);
                opt_ref(f, &mut value.types);
                opt_ref(f, &mut value.parts);
                opt_ref(f, &mut value.constants);
                opt_ref(f, &mut value.members);
                opt_ref(f, &mut value.properties);
                opt_ref(f, &mut value.position);
                opt_ref(f, &mut value.responsibilities);
                opt_ref(f, &mut value.non_responsibilities);
                opt_ref(f, &mut value.features);
                opt_ref(f, &mut value.non_features);
                opt_ref(f, &mut value.invariants);
                opt_ref(f, &mut value.constraints);
                opt_ref(f, &mut value.design_principles);
                opt_ref(f, &mut value.usage);
                opt_ref(f, &mut value.design_points);
            }
            Self::TechnicalStruct(value) => {
                opt_ref(f, &mut value.method);
                opt_ref(f, &mut value.types);
                opt_ref(f, &mut value.constants);
                opt_ref(f, &mut value.members);
                opt_ref(f, &mut value.position);
                opt_ref(f, &mut value.invariants);
            }
            Self::TechnicalEnum(value) => {
                opt_ref(f, &mut value.values);
                opt_ref(f, &mut value.position);
            }
            Self::TechnicalInterface(value) => {
                opt_ref(f, &mut value.methods);
                opt_ref(f, &mut value.position);
                opt_ref(f, &mut value.responsibilities);
                opt_ref(f, &mut value.non_responsibilities);
                opt_ref(f, &mut value.features);
                opt_ref(f, &mut value.non_features);
                opt_ref(f, &mut value.invariants);
                opt_ref(f, &mut value.usage);
                opt_ref(f, &mut value.design_points);
            }
            Self::TechnicalMethod(value) => {
                opt_ref(f, &mut value.position);
                opt_ref(f, &mut value.arguments);
                opt_ref(f, &mut value.return_value);
                opt_ref(f, &mut value.usage);
                opt_ref(f, &mut value.design_points);
            }
            Self::TechnicalFunction(value) => {
                opt_ref(f, &mut value.position);
                opt_ref(f, &mut value.arguments);
                opt_ref(f, &mut value.return_value);
                opt_ref(f, &mut value.usage);
                opt_ref(f, &mut value.design_points);
            }
            Self::TechnicalMixin(value) => {
                opt_ref(f, &mut value.methods);
                opt_ref(f, &mut value.types);
                opt_ref(f, &mut value.position);
                opt_ref(f, &mut value.responsibilities);
                opt_ref(f, &mut value.non_responsibilities);
                opt_ref(f, &mut value.features);
                opt_ref(f, &mut value.non_features);
                opt_ref(f, &mut value.invariants);
                opt_ref(f, &mut value.usage);
                opt_ref(f, &mut value.design_points);
            }
            Self::TechnicalPart(value) => {
                opt_ref(f, &mut value.members);
                opt_ref(f, &mut value.methods);
                opt_ref(f, &mut value.types);
            }
            Self::TechnicalVariable(value) => opt_ref(f, &mut value.position),
            Self::TechnicalConstant(value) => opt_ref(f, &mut value.position),
            Self::TechnicalMember(value) => opt_ref(f, &mut value.position),
            Self::TechnicalProperty(value) => opt_ref(f, &mut value.position),
            Self::TechnicalEnumValue(value) => opt_ref(f, &mut value.position),
            Self::TechnicalArgument(value) => opt_ref(f, &mut value.position),
            Self::TechnicalReturnValue(value) => opt_ref(f, &mut value.position),
            Self::ProjectRoot(value) => opt_ref(f, &mut value.document),
            Self::ProjectDocument(value) => opt_ref(f, &mut value.document),
            Self::DocumentText(_)
            | Self::DocumentCode(_)
            | Self::DocumentMermaid(_)
            | Self::DocumentHtml(_) => {}
        }
    }

    pub fn replace_reference(&mut self, old: ObjectHash, new: ObjectHash) -> bool {
        let mut changed = false;
        self.for_each_reference_mut(|value| {
            if *value == old {
                *value = new;
                changed = true;
            }
        });
        changed
    }

    pub fn parent(&self) -> SymbolHash {
        match self {
            Self::TechnicalRoot(value) => value.parent,
            Self::RequirementRoot(value) => value.parent,
            Self::ConceptDirectory(value) => value.parent,
            Self::TechnicalSystem(value) => value.parent,
            Self::TechnicalModule(value) => value.parent,
            Self::Root(_) => SymbolHash::ZERO,
            Self::ObjectList(value) => value.parent,
            Self::DocumentText(value) => value.parent,
            Self::DocumentList(value) => value.parent,
            Self::DocumentParagraphList(value) => value.parent,
            Self::DocumentTopic(value) => value.parent,
            Self::DocumentCode(value) => value.parent,
            Self::DocumentMermaid(value) => value.parent,
            Self::DocumentHtml(value) => value.parent,
            Self::Concept(value) => value.parent,
            Self::RequirementUser(value) => value.parent,
            Self::RequirementStory(value) => value.parent,
            Self::RequirementSystem(value) => value.parent,
            Self::RequirementModule(value) => value.parent,
            Self::RequirementFunction(value) => value.parent,
            Self::RequirementInteraction(value) => value.parent,
            Self::TechnicalDirectory(value) => value.parent,
            Self::TechnicalFile(value) => value.parent,
            Self::TechnicalLayer(value) => value.parent,
            Self::TechnicalNamespace(value) => value.parent,
            Self::TechnicalClass(value) => value.parent,
            Self::TechnicalStruct(value) => value.parent,
            Self::TechnicalEnum(value) => value.parent,
            Self::TechnicalInterface(value) => value.parent,
            Self::TechnicalMethod(value) => value.parent,
            Self::TechnicalFunction(value) => value.parent,
            Self::TechnicalMixin(value) => value.parent,
            Self::TechnicalPart(value) => value.parent,
            Self::TechnicalVariable(value) => value.parent,
            Self::TechnicalConstant(value) => value.parent,
            Self::TechnicalMember(value) => value.parent,
            Self::TechnicalProperty(value) => value.parent,
            Self::TechnicalEnumValue(value) => value.parent,
            Self::TechnicalArgument(value) => value.parent,
            Self::TechnicalReturnValue(value) => value.parent,
            Self::ProjectRoot(value) => value.parent,
            Self::ProjectDocument(value) => value.parent,
        }
    }

    pub fn references(&self) -> Vec<ObjectHash> {
        let mut references = Vec::new();
        let mut copy = self.clone();
        copy.for_each_reference_mut(|value| references.push(*value));
        references
    }

    pub fn name(&self) -> SymbolHash {
        match self {
            Self::TechnicalRoot(value) => value.name,
            Self::RequirementRoot(value) => value.name,
            Self::ConceptDirectory(value) => value.name,
            Self::TechnicalSystem(value) => value.name,
            Self::TechnicalModule(value) => value.name,
            Self::Root(value) => value.name,
            Self::ObjectList(value) => value.name,
            Self::DocumentText(value) => value.name,
            Self::DocumentList(value) => value.name,
            Self::DocumentParagraphList(value) => value.name,
            Self::DocumentTopic(value) => value.name,
            Self::DocumentCode(value) => value.name,
            Self::DocumentMermaid(value) => value.name,
            Self::DocumentHtml(value) => value.name,
            Self::Concept(value) => value.name,
            Self::RequirementUser(value) => value.name,
            Self::RequirementStory(value) => value.name,
            Self::RequirementSystem(value) => value.name,
            Self::RequirementModule(value) => value.name,
            Self::RequirementFunction(value) => value.name,
            Self::RequirementInteraction(value) => value.name,
            Self::TechnicalDirectory(value) => value.name,
            Self::TechnicalFile(value) => value.name,
            Self::TechnicalLayer(value) => value.name,
            Self::TechnicalNamespace(value) => value.name,
            Self::TechnicalClass(value) => value.name,
            Self::TechnicalStruct(value) => value.name,
            Self::TechnicalEnum(value) => value.name,
            Self::TechnicalInterface(value) => value.name,
            Self::TechnicalMethod(value) => value.name,
            Self::TechnicalFunction(value) => value.name,
            Self::TechnicalMixin(value) => value.name,
            Self::TechnicalPart(value) => value.name,
            Self::TechnicalVariable(value) => value.name,
            Self::TechnicalConstant(value) => value.name,
            Self::TechnicalMember(value) => value.name,
            Self::TechnicalProperty(value) => value.name,
            Self::TechnicalEnumValue(value) => value.name,
            Self::TechnicalArgument(value) => value.name,
            Self::TechnicalReturnValue(value) => value.name,
            Self::ProjectRoot(value) => value.name,
            Self::ProjectDocument(value) => value.name,
        }
    }
}

fn opt_ref(f: &mut impl FnMut(&mut ObjectHash), field: &mut ObjectHash) {
    if !field.is_zero() {
        f(field);
    }
}

fn list_ref(f: &mut impl FnMut(&mut ObjectHash), items: &mut Vec<ObjectHash>) {
    for value in items {
        f(value);
    }
}
