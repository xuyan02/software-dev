use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::comment::{Comment, CommentStore};
use crate::commit::Commit;
use crate::data::{InputObject, OutputObject};
use crate::hash::{CommitHash, GitCommit, Hash32, ObjectHash, SymbolHash};
use crate::object::{
    Concept, ConceptDirectory, DocumentList, DocumentMermaid, DocumentParagraphList, Object,
    ObjectKind, ObjectList, RequirementFunction, RequirementInteraction, RequirementModule,
    RequirementRoot, RequirementStory, RequirementSystem, RequirementUser, Root, TechnicalClass,
    TechnicalDirectory, TechnicalEnum, TechnicalFile, TechnicalFunction, TechnicalInterface,
    TechnicalLayer, TechnicalMethod, TechnicalMixin, TechnicalNamespace, TechnicalRoot,
    TechnicalStruct, TechnicalSystem,
};
use crate::pack::{PackKind, PackRecord};
use crate::property::{PropertyAccess, PropertyValue, ValueKind};
use crate::protocol::{read_ref_file, refs_under, valid_record_path, write_ref_file};
use crate::refs::StringRefList;
use crate::snapshot::Snapshot;
use crate::source::{SourcePath, SourceSpan};
use crate::store::{CommitStore, LooseObjectStore, Store};
use crate::symbol::SymbolStore;
use crate::util::{list_files, write_atomic};

type CodeTypeOwner = (String, &'static str, SymbolHash, SymbolHash, String);

fn is_assignable(object: &Object) -> bool {
    matches!(
        object,
        Object::TechnicalClass(_)
            | Object::TechnicalStruct(_)
            | Object::TechnicalEnum(_)
            | Object::TechnicalInterface(_)
            | Object::TechnicalFunction(_)
            | Object::TechnicalMixin(_)
    )
}

fn requires_design(object: &Object) -> bool {
    matches!(
        object,
        Object::TechnicalSystem(_)
            | Object::TechnicalModule(_)
            | Object::TechnicalLayer(_)
            | Object::TechnicalDirectory(_)
            | Object::TechnicalFile(_)
            | Object::TechnicalNamespace(_)
            | Object::TechnicalClass(_)
            | Object::TechnicalStruct(_)
            | Object::TechnicalEnum(_)
            | Object::TechnicalInterface(_)
            | Object::TechnicalMixin(_)
            | Object::TechnicalFunction(_)
            | Object::TechnicalMethod(_)
    )
}

struct RefLock {
    path: PathBuf,
    _file: fs::File,
}

impl Drop for RefLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub const ARCHE_DIR: &str = ".arche";
const MAIN_REF: &str = "refs/heads/main";
static WORKTREE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct Repository {
    root: PathBuf,
    dir: PathBuf,
    tmp: PathBuf,
    source_url: Option<String>,
    remote_url: Option<String>,
    symbols: SymbolStore,
    objects: LooseObjectStore,
    commits: CommitStore,
    comments: CommentStore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocFieldKind {
    Text,
    List,
    ParagraphList,
    Mermaid,
}

#[derive(Debug, Clone, Copy)]
enum ListField {
    Users,
    Stories,
    Systems,
    Modules,
    Functions,
    Interactions,
}

impl ListField {
    fn name(self) -> &'static str {
        match self {
            Self::Users => "user",
            Self::Stories => "story",
            Self::Systems => "system",
            Self::Modules => "modules",
            Self::Functions => "functions",
            Self::Interactions => "interactions",
        }
    }

    fn get(self, object: &Object) -> Result<ObjectHash> {
        Ok(match (self, object) {
            (Self::Users, Object::RequirementRoot(value)) => value.user,
            (Self::Stories, Object::RequirementRoot(value)) => value.story,
            (Self::Systems, Object::RequirementRoot(value)) => value.system,
            (Self::Modules, Object::RequirementSystem(value)) => value.modules,
            (Self::Functions, Object::RequirementModule(value)) => value.functions,
            (Self::Interactions, Object::RequirementFunction(value)) => value.interactions,
            (_, other) => bail!("{} has no {} list", other.kind().name(), self.name()),
        })
    }

    fn set(self, object: &mut Object, value: ObjectHash) -> Result<()> {
        match (self, object) {
            (Self::Users, Object::RequirementRoot(object)) => object.user = value,
            (Self::Stories, Object::RequirementRoot(object)) => object.story = value,
            (Self::Systems, Object::RequirementRoot(object)) => object.system = value,
            (Self::Modules, Object::RequirementSystem(object)) => object.modules = value,
            (Self::Functions, Object::RequirementModule(object)) => object.functions = value,
            (Self::Interactions, Object::RequirementFunction(object)) => {
                object.interactions = value;
            }
            (_, other) => bail!("{} has no {} list", other.kind().name(), self.name()),
        }
        Ok(())
    }
}

fn clean_zero_items(object: &mut Object) {
    match object {
        Object::ObjectList(value) => value.items.retain(|item| !item.is_zero()),
        Object::DocumentList(value) => value.items.retain(|item| !item.is_zero()),
        Object::DocumentParagraphList(value) => value.items.retain(|item| !item.is_zero()),
        _ => {}
    }
}

fn object_list_field(object: &Object, field: &str) -> Result<ObjectHash> {
    Ok(match (object, field) {
        (Object::TechnicalRoot(value), "file") => value.file,
        (Object::TechnicalRoot(value), "namespace") => value.namespace,
        (Object::TechnicalRoot(value), "system") => value.system,
        (Object::TechnicalSystem(value), "members") => value.members,
        (Object::TechnicalSystem(value), "modules") => value.modules,
        (Object::TechnicalSystem(value), "layers") => value.layers,
        (Object::TechnicalSystem(value), "constants") => value.constants,
        (Object::TechnicalSystem(value), "variables") => value.variables,
        (Object::TechnicalModule(value), "members") => value.members,
        (Object::TechnicalModule(value), "constants") => value.constants,
        (Object::TechnicalModule(value), "variables") => value.variables,
        (Object::TechnicalNamespace(value), "children") => value.children,
        (Object::TechnicalNamespace(value), "entities") => value.entities,
        (Object::TechnicalNamespace(value), "constants") => value.constants,
        (Object::TechnicalNamespace(value), "variables") => value.variables,
        (Object::TechnicalDirectory(value), "children") => value.children,
        (Object::TechnicalFile(value), "entities") => value.entities,
        (Object::TechnicalClass(value), "method") => value.method,
        (Object::TechnicalClass(value), "types") => value.types,
        (Object::TechnicalClass(value), "parts") => value.parts,
        (Object::TechnicalClass(value), "constants") => value.constants,
        (Object::TechnicalClass(value), "members") => value.members,
        (Object::TechnicalClass(value), "properties") => value.properties,
        (Object::TechnicalStruct(value), "method") => value.method,
        (Object::TechnicalStruct(value), "types") => value.types,
        (Object::TechnicalStruct(value), "constants") => value.constants,
        (Object::TechnicalStruct(value), "members") => value.members,
        (Object::TechnicalEnum(value), "values") => value.values,
        (Object::TechnicalInterface(value), "methods") => value.methods,
        (Object::TechnicalMethod(value), "arguments") => value.arguments,
        (Object::TechnicalFunction(value), "arguments") => value.arguments,
        (Object::TechnicalMixin(value), "methods") => value.methods,
        (Object::TechnicalMixin(value), "types") => value.types,
        (Object::TechnicalPart(value), "members") => value.members,
        (Object::TechnicalPart(value), "methods") => value.methods,
        (Object::TechnicalPart(value), "types") => value.types,
        (Object::ConceptDirectory(value), "directories") => value.directories,
        (Object::ConceptDirectory(value), "concepts") => value.concepts,
        (Object::RequirementRoot(value), "user") => value.user,
        (Object::RequirementRoot(value), "story") => value.story,
        (Object::RequirementRoot(value), "system") => value.system,
        (Object::RequirementSystem(value), "modules") => value.modules,
        (Object::RequirementModule(value), "functions") => value.functions,
        (Object::RequirementFunction(value), "interactions") => value.interactions,
        _ => bail!("{} has no list field {field}", object.kind().name()),
    })
}

fn set_object_list_field(object: &mut Object, field: &str, value: ObjectHash) -> Result<()> {
    match (object, field) {
        (Object::TechnicalRoot(object), "file") => object.file = value,
        (Object::TechnicalRoot(object), "namespace") => object.namespace = value,
        (Object::TechnicalRoot(object), "system") => object.system = value,
        (Object::TechnicalSystem(object), "members") => object.members = value,
        (Object::TechnicalSystem(object), "modules") => object.modules = value,
        (Object::TechnicalSystem(object), "layers") => object.layers = value,
        (Object::TechnicalSystem(object), "constants") => object.constants = value,
        (Object::TechnicalSystem(object), "variables") => object.variables = value,
        (Object::TechnicalModule(object), "members") => object.members = value,
        (Object::TechnicalModule(object), "constants") => object.constants = value,
        (Object::TechnicalModule(object), "variables") => object.variables = value,
        (Object::TechnicalNamespace(object), "children") => object.children = value,
        (Object::TechnicalNamespace(object), "entities") => object.entities = value,
        (Object::TechnicalNamespace(object), "constants") => object.constants = value,
        (Object::TechnicalNamespace(object), "variables") => object.variables = value,
        (Object::TechnicalDirectory(object), "children") => object.children = value,
        (Object::TechnicalFile(object), "entities") => object.entities = value,
        (Object::TechnicalClass(object), "method") => object.method = value,
        (Object::TechnicalClass(object), "types") => object.types = value,
        (Object::TechnicalClass(object), "parts") => object.parts = value,
        (Object::TechnicalClass(object), "constants") => object.constants = value,
        (Object::TechnicalClass(object), "members") => object.members = value,
        (Object::TechnicalClass(object), "properties") => object.properties = value,
        (Object::TechnicalStruct(object), "method") => object.method = value,
        (Object::TechnicalStruct(object), "types") => object.types = value,
        (Object::TechnicalStruct(object), "constants") => object.constants = value,
        (Object::TechnicalStruct(object), "members") => object.members = value,
        (Object::TechnicalEnum(object), "values") => object.values = value,
        (Object::TechnicalInterface(object), "methods") => object.methods = value,
        (Object::TechnicalMethod(object), "arguments") => object.arguments = value,
        (Object::TechnicalFunction(object), "arguments") => object.arguments = value,
        (Object::TechnicalMixin(object), "methods") => object.methods = value,
        (Object::TechnicalMixin(object), "types") => object.types = value,
        (Object::TechnicalPart(object), "members") => object.members = value,
        (Object::TechnicalPart(object), "methods") => object.methods = value,
        (Object::TechnicalPart(object), "types") => object.types = value,
        (Object::ConceptDirectory(object), "directories") => object.directories = value,
        (Object::ConceptDirectory(object), "concepts") => object.concepts = value,
        (Object::RequirementRoot(object), "user") => object.user = value,
        (Object::RequirementRoot(object), "story") => object.story = value,
        (Object::RequirementRoot(object), "system") => object.system = value,
        (Object::RequirementSystem(object), "modules") => object.modules = value,
        (Object::RequirementModule(object), "functions") => object.functions = value,
        (Object::RequirementFunction(object), "interactions") => object.interactions = value,
        _ => bail!("object has no list field {field}"),
    }
    Ok(())
}

fn doc_field_kind(object: &Object, field: &str) -> Result<DocFieldKind> {
    let kind = match object {
        Object::TechnicalSystem(_) => match field {
            "position" => DocFieldKind::Text,
            "responsibilities"
            | "non_responsibilities"
            | "features"
            | "non_features"
            | "invariants"
            | "constraints"
            | "design_principles"
            | "external_dependencies" => DocFieldKind::List,
            "usage" | "design_points" => DocFieldKind::ParagraphList,
            _ => bail!("{} has no field {field}", object.kind().name()),
        },
        Object::TechnicalModule(_) => match field {
            "position" => DocFieldKind::Text,
            "responsibilities"
            | "non_responsibilities"
            | "features"
            | "non_features"
            | "invariants"
            | "constraints"
            | "design_principles" => DocFieldKind::List,
            "usage" | "design_points" => DocFieldKind::ParagraphList,
            _ => bail!("{} has no field {field}", object.kind().name()),
        },
        Object::TechnicalDirectory(_)
        | Object::TechnicalFile(_)
        | Object::TechnicalLayer(_)
        | Object::TechnicalNamespace(_) => match field {
            "position" => DocFieldKind::Text,
            _ => bail!("{} has no field {field}", object.kind().name()),
        },
        Object::TechnicalClass(_) => match field {
            "position" => DocFieldKind::Text,
            "responsibilities"
            | "non_responsibilities"
            | "features"
            | "non_features"
            | "invariants"
            | "constraints"
            | "design_principles" => DocFieldKind::List,
            "usage" | "design_points" => DocFieldKind::ParagraphList,
            _ => bail!("TechnicalClass has no field {field}"),
        },
        Object::TechnicalStruct(_) => match field {
            "position" => DocFieldKind::Text,
            "invariants" => DocFieldKind::List,
            _ => bail!("TechnicalStruct has no field {field}"),
        },
        Object::TechnicalEnum(_) | Object::TechnicalEnumValue(_) => match field {
            "position" => DocFieldKind::Text,
            _ => bail!("{} has no field {field}", object.kind().name()),
        },
        Object::TechnicalInterface(_) | Object::TechnicalMixin(_) => match field {
            "position" => DocFieldKind::Text,
            "responsibilities"
            | "non_responsibilities"
            | "features"
            | "non_features"
            | "invariants" => DocFieldKind::List,
            "usage" | "design_points" => DocFieldKind::ParagraphList,
            _ => bail!("{} has no field {field}", object.kind().name()),
        },
        Object::TechnicalMethod(_) => match field {
            "position" => DocFieldKind::Text,
            "usage" | "design_points" => DocFieldKind::ParagraphList,
            _ => bail!("{} has no field {field}", object.kind().name()),
        },
        Object::TechnicalFunction(_) => match field {
            "position" => DocFieldKind::Text,
            "usage" | "design_points" => DocFieldKind::ParagraphList,
            _ => bail!("{} has no field {field}", object.kind().name()),
        },
        Object::TechnicalPart(_) => bail!("TechnicalPart has no document fields"),
        Object::RequirementUser(_) => match field {
            "situations" | "hobbies" | "needs" => DocFieldKind::List,
            _ => bail!("RequirementUser has no field {field}"),
        },
        Object::RequirementStory(_) => match field {
            "preconditions" | "requirement" => DocFieldKind::List,
            "description" => DocFieldKind::ParagraphList,
            "flow" => DocFieldKind::Mermaid,
            _ => bail!("RequirementStory has no field {field}"),
        },
        Object::RequirementSystem(_) => match field {
            "position" | "app_form" => DocFieldKind::Text,
            _ => bail!("RequirementSystem has no field {field}"),
        },
        Object::RequirementModule(_) => match field {
            "position" => DocFieldKind::Text,
            _ => bail!("RequirementModule has no field {field}"),
        },
        Object::RequirementFunction(_) => match field {
            "initiator" | "feedback" => DocFieldKind::Text,
            "system_change" | "preconditions" | "description" => DocFieldKind::ParagraphList,
            "flow" => DocFieldKind::Mermaid,
            _ => bail!("RequirementFunction has no field {field}"),
        },
        Object::RequirementInteraction(_) => match field {
            "description" | "ui" => DocFieldKind::ParagraphList,
            _ => bail!("RequirementInteraction has no field {field}"),
        },
        Object::Concept(_) => match field {
            "explanation" => DocFieldKind::ParagraphList,
            _ => bail!("Concept has no field {field}"),
        },
        Object::ProjectDocument(_) => match field {
            "document" => DocFieldKind::ParagraphList,
            _ => bail!("ProjectDocument has no field {field}"),
        },
        _ => bail!("{} has no document fields", object.kind().name()),
    };
    Ok(kind)
}

fn set_doc_field(object: &mut Object, field: &str, value: ObjectHash) -> Result<()> {
    match object {
        Object::TechnicalSystem(object) => match field {
            "position" => object.position = value,
            "responsibilities" => object.responsibilities = value,
            "non_responsibilities" => object.non_responsibilities = value,
            "features" => object.features = value,
            "non_features" => object.non_features = value,
            "invariants" => object.invariants = value,
            "constraints" => object.constraints = value,
            "design_principles" => object.design_principles = value,
            "external_dependencies" => object.external_dependencies = value,
            "usage" => object.usage = value,
            "design_points" => object.design_points = value,
            _ => bail!("TechnicalSystem has no field {field}"),
        },
        Object::TechnicalDirectory(object) => match field {
            "position" => object.position = value,
            _ => bail!("TechnicalDirectory has no field {field}"),
        },
        Object::TechnicalFile(object) => match field {
            "position" => object.position = value,
            _ => bail!("TechnicalFile has no field {field}"),
        },
        Object::TechnicalLayer(object) => match field {
            "position" => object.position = value,
            _ => bail!("TechnicalLayer has no field {field}"),
        },
        Object::TechnicalNamespace(object) => match field {
            "position" => object.position = value,
            _ => bail!("TechnicalNamespace has no field {field}"),
        },
        Object::TechnicalClass(object) => match field {
            "position" => object.position = value,
            "responsibilities" => object.responsibilities = value,
            "non_responsibilities" => object.non_responsibilities = value,
            "features" => object.features = value,
            "non_features" => object.non_features = value,
            "invariants" => object.invariants = value,
            "constraints" => object.constraints = value,
            "design_principles" => object.design_principles = value,
            "usage" => object.usage = value,
            "design_points" => object.design_points = value,
            _ => bail!("TechnicalClass has no field {field}"),
        },
        Object::TechnicalStruct(object) => match field {
            "position" => object.position = value,
            "invariants" => object.invariants = value,
            _ => bail!("TechnicalStruct has no field {field}"),
        },
        Object::TechnicalEnum(object) => match field {
            "position" => object.position = value,
            _ => bail!("TechnicalEnum has no field {field}"),
        },
        Object::TechnicalInterface(object) => match field {
            "position" => object.position = value,
            "responsibilities" => object.responsibilities = value,
            "non_responsibilities" => object.non_responsibilities = value,
            "features" => object.features = value,
            "non_features" => object.non_features = value,
            "invariants" => object.invariants = value,
            "usage" => object.usage = value,
            "design_points" => object.design_points = value,
            _ => bail!("TechnicalInterface has no field {field}"),
        },
        Object::TechnicalMixin(object) => match field {
            "position" => object.position = value,
            "responsibilities" => object.responsibilities = value,
            "non_responsibilities" => object.non_responsibilities = value,
            "features" => object.features = value,
            "non_features" => object.non_features = value,
            "invariants" => object.invariants = value,
            "usage" => object.usage = value,
            "design_points" => object.design_points = value,
            _ => bail!("TechnicalMixin has no field {field}"),
        },
        Object::TechnicalMethod(object) => match field {
            "position" => object.position = value,
            "usage" => object.usage = value,
            "design_points" => object.design_points = value,
            _ => bail!("TechnicalMethod has no field {field}"),
        },
        Object::TechnicalFunction(object) => match field {
            "position" => object.position = value,
            "usage" => object.usage = value,
            "design_points" => object.design_points = value,
            _ => bail!("TechnicalFunction has no field {field}"),
        },
        Object::TechnicalPart(_) => bail!("TechnicalPart has no document fields"),
        Object::TechnicalModule(object) => match field {
            "position" => object.position = value,
            "responsibilities" => object.responsibilities = value,
            "non_responsibilities" => object.non_responsibilities = value,
            "features" => object.features = value,
            "non_features" => object.non_features = value,
            "invariants" => object.invariants = value,
            "constraints" => object.constraints = value,
            "design_principles" => object.design_principles = value,
            "usage" => object.usage = value,
            "design_points" => object.design_points = value,
            _ => bail!("TechnicalModule has no field {field}"),
        },
        Object::RequirementUser(object) => match field {
            "situations" => object.situations = value,
            "hobbies" => object.hobbies = value,
            "needs" => object.needs = value,
            _ => bail!("RequirementUser has no field {field}"),
        },
        Object::RequirementStory(object) => match field {
            "preconditions" => object.preconditions = value,
            "description" => object.description = value,
            "requirement" => object.requirement = value,
            "flow" => object.flow = value,
            _ => bail!("RequirementStory has no field {field}"),
        },
        Object::RequirementSystem(object) => match field {
            "position" => object.position = value,
            "app_form" => object.app_form = value,
            _ => bail!("RequirementSystem has no field {field}"),
        },
        Object::RequirementModule(object) => match field {
            "position" => object.position = value,
            _ => bail!("RequirementModule has no field {field}"),
        },
        Object::RequirementFunction(object) => match field {
            "flow" => object.flow = value,
            "initiator" => object.initiator = value,
            "system_change" => object.system_change = value,
            "preconditions" => object.preconditions = value,
            "feedback" => object.feedback = value,
            "description" => object.description = value,
            _ => bail!("RequirementFunction has no field {field}"),
        },
        Object::RequirementInteraction(object) => match field {
            "description" => object.description = value,
            "ui" => object.ui = value,
            _ => bail!("RequirementInteraction has no field {field}"),
        },
        Object::Concept(object) => match field {
            "explanation" => object.explanation = value,
            _ => bail!("Concept has no field {field}"),
        },
        Object::ProjectDocument(object) => match field {
            "document" => object.document = value,
            _ => bail!("ProjectDocument has no field {field}"),
        },
        other => bail!("{} has no document fields", other.kind().name()),
    }
    Ok(())
}

pub fn doc_fields(object: &Object) -> Vec<(&'static str, ObjectHash)> {
    match object {
        Object::TechnicalSystem(value) => vec![
            ("position", value.position),
            ("responsibilities", value.responsibilities),
            ("non_responsibilities", value.non_responsibilities),
            ("features", value.features),
            ("non_features", value.non_features),
            ("invariants", value.invariants),
            ("constraints", value.constraints),
            ("design_principles", value.design_principles),
            ("external_dependencies", value.external_dependencies),
            ("usage", value.usage),
            ("design_points", value.design_points),
        ],
        Object::TechnicalDirectory(value) => vec![("position", value.position)],
        Object::TechnicalFile(value) => vec![("position", value.position)],
        Object::TechnicalLayer(value) => vec![("position", value.position)],
        Object::TechnicalNamespace(value) => vec![("position", value.position)],
        Object::TechnicalClass(value) => vec![
            ("position", value.position),
            ("responsibilities", value.responsibilities),
            ("non_responsibilities", value.non_responsibilities),
            ("features", value.features),
            ("non_features", value.non_features),
            ("invariants", value.invariants),
            ("constraints", value.constraints),
            ("design_principles", value.design_principles),
            ("usage", value.usage),
            ("design_points", value.design_points),
        ],
        Object::TechnicalStruct(value) => vec![
            ("position", value.position),
            ("invariants", value.invariants),
        ],
        Object::TechnicalEnum(value) => vec![("position", value.position)],
        Object::TechnicalInterface(value) => vec![
            ("position", value.position),
            ("responsibilities", value.responsibilities),
            ("non_responsibilities", value.non_responsibilities),
            ("features", value.features),
            ("non_features", value.non_features),
            ("invariants", value.invariants),
            ("usage", value.usage),
            ("design_points", value.design_points),
        ],
        Object::TechnicalMixin(value) => vec![
            ("position", value.position),
            ("responsibilities", value.responsibilities),
            ("non_responsibilities", value.non_responsibilities),
            ("features", value.features),
            ("non_features", value.non_features),
            ("invariants", value.invariants),
            ("usage", value.usage),
            ("design_points", value.design_points),
        ],
        Object::TechnicalMethod(value) => vec![
            ("position", value.position),
            ("usage", value.usage),
            ("design_points", value.design_points),
        ],
        Object::TechnicalFunction(value) => vec![
            ("position", value.position),
            ("usage", value.usage),
            ("design_points", value.design_points),
        ],
        Object::TechnicalModule(value) => vec![
            ("position", value.position),
            ("responsibilities", value.responsibilities),
            ("non_responsibilities", value.non_responsibilities),
            ("features", value.features),
            ("non_features", value.non_features),
            ("invariants", value.invariants),
            ("constraints", value.constraints),
            ("design_principles", value.design_principles),
            ("usage", value.usage),
            ("design_points", value.design_points),
        ],
        Object::RequirementUser(value) => vec![
            ("situations", value.situations),
            ("hobbies", value.hobbies),
            ("needs", value.needs),
        ],
        Object::RequirementStory(value) => vec![
            ("preconditions", value.preconditions),
            ("description", value.description),
            ("requirement", value.requirement),
            ("flow", value.flow),
        ],
        Object::RequirementSystem(value) => {
            vec![("position", value.position), ("app_form", value.app_form)]
        }
        Object::RequirementModule(value) => vec![("position", value.position)],
        Object::RequirementFunction(value) => vec![
            ("flow", value.flow),
            ("initiator", value.initiator),
            ("system_change", value.system_change),
            ("preconditions", value.preconditions),
            ("feedback", value.feedback),
            ("description", value.description),
        ],
        Object::RequirementInteraction(value) => {
            vec![("description", value.description), ("ui", value.ui)]
        }
        Object::Concept(value) => vec![("explanation", value.explanation)],
        Object::ProjectDocument(value) => vec![("document", value.document)],
        _ => Vec::new(),
    }
}

fn expected_doc_kind(kind: DocFieldKind) -> crate::object::ObjectKind {
    match kind {
        DocFieldKind::Text => crate::object::ObjectKind::DocumentText,
        DocFieldKind::List => crate::object::ObjectKind::DocumentList,
        DocFieldKind::ParagraphList => crate::object::ObjectKind::DocumentParagraphList,
        DocFieldKind::Mermaid => crate::object::ObjectKind::DocumentMermaid,
    }
}

impl Repository {
    pub fn init(path: impl AsRef<Path>) -> Result<Self> {
        Self::init_with_source(path, None)
    }

    pub fn init_with_source(path: impl AsRef<Path>, source_url: Option<&str>) -> Result<Self> {
        let root = path.as_ref().to_path_buf();
        let dir = root.join(ARCHE_DIR);
        if dir.exists() {
            bail!("{} already exists", dir.display());
        }
        fs::create_dir_all(&root)
            .with_context(|| format!("create repository root {}", root.display()))?;
        let tmp = dir.join("tmp");
        for child in ["objects", "symbols", "commits", "refs/heads", "tmp"] {
            fs::create_dir_all(dir.join(child))
                .with_context(|| format!("create {}", dir.join(child).display()))?;
        }
        let repo = Self {
            symbols: SymbolStore::new(dir.join("symbols"), tmp.clone()),
            objects: LooseObjectStore::new(dir.join("objects"), tmp.clone()),
            commits: CommitStore::new(dir.join("commits"), tmp.clone()),
            comments: CommentStore::new(&dir, tmp.clone()),
            root,
            dir,
            tmp,
            source_url: source_url.map(str::to_owned),
            remote_url: None,
        };
        repo.bootstrap()?;
        Ok(repo)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let root = path.as_ref().to_path_buf();
        let dir = root.join(ARCHE_DIR);
        if !dir.is_dir() {
            bail!("{} is not an Arche repository", root.display());
        }
        let tmp = dir.join("tmp");
        let config = fs::read_to_string(dir.join("config")).unwrap_or_default();
        let source_url = config
            .lines()
            .find_map(|line| line.strip_prefix("source=").map(str::to_owned));
        let remote_url = config
            .lines()
            .find_map(|line| line.strip_prefix("remote=").map(str::to_owned));
        let repo = Self {
            symbols: SymbolStore::new(dir.join("symbols"), tmp.clone()),
            objects: LooseObjectStore::new(dir.join("objects"), tmp.clone()),
            commits: CommitStore::new(dir.join("commits"), tmp.clone()),
            comments: CommentStore::new(&dir, tmp.clone()),
            root,
            dir,
            tmp,
            source_url,
            remote_url,
        };
        repo.recover_ref_transaction()?;
        Ok(repo)
    }

    pub fn source_url(&self) -> Option<&str> {
        self.source_url.as_deref()
    }

    /// Record the read-only source Git URL in `.arche/config`.
    pub fn set_source_url(&self, url: &str) -> Result<()> {
        let url = url.trim();
        if url.is_empty() {
            bail!("source URL must not be empty");
        }
        let mut config = String::from("format = 2\n");
        if let Some(remote) = self.remote_url() {
            config.push_str(&format!("remote={remote}\n"));
        }
        config.push_str(&format!("source={url}\n"));
        write_atomic(&self.dir.join("config"), config.as_bytes(), &self.tmp)
    }

    pub fn remote_url(&self) -> Option<&str> {
        self.remote_url.as_deref()
    }

    pub fn set_remote(&self, remote: &str) -> Result<()> {
        let path = self.dir.join("config");
        let text = fs::read_to_string(&path).unwrap_or_default();
        let mut lines = text
            .lines()
            .filter(|line| !line.starts_with("remote="))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        lines.push(format!("remote={remote}"));
        fs::write(path, format!("{}\n", lines.join("\n")))?;
        Ok(())
    }

    /// Point a ref at a commit. Used by the remote client.
    pub fn set_ref(&self, name: &str, hash: CommitHash) -> Result<()> {
        write_ref_file(&self.dir, name, hash)
    }

    /// Make `branch` the current branch without changing its commit.
    pub fn set_head(&self, branch: &str) -> Result<()> {
        fs::write(self.dir.join("HEAD"), format!("ref: refs/heads/{branch}\n"))?;
        Ok(())
    }

    /// Read a commit by hash. Used by the remote client.
    pub fn read_commit(&self, hash: CommitHash) -> Result<Commit> {
        self.commits.read(hash)
    }

    /// Replace index and worktree with the snapshot recorded by `hash`.
    pub fn adopt_commit(&self, hash: CommitHash) -> Result<()> {
        let commit = self.commits.read(hash)?;
        let snapshot = Snapshot::from_entries(commit.objects);
        self.write_snapshot("index", &snapshot)?;
        self.write_snapshot("worktree", &snapshot)?;
        Ok(())
    }

    /// Branches fetched from the configured remote.
    pub fn remote_branches(&self) -> Result<Vec<(String, CommitHash)>> {
        refs_under(&self.dir, "refs/remotes/origin")
    }

    /// Atomically write one record file. Used by client fetch and the server.
    pub fn write_record(&self, relative: &str, bytes: &[u8]) -> Result<()> {
        write_atomic(&self.dir.join(relative), bytes, &self.tmp)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn intern(&self, text: &str) -> Result<SymbolHash> {
        self.symbols.intern(text)
    }

    pub fn resolve(&self, hash: impl Into<SymbolHash>) -> Result<String> {
        self.symbols.resolve(hash.into())
    }

    pub fn read(&self, hash: ObjectHash) -> Result<Object> {
        Object::load(self, hash)
    }

    fn save_object(&self, object: &Object) -> Result<ObjectHash> {
        let mut object = object.clone();
        object.save(self)
    }

    pub fn list_objects(&self) -> Result<Vec<(ObjectHash, Object)>> {
        let mut result = Vec::new();
        for (_, hash) in self.worktree_snapshot()?.entries() {
            result.push((*hash, self.read(*hash)?));
        }
        Ok(result)
    }

    pub fn find_by_qname(&self, qname: &str) -> Result<ObjectHash> {
        let target = SymbolHash::of(qname);
        self.worktree_snapshot()?
            .get(target)
            .ok_or_else(|| anyhow::anyhow!("object {qname} not found"))
    }

    pub fn head_snapshot(&self) -> Result<Snapshot> {
        Ok(Snapshot::from_entries(self.head()?.objects))
    }

    pub fn index_snapshot(&self) -> Result<Snapshot> {
        self.read_snapshot("index")
    }

    pub fn worktree_snapshot(&self) -> Result<Snapshot> {
        self.read_snapshot("worktree")
    }

    fn read_snapshot(&self, name: &str) -> Result<Snapshot> {
        let path = self.dir.join(name);
        if !path.exists() {
            return Ok(Snapshot::default());
        }
        let bytes = fs::read(&path).with_context(|| format!("read snapshot {}", path.display()))?;
        Snapshot::decode(&bytes).with_context(|| format!("decode snapshot {}", path.display()))
    }

    fn write_snapshot(&self, name: &str, snapshot: &Snapshot) -> Result<()> {
        let snapshot = if name == "worktree" {
            self.normalize_snapshot(snapshot)?
        } else {
            snapshot.clone()
        };
        let path = self.dir.join(name);
        write_atomic(&path, &snapshot.encode(), &self.tmp)
    }

    fn normalize_snapshot(&self, snapshot: &Snapshot) -> Result<Snapshot> {
        let mut result = snapshot.clone();
        let mut replaced = HashMap::new();
        for _ in 0..64 {
            let mut changed = false;
            for (qname, hash) in result.entries().to_vec() {
                let Ok(mut object) = self.read(hash) else {
                    continue;
                };
                let mut object_changed = false;
                object.for_each_reference_mut(|value| {
                    if let Some(current) = replaced.get(value) {
                        *value = *current;
                        object_changed = true;
                        return;
                    }
                    let Ok(target) = self.read(*value) else {
                        return;
                    };
                    if let Some(current) = result.get(target.qname())
                        && current != *value
                    {
                        *value = current;
                        object_changed = true;
                    }
                });
                if object_changed {
                    let new_hash = self.save_object(&object)?;
                    result.insert(qname, new_hash);
                    replaced.insert(hash, new_hash);
                    changed = true;
                }
            }
            if !changed {
                return Ok(result);
            }
        }
        bail!("worktree references did not converge")
    }

    pub fn current_branch(&self) -> Result<String> {
        let text = fs::read_to_string(self.dir.join("HEAD")).context("read HEAD")?;
        let text = text.trim();
        text.strip_prefix("ref: refs/heads/")
            .map(str::to_owned)
            .with_context(|| format!("unsupported HEAD {text}"))
    }

    pub fn head_commit(&self) -> Result<CommitHash> {
        let branch = self.current_branch()?;
        let path = self.dir.join("refs/heads").join(branch);
        let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        CommitHash::from_hex(text.trim())
    }

    pub fn branch_list(&self) -> Result<Vec<String>> {
        Ok(refs_under(&self.dir, "refs/heads")?
            .into_iter()
            .map(|(branch, _)| branch)
            .collect())
    }

    pub fn incoming_path(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty() || id.contains('/') || id.contains("..") {
            bail!("invalid push id");
        }
        Ok(self.dir.join("incoming").join(id))
    }

    pub fn publish_incoming(&self, id: &str) -> Result<usize> {
        let incoming = self.incoming_path(id)?;
        if !incoming.is_dir() {
            return Ok(0);
        }
        let mut count = 0;
        for path in list_files(&incoming)? {
            let relative = path
                .strip_prefix(&incoming)?
                .to_string_lossy()
                .replace('\\', "/");
            if !valid_record_path(&relative) {
                bail!("invalid incoming record {relative}");
            }
            let target = self.dir.join(&relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&path, &target)?;
            count += 1;
        }
        let _ = fs::remove_dir_all(&incoming);
        Ok(count)
    }

    pub fn discard_incoming(&self, id: &str) -> Result<()> {
        let incoming = self.incoming_path(id)?;
        if incoming.exists() {
            fs::remove_dir_all(incoming)?;
        }
        Ok(())
    }

    pub fn repack(&self) -> Result<usize> {
        let _lock = self.lock_refs()?;
        let mut records = Vec::new();
        let mut loose = Vec::new();
        for path in list_files(&self.dir.join("objects"))? {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Ok(hash) = ObjectHash::from_hex(name) else {
                continue;
            };
            let bytes = fs::read(&path)?;
            if bytes.starts_with(b"DSO2") {
                records.push(PackRecord {
                    kind: PackKind::Object,
                    hash: hash.0,
                    payload: bytes[4..].to_vec(),
                });
                loose.push(path);
            }
        }
        for path in list_files(&self.dir.join("symbols"))? {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Ok(hash) = SymbolHash::from_hex(name) else {
                continue;
            };
            records.push(PackRecord {
                kind: PackKind::Symbol,
                hash: hash.0,
                payload: fs::read(&path)?,
            });
            loose.push(path);
        }
        for path in list_files(&self.dir.join("commits"))? {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Ok(hash) = CommitHash::from_hex(name) else {
                continue;
            };
            records.push(PackRecord {
                kind: PackKind::Commit,
                hash: hash.0,
                payload: fs::read(&path)?,
            });
            loose.push(path);
        }
        if records.is_empty() {
            return Ok(0);
        }
        let bytes = crate::pack::encode(&records);
        let pack = Hash32::sha256(&bytes);
        let path = self
            .dir
            .join("packs")
            .join(format!("{}.pack", pack.to_hex()));
        fs::create_dir_all(self.dir.join("packs"))?;
        write_atomic(&path, &bytes, &self.tmp)?;
        for path in loose {
            fs::remove_file(path)?;
        }
        Ok(records.len())
    }

    pub fn gc(&self) -> Result<usize> {
        let mut reachable = HashSet::new();
        for snapshot in [
            self.head_snapshot()?,
            self.index_snapshot()?,
            self.worktree_snapshot()?,
        ] {
            for (_, hash) in snapshot.entries() {
                reachable.insert(*hash);
            }
        }
        for path in list_files(&self.dir.join("commits"))? {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Ok(hash) = CommitHash::from_hex(name) else {
                continue;
            };
            let commit = self.commits.read(hash)?;
            reachable.insert(commit.root);
            for (_, object) in commit.objects {
                reachable.insert(object);
            }
        }
        let mut queue = reachable.iter().copied().collect::<Vec<_>>();
        while let Some(hash) = queue.pop() {
            let Ok(object) = self.read(hash) else {
                continue;
            };
            for reference in object.references() {
                if reachable.insert(reference) {
                    queue.push(reference);
                }
            }
        }
        let mut removed = 0;
        for path in list_files(&self.dir.join("objects"))? {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Ok(hash) = ObjectHash::from_hex(name) else {
                continue;
            };
            if !reachable.contains(&hash) {
                fs::remove_file(path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn ref_hash(&self, name: &str) -> Result<Option<CommitHash>> {
        let path = self.dir.join(name);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(read_ref_file(&self.dir, name)?))
    }

    pub fn is_ancestor(&self, ancestor: CommitHash, descendant: CommitHash) -> Result<bool> {
        let mut current = descendant;
        for _ in 0..100_000 {
            if current == ancestor {
                return Ok(true);
            }
            if current.is_zero() {
                return Ok(false);
            }
            let commit = self.read_commit(current)?;
            current = commit.parent.unwrap_or(CommitHash::ZERO);
        }
        bail!("commit ancestry is too deep")
    }

    pub fn update_ref_checked(
        &self,
        name: &str,
        expected: Option<CommitHash>,
        new: CommitHash,
        force: bool,
    ) -> Result<()> {
        let _lock = self.lock_refs()?;
        let current = self.ref_hash(name)?;
        if current != expected {
            bail!(
                "ref {name} changed: expected {}, found {}",
                expected.map_or_else(|| "none".to_owned(), |hash| hash.to_hex()),
                current.map_or_else(|| "none".to_owned(), |hash| hash.to_hex())
            );
        }
        if !force
            && let Some(old) = current
            && !self.is_ancestor(old, new)?
        {
            bail!("ref {name} update is not a fast-forward");
        }
        write_ref_file(&self.dir, name, new)
    }

    pub fn update_refs_transaction(
        &self,
        updates: &[(String, Option<CommitHash>, CommitHash, bool)],
    ) -> Result<()> {
        let _lock = self.lock_refs()?;
        for (name, expected, new, force) in updates {
            let current = self.ref_hash(name)?;
            if current != *expected {
                bail!("ref {name} changed");
            }
            if !force
                && let Some(old) = current
                && !self.is_ancestor(old, *new)?
            {
                bail!("ref {name} update is not a fast-forward");
            }
        }
        let journal = self.dir.join("refs.transaction");
        let mut text = String::new();
        for (name, _, new, _) in updates {
            text.push_str(&format!("{name}\t{}\n", new.to_hex()));
        }
        fs::write(&journal, text)?;
        for (name, _, new, _) in updates {
            write_ref_file(&self.dir, name, *new)?;
        }
        fs::remove_file(journal)?;
        Ok(())
    }

    pub fn recover_ref_transaction(&self) -> Result<()> {
        let journal = self.dir.join("refs.transaction");
        if !journal.exists() {
            return Ok(());
        }
        let text = fs::read_to_string(&journal)?;
        let _lock = self.lock_refs()?;
        for line in text.lines() {
            let Some((name, hash)) = line.split_once('\t') else {
                continue;
            };
            if !name.starts_with("refs/heads/") {
                bail!("invalid ref transaction path {name}");
            }
            write_ref_file(&self.dir, name, CommitHash::from_hex(hash)?)?;
        }
        fs::remove_file(journal)?;
        Ok(())
    }

    fn lock_refs(&self) -> Result<RefLock> {
        let path = self.dir.join("refs.lock");
        for _ in 0..200 {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok(RefLock { path, _file: file });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error.into()),
            }
        }
        bail!("refs are locked")
    }

    pub fn branch_create(&self, name: &str) -> Result<()> {
        validate_branch_name(name)?;
        let path = self.dir.join("refs/heads").join(name);
        if path.exists() {
            bail!("branch {name} already exists");
        }
        write_ref_file(
            &self.dir,
            &format!("refs/heads/{name}"),
            self.head_commit()?,
        )?;
        Ok(())
    }

    pub fn branch_delete(&self, name: &str) -> Result<()> {
        if self.current_branch()? == name {
            bail!("cannot delete the current branch");
        }
        let path = self.dir.join("refs/heads").join(name);
        if !path.exists() {
            // Deleting an already-removed branch is idempotent: merge cleanup
            // may run after the merge itself removed the source branch.
            return Ok(());
        }
        fs::remove_file(path)?;
        Ok(())
    }

    pub fn checkout(&self, branch: &str, create: bool) -> Result<()> {
        validate_branch_name(branch)?;
        let path = self.dir.join("refs/heads").join(branch);
        if create {
            if path.exists() {
                bail!("branch {branch} already exists");
            }
            write_ref_file(
                &self.dir,
                &format!("refs/heads/{branch}"),
                self.head_commit()?,
            )?;
            fs::write(self.dir.join("HEAD"), format!("ref: refs/heads/{branch}\n"))?;
            return Ok(());
        }
        if !path.exists() {
            let remote = self.ref_hash(&format!("refs/remotes/origin/{branch}"))?;
            let Some(remote) = remote else {
                bail!("branch {branch} does not exist");
            };
            write_ref_file(&self.dir, &format!("refs/heads/{branch}"), remote)?;
        }
        let head = self.head_snapshot()?;
        if self.index_snapshot()? != head || self.worktree_snapshot()? != head {
            bail!("index or worktree has local changes; commit or reset first");
        }
        let target = read_ref_file(&self.dir, &format!("refs/heads/{branch}"))?;
        let commit = self.commits.read(target)?;
        fs::write(self.dir.join("HEAD"), format!("ref: refs/heads/{branch}\n"))?;
        let snapshot = Snapshot::from_entries(commit.objects);
        self.write_snapshot("index", &snapshot)?;
        self.write_snapshot("worktree", &snapshot)?;
        Ok(())
    }

    pub fn head(&self) -> Result<Commit> {
        self.commits.read(self.head_commit()?)
    }

    pub fn log(&self) -> Result<Vec<(CommitHash, Commit)>> {
        let mut result = Vec::new();
        let mut current = Some(self.head_commit()?);
        while let Some(hash) = current {
            let commit = self.commits.read(hash)?;
            current = commit.parent;
            result.push((hash, commit));
        }
        Ok(result)
    }

    pub fn source_fetch(&self) -> Result<()> {
        let url = self
            .source_url
            .as_deref()
            .context("repository has no source URL")?;
        let cache = self.source_cache();
        if !source_cache_ready(&cache) {
            if cache.exists() {
                fs::remove_dir_all(&cache)?;
            }
            if let Some(parent) = cache.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut command = Command::new("git");
            command.args(["clone", "--bare", "--quiet", url]);
            command.arg(&cache);
            run_git(command, "clone source")?;
            if !source_cache_ready(&cache) {
                let _ = fs::remove_dir_all(&cache);
                bail!("source clone did not produce a valid HEAD");
            }
        } else {
            let mut command = Command::new("git");
            command.arg("-C").arg(&cache).args([
                "fetch",
                "--prune",
                "--tags",
                "origin",
                "+refs/heads/*:refs/remotes/origin/*",
            ]);
            run_git(command, "fetch source")?;
        }
        self.source_worktree()?;
        Ok(())
    }

    pub fn source_worktree(&self) -> Result<PathBuf> {
        let root_git = self.root.join(".git");
        if root_git.exists() {
            let text = fs::read_to_string(&root_git)
                .with_context(|| format!("read {}", root_git.display()))?;
            if !text.contains(".arche/source/worktrees/root") {
                bail!("workspace root has a different .git pointer");
            }
            self.ensure_source_exclude()?;
            return Ok(self.root.clone());
        }
        let cache = self.source_cache();
        if !cache.join("HEAD").is_file() {
            bail!("source repository is missing; run `arche source fetch`");
        }
        let commit = self.source_git_text(&cache, ["rev-parse", "HEAD"])?;
        let commit = commit.trim();
        let parent = self
            .root
            .parent()
            .context("workspace root has no parent")?
            .to_path_buf();
        let temp = parent.join(format!(
            ".arche-worktree-{}-{}",
            std::process::id(),
            WORKTREE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        if temp.exists() {
            fs::remove_dir_all(&temp)?;
        }
        let mut command = Command::new("git");
        command
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "submodule.recurse=false",
            ])
            .arg("--git-dir")
            .arg(&cache)
            .args(["worktree", "add", "--detach", "--force"])
            .arg(&temp)
            .arg(commit);
        if let Err(error) = run_git(command, "create source worktree") {
            let _ = fs::remove_dir_all(&temp);
            return Err(error);
        }
        if temp.join(".arche").exists() {
            let _ = fs::remove_dir_all(&temp);
            bail!("source repository contains a .arche directory");
        }
        let gitfile = fs::read_to_string(temp.join(".git"))?;
        let admin = gitfile
            .trim()
            .strip_prefix("gitdir: ")
            .map(PathBuf::from)
            .context("source worktree has no gitdir pointer")?;
        for entry in fs::read_dir(&temp)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            let target = self.root.join(entry.file_name());
            if target.exists() {
                let _ = fs::remove_dir_all(&temp);
                bail!(
                    "source file collides with workspace data: {}",
                    target.display()
                );
            }
            fs::rename(entry.path(), target)?;
        }
        let worktrees = cache.join("worktrees");
        let final_admin = worktrees.join("root");
        if final_admin.exists() {
            fs::remove_dir_all(&final_admin)?;
        }
        fs::create_dir_all(&worktrees)?;
        fs::rename(&admin, &final_admin)?;
        fs::write(root_git, "gitdir: .arche/source/worktrees/root\n")?;
        fs::write(final_admin.join("gitdir"), "../../../.git\n")?;
        let _ = fs::remove_dir_all(&temp);
        self.ensure_source_exclude()?;
        Ok(self.root.clone())
    }

    pub fn source_checkout(&self, git_ref: &str) -> Result<()> {
        if git_ref.trim().is_empty() || git_ref.starts_with('-') || git_ref.contains('\0') {
            bail!("invalid git ref");
        }
        let worktree = self.source_worktree()?;
        let dirty = self.source_git_text(&worktree, ["status", "--porcelain"])?;
        if !dirty.trim().is_empty() {
            bail!("source worktree has uncommitted changes");
        }
        let ref_text = git_ref.trim();
        let commit = self
            .source_git_text(
                &self.source_cache(),
                [
                    "rev-parse",
                    "--verify",
                    &format!("refs/remotes/origin/{ref_text}^{{commit}}"),
                ],
            )
            .or_else(|_| {
                self.source_git_text(
                    &self.source_cache(),
                    ["rev-parse", "--verify", &format!("{ref_text}^{{commit}}")],
                )
            })?;
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&worktree)
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "submodule.recurse=false",
            ])
            .args(["checkout", "--force", "--detach", commit.trim()]);
        run_git(command, "checkout source")?;
        Ok(())
    }

    pub fn source_status(&self) -> Result<String> {
        if !self.root.join(".git").is_file() {
            return Ok("no source worktree\n".to_owned());
        }
        self.source_git_text(&self.root, ["status", "--porcelain"])
    }

    fn sync_directories(
        &self,
        snapshot: &mut Snapshot,
        dir_paths: &BTreeSet<String>,
    ) -> Result<usize> {
        let technical = self.intern("technical")?;
        let root_list_qname = self.intern("technical/directory")?;
        let mut positions = HashMap::new();
        let prefix = "technical/directory/";
        for (qname, hash) in snapshot.entries().to_vec() {
            let text = self.resolve(qname)?;
            if !text.starts_with(prefix) {
                continue;
            }
            if let Object::TechnicalDirectory(value) = self.read(hash)? {
                positions.insert(text[prefix.len()..].to_owned(), value.position);
            }
            snapshot.remove(qname);
        }

        let mut dir_hashes = HashMap::new();
        for path in dir_paths {
            let qname = self.intern(&format!("technical/directory/{path}"))?;
            let parent = match parent_path(path) {
                Some(parent) => self.intern(&format!("technical/directory/{parent}"))?,
                None => technical,
            };
            let position = positions.get(path).copied().unwrap_or(ObjectHash::ZERO);
            let object = Object::TechnicalDirectory(TechnicalDirectory {
                qname,
                name: self.intern(path.rsplit('/').next().unwrap_or(path))?,
                parent,
                source: SourcePath {
                    path: self.intern(path)?,
                },
                children: ObjectHash::ZERO,
                position,
            });
            let hash = self.save_object(&object)?;
            snapshot.insert(qname, hash);
            dir_hashes.insert(path.clone(), (qname, hash));
        }

        for path in dir_paths {
            let children = dir_paths
                .iter()
                .filter(|child| parent_path(child) == Some(path.as_str()))
                .filter_map(|child| dir_hashes.get(child).map(|(_, hash)| *hash))
                .collect::<Vec<_>>();
            if children.is_empty() {
                continue;
            }
            let (dir_qname, dir_hash) = dir_hashes[path];
            let list_qname = self.intern(&format!("{}/children", self.resolve(dir_qname)?))?;
            let list = Object::ObjectList(ObjectList {
                qname: list_qname,
                name: self.intern("children")?,
                parent: dir_qname,
                items: children,
            });
            let list_hash = self.save_object(&list)?;
            snapshot.insert(list_qname, list_hash);
            let mut directory = self.read(dir_hash)?;
            if let Object::TechnicalDirectory(value) = &mut directory {
                value.children = list_hash;
            }
            let hash = self.save_object(&directory)?;
            snapshot.insert(dir_qname, hash);
            dir_hashes.insert(path.clone(), (dir_qname, hash));
        }

        let roots = dir_paths
            .iter()
            .filter(|path| parent_path(path).is_none())
            .filter_map(|path| dir_hashes.get(path).map(|(_, hash)| *hash))
            .collect::<Vec<_>>();
        let root_list = Object::ObjectList(ObjectList {
            qname: root_list_qname,
            name: self.intern("directory")?,
            parent: technical,
            items: roots,
        });
        let root_list_hash = self.save_object(&root_list)?;
        snapshot.insert(root_list_qname, root_list_hash);
        if let Some(root_hash) = snapshot.get(technical) {
            let mut root = self.read(root_hash)?;
            if let Object::TechnicalRoot(value) = &mut root {
                value.directory = root_list_hash;
            }
            let hash = self.save_object(&root)?;
            snapshot.insert(technical, hash);
        }
        Ok(dir_hashes.len())
    }

    /// Tracked files in the current source checkout.
    pub fn source_ls(&self) -> Result<Vec<String>> {
        let worktree = self.source_worktree()?;
        let output = Command::new("git")
            .arg("-C")
            .arg(&worktree)
            .args(["ls-files", "-z"])
            .output()
            .context("list source files")?;
        if !output.status.success() {
            bail!(
                "git ls-files failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let mut files = String::from_utf8(output.stdout)?
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        files.sort();
        Ok(files)
    }

    /// Read one tracked file from the current source checkout.
    pub fn source_cat(&self, path: &str) -> Result<String> {
        let relative = Path::new(path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            bail!("invalid source path: {path}");
        }
        let worktree = self.source_worktree()?;
        let full = worktree.join(relative);
        fs::read_to_string(&full).with_context(|| format!("read source file {}", full.display()))
    }

    pub fn source_sync(&self) -> Result<usize> {
        let _ = self.source_worktree()?;
        let output = self.source_git_bytes(&self.root, ["ls-files", "-z"])?;
        let mut snapshot = self.worktree_snapshot()?;
        let files_qname = self.intern("technical/file")?;
        let files_hash = snapshot
            .get(files_qname)
            .context("technical/file list not found")?;
        let mut file_list = match self.read(files_hash)? {
            Object::ObjectList(value) => value,
            other => bail!("technical/file is {}", other.kind().name()),
        };
        let root_symbol = self.intern("root")?;
        let base_commit = snapshot
            .get(root_symbol)
            .and_then(|hash| self.read(hash).ok())
            .and_then(|object| match object {
                Object::Root(value) => Some(value.source_commit),
                _ => None,
            })
            .unwrap_or(GitCommit::ZERO);
        let head_commit = self.source_head_commit()?;
        let renames = if base_commit.is_zero() || base_commit == head_commit {
            HashMap::new()
        } else {
            self.source_renames(base_commit, head_commit)?
        };

        let mut tracked = BTreeSet::new();
        for chunk in output.split(|byte| *byte == 0) {
            if chunk.is_empty() {
                continue;
            }
            let path = std::str::from_utf8(chunk)?;
            if path.starts_with(".arche/") || path == ".arche" {
                continue;
            }
            tracked.insert(path.to_owned());
        }

        let mut dir_paths = BTreeSet::new();
        for path in &tracked {
            let mut parent = parent_path(path);
            while let Some(dir) = parent {
                dir_paths.insert(dir.to_owned());
                parent = parent_path(dir);
            }
        }
        self.sync_directories(&mut snapshot, &dir_paths)?;

        let mut removed = 0;
        for (qname, hash) in snapshot.entries().to_vec() {
            let Ok(Object::TechnicalFile(file)) = self.read(hash) else {
                continue;
            };
            let path = self.resolve(file.source.path)?;
            if !tracked.contains(&path) {
                file_list.items.retain(|item| *item != hash);
                snapshot.remove(qname);
                removed += 1;
            }
        }

        let mut count = 0;
        for path in &tracked {
            let qname_text = format!("technical/file/{path}");
            let qname = self.intern(&qname_text)?;
            let source_path = self.intern(path)?;
            let renamed_from = renames.get(path);
            let mut position = match snapshot.get(qname).and_then(|hash| self.read(hash).ok()) {
                Some(Object::TechnicalFile(value)) => value.position,
                _ => ObjectHash::ZERO,
            };
            if position.is_zero()
                && let Some(old_path) = renamed_from
            {
                let old_qname = self.intern(&format!("technical/file/{old_path}"))?;
                if let Some(old_hash) = snapshot.get(old_qname)
                    && let Object::TechnicalFile(old) = self.read(old_hash)?
                {
                    position = old.position;
                    file_list.items.retain(|item| *item != old_hash);
                    snapshot.remove(old_qname);
                }
            }
            let file = Object::TechnicalFile(TechnicalFile {
                qname,
                name: self.intern(path.rsplit('/').next().unwrap_or(path))?,
                parent: files_qname,
                source: SourcePath { path: source_path },
                entities: ObjectHash::ZERO,
                position,
            });
            let hash = self.save_object(&file)?;
            if let Some(old) = snapshot.get(qname) {
                file_list.items.retain(|item| *item != old);
            }
            if !file_list.items.contains(&hash) {
                file_list.items.push(hash);
            }
            snapshot.insert(qname, hash);
            count += 1;
        }
        let files_hash = self.save_object(&Object::ObjectList(file_list))?;
        snapshot.insert(files_qname, files_hash);
        let technical = self.intern("technical")?;
        if let Some(root_hash) = snapshot.get(technical) {
            let mut root = self.read(root_hash)?;
            if let Object::TechnicalRoot(value) = &mut root {
                value.file = files_hash;
                let root_hash = self.save_object(&root)?;
                snapshot.insert(technical, root_hash);
            }
        }
        if let Some(root_hash) = snapshot.get(root_symbol) {
            let mut root = self.read(root_hash)?;
            if let Object::Root(value) = &mut root {
                value.source_commit = head_commit;
                let root_hash = self.save_object(&root)?;
                snapshot.insert(root_symbol, root_hash);
            }
        }
        self.write_snapshot("worktree", &snapshot)?;
        let _ = removed;
        Ok(count)
    }

    pub fn source_changes(&self, base: Option<&str>) -> Result<String> {
        let head = self.source_git_text(&self.root, ["rev-parse", "HEAD"])?;
        let head = head.trim();
        let base = match base {
            Some(base) => self
                .source_git_text(&self.root, ["rev-parse", base])?
                .trim()
                .to_owned(),
            None => {
                let snapshot = self.worktree_snapshot()?;
                let root_symbol = self.intern("root")?;
                let commit = snapshot
                    .get(root_symbol)
                    .and_then(|hash| self.read(hash).ok())
                    .and_then(|object| match object {
                        Object::Root(value) => Some(value.source_commit),
                        _ => None,
                    })
                    .unwrap_or(GitCommit::ZERO);
                if commit.is_zero() {
                    let output = self.source_git_bytes(&self.root, ["ls-files", "-z"])?;
                    let mut text = String::new();
                    for chunk in output.split(|byte| *byte == 0) {
                        if chunk.is_empty() {
                            continue;
                        }
                        text.push_str("A\t");
                        text.push_str(std::str::from_utf8(chunk)?);
                        text.push('\n');
                    }
                    return Ok(text);
                }
                git_commit_hex(commit)
            }
        };
        if base == head {
            return Ok(String::new());
        }
        let ancestor = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["merge-base", "--is-ancestor", &base, head])
            .status()
            .context("run git merge-base")?
            .success();
        if !ancestor {
            let output = self.source_git_bytes(&self.root, ["ls-files", "-z"])?;
            let mut text = String::new();
            for chunk in output.split(|byte| *byte == 0) {
                if chunk.is_empty() {
                    continue;
                }
                text.push_str("A\t");
                text.push_str(std::str::from_utf8(chunk)?);
                text.push('\n');
            }
            return Ok(text);
        }
        let range = format!("{base}..{head}");
        let output =
            self.source_git_text(&self.root, ["diff", "--name-status", "-M", range.as_str()])?;
        let mut text = String::new();
        for line in output.lines() {
            let fields = line.split('\t').collect::<Vec<_>>();
            match fields.as_slice() {
                [status, path]
                    if status.starts_with('A')
                        || status.starts_with('M')
                        || status.starts_with('D') =>
                {
                    text.push_str(&format!("{}\t{path}\n", &status[..1]));
                }
                [status, old, new] if status.starts_with('R') => {
                    text.push_str(&format!("R\t{old}\t{new}\n"));
                }
                _ => {}
            }
        }
        Ok(text)
    }

    fn source_head_commit(&self) -> Result<GitCommit> {
        let text = self.source_git_text(&self.root, ["rev-parse", "HEAD"])?;
        parse_git_commit(text.trim())
    }

    fn source_renames(&self, base: GitCommit, head: GitCommit) -> Result<HashMap<String, String>> {
        let range = format!("{}..{}", git_commit_hex(base), git_commit_hex(head));
        let output =
            self.source_git_text(&self.root, ["diff", "--name-status", "-M", range.as_str()])?;
        let mut renames = HashMap::new();
        for line in output.lines() {
            let fields = line.split('\t').collect::<Vec<_>>();
            if let [status, old, new] = fields.as_slice()
                && status.starts_with('R')
            {
                renames.insert(new.to_string(), old.to_string());
            }
        }
        Ok(renames)
    }

    pub fn add_file(&self, path: &str) -> Result<()> {
        let snapshot = self.worktree_snapshot()?;
        let file_qname = self.intern(&format!("technical/file/{path}"))?;
        let source_path = self.intern(path)?;
        let mut selected = BTreeSet::new();
        selected.insert(file_qname);
        for (qname, hash) in snapshot.entries() {
            let object = self.read(*hash)?;
            if object_file(&object) == Some(file_qname)
                || object_source_path(&object) == Some(source_path)
            {
                selected.insert(*qname);
            }
        }
        for _ in 0..64 {
            let mut changed = false;
            let hashes = selected
                .iter()
                .filter_map(|qname| snapshot.get(*qname))
                .collect::<HashSet<_>>();
            for qname in selected.clone() {
                if let Some(hash) = snapshot.get(qname) {
                    let object = self.read(hash)?;
                    let parent = object.parent();
                    if !parent.is_zero()
                        && snapshot.get(parent).is_some()
                        && selected.insert(parent)
                    {
                        changed = true;
                    }
                    for (_, value) in doc_fields(&object) {
                        if value.is_zero() {
                            continue;
                        }
                        let doc = self.read(value)?;
                        if selected.insert(doc.qname()) {
                            changed = true;
                        }
                    }
                }
            }
            for (qname, hash) in snapshot.entries() {
                if selected.contains(qname) {
                    continue;
                }
                let object = self.read(*hash)?;
                if matches!(
                    object,
                    Object::ObjectList(_)
                        | Object::DocumentList(_)
                        | Object::DocumentParagraphList(_)
                ) && object
                    .references()
                    .iter()
                    .any(|reference| hashes.contains(reference))
                    && selected.insert(*qname)
                {
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let mut qnames = selected
            .iter()
            .map(|qname| self.resolve(*qname))
            .collect::<Result<Vec<_>>>()?;
        qnames.push("technical/file".to_owned());
        qnames.push("technical/directory".to_owned());
        qnames.push("technical".to_owned());
        let mut parent = parent_path(path);
        while let Some(dir) = parent {
            let qname = format!("technical/directory/{dir}");
            qnames.push(qname.clone());
            qnames.push(format!("{qname}/children"));
            parent = parent_path(dir);
        }
        self.add(&qnames)
    }

    pub fn add_directory(&self, path: &str) -> Result<()> {
        let qname = format!("technical/directory/{path}");
        if self
            .worktree_snapshot()?
            .get(SymbolHash::of(&qname))
            .is_none()
        {
            bail!("directory {path} is not tracked");
        }
        self.add(&[qname])
    }

    pub fn unstage_file(&self, path: &str) -> Result<()> {
        self.unstage(&[
            format!("technical/file/{path}"),
            "technical/file".to_owned(),
            "technical".to_owned(),
        ])
    }

    pub fn unstage_directory(&self, path: &str) -> Result<()> {
        self.unstage(&[
            format!("technical/directory/{path}"),
            "technical/file".to_owned(),
            "technical".to_owned(),
        ])
    }

    fn source_cache(&self) -> PathBuf {
        self.dir.join("source")
    }

    fn ensure_source_exclude(&self) -> Result<()> {
        let info = self.source_cache().join("info");
        fs::create_dir_all(&info)?;
        let path = info.join("exclude");
        let mut text = fs::read_to_string(&path).unwrap_or_default();
        if !text.lines().any(|line| line.trim() == ".arche/") {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(".arche/\n");
        }
        fs::write(path, text)?;
        // `.git/info/exclude` is the local, non-tracked way to ignore
        // `.arche/`. Also install a hook so even `git add -f .arche` cannot
        // commit design-space data into the source repository.
        let hooks = self.source_cache().join("hooks");
        fs::create_dir_all(&hooks)?;
        let hook = hooks.join("pre-commit");
        fs::write(
            &hook,
            "#!/bin/sh\n\
             if git diff --cached --name-only | grep -q '^\\.arche/'; then\n\
             \tprintf '%s\\n' 'refusing to commit .arche/: Arche data is not source' >&2\n\
             \texit 1\n\
             fi\n",
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&hook)?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&hook, permissions)?;
        }
        Ok(())
    }

    fn source_git_text<const N: usize>(&self, cwd: &Path, args: [&str; N]) -> Result<String> {
        let mut command = Command::new("git");
        command.arg("-C").arg(cwd).args(args);
        let output = command.output().context("run git")?;
        if !output.status.success() {
            bail!(
                "git failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        String::from_utf8(output.stdout).context("git output is not UTF-8")
    }

    fn source_git_bytes<const N: usize>(&self, cwd: &Path, args: [&str; N]) -> Result<Vec<u8>> {
        let mut command = Command::new("git");
        command.arg("-C").arg(cwd).args(args);
        let output = command.output().context("run git")?;
        if !output.status.success() {
            bail!(
                "git failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(output.stdout)
    }

    fn append_to_list(
        &self,
        snapshot: &mut Snapshot,
        owner_qname: &str,
        field: &str,
        item: ObjectHash,
    ) -> Result<()> {
        let owner = self.intern(owner_qname)?;
        let owner_hash = snapshot
            .get(owner)
            .with_context(|| format!("{owner_qname} not found"))?;
        let mut object = self.read(owner_hash)?;
        let existing = object_list_field(&object, field)?;
        let existing = (!existing.is_zero()).then_some(existing);
        let (list_qname, existing_hash) = match existing {
            Some(hash) => {
                let list = self.read(hash)?;
                let qname = match &list {
                    Object::ObjectList(value) => value.qname,
                    other => bail!("{owner_qname}/{field} is {}", other.kind().name()),
                };
                (qname, Some(hash))
            }
            None => (self.intern(&format!("{owner_qname}/{field}"))?, None),
        };
        let mut list = match existing_hash {
            Some(hash) => match self.read(hash)? {
                Object::ObjectList(value) => value,
                other => bail!("{owner_qname}/{field} is {}", other.kind().name()),
            },
            None => ObjectList {
                qname: list_qname,
                name: self.intern(field)?,
                parent: owner,
                items: Vec::new(),
            },
        };
        if !list.items.contains(&item) {
            list.items.push(item);
        }
        let list_hash = self.save_object(&Object::ObjectList(list))?;
        snapshot.insert(list_qname, list_hash);
        set_object_list_field(&mut object, field, list_hash)?;
        let owner_hash = self.save_object(&object)?;
        snapshot.insert(owner, owner_hash);
        Ok(())
    }

    pub fn create_technical_system(&self, name: &str) -> Result<(SymbolHash, ObjectHash)> {
        validate_local_name(name)?;
        let technical_name = self.intern("technical")?;
        let qname_text = format!("technical/{name}");
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(SymbolHash::of(&qname_text)).is_some() {
            bail!("object {qname_text} already exists");
        }
        let qname = self.intern(&qname_text)?;
        let name_hash = self.intern(name)?;
        let system = Object::TechnicalSystem(TechnicalSystem {
            qname,
            name: name_hash,
            parent: technical_name,
            members: ObjectHash::ZERO,
            modules: ObjectHash::ZERO,
            layers: ObjectHash::ZERO,
            constants: ObjectHash::ZERO,
            variables: ObjectHash::ZERO,
            position: ObjectHash::ZERO,
            responsibilities: ObjectHash::ZERO,
            non_responsibilities: ObjectHash::ZERO,
            features: ObjectHash::ZERO,
            non_features: ObjectHash::ZERO,
            invariants: ObjectHash::ZERO,
            constraints: ObjectHash::ZERO,
            design_principles: ObjectHash::ZERO,
            external_dependencies: ObjectHash::ZERO,
            usage: ObjectHash::ZERO,
            design_points: ObjectHash::ZERO,
        });
        let system_hash = self.save_object(&system)?;

        let technical_hash = snapshot
            .get(technical_name)
            .context("technical root not found")?;
        let technical = match self.read(technical_hash)? {
            Object::TechnicalRoot(value) => value,
            other => bail!("technical root is {}", other.kind().name()),
        };
        let systems_hash = technical.system;
        if systems_hash.is_zero() {
            bail!("technical root has no system list");
        }
        let mut system = match self.read(systems_hash)? {
            Object::ObjectList(value) => value,
            other => bail!("technical system is {}", other.kind().name()),
        };
        let systems_qname = system.qname;
        if !system.items.contains(&system_hash) {
            system.items.push(system_hash);
        }
        let systems_object = Object::ObjectList(system);
        let systems_hash = self.save_object(&systems_object)?;

        let technical_object = Object::TechnicalRoot(TechnicalRoot {
            system: systems_hash,
            ..technical
        });
        let technical_hash = self.save_object(&technical_object)?;

        snapshot.insert(qname, system_hash);
        snapshot.insert(systems_qname, systems_hash);
        snapshot.insert(technical_name, technical_hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, system_hash))
    }

    pub fn create_technical_module(
        &self,
        parent_qname: &str,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_local_name(name)?;
        let parent_symbol = self.intern(parent_qname)?;
        let mut snapshot = self.worktree_snapshot()?;
        let parent_hash = snapshot
            .get(parent_symbol)
            .with_context(|| format!("{parent_qname} not found"))?;
        let parent_object = self.read(parent_hash)?;
        let qname_text = format!("{parent_qname}/{name}");
        if snapshot.get(SymbolHash::of(&qname_text)).is_some() {
            bail!("object {qname_text} already exists");
        }
        let qname = self.intern(&qname_text)?;
        let name_hash = self.intern(name)?;
        let module = Object::TechnicalModule(crate::object::TechnicalModule {
            qname,
            name: name_hash,
            parent: parent_symbol,
            members: ObjectHash::ZERO,
            dependencies: Vec::new(),
            constants: ObjectHash::ZERO,
            variables: ObjectHash::ZERO,
            position: ObjectHash::ZERO,
            responsibilities: ObjectHash::ZERO,
            non_responsibilities: ObjectHash::ZERO,
            features: ObjectHash::ZERO,
            non_features: ObjectHash::ZERO,
            invariants: ObjectHash::ZERO,
            constraints: ObjectHash::ZERO,
            design_principles: ObjectHash::ZERO,
            usage: ObjectHash::ZERO,
            design_points: ObjectHash::ZERO,
        });
        let module_hash = self.save_object(&module)?;

        snapshot.insert(qname, module_hash);
        match parent_object {
            Object::TechnicalSystem(mut parent) => {
                let (modules_qname, modules_hash) = if parent.modules.is_zero() {
                    let text = format!("{parent_qname}/modules");
                    let modules_qname = self.intern(&text)?;
                    let list = ObjectList {
                        qname: modules_qname,
                        name: self.intern("modules")?,
                        parent: parent_symbol,
                        items: vec![module_hash],
                    };
                    let object = Object::ObjectList(list);
                    let hash = self.save_object(&object)?;
                    (modules_qname, hash)
                } else {
                    let mut list = match self.read(parent.modules)? {
                        Object::ObjectList(value) => value,
                        other => bail!("module list is {}", other.kind().name()),
                    };
                    let modules_qname = list.qname;
                    if !list.items.contains(&module_hash) {
                        list.items.push(module_hash);
                    }
                    let object = Object::ObjectList(list);
                    let hash = self.save_object(&object)?;
                    (modules_qname, hash)
                };
                parent.modules = modules_hash;
                let updated_parent_hash = self.save_object(&Object::TechnicalSystem(parent))?;
                snapshot.insert(modules_qname, modules_hash);
                snapshot.insert(parent_symbol, updated_parent_hash);
            }
            Object::TechnicalModule(_) => {}
            other => bail!("{} cannot contain a module", other.kind().name()),
        }
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, module_hash))
    }

    fn replace_doc_container(
        &self,
        snapshot: &mut Snapshot,
        owner_qname: &str,
        field: &str,
        value: Object,
    ) -> Result<ObjectHash> {
        let value_hash = self.save_object(&value)?;
        let owner_symbol = self.intern(owner_qname)?;
        let field_qname = self.intern(&format!("{owner_qname}/{field}"))?;
        snapshot.insert(field_qname, value_hash);
        let owner_hash = snapshot
            .get(owner_symbol)
            .with_context(|| format!("{owner_qname} not found"))?;
        let mut owner = self.read(owner_hash)?;
        set_doc_field(&mut owner, field, value_hash)?;
        let owner_hash = self.save_object(&owner)?;
        snapshot.insert(owner_symbol, owner_hash);
        Ok(value_hash)
    }

    pub fn document_field_kind(&self, owner_qname: &str, field: &str) -> Result<DocFieldKind> {
        let hash = self.find_by_qname(owner_qname)?;
        let object = self.read(hash)?;
        doc_field_kind(&object, field)
    }

    pub fn set_document_text(
        &self,
        owner_qname: &str,
        field: &str,
        text: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_paragraph(text, "text")?;
        let (mut snapshot, field_qname, _, _) =
            self.ensure_doc_container(owner_qname, field, &[DocFieldKind::Text])?;
        let document = Object::new_document_text(self, owner_qname, Some(field), text.to_owned())?;
        let document_hash =
            self.replace_doc_container(&mut snapshot, owner_qname, field, document)?;
        self.write_snapshot("worktree", &snapshot)?;
        Ok((field_qname, document_hash))
    }

    pub fn doc_list_add(&self, owner_qname: &str, field: &str) -> Result<(SymbolHash, ObjectHash)> {
        let (snapshot, field_qname, field_hash, _) =
            self.ensure_doc_container(owner_qname, field, &[DocFieldKind::List])?;
        self.write_snapshot("worktree", &snapshot)?;
        Ok((field_qname, field_hash))
    }

    pub fn doc_list_item_add(
        &self,
        owner_qname: &str,
        field: &str,
        text: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_paragraph(text, "list item")?;
        let (mut snapshot, field_qname, field_hash, _) =
            self.ensure_doc_container(owner_qname, field, &[DocFieldKind::List])?;
        let mut list = match self.read(field_hash)? {
            Object::DocumentList(value) => value,
            other => bail!("{} is {}", self.resolve(field_qname)?, other.kind().name()),
        };
        let item = Object::new_document_text(self, owner_qname, None, text.to_owned())?;
        let item_qname = item.qname();
        let item_hash = self.save_object(&item)?;
        list.items.push(item_hash);
        self.replace_doc_container(
            &mut snapshot,
            owner_qname,
            field,
            Object::DocumentList(list),
        )?;
        snapshot.insert(item_qname, item_hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((item_qname, item_hash))
    }

    pub fn doc_topic_add(
        &self,
        owner_qname: &str,
        field: &str,
        title: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_paragraph(title, "topic title")?;
        let (mut snapshot, field_qname, field_hash, _) =
            self.ensure_doc_container(owner_qname, field, &[DocFieldKind::ParagraphList])?;
        let mut list = match self.read(field_hash)? {
            Object::DocumentParagraphList(value) => value,
            other => bail!("{} is {}", self.resolve(field_qname)?, other.kind().name()),
        };
        let mut topic = Object::new_document_topic(
            self,
            owner_qname,
            None,
            title.to_owned(),
            ObjectHash::ZERO,
        )?;
        let topic_qname = topic.qname();
        let body_qname = self.intern(&format!("{}/body", self.resolve(topic_qname)?))?;
        let body = Object::new_document_paragraph_list(
            body_qname,
            self.intern("body")?,
            topic_qname,
            Vec::new(),
        );
        let body_hash = self.save_object(&body)?;
        if let Object::DocumentTopic(value) = &mut topic {
            value.body = body_hash;
        }
        let topic_hash = self.save_object(&topic)?;
        list.items.push(topic_hash);
        self.replace_doc_container(
            &mut snapshot,
            owner_qname,
            field,
            Object::DocumentParagraphList(list),
        )?;
        snapshot.insert(body_qname, body_hash);
        snapshot.insert(topic_qname, topic_hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((topic_qname, topic_hash))
    }

    pub fn doc_mermaid_set(
        &self,
        owner_qname: &str,
        field: &str,
        source: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        let (mut snapshot, field_qname, _, _) =
            self.ensure_doc_container(owner_qname, field, &[DocFieldKind::Mermaid])?;
        let document = Object::DocumentMermaid(DocumentMermaid {
            qname: field_qname,
            name: self.intern(field)?,
            parent: self.intern(owner_qname)?,
            source: source.to_owned(),
        });
        let hash = self.replace_doc_container(&mut snapshot, owner_qname, field, document)?;
        self.write_snapshot("worktree", &snapshot)?;
        Ok((field_qname, hash))
    }

    pub fn doc_code_add(
        &self,
        owner_qname: &str,
        field: &str,
        language: &str,
        source: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        let (mut snapshot, field_qname, field_hash, _) =
            self.ensure_doc_container(owner_qname, field, &[DocFieldKind::ParagraphList])?;
        let mut list = match self.read(field_hash)? {
            Object::DocumentParagraphList(value) => value,
            other => bail!("{} is {}", self.resolve(field_qname)?, other.kind().name()),
        };
        let code = Object::new_document_code(
            self,
            owner_qname,
            None,
            language.to_owned(),
            source.to_owned(),
        )?;
        let code_qname = code.qname();
        let code_hash = self.save_object(&code)?;
        list.items.push(code_hash);
        self.replace_doc_container(
            &mut snapshot,
            owner_qname,
            field,
            Object::DocumentParagraphList(list),
        )?;
        snapshot.insert(code_qname, code_hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((code_qname, code_hash))
    }

    pub fn doc_html_add(
        &self,
        owner_qname: &str,
        field: &str,
        source: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        let (mut snapshot, field_qname, field_hash, _) =
            self.ensure_doc_container(owner_qname, field, &[DocFieldKind::ParagraphList])?;
        let mut list = match self.read(field_hash)? {
            Object::DocumentParagraphList(value) => value,
            other => bail!("{} is {}", self.resolve(field_qname)?, other.kind().name()),
        };
        let html = Object::new_document_html(self, owner_qname, None, source.to_owned())?;
        let html_qname = html.qname();
        let html_hash = self.save_object(&html)?;
        list.items.push(html_hash);
        self.replace_doc_container(
            &mut snapshot,
            owner_qname,
            field,
            Object::DocumentParagraphList(list),
        )?;
        snapshot.insert(html_qname, html_hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((html_qname, html_hash))
    }

    fn ensure_doc_container(
        &self,
        owner_qname: &str,
        field: &str,
        allowed: &[DocFieldKind],
    ) -> Result<(Snapshot, SymbolHash, ObjectHash, DocFieldKind)> {
        let owner_symbol = self.intern(owner_qname)?;
        let mut snapshot = self.worktree_snapshot()?;
        let owner_hash = snapshot
            .get(owner_symbol)
            .with_context(|| format!("{owner_qname} not found"))?;
        let mut owner = self.read(owner_hash)?;
        let kind = doc_field_kind(&owner, field)?;
        if !allowed.contains(&kind) {
            bail!("{field} on {owner_qname} is a {kind:?} field");
        }
        let field_qname_text = format!("{owner_qname}/{field}");
        let field_qname = self.intern(&field_qname_text)?;
        let existing = doc_fields(&owner)
            .into_iter()
            .find(|(name, _)| *name == field)
            .map(|(_, value)| value)
            .filter(|value| !value.is_zero());
        let field_hash = match existing {
            Some(hash) => hash,
            None => {
                let object = match kind {
                    DocFieldKind::Text => {
                        Object::new_document_text(self, owner_qname, Some(field), String::new())?
                    }
                    DocFieldKind::List => Object::DocumentList(DocumentList {
                        qname: field_qname,
                        name: self.intern(field)?,
                        parent: self.intern(owner_qname)?,
                        items: Vec::new(),
                    }),
                    DocFieldKind::ParagraphList => {
                        Object::DocumentParagraphList(DocumentParagraphList {
                            qname: field_qname,
                            name: self.intern(field)?,
                            parent: self.intern(owner_qname)?,
                            items: Vec::new(),
                        })
                    }
                    DocFieldKind::Mermaid => Object::DocumentMermaid(DocumentMermaid {
                        qname: field_qname,
                        name: self.intern(field)?,
                        parent: self.intern(owner_qname)?,
                        source: String::new(),
                    }),
                };
                let hash = self.save_object(&object)?;
                snapshot.insert(field_qname, hash);
                hash
            }
        };
        set_doc_field(&mut owner, field, field_hash)?;
        let owner_hash = self.save_object(&owner)?;
        snapshot.insert(owner_symbol, owner_hash);
        Ok((snapshot, field_qname, field_hash, kind))
    }

    pub fn comment_add(&self, target_qname: &str, text: &str) -> Result<Comment> {
        let branch = self.current_branch()?;
        let target = self.intern(target_qname)?;
        let snapshot = self.worktree_snapshot()?;
        snapshot
            .get(target)
            .with_context(|| format!("{target_qname} not found"))?;
        self.comments.add(&branch, target_qname, text, "arche")
    }

    pub fn comment_list(&self, target: Option<&str>, open_only: bool) -> Result<Vec<Comment>> {
        let branch = self.current_branch()?;
        self.comments.list(&branch, target, open_only)
    }

    pub fn comment_list_on(
        &self,
        branch: &str,
        target: Option<&str>,
        open_only: bool,
    ) -> Result<Vec<Comment>> {
        self.comments.list(branch, target, open_only)
    }

    /// List comments for the current branch on `target` and every qname below
    /// it, including field and anchored-position comments.
    pub fn comment_list_subtree(&self, target: &str, open_only: bool) -> Result<Vec<Comment>> {
        let branch = self.current_branch()?;
        self.comments.list_subtree(&branch, target, open_only)
    }

    pub fn comment_clear(&self, target: Option<&str>) -> Result<usize> {
        let branch = self.current_branch()?;
        self.comments.clear(&branch, target)
    }

    pub fn comment_resolve(&self, id: u64, conclusion: &str, agent: &str) -> Result<Comment> {
        self.comments.resolve(id, conclusion, agent)
    }

    /// Export every branch's comments for push.
    pub fn export_comments(&self) -> Result<Vec<u8>> {
        self.comments.export()
    }

    /// Import a remote comment snapshot. Returns the merged store.
    pub fn import_comments(&self, bytes: &[u8], remote_authoritative: bool) -> Result<Vec<u8>> {
        self.comments.merge(bytes, remote_authoritative)
    }

    /// Replace the comment store with an authoritative server snapshot.
    pub fn replace_comments(&self, bytes: &[u8]) -> Result<Vec<Comment>> {
        self.comments.replace(bytes)
    }

    pub fn concept_mkdir(&self, qname_text: &str) -> Result<(SymbolHash, ObjectHash)> {
        validate_concept_path(qname_text, true)?;
        let qname = self.intern(qname_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let (parent_text, local) = qname_text
            .rsplit_once('/')
            .with_context(|| format!("concept directory {qname_text} needs a parent"))?;
        let parent = self.intern(parent_text)?;
        let parent_hash = snapshot
            .get(parent)
            .with_context(|| format!("{parent_text} not found"))?;
        let directory = match self.read(parent_hash)? {
            Object::ConceptDirectory(value) => value,
            other => bail!("{parent_text} is {}", other.kind().name()),
        };
        let directories_hash = directory.directories;
        if directories_hash.is_zero() {
            bail!("concept directory has no directory list");
        }
        let mut directories = match self.read(directories_hash)? {
            Object::ObjectList(value) => value,
            other => bail!("concept directory list is {}", other.kind().name()),
        };
        let directories_qname = directories.qname;
        let (new_directories_qname, new_directories_hash) =
            self.create_list(&format!("{qname_text}/directories"), qname)?;
        let (new_concepts_qname, new_concepts_hash) =
            self.create_list(&format!("{qname_text}/concepts"), qname)?;
        let object = Object::ConceptDirectory(ConceptDirectory {
            qname,
            name: self.intern(local)?,
            parent,
            directories: new_directories_hash,
            concepts: new_concepts_hash,
        });
        let hash = self.save_object(&object)?;
        directories.items.push(hash);
        let directories_object = Object::ObjectList(directories);
        let directories_hash = self.save_object(&directories_object)?;
        snapshot.insert(qname, hash);
        snapshot.insert(new_directories_qname, new_directories_hash);
        snapshot.insert(new_concepts_qname, new_concepts_hash);
        snapshot.insert(directories_qname, directories_hash);
        let mut parent_object = self.read(parent_hash)?;
        if let Object::ConceptDirectory(value) = &mut parent_object {
            value.directories = directories_hash;
            let parent_hash = self.save_object(&parent_object)?;
            snapshot.insert(parent, parent_hash);
        }
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn concept_create(
        &self,
        qname_text: &str,
        name: &str,
        explanation: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_concept_path(qname_text, false)?;
        let qname = self.intern(qname_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let (parent_text, local) = qname_text
            .rsplit_once('/')
            .with_context(|| format!("concept {qname_text} needs a parent"))?;
        let parent = self.intern(parent_text)?;
        let parent_hash = snapshot
            .get(parent)
            .with_context(|| format!("{parent_text} not found"))?;
        let directory = match self.read(parent_hash)? {
            Object::ConceptDirectory(value) => value,
            other => bail!("{parent_text} is {}", other.kind().name()),
        };
        let concepts_hash = directory.concepts;
        if concepts_hash.is_zero() {
            bail!("concept directory has no concept list");
        }
        let mut concepts = match self.read(concepts_hash)? {
            Object::ObjectList(value) => value,
            other => bail!("concept list is {}", other.kind().name()),
        };
        let concepts_qname = concepts.qname;
        let text = Object::new_document_text(self, qname_text, None, explanation.to_owned())?;
        let text_qname = text.qname();
        let text_hash = self.save_object(&text)?;
        let explanation_qname = self.intern(&format!("{qname_text}/explanation"))?;
        let explanation_object = Object::DocumentParagraphList(DocumentParagraphList {
            qname: explanation_qname,
            name: self.intern("explanation")?,
            parent: qname,
            items: vec![text_hash],
        });
        let explanation_hash = self.save_object(&explanation_object)?;
        let concept = Object::Concept(Concept {
            qname,
            name: self.intern(local)?,
            parent,
            display_name: name.to_owned(),
            explanation: explanation_hash,
        });
        let concept_hash = self.save_object(&concept)?;
        concepts.items.push(concept_hash);
        let concepts_object = Object::ObjectList(concepts);
        let concepts_hash = self.save_object(&concepts_object)?;
        snapshot.insert(text_qname, text_hash);
        snapshot.insert(explanation_qname, explanation_hash);
        snapshot.insert(qname, concept_hash);
        snapshot.insert(concepts_qname, concepts_hash);
        let mut parent_object = self.read(parent_hash)?;
        if let Object::ConceptDirectory(value) = &mut parent_object {
            value.concepts = concepts_hash;
            let parent_hash = self.save_object(&parent_object)?;
            snapshot.insert(parent, parent_hash);
        }
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, concept_hash))
    }

    pub fn concept_list(&self) -> Result<Vec<(SymbolHash, Concept)>> {
        let snapshot = self.worktree_snapshot()?;
        let mut concept = Vec::new();
        for (qname, hash) in snapshot.entries() {
            if let Object::Concept(value) = self.read(*hash)? {
                concept.push((*qname, value));
            }
        }
        concept.sort_by_key(|(_, value)| value.name);
        Ok(concept)
    }

    pub fn create_technical_layer(
        &self,
        parent_qname: &str,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_local_name(name)?;
        let parent = self.intern(parent_qname)?;
        let mut snapshot = self.worktree_snapshot()?;
        let parent_hash = snapshot
            .get(parent)
            .with_context(|| format!("{parent_qname} not found"))?;
        if !matches!(
            self.read(parent_hash)?,
            Object::TechnicalSystem(_) | Object::TechnicalModule(_)
        ) {
            bail!("{parent_qname} cannot contain a layer");
        }
        let qname_text = format!("{parent_qname}/{name}");
        if snapshot.get(SymbolHash::of(&qname_text)).is_some() {
            bail!("object {qname_text} already exists");
        }
        let qname = self.intern(&qname_text)?;
        let object = Object::TechnicalLayer(TechnicalLayer {
            qname,
            name: self.intern(name)?,
            parent,
            members: ObjectHash::ZERO,
            dependencies: Vec::new(),
            position: ObjectHash::ZERO,
        });
        let hash = self.save_object(&object)?;
        self.append_to_list(&mut snapshot, parent_qname, "layers", hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_technical_namespace(
        &self,
        parent_qname: &str,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_local_name(name)?;
        let parent = self.intern(parent_qname)?;
        let mut snapshot = self.worktree_snapshot()?;
        let parent_hash = snapshot
            .get(parent)
            .with_context(|| format!("{parent_qname} not found"))?;
        let (qname_text, field) = match self.read(parent_hash)? {
            Object::TechnicalRoot(_) => (format!("technical/namespace/{name}"), "namespace"),
            Object::TechnicalNamespace(_) => (format!("{parent_qname}/{name}"), "children"),
            other => bail!("{} cannot contain a namespace", other.kind().name()),
        };
        if snapshot.get(SymbolHash::of(&qname_text)).is_some() {
            bail!("object {qname_text} already exists");
        }
        let qname = self.intern(&qname_text)?;
        let object = Object::TechnicalNamespace(TechnicalNamespace {
            qname,
            name: self.intern(name)?,
            parent,
            children: ObjectHash::ZERO,
            entities: ObjectHash::ZERO,
            constants: ObjectHash::ZERO,
            variables: ObjectHash::ZERO,
            position: ObjectHash::ZERO,
        });
        let hash = self.save_object(&object)?;
        self.append_to_list(&mut snapshot, parent_qname, field, hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_technical_class(
        &self,
        parent_qname: Option<&str>,
        namespace_qname: Option<&str>,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_local_name(name)?;
        let (owner_qname, field, parent, namespace, qname_text) =
            self.code_type_owner(parent_qname, namespace_qname, name, "types")?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(SymbolHash::of(&qname_text)).is_some() {
            bail!("object {qname_text} already exists");
        }
        let qname = self.intern(&qname_text)?;
        let object = Object::TechnicalClass(TechnicalClass {
            qname,
            name: self.intern(name)?,
            parent,
            namespace,
            file: SymbolHash::ZERO,
            module: SymbolHash::ZERO,
            source: SourceSpan::default(),
            superclass: SymbolHash::ZERO,
            interfaces: Vec::new(),
            mixins: Vec::new(),
            explicit_dependencies: Vec::new(),
            implicit_dependencies: Vec::new(),
            method: ObjectHash::ZERO,
            types: ObjectHash::ZERO,
            parts: ObjectHash::ZERO,
            constants: ObjectHash::ZERO,
            members: ObjectHash::ZERO,
            properties: ObjectHash::ZERO,
            position: ObjectHash::ZERO,
            responsibilities: ObjectHash::ZERO,
            non_responsibilities: ObjectHash::ZERO,
            features: ObjectHash::ZERO,
            non_features: ObjectHash::ZERO,
            invariants: ObjectHash::ZERO,
            constraints: ObjectHash::ZERO,
            design_principles: ObjectHash::ZERO,
            usage: ObjectHash::ZERO,
            design_points: ObjectHash::ZERO,
        });
        let hash = self.save_object(&object)?;
        self.append_to_list(&mut snapshot, &owner_qname, field, hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    fn code_type_owner(
        &self,
        parent_qname: Option<&str>,
        namespace_qname: Option<&str>,
        name: &str,
        inner_field: &'static str,
    ) -> Result<CodeTypeOwner> {
        if let Some(parent) = parent_qname {
            let parent_symbol = self.intern(parent)?;
            Ok((
                parent.to_owned(),
                inner_field,
                parent_symbol,
                SymbolHash::ZERO,
                format!("{parent}/{name}"),
            ))
        } else if let Some(namespace) = namespace_qname {
            let namespace_symbol = self.intern(namespace)?;
            Ok((
                namespace.to_owned(),
                "entities",
                SymbolHash::ZERO,
                namespace_symbol,
                format!("{namespace}/{name}"),
            ))
        } else {
            bail!("code type requires --parent or --namespace")
        }
    }

    pub fn create_technical_struct(
        &self,
        parent_qname: Option<&str>,
        namespace_qname: Option<&str>,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_local_name(name)?;
        let (owner_qname, field, parent, namespace, qname_text) =
            self.code_type_owner(parent_qname, namespace_qname, name, "types")?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(SymbolHash::of(&qname_text)).is_some() {
            bail!("object {qname_text} already exists");
        }
        let qname = self.intern(&qname_text)?;
        let object = Object::TechnicalStruct(TechnicalStruct {
            qname,
            name: self.intern(name)?,
            parent,
            namespace,
            file: SymbolHash::ZERO,
            module: SymbolHash::ZERO,
            source: SourceSpan::default(),
            dependencies: Vec::new(),
            method: ObjectHash::ZERO,
            types: ObjectHash::ZERO,
            constants: ObjectHash::ZERO,
            members: ObjectHash::ZERO,
            position: ObjectHash::ZERO,
            invariants: ObjectHash::ZERO,
        });
        let hash = self.save_object(&object)?;
        self.append_to_list(&mut snapshot, &owner_qname, field, hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_technical_enum(
        &self,
        parent_qname: Option<&str>,
        namespace_qname: Option<&str>,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_local_name(name)?;
        let (owner_qname, field, parent, namespace, qname_text) =
            self.code_type_owner(parent_qname, namespace_qname, name, "types")?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(SymbolHash::of(&qname_text)).is_some() {
            bail!("object {qname_text} already exists");
        }
        let qname = self.intern(&qname_text)?;
        let object = Object::TechnicalEnum(TechnicalEnum {
            qname,
            name: self.intern(name)?,
            parent,
            namespace,
            file: SymbolHash::ZERO,
            module: SymbolHash::ZERO,
            source: SourceSpan::default(),
            dependencies: Vec::new(),
            values: ObjectHash::ZERO,
            position: ObjectHash::ZERO,
        });
        let hash = self.save_object(&object)?;
        self.append_to_list(&mut snapshot, &owner_qname, field, hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_technical_interface(
        &self,
        parent_qname: Option<&str>,
        namespace_qname: Option<&str>,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_local_name(name)?;
        let (owner_qname, field, parent, namespace, qname_text) =
            self.code_type_owner(parent_qname, namespace_qname, name, "types")?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(SymbolHash::of(&qname_text)).is_some() {
            bail!("object {qname_text} already exists");
        }
        let qname = self.intern(&qname_text)?;
        let object = Object::TechnicalInterface(TechnicalInterface {
            qname,
            name: self.intern(name)?,
            parent,
            namespace,
            file: SymbolHash::ZERO,
            module: SymbolHash::ZERO,
            source: SourceSpan::default(),
            extends: SymbolHash::ZERO,
            dependencies: Vec::new(),
            methods: ObjectHash::ZERO,
            position: ObjectHash::ZERO,
            responsibilities: ObjectHash::ZERO,
            non_responsibilities: ObjectHash::ZERO,
            features: ObjectHash::ZERO,
            non_features: ObjectHash::ZERO,
            invariants: ObjectHash::ZERO,
            usage: ObjectHash::ZERO,
            design_points: ObjectHash::ZERO,
        });
        let hash = self.save_object(&object)?;
        self.append_to_list(&mut snapshot, &owner_qname, field, hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_technical_mixin(
        &self,
        parent_qname: Option<&str>,
        namespace_qname: Option<&str>,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_local_name(name)?;
        let (owner_qname, field, parent, namespace, qname_text) =
            self.code_type_owner(parent_qname, namespace_qname, name, "types")?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(SymbolHash::of(&qname_text)).is_some() {
            bail!("object {qname_text} already exists");
        }
        let qname = self.intern(&qname_text)?;
        let object = Object::TechnicalMixin(TechnicalMixin {
            qname,
            name: self.intern(name)?,
            parent,
            namespace,
            file: SymbolHash::ZERO,
            module: SymbolHash::ZERO,
            source: SourceSpan::default(),
            on: Vec::new(),
            superclass: SymbolHash::ZERO,
            interfaces: Vec::new(),
            mixins: Vec::new(),
            explicit_dependencies: Vec::new(),
            implicit_dependencies: Vec::new(),
            methods: ObjectHash::ZERO,
            types: ObjectHash::ZERO,
            position: ObjectHash::ZERO,
            responsibilities: ObjectHash::ZERO,
            non_responsibilities: ObjectHash::ZERO,
            features: ObjectHash::ZERO,
            non_features: ObjectHash::ZERO,
            invariants: ObjectHash::ZERO,
            usage: ObjectHash::ZERO,
            design_points: ObjectHash::ZERO,
        });
        let hash = self.save_object(&object)?;
        self.append_to_list(&mut snapshot, &owner_qname, field, hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_technical_function(
        &self,
        namespace_qname: &str,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_local_name(name)?;
        let namespace = self.intern(namespace_qname)?;
        let qname_text = format!("{namespace_qname}/{name}");
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(SymbolHash::of(&qname_text)).is_some() {
            bail!("object {qname_text} already exists");
        }
        let qname = self.intern(&qname_text)?;
        let object = Object::TechnicalFunction(TechnicalFunction {
            qname,
            name: self.intern(name)?,
            parent: namespace,
            namespace,
            file: SymbolHash::ZERO,
            module: SymbolHash::ZERO,
            source: SourceSpan::default(),
            is_async: false,
            explicit_dependencies: Vec::new(),
            implicit_dependencies: Vec::new(),
            position: ObjectHash::ZERO,
            arguments: ObjectHash::ZERO,
            return_value: ObjectHash::ZERO,
            usage: ObjectHash::ZERO,
            design_points: ObjectHash::ZERO,
        });
        let hash = self.save_object(&object)?;
        self.append_to_list(&mut snapshot, namespace_qname, "children", hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_technical_method(
        &self,
        parent_qname: &str,
        prototype: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_paragraph(prototype, "prototype")?;
        if prototype.trim().is_empty() {
            bail!("method prototype must not be empty");
        }
        if let Some(visibility) = prototype.split_whitespace().find(|token| {
            matches!(
                *token,
                "pub" | "public" | "private" | "protected" | "internal"
            )
        }) {
            bail!("method prototype must not include visibility modifier {visibility}");
        }
        let parent = self.intern(parent_qname)?;
        let name = self.intern(prototype)?;
        let qname_text = format!("{parent_qname}/{prototype}");
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(SymbolHash::of(&qname_text)).is_some() {
            bail!("object {qname_text} already exists");
        }
        let qname = self.intern(&qname_text)?;
        let object = Object::TechnicalMethod(TechnicalMethod {
            qname,
            name,
            parent,
            source: SourceSpan::default(),
            is_static: false,
            is_async: false,
            is_abstract: false,
            is_const: false,
            overrides: SymbolHash::ZERO,
            explicit_dependencies: Vec::new(),
            implicit_dependencies: Vec::new(),
            position: ObjectHash::ZERO,
            arguments: ObjectHash::ZERO,
            return_value: ObjectHash::ZERO,
            usage: ObjectHash::ZERO,
            design_points: ObjectHash::ZERO,
        });
        let hash = self.save_object(&object)?;
        self.append_to_list(&mut snapshot, parent_qname, "method", hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    fn parse_source_span(&self, text: &str) -> Result<SourceSpan> {
        let (path, range) = text
            .rsplit_once(':')
            .with_context(|| format!("source span {text} must be path:start-end"))?;
        let (start, end) = range
            .split_once('-')
            .with_context(|| format!("source span {text} must be path:start-end"))?;
        let start = start.parse::<u32>()?;
        let end = end.parse::<u32>()?;
        if start > end {
            bail!("source span {text} has start after end");
        }
        Ok(SourceSpan {
            path: self.intern(path)?,
            start,
            end,
        })
    }

    pub fn create_technical_part(
        &self,
        parent_qname: &str,
        source_path: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        if source_path.trim().is_empty() {
            bail!("part source must not be empty");
        }
        let source = self.parse_source_span(source_path)?;
        let object = Object::new_technical_part(
            self,
            parent_qname,
            None,
            source,
            Vec::new(),
            Vec::new(),
            ObjectHash::ZERO,
            ObjectHash::ZERO,
            ObjectHash::ZERO,
        )?;
        let qname = object.qname();
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {} already exists", self.resolve(qname)?);
        }
        let hash = self.save_object(&object)?;
        self.append_to_list(&mut snapshot, parent_qname, "parts", hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    /// Create a requirement persona with a role-representative local id and a
    /// display name. The id is the qname key (`requirement/user/<id>`);
    /// display_name carries the fictional persona's personal name.
    pub fn create_requirement_user(
        &self,
        name_text: &str,
        display_name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_local_name(name_text)?;
        let qname_text = format!("requirement/user/{name_text}");
        let qname = self.intern(&qname_text)?;
        let name = self.intern(name_text)?;
        let parent = self.intern("requirement")?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let object = Object::RequirementUser(RequirementUser {
            qname,
            name,
            parent,
            display_name: display_name.to_owned(),
            age: String::new(),
            occupation: String::new(),
            situations: ObjectHash::ZERO,
            hobbies: ObjectHash::ZERO,
            needs: ObjectHash::ZERO,
            priority: 0,
            stories: Vec::new(),
        });
        let hash = self.save_object(&object)?;
        self.append_child(&mut snapshot, parent, ListField::Users, hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_requirement_story(&self, name: &str) -> Result<(SymbolHash, ObjectHash)> {
        validate_requirement_name(name)?;
        let parent = self.intern("requirement")?;
        let qname_text = format!("requirement/story/{name}");
        let qname = self.intern(&qname_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let object = Object::RequirementStory(RequirementStory {
            qname,
            name: self.intern(name)?,
            parent,
            preconditions: ObjectHash::ZERO,
            frequency: 0,
            description: ObjectHash::ZERO,
            requirement: ObjectHash::ZERO,
            users: Vec::new(),
            functions: Vec::new(),
            protagonist: SymbolHash::ZERO,
            flow: ObjectHash::ZERO,
        });
        let hash = self.save_object(&object)?;
        self.append_child(&mut snapshot, parent, ListField::Stories, hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_requirement_system(&self, name: &str) -> Result<(SymbolHash, ObjectHash)> {
        validate_requirement_name(name)?;
        let parent = self.intern("requirement")?;
        let qname_text = format!("requirement/system/{name}");
        let qname = self.intern(&qname_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let object = Object::RequirementSystem(RequirementSystem {
            qname,
            name: self.intern(name)?,
            parent,
            display_name: String::new(),
            position: ObjectHash::ZERO,
            app_form: ObjectHash::ZERO,
            users: Vec::new(),
            modules: ObjectHash::ZERO,
            ..Default::default()
        });
        let hash = self.save_object(&object)?;
        self.append_child(&mut snapshot, parent, ListField::Systems, hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_requirement_module(
        &self,
        parent_qname: &str,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_requirement_name(name)?;
        let parent = self.intern(parent_qname)?;
        let qname_text = format!("{parent_qname}/{name}");
        let qname = self.intern(&qname_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let parent_hash = snapshot
            .get(parent)
            .with_context(|| format!("{parent_qname} not found"))?;
        if !matches!(self.read(parent_hash)?, Object::RequirementSystem(_)) {
            bail!("{parent_qname} is not a RequirementSystem");
        }
        let object = Object::RequirementModule(RequirementModule {
            qname,
            name: self.intern(name)?,
            parent,
            display_name: String::new(),
            position: ObjectHash::ZERO,
            functions: ObjectHash::ZERO,
        });
        let hash = self.save_object(&object)?;
        self.append_child(&mut snapshot, parent, ListField::Modules, hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_requirement_function(
        &self,
        parent_qname: &str,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_requirement_name(name)?;
        let parent = self.intern(parent_qname)?;
        let qname_text = format!("{parent_qname}/{name}");
        let qname = self.intern(&qname_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let parent_hash = snapshot
            .get(parent)
            .with_context(|| format!("{parent_qname} not found"))?;
        if !matches!(self.read(parent_hash)?, Object::RequirementModule(_)) {
            bail!("{parent_qname} is not a RequirementModule");
        }
        let object = Object::RequirementFunction(RequirementFunction {
            qname,
            name: self.intern(name)?,
            parent,
            flow: ObjectHash::ZERO,
            interactions: ObjectHash::ZERO,
            initiator: ObjectHash::ZERO,
            system_change: ObjectHash::ZERO,
            preconditions: ObjectHash::ZERO,
            feedback: ObjectHash::ZERO,
            description: ObjectHash::ZERO,
        });
        let hash = self.save_object(&object)?;
        self.append_child(&mut snapshot, parent, ListField::Functions, hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    pub fn create_requirement_interaction(
        &self,
        parent_qname: &str,
        name: &str,
    ) -> Result<(SymbolHash, ObjectHash)> {
        validate_requirement_name(name)?;
        let parent = self.intern(parent_qname)?;
        let qname_text = format!("{parent_qname}/{name}");
        let qname = self.intern(&qname_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        if snapshot.get(qname).is_some() {
            bail!("object {qname_text} already exists");
        }
        let parent_hash = snapshot
            .get(parent)
            .with_context(|| format!("{parent_qname} not found"))?;
        if !matches!(self.read(parent_hash)?, Object::RequirementFunction(_)) {
            bail!("{parent_qname} is not a RequirementFunction");
        }
        let object = Object::RequirementInteraction(RequirementInteraction {
            qname,
            name: self.intern(name)?,
            parent,
            description: ObjectHash::ZERO,
            ui: ObjectHash::ZERO,
        });
        let hash = self.save_object(&object)?;
        self.append_child(&mut snapshot, parent, ListField::Interactions, hash)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok((qname, hash))
    }

    fn append_child(
        &self,
        snapshot: &mut Snapshot,
        parent: SymbolHash,
        field: ListField,
        child: ObjectHash,
    ) -> Result<()> {
        let parent_hash = snapshot
            .get(parent)
            .with_context(|| format!("parent {} not found", parent.to_hex()))?;
        let mut parent_object = self.read(parent_hash)?;
        let mut list_hash = field.get(&parent_object)?;
        if list_hash.is_zero() {
            let parent_text = self.resolve(parent)?;
            let list_qname = self.intern(&format!("{parent_text}/{}", field.name()))?;
            let list = ObjectList {
                qname: list_qname,
                name: self.intern(field.name())?,
                parent,
                items: Vec::new(),
            };
            list_hash = self.save_object(&Object::ObjectList(list))?;
            field.set(&mut parent_object, list_hash)?;
            let parent_hash = self.save_object(&parent_object)?;
            snapshot.insert(parent, parent_hash);
            snapshot.insert(list_qname, list_hash);
        }
        let mut list = match self.read(list_hash)? {
            Object::ObjectList(value) => value,
            other => bail!("list is {}", other.kind().name()),
        };
        let list_qname = list.qname;
        if !list.items.contains(&child) {
            list.items.push(child);
        }
        let list_hash = self.save_object(&Object::ObjectList(list))?;
        snapshot.insert(list_qname, list_hash);
        field.set(&mut parent_object, list_hash)?;
        let parent_hash = self.save_object(&parent_object)?;
        snapshot.insert(parent, parent_hash);
        Ok(())
    }

    pub fn search(
        &self,
        query: Option<&str>,
        kinds: &[String],
        has_design: bool,
    ) -> Result<Vec<(ObjectHash, Object)>> {
        let normalized_kinds = kinds
            .iter()
            .map(|kind| kind.to_lowercase().replace('-', ""))
            .collect::<Vec<_>>();
        let snapshot = self.worktree_snapshot()?;
        let mut result = Vec::new();
        for (qname, hash) in snapshot.entries() {
            let object = self.read(*hash)?;
            let kind = object.kind().name().to_lowercase();
            if !normalized_kinds.is_empty() && !normalized_kinds.contains(&kind) {
                continue;
            }
            if has_design
                && !doc_fields(&object)
                    .iter()
                    .any(|(_, value)| !value.is_zero())
            {
                continue;
            }
            if let Some(query) = query {
                let qname_text = self.resolve(*qname)?;
                let name = object.name();
                let name_text = if name.is_zero() {
                    String::new()
                } else {
                    self.resolve(name)?
                };
                if !qname_text.contains(query) && !name_text.contains(query) {
                    continue;
                }
            }
            result.push((*hash, object));
        }
        Ok(result)
    }

    #[allow(clippy::collapsible_if)]
    /// Set one scalar property (String, bool or integer) from a CLI string.
    pub fn set_scalar(&self, qname_text: &str, field: &str, text: &str) -> Result<()> {
        let mut snapshot = self.worktree_snapshot()?;
        let qname = self.intern(qname_text)?;
        let hash = snapshot
            .get(qname)
            .with_context(|| format!("{qname_text} not found"))?;
        let mut object = self.read(hash)?;
        let kind = object
            .get_property(field)
            .map_err(|error| anyhow::anyhow!("{qname_text}.{field}: {error}"))?
            .kind();
        let value = match kind {
            ValueKind::String => PropertyValue::String(text.to_owned()),
            ValueKind::Bool => PropertyValue::Bool(match text {
                "true" => true,
                "false" => false,
                _ => bail!("{qname_text}.{field} expects true or false"),
            }),
            ValueKind::Int => PropertyValue::Int(
                text.parse()
                    .with_context(|| format!("{qname_text}.{field} expects an integer"))?,
            ),
            other => bail!("{qname_text}.{field} is {other:?}, not a scalar property"),
        };
        object
            .set_property(field, value)
            .map_err(|error| anyhow::anyhow!("{qname_text}.{field}: {error}"))?;
        let hash = self.save_object(&object)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok(())
    }

    pub fn import_doc_json(&self, owner_qname: &str, text: &str) -> Result<usize> {
        let value: Value = serde_json::from_str(text).context("parse document JSON")?;
        let Some(fields) = value
            .get("fields")
            .and_then(Value::as_object)
            .or_else(|| value.as_object())
        else {
            bail!("document JSON must contain an object of fields");
        };
        let owner_symbol = self.intern(owner_qname)?;
        let mut snapshot = self.worktree_snapshot()?;
        let owner_hash = snapshot
            .get(owner_symbol)
            .with_context(|| format!("{owner_qname} not found"))?;
        let mut owner = self.read(owner_hash)?;
        let mut count = 0;
        // Scalar properties (display_name, priority, ...) are not document
        // fields, but batch/import callers expect to set them in the same
        // fields object. Apply them first so the later document writes see a
        // fresh owner with both scalar and document fields.
        let mut documents = Vec::new();
        let mut scalar_changed = false;
        for (field, spec) in fields {
            let kind = match doc_field_kind(&owner, field) {
                Ok(kind) => kind,
                Err(document_error) => {
                    let property = owner.get_property(field).map_err(|_| document_error)?;
                    let value = Self::scalar_property_value(field, spec, property.kind())?;
                    owner
                        .set_property(field, value)
                        .map_err(|error| anyhow::anyhow!("{owner_qname}.{field}: {error}"))?;
                    scalar_changed = true;
                    count += 1;
                    continue;
                }
            };
            documents.push((field.clone(), spec.clone(), kind));
        }
        if scalar_changed {
            let hash = self.save_object(&owner)?;
            snapshot.insert(owner_symbol, hash);
        }
        for (field, spec, kind) in documents {
            let field = field.as_str();
            let spec = &spec;
            match kind {
                DocFieldKind::Text => {
                    let text = document_text(spec)?;
                    validate_paragraph(&text, field)?;
                    let document = Object::new_document_text(self, owner_qname, Some(field), text)?;
                    self.replace_doc_container(&mut snapshot, owner_qname, field, document)?;
                }
                DocFieldKind::List => {
                    let items = self.import_list_items(&mut snapshot, owner_qname, spec)?;
                    let document = Object::DocumentList(DocumentList {
                        qname: self.intern(&format!("{owner_qname}/{field}"))?,
                        name: self.intern(field)?,
                        parent: owner_symbol,
                        items,
                    });
                    self.replace_doc_container(&mut snapshot, owner_qname, field, document)?;
                }
                DocFieldKind::ParagraphList => {
                    let items = self.import_paragraph_items(&mut snapshot, owner_qname, spec)?;
                    let document = Object::DocumentParagraphList(DocumentParagraphList {
                        qname: self.intern(&format!("{owner_qname}/{field}"))?,
                        name: self.intern(field)?,
                        parent: owner_symbol,
                        items,
                    });
                    self.replace_doc_container(&mut snapshot, owner_qname, field, document)?;
                }
                DocFieldKind::Mermaid => {
                    let source = document_source(spec)?;
                    let document = Object::DocumentMermaid(DocumentMermaid {
                        qname: self.intern(&format!("{owner_qname}/{field}"))?,
                        name: self.intern(field)?,
                        parent: owner_symbol,
                        source,
                    });
                    self.replace_doc_container(&mut snapshot, owner_qname, field, document)?;
                }
            }
            count += 1;
        }
        self.write_snapshot("worktree", &snapshot)?;
        Ok(count)
    }

    fn scalar_property_value(field: &str, spec: &Value, kind: ValueKind) -> Result<PropertyValue> {
        Ok(match kind {
            ValueKind::String => PropertyValue::String(
                spec.as_str()
                    .with_context(|| format!("{field} expects a string"))?
                    .to_owned(),
            ),
            ValueKind::Bool => PropertyValue::Bool(
                spec.as_bool()
                    .with_context(|| format!("{field} expects true or false"))?,
            ),
            ValueKind::Int => PropertyValue::Int(
                spec.as_i64()
                    .with_context(|| format!("{field} expects an integer"))?,
            ),
            other => bail!("{field} is {other:?}, not a scalar property"),
        })
    }

    fn import_list_items(
        &self,
        snapshot: &mut Snapshot,
        owner_qname: &str,
        spec: &Value,
    ) -> Result<Vec<ObjectHash>> {
        let items = spec
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut hashes = Vec::new();
        for item in items {
            let text = document_text(&item)?;
            let object = Object::new_document_text(self, owner_qname, None, text)?;
            let qname = object.qname();
            let hash = self.save_object(&object)?;
            snapshot.insert(qname, hash);
            hashes.push(hash);
        }
        Ok(hashes)
    }

    fn import_paragraph_items(
        &self,
        snapshot: &mut Snapshot,
        owner_qname: &str,
        spec: &Value,
    ) -> Result<Vec<ObjectHash>> {
        if spec.is_null() {
            return Ok(Vec::new());
        }
        if let Some(items) = spec.get("items").and_then(Value::as_array) {
            let mut hashes = Vec::new();
            for item in items {
                hashes.push(self.import_block(snapshot, owner_qname, item)?);
            }
            return Ok(hashes);
        }
        Ok(vec![self.import_block(snapshot, owner_qname, spec)?])
    }

    fn import_block(
        &self,
        snapshot: &mut Snapshot,
        owner_qname: &str,
        spec: &Value,
    ) -> Result<ObjectHash> {
        let kind = spec.get("type").and_then(Value::as_str).unwrap_or("text");
        let object = match kind {
            "text" | "DocumentText" => {
                Object::new_document_text(self, owner_qname, None, document_text(spec)?)?
            }
            "list" | "DocumentList" => {
                let (qname, name, parent) =
                    crate::construct::new_identity(self, owner_qname, None)?;
                Object::DocumentList(DocumentList {
                    qname,
                    name,
                    parent,
                    items: self.import_list_items(snapshot, owner_qname, spec)?,
                })
            }
            "topic" => {
                let title = document_text(spec.get("title").unwrap_or(&Value::Null))?;
                let mut topic =
                    Object::new_document_topic(self, owner_qname, None, title, ObjectHash::ZERO)?;
                let topic_qname = topic.qname();
                let body_qname = self.intern(&format!("{}/body", self.resolve(topic_qname)?))?;
                let body = Object::DocumentParagraphList(DocumentParagraphList {
                    qname: body_qname,
                    name: self.intern("body")?,
                    parent: topic_qname,
                    items: self.import_paragraph_items(
                        snapshot,
                        owner_qname,
                        spec.get("body").unwrap_or(&Value::Null),
                    )?,
                });
                let body_hash = self.save_object(&body)?;
                if let Object::DocumentTopic(value) = &mut topic {
                    value.body = body_hash;
                }
                snapshot.insert(body_qname, body_hash);
                topic
            }
            "code" | "DocumentCode" => {
                let language = spec
                    .get("language")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                Object::new_document_code(
                    self,
                    owner_qname,
                    None,
                    language.to_owned(),
                    document_source(spec)?,
                )?
            }
            "mermaid" | "DocumentMermaid" => {
                let (qname, name, parent) =
                    crate::construct::new_identity(self, owner_qname, None)?;
                Object::DocumentMermaid(DocumentMermaid {
                    qname,
                    name,
                    parent,
                    source: document_source(spec)?,
                })
            }
            "html" | "DocumentHtml" => {
                Object::new_document_html(self, owner_qname, None, document_source(spec)?)?
            }
            other => bail!("unsupported document block type {other}"),
        };
        let qname = object.qname();
        let hash = self.save_object(&object)?;
        snapshot.insert(qname, hash);
        Ok(hash)
    }

    pub fn doc_lint(&self) -> Result<Vec<String>> {
        let mut errors = Vec::new();
        for (_, hash) in self.worktree_snapshot()?.entries() {
            match self.read(*hash)? {
                Object::DocumentText(value) => {
                    if value.content.contains('\n') || value.content.contains('\r') {
                        errors.push(format!(
                            "{}: text must be a single paragraph",
                            self.resolve(value.qname)?
                        ));
                    }
                }
                Object::DocumentTopic(value) => {
                    if value.title.contains('\n') || value.title.contains('\r') {
                        errors.push(format!(
                            "{}: topic title must be a single line",
                            self.resolve(value.qname)?
                        ));
                    }
                }
                _ => {}
            }
        }
        errors.sort();
        Ok(errors)
    }

    pub fn check(&self) -> Result<Vec<String>> {
        let snapshot = self.worktree_snapshot()?;
        let mut errors = Vec::new();
        for (_, hash) in snapshot.entries() {
            let object = self.read(*hash)?;
            let qname = self.resolve(object.qname())?;
            // Hash verification is intentionally disabled for now. It needs a
            // raw-record check so old object versions can be read without
            // retaining legacy fields in the current model.
            for (field, value) in doc_fields(&object) {
                if value.is_zero() {
                    continue;
                }
                if self.read(value).is_err() {
                    errors.push(format!("{qname}: field {field} target is missing"));
                    continue;
                }
                let field_object = self.read(value)?;
                let kind = doc_field_kind(&object, field)?;
                let expected = expected_doc_kind(kind);
                if field_object.kind() != expected {
                    errors.push(format!(
                        "{qname}: field {field} must be {}, found {}",
                        expected.name(),
                        field_object.kind().name()
                    ));
                }
            }
            match &object {
                Object::ObjectList(value) => {
                    for item in &value.items {
                        if self.read(*item).is_err() {
                            errors.push(format!("{qname}: list item {} is missing", item.to_hex()));
                        }
                    }
                }
                Object::DocumentList(value) => {
                    for item in &value.items {
                        if self.read(*item).is_err() {
                            errors.push(format!("{qname}: item {} is missing", item.to_hex()));
                        }
                    }
                }
                Object::DocumentParagraphList(value) => {
                    for item in &value.items {
                        if self.read(*item).is_err() {
                            errors.push(format!("{qname}: item {} is missing", item.to_hex()));
                        }
                    }
                }
                Object::DocumentTopic(value) => {
                    if self.read(value.body).is_err() {
                        errors.push(format!("{qname}: topic body is missing"));
                    }
                }
                Object::RequirementInteraction(value) => {
                    if !value.ui.is_zero()
                        && let Ok(Object::DocumentParagraphList(list)) = self.read(value.ui)
                    {
                        for item in &list.items {
                            if let Ok(Object::DocumentHtml(_)) = self.read(*item) {
                            } else {
                                errors.push(format!("{qname}: ui item must be DocumentHtml"));
                            }
                        }
                    }
                }
                Object::RequirementStory(value) => {
                    for user in &value.users {
                        let Ok(user_qname) = self.resolve(*user) else {
                            errors.push(format!("{qname}: user {} is missing", user.to_hex()));
                            continue;
                        };
                        match self.find_by_qname(&user_qname) {
                            Ok(hash) if matches!(self.read(hash)?, Object::RequirementUser(_)) => {}
                            Ok(_) => errors
                                .push(format!("{qname}: {user_qname} is not a RequirementUser")),
                            Err(_) => errors.push(format!("{qname}: {user_qname} is missing")),
                        }
                    }
                    for function in &value.functions {
                        let Ok(function_qname) = self.resolve(*function) else {
                            errors.push(format!(
                                "{qname}: function {} is missing",
                                function.to_hex()
                            ));
                            continue;
                        };
                        match self.find_by_qname(&function_qname) {
                            Ok(hash)
                                if matches!(self.read(hash)?, Object::RequirementFunction(_)) => {}
                            Ok(_) => errors.push(format!(
                                "{qname}: {function_qname} is not a RequirementFunction"
                            )),
                            Err(_) => errors.push(format!("{qname}: {function_qname} is missing")),
                        }
                    }
                    if !value.protagonist.is_zero() {
                        match self.find_by_qname(&self.resolve(value.protagonist)?) {
                            Ok(hash) if matches!(self.read(hash)?, Object::RequirementUser(_)) => {
                                if !value.users.contains(&value.protagonist) {
                                    errors
                                        .push(format!("{qname}: protagonist must be one of users"));
                                }
                            }
                            Ok(_) => errors.push(format!(
                                "{qname}: protagonist {} is not a RequirementUser",
                                value.protagonist.to_hex()
                            )),
                            Err(_) => errors.push(format!(
                                "{qname}: protagonist {} is missing",
                                value.protagonist.to_hex()
                            )),
                        }
                    }
                }
                Object::RequirementFunction(value) => {
                    if !value.interactions.is_zero()
                        && let Ok(Object::ObjectList(list)) = self.read(value.interactions)
                    {
                        for item in list.items {
                            if !matches!(self.read(item)?, Object::RequirementInteraction(_)) {
                                errors.push(format!(
                                    "{qname}: interaction {} has wrong kind",
                                    item.to_hex()
                                ));
                            }
                        }
                    }
                }
                Object::RequirementSystem(value) => {
                    if !value.modules.is_zero()
                        && let Ok(Object::ObjectList(list)) = self.read(value.modules)
                    {
                        for item in list.items {
                            if !matches!(self.read(item)?, Object::RequirementModule(_)) {
                                errors.push(format!(
                                    "{qname}: module {} has wrong kind",
                                    item.to_hex()
                                ));
                            }
                        }
                    }
                }
                Object::RequirementModule(value) => {
                    if !value.functions.is_zero()
                        && let Ok(Object::ObjectList(list)) = self.read(value.functions)
                    {
                        for item in list.items {
                            if !matches!(self.read(item)?, Object::RequirementFunction(_)) {
                                errors.push(format!(
                                    "{qname}: function {} has wrong kind",
                                    item.to_hex()
                                ));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(errors)
    }

    /// Update one string-reference field (for example `file` or `module`) and
    /// re-save the object. References to the old object are rewritten by the
    /// worktree snapshot normalizer.
    pub fn set_string_ref(&self, qname_text: &str, field: &str, target_qname: &str) -> Result<()> {
        let mut snapshot = self.worktree_snapshot()?;
        let qname = self.intern(qname_text)?;
        let hash = snapshot
            .get(qname)
            .with_context(|| format!("{qname_text} not found"))?;
        let mut object = self.read(hash)?;
        let target = self.intern(target_qname)?;
        object
            .set_property(field, PropertyValue::StringRef(target))
            .map_err(|error| anyhow::anyhow!("{qname_text}.{field}: {error}"))?;
        let hash = self.save_object(&object)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok(())
    }

    /// Project non-document attributes (String, integer, bool, name/qname
    /// references and reference lists) for display. Document fields and
    /// object references are rendered separately by their field renderer.
    pub fn object_attributes(&self, object: &Object) -> Result<Vec<(String, String)>> {
        let mut attributes = Vec::new();
        for spec in object.properties() {
            if matches!(spec.name.as_str(), "qname" | "name" | "parent") {
                continue;
            }
            let value = match object.get_property(&spec.name) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let rendered = match value {
                PropertyValue::String(value) if !value.is_empty() => value,
                PropertyValue::Bool(value) => value.to_string(),
                PropertyValue::Int(value) if value != 0 => value.to_string(),
                PropertyValue::GitCommit(value) => value.to_hex(),
                PropertyValue::StringRef(value) if !value.is_zero() => self.resolve(value)?,
                PropertyValue::StringRefList(values) if !values.is_empty() => values
                    .as_slice()
                    .iter()
                    .map(|value| self.resolve(*value))
                    .collect::<Result<Vec<_>>>()?
                    .join("、"),
                _ => continue,
            };
            attributes.push((spec.name, rendered));
        }
        Ok(attributes)
    }

    /// Replace a string-reference field with one or more qnames. Single
    /// references accept at most one target; list references accept any
    /// number, including zero to clear the list.
    pub fn set_refs(&self, qname_text: &str, field: &str, targets: &[String]) -> Result<()> {
        let mut snapshot = self.worktree_snapshot()?;
        let qname = self.intern(qname_text)?;
        let hash = snapshot
            .get(qname)
            .with_context(|| format!("{qname_text} not found"))?;
        let mut object = self.read(hash)?;
        let kind = object
            .get_property(field)
            .map_err(|error| anyhow::anyhow!("{qname_text}.{field}: {error}"))?
            .kind();
        let value = match kind {
            ValueKind::StringRef => {
                if targets.len() > 1 {
                    bail!("{qname_text}.{field} is a single reference");
                }
                let target = match targets.first() {
                    Some(target) => self.intern(target)?,
                    None => SymbolHash::ZERO,
                };
                PropertyValue::StringRef(target)
            }
            ValueKind::StringRefList => {
                let mut values = Vec::with_capacity(targets.len());
                for target in targets {
                    values.push(self.intern(target)?);
                }
                PropertyValue::StringRefList(StringRefList(values))
            }
            other => bail!("{qname_text}.{field} is {other:?}, not a reference"),
        };
        object
            .set_property(field, value)
            .map_err(|error| anyhow::anyhow!("{qname_text}.{field}: {error}"))?;
        let hash = self.save_object(&object)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok(())
    }

    /// Set the source span of a code object from `path:start-end`.
    pub fn set_source(&self, qname_text: &str, source_text: &str) -> Result<()> {
        let span = self.parse_source_span(source_text)?;
        let mut snapshot = self.worktree_snapshot()?;
        let qname = self.intern(qname_text)?;
        let hash = snapshot
            .get(qname)
            .with_context(|| format!("{qname_text} not found"))?;
        let mut object = self.read(hash)?;
        object
            .set_property("source", PropertyValue::SourceSpan(span))
            .map_err(|error| anyhow::anyhow!("{qname_text}.source: {error}"))?;
        let hash = self.save_object(&object)?;
        snapshot.insert(qname, hash);
        self.write_snapshot("worktree", &snapshot)?;
        Ok(())
    }

    /// Assign a top-level code element to exactly one system, module, or layer.
    pub fn assign_member(&self, member_qname: &str, parent_qname: &str) -> Result<()> {
        let mut snapshot = self.worktree_snapshot()?;
        let member_symbol = self.intern(member_qname)?;
        let member_hash = snapshot
            .get(member_symbol)
            .with_context(|| format!("{member_qname} not found"))?;
        let member = self.read(member_hash)?;
        if !is_assignable(&member) {
            bail!("{member_qname} is not a system/module-level design element");
        }
        let parent_symbol = self.intern(parent_qname)?;
        let parent_hash = snapshot
            .get(parent_symbol)
            .with_context(|| format!("{parent_qname} not found"))?;
        let parent = self.read(parent_hash)?;
        if !matches!(
            parent,
            Object::TechnicalSystem(_) | Object::TechnicalModule(_) | Object::TechnicalLayer(_)
        ) {
            bail!("{parent_qname} is not a system, module, or layer");
        }
        self.remove_member(&mut snapshot, member_hash, parent_qname)?;
        self.append_to_list(&mut snapshot, parent_qname, "members", member_hash)?;
        self.write_snapshot("worktree", &snapshot)?;
        Ok(())
    }

    fn remove_member(
        &self,
        snapshot: &mut Snapshot,
        member: ObjectHash,
        keep_owner: &str,
    ) -> Result<()> {
        for (qname, hash) in snapshot.entries().to_vec() {
            let qname_text = self.resolve(qname)?;
            let Some(owner_qname) = qname_text.strip_suffix("/members") else {
                continue;
            };
            if owner_qname == keep_owner {
                continue;
            }
            let owner_symbol = SymbolHash::of(owner_qname);
            let Some(owner_hash) = snapshot.get(owner_symbol) else {
                continue;
            };
            let mut owner = self.read(owner_hash)?;
            if !matches!(
                owner,
                Object::TechnicalSystem(_) | Object::TechnicalModule(_) | Object::TechnicalLayer(_)
            ) {
                continue;
            }
            let mut list = match self.read(hash)? {
                Object::ObjectList(value) => value,
                _ => continue,
            };
            if !list.items.contains(&member) {
                continue;
            }
            list.items.retain(|item| *item != member);
            let list_hash = self.save_object(&Object::ObjectList(list))?;
            snapshot.insert(qname, list_hash);
            set_object_list_field(&mut owner, "members", list_hash)?;
            let owner_hash = self.save_object(&owner)?;
            snapshot.insert(owner_symbol, owner_hash);
        }
        Ok(())
    }

    fn list_has_items(&self, hash: ObjectHash) -> Result<bool> {
        if hash.is_zero() {
            return Ok(false);
        }
        Ok(match self.read(hash)? {
            Object::ObjectList(value) => !value.items.is_empty(),
            _ => false,
        })
    }

    fn membership_lists(&self, snapshot: &Snapshot) -> Result<Vec<(String, Vec<ObjectHash>)>> {
        let mut lists = Vec::new();
        for (qname, hash) in snapshot.entries() {
            let object = self.read(*hash)?;
            if !matches!(
                object,
                Object::TechnicalSystem(_) | Object::TechnicalModule(_) | Object::TechnicalLayer(_)
            ) {
                continue;
            }
            let members = object_list_field(&object, "members")?;
            let items = if members.is_zero() {
                Vec::new()
            } else {
                match self.read(members)? {
                    Object::ObjectList(value) => value.items,
                    other => bail!(
                        "{} members is {}",
                        self.resolve(*qname)?,
                        other.kind().name()
                    ),
                }
            };
            lists.push((self.resolve(*qname)?, items));
        }
        Ok(lists)
    }

    /// Systems, modules, layers and their direct members.
    pub fn structure_list(&self) -> Result<Vec<(String, Vec<String>)>> {
        let snapshot = self.worktree_snapshot()?;
        let mut out = Vec::new();
        for (owner, items) in self.membership_lists(&snapshot)? {
            let mut names = Vec::new();
            for item in items {
                names.push(self.resolve(self.read(item)?.qname())?);
            }
            names.sort();
            out.push((owner, names));
        }
        out.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(out)
    }

    /// Top-level code elements that are not assigned to any system or module.
    pub fn unassigned_members(&self) -> Result<Vec<String>> {
        let snapshot = self.worktree_snapshot()?;
        let assigned = self
            .membership_lists(&snapshot)?
            .into_iter()
            .flat_map(|(_, items)| items)
            .collect::<HashSet<_>>();
        let mut out = Vec::new();
        for (qname, hash) in snapshot.entries() {
            let object = self.read(*hash)?;
            if is_assignable(&object) && !assigned.contains(hash) {
                out.push(self.resolve(*qname)?);
            }
        }
        out.sort();
        Ok(out)
    }

    /// Validate that every top-level code element has exactly one owner and
    /// that every membership list points at an existing object.
    pub fn membership_check(&self) -> Result<Vec<String>> {
        let snapshot = self.worktree_snapshot()?;
        let lists = self.membership_lists(&snapshot)?;
        let mut counts: HashMap<ObjectHash, usize> = HashMap::new();
        let mut errors = Vec::new();
        for (owner, items) in &lists {
            if items.is_empty() {
                let owner_hash = snapshot
                    .get(SymbolHash::of(owner))
                    .with_context(|| format!("{owner} not found"))?;
                let has_children = match self.read(owner_hash)? {
                    Object::TechnicalSystem(value) => {
                        self.list_has_items(value.modules)? || self.list_has_items(value.layers)?
                    }
                    _ => false,
                };
                if !has_children {
                    errors.push(format!("{owner}: has no members"));
                }
            }
            for item in items {
                *counts.entry(*item).or_default() += 1;
                if self.read(*item).is_err() {
                    errors.push(format!("{owner}: member {} is missing", item.to_hex()));
                }
            }
        }
        for (qname, hash) in snapshot.entries() {
            let object = self.read(*hash)?;
            if !is_assignable(&object) {
                continue;
            }
            let qname = self.resolve(*qname)?;
            match counts.get(hash).copied().unwrap_or(0) {
                0 => errors.push(format!("{qname}: not assigned to a system or module")),
                n if n > 1 => errors.push(format!("{qname}: assigned to {n} owners")),
                _ => {}
            }
        }
        errors.sort();
        Ok(errors)
    }

    /// Report design objects that carry no document fields at all.
    pub fn doc_check(&self) -> Result<Vec<String>> {
        let snapshot = self.worktree_snapshot()?;
        let mut errors = Vec::new();
        for (qname, hash) in snapshot.entries() {
            let object = self.read(*hash)?;
            if !requires_design(&object) {
                continue;
            }
            if matches!(object, Object::TechnicalMethod(_)) {
                let parent = object.parent();
                if let Some(parent_hash) = snapshot.get(parent)
                    && matches!(self.read(parent_hash)?, Object::TechnicalPart(_))
                {
                    continue;
                }
            }
            if doc_fields(&object).iter().all(|(_, value)| value.is_zero()) {
                errors.push(format!("{}: missing design fields", self.resolve(*qname)?));
            }
        }
        errors.sort();
        Ok(errors)
    }

    pub fn show_qname(&self, qname: &str) -> Result<(ObjectHash, Object)> {
        let hash = self.find_by_qname(qname)?;
        let object = self.read(hash)?;
        Ok((hash, object))
    }

    fn bootstrap(&self) -> Result<()> {
        let mut config = String::from("format = 2\n");
        if let Some(source_url) = &self.source_url {
            config.push_str(&format!("source={source_url}\n"));
        }
        if let Some(remote_url) = &self.remote_url {
            config.push_str(&format!("remote={remote_url}\n"));
        }
        fs::write(self.dir.join("config"), config).with_context(|| "write Arche config")?;
        fs::write(self.dir.join("HEAD"), format!("ref: {MAIN_REF}\n"))
            .with_context(|| "write HEAD")?;

        let technical_name = self.intern("technical")?;
        let requirements_name = self.intern("requirement")?;
        let concepts_name = self.intern("concept")?;
        let root_name = self.intern("root")?;

        let (technical_systems, technical_systems_hash) =
            self.create_list("technical/system", technical_name)?;
        let (technical_files, technical_files_hash) =
            self.create_list("technical/file", technical_name)?;
        let (technical_directories, technical_directories_hash) =
            self.create_list("technical/directory", technical_name)?;
        let (technical_namespaces, technical_namespaces_hash) =
            self.create_list("technical/namespace", technical_name)?;
        let technical = Object::TechnicalRoot(TechnicalRoot {
            qname: technical_name,
            name: technical_name,
            parent: root_name,
            file: technical_files_hash,
            directory: technical_directories_hash,
            namespace: technical_namespaces_hash,
            system: technical_systems_hash,
        });
        let _ = (
            technical_systems,
            technical_files,
            technical_directories,
            technical_namespaces,
        );
        let technical_hash = self.save_object(&technical)?;

        let (requirement_users, requirement_users_hash) =
            self.create_list("requirement/user", requirements_name)?;
        let (requirement_stories, requirement_stories_hash) =
            self.create_list("requirement/story", requirements_name)?;
        let (requirement_systems, requirement_systems_hash) =
            self.create_list("requirement/system", requirements_name)?;
        let requirement = Object::RequirementRoot(RequirementRoot {
            qname: requirements_name,
            name: requirements_name,
            parent: root_name,
            user: requirement_users_hash,
            story: requirement_stories_hash,
            system: requirement_systems_hash,
        });
        let _ = (requirement_users, requirement_stories, requirement_systems);
        let requirements_hash = self.save_object(&requirement)?;

        let concepts_domain = self.intern("concept/domain")?;
        let concepts_requirements = self.intern("concept/requirement")?;
        let concepts_technical = self.intern("concept/technical")?;
        let (_, domain_directories_hash) =
            self.create_list("concept/domain/directories", concepts_domain)?;
        let (_, domain_concepts_hash) =
            self.create_list("concept/domain/concepts", concepts_domain)?;
        let domain = Object::ConceptDirectory(ConceptDirectory {
            qname: concepts_domain,
            name: self.intern("domain")?,
            parent: concepts_name,
            directories: domain_directories_hash,
            concepts: domain_concepts_hash,
        });
        let domain_hash = self.save_object(&domain)?;
        let (_, requirement_directories_hash) =
            self.create_list("concept/requirement/directories", concepts_requirements)?;
        let (_, requirement_concepts_hash) =
            self.create_list("concept/requirement/concepts", concepts_requirements)?;
        let requirement_concepts = Object::ConceptDirectory(ConceptDirectory {
            qname: concepts_requirements,
            name: self.intern("requirement")?,
            parent: concepts_name,
            directories: requirement_directories_hash,
            concepts: requirement_concepts_hash,
        });
        let requirements_concepts_hash = self.save_object(&requirement_concepts)?;
        let (_, technical_directories_hash) =
            self.create_list("concept/technical/directories", concepts_technical)?;
        let (_, technical_concepts_hash) =
            self.create_list("concept/technical/concepts", concepts_technical)?;
        let technical_concepts = Object::ConceptDirectory(ConceptDirectory {
            qname: concepts_technical,
            name: self.intern("technical")?,
            parent: concepts_name,
            directories: technical_directories_hash,
            concepts: technical_concepts_hash,
        });
        let technical_concepts_hash = self.save_object(&technical_concepts)?;
        let (_, concept_directories_hash) = self.create_list_with_items(
            "concept/directories",
            concepts_name,
            vec![
                domain_hash,
                requirements_concepts_hash,
                technical_concepts_hash,
            ],
        )?;
        let (_, concept_concepts_hash) = self.create_list("concept/concepts", concepts_name)?;
        let concept = Object::ConceptDirectory(ConceptDirectory {
            qname: concepts_name,
            name: concepts_name,
            parent: root_name,
            directories: concept_directories_hash,
            concepts: concept_concepts_hash,
        });
        let concepts_hash = self.save_object(&concept)?;

        self.bootstrap_project(root_name)?;

        let root = Object::Root(Root {
            qname: root_name,
            name: root_name,
            source_commit: GitCommit::ZERO,
            technical: technical_hash,
            requirement: requirements_hash,
            concept: concepts_hash,
        });
        let root_hash = self.save_object(&root)?;
        let snapshot = self.snapshot_from_store()?;
        self.commit(None, root_hash, "init".to_owned(), "arche", &snapshot)?;
        self.write_snapshot("index", &snapshot)?;
        self.write_snapshot("worktree", &snapshot)?;
        Ok(())
    }

    fn bootstrap_project(&self, root_name: SymbolHash) -> Result<()> {
        let project_name = self.intern("project")?;
        let project_document_qname = self.intern("project/document")?;
        let document_qname = "project/document/document";

        let positioning = self.bootstrap_doc_topic(
            document_qname,
            "positioning",
            "定位",
            vec![
                self.bootstrap_doc_topic(
                    "project/document/document/positioning/body",
                    "what_is",
                    "是什么",
                    vec![self.bootstrap_doc_text(
                        "project/document/document/positioning/body/what_is/body",
                        "text",
                        "",
                    )?],
                )?,
                self.bootstrap_doc_topic(
                    "project/document/document/positioning/body",
                    "what_is_not",
                    "不是什么",
                    vec![self.bootstrap_doc_list(
                        "project/document/document/positioning/body/what_is_not/body",
                        "list",
                    )?],
                )?,
            ],
        )?;
        let background = self.bootstrap_doc_topic(
            document_qname,
            "background",
            "背景",
            vec![
                self.bootstrap_doc_topic(
                    "project/document/document/background/body",
                    "context",
                    "现状",
                    vec![self.bootstrap_doc_text(
                        "project/document/document/background/body/context/body",
                        "text",
                        "",
                    )?],
                )?,
                self.bootstrap_doc_topic(
                    "project/document/document/background/body",
                    "problems",
                    "问题",
                    vec![self.bootstrap_doc_list(
                        "project/document/document/background/body/problems/body",
                        "list",
                    )?],
                )?,
            ],
        )?;
        let goals = self.bootstrap_doc_topic(
            document_qname,
            "goals",
            "目标",
            vec![self.bootstrap_doc_list("project/document/document/goals/body", "list")?],
        )?;
        let non_goals = self.bootstrap_doc_topic(
            document_qname,
            "non_goals",
            "非目标",
            vec![self.bootstrap_doc_list("project/document/document/non_goals/body", "list")?],
        )?;
        let phasing_plan = self.bootstrap_doc_topic(
            document_qname,
            "phasing_plan",
            "分期计划",
            vec![self.bootstrap_doc_list("project/document/document/phasing_plan/body", "list")?],
        )?;
        let constraints = self.bootstrap_doc_topic(
            document_qname,
            "constraints",
            "约束",
            vec![self.bootstrap_doc_list("project/document/document/constraints/body", "list")?],
        )?;
        let risks = self.bootstrap_doc_topic(
            document_qname,
            "risks",
            "风险",
            vec![self.bootstrap_doc_list("project/document/document/risks/body", "list")?],
        )?;

        let document = Object::new_document_paragraph_list(
            self.intern(document_qname)?,
            self.intern("document")?,
            project_document_qname,
            vec![
                positioning,
                background,
                goals,
                non_goals,
                phasing_plan,
                constraints,
                risks,
            ],
        );
        let document_hash = self.save_object(&document)?;
        let project_document = Object::new_project_document(
            project_document_qname,
            self.intern("项目说明书")?,
            project_name,
            document_hash,
        );
        let project_document_hash = self.save_object(&project_document)?;
        let project = Object::new_project_root(
            project_name,
            self.intern("项目空间")?,
            root_name,
            project_document_hash,
        );
        self.save_object(&project)?;
        Ok(())
    }

    fn bootstrap_doc_topic(
        &self,
        parent_qname: &str,
        name: &str,
        title: &str,
        body_items: Vec<ObjectHash>,
    ) -> Result<ObjectHash> {
        let topic_qname_text = format!("{parent_qname}/{name}");
        let topic_qname = self.intern(&topic_qname_text)?;
        let body_qname = self.intern(&format!("{topic_qname_text}/body"))?;
        let body = Object::new_document_paragraph_list(
            body_qname,
            self.intern("body")?,
            topic_qname,
            body_items,
        );
        let body_hash = self.save_object(&body)?;
        let topic = Object::new_document_topic(
            self,
            parent_qname,
            Some(name),
            title.to_owned(),
            body_hash,
        )?;
        self.save_object(&topic)
    }

    fn bootstrap_doc_text(
        &self,
        parent_qname: &str,
        name: &str,
        content: &str,
    ) -> Result<ObjectHash> {
        let text = Object::new_document_text(self, parent_qname, Some(name), content.to_owned())?;
        self.save_object(&text)
    }

    fn bootstrap_doc_list(&self, parent_qname: &str, name: &str) -> Result<ObjectHash> {
        let qname = self.intern(&format!("{parent_qname}/{name}"))?;
        let list = Object::new_document_list(
            qname,
            self.intern(name)?,
            self.intern(parent_qname)?,
            Vec::new(),
        );
        self.save_object(&list)
    }

    fn snapshot_from_store(&self) -> Result<Snapshot> {
        let mut snapshot = Snapshot::default();
        for (hash, object) in self.objects.all(self)? {
            snapshot.insert(object.qname(), hash);
        }
        Ok(snapshot)
    }

    fn create_list(
        &self,
        qname_text: &str,
        owner_text: SymbolHash,
    ) -> Result<(SymbolHash, ObjectHash)> {
        self.create_list_with_items(qname_text, owner_text, Vec::new())
    }

    fn create_list_with_items(
        &self,
        qname_text: &str,
        owner_text: SymbolHash,
        items: Vec<ObjectHash>,
    ) -> Result<(SymbolHash, ObjectHash)> {
        let qname = self.intern(qname_text)?;
        let name = self.intern(qname_text.rsplit('/').next().unwrap_or(qname_text))?;
        let object = Object::ObjectList(ObjectList {
            qname,
            name,
            parent: owner_text,
            items,
        });
        let hash = self.save_object(&object)?;
        Ok((qname, hash))
    }

    pub fn add_all(&self) -> Result<()> {
        let snapshot = self.worktree_snapshot()?;
        self.write_snapshot("index", &snapshot)
    }

    pub fn add(&self, qnames: &[String]) -> Result<()> {
        if qnames.is_empty() {
            return self.add_all();
        }
        let worktree = self.worktree_snapshot()?;
        let mut index = self.index_snapshot()?;
        for qname in qnames {
            let qname = SymbolHash::of(qname);
            match worktree.get(qname) {
                Some(hash) => index.insert(qname, hash),
                None => {
                    index.remove(qname);
                }
            }
        }
        self.write_snapshot("index", &index)
    }

    pub fn unstage(&self, qnames: &[String]) -> Result<()> {
        let head = self.head_snapshot()?;
        if qnames.is_empty() {
            return self.write_snapshot("index", &head);
        }
        let mut index = self.index_snapshot()?;
        for qname in qnames {
            let qname = SymbolHash::of(qname);
            match head.get(qname) {
                Some(hash) => index.insert(qname, hash),
                None => {
                    index.remove(qname);
                }
            }
        }
        self.write_snapshot("index", &index)
    }

    pub fn reset_paths(&self, qnames: &[String]) -> Result<()> {
        self.unstage(qnames)
    }

    pub fn remove_objects(&self, qnames: &[String], force: bool, recursive: bool) -> Result<()> {
        let mut snapshot = self.worktree_snapshot()?;
        let mut targets = BTreeSet::new();
        for qname in qnames {
            if matches!(
                qname.as_str(),
                "root" | "technical" | "requirement" | "concept"
            ) {
                bail!("root object {qname} cannot be removed");
            }
            let symbol = self.intern(qname)?;
            if snapshot.get(symbol).is_none() {
                bail!("object {qname} not found");
            }
            targets.insert(symbol);
        }

        let mut children: BTreeMap<SymbolHash, Vec<SymbolHash>> = BTreeMap::new();
        for (qname, hash) in snapshot.entries() {
            let parent = self.read(*hash)?.parent();
            if !parent.is_zero() {
                children.entry(parent).or_default().push(*qname);
            }
        }
        if recursive {
            let mut queue = targets.iter().copied().collect::<Vec<_>>();
            while let Some(qname) = queue.pop() {
                for child in children.get(&qname).cloned().unwrap_or_default() {
                    if targets.insert(child) {
                        queue.push(child);
                    }
                }
            }
        } else {
            for qname in &targets {
                if children.contains_key(qname) {
                    bail!(
                        "object {} has children; use --recursive",
                        self.resolve(*qname)?
                    );
                }
            }
        }

        let target_hashes = targets
            .iter()
            .filter_map(|qname| snapshot.get(*qname))
            .collect::<HashSet<_>>();

        let mut referrers = Vec::new();
        let mut containers = Vec::new();
        for (qname, hash) in snapshot.entries() {
            if targets.contains(qname) {
                continue;
            }
            let object = self.read(*hash)?;
            if object
                .references()
                .iter()
                .any(|reference| target_hashes.contains(reference))
            {
                match &object {
                    Object::ObjectList(_)
                    | Object::DocumentList(_)
                    | Object::DocumentParagraphList(_) => containers.push((*qname, object)),
                    _ => referrers.push((*qname, object)),
                }
            }
        }
        if !referrers.is_empty() && !force {
            let names = referrers
                .iter()
                .map(|(qname, _)| self.resolve(*qname))
                .collect::<Result<Vec<_>>>()?;
            bail!("objects are still referenced by: {}", names.join(", "));
        }
        for (qname, mut object) in referrers.into_iter().chain(containers) {
            for reference in target_hashes.iter().copied() {
                object.replace_reference(reference, ObjectHash::ZERO);
            }
            clean_zero_items(&mut object);
            let hash = self.save_object(&object)?;
            snapshot.insert(qname, hash);
        }

        for qname in targets {
            snapshot.remove(qname);
        }
        self.write_snapshot("worktree", &snapshot)?;
        Ok(())
    }

    pub fn reset(&self, hard: bool) -> Result<()> {
        let head = self.head_snapshot()?;
        self.write_snapshot("index", &head)?;
        if hard {
            self.write_snapshot("worktree", &head)?;
        }
        Ok(())
    }

    pub fn commit_index(&self, message: &str, allow_empty: bool) -> Result<CommitHash> {
        self.commit_index_as(message, allow_empty, "arche")
    }

    pub fn commit_index_as(
        &self,
        message: &str,
        allow_empty: bool,
        author: &str,
    ) -> Result<CommitHash> {
        let head = self.head()?;
        let head_snapshot = Snapshot::from_entries(head.objects.clone());
        let mut index = self.index_snapshot()?;
        if index == head_snapshot && !allow_empty {
            bail!("nothing to commit");
        }
        let qname = SymbolHash::of;
        let technical = index
            .get(qname("technical"))
            .context("missing technical root")?;
        let requirement = index
            .get(qname("requirement"))
            .context("missing requirement root")?;
        let concept = index
            .get(qname("concept"))
            .context("missing concept root")?;
        let source_commit = match self
            .worktree_snapshot()?
            .get(qname("root"))
            .and_then(|hash| self.read(hash).ok())
        {
            Some(Object::Root(value)) => value.source_commit,
            _ => match self.read(head.root)? {
                Object::Root(value) => value.source_commit,
                _ => GitCommit::ZERO,
            },
        };
        let root = Object::Root(Root {
            qname: qname("root"),
            name: qname("root"),
            source_commit,
            technical,
            requirement,
            concept,
        });
        let root_hash = self.save_object(&root)?;
        index.insert(qname("root"), root_hash);
        let commit = self.commit(
            Some(self.head_commit()?),
            root_hash,
            message.to_owned(),
            author,
            &index,
        )?;
        self.write_snapshot("index", &index)?;
        let mut worktree = self.worktree_snapshot()?;
        worktree.insert(qname("root"), root_hash);
        self.write_snapshot("worktree", &worktree)?;
        Ok(commit)
    }

    fn commit(
        &self,
        parent: Option<CommitHash>,
        root: ObjectHash,
        message: String,
        author: &str,
        objects: &Snapshot,
    ) -> Result<CommitHash> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default();
        let commit = Commit {
            parent,
            root,
            message,
            author: author.to_owned(),
            timestamp,
            objects: objects.entries().to_vec(),
        };
        let hash = self.commits.write(&commit)?;
        let branch = self.current_branch()?;
        self.write_ref(&format!("refs/heads/{branch}"), hash)?;
        Ok(hash)
    }

    fn write_ref(&self, name: &str, hash: CommitHash) -> Result<()> {
        let path = self.dir.join(name);
        write_atomic(&path, format!("{}\n", hash.to_hex()).as_bytes(), &self.tmp)
    }
}

fn source_cache_ready(cache: &Path) -> bool {
    if !cache.join("HEAD").is_file() {
        return false;
    }
    Command::new("git")
        .arg("--git-dir")
        .arg(cache)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn run_git(mut command: Command, label: &str) -> Result<()> {
    let output = command.output().with_context(|| format!("run {label}"))?;
    if !output.status.success() {
        bail!(
            "{label} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn validate_concept_path(path: &str, _directory: bool) -> Result<()> {
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() < 3 || segments[0] != "concept" {
        bail!("concept path must be concept/<domain|requirement|technical>/...");
    }
    if !matches!(segments[1], "domain" | "requirement" | "technical") {
        bail!(
            "concept path must be under concept/domain, concept/requirement, or concept/technical"
        );
    }
    if segments.iter().any(|segment| segment.is_empty()) {
        bail!("concept path contains an empty segment");
    }
    Ok(())
}

fn validate_branch_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.starts_with('-')
        || name.starts_with('/')
        || name.ends_with('/')
        || name.contains("..")
        || name.contains('\\')
        || name.chars().any(char::is_whitespace)
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        bail!("invalid branch name {name}");
    }
    Ok(())
}

fn object_file(object: &Object) -> Option<SymbolHash> {
    let file = match object {
        Object::TechnicalClass(value) => value.file().get(),
        Object::TechnicalStruct(value) => value.file().get(),
        Object::TechnicalEnum(value) => value.file().get(),
        Object::TechnicalInterface(value) => value.file().get(),
        Object::TechnicalFunction(value) => value.file().get(),
        Object::TechnicalMixin(value) => value.file().get(),
        _ => return None,
    };
    (!file.is_zero()).then_some(file)
}

fn object_source_path(object: &Object) -> Option<SymbolHash> {
    let path = match object {
        Object::TechnicalDirectory(value) => value.source().path,
        Object::TechnicalFile(value) => value.source().path,
        Object::TechnicalClass(value) => value.source().path,
        Object::TechnicalStruct(value) => value.source().path,
        Object::TechnicalEnum(value) => value.source().path,
        Object::TechnicalInterface(value) => value.source().path,
        Object::TechnicalMethod(value) => value.source().path,
        Object::TechnicalFunction(value) => value.source().path,
        Object::TechnicalMixin(value) => value.source().path,
        Object::TechnicalPart(value) => value.source().path,
        Object::TechnicalVariable(value) => value.source().path,
        Object::TechnicalConstant(value) => value.source().path,
        Object::TechnicalMember(value) => value.source().path,
        Object::TechnicalEnumValue(value) => value.source().path,
        Object::TechnicalArgument(value) => value.source().path,
        Object::TechnicalReturnValue(value) => value.source().path,
        _ => return None,
    };
    (!path.is_zero()).then_some(path)
}

fn parent_path(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(parent, _)| parent)
}

fn parse_git_commit(text: &str) -> Result<GitCommit> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid git commit {text}");
    }
    let raw = (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if raw.len() != 20 && raw.len() != 32 {
        bail!("git commit {text} must be a SHA-1 or SHA-256 object id");
    }
    let mut bytes = [0u8; 32];
    bytes[..raw.len()].copy_from_slice(&raw);
    Ok(GitCommit(Hash32::from_bytes(bytes)))
}

fn git_commit_hex(commit: GitCommit) -> String {
    let bytes = commit.0.as_bytes();
    let length = if bytes[20..].iter().all(|byte| *byte == 0) {
        20
    } else {
        32
    };
    let mut text = String::with_capacity(length * 2);
    for byte in &bytes[..length] {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn document_text(value: &Value) -> Result<String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Null => Ok(String::new()),
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                return Ok(text.to_owned());
            }
            if let Some(content) = map.get("content").and_then(Value::as_array) {
                return content.iter().map(document_text).collect();
            }
            if let Some(items) = map.get("items").and_then(Value::as_array) {
                return items.iter().map(document_text).collect();
            }
            bail!("document block has no text")
        }
        _ => bail!("document block must be text"),
    }
}

fn document_source(value: &Value) -> Result<String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Object(map) => {
            if let Some(source) = map.get("source").and_then(Value::as_str) {
                return Ok(source.to_owned());
            }
            document_text(value)
        }
        _ => document_text(value),
    }
}

fn validate_paragraph(text: &str, what: &str) -> Result<()> {
    if text.contains('\n') || text.contains('\r') {
        bail!("{what} must be a single paragraph without line breaks");
    }
    Ok(())
}

fn validate_local_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("name must not be empty");
    }
    if name.contains('/') || name.contains('.') || name.chars().any(char::is_whitespace) {
        bail!("name must not contain '/', '.', or whitespace");
    }
    Ok(())
}

fn validate_requirement_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        bail!("name must not be empty");
    }
    if name.contains('/') || name.contains('.') {
        bail!("name must not contain '/' or '.'");
    }
    Ok(())
}

impl Store for Repository {
    fn open_object(&self, hash: ObjectHash) -> Result<Box<dyn InputObject>> {
        self.objects.open(hash)
    }

    fn create_object(&self, version: u8, kind: ObjectKind) -> Result<Box<dyn OutputObject + '_>> {
        Ok(self.objects.create(version, kind))
    }

    fn read_symbol(&self, hash: SymbolHash) -> Result<String> {
        self.symbols.resolve(hash)
    }

    fn find_symbol(&self, text: &str) -> Result<SymbolHash> {
        self.symbols.find(text)
    }

    fn intern_symbol(&self, text: &str) -> Result<SymbolHash> {
        self.symbols.intern(text)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn init_creates_roots_and_commit() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let commit = repo.head().unwrap();
        assert_eq!(commit.message, "init");
        assert!(repo.find_by_qname("technical").is_ok());
        assert!(repo.find_by_qname("requirement").is_ok());
        assert!(repo.find_by_qname("concept").is_ok());
    }

    #[test]
    fn checkout_tracks_a_fetched_remote_branch() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let head = repo.head_commit().unwrap();
        crate::protocol::write_ref_file(
            repo.dir(),
            "refs/remotes/origin/project-document-test",
            head,
        )
        .unwrap();
        repo.checkout("project-document-test", false).unwrap();
        assert_eq!(
            repo.current_branch().unwrap(),
            "project-document-test".to_string()
        );
    }

    #[test]
    fn create_system_updates_worktree_then_commits() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let (qname, _) = repo.create_technical_system("app").unwrap();
        assert_eq!(repo.resolve(qname).unwrap(), "technical/app");
        assert!(repo.find_by_qname("technical/app").is_ok());
        assert_eq!(repo.log().unwrap().len(), 1);

        repo.add_all().unwrap();
        repo.commit_index("add app", false).unwrap();
        let log = repo.log().unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].1.message, "add app");
    }

    #[test]
    fn comments_are_stored_outside_object_graph() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        repo.create_technical_system("app").unwrap();
        repo.add_all().unwrap();
        repo.commit_index("add app", false).unwrap();
        let before = repo.find_by_qname("technical/app").unwrap();
        let comment = repo.comment_add("technical/app", "补充错误码").unwrap();
        assert_eq!(comment.id, 1);
        assert_eq!(comment.target, "technical/app");
        assert_eq!(repo.find_by_qname("technical/app").unwrap(), before);
        let comments = repo.comment_list(Some("technical/app"), false).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].body, "补充错误码");
        repo.comment_resolve(comment.id, "已补充错误码", "reviewer-agent")
            .unwrap();
        let comments = repo.comment_list(None, false).unwrap();
        assert!(comments[0].resolved);
        assert_eq!(comments[0].conclusion, "已补充错误码");
        assert_eq!(comments[0].resolved_by, "reviewer-agent");
    }

    #[test]
    fn comments_can_be_listed_for_a_subtree() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        repo.create_technical_namespace("technical", "ns").unwrap();
        repo.create_technical_class(None, Some("technical/namespace/ns"), "Foo")
            .unwrap();
        repo.add_all().unwrap();
        repo.commit_index("add ns", false).unwrap();
        repo.comment_add("technical/namespace/ns", "命名空间评论")
            .unwrap();
        let class = repo
            .comment_add("technical/namespace/ns/Foo", "类型评论")
            .unwrap();

        assert_eq!(
            repo.comment_list(Some("technical/namespace/ns"), false)
                .unwrap()
                .len(),
            1
        );
        let subtree = repo
            .comment_list_subtree("technical/namespace/ns", false)
            .unwrap();
        assert_eq!(subtree.len(), 2);
        assert!(
            subtree
                .iter()
                .any(|comment| comment.target == "technical/namespace/ns/Foo")
        );

        repo.comment_resolve(class.id, "已处理", "reviewer")
            .unwrap();
        assert_eq!(
            repo.comment_list_subtree("technical/namespace/ns", true)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn comments_are_branch_scoped_and_merge_across_workspaces() {
        let source_temp = TempDir::new().unwrap();
        let source = Repository::init(source_temp.path()).unwrap();
        source.create_technical_system("app").unwrap();
        source.add_all().unwrap();
        source.commit_index("add app", false).unwrap();
        source.comment_add("technical/app", "主分支评论").unwrap();
        source.branch_create("feature").unwrap();
        source.checkout("feature", false).unwrap();
        source.comment_add("technical/app", "功能分支评论").unwrap();
        assert_eq!(
            source.comment_list(None, false).unwrap().len(),
            1,
            "comment list must be scoped to the current branch"
        );
        assert_eq!(
            source.comment_list_on("main", None, false).unwrap().len(),
            1
        );
        let exported = source.export_comments().unwrap();

        let target_temp = TempDir::new().unwrap();
        let target = Repository::init(target_temp.path()).unwrap();
        target.create_technical_system("app").unwrap();
        target.add_all().unwrap();
        target.commit_index("add app", false).unwrap();
        target.comment_add("technical/app", "本地评论").unwrap();
        target.import_comments(&exported, false).unwrap();

        let main = target.comment_list_on("main", None, false).unwrap();
        assert_eq!(main.len(), 2);
        assert_ne!(main[0].id, main[1].id);
        assert_eq!(
            target.comment_list_on("feature", None, false).unwrap()[0].body,
            "功能分支评论"
        );
    }

    #[test]
    fn concepts_can_share_a_name_across_dimensions() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        repo.concept_create("concept/domain/agent", "智能体", "现实中的智能机器")
            .unwrap();
        repo.concept_create(
            "concept/requirement/agent",
            "智能体",
            "用户感知到的智能虚拟对象",
        )
        .unwrap();
        repo.concept_create("concept/technical/agent", "智能体", "程序中的内存对象")
            .unwrap();
        let concept = repo.concept_list().unwrap();
        assert_eq!(concept.len(), 3);
        let names = concept
            .iter()
            .map(|(_, concept)| concept.display_name.clone())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["智能体", "智能体", "智能体"]);
        assert!(repo.check().unwrap().is_empty());
    }

    #[test]
    fn slash_separated_branches_are_created_listed_and_deleted() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        repo.branch_create("requirements/abc").unwrap();
        repo.checkout("requirements/abc", false).unwrap();
        assert_eq!(repo.current_branch().unwrap(), "requirements/abc");
        assert!(
            repo.branch_list()
                .unwrap()
                .contains(&"requirements/abc".to_string())
        );
        repo.checkout("main", false).unwrap();
        repo.branch_delete("requirements/abc").unwrap();
        assert!(
            !repo
                .branch_list()
                .unwrap()
                .contains(&"requirements/abc".to_string())
        );
        repo.branch_delete("requirements/abc").unwrap();
    }

    #[test]
    fn requirement_story_v2_links_users_functions_protagonist_and_flow() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let (user, _) = repo.create_requirement_user("protagonist", "主角").unwrap();
        let user = repo.resolve(user).unwrap();
        repo.create_requirement_system("系统").unwrap();
        repo.create_requirement_module("requirement/system/系统", "模块")
            .unwrap();
        let (function, _) = repo
            .create_requirement_function("requirement/system/系统/模块", "功能")
            .unwrap();
        let function = repo.resolve(function).unwrap();
        let (story, _) = repo.create_requirement_story("旅程").unwrap();
        let story = repo.resolve(story).unwrap();

        repo.set_refs(&story, "users", &[user.clone()]).unwrap();
        repo.set_refs(&story, "protagonist", &[user]).unwrap();
        repo.set_refs(&story, "functions", &[function]).unwrap();
        repo.doc_mermaid_set(&story, "flow", "graph TD; A-->B;")
            .unwrap();
        repo.import_doc_json(
            &story,
            r#"{"fields":{
                "description":{"type":"DocumentParagraphList","items":[
                    {"type":"DocumentText","content":[{"type":"text","text":"主角打开列表，选择一项并完成操作。"}]},
                    {"type":"DocumentText","content":[{"type":"text","text":"其他用户收到结果并确认。"}]}
                ]},
                "requirement":{"type":"DocumentList","items":[
                    {"type":"text","text":"满足主角快速完成操作的需求。"}
                ]}
            }}"#,
        )
        .unwrap();

        let hash = repo.find_by_qname(&story).unwrap();
        let Object::RequirementStory(story) = repo.read(hash).unwrap() else {
            panic!("not a story");
        };
        assert_eq!(story.users().len(), 1);
        assert_eq!(story.functions().len(), 1);
        assert!(!story.protagonist().is_zero());
        assert!(!story.flow().is_zero());
        assert!(story.description().get() != ObjectHash::ZERO);
        assert!(story.requirement().get() != ObjectHash::ZERO);
    }

    #[test]
    fn requirement_users_are_appended_to_the_root_user_list() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let (user, hash) = repo
            .create_requirement_user("project-sponsor", "项目发起人")
            .unwrap();
        let qname = repo.resolve(user).unwrap();
        assert!(qname.starts_with("requirement/user/"), "{qname}");
        let root = repo.find_by_qname("requirement").unwrap();
        let root = repo.read(root).unwrap();
        let Object::RequirementRoot(root) = root else {
            panic!("requirement is not a root");
        };
        let list = repo.read(root.user).unwrap();
        let Object::ObjectList(list) = list else {
            panic!("requirement/user is not a list");
        };
        assert_eq!(list.items, vec![hash]);
        assert!(repo.find_by_qname(&qname).is_ok());

        repo.import_doc_json(
            &qname,
            r#"{"fields":{
                "display_name":"修改后的发起人",
                "priority":2,
                "needs":{"type":"DocumentList","items":[{"type":"text","text":"能够初始化平台"}]}
            }}"#,
        )
        .unwrap();
        repo.set_scalar(&qname, "occupation", "管理员").unwrap();
        let hash = repo.find_by_qname(&qname).unwrap();
        let Object::RequirementUser(user) = repo.read(hash).unwrap() else {
            panic!("not a user");
        };
        assert_eq!(user.display_name(), "修改后的发起人");
        assert_eq!(user.occupation(), "管理员");
        assert_eq!(user.priority(), 2);
        assert!(!user.needs().is_zero());
    }

    #[test]
    fn requirement_hierarchy_is_explicit() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let (system, _) = repo.create_requirement_system("仓库系统").unwrap();
        let (module, _) = repo
            .create_requirement_module("requirement/system/仓库系统", "订单管理")
            .unwrap();
        let (function, _) = repo
            .create_requirement_function("requirement/system/仓库系统/订单管理", "创建订单")
            .unwrap();
        let (interaction, _) = repo
            .create_requirement_interaction(
                "requirement/system/仓库系统/订单管理/创建订单",
                "填写订单",
            )
            .unwrap();
        assert_eq!(repo.resolve(system).unwrap(), "requirement/system/仓库系统");
        assert_eq!(
            repo.resolve(module).unwrap(),
            "requirement/system/仓库系统/订单管理"
        );
        assert_eq!(
            repo.resolve(function).unwrap(),
            "requirement/system/仓库系统/订单管理/创建订单"
        );
        assert_eq!(
            repo.resolve(interaction).unwrap(),
            "requirement/system/仓库系统/订单管理/创建订单/填写订单"
        );
        assert!(
            repo.find_by_qname("requirement/system/仓库系统/订单管理")
                .is_ok()
        );
    }

    #[test]
    fn document_fields_are_typed_and_authorable() {
        let temp = TempDir::new().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let system = "requirement/system/仓库系统";
        let module = "requirement/system/仓库系统/订单管理";
        let function = "requirement/system/仓库系统/订单管理/创建订单";
        let interaction = "requirement/system/仓库系统/订单管理/创建订单/填写订单";
        repo.create_requirement_system("仓库系统").unwrap();
        repo.create_requirement_module(system, "订单管理").unwrap();
        repo.create_requirement_function(module, "创建订单")
            .unwrap();
        repo.create_requirement_interaction(function, "填写订单")
            .unwrap();
        let story = "requirement/story/order-story";
        repo.create_requirement_story("order-story").unwrap();

        repo.doc_mermaid_set(function, "flow", "flowchart TD\nA-->B")
            .unwrap();
        let (html_qname, _) = repo
            .doc_html_add(interaction, "ui", "<div>UI</div>")
            .unwrap();
        repo.doc_list_add(story, "requirement").unwrap();
        let (requirement_item, _) = repo
            .doc_list_item_add(story, "requirement", "满足创建订单的需求")
            .unwrap();
        let (topic_qname, _) = repo
            .doc_topic_add(function, "description", "创建订单")
            .unwrap();

        assert!(repo.find_by_qname(&format!("{function}/flow")).is_ok());
        assert!(
            repo.resolve(html_qname)
                .unwrap()
                .starts_with(&format!("{interaction}/"))
        );
        assert!(
            repo.resolve(requirement_item)
                .unwrap()
                .starts_with(&format!("{story}/"))
        );
        assert!(
            repo.resolve(topic_qname)
                .unwrap()
                .starts_with(&format!("{function}/"))
        );
    }
}
