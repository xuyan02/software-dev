//! C++ adapter.
//!
//! Preprocessor directives and macros are kept as written; nothing is
//! expanded. Conditional blocks become containers.

use tree_sitter::Node;

use super::common::{self, Entity};
use crate::types::*;

pub(crate) fn collect(root: Node, source: &str) -> Vec<Entity> {
    let mut out = Vec::new();
    collect_cpp_children(root, source, &mut out, false);
    out
}

fn collect_cpp_children(node: Node, source: &str, out: &mut Vec<Entity>, in_type: bool) {
    for child in common::named_children(node) {
        collect_cpp_node(child, source, out, in_type);
    }
    merge_cpp_definitions(out);
}

/// Replace an in-class declaration with its out-of-line definition.
fn merge_cpp_definitions(entities: &mut Vec<Entity>) {
    if !entities
        .iter()
        .any(|entity| entity.ty == CPP_FUNCTION && entity.name.contains("::"))
    {
        return;
    }

    let all = std::mem::take(entities);
    let mut kept = Vec::new();
    let mut definitions = Vec::new();
    for entity in all {
        if entity.ty == CPP_FUNCTION && entity.name.contains("::") {
            definitions.push(entity);
        } else {
            kept.push(entity);
        }
    }

    for definition in definitions {
        let mut path: Vec<String> = definition
            .name
            .split("::")
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect();
        let Some(local) = path.pop() else {
            kept.push(definition);
            continue;
        };

        let mut merged = false;
        if let Some(parent) = find_entity_mut(&mut kept, &path)
            && let Some(position) = parent.children.iter().position(|child| child.name == local)
        {
            let old = parent.children.remove(position);
            let mut replacement = definition.clone();
            replacement.name = local;
            replacement.ty = old.ty;
            parent.children.push(replacement);
            merged = true;
        }
        if !merged {
            kept.push(definition);
        }
    }

    *entities = kept;
}

fn find_entity_mut<'a>(entities: &'a mut [Entity], path: &[String]) -> Option<&'a mut Entity> {
    let (first, rest) = path.split_first()?;
    let entity = entities.iter_mut().find(|entity| entity.name == *first)?;
    if rest.is_empty() {
        Some(entity)
    } else {
        find_entity_mut(&mut entity.children, rest)
    }
}

fn collect_cpp_node(node: Node, source: &str, out: &mut Vec<Entity>, in_type: bool) {
    match node.kind() {
        "namespace_definition" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "namespace", source));
            let mut children = Vec::new();
            if let Some(body) = node.child_by_field_name("body") {
                collect_cpp_children(body, source, &mut children, false);
            } else {
                for child in common::named_children(node) {
                    if child.kind() == "declaration_list" {
                        collect_cpp_children(child, source, &mut children, false);
                    }
                }
            }
            out.push(Entity::new(name, CPP_NAMESPACE, node, children));
        }
        "class_specifier" | "struct_specifier" | "union_specifier" => {
            let ty = match node.kind() {
                "class_specifier" => CPP_CLASS,
                "struct_specifier" => CPP_STRUCT,
                _ => CPP_UNION,
            };
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "type", source));
            let mut children = Vec::new();
            if let Some(body) = node.child_by_field_name("body") {
                collect_cpp_children(body, source, &mut children, true);
            } else {
                for child in common::named_children(node) {
                    if child.kind() == "field_declaration_list" {
                        collect_cpp_children(child, source, &mut children, true);
                    }
                }
            }
            out.push(Entity::new(name, ty, node, children));
        }
        "enum_specifier" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "enum", source));
            let mut children = Vec::new();
            if let Some(body) = node.child_by_field_name("body") {
                collect_cpp_enumerators(body, source, &mut children);
            }
            out.push(Entity::new(name, CPP_ENUM, node, children));
        }
        "function_definition" => {
            let name = node
                .child_by_field_name("declarator")
                .and_then(|declarator| declarator_name(declarator, source))
                .unwrap_or_else(|| common::anonymous(node, "function", source));
            out.push(Entity::new(
                name,
                if in_type { CPP_METHOD } else { CPP_FUNCTION },
                node,
                Vec::new(),
            ));
        }
        "declaration" | "field_declaration" => {
            let Some(declarator) = find_declarator(node) else {
                return;
            };
            let is_function = contains_kind(declarator, "function_declarator");
            let name = declarator_name(declarator, source)
                .unwrap_or_else(|| common::anonymous(node, "declaration", source));
            let ty = if is_function {
                if in_type { CPP_METHOD } else { CPP_FUNCTION }
            } else if in_type {
                CPP_FIELD
            } else {
                CPP_GLOBAL_VARIABLE
            };
            out.push(Entity::new(name, ty, node, Vec::new()));
        }
        "template_declaration" => {
            for child in common::named_children(node) {
                collect_cpp_node(child, source, out, in_type);
            }
        }
        "linkage_specification" => {
            if let Some(body) = node.child_by_field_name("body") {
                collect_cpp_children(body, source, out, in_type);
            } else {
                for child in common::named_children(node) {
                    if child.kind() == "declaration_list" {
                        collect_cpp_children(child, source, out, in_type);
                    }
                }
            }
        }
        "preproc_def" | "preproc_function_def" => {
            let ty = if node.kind() == "preproc_def" {
                CPP_MACRO_OBJECT
            } else {
                CPP_MACRO_FUNCTION
            };
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "macro", source));
            out.push(Entity::new(name, ty, node, Vec::new()));
        }
        "preproc_include" => {
            let name = common::field_text(node, "path", source)
                .unwrap_or_else(|| common::anonymous(node, "include", source));
            out.push(Entity::new(name, CPP_INCLUDE, node, Vec::new()));
        }
        "preproc_ifdef" | "preproc_if" | "preproc_elif" | "preproc_else" => {
            let first_line = common::text(node, source);
            let first_line = first_line.lines().next().unwrap_or(node.kind()).trim();
            let name = format!(
                "{first_line}@{}",
                common::line_of(source, node.start_byte())
            );
            let mut children = Vec::new();
            collect_cpp_children(node, source, &mut children, in_type);
            out.push(Entity::new(name, CPP_PREPROC_CONDITIONAL, node, children));
        }
        _ => {}
    }
}

fn collect_cpp_enumerators(node: Node, source: &str, out: &mut Vec<Entity>) {
    for child in common::named_children(node) {
        if child.kind() == "enumerator" {
            let name = common::field_text(child, "name", source)
                .unwrap_or_else(|| common::anonymous(child, "enumerator", source));
            out.push(Entity::new(name, CPP_ENUMERATOR, child, Vec::new()));
        }
    }
}

fn contains_kind(node: Node, kind: &str) -> bool {
    if node.kind() == kind {
        return true;
    }
    common::named_children(node)
        .into_iter()
        .any(|child| contains_kind(child, kind))
}

fn declarator_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "identifier"
        | "field_identifier"
        | "type_identifier"
        | "namespace_identifier"
        | "destructor_name"
        | "operator_name"
        | "qualified_identifier" => {
            let value = common::text(node, source);
            (!value.is_empty()).then_some(value)
        }
        _ => {
            if let Some(inner) = node.child_by_field_name("declarator")
                && let Some(name) = declarator_name(inner, source)
            {
                return Some(name);
            }
            for child in common::named_children(node) {
                if let Some(name) = declarator_name(child, source) {
                    return Some(name);
                }
            }
            None
        }
    }
}

fn find_declarator(node: Node) -> Option<Node> {
    for child in common::named_children(node) {
        match child.kind() {
            "init_declarator"
            | "function_declarator"
            | "pointer_declarator"
            | "reference_declarator"
            | "array_declarator"
            | "identifier"
            | "field_identifier"
            | "qualified_identifier" => return Some(child),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::{Language, extract_file};

    #[test]
    fn extracts_namespaces_classes_functions_and_macros() {
        let source = r#"
#define LIMIT 10
#include <vector>
namespace acme {
class Foo {
public:
    int count() const;
    int value;
};
int Foo::count() const { return value; }
}
#ifdef DEBUG
void trace();
#endif
"#;
        let elements = extract_file(Language::Cpp, "src/a.cpp", &"0".repeat(64), source).unwrap();
        let names: Vec<_> = elements
            .iter()
            .map(|element| element.name.as_str())
            .collect();
        assert!(names.contains(&"LIMIT"));
        assert!(names.contains(&"<vector>"));
        assert!(names.contains(&"acme"));
        assert!(names.contains(&"acme::Foo"));
        assert!(names.contains(&"acme::Foo::count"));
        assert_eq!(
            names
                .iter()
                .filter(|name| **name == "acme::Foo::count")
                .count(),
            1,
            "declaration and definition should merge"
        );
        assert!(names.contains(&"acme::Foo::value"));
        assert!(names.iter().any(|name| name.starts_with("#ifdef DEBUG")));

        let class = elements
            .iter()
            .find(|element| element.name == "acme::Foo")
            .unwrap();
        assert_eq!(class.ty, CPP_CLASS);
        assert!(
            class
                .children
                .iter()
                .any(|child| child == "acme::Foo::count")
        );
        assert!(
            class
                .children
                .iter()
                .any(|child| child == "acme::Foo::value")
        );
    }

    #[test]
    fn own_text_excludes_children() {
        let source = "class Foo { int a; int b; };";
        let elements = extract_file(Language::Cpp, "a.cpp", &"0".repeat(64), source).unwrap();
        let class = elements
            .iter()
            .find(|element| element.name == "Foo")
            .unwrap();
        assert!(class.own_text.contains("class Foo"));
        assert!(!class.own_text.contains("int a;"));
        assert!(!class.own_text.contains("int b;"));
    }
}
