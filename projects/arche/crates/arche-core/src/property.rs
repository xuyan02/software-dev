use std::fmt;

use crate::hash::{GitCommit, ObjectHash, SymbolHash};
use crate::object::{
    Concept, ConceptDirectory, DocumentCode, DocumentHtml, DocumentList, DocumentMermaid,
    DocumentParagraphList, DocumentText, DocumentTopic, Object, ObjectList, ProjectDocument,
    ProjectRoot, RequirementFunction, RequirementInteraction, RequirementModule, RequirementRoot,
    RequirementStory, RequirementSystem, RequirementUser, Root, TechnicalArgument, TechnicalClass,
    TechnicalConstant, TechnicalDirectory, TechnicalEnum, TechnicalEnumValue, TechnicalFile,
    TechnicalFunction, TechnicalInterface, TechnicalLayer, TechnicalMember, TechnicalMethod,
    TechnicalMixin, TechnicalModule, TechnicalNamespace, TechnicalPart, TechnicalProperty,
    TechnicalReturnValue, TechnicalRoot, TechnicalStruct, TechnicalSystem, TechnicalVariable,
};
use crate::refs::StringRefList;
use crate::source::{SourcePath, SourceSpan};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropertyValue {
    Bool(bool),
    String(String),
    Int(i64),
    GitCommit(GitCommit),
    StringRef(SymbolHash),
    StringRefList(StringRefList),
    ObjectRef(ObjectHash),
    SourcePath(SourcePath),
    SourceSpan(SourceSpan),
}

impl PropertyValue {
    pub fn kind(&self) -> ValueKind {
        match self {
            Self::Bool(_) => ValueKind::Bool,
            Self::String(_) => ValueKind::String,
            Self::Int(_) => ValueKind::Int,
            Self::GitCommit(_) => ValueKind::GitCommit,
            Self::StringRef(_) => ValueKind::StringRef,
            Self::StringRefList(_) => ValueKind::StringRefList,
            Self::ObjectRef(_) => ValueKind::ObjectRef,
            Self::SourcePath(_) => ValueKind::SourcePath,
            Self::SourceSpan(_) => ValueKind::SourceSpan,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    Bool,
    String,
    Int,
    GitCommit,
    StringRef,
    StringRefList,
    ObjectRef,
    SourcePath,
    SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertySpec {
    pub name: String,
    pub kind: ValueKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyError {
    UnknownProperty(String),
    ReadOnly(String),
    TypeMismatch {
        property: String,
        expected: ValueKind,
        actual: ValueKind,
    },
    ValueOutOfRange(String),
}

impl fmt::Display for PropertyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProperty(property) => write!(f, "unknown property {property}"),
            Self::ReadOnly(property) => write!(f, "property {property} is read-only"),
            Self::TypeMismatch {
                property,
                expected,
                actual,
            } => write!(
                f,
                "property {property} expects {expected:?}, found {actual:?}"
            ),
            Self::ValueOutOfRange(property) => write!(f, "value out of range for {property}"),
        }
    }
}

impl std::error::Error for PropertyError {}

pub trait PropertyAccess {
    fn properties(&self) -> Vec<PropertySpec>;

    fn get_property(&self, name: &str) -> Result<PropertyValue, PropertyError>;

    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), PropertyError>;
}

fn type_mismatch(property: &str, expected: ValueKind, value: &PropertyValue) -> PropertyError {
    PropertyError::TypeMismatch {
        property: property.to_owned(),
        expected,
        actual: value.kind(),
    }
}

trait PropertyField: Sized {
    fn to_property(&self) -> PropertyValue;

    fn set_property(&mut self, property: &str, value: PropertyValue) -> Result<(), PropertyError>;
}

impl PropertyField for ObjectHash {
    fn to_property(&self) -> PropertyValue {
        PropertyValue::ObjectRef(*self)
    }

    fn set_property(&mut self, property: &str, value: PropertyValue) -> Result<(), PropertyError> {
        match value {
            PropertyValue::ObjectRef(value) => {
                *self = value;
                Ok(())
            }
            value => Err(type_mismatch(property, ValueKind::ObjectRef, &value)),
        }
    }
}

impl PropertyField for SymbolHash {
    fn to_property(&self) -> PropertyValue {
        PropertyValue::StringRef(*self)
    }

    fn set_property(&mut self, property: &str, value: PropertyValue) -> Result<(), PropertyError> {
        match value {
            PropertyValue::StringRef(value) => {
                *self = value;
                Ok(())
            }
            value => Err(type_mismatch(property, ValueKind::StringRef, &value)),
        }
    }
}

impl PropertyField for Vec<SymbolHash> {
    fn to_property(&self) -> PropertyValue {
        PropertyValue::StringRefList(StringRefList(self.clone()))
    }

    fn set_property(&mut self, property: &str, value: PropertyValue) -> Result<(), PropertyError> {
        match value {
            PropertyValue::StringRefList(value) => {
                *self = value.0;
                Ok(())
            }
            value => Err(type_mismatch(property, ValueKind::StringRefList, &value)),
        }
    }
}

impl PropertyField for GitCommit {
    fn to_property(&self) -> PropertyValue {
        PropertyValue::GitCommit(*self)
    }

    fn set_property(&mut self, property: &str, value: PropertyValue) -> Result<(), PropertyError> {
        match value {
            PropertyValue::GitCommit(value) => {
                *self = value;
                Ok(())
            }
            value => Err(type_mismatch(property, ValueKind::GitCommit, &value)),
        }
    }
}

impl PropertyField for i64 {
    fn to_property(&self) -> PropertyValue {
        PropertyValue::Int(*self)
    }

    fn set_property(&mut self, property: &str, value: PropertyValue) -> Result<(), PropertyError> {
        match value {
            PropertyValue::Int(value) => {
                *self = value;
                Ok(())
            }
            value => Err(type_mismatch(property, ValueKind::Int, &value)),
        }
    }
}

impl PropertyField for u32 {
    fn to_property(&self) -> PropertyValue {
        PropertyValue::Int(i64::from(*self))
    }

    fn set_property(&mut self, property: &str, value: PropertyValue) -> Result<(), PropertyError> {
        match value {
            PropertyValue::Int(value) => {
                *self = u32::try_from(value)
                    .map_err(|_| PropertyError::ValueOutOfRange(property.to_owned()))?;
                Ok(())
            }
            value => Err(type_mismatch(property, ValueKind::Int, &value)),
        }
    }
}

impl PropertyField for SourceSpan {
    fn to_property(&self) -> PropertyValue {
        PropertyValue::SourceSpan(*self)
    }

    fn set_property(&mut self, property: &str, value: PropertyValue) -> Result<(), PropertyError> {
        match value {
            PropertyValue::SourceSpan(value) => {
                *self = value;
                Ok(())
            }
            value => Err(type_mismatch(property, ValueKind::SourceSpan, &value)),
        }
    }
}

impl PropertyField for String {
    fn to_property(&self) -> PropertyValue {
        PropertyValue::String(self.clone())
    }

    fn set_property(&mut self, property: &str, value: PropertyValue) -> Result<(), PropertyError> {
        match value {
            PropertyValue::String(value) => {
                *self = value;
                Ok(())
            }
            value => Err(type_mismatch(property, ValueKind::String, &value)),
        }
    }
}

impl PropertyField for u8 {
    fn to_property(&self) -> PropertyValue {
        PropertyValue::Int(i64::from(*self))
    }

    fn set_property(&mut self, property: &str, value: PropertyValue) -> Result<(), PropertyError> {
        match value {
            PropertyValue::Int(value) => {
                *self = u8::try_from(value)
                    .map_err(|_| PropertyError::ValueOutOfRange(property.to_owned()))?;
                Ok(())
            }
            value => Err(type_mismatch(property, ValueKind::Int, &value)),
        }
    }
}

impl PropertyField for bool {
    fn to_property(&self) -> PropertyValue {
        PropertyValue::Bool(*self)
    }

    fn set_property(&mut self, property: &str, value: PropertyValue) -> Result<(), PropertyError> {
        match value {
            PropertyValue::Bool(value) => {
                *self = value;
                Ok(())
            }
            value => Err(type_mismatch(property, ValueKind::Bool, &value)),
        }
    }
}

macro_rules! impl_properties {
    ($ty:ty { $(source: $source_kind:ident;)? $( $name:literal => $field:ident : $kind:ident ),* $(,)? }) => {
        impl PropertyAccess for $ty {
            fn properties(&self) -> Vec<PropertySpec> {
                let mut specs = vec![
                    PropertySpec {
                        name: "qname".to_owned(),
                        kind: ValueKind::StringRef,
                    },
                    PropertySpec {
                        name: "name".to_owned(),
                        kind: ValueKind::StringRef,
                    },
                ];
                $( specs.push(PropertySpec {
                    name: "source".to_owned(),
                    kind: ValueKind::$source_kind,
                }); )?
                specs.extend(vec![
                    $( PropertySpec { name: $name.to_string(), kind: ValueKind::$kind }, )*
                ]);
                specs
            }

            fn get_property(&self, name: &str) -> Result<PropertyValue, PropertyError> {
                match name {
                    "qname" => Ok(PropertyValue::StringRef(self.qname)),
                    "name" => Ok(PropertyValue::StringRef(self.name)),
                    $(
                        "source" => Ok(PropertyValue::$source_kind(self.source)),
                    )?
                    $( $name => Ok(PropertyField::to_property(&self.$field)), )*
                    _ => Err(PropertyError::UnknownProperty(name.to_owned())),
                }
            }

            fn set_property(
                &mut self,
                name: &str,
                value: PropertyValue,
            ) -> Result<(), PropertyError> {
                let _ = &value;
                match name {
                    "qname" | "name" => Err(PropertyError::ReadOnly(name.to_owned())),
                    $(
                        "source" => match value {
                            PropertyValue::$source_kind(value) => {
                                self.source = value;
                                Ok(())
                            }
                            value => Err(type_mismatch(
                                "source",
                                ValueKind::$source_kind,
                                &value,
                            )),
                        },
                    )?
                    $(
                        $name => {
                            PropertyField::set_property(&mut self.$field, name, value)?;
                            Ok(())
                        }
                    )*
                    _ => Err(PropertyError::UnknownProperty(name.to_owned())),
                }
            }
        }
    };
}

impl_properties!(TechnicalRoot {
    "file" => file : ObjectRef,
    "directory" => directory : ObjectRef,
    "namespace" => namespace : ObjectRef,
    "system" => system : ObjectRef,
});

impl_properties!(RequirementRoot {
    "user" => user : ObjectRef,
    "story" => story : ObjectRef,
    "system" => system : ObjectRef,
});

impl_properties!(ConceptDirectory {
    "parent" => parent : StringRef,
    "directories" => directories : ObjectRef,
    "concepts" => concepts : ObjectRef,
});

impl_properties!(TechnicalSystem {
    "parent" => parent : StringRef,
    "members" => members : ObjectRef,
    "modules" => modules : ObjectRef,
    "layers" => layers : ObjectRef,
    "constants" => constants : ObjectRef,
    "variables" => variables : ObjectRef,
    "position" => position : ObjectRef,
    "responsibilities" => responsibilities : ObjectRef,
    "non_responsibilities" => non_responsibilities : ObjectRef,
    "features" => features : ObjectRef,
    "non_features" => non_features : ObjectRef,
    "invariants" => invariants : ObjectRef,
    "constraints" => constraints : ObjectRef,
    "design_principles" => design_principles : ObjectRef,
    "external_dependencies" => external_dependencies : ObjectRef,
    "usage" => usage : ObjectRef,
    "design_points" => design_points : ObjectRef,
});

impl_properties!(TechnicalModule {
    "parent" => parent : StringRef,
    "members" => members : ObjectRef,
    "dependencies" => dependencies : StringRefList,
    "constants" => constants : ObjectRef,
    "variables" => variables : ObjectRef,
    "position" => position : ObjectRef,
    "responsibilities" => responsibilities : ObjectRef,
    "non_responsibilities" => non_responsibilities : ObjectRef,
    "features" => features : ObjectRef,
    "non_features" => non_features : ObjectRef,
    "invariants" => invariants : ObjectRef,
    "constraints" => constraints : ObjectRef,
    "design_principles" => design_principles : ObjectRef,
    "usage" => usage : ObjectRef,
    "design_points" => design_points : ObjectRef,
});

impl_properties!(Root {
    "source_commit" => source_commit : GitCommit,
    "technical" => technical : ObjectRef,
    "requirement" => requirement : ObjectRef,
    "concept" => concept : ObjectRef,
});

impl_properties!(ProjectRoot {
    "parent" => parent : StringRef,
    "document" => document : ObjectRef,
});

impl_properties!(ProjectDocument {
    "parent" => parent : StringRef,
    "document" => document : ObjectRef,
});

impl_properties!(ObjectList {
    "parent" => parent : StringRef,
});

impl_properties!(DocumentText {});

impl_properties!(DocumentList {
    "parent" => parent : StringRef,
});

impl_properties!(DocumentParagraphList {});

impl_properties!(DocumentTopic {});

impl_properties!(DocumentCode {});

impl_properties!(DocumentMermaid {});

impl_properties!(DocumentHtml {});

impl_properties!(Concept {
    "display_name" => display_name : String,
    "explanation" => explanation : ObjectRef,
});

impl_properties!(RequirementUser {
    "display_name" => display_name : String,
    "age" => age : String,
    "occupation" => occupation : String,
    "situations" => situations : ObjectRef,
    "hobbies" => hobbies : ObjectRef,
    "needs" => needs : ObjectRef,
    "priority" => priority : Int,
    "stories" => stories : StringRefList,
});

impl_properties!(RequirementStory {
    "preconditions" => preconditions : ObjectRef,
    "frequency" => frequency : Int,
    "description" => description : ObjectRef,
    "requirement" => requirement : ObjectRef,
    "users" => users : StringRefList,
    "functions" => functions : StringRefList,
    "protagonist" => protagonist : StringRef,
    "flow" => flow : ObjectRef,
});

impl_properties!(RequirementSystem {
    "parent" => parent : StringRef,
    "display_name" => display_name : String,
    "position" => position : ObjectRef,
    "app_form" => app_form : ObjectRef,
    "users" => users : StringRefList,
    "modules" => modules : ObjectRef,
});

impl_properties!(RequirementModule {
    "parent" => parent : StringRef,
    "display_name" => display_name : String,
    "position" => position : ObjectRef,
    "functions" => functions : ObjectRef,
});

impl_properties!(RequirementFunction {
    "parent" => parent : StringRef,
    "flow" => flow : ObjectRef,
    "interactions" => interactions : ObjectRef,
    "initiator" => initiator : ObjectRef,
    "system_change" => system_change : ObjectRef,
    "preconditions" => preconditions : ObjectRef,
    "feedback" => feedback : ObjectRef,
    "description" => description : ObjectRef,
});

impl_properties!(RequirementInteraction {
    "parent" => parent : StringRef,
    "description" => description : ObjectRef,
    "ui" => ui : ObjectRef,
});

impl_properties!(TechnicalDirectory {
    source: SourcePath;
    "parent" => parent : StringRef,
    "children" => children : ObjectRef,
    "position" => position : ObjectRef,
});

impl_properties!(TechnicalFile {
    source: SourcePath;
    "parent" => parent : StringRef,
    "entities" => entities : ObjectRef,
    "position" => position : ObjectRef,
});

impl_properties!(TechnicalLayer {
    "parent" => parent : StringRef,
    "members" => members : ObjectRef,
    "dependencies" => dependencies : StringRefList,
    "position" => position : ObjectRef,
});

impl_properties!(TechnicalNamespace {
    "parent" => parent : StringRef,
    "children" => children : ObjectRef,
    "entities" => entities : ObjectRef,
    "constants" => constants : ObjectRef,
    "variables" => variables : ObjectRef,
    "position" => position : ObjectRef,
});

impl_properties!(TechnicalClass {
    source: SourceSpan;
    "parent" => parent : StringRef,
    "namespace" => namespace : StringRef,
    "file" => file : StringRef,
    "module" => module : StringRef,
    "superclass" => superclass : StringRef,
    "interfaces" => interfaces : StringRefList,
    "mixins" => mixins : StringRefList,
    "explicit_dependencies" => explicit_dependencies : StringRefList,
    "implicit_dependencies" => implicit_dependencies : StringRefList,
    "method" => method : ObjectRef,
    "types" => types : ObjectRef,
    "parts" => parts : ObjectRef,
    "constants" => constants : ObjectRef,
    "members" => members : ObjectRef,
    "properties" => properties : ObjectRef,
    "position" => position : ObjectRef,
    "responsibilities" => responsibilities : ObjectRef,
    "non_responsibilities" => non_responsibilities : ObjectRef,
    "features" => features : ObjectRef,
    "non_features" => non_features : ObjectRef,
    "invariants" => invariants : ObjectRef,
    "constraints" => constraints : ObjectRef,
    "design_principles" => design_principles : ObjectRef,
    "usage" => usage : ObjectRef,
    "design_points" => design_points : ObjectRef,
});

impl_properties!(TechnicalStruct {
    source: SourceSpan;
    "parent" => parent : StringRef,
    "namespace" => namespace : StringRef,
    "file" => file : StringRef,
    "module" => module : StringRef,
    "dependencies" => dependencies : StringRefList,
    "method" => method : ObjectRef,
    "types" => types : ObjectRef,
    "constants" => constants : ObjectRef,
    "members" => members : ObjectRef,
    "position" => position : ObjectRef,
    "invariants" => invariants : ObjectRef,
});

impl_properties!(TechnicalEnum {
    source: SourceSpan;
    "parent" => parent : StringRef,
    "namespace" => namespace : StringRef,
    "file" => file : StringRef,
    "module" => module : StringRef,
    "dependencies" => dependencies : StringRefList,
    "values" => values : ObjectRef,
    "position" => position : ObjectRef,
});

impl_properties!(TechnicalEnumValue {
    source: SourceSpan;
    "parent" => parent : StringRef,
    "position" => position : ObjectRef,
});

impl_properties!(TechnicalArgument {
    source: SourceSpan;
    "parent" => parent : StringRef,
    "ty" => ty : StringRef,
    "position" => position : ObjectRef,
});

impl_properties!(TechnicalReturnValue {
    source: SourceSpan;
    "parent" => parent : StringRef,
    "ty" => ty : StringRef,
    "position" => position : ObjectRef,
});

impl_properties!(TechnicalInterface {
    source: SourceSpan;
    "parent" => parent : StringRef,
    "namespace" => namespace : StringRef,
    "file" => file : StringRef,
    "module" => module : StringRef,
    "extends" => extends : StringRef,
    "dependencies" => dependencies : StringRefList,
    "methods" => methods : ObjectRef,
    "position" => position : ObjectRef,
    "responsibilities" => responsibilities : ObjectRef,
    "non_responsibilities" => non_responsibilities : ObjectRef,
    "features" => features : ObjectRef,
    "non_features" => non_features : ObjectRef,
    "invariants" => invariants : ObjectRef,
    "usage" => usage : ObjectRef,
    "design_points" => design_points : ObjectRef,
});

impl_properties!(TechnicalMethod {
    source: SourceSpan;
    "parent" => parent : StringRef,
    "is_static" => is_static : Bool,
    "is_async" => is_async : Bool,
    "is_abstract" => is_abstract : Bool,
    "is_const" => is_const : Bool,
    "overrides" => overrides : StringRef,
    "explicit_dependencies" => explicit_dependencies : StringRefList,
    "implicit_dependencies" => implicit_dependencies : StringRefList,
    "position" => position : ObjectRef,
    "arguments" => arguments : ObjectRef,
    "return_value" => return_value : ObjectRef,
    "usage" => usage : ObjectRef,
    "design_points" => design_points : ObjectRef,
});

impl_properties!(TechnicalFunction {
    source: SourceSpan;
    "namespace" => namespace : StringRef,
    "file" => file : StringRef,
    "module" => module : StringRef,
    "is_async" => is_async : Bool,
    "explicit_dependencies" => explicit_dependencies : StringRefList,
    "implicit_dependencies" => implicit_dependencies : StringRefList,
    "position" => position : ObjectRef,
    "arguments" => arguments : ObjectRef,
    "return_value" => return_value : ObjectRef,
    "usage" => usage : ObjectRef,
    "design_points" => design_points : ObjectRef,
});

impl_properties!(TechnicalMixin {
    source: SourceSpan;
    "parent" => parent : StringRef,
    "namespace" => namespace : StringRef,
    "file" => file : StringRef,
    "module" => module : StringRef,
    "on" => on : StringRefList,
    "superclass" => superclass : StringRef,
    "interfaces" => interfaces : StringRefList,
    "mixins" => mixins : StringRefList,
    "explicit_dependencies" => explicit_dependencies : StringRefList,
    "implicit_dependencies" => implicit_dependencies : StringRefList,
    "methods" => methods : ObjectRef,
    "types" => types : ObjectRef,
    "position" => position : ObjectRef,
    "responsibilities" => responsibilities : ObjectRef,
    "non_responsibilities" => non_responsibilities : ObjectRef,
    "features" => features : ObjectRef,
    "non_features" => non_features : ObjectRef,
    "invariants" => invariants : ObjectRef,
    "usage" => usage : ObjectRef,
    "design_points" => design_points : ObjectRef,
});

impl_properties!(TechnicalPart {
    source: SourceSpan;
    "parent" => parent : StringRef,
    "explicit_dependencies" => explicit_dependencies : StringRefList,
    "implicit_dependencies" => implicit_dependencies : StringRefList,
    "members" => members : ObjectRef,
    "methods" => methods : ObjectRef,
    "types" => types : ObjectRef,
});

impl_properties!(TechnicalVariable {
    source: SourceSpan;
    "parent" => parent : StringRef,
    "ty" => ty : StringRef,
    "position" => position : ObjectRef,
});

impl_properties!(TechnicalConstant {
    source: SourceSpan;
    "parent" => parent : StringRef,
    "ty" => ty : StringRef,
    "position" => position : ObjectRef,
});

impl_properties!(TechnicalMember {
    source: SourceSpan;
    "parent" => parent : StringRef,
    "ty" => ty : StringRef,
    "is_static" => is_static : Bool,
    "is_const" => is_const : Bool,
    "position" => position : ObjectRef,
});

impl_properties!(TechnicalProperty {
    "parent" => parent : StringRef,
    "ty" => ty : StringRef,
    "getter" => getter : SourceSpan,
    "setter" => setter : SourceSpan,
    "position" => position : ObjectRef,
});

impl PropertyAccess for Object {
    fn properties(&self) -> Vec<PropertySpec> {
        match self {
            Self::TechnicalRoot(value) => value.properties(),
            Self::RequirementRoot(value) => value.properties(),
            Self::ConceptDirectory(value) => value.properties(),
            Self::TechnicalSystem(value) => value.properties(),
            Self::TechnicalModule(value) => value.properties(),
            Self::Root(value) => value.properties(),
            Self::ObjectList(value) => value.properties(),
            Self::DocumentText(value) => value.properties(),
            Self::DocumentList(value) => value.properties(),
            Self::DocumentParagraphList(value) => value.properties(),
            Self::DocumentTopic(value) => value.properties(),
            Self::DocumentCode(value) => value.properties(),
            Self::DocumentMermaid(value) => value.properties(),
            Self::DocumentHtml(value) => value.properties(),
            Self::Concept(value) => value.properties(),
            Self::RequirementUser(value) => value.properties(),
            Self::RequirementStory(value) => value.properties(),
            Self::RequirementSystem(value) => value.properties(),
            Self::RequirementModule(value) => value.properties(),
            Self::RequirementFunction(value) => value.properties(),
            Self::RequirementInteraction(value) => value.properties(),
            Self::TechnicalDirectory(value) => value.properties(),
            Self::TechnicalFile(value) => value.properties(),
            Self::TechnicalLayer(value) => value.properties(),
            Self::TechnicalNamespace(value) => value.properties(),
            Self::TechnicalClass(value) => PropertyAccess::properties(value),
            Self::TechnicalStruct(value) => value.properties(),
            Self::TechnicalEnum(value) => value.properties(),
            Self::TechnicalInterface(value) => value.properties(),
            Self::TechnicalMethod(value) => value.properties(),
            Self::TechnicalFunction(value) => value.properties(),
            Self::TechnicalMixin(value) => value.properties(),
            Self::TechnicalPart(value) => value.properties(),
            Self::TechnicalVariable(value) => value.properties(),
            Self::TechnicalConstant(value) => value.properties(),
            Self::TechnicalMember(value) => value.properties(),
            Self::TechnicalProperty(value) => value.properties(),
            Self::TechnicalEnumValue(value) => value.properties(),
            Self::TechnicalArgument(value) => value.properties(),
            Self::TechnicalReturnValue(value) => value.properties(),
            Self::ProjectRoot(value) => value.properties(),
            Self::ProjectDocument(value) => value.properties(),
        }
    }

    fn get_property(&self, name: &str) -> Result<PropertyValue, PropertyError> {
        match self {
            Self::TechnicalRoot(value) => value.get_property(name),
            Self::RequirementRoot(value) => value.get_property(name),
            Self::ConceptDirectory(value) => value.get_property(name),
            Self::TechnicalSystem(value) => value.get_property(name),
            Self::TechnicalModule(value) => value.get_property(name),
            Self::Root(value) => value.get_property(name),
            Self::ObjectList(value) => value.get_property(name),
            Self::DocumentText(value) => value.get_property(name),
            Self::DocumentList(value) => value.get_property(name),
            Self::DocumentParagraphList(value) => value.get_property(name),
            Self::DocumentTopic(value) => value.get_property(name),
            Self::DocumentCode(value) => value.get_property(name),
            Self::DocumentMermaid(value) => value.get_property(name),
            Self::DocumentHtml(value) => value.get_property(name),
            Self::Concept(value) => value.get_property(name),
            Self::RequirementUser(value) => value.get_property(name),
            Self::RequirementStory(value) => value.get_property(name),
            Self::RequirementSystem(value) => value.get_property(name),
            Self::RequirementModule(value) => value.get_property(name),
            Self::RequirementFunction(value) => value.get_property(name),
            Self::RequirementInteraction(value) => value.get_property(name),
            Self::TechnicalDirectory(value) => value.get_property(name),
            Self::TechnicalFile(value) => value.get_property(name),
            Self::TechnicalLayer(value) => value.get_property(name),
            Self::TechnicalNamespace(value) => value.get_property(name),
            Self::TechnicalClass(value) => value.get_property(name),
            Self::TechnicalStruct(value) => value.get_property(name),
            Self::TechnicalEnum(value) => value.get_property(name),
            Self::TechnicalInterface(value) => value.get_property(name),
            Self::TechnicalMethod(value) => value.get_property(name),
            Self::TechnicalFunction(value) => value.get_property(name),
            Self::TechnicalMixin(value) => value.get_property(name),
            Self::TechnicalPart(value) => value.get_property(name),
            Self::TechnicalVariable(value) => value.get_property(name),
            Self::TechnicalConstant(value) => value.get_property(name),
            Self::TechnicalMember(value) => value.get_property(name),
            Self::TechnicalProperty(value) => value.get_property(name),
            Self::TechnicalEnumValue(value) => value.get_property(name),
            Self::TechnicalArgument(value) => value.get_property(name),
            Self::TechnicalReturnValue(value) => value.get_property(name),
            Self::ProjectRoot(value) => value.get_property(name),
            Self::ProjectDocument(value) => value.get_property(name),
        }
    }

    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), PropertyError> {
        match self {
            Self::TechnicalRoot(object) => object.set_property(name, value),
            Self::RequirementRoot(object) => object.set_property(name, value),
            Self::ConceptDirectory(object) => object.set_property(name, value),
            Self::TechnicalSystem(object) => object.set_property(name, value),
            Self::TechnicalModule(object) => object.set_property(name, value),
            Self::Root(object) => object.set_property(name, value),
            Self::ObjectList(object) => object.set_property(name, value),
            Self::DocumentText(object) => object.set_property(name, value),
            Self::DocumentList(object) => object.set_property(name, value),
            Self::DocumentParagraphList(object) => object.set_property(name, value),
            Self::DocumentTopic(object) => object.set_property(name, value),
            Self::DocumentCode(object) => object.set_property(name, value),
            Self::DocumentMermaid(object) => object.set_property(name, value),
            Self::DocumentHtml(object) => object.set_property(name, value),
            Self::Concept(object) => object.set_property(name, value),
            Self::RequirementUser(object) => object.set_property(name, value),
            Self::RequirementStory(object) => object.set_property(name, value),
            Self::RequirementSystem(object) => object.set_property(name, value),
            Self::RequirementModule(object) => object.set_property(name, value),
            Self::RequirementFunction(object) => object.set_property(name, value),
            Self::RequirementInteraction(object) => object.set_property(name, value),
            Self::TechnicalDirectory(object) => object.set_property(name, value),
            Self::TechnicalFile(object) => object.set_property(name, value),
            Self::TechnicalLayer(object) => object.set_property(name, value),
            Self::TechnicalNamespace(object) => object.set_property(name, value),
            Self::TechnicalClass(object) => object.set_property(name, value),
            Self::TechnicalStruct(object) => object.set_property(name, value),
            Self::TechnicalEnum(object) => object.set_property(name, value),
            Self::TechnicalInterface(object) => object.set_property(name, value),
            Self::TechnicalMethod(object) => object.set_property(name, value),
            Self::TechnicalFunction(object) => object.set_property(name, value),
            Self::TechnicalMixin(object) => object.set_property(name, value),
            Self::TechnicalPart(object) => object.set_property(name, value),
            Self::TechnicalVariable(object) => object.set_property(name, value),
            Self::TechnicalConstant(object) => object.set_property(name, value),
            Self::TechnicalMember(object) => object.set_property(name, value),
            Self::TechnicalProperty(object) => object.set_property(name, value),
            Self::TechnicalEnumValue(object) => object.set_property(name, value),
            Self::TechnicalArgument(object) => object.set_property(name, value),
            Self::TechnicalReturnValue(object) => object.set_property(name, value),
            Self::ProjectRoot(object) => object.set_property(name, value),
            Self::ProjectDocument(object) => object.set_property(name, value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(parent: SymbolHash) -> TechnicalSystem {
        TechnicalSystem {
            qname: SymbolHash::of("technical/app"),
            name: SymbolHash::of("app"),
            parent,
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
        }
    }

    #[test]
    fn lists_and_reads_properties() {
        let mut object = system(SymbolHash::of("technical"));
        let properties = object.properties();
        assert!(
            properties
                .iter()
                .any(|spec| spec.name == "qname" && spec.kind == ValueKind::StringRef)
        );
        assert!(
            properties
                .iter()
                .any(|spec| spec.name == "name" && spec.kind == ValueKind::StringRef)
        );
        assert!(
            properties
                .iter()
                .any(|spec| spec.name == "parent" && spec.kind == ValueKind::StringRef)
        );
        assert!(
            properties
                .iter()
                .any(|spec| spec.name == "members" && spec.kind == ValueKind::ObjectRef)
        );
        assert_eq!(
            object.get_property("parent").unwrap(),
            PropertyValue::StringRef(SymbolHash::of("technical"))
        );
        assert_eq!(
            object.get_property("qname").unwrap(),
            PropertyValue::StringRef(SymbolHash::of("technical/app"))
        );
        assert_eq!(
            object.get_property("name").unwrap(),
            PropertyValue::StringRef(SymbolHash::of("app"))
        );
        assert_eq!(
            object.set_property("qname", PropertyValue::StringRef(SymbolHash::ZERO)),
            Err(PropertyError::ReadOnly("qname".to_owned()))
        );
    }

    #[test]
    fn sets_properties() {
        let mut object = system(SymbolHash::of("technical"));
        object
            .set_property(
                "parent",
                PropertyValue::StringRef(SymbolHash::of("technical/core")),
            )
            .unwrap();
        assert_eq!(object.parent, SymbolHash::of("technical/core"));
    }

    #[test]
    fn rejects_unknown_and_mismatched_properties() {
        let mut object = system(SymbolHash::of("technical"));
        assert_eq!(
            object.get_property("missing"),
            Err(PropertyError::UnknownProperty("missing".to_owned()))
        );
        let error = object
            .set_property("parent", PropertyValue::ObjectRef(ObjectHash::ZERO))
            .unwrap_err();
        assert!(matches!(error, PropertyError::TypeMismatch { .. }));
    }

    #[test]
    fn sets_string_ref_lists() {
        let mut object = TechnicalClass {
            qname: SymbolHash::of("technical/Foo"),
            name: SymbolHash::of("Foo"),
            parent: SymbolHash::ZERO,
            namespace: SymbolHash::ZERO,
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
        };
        let interface = SymbolHash::of("technical/Iface");
        assert!(
            PropertyAccess::properties(&object)
                .iter()
                .any(|spec| spec.name == "source" && spec.kind == ValueKind::SourceSpan)
        );
        assert!(
            !PropertyAccess::properties(&object)
                .iter()
                .any(|spec| spec.name == "source_path")
        );
        object
            .set_property(
                "interfaces",
                PropertyValue::StringRefList(StringRefList(vec![interface])),
            )
            .unwrap();
        assert_eq!(object.interfaces, vec![interface]);
        assert_eq!(
            object.get_property("interfaces").unwrap(),
            PropertyValue::StringRefList(StringRefList(vec![interface]))
        );
    }

    #[test]
    fn source_properties_follow_shape() {
        let directory = TechnicalDirectory {
            qname: SymbolHash::of("technical/src"),
            name: SymbolHash::of("src"),
            parent: SymbolHash::of("technical"),
            source: SourcePath {
                path: SymbolHash::of("src"),
            },
            children: ObjectHash::ZERO,
            position: ObjectHash::ZERO,
        };
        assert!(
            directory
                .properties()
                .iter()
                .any(|spec| spec.name == "source" && spec.kind == ValueKind::SourcePath)
        );
        assert_eq!(
            directory.get_property("source").unwrap(),
            PropertyValue::SourcePath(SourcePath {
                path: SymbolHash::of("src"),
            })
        );

        let namespace = TechnicalNamespace {
            qname: SymbolHash::of("technical/ns"),
            name: SymbolHash::of("ns"),
            parent: SymbolHash::of("technical"),
            children: ObjectHash::ZERO,
            entities: ObjectHash::ZERO,
            constants: ObjectHash::ZERO,
            variables: ObjectHash::ZERO,
            position: ObjectHash::ZERO,
        };
        assert!(
            !namespace
                .properties()
                .iter()
                .any(|spec| spec.name == "source")
        );
        assert_eq!(
            namespace.get_property("source"),
            Err(PropertyError::UnknownProperty("source".to_owned()))
        );
    }

    #[test]
    fn objects_without_properties_reject_access() {
        let mut object = Object::DocumentText(DocumentText {
            qname: SymbolHash::of("technical/app/position"),
            name: SymbolHash::of("position"),
            parent: SymbolHash::of("technical/app"),
            content: "入口".to_owned(),
        });
        let properties = object.properties();
        assert_eq!(properties.len(), 2);
        assert_eq!(
            object.get_property("name").unwrap(),
            PropertyValue::StringRef(SymbolHash::of("position"))
        );
        assert_eq!(
            object.get_property("content"),
            Err(PropertyError::UnknownProperty("content".to_owned()))
        );
        assert_eq!(
            object.set_property("content", PropertyValue::StringRef(SymbolHash::ZERO)),
            Err(PropertyError::UnknownProperty("content".to_owned()))
        );
    }
}
