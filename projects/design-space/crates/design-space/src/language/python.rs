//! Python adapter. Top-level names are prefixed with the module path.

use tree_sitter::Node;

use super::common::{self, Entity};
use crate::types::*;

pub(crate) fn collect(root: Node, source: &str) -> Vec<Entity> {
    let mut out = Vec::new();
    collect_python_children(root, source, &mut out, false);
    out
}

pub(crate) fn prefix(path: &str) -> Option<String> {
    let without = path
        .strip_suffix(".pyi")
        .or_else(|| path.strip_suffix(".py"))?;
    let module = without.replace('/', "::");
    (!module.is_empty()).then_some(module)
}

fn collect_python_children(node: Node, source: &str, out: &mut Vec<Entity>, in_class: bool) {
    for child in common::named_children(node) {
        collect_python_node(child, source, out, in_class);
    }
}

fn collect_python_node(node: Node, source: &str, out: &mut Vec<Entity>, in_class: bool) {
    match node.kind() {
        "class_definition" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "class", source));
            let mut children = Vec::new();
            if let Some(body) = node.child_by_field_name("body") {
                collect_python_children(body, source, &mut children, true);
            }
            out.push(Entity::new(name, PYTHON_CLASS, node, children));
        }
        "function_definition" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "def", source));
            let ty = if in_class {
                PYTHON_METHOD
            } else {
                PYTHON_FUNCTION
            };
            out.push(Entity::new(name, ty, node, Vec::new()));
        }
        "decorated_definition" => {
            if let Some(definition) = node.child_by_field_name("definition") {
                collect_python_node(definition, source, out, in_class);
            } else {
                for child in common::named_children(node) {
                    if matches!(child.kind(), "class_definition" | "function_definition") {
                        collect_python_node(child, source, out, in_class);
                    }
                }
            }
        }
        "expression_statement" => {
            for child in common::named_children(node) {
                collect_python_node(child, source, out, in_class);
            }
        }
        "assignment" => {
            if let Some(left) = node.child_by_field_name("left")
                && left.kind() == "identifier"
            {
                let name = common::text(left, source);
                let ty = if is_constant_name(&name) {
                    PYTHON_CONSTANT
                } else {
                    PYTHON_VARIABLE
                };
                out.push(Entity::new(name, ty, node, Vec::new()));
            }
        }
        _ => {}
    }
}

fn is_constant_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().any(|c| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::{Language, extract_file};

    #[test]
    fn extracts_classes_functions_and_constants() {
        let source = r#"
CONST = 1

class Foo:
    def count(self):
        return 1

def helper():
    pass
"#;
        let elements = extract_file(Language::Python, "a.py", &"0".repeat(64), source).unwrap();
        let names: Vec<_> = elements
            .iter()
            .map(|element| element.name.as_str())
            .collect();
        assert!(names.contains(&"a::CONST"));
        assert!(names.contains(&"a::Foo"));
        assert!(names.contains(&"a::Foo::count"));
        assert!(names.contains(&"a::helper"));

        let constant = elements
            .iter()
            .find(|element| element.name == "a::CONST")
            .unwrap();
        assert_eq!(constant.ty, PYTHON_CONSTANT);
        let method = elements
            .iter()
            .find(|element| element.name == "a::Foo::count")
            .unwrap();
        assert_eq!(method.ty, PYTHON_METHOD);
    }
}
