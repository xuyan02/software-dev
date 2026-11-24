use anyhow::{Result, bail};
use sha2::{Digest, Sha256};

use crate::data::{InputObject, OutputObject};
use crate::hash::{Hash32, ObjectHash, SymbolHash};
use crate::object::*;
use crate::source::{SourcePath, SourceSpan};
use crate::store::Store;

/// A concrete design object that knows its own record format.
pub trait ObjectCodec: Sized {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()>;

    /// Write the object and return its object hash.
    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32>;
}

struct ObjectHasher {
    hasher: Sha256,
}

impl ObjectHasher {
    fn new(kind: ObjectKind, version: u8) -> Self {
        let mut hasher = Sha256::new();
        hasher.update([kind.tag(), version]);
        Self { hasher }
    }

    fn u8(&mut self, value: u8) {
        self.hasher.update([value]);
    }

    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    fn u32(&mut self, value: u32) {
        self.hasher.update(value.to_le_bytes());
    }

    fn hash(&mut self, value: impl Into<Hash32>) {
        self.hasher.update(value.into().as_bytes());
    }

    fn symbol(&mut self, value: SymbolHash) {
        self.hash(value.0);
    }

    fn text(&mut self, value: &str) {
        self.u32(value.len() as u32);
        self.hasher.update(value.as_bytes());
    }

    fn symbols(&mut self, values: &[SymbolHash]) {
        self.u32(values.len() as u32);
        for value in values {
            self.symbol(*value);
        }
    }

    fn objects(&mut self, values: &[ObjectHash]) {
        self.u32(values.len() as u32);
        for value in values {
            self.hash(value.0);
        }
    }

    fn source_path(&mut self, value: SourcePath) {
        self.symbol(value.path);
    }

    fn source_span(&mut self, value: SourceSpan) {
        self.symbol(value.path);
        self.u32(value.start);
        self.u32(value.end);
    }

    fn finish(self) -> Hash32 {
        Hash32::from_bytes(self.hasher.finalize().into())
    }
}

impl Object {
    pub fn load(store: &dyn Store, hash: ObjectHash) -> Result<Self> {
        let mut input = store.open_object(hash)?;
        let mut object = Self::alloc(input.kind());
        object.load_into(input.as_mut())?;
        input.finish()?;
        Ok(object)
    }

    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match self {
            Self::TechnicalRoot(value) => value.load_into(input),
            Self::RequirementRoot(value) => value.load_into(input),
            Self::ConceptDirectory(value) => value.load_into(input),
            Self::TechnicalSystem(value) => value.load_into(input),
            Self::TechnicalModule(value) => value.load_into(input),
            Self::Root(value) => value.load_into(input),
            Self::ObjectList(value) => value.load_into(input),
            Self::DocumentText(value) => value.load_into(input),
            Self::DocumentList(value) => value.load_into(input),
            Self::DocumentParagraphList(value) => value.load_into(input),
            Self::DocumentTopic(value) => value.load_into(input),
            Self::DocumentCode(value) => value.load_into(input),
            Self::DocumentMermaid(value) => value.load_into(input),
            Self::DocumentHtml(value) => value.load_into(input),
            Self::Concept(value) => value.load_into(input),
            Self::RequirementUser(value) => value.load_into(input),
            Self::RequirementStory(value) => value.load_into(input),
            Self::RequirementSystem(value) => value.load_into(input),
            Self::RequirementModule(value) => value.load_into(input),
            Self::RequirementFunction(value) => value.load_into(input),
            Self::RequirementInteraction(value) => value.load_into(input),
            Self::TechnicalDirectory(value) => value.load_into(input),
            Self::TechnicalFile(value) => value.load_into(input),
            Self::TechnicalLayer(value) => value.load_into(input),
            Self::TechnicalNamespace(value) => value.load_into(input),
            Self::TechnicalClass(value) => value.load_into(input),
            Self::TechnicalStruct(value) => value.load_into(input),
            Self::TechnicalEnum(value) => value.load_into(input),
            Self::TechnicalInterface(value) => value.load_into(input),
            Self::TechnicalMethod(value) => value.load_into(input),
            Self::TechnicalFunction(value) => value.load_into(input),
            Self::TechnicalMixin(value) => value.load_into(input),
            Self::TechnicalPart(value) => value.load_into(input),
            Self::TechnicalVariable(value) => value.load_into(input),
            Self::TechnicalConstant(value) => value.load_into(input),
            Self::TechnicalMember(value) => value.load_into(input),
            Self::TechnicalProperty(value) => value.load_into(input),
            Self::TechnicalEnumValue(value) => value.load_into(input),
            Self::TechnicalArgument(value) => value.load_into(input),
            Self::TechnicalReturnValue(value) => value.load_into(input),
        }
    }

    pub fn save(&mut self, store: &dyn Store) -> Result<ObjectHash> {
        let mut output = store.create_object(OBJECT_VERSION, self.kind())?;
        let object_hash = match self {
            Self::TechnicalRoot(value) => value.save(output.as_mut())?,
            Self::RequirementRoot(value) => value.save(output.as_mut())?,
            Self::ConceptDirectory(value) => value.save(output.as_mut())?,
            Self::TechnicalSystem(value) => value.save(output.as_mut())?,
            Self::TechnicalModule(value) => value.save(output.as_mut())?,
            Self::Root(value) => value.save(output.as_mut())?,
            Self::ObjectList(value) => value.save(output.as_mut())?,
            Self::DocumentText(value) => value.save(output.as_mut())?,
            Self::DocumentList(value) => value.save(output.as_mut())?,
            Self::DocumentParagraphList(value) => value.save(output.as_mut())?,
            Self::DocumentTopic(value) => value.save(output.as_mut())?,
            Self::DocumentCode(value) => value.save(output.as_mut())?,
            Self::DocumentMermaid(value) => value.save(output.as_mut())?,
            Self::DocumentHtml(value) => value.save(output.as_mut())?,
            Self::Concept(value) => value.save(output.as_mut())?,
            Self::RequirementUser(value) => value.save(output.as_mut())?,
            Self::RequirementStory(value) => value.save(output.as_mut())?,
            Self::RequirementSystem(value) => value.save(output.as_mut())?,
            Self::RequirementModule(value) => value.save(output.as_mut())?,
            Self::RequirementFunction(value) => value.save(output.as_mut())?,
            Self::RequirementInteraction(value) => value.save(output.as_mut())?,
            Self::TechnicalDirectory(value) => value.save(output.as_mut())?,
            Self::TechnicalFile(value) => value.save(output.as_mut())?,
            Self::TechnicalLayer(value) => value.save(output.as_mut())?,
            Self::TechnicalNamespace(value) => value.save(output.as_mut())?,
            Self::TechnicalClass(value) => value.save(output.as_mut())?,
            Self::TechnicalStruct(value) => value.save(output.as_mut())?,
            Self::TechnicalEnum(value) => value.save(output.as_mut())?,
            Self::TechnicalInterface(value) => value.save(output.as_mut())?,
            Self::TechnicalMethod(value) => value.save(output.as_mut())?,
            Self::TechnicalFunction(value) => value.save(output.as_mut())?,
            Self::TechnicalMixin(value) => value.save(output.as_mut())?,
            Self::TechnicalPart(value) => value.save(output.as_mut())?,
            Self::TechnicalVariable(value) => value.save(output.as_mut())?,
            Self::TechnicalConstant(value) => value.save(output.as_mut())?,
            Self::TechnicalMember(value) => value.save(output.as_mut())?,
            Self::TechnicalProperty(value) => value.save(output.as_mut())?,
            Self::TechnicalEnumValue(value) => value.save(output.as_mut())?,
            Self::TechnicalArgument(value) => value.save(output.as_mut())?,
            Self::TechnicalReturnValue(value) => value.save(output.as_mut())?,
        };
        let hash = ObjectHash(object_hash);
        output.commit(hash)?;
        Ok(hash)
    }

    pub fn compute_object_hash(&self) -> Hash32 {
        match self {
            Self::TechnicalRoot(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::RequirementRoot(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::ConceptDirectory(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalSystem(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalModule(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::Root(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::ObjectList(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::DocumentText(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::DocumentList(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::DocumentParagraphList(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::DocumentTopic(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::DocumentCode(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::DocumentMermaid(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::DocumentHtml(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::Concept(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::RequirementUser(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::RequirementStory(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::RequirementSystem(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::RequirementModule(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::RequirementFunction(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::RequirementInteraction(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalDirectory(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalFile(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalLayer(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalNamespace(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalClass(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalStruct(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalEnum(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalInterface(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalMethod(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalFunction(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalMixin(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalPart(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalVariable(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalConstant(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalMember(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalProperty(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalEnumValue(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalArgument(value) => value.compute_object_hash(OBJECT_VERSION),
            Self::TechnicalReturnValue(value) => value.compute_object_hash(OBJECT_VERSION),
        }
    }
}

impl ObjectCodec for TechnicalRoot {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalRoot version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.object(self.file);
        output.object(self.directory);
        output.object(self.namespace);
        output.object(self.system);
        Ok(object_hash)
    }
}

impl TechnicalRoot {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.file = input.object()?;
        self.directory = input.object()?;
        self.namespace = input.object()?;
        self.system = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalRoot, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.hash(self.file);
        object.hash(self.directory);
        object.hash(self.namespace);
        object.hash(self.system);
        object.finish()
    }
}

impl ObjectCodec for RequirementRoot {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported RequirementRoot version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.object(self.user);
        output.object(self.story);
        output.object(self.system);
        Ok(object_hash)
    }
}

impl RequirementRoot {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.user = input.object()?;
        self.story = input.object()?;
        self.system = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::RequirementRoot, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.hash(self.user);
        object.hash(self.story);
        object.hash(self.system);
        object.finish()
    }
}

impl ObjectCodec for ConceptDirectory {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported ConceptDirectory version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.object(self.directories);
        output.object(self.concepts);
        Ok(object_hash)
    }
}

impl ConceptDirectory {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.directories = input.object()?;
        self.concepts = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::ConceptDirectory, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.hash(self.directories);
        object.hash(self.concepts);
        object.finish()
    }
}

impl ObjectCodec for TechnicalSystem {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalSystem version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.object(self.members);
        output.object(self.modules);
        output.object(self.layers);
        output.object(self.constants);
        output.object(self.variables);
        output.object(self.position);
        output.object(self.responsibilities);
        output.object(self.non_responsibilities);
        output.object(self.features);
        output.object(self.non_features);
        output.object(self.invariants);
        output.object(self.constraints);
        output.object(self.design_principles);
        output.object(self.external_dependencies);
        output.object(self.usage);
        output.object(self.design_points);
        Ok(object_hash)
    }
}

impl TechnicalSystem {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.members = input.object()?;
        self.modules = input.object()?;
        self.layers = input.object()?;
        self.constants = input.object()?;
        self.variables = input.object()?;
        self.position = input.object()?;
        self.responsibilities = input.object()?;
        self.non_responsibilities = input.object()?;
        self.features = input.object()?;
        self.non_features = input.object()?;
        self.invariants = input.object()?;
        self.constraints = input.object()?;
        self.design_principles = input.object()?;
        self.external_dependencies = input.object()?;
        self.usage = input.object()?;
        self.design_points = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalSystem, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.hash(self.members);
        object.hash(self.modules);
        object.hash(self.layers);
        object.hash(self.constants);
        object.hash(self.variables);
        object.hash(self.position);
        object.hash(self.responsibilities);
        object.hash(self.non_responsibilities);
        object.hash(self.features);
        object.hash(self.non_features);
        object.hash(self.invariants);
        object.hash(self.constraints);
        object.hash(self.design_principles);
        object.hash(self.external_dependencies);
        object.hash(self.usage);
        object.hash(self.design_points);
        object.finish()
    }
}

impl ObjectCodec for TechnicalModule {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalModule version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.object(self.members);
        output.symbols(&self.dependencies);
        output.object(self.constants);
        output.object(self.variables);
        output.object(self.position);
        output.object(self.responsibilities);
        output.object(self.non_responsibilities);
        output.object(self.features);
        output.object(self.non_features);
        output.object(self.invariants);
        output.object(self.constraints);
        output.object(self.design_principles);
        output.object(self.usage);
        output.object(self.design_points);
        Ok(object_hash)
    }
}

impl TechnicalModule {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.members = input.object()?;
        self.dependencies = input.symbols()?;
        self.constants = input.object()?;
        self.variables = input.object()?;
        self.position = input.object()?;
        self.responsibilities = input.object()?;
        self.non_responsibilities = input.object()?;
        self.features = input.object()?;
        self.non_features = input.object()?;
        self.invariants = input.object()?;
        self.constraints = input.object()?;
        self.design_principles = input.object()?;
        self.usage = input.object()?;
        self.design_points = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalModule, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.hash(self.members);
        object.symbols(&self.dependencies);
        object.hash(self.constants);
        object.hash(self.variables);
        object.hash(self.position);
        object.hash(self.responsibilities);
        object.hash(self.non_responsibilities);
        object.hash(self.features);
        object.hash(self.non_features);
        object.hash(self.invariants);
        object.hash(self.constraints);
        object.hash(self.design_principles);
        object.hash(self.usage);
        object.hash(self.design_points);
        object.finish()
    }
}

impl ObjectCodec for Root {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported Root version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.hash(self.source_commit.0);
        output.object(self.technical);
        output.object(self.requirement);
        output.object(self.concept);
        Ok(object_hash)
    }
}

impl Root {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.source_commit = crate::hash::GitCommit(input.hash()?);
        self.technical = input.object()?;
        self.requirement = input.object()?;
        self.concept = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::Root, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.hash(self.source_commit.0);
        object.hash(self.technical);
        object.hash(self.requirement);
        object.hash(self.concept);
        object.finish()
    }
}

impl ObjectCodec for ObjectList {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported ObjectList version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.objects(&self.items);
        Ok(object_hash)
    }
}

impl ObjectList {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.items = input.objects()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::ObjectList, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.objects(&self.items);
        object.finish()
    }
}

impl ObjectCodec for DocumentText {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported DocumentText version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.text(&self.content);
        Ok(object_hash)
    }
}

impl DocumentText {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.content = input.text()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::DocumentText, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.text(&self.content);
        object.finish()
    }
}

impl ObjectCodec for DocumentList {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported DocumentList version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.objects(&self.items);
        Ok(object_hash)
    }
}

impl DocumentList {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.items = input.objects()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::DocumentList, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.objects(&self.items);
        object.finish()
    }
}

impl ObjectCodec for DocumentParagraphList {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported DocumentParagraphList version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.objects(&self.items);
        Ok(object_hash)
    }
}

impl DocumentParagraphList {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.items = input.objects()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::DocumentParagraphList, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.objects(&self.items);
        object.finish()
    }
}

impl ObjectCodec for DocumentTopic {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported DocumentTopic version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.text(&self.title);
        output.object(self.body);
        Ok(object_hash)
    }
}

impl DocumentTopic {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.title = input.text()?;
        self.body = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::DocumentTopic, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.text(&self.title);
        object.hash(self.body);
        object.finish()
    }
}

impl ObjectCodec for DocumentCode {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported DocumentCode version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.text(&self.language);
        output.text(&self.source);
        Ok(object_hash)
    }
}

impl DocumentCode {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.language = input.text()?;
        self.source = input.text()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::DocumentCode, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.text(&self.language);
        object.text(&self.source);
        object.finish()
    }
}

impl ObjectCodec for DocumentMermaid {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported DocumentMermaid version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.text(&self.source);
        Ok(object_hash)
    }
}

impl DocumentMermaid {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.source = input.text()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::DocumentMermaid, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.text(&self.source);
        object.finish()
    }
}

impl ObjectCodec for DocumentHtml {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported DocumentHtml version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.text(&self.source);
        Ok(object_hash)
    }
}

impl DocumentHtml {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.source = input.text()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::DocumentHtml, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.text(&self.source);
        object.finish()
    }
}

impl ObjectCodec for Concept {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported Concept version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.text(&self.display_name);
        output.object(self.explanation);
        Ok(object_hash)
    }
}

impl Concept {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.display_name = input.text()?;
        self.explanation = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::Concept, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.text(&self.display_name);
        object.hash(self.explanation);
        object.finish()
    }
}

impl ObjectCodec for RequirementUser {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported RequirementUser version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.text(&self.real_name);
        output.text(&self.age);
        output.text(&self.occupation);
        output.object(self.situations);
        output.object(self.hobbies);
        output.object(self.needs);
        output.u8(self.priority);
        output.symbols(&self.stories);
        Ok(object_hash)
    }
}

impl RequirementUser {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.real_name = input.text()?;
        self.age = input.text()?;
        self.occupation = input.text()?;
        self.situations = input.object()?;
        self.hobbies = input.object()?;
        self.needs = input.object()?;
        self.priority = input.u8()?;
        self.stories = input.symbols()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::RequirementUser, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.text(&self.real_name);
        object.text(&self.age);
        object.text(&self.occupation);
        object.hash(self.situations);
        object.hash(self.hobbies);
        object.hash(self.needs);
        object.u8(self.priority);
        object.symbols(&self.stories);
        object.finish()
    }
}

impl ObjectCodec for RequirementStory {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported RequirementStory version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.object(self.preconditions);
        output.u8(self.frequency);
        output.object(self.description);
        output.object(self.requirement);
        output.symbols(&self.users);
        output.symbols(&self.functions);
        Ok(object_hash)
    }
}

impl RequirementStory {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.preconditions = input.object()?;
        self.frequency = input.u8()?;
        self.description = input.object()?;
        self.requirement = input.object()?;
        self.users = input.symbols()?;
        self.functions = input.symbols()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::RequirementStory, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.hash(self.preconditions);
        object.u8(self.frequency);
        object.hash(self.description);
        object.hash(self.requirement);
        object.symbols(&self.users);
        object.symbols(&self.functions);
        object.finish()
    }
}

impl ObjectCodec for RequirementSystem {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported RequirementSystem version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.object(self.modules);
        output.object(self.form);
        output.object(self.covered_users);
        output.object(self.closed_loop_goal);
        output.object(self.relationships);
        output.object(self.boundary);
        Ok(object_hash)
    }
}

impl RequirementSystem {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.modules = input.object()?;
        self.form = input.object()?;
        self.covered_users = input.object()?;
        self.closed_loop_goal = input.object()?;
        self.relationships = input.object()?;
        self.boundary = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::RequirementSystem, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.hash(self.modules);
        object.hash(self.form);
        object.hash(self.covered_users);
        object.hash(self.closed_loop_goal);
        object.hash(self.relationships);
        object.hash(self.boundary);
        object.finish()
    }
}

impl ObjectCodec for RequirementModule {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported RequirementModule version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.object(self.functions);
        output.object(self.core_concept);
        output.object(self.included_concepts);
        output.object(self.concept_relations);
        output.object(self.boundary);
        Ok(object_hash)
    }
}

impl RequirementModule {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.functions = input.object()?;
        self.core_concept = input.object()?;
        self.included_concepts = input.object()?;
        self.concept_relations = input.object()?;
        self.boundary = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::RequirementModule, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.hash(self.functions);
        object.hash(self.core_concept);
        object.hash(self.included_concepts);
        object.hash(self.concept_relations);
        object.hash(self.boundary);
        object.finish()
    }
}

impl ObjectCodec for RequirementFunction {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported RequirementFunction version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.object(self.flow);
        output.object(self.interactions);
        output.object(self.initiator);
        output.object(self.system_change);
        output.object(self.preconditions);
        output.object(self.feedback);
        output.object(self.description);
        Ok(object_hash)
    }
}

impl RequirementFunction {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.flow = input.object()?;
        self.interactions = input.object()?;
        self.initiator = input.object()?;
        self.system_change = input.object()?;
        self.preconditions = input.object()?;
        self.feedback = input.object()?;
        self.description = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::RequirementFunction, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.hash(self.flow);
        object.hash(self.interactions);
        object.hash(self.initiator);
        object.hash(self.system_change);
        object.hash(self.preconditions);
        object.hash(self.feedback);
        object.hash(self.description);
        object.finish()
    }
}

impl ObjectCodec for RequirementInteraction {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported RequirementInteraction version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.object(self.description);
        output.object(self.ui);
        Ok(object_hash)
    }
}

impl RequirementInteraction {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.description = input.object()?;
        self.ui = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::RequirementInteraction, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.hash(self.description);
        object.hash(self.ui);
        object.finish()
    }
}

impl ObjectCodec for TechnicalDirectory {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalDirectory version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.source_path(self.source);
        output.object(self.children);
        output.object(self.position);
        Ok(object_hash)
    }
}

impl TechnicalDirectory {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.source = input.source_path()?;
        self.children = input.object()?;
        self.position = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalDirectory, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.source_path(self.source);
        object.hash(self.children);
        object.hash(self.position);
        object.finish()
    }
}

impl ObjectCodec for TechnicalFile {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalFile version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.source_path(self.source);
        output.object(self.entities);
        output.object(self.position);
        Ok(object_hash)
    }
}

impl TechnicalFile {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.source = input.source_path()?;
        self.entities = input.object()?;
        self.position = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalFile, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.source_path(self.source);
        object.hash(self.entities);
        object.hash(self.position);
        object.finish()
    }
}

impl ObjectCodec for TechnicalLayer {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalLayer version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.object(self.members);
        output.symbols(&self.dependencies);
        output.object(self.position);
        Ok(object_hash)
    }
}

impl TechnicalLayer {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.members = input.object()?;
        self.dependencies = input.symbols()?;
        self.position = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalLayer, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.hash(self.members);
        object.symbols(&self.dependencies);
        object.hash(self.position);
        object.finish()
    }
}

impl ObjectCodec for TechnicalNamespace {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalNamespace version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.object(self.children);
        output.object(self.entities);
        output.object(self.constants);
        output.object(self.variables);
        output.object(self.position);
        Ok(object_hash)
    }
}

impl TechnicalNamespace {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.children = input.object()?;
        self.entities = input.object()?;
        self.constants = input.object()?;
        self.variables = input.object()?;
        self.position = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalNamespace, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.hash(self.children);
        object.hash(self.entities);
        object.hash(self.constants);
        object.hash(self.variables);
        object.hash(self.position);
        object.finish()
    }
}

impl ObjectCodec for TechnicalClass {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalClass version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.symbol(self.namespace);
        output.symbol(self.file);
        output.symbol(self.module);
        output.source_span(self.source);
        output.symbol(self.superclass);
        output.symbols(&self.interfaces);
        output.symbols(&self.mixins);
        output.symbols(&self.explicit_dependencies);
        output.symbols(&self.implicit_dependencies);
        output.object(self.method);
        output.object(self.types);
        output.object(self.parts);
        output.object(self.constants);
        output.object(self.members);
        output.object(self.properties);
        output.object(self.position);
        output.object(self.responsibilities);
        output.object(self.non_responsibilities);
        output.object(self.features);
        output.object(self.non_features);
        output.object(self.invariants);
        output.object(self.constraints);
        output.object(self.design_principles);
        output.object(self.usage);
        output.object(self.design_points);
        Ok(object_hash)
    }
}

impl TechnicalClass {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.namespace = input.symbol()?;
        self.file = input.symbol()?;
        self.module = input.symbol()?;
        self.source = input.source_span()?;
        self.superclass = input.symbol()?;
        self.interfaces = input.symbols()?;
        self.mixins = input.symbols()?;
        self.explicit_dependencies = input.symbols()?;
        self.implicit_dependencies = input.symbols()?;
        self.method = input.object()?;
        self.types = input.object()?;
        self.parts = input.object()?;
        self.constants = input.object()?;
        self.members = input.object()?;
        self.properties = input.object()?;
        self.position = input.object()?;
        self.responsibilities = input.object()?;
        self.non_responsibilities = input.object()?;
        self.features = input.object()?;
        self.non_features = input.object()?;
        self.invariants = input.object()?;
        self.constraints = input.object()?;
        self.design_principles = input.object()?;
        self.usage = input.object()?;
        self.design_points = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalClass, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.symbol(self.namespace);
        object.symbol(self.file);
        object.symbol(self.module);
        object.source_span(self.source);
        object.symbol(self.superclass);
        object.symbols(&self.interfaces);
        object.symbols(&self.mixins);
        object.symbols(&self.explicit_dependencies);
        object.symbols(&self.implicit_dependencies);
        object.hash(self.method);
        object.hash(self.types);
        object.hash(self.parts);
        object.hash(self.constants);
        object.hash(self.members);
        object.hash(self.properties);
        object.hash(self.position);
        object.hash(self.responsibilities);
        object.hash(self.non_responsibilities);
        object.hash(self.features);
        object.hash(self.non_features);
        object.hash(self.invariants);
        object.hash(self.constraints);
        object.hash(self.design_principles);
        object.hash(self.usage);
        object.hash(self.design_points);
        object.finish()
    }
}

impl ObjectCodec for TechnicalStruct {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalStruct version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.symbol(self.namespace);
        output.symbol(self.file);
        output.symbol(self.module);
        output.source_span(self.source);
        output.symbols(&self.dependencies);
        output.object(self.method);
        output.object(self.types);
        output.object(self.constants);
        output.object(self.members);
        output.object(self.position);
        output.object(self.invariants);
        Ok(object_hash)
    }
}

impl TechnicalStruct {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.namespace = input.symbol()?;
        self.file = input.symbol()?;
        self.module = input.symbol()?;
        self.source = input.source_span()?;
        self.dependencies = input.symbols()?;
        self.method = input.object()?;
        self.types = input.object()?;
        self.constants = input.object()?;
        self.members = input.object()?;
        self.position = input.object()?;
        self.invariants = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalStruct, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.symbol(self.namespace);
        object.symbol(self.file);
        object.symbol(self.module);
        object.source_span(self.source);
        object.symbols(&self.dependencies);
        object.hash(self.method);
        object.hash(self.types);
        object.hash(self.constants);
        object.hash(self.members);
        object.hash(self.position);
        object.hash(self.invariants);
        object.finish()
    }
}

impl ObjectCodec for TechnicalEnum {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalEnum version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.symbol(self.namespace);
        output.symbol(self.file);
        output.symbol(self.module);
        output.source_span(self.source);
        output.symbols(&self.dependencies);
        output.object(self.values);
        output.object(self.position);
        Ok(object_hash)
    }
}

impl TechnicalEnum {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.namespace = input.symbol()?;
        self.file = input.symbol()?;
        self.module = input.symbol()?;
        self.source = input.source_span()?;
        self.dependencies = input.symbols()?;
        self.values = input.object()?;
        self.position = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalEnum, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.symbol(self.namespace);
        object.symbol(self.file);
        object.symbol(self.module);
        object.source_span(self.source);
        object.symbols(&self.dependencies);
        object.hash(self.values);
        object.hash(self.position);
        object.finish()
    }
}

impl ObjectCodec for TechnicalInterface {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalInterface version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.symbol(self.namespace);
        output.symbol(self.file);
        output.symbol(self.module);
        output.source_span(self.source);
        output.symbol(self.extends);
        output.symbols(&self.dependencies);
        output.object(self.methods);
        output.object(self.position);
        output.object(self.responsibilities);
        output.object(self.non_responsibilities);
        output.object(self.features);
        output.object(self.non_features);
        output.object(self.invariants);
        output.object(self.usage);
        output.object(self.design_points);
        Ok(object_hash)
    }
}

impl TechnicalInterface {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.namespace = input.symbol()?;
        self.file = input.symbol()?;
        self.module = input.symbol()?;
        self.source = input.source_span()?;
        self.extends = input.symbol()?;
        self.dependencies = input.symbols()?;
        self.methods = input.object()?;
        self.position = input.object()?;
        self.responsibilities = input.object()?;
        self.non_responsibilities = input.object()?;
        self.features = input.object()?;
        self.non_features = input.object()?;
        self.invariants = input.object()?;
        self.usage = input.object()?;
        self.design_points = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalInterface, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.symbol(self.namespace);
        object.symbol(self.file);
        object.symbol(self.module);
        object.source_span(self.source);
        object.symbol(self.extends);
        object.symbols(&self.dependencies);
        object.hash(self.methods);
        object.hash(self.position);
        object.hash(self.responsibilities);
        object.hash(self.non_responsibilities);
        object.hash(self.features);
        object.hash(self.non_features);
        object.hash(self.invariants);
        object.hash(self.usage);
        object.hash(self.design_points);
        object.finish()
    }
}

impl ObjectCodec for TechnicalMethod {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalMethod version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.source_span(self.source);
        output.bool(self.is_static);
        output.bool(self.is_async);
        output.bool(self.is_abstract);
        output.bool(self.is_const);
        output.symbol(self.overrides);
        output.symbols(&self.explicit_dependencies);
        output.symbols(&self.implicit_dependencies);
        output.object(self.position);
        output.object(self.arguments);
        output.object(self.return_value);
        output.object(self.usage);
        output.object(self.design_points);
        Ok(object_hash)
    }
}

impl TechnicalMethod {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.source = input.source_span()?;
        self.is_static = input.bool()?;
        self.is_async = input.bool()?;
        self.is_abstract = input.bool()?;
        self.is_const = input.bool()?;
        self.overrides = input.symbol()?;
        self.explicit_dependencies = input.symbols()?;
        self.implicit_dependencies = input.symbols()?;
        self.position = input.object()?;
        self.arguments = input.object()?;
        self.return_value = input.object()?;
        self.usage = input.object()?;
        self.design_points = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalMethod, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.source_span(self.source);
        object.bool(self.is_static);
        object.bool(self.is_async);
        object.bool(self.is_abstract);
        object.bool(self.is_const);
        object.symbol(self.overrides);
        object.symbols(&self.explicit_dependencies);
        object.symbols(&self.implicit_dependencies);
        object.hash(self.position);
        object.hash(self.arguments);
        object.hash(self.return_value);
        object.hash(self.usage);
        object.hash(self.design_points);
        object.finish()
    }
}

impl ObjectCodec for TechnicalFunction {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalFunction version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.symbol(self.namespace);
        output.symbol(self.file);
        output.symbol(self.module);
        output.source_span(self.source);
        output.bool(self.is_async);
        output.symbols(&self.explicit_dependencies);
        output.symbols(&self.implicit_dependencies);
        output.object(self.position);
        output.object(self.arguments);
        output.object(self.return_value);
        output.object(self.usage);
        output.object(self.design_points);
        Ok(object_hash)
    }
}

impl TechnicalFunction {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.namespace = input.symbol()?;
        self.file = input.symbol()?;
        self.module = input.symbol()?;
        self.source = input.source_span()?;
        self.is_async = input.bool()?;
        self.explicit_dependencies = input.symbols()?;
        self.implicit_dependencies = input.symbols()?;
        self.position = input.object()?;
        self.arguments = input.object()?;
        self.return_value = input.object()?;
        self.usage = input.object()?;
        self.design_points = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalFunction, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.symbol(self.namespace);
        object.symbol(self.file);
        object.symbol(self.module);
        object.source_span(self.source);
        object.bool(self.is_async);
        object.symbols(&self.explicit_dependencies);
        object.symbols(&self.implicit_dependencies);
        object.hash(self.position);
        object.hash(self.arguments);
        object.hash(self.return_value);
        object.hash(self.usage);
        object.hash(self.design_points);
        object.finish()
    }
}

impl ObjectCodec for TechnicalMixin {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalMixin version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.symbol(self.namespace);
        output.symbol(self.file);
        output.symbol(self.module);
        output.source_span(self.source);
        output.symbols(&self.on);
        output.symbol(self.superclass);
        output.symbols(&self.interfaces);
        output.symbols(&self.mixins);
        output.symbols(&self.explicit_dependencies);
        output.symbols(&self.implicit_dependencies);
        output.object(self.methods);
        output.object(self.types);
        output.object(self.position);
        output.object(self.responsibilities);
        output.object(self.non_responsibilities);
        output.object(self.features);
        output.object(self.non_features);
        output.object(self.invariants);
        output.object(self.usage);
        output.object(self.design_points);
        Ok(object_hash)
    }
}

impl TechnicalMixin {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.namespace = input.symbol()?;
        self.file = input.symbol()?;
        self.module = input.symbol()?;
        self.source = input.source_span()?;
        self.on = input.symbols()?;
        self.superclass = input.symbol()?;
        self.interfaces = input.symbols()?;
        self.mixins = input.symbols()?;
        self.explicit_dependencies = input.symbols()?;
        self.implicit_dependencies = input.symbols()?;
        self.methods = input.object()?;
        self.types = input.object()?;
        self.position = input.object()?;
        self.responsibilities = input.object()?;
        self.non_responsibilities = input.object()?;
        self.features = input.object()?;
        self.non_features = input.object()?;
        self.invariants = input.object()?;
        self.usage = input.object()?;
        self.design_points = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalMixin, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.symbol(self.namespace);
        object.symbol(self.file);
        object.symbol(self.module);
        object.source_span(self.source);
        object.symbols(&self.on);
        object.symbol(self.superclass);
        object.symbols(&self.interfaces);
        object.symbols(&self.mixins);
        object.symbols(&self.explicit_dependencies);
        object.symbols(&self.implicit_dependencies);
        object.hash(self.methods);
        object.hash(self.types);
        object.hash(self.position);
        object.hash(self.responsibilities);
        object.hash(self.non_responsibilities);
        object.hash(self.features);
        object.hash(self.non_features);
        object.hash(self.invariants);
        object.hash(self.usage);
        object.hash(self.design_points);
        object.finish()
    }
}

impl ObjectCodec for TechnicalPart {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalPart version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.source_span(self.source);
        output.symbols(&self.explicit_dependencies);
        output.symbols(&self.implicit_dependencies);
        output.object(self.members);
        output.object(self.methods);
        output.object(self.types);
        Ok(object_hash)
    }
}

impl TechnicalPart {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.source = input.source_span()?;
        self.explicit_dependencies = input.symbols()?;
        self.implicit_dependencies = input.symbols()?;
        self.members = input.object()?;
        self.methods = input.object()?;
        self.types = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalPart, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.source_span(self.source);
        object.symbols(&self.explicit_dependencies);
        object.symbols(&self.implicit_dependencies);
        object.hash(self.members);
        object.hash(self.methods);
        object.hash(self.types);
        object.finish()
    }
}

impl ObjectCodec for TechnicalVariable {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalVariable version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.source_span(self.source);
        output.symbol(self.ty);
        output.object(self.position);
        Ok(object_hash)
    }
}

impl TechnicalVariable {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.source = input.source_span()?;
        self.ty = input.symbol()?;
        self.position = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalVariable, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.source_span(self.source);
        object.symbol(self.ty);
        object.hash(self.position);
        object.finish()
    }
}

impl ObjectCodec for TechnicalConstant {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalConstant version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.source_span(self.source);
        output.symbol(self.ty);
        output.object(self.position);
        Ok(object_hash)
    }
}

impl TechnicalConstant {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.source = input.source_span()?;
        self.ty = input.symbol()?;
        self.position = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalConstant, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.source_span(self.source);
        object.symbol(self.ty);
        object.hash(self.position);
        object.finish()
    }
}

impl ObjectCodec for TechnicalMember {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalMember version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.source_span(self.source);
        output.symbol(self.ty);
        output.bool(self.is_static);
        output.bool(self.is_const);
        output.object(self.position);
        Ok(object_hash)
    }
}

impl TechnicalMember {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.source = input.source_span()?;
        self.ty = input.symbol()?;
        self.is_static = input.bool()?;
        self.is_const = input.bool()?;
        self.position = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalMember, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.source_span(self.source);
        object.symbol(self.ty);
        object.bool(self.is_static);
        object.bool(self.is_const);
        object.hash(self.position);
        object.finish()
    }
}

impl ObjectCodec for TechnicalProperty {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalProperty version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.symbol(self.ty);
        output.source_span(self.getter);
        output.source_span(self.setter);
        output.object(self.position);
        Ok(object_hash)
    }
}

impl TechnicalProperty {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.ty = input.symbol()?;
        self.getter = input.source_span()?;
        self.setter = input.source_span()?;
        self.position = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalProperty, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.symbol(self.ty);
        object.source_span(self.getter);
        object.source_span(self.setter);
        object.hash(self.position);
        object.finish()
    }
}

impl ObjectCodec for TechnicalEnumValue {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalEnumValue version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.source_span(self.source);
        output.object(self.position);
        Ok(object_hash)
    }
}

impl TechnicalEnumValue {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.source = input.source_span()?;
        self.position = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalEnumValue, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.source_span(self.source);
        object.hash(self.position);
        object.finish()
    }
}

impl ObjectCodec for TechnicalArgument {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalArgument version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.source_span(self.source);
        output.symbol(self.ty);
        output.object(self.position);
        Ok(object_hash)
    }
}

impl TechnicalArgument {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.source = input.source_span()?;
        self.ty = input.symbol()?;
        self.position = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalArgument, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.source_span(self.source);
        object.symbol(self.ty);
        object.hash(self.position);
        object.finish()
    }
}

impl ObjectCodec for TechnicalReturnValue {
    fn load_into(&mut self, input: &mut dyn InputObject) -> Result<()> {
        match input.version() {
            1 => self.load_v1(input),
            version => bail!("unsupported TechnicalReturnValue version {version}"),
        }
    }

    fn save(&mut self, output: &mut dyn OutputObject) -> Result<Hash32> {
        let object_hash = self.compute_object_hash(output.version());
        output.symbol(self.qname);
        output.symbol(self.name);
        output.symbol(self.parent);
        output.source_span(self.source);
        output.symbol(self.ty);
        output.object(self.position);
        Ok(object_hash)
    }
}

impl TechnicalReturnValue {
    fn load_v1(&mut self, input: &mut dyn InputObject) -> Result<()> {
        self.qname = input.symbol()?;
        self.name = input.symbol()?;
        self.parent = input.symbol()?;
        self.source = input.source_span()?;
        self.ty = input.symbol()?;
        self.position = input.object()?;
        Ok(())
    }

    fn compute_object_hash(&self, version: u8) -> Hash32 {
        let mut object = ObjectHasher::new(ObjectKind::TechnicalReturnValue, version);
        object.symbol(self.qname);
        object.symbol(self.name);
        object.symbol(self.parent);
        object.source_span(self.source);
        object.symbol(self.ty);
        object.hash(self.position);
        object.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::GitCommit;
    use crate::repo::Repository;
    use crate::source::{SourcePath, SourceSpan};
    use tempfile::TempDir;

    fn sym(text: &str) -> SymbolHash {
        SymbolHash::of(text)
    }

    fn zero_refs<const N: usize>() -> [ObjectHash; N] {
        [ObjectHash::ZERO; N]
    }

    #[test]
    fn every_object_kind_round_trips() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let [
            a,
            b,
            c,
            d,
            e,
            f,
            g,
            h,
            i,
            j,
            k,
            l,
            m,
            n,
            o,
            p,
            q,
            r,
            s,
            t,
            u,
            v,
            w,
            x,
            y,
            z,
            aa,
            ab,
        ] = zero_refs::<28>();
        let _ = (n, o, p, q, r, s, t, u, v, w, x, y, z, aa, ab);

        let objects = vec![
            Object::TechnicalRoot(TechnicalRoot {
                qname: sym("technical"),
                name: sym("technical"),
                parent: sym("root"),
                file: a,
                directory: b,
                namespace: b,
                system: c,
            }),
            Object::RequirementRoot(RequirementRoot {
                qname: sym("requirement"),
                name: sym("requirement"),
                parent: sym("root"),
                user: a,
                story: b,
                system: c,
            }),
            Object::ConceptDirectory(ConceptDirectory {
                qname: sym("concept"),
                name: sym("concept"),
                parent: sym("root"),
                directories: a,
                concepts: b,
            }),
            Object::TechnicalSystem(TechnicalSystem {
                qname: sym("technical/app"),
                name: sym("app"),
                parent: sym("technical"),
                members: a,
                modules: b,
                layers: c,
                constants: d,
                variables: e,
                position: d,
                responsibilities: e,
                non_responsibilities: f,
                features: g,
                non_features: h,
                invariants: i,
                constraints: j,
                design_principles: k,
                external_dependencies: l,
                usage: m,
                design_points: n,
            }),
            Object::TechnicalModule(TechnicalModule {
                qname: sym("technical/app/module"),
                name: sym("module"),
                parent: sym("technical/app"),
                members: a,
                dependencies: Vec::new(),
                constants: d,
                variables: e,
                position: d,
                responsibilities: e,
                non_responsibilities: f,
                features: g,
                non_features: h,
                invariants: i,
                constraints: j,
                design_principles: k,
                usage: m,
                design_points: n,
            }),
            Object::Root(Root {
                qname: sym("root"),
                name: sym("root"),
                source_commit: GitCommit::ZERO,
                technical: a,
                requirement: b,
                concept: c,
            }),
            Object::ObjectList(ObjectList {
                qname: sym("list"),
                name: sym("list"),
                parent: sym("owner"),
                items: Vec::new(),
            }),
            Object::DocumentText(DocumentText {
                qname: sym("text"),
                name: sym("text"),
                parent: sym("parent"),
                content: "text".into(),
            }),
            Object::DocumentList(DocumentList {
                qname: sym("doc-list"),
                name: sym("doc-list"),
                parent: sym("parent"),
                items: Vec::new(),
            }),
            Object::DocumentParagraphList(DocumentParagraphList {
                qname: sym("paragraphs"),
                name: sym("paragraphs"),
                parent: sym("parent"),
                items: Vec::new(),
            }),
            Object::DocumentTopic(DocumentTopic {
                qname: sym("topic"),
                name: sym("topic"),
                parent: sym("parent"),
                title: "title".into(),
                body: a,
            }),
            Object::DocumentCode(DocumentCode {
                qname: sym("code"),
                name: sym("code"),
                parent: sym("parent"),
                language: "rust".into(),
                source: "fn main() {}".into(),
            }),
            Object::DocumentMermaid(DocumentMermaid {
                qname: sym("mermaid"),
                name: sym("mermaid"),
                parent: sym("parent"),
                source: "flowchart TD".into(),
            }),
            Object::DocumentHtml(DocumentHtml {
                qname: sym("html"),
                name: sym("html"),
                parent: sym("parent"),
                source: "<div></div>".into(),
            }),
            Object::Concept(Concept {
                qname: sym("concept/domain/agent"),
                name: sym("agent"),
                parent: sym("concept/domain"),
                display_name: "智能体".to_owned(),
                explanation: b,
            }),
            Object::RequirementUser(RequirementUser {
                qname: sym("requirement/user/u"),
                name: sym("u"),
                parent: sym("requirement"),
                real_name: "Alice".to_owned(),
                age: "30".to_owned(),
                occupation: "engineer".to_owned(),
                situations: b,
                hobbies: c,
                needs: f,
                priority: 3,
                stories: Vec::new(),
            }),
            Object::RequirementStory(RequirementStory {
                qname: sym("requirement/story/s"),
                name: sym("s"),
                parent: sym("requirement"),
                preconditions: a,
                frequency: 2,
                description: c,
                requirement: d,
                users: Vec::new(),
                functions: Vec::new(),
            }),
            Object::RequirementSystem(RequirementSystem {
                qname: sym("requirement/system/sys"),
                name: sym("sys"),
                parent: sym("requirement/system"),
                modules: a,
                form: b,
                covered_users: c,
                closed_loop_goal: d,
                relationships: e,
                boundary: f,
            }),
            Object::RequirementModule(RequirementModule {
                qname: sym("requirement/system/sys/module"),
                name: sym("module"),
                parent: sym("requirement/system/sys"),
                functions: a,
                core_concept: b,
                included_concepts: c,
                concept_relations: d,
                boundary: e,
            }),
            Object::RequirementFunction(RequirementFunction {
                qname: sym("requirement/system/sys/module/function"),
                name: sym("function"),
                parent: sym("requirement/system/sys/module"),
                flow: a,
                interactions: b,
                initiator: c,
                system_change: d,
                preconditions: e,
                feedback: f,
                description: g,
            }),
            Object::RequirementInteraction(RequirementInteraction {
                qname: sym("requirement/system/sys/module/function/interaction"),
                name: sym("interaction"),
                parent: sym("requirement/system/sys/module/function"),
                description: a,
                ui: b,
            }),
            Object::TechnicalDirectory(TechnicalDirectory {
                qname: sym("technical/directory"),
                name: sym("directory"),
                parent: sym("technical"),
                source: SourcePath {
                    path: sym("src/directory"),
                },
                children: a,
                position: b,
            }),
            Object::TechnicalFile(TechnicalFile {
                qname: sym("technical/file"),
                name: sym("file"),
                parent: sym("technical/directory"),
                source: SourcePath {
                    path: sym("src/file"),
                },
                entities: a,
                position: b,
            }),
            Object::TechnicalLayer(TechnicalLayer {
                qname: sym("technical/layer"),
                name: sym("layer"),
                parent: sym("technical"),
                members: a,
                dependencies: Vec::new(),
                position: b,
            }),
            Object::TechnicalNamespace(TechnicalNamespace {
                qname: sym("technical/namespace"),
                name: sym("namespace"),
                parent: sym("technical"),
                children: a,
                entities: c,
                constants: d,
                variables: e,
                position: b,
            }),
            Object::TechnicalClass(TechnicalClass {
                qname: sym("technical/class"),
                name: sym("class"),
                parent: sym("technical"),
                namespace: sym("technical/namespace"),
                file: sym("technical/file"),
                module: sym("technical/module"),
                source: SourceSpan {
                    path: sym("src/class"),
                    start: 0,
                    end: 0,
                },
                superclass: sym("base"),
                interfaces: Vec::new(),
                mixins: Vec::new(),
                explicit_dependencies: Vec::new(),
                implicit_dependencies: Vec::new(),
                method: a,
                types: b,
                parts: c,
                constants: d,
                members: e,
                properties: f,
                position: d,
                responsibilities: e,
                non_responsibilities: f,
                features: g,
                non_features: h,
                invariants: i,
                constraints: j,
                design_principles: k,
                usage: l,
                design_points: m,
            }),
            Object::TechnicalStruct(TechnicalStruct {
                qname: sym("technical/struct"),
                name: sym("struct"),
                parent: sym("technical"),
                namespace: sym("technical/namespace"),
                file: sym("technical/file"),
                module: sym("technical/module"),
                source: SourceSpan {
                    path: sym("src/struct"),
                    start: 0,
                    end: 0,
                },
                dependencies: Vec::new(),
                method: a,
                types: b,
                constants: d,
                members: e,
                position: d,
                invariants: e,
            }),
            Object::TechnicalEnum(TechnicalEnum {
                qname: sym("technical/enum"),
                name: sym("enum"),
                parent: sym("technical"),
                namespace: sym("technical/namespace"),
                file: sym("technical/file"),
                module: sym("technical/module"),
                source: SourceSpan {
                    path: sym("src/enum"),
                    start: 0,
                    end: 0,
                },
                dependencies: Vec::new(),
                values: a,
                position: c,
            }),
            Object::TechnicalEnumValue(TechnicalEnumValue {
                qname: sym("technical/enum/RED"),
                name: sym("RED"),
                parent: sym("technical/enum"),
                source: SourceSpan {
                    path: sym("src/enum"),
                    start: 3,
                    end: 6,
                },
                position: b,
            }),
            Object::TechnicalArgument(TechnicalArgument {
                qname: sym("technical/method/arg"),
                name: sym("arg"),
                parent: sym("technical/method"),
                source: SourceSpan {
                    path: sym("src/method"),
                    start: 5,
                    end: 10,
                },
                ty: sym("int"),
                position: a,
            }),
            Object::TechnicalReturnValue(TechnicalReturnValue {
                qname: sym("technical/method/return"),
                name: sym("return"),
                parent: sym("technical/method"),
                source: SourceSpan {
                    path: sym("src/method"),
                    start: 11,
                    end: 14,
                },
                ty: sym("int"),
                position: a,
            }),
            Object::TechnicalInterface(TechnicalInterface {
                qname: sym("technical/interface"),
                name: sym("interface"),
                parent: sym("technical"),
                namespace: sym("technical/namespace"),
                file: sym("technical/file"),
                module: sym("technical/module"),
                source: SourceSpan {
                    path: sym("src/interface"),
                    start: 0,
                    end: 0,
                },
                extends: sym("base"),
                dependencies: Vec::new(),
                methods: a,
                position: d,
                responsibilities: e,
                non_responsibilities: f,
                features: g,
                non_features: h,
                invariants: i,
                usage: j,
                design_points: k,
            }),
            Object::TechnicalMethod(TechnicalMethod {
                qname: sym("technical/method"),
                name: sym("method"),
                parent: sym("technical/class"),
                source: SourceSpan {
                    path: sym("src/method"),
                    start: 0,
                    end: 0,
                },
                is_static: false,
                is_async: true,
                is_abstract: true,
                is_const: false,
                overrides: sym("base.method"),
                explicit_dependencies: Vec::new(),
                implicit_dependencies: Vec::new(),
                position: a,
                arguments: c,
                return_value: d,
                usage: e,
                design_points: f,
            }),
            Object::TechnicalFunction(TechnicalFunction {
                qname: sym("technical/function"),
                name: sym("function"),
                parent: sym("technical/namespace"),
                namespace: sym("technical/namespace"),
                file: sym("technical/file"),
                module: sym("technical/module"),
                source: SourceSpan {
                    path: sym("src/function"),
                    start: 0,
                    end: 0,
                },
                is_async: true,
                explicit_dependencies: Vec::new(),
                implicit_dependencies: Vec::new(),
                position: a,
                arguments: c,
                return_value: d,
                usage: e,
                design_points: f,
            }),
            Object::TechnicalMixin(TechnicalMixin {
                qname: sym("technical/mixin"),
                name: sym("mixin"),
                parent: sym("technical"),
                namespace: sym("technical/namespace"),
                file: sym("technical/file"),
                module: sym("technical/module"),
                source: SourceSpan {
                    path: sym("src/mixin"),
                    start: 0,
                    end: 0,
                },
                on: Vec::new(),
                superclass: sym("base"),
                interfaces: Vec::new(),
                mixins: Vec::new(),
                explicit_dependencies: Vec::new(),
                implicit_dependencies: Vec::new(),
                methods: a,
                types: b,
                position: d,
                responsibilities: e,
                non_responsibilities: f,
                features: g,
                non_features: h,
                invariants: i,
                usage: j,
                design_points: k,
            }),
            Object::TechnicalPart(TechnicalPart {
                qname: sym("technical/part"),
                name: sym("part"),
                parent: sym("technical"),
                source: SourceSpan {
                    path: sym("src/part"),
                    start: 0,
                    end: 0,
                },
                explicit_dependencies: Vec::new(),
                implicit_dependencies: Vec::new(),
                members: a,
                methods: b,
                types: c,
            }),
            Object::TechnicalVariable(TechnicalVariable {
                qname: sym("technical/variable"),
                name: sym("variable"),
                parent: sym("technical"),
                source: SourceSpan {
                    path: sym("src/variable"),
                    start: 0,
                    end: 0,
                },
                ty: sym("int"),
                position: a,
            }),
            Object::TechnicalConstant(TechnicalConstant {
                qname: sym("technical/constant"),
                name: sym("constant"),
                parent: sym("technical"),
                source: SourceSpan {
                    path: sym("src/constant"),
                    start: 0,
                    end: 0,
                },
                ty: sym("int"),
                position: a,
            }),
            Object::TechnicalMember(TechnicalMember {
                qname: sym("technical/member"),
                name: sym("member"),
                parent: sym("technical/class"),
                source: SourceSpan {
                    path: sym("src/member"),
                    start: 0,
                    end: 0,
                },
                ty: sym("int"),
                is_static: true,
                is_const: true,
                position: a,
            }),
            Object::TechnicalProperty(TechnicalProperty {
                qname: sym("technical/property"),
                name: sym("property"),
                parent: sym("technical/class"),
                ty: sym("int"),
                getter: SourceSpan {
                    path: sym("src/property"),
                    start: 0,
                    end: 4,
                },
                setter: SourceSpan {
                    path: sym("src/property"),
                    start: 5,
                    end: 9,
                },
                position: a,
            }),
        ];

        for mut object in objects {
            let hash = object.save(&repo).unwrap();
            let loaded = Object::load(&repo, hash).unwrap();
            assert_eq!(
                loaded,
                object,
                "{} did not round trip",
                object.kind().name()
            );
        }
    }
}
