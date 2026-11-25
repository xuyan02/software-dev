#![allow(clippy::too_many_arguments)]

use anyhow::Result;

use crate::hash::{GitCommit, ObjectHash, SymbolHash};
use crate::object::*;
use crate::source::{SourcePath, SourceSpan};
use crate::store::Store;

fn random_name_text() -> Result<String> {
    let mut bytes = [0u8; 4];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| anyhow::anyhow!("generate random object name: {error}"))?;
    Ok(bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>())
}

pub(crate) fn new_identity(
    store: &dyn Store,
    parent_qname: &str,
    name: Option<&str>,
) -> Result<(SymbolHash, SymbolHash, SymbolHash)> {
    let parent = store.intern_symbol(parent_qname)?;
    let name_text = match name {
        Some(name) => name.to_owned(),
        None => random_name_text()?,
    };
    let name = store.intern_symbol(&name_text)?;
    let qname = store.intern_symbol(&format!("{parent_qname}/{name_text}"))?;
    Ok((qname, name, parent))
}

impl Object {
    pub fn alloc(kind: ObjectKind) -> Self {
        match kind {
            ObjectKind::TechnicalRoot => Self::TechnicalRoot(TechnicalRoot::default()),
            ObjectKind::RequirementRoot => Self::RequirementRoot(RequirementRoot::default()),
            ObjectKind::ConceptDirectory => Self::ConceptDirectory(ConceptDirectory::default()),
            ObjectKind::TechnicalSystem => Self::TechnicalSystem(TechnicalSystem::default()),
            ObjectKind::TechnicalModule => Self::TechnicalModule(TechnicalModule::default()),
            ObjectKind::Root => Self::Root(Root::default()),
            ObjectKind::ObjectList => Self::ObjectList(ObjectList::default()),
            ObjectKind::DocumentText => Self::DocumentText(DocumentText::default()),
            ObjectKind::DocumentList => Self::DocumentList(DocumentList::default()),
            ObjectKind::DocumentParagraphList => {
                Self::DocumentParagraphList(DocumentParagraphList::default())
            }
            ObjectKind::DocumentTopic => Self::DocumentTopic(DocumentTopic::default()),
            ObjectKind::DocumentCode => Self::DocumentCode(DocumentCode::default()),
            ObjectKind::DocumentMermaid => Self::DocumentMermaid(DocumentMermaid::default()),
            ObjectKind::DocumentHtml => Self::DocumentHtml(DocumentHtml::default()),
            ObjectKind::Concept => Self::Concept(Concept::default()),
            ObjectKind::RequirementUser => Self::RequirementUser(RequirementUser::default()),
            ObjectKind::RequirementStory => Self::RequirementStory(RequirementStory::default()),
            ObjectKind::RequirementSystem => Self::RequirementSystem(RequirementSystem::default()),
            ObjectKind::RequirementModule => Self::RequirementModule(RequirementModule::default()),
            ObjectKind::RequirementFunction => {
                Self::RequirementFunction(RequirementFunction::default())
            }
            ObjectKind::RequirementInteraction => {
                Self::RequirementInteraction(RequirementInteraction::default())
            }
            ObjectKind::TechnicalDirectory => {
                Self::TechnicalDirectory(TechnicalDirectory::default())
            }
            ObjectKind::TechnicalFile => Self::TechnicalFile(TechnicalFile::default()),
            ObjectKind::TechnicalLayer => Self::TechnicalLayer(TechnicalLayer::default()),
            ObjectKind::TechnicalNamespace => {
                Self::TechnicalNamespace(TechnicalNamespace::default())
            }
            ObjectKind::TechnicalClass => Self::TechnicalClass(TechnicalClass::default()),
            ObjectKind::TechnicalStruct => Self::TechnicalStruct(TechnicalStruct::default()),
            ObjectKind::TechnicalEnum => Self::TechnicalEnum(TechnicalEnum::default()),
            ObjectKind::TechnicalInterface => {
                Self::TechnicalInterface(TechnicalInterface::default())
            }
            ObjectKind::TechnicalMethod => Self::TechnicalMethod(TechnicalMethod::default()),
            ObjectKind::TechnicalFunction => Self::TechnicalFunction(TechnicalFunction::default()),
            ObjectKind::TechnicalMixin => Self::TechnicalMixin(TechnicalMixin::default()),
            ObjectKind::TechnicalPart => Self::TechnicalPart(TechnicalPart::default()),
            ObjectKind::TechnicalVariable => Self::TechnicalVariable(TechnicalVariable::default()),
            ObjectKind::TechnicalConstant => Self::TechnicalConstant(TechnicalConstant::default()),
            ObjectKind::TechnicalMember => Self::TechnicalMember(TechnicalMember::default()),
            ObjectKind::TechnicalProperty => Self::TechnicalProperty(TechnicalProperty::default()),
            ObjectKind::TechnicalEnumValue => {
                Self::TechnicalEnumValue(TechnicalEnumValue::default())
            }
            ObjectKind::TechnicalArgument => Self::TechnicalArgument(TechnicalArgument::default()),
            ObjectKind::TechnicalReturnValue => {
                Self::TechnicalReturnValue(TechnicalReturnValue::default())
            }
            ObjectKind::ProjectRoot => Self::ProjectRoot(ProjectRoot::default()),
            ObjectKind::ProjectDocument => Self::ProjectDocument(ProjectDocument::default()),
        }
    }

    pub fn new_project_root(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        document: ObjectHash,
    ) -> Self {
        Self::ProjectRoot(ProjectRoot {
            qname,
            name,
            parent,
            document,
        })
    }

    pub fn new_project_document(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        document: ObjectHash,
    ) -> Self {
        Self::ProjectDocument(ProjectDocument {
            qname,
            name,
            parent,
            document,
        })
    }

    pub fn new_technical_root(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        file: ObjectHash,
        directory: ObjectHash,
        namespace: ObjectHash,
        system: ObjectHash,
    ) -> Self {
        Self::TechnicalRoot(TechnicalRoot {
            qname,
            name,
            parent,
            file,
            directory,
            namespace,
            system,
        })
    }

    pub fn new_requirement_root(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        user: ObjectHash,
        story: ObjectHash,
        system: ObjectHash,
    ) -> Self {
        Self::RequirementRoot(RequirementRoot {
            qname,
            name,
            parent,
            user,
            story,
            system,
        })
    }

    pub fn new_concept_directory(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        directories: ObjectHash,
        concepts: ObjectHash,
    ) -> Self {
        Self::ConceptDirectory(ConceptDirectory {
            qname,
            name,
            parent,
            directories,
            concepts,
        })
    }

    pub fn new_technical_system(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        members: ObjectHash,
        modules: ObjectHash,
        layers: ObjectHash,
        constants: ObjectHash,
        variables: ObjectHash,
        position: ObjectHash,
        responsibilities: ObjectHash,
        non_responsibilities: ObjectHash,
        features: ObjectHash,
        non_features: ObjectHash,
        invariants: ObjectHash,
        constraints: ObjectHash,
        design_principles: ObjectHash,
        external_dependencies: ObjectHash,
        usage: ObjectHash,
        design_points: ObjectHash,
    ) -> Self {
        Self::TechnicalSystem(TechnicalSystem {
            qname,
            name,
            parent,
            members,
            modules,
            layers,
            constants,
            variables,
            position,
            responsibilities,
            non_responsibilities,
            features,
            non_features,
            invariants,
            constraints,
            design_principles,
            external_dependencies,
            usage,
            design_points,
        })
    }

    pub fn new_technical_module(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        members: ObjectHash,
        dependencies: Vec<SymbolHash>,
        constants: ObjectHash,
        variables: ObjectHash,
        position: ObjectHash,
        responsibilities: ObjectHash,
        non_responsibilities: ObjectHash,
        features: ObjectHash,
        non_features: ObjectHash,
        invariants: ObjectHash,
        constraints: ObjectHash,
        design_principles: ObjectHash,
        usage: ObjectHash,
        design_points: ObjectHash,
    ) -> Self {
        Self::TechnicalModule(TechnicalModule {
            qname,
            name,
            parent,
            members,
            dependencies,
            constants,
            variables,
            position,
            responsibilities,
            non_responsibilities,
            features,
            non_features,
            invariants,
            constraints,
            design_principles,
            usage,
            design_points,
        })
    }

    pub fn new_root(
        qname: SymbolHash,
        name: SymbolHash,
        source_commit: GitCommit,
        technical: ObjectHash,
        requirement: ObjectHash,
        concept: ObjectHash,
    ) -> Self {
        Self::Root(Root {
            qname,
            name,
            source_commit,
            technical,
            requirement,
            concept,
        })
    }

    pub fn new_object_list(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        items: Vec<ObjectHash>,
    ) -> Self {
        Self::ObjectList(ObjectList {
            qname,
            name,
            parent,
            items,
        })
    }

    pub fn new_document_text(
        store: &dyn Store,
        parent_qname: &str,
        name: Option<&str>,
        content: String,
    ) -> Result<Self> {
        let (qname, name, parent) = new_identity(store, parent_qname, name)?;
        Ok(Self::DocumentText(DocumentText {
            qname,
            name,
            parent,
            content,
        }))
    }

    pub fn new_document_list(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        items: Vec<ObjectHash>,
    ) -> Self {
        Self::DocumentList(DocumentList {
            qname,
            name,
            parent,
            items,
        })
    }

    pub fn new_document_paragraph_list(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        items: Vec<ObjectHash>,
    ) -> Self {
        Self::DocumentParagraphList(DocumentParagraphList {
            qname,
            name,
            parent,
            items,
        })
    }

    pub fn new_document_topic(
        store: &dyn Store,
        parent_qname: &str,
        name: Option<&str>,
        title: String,
        body: ObjectHash,
    ) -> Result<Self> {
        let (qname, name, parent) = new_identity(store, parent_qname, name)?;
        Ok(Self::DocumentTopic(DocumentTopic {
            qname,
            name,
            parent,
            title,
            body,
        }))
    }

    pub fn new_document_code(
        store: &dyn Store,
        parent_qname: &str,
        name: Option<&str>,
        language: String,
        source: String,
    ) -> Result<Self> {
        let (qname, name, parent) = new_identity(store, parent_qname, name)?;
        Ok(Self::DocumentCode(DocumentCode {
            qname,
            name,
            parent,
            language,
            source,
        }))
    }

    pub fn new_document_mermaid(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        source: String,
    ) -> Self {
        Self::DocumentMermaid(DocumentMermaid {
            qname,
            name,
            parent,
            source,
        })
    }

    pub fn new_document_html(
        store: &dyn Store,
        parent_qname: &str,
        name: Option<&str>,
        source: String,
    ) -> Result<Self> {
        let (qname, name, parent) = new_identity(store, parent_qname, name)?;
        Ok(Self::DocumentHtml(DocumentHtml {
            qname,
            name,
            parent,
            source,
        }))
    }

    pub fn new_concept(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        display_name: String,
        explanation: ObjectHash,
    ) -> Self {
        Self::Concept(Concept {
            qname,
            name,
            parent,
            display_name,
            explanation,
        })
    }

    pub fn new_requirement_user(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        display_name: String,
        age: String,
        occupation: String,
        situations: ObjectHash,
        hobbies: ObjectHash,
        needs: ObjectHash,
        priority: u8,
        stories: Vec<SymbolHash>,
    ) -> Self {
        Self::RequirementUser(RequirementUser {
            qname,
            name,
            parent,
            display_name,
            age,
            occupation,
            situations,
            hobbies,
            needs,
            priority,
            stories,
        })
    }

    pub fn new_requirement_story(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        preconditions: ObjectHash,
        frequency: u8,
        description: ObjectHash,
        requirement: ObjectHash,
        users: Vec<SymbolHash>,
        functions: Vec<SymbolHash>,
        protagonist: SymbolHash,
        flow: ObjectHash,
    ) -> Self {
        Self::RequirementStory(RequirementStory {
            qname,
            name,
            parent,
            preconditions,
            frequency,
            description,
            requirement,
            users,
            functions,
            protagonist,
            flow,
        })
    }

    pub fn new_requirement_system(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        display_name: String,
        position: ObjectHash,
        app_form: ObjectHash,
        users: Vec<SymbolHash>,
        modules: ObjectHash,
    ) -> Self {
        Self::RequirementSystem(RequirementSystem {
            qname,
            name,
            parent,
            display_name,
            position,
            app_form,
            users,
            modules,
            ..Default::default()
        })
    }

    pub fn new_requirement_module(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        display_name: String,
        position: ObjectHash,
        functions: ObjectHash,
    ) -> Self {
        Self::RequirementModule(RequirementModule {
            qname,
            name,
            parent,
            display_name,
            position,
            functions,
        })
    }

    pub fn new_requirement_function(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        flow: ObjectHash,
        interactions: ObjectHash,
        initiator: ObjectHash,
        system_change: ObjectHash,
        preconditions: ObjectHash,
        feedback: ObjectHash,
        description: ObjectHash,
    ) -> Self {
        Self::RequirementFunction(RequirementFunction {
            qname,
            name,
            parent,
            flow,
            interactions,
            initiator,
            system_change,
            preconditions,
            feedback,
            description,
        })
    }

    pub fn new_requirement_interaction(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        description: ObjectHash,
        ui: ObjectHash,
    ) -> Self {
        Self::RequirementInteraction(RequirementInteraction {
            qname,
            name,
            parent,
            description,
            ui,
        })
    }

    pub fn new_technical_directory(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        source: SourcePath,
        children: ObjectHash,
        position: ObjectHash,
    ) -> Self {
        Self::TechnicalDirectory(TechnicalDirectory {
            qname,
            name,
            parent,
            source,
            children,
            position,
        })
    }

    pub fn new_technical_file(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        source: SourcePath,
        entities: ObjectHash,
        position: ObjectHash,
    ) -> Self {
        Self::TechnicalFile(TechnicalFile {
            qname,
            name,
            parent,
            source,
            entities,
            position,
        })
    }

    pub fn new_technical_layer(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        members: ObjectHash,
        dependencies: Vec<SymbolHash>,
        position: ObjectHash,
    ) -> Self {
        Self::TechnicalLayer(TechnicalLayer {
            qname,
            name,
            parent,
            members,
            dependencies,
            position,
        })
    }

    pub fn new_technical_namespace(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        children: ObjectHash,
        entities: ObjectHash,
        constants: ObjectHash,
        variables: ObjectHash,
        position: ObjectHash,
    ) -> Self {
        Self::TechnicalNamespace(TechnicalNamespace {
            qname,
            name,
            parent,
            children,
            entities,
            constants,
            variables,
            position,
        })
    }

    pub fn new_technical_class(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        namespace: SymbolHash,
        file: SymbolHash,
        module: SymbolHash,
        source: SourceSpan,
        superclass: SymbolHash,
        interfaces: Vec<SymbolHash>,
        mixins: Vec<SymbolHash>,
        explicit_dependencies: Vec<SymbolHash>,
        implicit_dependencies: Vec<SymbolHash>,
        method: ObjectHash,
        types: ObjectHash,
        parts: ObjectHash,
        constants: ObjectHash,
        members: ObjectHash,
        properties: ObjectHash,
        position: ObjectHash,
        responsibilities: ObjectHash,
        non_responsibilities: ObjectHash,
        features: ObjectHash,
        non_features: ObjectHash,
        invariants: ObjectHash,
        constraints: ObjectHash,
        design_principles: ObjectHash,
        usage: ObjectHash,
        design_points: ObjectHash,
    ) -> Self {
        Self::TechnicalClass(TechnicalClass {
            qname,
            name,
            parent,
            namespace,
            file,
            module,
            source,
            superclass,
            interfaces,
            mixins,
            explicit_dependencies,
            implicit_dependencies,
            method,
            types,
            parts,
            constants,
            members,
            properties,
            position,
            responsibilities,
            non_responsibilities,
            features,
            non_features,
            invariants,
            constraints,
            design_principles,
            usage,
            design_points,
        })
    }

    pub fn new_technical_struct(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        namespace: SymbolHash,
        file: SymbolHash,
        module: SymbolHash,
        source: SourceSpan,
        dependencies: Vec<SymbolHash>,
        method: ObjectHash,
        types: ObjectHash,
        constants: ObjectHash,
        members: ObjectHash,
        position: ObjectHash,
        invariants: ObjectHash,
    ) -> Self {
        Self::TechnicalStruct(TechnicalStruct {
            qname,
            name,
            parent,
            namespace,
            file,
            module,
            source,
            dependencies,
            method,
            types,
            constants,
            members,
            position,
            invariants,
        })
    }

    pub fn new_technical_enum(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        namespace: SymbolHash,
        file: SymbolHash,
        module: SymbolHash,
        source: SourceSpan,
        dependencies: Vec<SymbolHash>,
        values: ObjectHash,
        position: ObjectHash,
    ) -> Self {
        Self::TechnicalEnum(TechnicalEnum {
            qname,
            name,
            parent,
            namespace,
            file,
            module,
            source,
            dependencies,
            values,
            position,
        })
    }

    pub fn new_technical_interface(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        namespace: SymbolHash,
        file: SymbolHash,
        module: SymbolHash,
        source: SourceSpan,
        extends: SymbolHash,
        dependencies: Vec<SymbolHash>,
        methods: ObjectHash,
        position: ObjectHash,
        responsibilities: ObjectHash,
        non_responsibilities: ObjectHash,
        features: ObjectHash,
        non_features: ObjectHash,
        invariants: ObjectHash,
        usage: ObjectHash,
        design_points: ObjectHash,
    ) -> Self {
        Self::TechnicalInterface(TechnicalInterface {
            qname,
            name,
            parent,
            namespace,
            file,
            module,
            source,
            extends,
            dependencies,
            methods,
            position,
            responsibilities,
            non_responsibilities,
            features,
            non_features,
            invariants,
            usage,
            design_points,
        })
    }

    pub fn new_technical_method(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        source: SourceSpan,
        is_static: bool,
        is_async: bool,
        is_abstract: bool,
        is_const: bool,
        overrides: SymbolHash,
        explicit_dependencies: Vec<SymbolHash>,
        implicit_dependencies: Vec<SymbolHash>,
        position: ObjectHash,
        arguments: ObjectHash,
        return_value: ObjectHash,
        usage: ObjectHash,
        design_points: ObjectHash,
    ) -> Self {
        Self::TechnicalMethod(TechnicalMethod {
            qname,
            name,
            parent,
            source,
            is_static,
            is_async,
            is_abstract,
            is_const,
            overrides,
            explicit_dependencies,
            implicit_dependencies,
            position,
            arguments,
            return_value,
            usage,
            design_points,
        })
    }

    pub fn new_technical_function(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        namespace: SymbolHash,
        file: SymbolHash,
        module: SymbolHash,
        source: SourceSpan,
        is_async: bool,
        explicit_dependencies: Vec<SymbolHash>,
        implicit_dependencies: Vec<SymbolHash>,
        position: ObjectHash,
        arguments: ObjectHash,
        return_value: ObjectHash,
        usage: ObjectHash,
        design_points: ObjectHash,
    ) -> Self {
        Self::TechnicalFunction(TechnicalFunction {
            qname,
            name,
            parent,
            namespace,
            file,
            module,
            source,
            is_async,
            explicit_dependencies,
            implicit_dependencies,
            position,
            arguments,
            return_value,
            usage,
            design_points,
        })
    }

    pub fn new_technical_mixin(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        namespace: SymbolHash,
        file: SymbolHash,
        module: SymbolHash,
        source: SourceSpan,
        on: Vec<SymbolHash>,
        superclass: SymbolHash,
        interfaces: Vec<SymbolHash>,
        mixins: Vec<SymbolHash>,
        explicit_dependencies: Vec<SymbolHash>,
        implicit_dependencies: Vec<SymbolHash>,
        methods: ObjectHash,
        types: ObjectHash,
        position: ObjectHash,
        responsibilities: ObjectHash,
        non_responsibilities: ObjectHash,
        features: ObjectHash,
        non_features: ObjectHash,
        invariants: ObjectHash,
        usage: ObjectHash,
        design_points: ObjectHash,
    ) -> Self {
        Self::TechnicalMixin(TechnicalMixin {
            qname,
            name,
            parent,
            namespace,
            file,
            module,
            source,
            on,
            superclass,
            interfaces,
            mixins,
            explicit_dependencies,
            implicit_dependencies,
            methods,
            types,
            position,
            responsibilities,
            non_responsibilities,
            features,
            non_features,
            invariants,
            usage,
            design_points,
        })
    }

    pub fn new_technical_part(
        store: &dyn Store,
        parent_qname: &str,
        name: Option<&str>,
        source: SourceSpan,
        explicit_dependencies: Vec<SymbolHash>,
        implicit_dependencies: Vec<SymbolHash>,
        members: ObjectHash,
        methods: ObjectHash,
        types: ObjectHash,
    ) -> Result<Self> {
        let (qname, name, parent) = new_identity(store, parent_qname, name)?;
        Ok(Self::TechnicalPart(TechnicalPart {
            qname,
            name,
            parent,
            source,
            explicit_dependencies,
            implicit_dependencies,
            members,
            methods,
            types,
        }))
    }

    pub fn new_technical_variable(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        source: SourceSpan,
        ty: SymbolHash,
        position: ObjectHash,
    ) -> Self {
        Self::TechnicalVariable(TechnicalVariable {
            qname,
            name,
            parent,
            source,
            ty,
            position,
        })
    }

    pub fn new_technical_constant(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        source: SourceSpan,
        ty: SymbolHash,
        position: ObjectHash,
    ) -> Self {
        Self::TechnicalConstant(TechnicalConstant {
            qname,
            name,
            parent,
            source,
            ty,
            position,
        })
    }

    pub fn new_technical_member(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        source: SourceSpan,
        ty: SymbolHash,
        is_static: bool,
        is_const: bool,
        position: ObjectHash,
    ) -> Self {
        Self::TechnicalMember(TechnicalMember {
            qname,
            name,
            parent,
            source,
            ty,
            is_static,
            is_const,
            position,
        })
    }

    pub fn new_technical_property(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        ty: SymbolHash,
        getter: SourceSpan,
        setter: SourceSpan,
        position: ObjectHash,
    ) -> Self {
        Self::TechnicalProperty(TechnicalProperty {
            qname,
            name,
            parent,
            ty,
            getter,
            setter,
            position,
        })
    }

    pub fn new_technical_enum_value(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        source: SourceSpan,
        position: ObjectHash,
    ) -> Self {
        Self::TechnicalEnumValue(TechnicalEnumValue {
            qname,
            name,
            parent,
            source,
            position,
        })
    }

    pub fn new_technical_argument(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        source: SourceSpan,
        ty: SymbolHash,
        position: ObjectHash,
    ) -> Self {
        Self::TechnicalArgument(TechnicalArgument {
            qname,
            name,
            parent,
            source,
            ty,
            position,
        })
    }

    pub fn new_technical_return_value(
        qname: SymbolHash,
        name: SymbolHash,
        parent: SymbolHash,
        source: SourceSpan,
        ty: SymbolHash,
        position: ObjectHash,
    ) -> Self {
        Self::TechnicalReturnValue(TechnicalReturnValue {
            qname,
            name,
            parent,
            source,
            ty,
            position,
        })
    }
}
