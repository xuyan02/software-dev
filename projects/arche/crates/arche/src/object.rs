use anyhow::{Result, bail};

use crate::codec::{Reader, Writer};
use crate::hash::{Hash32, ObjectHash, SymbolHash};

pub const OBJECT_MAGIC: &[u8; 4] = b"DSO2";
const OBJECT_VERSION: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Technical,
    Requirements,
    ConceptDirectory,
    TechnicalSystem,
    TechnicalModule,
    View,
    ObjectList,
    DocumentText,
    DocumentList,
    DocumentParagraphList,
    DocumentTopic,
    DocumentCode,
    DocumentMermaid,
    DocumentHtml,
    Concept,
    Comment,
    RequirementUser,
    RequirementStory,
    RequirementSystem,
    RequirementModule,
    RequirementFunction,
    RequirementInteraction,
    TechnicalDirectory,
    TechnicalFile,
}

impl ObjectKind {
    pub fn tag(self) -> u8 {
        match self {
            Self::Technical => 0,
            Self::Requirements => 1,
            Self::ConceptDirectory => 2,
            Self::TechnicalSystem => 3,
            Self::TechnicalModule => 4,
            Self::View => 5,
            Self::ObjectList => 6,
            Self::DocumentText => 7,
            Self::DocumentList => 8,
            Self::DocumentParagraphList => 9,
            Self::DocumentTopic => 10,
            Self::DocumentCode => 11,
            Self::DocumentMermaid => 12,
            Self::DocumentHtml => 13,
            Self::Concept => 14,
            Self::Comment => 15,
            Self::RequirementUser => 16,
            Self::RequirementStory => 17,
            Self::RequirementSystem => 18,
            Self::RequirementModule => 19,
            Self::RequirementFunction => 20,
            Self::RequirementInteraction => 21,
            Self::TechnicalDirectory => 22,
            Self::TechnicalFile => 23,
        }
    }

    pub fn from_tag(tag: u8) -> Result<Self> {
        Ok(match tag {
            0 => Self::Technical,
            1 => Self::Requirements,
            2 => Self::ConceptDirectory,
            3 => Self::TechnicalSystem,
            4 => Self::TechnicalModule,
            5 => Self::View,
            6 => Self::ObjectList,
            7 => Self::DocumentText,
            8 => Self::DocumentList,
            9 => Self::DocumentParagraphList,
            10 => Self::DocumentTopic,
            11 => Self::DocumentCode,
            12 => Self::DocumentMermaid,
            13 => Self::DocumentHtml,
            14 => Self::Concept,
            15 => Self::Comment,
            16 => Self::RequirementUser,
            17 => Self::RequirementStory,
            18 => Self::RequirementSystem,
            19 => Self::RequirementModule,
            20 => Self::RequirementFunction,
            21 => Self::RequirementInteraction,
            22 => Self::TechnicalDirectory,
            23 => Self::TechnicalFile,
            _ => bail!("unknown object kind tag {tag}"),
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Technical => "Technical",
            Self::Requirements => "Requirements",
            Self::ConceptDirectory => "ConceptDirectory",
            Self::TechnicalSystem => "TechnicalSystem",
            Self::TechnicalModule => "TechnicalModule",
            Self::View => "View",
            Self::ObjectList => "ObjectList",
            Self::DocumentText => "DocumentText",
            Self::DocumentList => "DocumentList",
            Self::DocumentParagraphList => "DocumentParagraphList",
            Self::DocumentTopic => "DocumentTopic",
            Self::DocumentCode => "DocumentCode",
            Self::DocumentMermaid => "DocumentMermaid",
            Self::DocumentHtml => "DocumentHtml",
            Self::Concept => "Concept",
            Self::Comment => "Comment",
            Self::RequirementUser => "RequirementUser",
            Self::RequirementStory => "RequirementStory",
            Self::RequirementSystem => "RequirementSystem",
            Self::RequirementModule => "RequirementModule",
            Self::RequirementFunction => "RequirementFunction",
            Self::RequirementInteraction => "RequirementInteraction",
            Self::TechnicalDirectory => "TechnicalDirectory",
            Self::TechnicalFile => "TechnicalFile",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Code(String),
    Strong(Vec<Inline>),
    Emphasis(Vec<Inline>),
    Link {
        text: Vec<Inline>,
        href: String,
    },
    ConceptRef {
        qname: SymbolHash,
        label: Vec<Inline>,
    },
    Break,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paragraph {
    Text(SymbolHash),
    List(SymbolHash),
    Topic(SymbolHash),
    Code(SymbolHash),
    Mermaid(SymbolHash),
    Html(SymbolHash),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConceptRelation {
    Refines,
    Realizes,
    Corresponds,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConceptLink {
    pub target: SymbolHash,
    pub relation: ConceptRelation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Technical {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub files: Option<SymbolHash>,
    pub namespaces: Option<SymbolHash>,
    pub systems: Option<SymbolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirements {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub users: Option<SymbolHash>,
    pub stories: Option<SymbolHash>,
    pub systems: Option<SymbolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConceptDirectory {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: Option<SymbolHash>,
    pub children: Option<SymbolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechnicalSystem {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub members: Option<SymbolHash>,
    pub modules: Option<SymbolHash>,
    pub layers: Option<SymbolHash>,
    pub position: Option<SymbolHash>,
    pub responsibilities: Option<SymbolHash>,
    pub non_responsibilities: Option<SymbolHash>,
    pub features: Option<SymbolHash>,
    pub non_features: Option<SymbolHash>,
    pub invariants: Option<SymbolHash>,
    pub constraints: Option<SymbolHash>,
    pub design_principles: Option<SymbolHash>,
    pub external_dependencies: Option<SymbolHash>,
    pub usage: Option<SymbolHash>,
    pub design_points: Option<SymbolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechnicalModule {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub members: Option<SymbolHash>,
    pub modules: Option<SymbolHash>,
    pub layers: Option<SymbolHash>,
    pub position: Option<SymbolHash>,
    pub responsibilities: Option<SymbolHash>,
    pub non_responsibilities: Option<SymbolHash>,
    pub features: Option<SymbolHash>,
    pub non_features: Option<SymbolHash>,
    pub invariants: Option<SymbolHash>,
    pub constraints: Option<SymbolHash>,
    pub design_principles: Option<SymbolHash>,
    pub external_dependencies: Option<SymbolHash>,
    pub usage: Option<SymbolHash>,
    pub design_points: Option<SymbolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub qname: SymbolHash,
    pub source_commit: Hash32,
    pub technical: ObjectHash,
    pub requirements: ObjectHash,
    pub concepts: ObjectHash,
    pub comments: Option<ObjectHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectList {
    pub qname: SymbolHash,
    pub owner: SymbolHash,
    pub next_id: u64,
    pub items: Vec<SymbolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentText {
    pub qname: SymbolHash,
    pub content: Vec<Inline>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentList {
    pub qname: SymbolHash,
    pub next_id: u64,
    pub ordered: bool,
    pub items: Vec<Paragraph>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentParagraphList {
    pub qname: SymbolHash,
    pub next_id: u64,
    pub items: Vec<Paragraph>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentTopic {
    pub qname: SymbolHash,
    pub title: Vec<Inline>,
    pub body: SymbolHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentCode {
    pub qname: SymbolHash,
    pub language: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentMermaid {
    pub qname: SymbolHash,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentHtml {
    pub qname: SymbolHash,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Concept {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub summary: Option<SymbolHash>,
    pub explanation: Option<SymbolHash>,
    pub characteristics: Option<SymbolHash>,
    pub relations: Option<SymbolHash>,
    pub aliases: Option<SymbolHash>,
    pub links: Vec<ConceptLink>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub qname: SymbolHash,
    pub target: SymbolHash,
    pub target_hash: Option<Hash32>,
    pub body: Vec<Inline>,
    pub reply_to: Option<SymbolHash>,
    pub resolved: bool,
    pub author: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementUser {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub age: Option<SymbolHash>,
    pub basic_situation: Option<SymbolHash>,
    pub hobbies: Option<SymbolHash>,
    pub occupation: Option<SymbolHash>,
    pub typical_scenario: Option<SymbolHash>,
    pub needs: Option<SymbolHash>,
    pub priority: Option<SymbolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementStory {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub preconditions: Option<SymbolHash>,
    pub frequency: Option<SymbolHash>,
    pub description: Option<SymbolHash>,
    pub related_requirements: Option<SymbolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementSystem {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub modules: Option<SymbolHash>,
    pub form: Option<SymbolHash>,
    pub covered_users: Option<SymbolHash>,
    pub closed_loop_goal: Option<SymbolHash>,
    pub relationships: Option<SymbolHash>,
    pub boundary: Option<SymbolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementModule {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub functions: Option<SymbolHash>,
    pub core_concept: Option<SymbolHash>,
    pub included_concepts: Option<SymbolHash>,
    pub concept_relations: Option<SymbolHash>,
    pub boundary: Option<SymbolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementFunction {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub flow: Option<SymbolHash>,
    pub interactions: Option<SymbolHash>,
    pub initiator: Option<SymbolHash>,
    pub system_change: Option<SymbolHash>,
    pub preconditions: Option<SymbolHash>,
    pub feedback: Option<SymbolHash>,
    pub description: Option<SymbolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementInteraction {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub description: Option<SymbolHash>,
    pub ui: Option<SymbolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechnicalDirectory {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub source_path: Option<SymbolHash>,
    pub children: Option<SymbolHash>,
    pub position: Option<SymbolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechnicalFile {
    pub qname: SymbolHash,
    pub name: SymbolHash,
    pub parent: SymbolHash,
    pub source_path: Option<SymbolHash>,
    pub source_hash: Hash32,
    pub children: Option<SymbolHash>,
    pub position: Option<SymbolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Object {
    Technical(Technical),
    Requirements(Requirements),
    ConceptDirectory(ConceptDirectory),
    TechnicalSystem(TechnicalSystem),
    TechnicalModule(TechnicalModule),
    View(View),
    ObjectList(ObjectList),
    DocumentText(DocumentText),
    DocumentList(DocumentList),
    DocumentParagraphList(DocumentParagraphList),
    DocumentTopic(DocumentTopic),
    DocumentCode(DocumentCode),
    DocumentMermaid(DocumentMermaid),
    DocumentHtml(DocumentHtml),
    Concept(Concept),
    Comment(Comment),
    RequirementUser(RequirementUser),
    RequirementStory(RequirementStory),
    RequirementSystem(RequirementSystem),
    RequirementModule(RequirementModule),
    RequirementFunction(RequirementFunction),
    RequirementInteraction(RequirementInteraction),
    TechnicalDirectory(TechnicalDirectory),
    TechnicalFile(TechnicalFile),
}

impl Object {
    pub fn kind(&self) -> ObjectKind {
        match self {
            Self::Technical(_) => ObjectKind::Technical,
            Self::Requirements(_) => ObjectKind::Requirements,
            Self::ConceptDirectory(_) => ObjectKind::ConceptDirectory,
            Self::TechnicalSystem(_) => ObjectKind::TechnicalSystem,
            Self::TechnicalModule(_) => ObjectKind::TechnicalModule,
            Self::View(_) => ObjectKind::View,
            Self::ObjectList(_) => ObjectKind::ObjectList,
            Self::DocumentText(_) => ObjectKind::DocumentText,
            Self::DocumentList(_) => ObjectKind::DocumentList,
            Self::DocumentParagraphList(_) => ObjectKind::DocumentParagraphList,
            Self::DocumentTopic(_) => ObjectKind::DocumentTopic,
            Self::DocumentCode(_) => ObjectKind::DocumentCode,
            Self::DocumentMermaid(_) => ObjectKind::DocumentMermaid,
            Self::DocumentHtml(_) => ObjectKind::DocumentHtml,
            Self::Concept(_) => ObjectKind::Concept,
            Self::Comment(_) => ObjectKind::Comment,
            Self::RequirementUser(_) => ObjectKind::RequirementUser,
            Self::RequirementStory(_) => ObjectKind::RequirementStory,
            Self::RequirementSystem(_) => ObjectKind::RequirementSystem,
            Self::RequirementModule(_) => ObjectKind::RequirementModule,
            Self::RequirementFunction(_) => ObjectKind::RequirementFunction,
            Self::RequirementInteraction(_) => ObjectKind::RequirementInteraction,
            Self::TechnicalDirectory(_) => ObjectKind::TechnicalDirectory,
            Self::TechnicalFile(_) => ObjectKind::TechnicalFile,
        }
    }

    pub fn qname(&self) -> SymbolHash {
        match self {
            Self::Technical(value) => value.qname,
            Self::Requirements(value) => value.qname,
            Self::ConceptDirectory(value) => value.qname,
            Self::TechnicalSystem(value) => value.qname,
            Self::TechnicalModule(value) => value.qname,
            Self::View(value) => value.qname,
            Self::ObjectList(value) => value.qname,
            Self::DocumentText(value) => value.qname,
            Self::DocumentList(value) => value.qname,
            Self::DocumentParagraphList(value) => value.qname,
            Self::DocumentTopic(value) => value.qname,
            Self::DocumentCode(value) => value.qname,
            Self::DocumentMermaid(value) => value.qname,
            Self::DocumentHtml(value) => value.qname,
            Self::Concept(value) => value.qname,
            Self::Comment(value) => value.qname,
            Self::RequirementUser(value) => value.qname,
            Self::RequirementStory(value) => value.qname,
            Self::RequirementSystem(value) => value.qname,
            Self::RequirementModule(value) => value.qname,
            Self::RequirementFunction(value) => value.qname,
            Self::RequirementInteraction(value) => value.qname,
            Self::TechnicalDirectory(value) => value.qname,
            Self::TechnicalFile(value) => value.qname,
        }
    }

    pub fn name(&self) -> Option<SymbolHash> {
        match self {
            Self::Technical(value) => Some(value.name),
            Self::Requirements(value) => Some(value.name),
            Self::ConceptDirectory(value) => Some(value.name),
            Self::TechnicalSystem(value) => Some(value.name),
            Self::TechnicalModule(value) => Some(value.name),
            Self::Concept(value) => Some(value.name),
            Self::RequirementUser(value) => Some(value.name),
            Self::RequirementStory(value) => Some(value.name),
            Self::RequirementSystem(value) => Some(value.name),
            Self::RequirementModule(value) => Some(value.name),
            Self::RequirementFunction(value) => Some(value.name),
            Self::RequirementInteraction(value) => Some(value.name),
            Self::TechnicalDirectory(value) => Some(value.name),
            Self::TechnicalFile(value) => Some(value.name),
            _ => None,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.bytes(OBJECT_MAGIC);
        writer.u8(OBJECT_VERSION);
        writer.u8(self.kind().tag());
        match self {
            Self::Technical(value) => encode_technical(&mut writer, value),
            Self::Requirements(value) => encode_requirements(&mut writer, value),
            Self::ConceptDirectory(value) => encode_concept_directory(&mut writer, value),
            Self::TechnicalSystem(value) => encode_technical_system(&mut writer, value),
            Self::TechnicalModule(value) => encode_technical_module(&mut writer, value),
            Self::View(value) => encode_view(&mut writer, value),
            Self::ObjectList(value) => encode_object_list(&mut writer, value),
            Self::DocumentText(value) => encode_document_text(&mut writer, value),
            Self::DocumentList(value) => encode_document_list(&mut writer, value),
            Self::DocumentParagraphList(value) => {
                encode_document_paragraph_list(&mut writer, value)
            }
            Self::DocumentTopic(value) => encode_document_topic(&mut writer, value),
            Self::DocumentCode(value) => encode_document_code(&mut writer, value),
            Self::DocumentMermaid(value) => encode_document_mermaid(&mut writer, value),
            Self::DocumentHtml(value) => encode_document_html(&mut writer, value),
            Self::Concept(value) => encode_concept(&mut writer, value),
            Self::Comment(value) => encode_comment(&mut writer, value),
            Self::RequirementUser(value) => encode_requirement_user(&mut writer, value),
            Self::RequirementStory(value) => encode_requirement_story(&mut writer, value),
            Self::RequirementSystem(value) => encode_requirement_system(&mut writer, value),
            Self::RequirementModule(value) => encode_requirement_module(&mut writer, value),
            Self::RequirementFunction(value) => encode_requirement_function(&mut writer, value),
            Self::RequirementInteraction(value) => {
                encode_requirement_interaction(&mut writer, value)
            }
            Self::TechnicalDirectory(value) => encode_technical_directory(&mut writer, value),
            Self::TechnicalFile(value) => encode_technical_file(&mut writer, value),
        }
        writer.finish()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let magic = reader.bytes(4)?;
        if magic != OBJECT_MAGIC {
            bail!("invalid object magic");
        }
        let version = reader.u8()?;
        if version != OBJECT_VERSION {
            bail!("unsupported object version {version}");
        }
        let kind = ObjectKind::from_tag(reader.u8()?)?;
        let object = match kind {
            ObjectKind::Technical => Self::Technical(decode_technical(&mut reader)?),
            ObjectKind::Requirements => Self::Requirements(decode_requirements(&mut reader)?),
            ObjectKind::ConceptDirectory => {
                Self::ConceptDirectory(decode_concept_directory(&mut reader)?)
            }
            ObjectKind::TechnicalSystem => {
                Self::TechnicalSystem(decode_technical_system(&mut reader)?)
            }
            ObjectKind::TechnicalModule => {
                Self::TechnicalModule(decode_technical_module(&mut reader)?)
            }
            ObjectKind::View => Self::View(decode_view(&mut reader)?),
            ObjectKind::ObjectList => Self::ObjectList(decode_object_list(&mut reader)?),
            ObjectKind::DocumentText => Self::DocumentText(decode_document_text(&mut reader)?),
            ObjectKind::DocumentList => Self::DocumentList(decode_document_list(&mut reader)?),
            ObjectKind::DocumentParagraphList => {
                Self::DocumentParagraphList(decode_document_paragraph_list(&mut reader)?)
            }
            ObjectKind::DocumentTopic => Self::DocumentTopic(decode_document_topic(&mut reader)?),
            ObjectKind::DocumentCode => Self::DocumentCode(decode_document_code(&mut reader)?),
            ObjectKind::DocumentMermaid => {
                Self::DocumentMermaid(decode_document_mermaid(&mut reader)?)
            }
            ObjectKind::DocumentHtml => Self::DocumentHtml(decode_document_html(&mut reader)?),
            ObjectKind::Concept => Self::Concept(decode_concept(&mut reader)?),
            ObjectKind::Comment => Self::Comment(decode_comment(&mut reader)?),
            ObjectKind::RequirementUser => {
                Self::RequirementUser(decode_requirement_user(&mut reader)?)
            }
            ObjectKind::RequirementStory => {
                Self::RequirementStory(decode_requirement_story(&mut reader)?)
            }
            ObjectKind::RequirementSystem => {
                Self::RequirementSystem(decode_requirement_system(&mut reader)?)
            }
            ObjectKind::RequirementModule => {
                Self::RequirementModule(decode_requirement_module(&mut reader)?)
            }
            ObjectKind::RequirementFunction => {
                Self::RequirementFunction(decode_requirement_function(&mut reader)?)
            }
            ObjectKind::RequirementInteraction => {
                Self::RequirementInteraction(decode_requirement_interaction(&mut reader)?)
            }
            ObjectKind::TechnicalDirectory => {
                Self::TechnicalDirectory(decode_technical_directory(&mut reader)?)
            }
            ObjectKind::TechnicalFile => Self::TechnicalFile(decode_technical_file(&mut reader)?),
        };
        reader.finish()?;
        Ok(object)
    }
}

impl Writer {
    pub fn bytes(&mut self, bytes: &[u8]) {
        self.raw(bytes);
    }
}

impl<'a> Reader<'a> {
    pub fn bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        self.take(len)
    }
}

fn encode_technical(writer: &mut Writer, value: &Technical) {
    writer.symbol(value.qname);
    writer.symbol(value.name);
    writer.opt_symbol(value.files);
    writer.opt_symbol(value.namespaces);
    writer.opt_symbol(value.systems);
}

fn decode_technical(reader: &mut Reader<'_>) -> Result<Technical> {
    Ok(Technical {
        qname: reader.symbol()?,
        name: reader.symbol()?,
        files: reader.opt_symbol()?,
        namespaces: reader.opt_symbol()?,
        systems: reader.opt_symbol()?,
    })
}

fn encode_requirements(writer: &mut Writer, value: &Requirements) {
    writer.symbol(value.qname);
    writer.symbol(value.name);
    writer.opt_symbol(value.users);
    writer.opt_symbol(value.stories);
    writer.opt_symbol(value.systems);
}

fn decode_requirements(reader: &mut Reader<'_>) -> Result<Requirements> {
    Ok(Requirements {
        qname: reader.symbol()?,
        name: reader.symbol()?,
        users: reader.opt_symbol()?,
        stories: reader.opt_symbol()?,
        systems: reader.opt_symbol()?,
    })
}

fn encode_concept_directory(writer: &mut Writer, value: &ConceptDirectory) {
    writer.symbol(value.qname);
    writer.symbol(value.name);
    writer.opt_symbol(value.parent);
    writer.opt_symbol(value.children);
}

fn decode_concept_directory(reader: &mut Reader<'_>) -> Result<ConceptDirectory> {
    Ok(ConceptDirectory {
        qname: reader.symbol()?,
        name: reader.symbol()?,
        parent: reader.opt_symbol()?,
        children: reader.opt_symbol()?,
    })
}

fn encode_technical_system(writer: &mut Writer, value: &TechnicalSystem) {
    writer.symbol(value.qname);
    writer.symbol(value.name);
    writer.symbol(value.parent);
    writer.opt_symbol(value.members);
    writer.opt_symbol(value.modules);
    writer.opt_symbol(value.layers);
    writer.opt_symbol(value.position);
    writer.opt_symbol(value.responsibilities);
    writer.opt_symbol(value.non_responsibilities);
    writer.opt_symbol(value.features);
    writer.opt_symbol(value.non_features);
    writer.opt_symbol(value.invariants);
    writer.opt_symbol(value.constraints);
    writer.opt_symbol(value.design_principles);
    writer.opt_symbol(value.external_dependencies);
    writer.opt_symbol(value.usage);
    writer.opt_symbol(value.design_points);
}

fn decode_technical_system(reader: &mut Reader<'_>) -> Result<TechnicalSystem> {
    Ok(TechnicalSystem {
        qname: reader.symbol()?,
        name: reader.symbol()?,
        parent: reader.symbol()?,
        members: reader.opt_symbol()?,
        modules: reader.opt_symbol()?,
        layers: reader.opt_symbol()?,
        position: reader.opt_symbol()?,
        responsibilities: reader.opt_symbol()?,
        non_responsibilities: reader.opt_symbol()?,
        features: reader.opt_symbol()?,
        non_features: reader.opt_symbol()?,
        invariants: reader.opt_symbol()?,
        constraints: reader.opt_symbol()?,
        design_principles: reader.opt_symbol()?,
        external_dependencies: reader.opt_symbol()?,
        usage: reader.opt_symbol()?,
        design_points: reader.opt_symbol()?,
    })
}

fn encode_technical_module(writer: &mut Writer, value: &TechnicalModule) {
    writer.symbol(value.qname);
    writer.symbol(value.name);
    writer.symbol(value.parent);
    writer.opt_symbol(value.members);
    writer.opt_symbol(value.modules);
    writer.opt_symbol(value.layers);
    writer.opt_symbol(value.position);
    writer.opt_symbol(value.responsibilities);
    writer.opt_symbol(value.non_responsibilities);
    writer.opt_symbol(value.features);
    writer.opt_symbol(value.non_features);
    writer.opt_symbol(value.invariants);
    writer.opt_symbol(value.constraints);
    writer.opt_symbol(value.design_principles);
    writer.opt_symbol(value.external_dependencies);
    writer.opt_symbol(value.usage);
    writer.opt_symbol(value.design_points);
}

fn decode_technical_module(reader: &mut Reader<'_>) -> Result<TechnicalModule> {
    Ok(TechnicalModule {
        qname: reader.symbol()?,
        name: reader.symbol()?,
        parent: reader.symbol()?,
        members: reader.opt_symbol()?,
        modules: reader.opt_symbol()?,
        layers: reader.opt_symbol()?,
        position: reader.opt_symbol()?,
        responsibilities: reader.opt_symbol()?,
        non_responsibilities: reader.opt_symbol()?,
        features: reader.opt_symbol()?,
        non_features: reader.opt_symbol()?,
        invariants: reader.opt_symbol()?,
        constraints: reader.opt_symbol()?,
        design_principles: reader.opt_symbol()?,
        external_dependencies: reader.opt_symbol()?,
        usage: reader.opt_symbol()?,
        design_points: reader.opt_symbol()?,
    })
}

fn encode_view(writer: &mut Writer, value: &View) {
    writer.symbol(value.qname);
    writer.hash(value.source_commit);
    writer.hash(value.technical.0);
    writer.hash(value.requirements.0);
    writer.hash(value.concepts.0);
    match value.comments {
        Some(comments) => writer.hash(comments.0),
        None => writer.hash(Hash32::ZERO),
    }
}

fn decode_view(reader: &mut Reader<'_>) -> Result<View> {
    let qname = reader.symbol()?;
    let source_commit = reader.hash()?;
    let technical = ObjectHash(reader.hash()?);
    let requirements = ObjectHash(reader.hash()?);
    let concepts = ObjectHash(reader.hash()?);
    let comments_hash = reader.hash()?;
    let comments = (!comments_hash.is_zero()).then_some(ObjectHash(comments_hash));
    Ok(View {
        qname,
        source_commit,
        technical,
        requirements,
        concepts,
        comments,
    })
}

fn encode_object_list(writer: &mut Writer, value: &ObjectList) {
    writer.symbol(value.qname);
    writer.symbol(value.owner);
    writer.u64(value.next_id);
    writer.symbol_list(&value.items);
}

fn decode_object_list(reader: &mut Reader<'_>) -> Result<ObjectList> {
    Ok(ObjectList {
        qname: reader.symbol()?,
        owner: reader.symbol()?,
        next_id: reader.u64()?,
        items: reader.symbol_list()?,
    })
}

fn encode_inlines(writer: &mut Writer, inlines: &[Inline]) {
    writer.u32(inlines.len() as u32);
    for inline in inlines {
        encode_inline(writer, inline);
    }
}

fn decode_inlines(reader: &mut Reader<'_>) -> Result<Vec<Inline>> {
    let count = reader.u32()? as usize;
    let mut inlines = Vec::with_capacity(count);
    for _ in 0..count {
        inlines.push(decode_inline(reader)?);
    }
    Ok(inlines)
}

fn encode_inline(writer: &mut Writer, inline: &Inline) {
    match inline {
        Inline::Text(text) => {
            writer.u8(0);
            writer.text(text);
        }
        Inline::Code(text) => {
            writer.u8(1);
            writer.text(text);
        }
        Inline::Strong(children) => {
            writer.u8(2);
            encode_inlines(writer, children);
        }
        Inline::Emphasis(children) => {
            writer.u8(3);
            encode_inlines(writer, children);
        }
        Inline::Link { text, href } => {
            writer.u8(4);
            encode_inlines(writer, text);
            writer.text(href);
        }
        Inline::ConceptRef { qname, label } => {
            writer.u8(5);
            writer.symbol(*qname);
            encode_inlines(writer, label);
        }
        Inline::Break => writer.u8(6),
    }
}

fn decode_inline(reader: &mut Reader<'_>) -> Result<Inline> {
    Ok(match reader.u8()? {
        0 => Inline::Text(reader.text()?),
        1 => Inline::Code(reader.text()?),
        2 => Inline::Strong(decode_inlines(reader)?),
        3 => Inline::Emphasis(decode_inlines(reader)?),
        4 => Inline::Link {
            text: decode_inlines(reader)?,
            href: reader.text()?,
        },
        5 => Inline::ConceptRef {
            qname: reader.symbol()?,
            label: decode_inlines(reader)?,
        },
        6 => Inline::Break,
        tag => bail!("unknown inline tag {tag}"),
    })
}

fn encode_paragraph(writer: &mut Writer, paragraph: Paragraph) {
    match paragraph {
        Paragraph::Text(qname) => {
            writer.u8(0);
            writer.symbol(qname);
        }
        Paragraph::List(qname) => {
            writer.u8(1);
            writer.symbol(qname);
        }
        Paragraph::Topic(qname) => {
            writer.u8(2);
            writer.symbol(qname);
        }
        Paragraph::Code(qname) => {
            writer.u8(3);
            writer.symbol(qname);
        }
        Paragraph::Mermaid(qname) => {
            writer.u8(4);
            writer.symbol(qname);
        }
        Paragraph::Html(qname) => {
            writer.u8(5);
            writer.symbol(qname);
        }
    }
}

fn decode_paragraph(reader: &mut Reader<'_>) -> Result<Paragraph> {
    Ok(match reader.u8()? {
        0 => Paragraph::Text(reader.symbol()?),
        1 => Paragraph::List(reader.symbol()?),
        2 => Paragraph::Topic(reader.symbol()?),
        3 => Paragraph::Code(reader.symbol()?),
        4 => Paragraph::Mermaid(reader.symbol()?),
        5 => Paragraph::Html(reader.symbol()?),
        tag => bail!("unknown paragraph tag {tag}"),
    })
}

fn encode_paragraphs(writer: &mut Writer, paragraphs: &[Paragraph]) {
    writer.u32(paragraphs.len() as u32);
    for paragraph in paragraphs {
        encode_paragraph(writer, *paragraph);
    }
}

fn decode_paragraphs(reader: &mut Reader<'_>) -> Result<Vec<Paragraph>> {
    let count = reader.u32()? as usize;
    let mut paragraphs = Vec::with_capacity(count);
    for _ in 0..count {
        paragraphs.push(decode_paragraph(reader)?);
    }
    Ok(paragraphs)
}

fn encode_document_text(writer: &mut Writer, value: &DocumentText) {
    writer.symbol(value.qname);
    encode_inlines(writer, &value.content);
}

fn decode_document_text(reader: &mut Reader<'_>) -> Result<DocumentText> {
    Ok(DocumentText {
        qname: reader.symbol()?,
        content: decode_inlines(reader)?,
    })
}

fn encode_document_list(writer: &mut Writer, value: &DocumentList) {
    writer.symbol(value.qname);
    writer.u64(value.next_id);
    writer.u8(u8::from(value.ordered));
    encode_paragraphs(writer, &value.items);
}

fn decode_document_list(reader: &mut Reader<'_>) -> Result<DocumentList> {
    Ok(DocumentList {
        qname: reader.symbol()?,
        next_id: reader.u64()?,
        ordered: reader.u8()? != 0,
        items: decode_paragraphs(reader)?,
    })
}

fn encode_document_paragraph_list(writer: &mut Writer, value: &DocumentParagraphList) {
    writer.symbol(value.qname);
    writer.u64(value.next_id);
    encode_paragraphs(writer, &value.items);
}

fn decode_document_paragraph_list(reader: &mut Reader<'_>) -> Result<DocumentParagraphList> {
    Ok(DocumentParagraphList {
        qname: reader.symbol()?,
        next_id: reader.u64()?,
        items: decode_paragraphs(reader)?,
    })
}

fn encode_document_topic(writer: &mut Writer, value: &DocumentTopic) {
    writer.symbol(value.qname);
    encode_inlines(writer, &value.title);
    writer.symbol(value.body);
}

fn decode_document_topic(reader: &mut Reader<'_>) -> Result<DocumentTopic> {
    Ok(DocumentTopic {
        qname: reader.symbol()?,
        title: decode_inlines(reader)?,
        body: reader.symbol()?,
    })
}

fn encode_document_code(writer: &mut Writer, value: &DocumentCode) {
    writer.symbol(value.qname);
    writer.text(&value.language);
    writer.text(&value.source);
}

fn decode_document_code(reader: &mut Reader<'_>) -> Result<DocumentCode> {
    Ok(DocumentCode {
        qname: reader.symbol()?,
        language: reader.text()?,
        source: reader.text()?,
    })
}

fn encode_document_mermaid(writer: &mut Writer, value: &DocumentMermaid) {
    writer.symbol(value.qname);
    writer.text(&value.source);
}

fn decode_document_mermaid(reader: &mut Reader<'_>) -> Result<DocumentMermaid> {
    Ok(DocumentMermaid {
        qname: reader.symbol()?,
        source: reader.text()?,
    })
}

fn encode_document_html(writer: &mut Writer, value: &DocumentHtml) {
    writer.symbol(value.qname);
    writer.text(&value.source);
}

fn decode_document_html(reader: &mut Reader<'_>) -> Result<DocumentHtml> {
    Ok(DocumentHtml {
        qname: reader.symbol()?,
        source: reader.text()?,
    })
}

fn encode_concept_link(writer: &mut Writer, value: &ConceptLink) {
    writer.symbol(value.target);
    writer.u8(match value.relation {
        ConceptRelation::Refines => 0,
        ConceptRelation::Realizes => 1,
        ConceptRelation::Corresponds => 2,
    });
}

fn decode_concept_link(reader: &mut Reader<'_>) -> Result<ConceptLink> {
    let target = reader.symbol()?;
    let relation = match reader.u8()? {
        0 => ConceptRelation::Refines,
        1 => ConceptRelation::Realizes,
        2 => ConceptRelation::Corresponds,
        tag => bail!("unknown concept relation tag {tag}"),
    };
    Ok(ConceptLink { target, relation })
}

fn encode_concept(writer: &mut Writer, value: &Concept) {
    writer.symbol(value.qname);
    writer.symbol(value.name);
    writer.opt_symbol(value.summary);
    writer.opt_symbol(value.explanation);
    writer.opt_symbol(value.characteristics);
    writer.opt_symbol(value.relations);
    writer.opt_symbol(value.aliases);
    writer.u32(value.links.len() as u32);
    for link in &value.links {
        encode_concept_link(writer, link);
    }
}

fn decode_concept(reader: &mut Reader<'_>) -> Result<Concept> {
    let qname = reader.symbol()?;
    let name = reader.symbol()?;
    let summary = reader.opt_symbol()?;
    let explanation = reader.opt_symbol()?;
    let characteristics = reader.opt_symbol()?;
    let relations = reader.opt_symbol()?;
    let aliases = reader.opt_symbol()?;
    let count = reader.u32()? as usize;
    let mut links = Vec::with_capacity(count);
    for _ in 0..count {
        links.push(decode_concept_link(reader)?);
    }
    Ok(Concept {
        qname,
        name,
        summary,
        explanation,
        characteristics,
        relations,
        aliases,
        links,
    })
}

fn encode_comment(writer: &mut Writer, value: &Comment) {
    writer.symbol(value.qname);
    writer.symbol(value.target);
    match value.target_hash {
        Some(hash) => writer.hash(hash),
        None => writer.hash(Hash32::ZERO),
    }
    encode_inlines(writer, &value.body);
    writer.opt_symbol(value.reply_to);
    writer.u8(u8::from(value.resolved));
    writer.text(&value.author);
    writer.u64(value.created_at as u64);
    writer.u64(value.updated_at as u64);
}

fn decode_comment(reader: &mut Reader<'_>) -> Result<Comment> {
    let qname = reader.symbol()?;
    let target = reader.symbol()?;
    let target_hash = {
        let hash = reader.hash()?;
        (!hash.is_zero()).then_some(hash)
    };
    let body = decode_inlines(reader)?;
    let reply_to = reader.opt_symbol()?;
    let resolved = reader.u8()? != 0;
    let author = reader.text()?;
    let created_at = reader.u64()? as i64;
    let updated_at = reader.u64()? as i64;
    Ok(Comment {
        qname,
        target,
        target_hash,
        body,
        reply_to,
        resolved,
        author,
        created_at,
        updated_at,
    })
}

fn encode_requirement_user(writer: &mut Writer, value: &RequirementUser) {
    writer.symbol(value.qname);
    writer.symbol(value.name);
    writer.opt_symbol(value.age);
    writer.opt_symbol(value.basic_situation);
    writer.opt_symbol(value.hobbies);
    writer.opt_symbol(value.occupation);
    writer.opt_symbol(value.typical_scenario);
    writer.opt_symbol(value.needs);
    writer.opt_symbol(value.priority);
}

fn decode_requirement_user(reader: &mut Reader<'_>) -> Result<RequirementUser> {
    Ok(RequirementUser {
        qname: reader.symbol()?,
        name: reader.symbol()?,
        age: reader.opt_symbol()?,
        basic_situation: reader.opt_symbol()?,
        hobbies: reader.opt_symbol()?,
        occupation: reader.opt_symbol()?,
        typical_scenario: reader.opt_symbol()?,
        needs: reader.opt_symbol()?,
        priority: reader.opt_symbol()?,
    })
}

fn encode_requirement_story(writer: &mut Writer, value: &RequirementStory) {
    writer.symbol(value.qname);
    writer.symbol(value.name);
    writer.opt_symbol(value.preconditions);
    writer.opt_symbol(value.frequency);
    writer.opt_symbol(value.description);
    writer.opt_symbol(value.related_requirements);
}

fn decode_requirement_story(reader: &mut Reader<'_>) -> Result<RequirementStory> {
    Ok(RequirementStory {
        qname: reader.symbol()?,
        name: reader.symbol()?,
        preconditions: reader.opt_symbol()?,
        frequency: reader.opt_symbol()?,
        description: reader.opt_symbol()?,
        related_requirements: reader.opt_symbol()?,
    })
}

fn encode_requirement_system(writer: &mut Writer, value: &RequirementSystem) {
    writer.symbol(value.qname);
    writer.symbol(value.name);
    writer.symbol(value.parent);
    writer.opt_symbol(value.modules);
    writer.opt_symbol(value.form);
    writer.opt_symbol(value.covered_users);
    writer.opt_symbol(value.closed_loop_goal);
    writer.opt_symbol(value.relationships);
    writer.opt_symbol(value.boundary);
}

fn decode_requirement_system(reader: &mut Reader<'_>) -> Result<RequirementSystem> {
    Ok(RequirementSystem {
        qname: reader.symbol()?,
        name: reader.symbol()?,
        parent: reader.symbol()?,
        modules: reader.opt_symbol()?,
        form: reader.opt_symbol()?,
        covered_users: reader.opt_symbol()?,
        closed_loop_goal: reader.opt_symbol()?,
        relationships: reader.opt_symbol()?,
        boundary: reader.opt_symbol()?,
    })
}

fn encode_requirement_module(writer: &mut Writer, value: &RequirementModule) {
    writer.symbol(value.qname);
    writer.symbol(value.name);
    writer.symbol(value.parent);
    writer.opt_symbol(value.functions);
    writer.opt_symbol(value.core_concept);
    writer.opt_symbol(value.included_concepts);
    writer.opt_symbol(value.concept_relations);
    writer.opt_symbol(value.boundary);
}

fn decode_requirement_module(reader: &mut Reader<'_>) -> Result<RequirementModule> {
    Ok(RequirementModule {
        qname: reader.symbol()?,
        name: reader.symbol()?,
        parent: reader.symbol()?,
        functions: reader.opt_symbol()?,
        core_concept: reader.opt_symbol()?,
        included_concepts: reader.opt_symbol()?,
        concept_relations: reader.opt_symbol()?,
        boundary: reader.opt_symbol()?,
    })
}

fn encode_requirement_function(writer: &mut Writer, value: &RequirementFunction) {
    writer.symbol(value.qname);
    writer.symbol(value.name);
    writer.symbol(value.parent);
    writer.opt_symbol(value.flow);
    writer.opt_symbol(value.interactions);
    writer.opt_symbol(value.initiator);
    writer.opt_symbol(value.system_change);
    writer.opt_symbol(value.preconditions);
    writer.opt_symbol(value.feedback);
    writer.opt_symbol(value.description);
}

fn decode_requirement_function(reader: &mut Reader<'_>) -> Result<RequirementFunction> {
    Ok(RequirementFunction {
        qname: reader.symbol()?,
        name: reader.symbol()?,
        parent: reader.symbol()?,
        flow: reader.opt_symbol()?,
        interactions: reader.opt_symbol()?,
        initiator: reader.opt_symbol()?,
        system_change: reader.opt_symbol()?,
        preconditions: reader.opt_symbol()?,
        feedback: reader.opt_symbol()?,
        description: reader.opt_symbol()?,
    })
}

fn encode_requirement_interaction(writer: &mut Writer, value: &RequirementInteraction) {
    writer.symbol(value.qname);
    writer.symbol(value.name);
    writer.symbol(value.parent);
    writer.opt_symbol(value.description);
    writer.opt_symbol(value.ui);
}

fn decode_requirement_interaction(reader: &mut Reader<'_>) -> Result<RequirementInteraction> {
    Ok(RequirementInteraction {
        qname: reader.symbol()?,
        name: reader.symbol()?,
        parent: reader.symbol()?,
        description: reader.opt_symbol()?,
        ui: reader.opt_symbol()?,
    })
}

fn encode_technical_directory(writer: &mut Writer, value: &TechnicalDirectory) {
    writer.symbol(value.qname);
    writer.symbol(value.name);
    writer.symbol(value.parent);
    writer.opt_symbol(value.source_path);
    writer.opt_symbol(value.children);
    writer.opt_symbol(value.position);
}

fn decode_technical_directory(reader: &mut Reader<'_>) -> Result<TechnicalDirectory> {
    Ok(TechnicalDirectory {
        qname: reader.symbol()?,
        name: reader.symbol()?,
        parent: reader.symbol()?,
        source_path: reader.opt_symbol()?,
        children: reader.opt_symbol()?,
        position: reader.opt_symbol()?,
    })
}

fn encode_technical_file(writer: &mut Writer, value: &TechnicalFile) {
    writer.symbol(value.qname);
    writer.symbol(value.name);
    writer.symbol(value.parent);
    writer.opt_symbol(value.source_path);
    writer.hash(value.source_hash);
    writer.opt_symbol(value.children);
    writer.opt_symbol(value.position);
}

fn decode_technical_file(reader: &mut Reader<'_>) -> Result<TechnicalFile> {
    Ok(TechnicalFile {
        qname: reader.symbol()?,
        name: reader.symbol()?,
        parent: reader.symbol()?,
        source_path: reader.opt_symbol()?,
        source_hash: reader.hash()?,
        children: reader.opt_symbol()?,
        position: reader.opt_symbol()?,
    })
}

impl Comment {
    pub fn body_text(&self) -> String {
        inline_plain_text(&self.body)
    }
}

pub fn inline_plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(text) | Inline::Code(text) => out.push_str(text),
            Inline::Strong(children) | Inline::Emphasis(children) => {
                out.push_str(&inline_plain_text(children));
            }
            Inline::Link { text, .. } => out.push_str(&inline_plain_text(text)),
            Inline::ConceptRef { label, .. } => out.push_str(&inline_plain_text(label)),
            Inline::Break => out.push('\n'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(text: &str) -> SymbolHash {
        SymbolHash::of(text)
    }

    #[test]
    fn object_round_trips() {
        let system = Object::TechnicalSystem(TechnicalSystem {
            qname: symbol("technical/app"),
            name: symbol("app"),
            parent: symbol("technical"),
            members: None,
            modules: None,
            layers: None,
            position: Some(symbol("technical/app/position")),
            responsibilities: None,
            non_responsibilities: None,
            features: None,
            non_features: None,
            invariants: None,
            constraints: None,
            design_principles: None,
            external_dependencies: None,
            usage: None,
            design_points: None,
        });
        let bytes = system.encode();
        assert_eq!(Object::decode(&bytes).unwrap(), system);
    }

    #[test]
    fn inline_round_trips() {
        let text = Object::DocumentText(DocumentText {
            qname: symbol("technical/app/position"),
            content: vec![
                Inline::Strong(vec![Inline::Text("定位".into())]),
                Inline::Break,
                Inline::ConceptRef {
                    qname: symbol("concepts/requirements/agent"),
                    label: vec![Inline::Text("智能体".into())],
                },
            ],
        });
        let bytes = text.encode();
        assert_eq!(Object::decode(&bytes).unwrap(), text);
    }
}
