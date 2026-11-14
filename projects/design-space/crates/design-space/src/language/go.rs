//! Go adapter. Top-level names are prefixed with the package name.

use tree_sitter::Node;

use super::common::{self, Entity};
use crate::types::*;

pub(crate) fn collect(root: Node, source: &str) -> Vec<Entity> {
    let mut out = Vec::new();
    collect_go_children(root, source, &mut out);
    out
}

pub(crate) fn prefix(root: Node, source: &str, path: &str) -> Option<String> {
    go_package(root, source).or_else(|| common::file_stem(path))
}

fn go_package(root: Node, source: &str) -> Option<String> {
    for child in common::named_children(root) {
        if child.kind() == "package_clause"
            && let Some(name) = common::named_children(child).into_iter().next()
        {
            let value = common::text(name, source);
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn collect_go_children(node: Node, source: &str, out: &mut Vec<Entity>) {
    for child in common::named_children(node) {
        collect_go_node(child, source, out);
    }
}

fn collect_go_node(node: Node, source: &str, out: &mut Vec<Entity>) {
    match node.kind() {
        "function_declaration" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "func", source));
            out.push(Entity::new(name, GO_FUNCTION, node, Vec::new()));
        }
        "method_declaration" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "method", source));
            let name = match node
                .child_by_field_name("receiver")
                .and_then(|receiver| go_receiver_name(receiver, source))
            {
                Some(receiver) => format!("{receiver}::{name}"),
                None => name,
            };
            out.push(Entity::new(name, GO_METHOD, node, Vec::new()));
        }
        "type_declaration" => {
            for child in common::named_children(node) {
                if child.kind() == "type_spec" {
                    collect_go_type_spec(child, source, out);
                }
            }
        }
        "const_declaration" => {
            for child in common::named_children(node) {
                if child.kind() == "const_spec" {
                    collect_go_value_spec(child, source, out, GO_CONST);
                }
            }
        }
        "var_declaration" => {
            for child in common::named_children(node) {
                if child.kind() == "var_spec" {
                    collect_go_value_spec(child, source, out, GO_VARIABLE);
                }
            }
        }
        _ => {}
    }
}

fn collect_go_type_spec(node: Node, source: &str, out: &mut Vec<Entity>) {
    let name = common::field_text(node, "name", source)
        .unwrap_or_else(|| common::anonymous(node, "type", source));
    let type_node = node.child_by_field_name("type");
    let (ty, children) = match type_node.map(|child| child.kind()) {
        Some("struct_type") => (
            GO_STRUCT,
            type_node
                .map(|child| go_fields(child, source))
                .unwrap_or_default(),
        ),
        Some("interface_type") => (
            GO_INTERFACE,
            type_node
                .map(|child| go_interface_methods(child, source))
                .unwrap_or_default(),
        ),
        _ => (GO_TYPE_DEFINITION, Vec::new()),
    };
    out.push(Entity::new(name, ty, node, children));
}

fn go_receiver_name(node: Node, source: &str) -> Option<String> {
    let parameter = common::named_children(node).into_iter().next()?;
    let type_node = parameter.child_by_field_name("type")?;
    let text = common::text(type_node, source);
    let text = text.trim_start_matches('*');
    let text = text.split('[').next().unwrap_or(text).trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn go_fields(node: Node, source: &str) -> Vec<Entity> {
    let mut out = Vec::new();
    for child in common::named_children(node) {
        match child.kind() {
            "field_declaration_list" => out.extend(go_fields(child, source)),
            "field_declaration" => {
                let names: Vec<Node> = common::named_children(child)
                    .into_iter()
                    .filter(|candidate| candidate.kind() == "field_identifier")
                    .collect();
                if names.is_empty() {
                    if let Some(type_node) = child.child_by_field_name("type") {
                        let name = common::text(type_node, source);
                        out.push(Entity::new(name, GO_FIELD, child, Vec::new()));
                    }
                } else {
                    for name_node in names {
                        let name = common::text(name_node, source);
                        out.push(Entity::new(name, GO_FIELD, child, Vec::new()));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn go_interface_methods(node: Node, source: &str) -> Vec<Entity> {
    let mut out = Vec::new();
    for child in common::named_children(node) {
        if child.kind() == "method_elem" {
            let name = common::field_text(child, "name", source)
                .unwrap_or_else(|| common::anonymous(child, "method", source));
            out.push(Entity::new(name, GO_METHOD, child, Vec::new()));
        }
    }
    out
}

fn collect_go_value_spec(node: Node, source: &str, out: &mut Vec<Entity>, ty: u32) {
    if let Some(name) = common::field_text(node, "name", source) {
        out.push(Entity::new(name, ty, node, Vec::new()));
        return;
    }
    for child in common::named_children(node) {
        if child.kind() == "identifier" {
            let name = common::text(child, source);
            out.push(Entity::new(name, ty, node, Vec::new()));
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::{Language, extract_file};

    #[test]
    fn extracts_package_types_methods_and_values() {
        let source = r#"
package demo

const Limit = 10

type Foo struct {
    Value int
}

func (f *Foo) Count() int { return f.Value }

func Helper() {}
"#;
        let elements = extract_file(Language::Go, "a.go", &"0".repeat(64), source).unwrap();
        let names: Vec<_> = elements
            .iter()
            .map(|element| element.name.as_str())
            .collect();
        assert!(names.contains(&"demo::Limit"));
        assert!(names.contains(&"demo::Foo"));
        assert!(names.contains(&"demo::Foo::Value"));
        assert!(names.contains(&"demo::Foo::Count"));
        assert!(names.contains(&"demo::Helper"));

        let foo = elements
            .iter()
            .find(|element| element.name == "demo::Foo")
            .unwrap();
        assert_eq!(foo.ty, GO_STRUCT);
        assert!(foo.children.iter().any(|child| child == "demo::Foo::Value"));
    }
}
