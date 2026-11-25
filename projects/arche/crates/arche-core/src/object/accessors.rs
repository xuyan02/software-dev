use crate::hash::GitCommit;
use crate::refs::{ObjectRef, ObjectRefList, StringRef, StringRefList};
use crate::source::{SourcePath, SourceSpan};

use super::*;

impl TechnicalRoot {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn file(&self) -> ObjectRef {
        ObjectRef(self.file)
    }
    pub fn directory(&self) -> ObjectRef {
        ObjectRef(self.directory)
    }
    pub fn namespace(&self) -> ObjectRef {
        ObjectRef(self.namespace)
    }
    pub fn system(&self) -> ObjectRef {
        ObjectRef(self.system)
    }
}

impl RequirementRoot {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn user(&self) -> ObjectRef {
        ObjectRef(self.user)
    }
    pub fn story(&self) -> ObjectRef {
        ObjectRef(self.story)
    }
    pub fn system(&self) -> ObjectRef {
        ObjectRef(self.system)
    }
}

impl ConceptDirectory {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn directories(&self) -> ObjectRef {
        ObjectRef(self.directories)
    }
    pub fn concepts(&self) -> ObjectRef {
        ObjectRef(self.concepts)
    }
}

impl TechnicalSystem {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn members(&self) -> ObjectRef {
        ObjectRef(self.members)
    }
    pub fn modules(&self) -> ObjectRef {
        ObjectRef(self.modules)
    }
    pub fn layers(&self) -> ObjectRef {
        ObjectRef(self.layers)
    }
    pub fn constants(&self) -> ObjectRef {
        ObjectRef(self.constants)
    }
    pub fn variables(&self) -> ObjectRef {
        ObjectRef(self.variables)
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
    pub fn responsibilities(&self) -> ObjectRef {
        ObjectRef(self.responsibilities)
    }
    pub fn non_responsibilities(&self) -> ObjectRef {
        ObjectRef(self.non_responsibilities)
    }
    pub fn features(&self) -> ObjectRef {
        ObjectRef(self.features)
    }
    pub fn non_features(&self) -> ObjectRef {
        ObjectRef(self.non_features)
    }
    pub fn invariants(&self) -> ObjectRef {
        ObjectRef(self.invariants)
    }
    pub fn constraints(&self) -> ObjectRef {
        ObjectRef(self.constraints)
    }
    pub fn design_principles(&self) -> ObjectRef {
        ObjectRef(self.design_principles)
    }
    pub fn external_dependencies(&self) -> ObjectRef {
        ObjectRef(self.external_dependencies)
    }
    pub fn usage(&self) -> ObjectRef {
        ObjectRef(self.usage)
    }
    pub fn design_points(&self) -> ObjectRef {
        ObjectRef(self.design_points)
    }
}

impl TechnicalModule {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn members(&self) -> ObjectRef {
        ObjectRef(self.members)
    }
    pub fn dependencies(&self) -> StringRefList {
        StringRefList(self.dependencies.clone())
    }
    pub fn constants(&self) -> ObjectRef {
        ObjectRef(self.constants)
    }
    pub fn variables(&self) -> ObjectRef {
        ObjectRef(self.variables)
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
    pub fn responsibilities(&self) -> ObjectRef {
        ObjectRef(self.responsibilities)
    }
    pub fn non_responsibilities(&self) -> ObjectRef {
        ObjectRef(self.non_responsibilities)
    }
    pub fn features(&self) -> ObjectRef {
        ObjectRef(self.features)
    }
    pub fn non_features(&self) -> ObjectRef {
        ObjectRef(self.non_features)
    }
    pub fn invariants(&self) -> ObjectRef {
        ObjectRef(self.invariants)
    }
    pub fn constraints(&self) -> ObjectRef {
        ObjectRef(self.constraints)
    }
    pub fn design_principles(&self) -> ObjectRef {
        ObjectRef(self.design_principles)
    }
    pub fn usage(&self) -> ObjectRef {
        ObjectRef(self.usage)
    }
    pub fn design_points(&self) -> ObjectRef {
        ObjectRef(self.design_points)
    }
}

impl TechnicalLayer {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn members(&self) -> ObjectRef {
        ObjectRef(self.members)
    }
    pub fn dependencies(&self) -> StringRefList {
        StringRefList(self.dependencies.clone())
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
}

impl TechnicalDirectory {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn source(&self) -> SourcePath {
        self.source
    }
    pub fn children(&self) -> ObjectRef {
        ObjectRef(self.children)
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
}

impl TechnicalFile {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn source(&self) -> SourcePath {
        self.source
    }
    pub fn entities(&self) -> ObjectRef {
        ObjectRef(self.entities)
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
}

impl TechnicalNamespace {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn children(&self) -> ObjectRef {
        ObjectRef(self.children)
    }
    pub fn entities(&self) -> ObjectRef {
        ObjectRef(self.entities)
    }
    pub fn constants(&self) -> ObjectRef {
        ObjectRef(self.constants)
    }
    pub fn variables(&self) -> ObjectRef {
        ObjectRef(self.variables)
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
}

impl TechnicalClass {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn namespace(&self) -> StringRef {
        StringRef(self.namespace)
    }
    pub fn file(&self) -> StringRef {
        StringRef(self.file)
    }
    pub fn module(&self) -> StringRef {
        StringRef(self.module)
    }
    pub fn source(&self) -> SourceSpan {
        self.source
    }
    pub fn superclass(&self) -> StringRef {
        StringRef(self.superclass)
    }
    pub fn interfaces(&self) -> StringRefList {
        StringRefList(self.interfaces.clone())
    }
    pub fn mixins(&self) -> StringRefList {
        StringRefList(self.mixins.clone())
    }
    pub fn explicit_dependencies(&self) -> StringRefList {
        StringRefList(self.explicit_dependencies.clone())
    }
    pub fn implicit_dependencies(&self) -> StringRefList {
        StringRefList(self.implicit_dependencies.clone())
    }
    pub fn method(&self) -> ObjectRef {
        ObjectRef(self.method)
    }
    pub fn types(&self) -> ObjectRef {
        ObjectRef(self.types)
    }
    pub fn parts(&self) -> ObjectRef {
        ObjectRef(self.parts)
    }
    pub fn constants(&self) -> ObjectRef {
        ObjectRef(self.constants)
    }
    pub fn members(&self) -> ObjectRef {
        ObjectRef(self.members)
    }
    pub fn properties(&self) -> ObjectRef {
        ObjectRef(self.properties)
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
    pub fn responsibilities(&self) -> ObjectRef {
        ObjectRef(self.responsibilities)
    }
    pub fn non_responsibilities(&self) -> ObjectRef {
        ObjectRef(self.non_responsibilities)
    }
    pub fn features(&self) -> ObjectRef {
        ObjectRef(self.features)
    }
    pub fn non_features(&self) -> ObjectRef {
        ObjectRef(self.non_features)
    }
    pub fn invariants(&self) -> ObjectRef {
        ObjectRef(self.invariants)
    }
    pub fn constraints(&self) -> ObjectRef {
        ObjectRef(self.constraints)
    }
    pub fn design_principles(&self) -> ObjectRef {
        ObjectRef(self.design_principles)
    }
    pub fn usage(&self) -> ObjectRef {
        ObjectRef(self.usage)
    }
    pub fn design_points(&self) -> ObjectRef {
        ObjectRef(self.design_points)
    }
}

impl TechnicalStruct {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn namespace(&self) -> StringRef {
        StringRef(self.namespace)
    }
    pub fn file(&self) -> StringRef {
        StringRef(self.file)
    }
    pub fn module(&self) -> StringRef {
        StringRef(self.module)
    }
    pub fn source(&self) -> SourceSpan {
        self.source
    }
    pub fn dependencies(&self) -> StringRefList {
        StringRefList(self.dependencies.clone())
    }
    pub fn method(&self) -> ObjectRef {
        ObjectRef(self.method)
    }
    pub fn types(&self) -> ObjectRef {
        ObjectRef(self.types)
    }
    pub fn constants(&self) -> ObjectRef {
        ObjectRef(self.constants)
    }
    pub fn members(&self) -> ObjectRef {
        ObjectRef(self.members)
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
    pub fn invariants(&self) -> ObjectRef {
        ObjectRef(self.invariants)
    }
}

impl TechnicalEnum {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn namespace(&self) -> StringRef {
        StringRef(self.namespace)
    }
    pub fn file(&self) -> StringRef {
        StringRef(self.file)
    }
    pub fn module(&self) -> StringRef {
        StringRef(self.module)
    }
    pub fn source(&self) -> SourceSpan {
        self.source
    }
    pub fn dependencies(&self) -> StringRefList {
        StringRefList(self.dependencies.clone())
    }
    pub fn values(&self) -> ObjectRef {
        ObjectRef(self.values)
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
}

impl TechnicalInterface {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn namespace(&self) -> StringRef {
        StringRef(self.namespace)
    }
    pub fn file(&self) -> StringRef {
        StringRef(self.file)
    }
    pub fn module(&self) -> StringRef {
        StringRef(self.module)
    }
    pub fn source(&self) -> SourceSpan {
        self.source
    }
    pub fn extends(&self) -> StringRef {
        StringRef(self.extends)
    }
    pub fn dependencies(&self) -> StringRefList {
        StringRefList(self.dependencies.clone())
    }
    pub fn methods(&self) -> ObjectRef {
        ObjectRef(self.methods)
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
    pub fn responsibilities(&self) -> ObjectRef {
        ObjectRef(self.responsibilities)
    }
    pub fn non_responsibilities(&self) -> ObjectRef {
        ObjectRef(self.non_responsibilities)
    }
    pub fn features(&self) -> ObjectRef {
        ObjectRef(self.features)
    }
    pub fn non_features(&self) -> ObjectRef {
        ObjectRef(self.non_features)
    }
    pub fn invariants(&self) -> ObjectRef {
        ObjectRef(self.invariants)
    }
    pub fn usage(&self) -> ObjectRef {
        ObjectRef(self.usage)
    }
    pub fn design_points(&self) -> ObjectRef {
        ObjectRef(self.design_points)
    }
}

impl TechnicalMethod {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn source(&self) -> SourceSpan {
        self.source
    }
    pub fn is_static(&self) -> bool {
        self.is_static
    }
    pub fn is_async(&self) -> bool {
        self.is_async
    }
    pub fn is_abstract(&self) -> bool {
        self.is_abstract
    }
    pub fn is_const(&self) -> bool {
        self.is_const
    }
    pub fn overrides(&self) -> StringRef {
        StringRef(self.overrides)
    }
    pub fn explicit_dependencies(&self) -> StringRefList {
        StringRefList(self.explicit_dependencies.clone())
    }
    pub fn implicit_dependencies(&self) -> StringRefList {
        StringRefList(self.implicit_dependencies.clone())
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
    pub fn arguments(&self) -> ObjectRef {
        ObjectRef(self.arguments)
    }
    pub fn return_value(&self) -> ObjectRef {
        ObjectRef(self.return_value)
    }
    pub fn usage(&self) -> ObjectRef {
        ObjectRef(self.usage)
    }
    pub fn design_points(&self) -> ObjectRef {
        ObjectRef(self.design_points)
    }
}

impl TechnicalFunction {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn namespace(&self) -> StringRef {
        StringRef(self.namespace)
    }
    pub fn file(&self) -> StringRef {
        StringRef(self.file)
    }
    pub fn module(&self) -> StringRef {
        StringRef(self.module)
    }
    pub fn source(&self) -> SourceSpan {
        self.source
    }
    pub fn is_async(&self) -> bool {
        self.is_async
    }
    pub fn explicit_dependencies(&self) -> StringRefList {
        StringRefList(self.explicit_dependencies.clone())
    }
    pub fn implicit_dependencies(&self) -> StringRefList {
        StringRefList(self.implicit_dependencies.clone())
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
    pub fn arguments(&self) -> ObjectRef {
        ObjectRef(self.arguments)
    }
    pub fn return_value(&self) -> ObjectRef {
        ObjectRef(self.return_value)
    }
    pub fn usage(&self) -> ObjectRef {
        ObjectRef(self.usage)
    }
    pub fn design_points(&self) -> ObjectRef {
        ObjectRef(self.design_points)
    }
}

impl TechnicalMixin {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn namespace(&self) -> StringRef {
        StringRef(self.namespace)
    }
    pub fn file(&self) -> StringRef {
        StringRef(self.file)
    }
    pub fn module(&self) -> StringRef {
        StringRef(self.module)
    }
    pub fn source(&self) -> SourceSpan {
        self.source
    }
    pub fn on(&self) -> StringRefList {
        StringRefList(self.on.clone())
    }
    pub fn superclass(&self) -> StringRef {
        StringRef(self.superclass)
    }
    pub fn interfaces(&self) -> StringRefList {
        StringRefList(self.interfaces.clone())
    }
    pub fn mixins(&self) -> StringRefList {
        StringRefList(self.mixins.clone())
    }
    pub fn explicit_dependencies(&self) -> StringRefList {
        StringRefList(self.explicit_dependencies.clone())
    }
    pub fn implicit_dependencies(&self) -> StringRefList {
        StringRefList(self.implicit_dependencies.clone())
    }
    pub fn methods(&self) -> ObjectRef {
        ObjectRef(self.methods)
    }
    pub fn types(&self) -> ObjectRef {
        ObjectRef(self.types)
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
    pub fn responsibilities(&self) -> ObjectRef {
        ObjectRef(self.responsibilities)
    }
    pub fn non_responsibilities(&self) -> ObjectRef {
        ObjectRef(self.non_responsibilities)
    }
    pub fn features(&self) -> ObjectRef {
        ObjectRef(self.features)
    }
    pub fn non_features(&self) -> ObjectRef {
        ObjectRef(self.non_features)
    }
    pub fn invariants(&self) -> ObjectRef {
        ObjectRef(self.invariants)
    }
    pub fn usage(&self) -> ObjectRef {
        ObjectRef(self.usage)
    }
    pub fn design_points(&self) -> ObjectRef {
        ObjectRef(self.design_points)
    }
}

impl TechnicalPart {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn source(&self) -> SourceSpan {
        self.source
    }
    pub fn explicit_dependencies(&self) -> StringRefList {
        StringRefList(self.explicit_dependencies.clone())
    }
    pub fn implicit_dependencies(&self) -> StringRefList {
        StringRefList(self.implicit_dependencies.clone())
    }
    pub fn members(&self) -> ObjectRef {
        ObjectRef(self.members)
    }
    pub fn methods(&self) -> ObjectRef {
        ObjectRef(self.methods)
    }
    pub fn types(&self) -> ObjectRef {
        ObjectRef(self.types)
    }
}

impl TechnicalVariable {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn source(&self) -> SourceSpan {
        self.source
    }
    pub fn ty(&self) -> StringRef {
        StringRef(self.ty)
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
}

impl TechnicalConstant {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn source(&self) -> SourceSpan {
        self.source
    }
    pub fn ty(&self) -> StringRef {
        StringRef(self.ty)
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
}

impl TechnicalMember {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn source(&self) -> SourceSpan {
        self.source
    }
    pub fn ty(&self) -> StringRef {
        StringRef(self.ty)
    }
    pub fn is_static(&self) -> bool {
        self.is_static
    }
    pub fn is_const(&self) -> bool {
        self.is_const
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
}

impl TechnicalProperty {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn ty(&self) -> StringRef {
        StringRef(self.ty)
    }
    pub fn getter(&self) -> SourceSpan {
        self.getter
    }
    pub fn setter(&self) -> SourceSpan {
        self.setter
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
}

impl TechnicalEnumValue {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn source(&self) -> SourceSpan {
        self.source
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
}

impl TechnicalArgument {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn source(&self) -> SourceSpan {
        self.source
    }
    pub fn ty(&self) -> StringRef {
        StringRef(self.ty)
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
}

impl TechnicalReturnValue {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn source(&self) -> SourceSpan {
        self.source
    }
    pub fn ty(&self) -> StringRef {
        StringRef(self.ty)
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
}

impl Root {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn source_commit(&self) -> GitCommit {
        self.source_commit
    }
    pub fn technical(&self) -> ObjectRef {
        ObjectRef(self.technical)
    }
    pub fn requirement(&self) -> ObjectRef {
        ObjectRef(self.requirement)
    }
    pub fn concept(&self) -> ObjectRef {
        ObjectRef(self.concept)
    }
}

impl ProjectRoot {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn document(&self) -> ObjectRef {
        ObjectRef(self.document)
    }
}

impl ProjectDocument {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn document(&self) -> ObjectRef {
        ObjectRef(self.document)
    }
}

impl ObjectList {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn items(&self) -> ObjectRefList {
        ObjectRefList(self.items.clone())
    }
}

impl DocumentText {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn content(&self) -> &str {
        &self.content
    }
}

impl DocumentList {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn items(&self) -> ObjectRefList {
        ObjectRefList(self.items.clone())
    }
}

impl DocumentParagraphList {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn items(&self) -> ObjectRefList {
        ObjectRefList(self.items.clone())
    }
}

impl DocumentTopic {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn body(&self) -> ObjectRef {
        ObjectRef(self.body)
    }
}

impl DocumentCode {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn language(&self) -> &str {
        &self.language
    }
    pub fn source(&self) -> &str {
        &self.source
    }
}

impl DocumentMermaid {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn source(&self) -> &str {
        &self.source
    }
}

impl DocumentHtml {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn source(&self) -> &str {
        &self.source
    }
}

impl Concept {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    pub fn explanation(&self) -> ObjectRef {
        ObjectRef(self.explanation)
    }
}

impl RequirementUser {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    pub fn age(&self) -> &str {
        &self.age
    }
    pub fn occupation(&self) -> &str {
        &self.occupation
    }
    pub fn situations(&self) -> ObjectRef {
        ObjectRef(self.situations)
    }
    pub fn hobbies(&self) -> ObjectRef {
        ObjectRef(self.hobbies)
    }
    pub fn needs(&self) -> ObjectRef {
        ObjectRef(self.needs)
    }
    pub fn priority(&self) -> u8 {
        self.priority
    }
    pub fn stories(&self) -> StringRefList {
        StringRefList(self.stories.clone())
    }
}

impl RequirementStory {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn preconditions(&self) -> ObjectRef {
        ObjectRef(self.preconditions)
    }
    pub fn frequency(&self) -> u8 {
        self.frequency
    }
    pub fn description(&self) -> ObjectRef {
        ObjectRef(self.description)
    }
    pub fn requirement(&self) -> ObjectRef {
        ObjectRef(self.requirement)
    }
    pub fn users(&self) -> StringRefList {
        StringRefList(self.users.clone())
    }
    pub fn functions(&self) -> StringRefList {
        StringRefList(self.functions.clone())
    }
    pub fn protagonist(&self) -> StringRef {
        StringRef(self.protagonist)
    }
    pub fn flow(&self) -> ObjectRef {
        ObjectRef(self.flow)
    }
}

impl RequirementSystem {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
    pub fn modules(&self) -> ObjectRef {
        ObjectRef(self.modules)
    }
    pub fn app_form(&self) -> ObjectRef {
        ObjectRef(self.app_form)
    }
    pub fn users(&self) -> StringRefList {
        StringRefList(self.users.clone())
    }
}

impl RequirementModule {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    pub fn position(&self) -> ObjectRef {
        ObjectRef(self.position)
    }
    pub fn functions(&self) -> ObjectRef {
        ObjectRef(self.functions)
    }
}

impl RequirementFunction {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn flow(&self) -> ObjectRef {
        ObjectRef(self.flow)
    }
    pub fn interactions(&self) -> ObjectRef {
        ObjectRef(self.interactions)
    }
    pub fn initiator(&self) -> ObjectRef {
        ObjectRef(self.initiator)
    }
    pub fn system_change(&self) -> ObjectRef {
        ObjectRef(self.system_change)
    }
    pub fn preconditions(&self) -> ObjectRef {
        ObjectRef(self.preconditions)
    }
    pub fn feedback(&self) -> ObjectRef {
        ObjectRef(self.feedback)
    }
    pub fn description(&self) -> ObjectRef {
        ObjectRef(self.description)
    }
}

impl RequirementInteraction {
    pub fn qname(&self) -> StringRef {
        StringRef(self.qname)
    }
    pub fn name(&self) -> StringRef {
        StringRef(self.name)
    }
    pub fn parent(&self) -> StringRef {
        StringRef(self.parent)
    }
    pub fn description(&self) -> ObjectRef {
        ObjectRef(self.description)
    }
    pub fn ui(&self) -> ObjectRef {
        ObjectRef(self.ui)
    }
}
