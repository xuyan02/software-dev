//! Java adapter. Top-level types are nested under their package.

use tree_sitter::Node;

use super::common::{self, Entity};
use crate::types::*;

pub(crate) fn collect(root: Node, source: &str) -> Vec<Entity> {
    let mut out = Vec::new();
    collect_java_root(root, source, &mut out);
    out
}

fn collect_java_root(node: Node, source: &str, out: &mut Vec<Entity>) {
    let mut package = None;
    let mut children = Vec::new();
    for child in common::named_children(node) {
        match child.kind() {
            "package_declaration" => {
                package = common::named_children(child)
                    .into_iter()
                    .next()
                    .map(|name| common::text(name, source))
                    .filter(|name| !name.is_empty());
            }
            _ => collect_java_node(child, source, &mut children),
        }
    }
    if let Some(name) = package {
        out.push(Entity {
            name,
            full: None,
            ty: JAVA_PACKAGE,
            start: node.start_byte(),
            end: node.end_byte(),
            children,
        });
    } else {
        *out = children;
    }
}

fn collect_java_children(node: Node, source: &str, out: &mut Vec<Entity>) {
    for child in common::named_children(node) {
        collect_java_node(child, source, out);
    }
}

fn collect_java_node(node: Node, source: &str, out: &mut Vec<Entity>) {
    match node.kind() {
        "class_declaration"
        | "interface_declaration"
        | "enum_declaration"
        | "record_declaration"
        | "annotation_type_declaration" => {
            let ty = match node.kind() {
                "class_declaration" => JAVA_CLASS,
                "interface_declaration" => JAVA_INTERFACE,
                "enum_declaration" => JAVA_ENUM,
                "record_declaration" => JAVA_RECORD,
                _ => JAVA_ANNOTATION_TYPE,
            };
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "type", source));
            let mut children = Vec::new();
            if let Some(body) = node.child_by_field_name("body") {
                collect_java_children(body, source, &mut children);
            }
            out.push(Entity::new(name, ty, node, children));
        }
        "method_declaration" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "method", source));
            out.push(Entity::new(name, JAVA_METHOD, node, Vec::new()));
        }
        "constructor_declaration" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "constructor", source));
            out.push(Entity::new(name, JAVA_CONSTRUCTOR, node, Vec::new()));
        }
        "field_declaration" => {
            for child in common::named_children(node) {
                if child.kind() == "variable_declarator"
                    && let Some(name) = common::field_text(child, "name", source)
                {
                    out.push(Entity::new(name, JAVA_FIELD, node, Vec::new()));
                }
            }
        }
        "enum_constant" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "constant", source));
            out.push(Entity::new(name, JAVA_ENUM_CONSTANT, node, Vec::new()));
        }
        "static_initializer" => {
            let name = format!("static@{}", common::line_of(source, node.start_byte()));
            out.push(Entity::new(name, JAVA_STATIC_INITIALIZER, node, Vec::new()));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::{Language, extract_file};

    #[test]
    fn extracts_package_class_fields_and_methods() {
        let source = r#"
package com.acme;

public class Foo {
    private int value;
    public int count() { return value; }
}
"#;
        let elements = extract_file(Language::Java, "Foo.java", &"0".repeat(64), source).unwrap();
        let names: Vec<_> = elements
            .iter()
            .map(|element| element.name.as_str())
            .collect();
        assert!(names.contains(&"com.acme"));
        assert!(names.contains(&"com.acme::Foo"));
        assert!(names.contains(&"com.acme::Foo::value"));
        assert!(names.contains(&"com.acme::Foo::count"));

        let class = elements
            .iter()
            .find(|element| element.name == "com.acme::Foo")
            .unwrap();
        assert_eq!(class.ty, JAVA_CLASS);
    }
}
