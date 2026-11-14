//! Rust adapter. `impl` methods are attached to their type.

use tree_sitter::Node;

use super::common::{self, Entity};
use crate::types::*;

pub(crate) fn collect(root: Node, source: &str) -> Vec<Entity> {
    let mut out = Vec::new();
    collect_rust_children(root, source, &mut out, false);
    out
}

pub(crate) fn prefix(path: &str) -> Option<String> {
    let without = path.strip_suffix(".rs")?;
    let without = without.strip_prefix("src/").unwrap_or(without);
    if without == "lib" || without == "main" {
        return Some("crate".to_owned());
    }
    let module = without.replace('/', "::");
    (!module.is_empty()).then_some(module)
}

fn collect_rust_children(node: Node, source: &str, out: &mut Vec<Entity>, in_impl: bool) {
    for child in common::named_children(node) {
        collect_rust_node(child, source, out, in_impl);
    }
    merge_impls(out);
}

fn collect_rust_node(node: Node, source: &str, out: &mut Vec<Entity>, in_impl: bool) {
    match node.kind() {
        "mod_item" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "mod", source));
            let mut children = Vec::new();
            if let Some(body) = node.child_by_field_name("body") {
                collect_rust_children(body, source, &mut children, false);
            } else {
                for child in common::named_children(node) {
                    if child.kind() == "declaration_list" {
                        collect_rust_children(child, source, &mut children, false);
                    }
                }
            }
            out.push(Entity::new(name, RUST_MODULE, node, children));
        }
        "struct_item" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "struct", source));
            let mut children = Vec::new();
            if let Some(body) = node.child_by_field_name("body") {
                collect_rust_children(body, source, &mut children, false);
            }
            out.push(Entity::new(name, RUST_STRUCT, node, children));
        }
        "enum_item" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "enum", source));
            let mut children = Vec::new();
            if let Some(body) = node.child_by_field_name("body") {
                collect_rust_children(body, source, &mut children, false);
            }
            out.push(Entity::new(name, RUST_ENUM, node, children));
        }
        "trait_item" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "trait", source));
            let mut children = Vec::new();
            if let Some(body) = node.child_by_field_name("body") {
                collect_rust_children(body, source, &mut children, true);
            }
            out.push(Entity::new(name, RUST_TRAIT, node, children));
        }
        "impl_item" => {
            let target = common::field_text(node, "type", source)
                .unwrap_or_else(|| common::anonymous(node, "impl", source));
            let name = match common::field_text(node, "trait", source) {
                Some(trait_name) => format!(
                    "impl {trait_name} for {target}@{}",
                    common::line_of(source, node.start_byte())
                ),
                None => format!(
                    "impl {target}@{}",
                    common::line_of(source, node.start_byte())
                ),
            };
            let mut children = Vec::new();
            if let Some(body) = node.child_by_field_name("body") {
                collect_rust_children(body, source, &mut children, true);
            }
            out.push(Entity::new(name, RUST_IMPL_BLOCK, node, children));
        }
        "function_item" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "fn", source));
            let ty = if in_impl { RUST_METHOD } else { RUST_FUNCTION };
            out.push(Entity::new(name, ty, node, Vec::new()));
        }
        "const_item" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "const", source));
            out.push(Entity::new(name, RUST_CONST, node, Vec::new()));
        }
        "static_item" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "static", source));
            out.push(Entity::new(name, RUST_STATIC, node, Vec::new()));
        }
        "type_item" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "type", source));
            out.push(Entity::new(name, RUST_TYPE_ALIAS, node, Vec::new()));
        }
        "macro_definition" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "macro", source));
            out.push(Entity::new(name, RUST_MACRO_RULES, node, Vec::new()));
        }
        "field_declaration" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "field", source));
            out.push(Entity::new(name, RUST_FIELD, node, Vec::new()));
        }
        "enum_variant" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "variant", source));
            out.push(Entity::new(name, RUST_ENUM_VARIANT, node, Vec::new()));
        }
        "associated_type" => {
            let name = common::field_text(node, "name", source)
                .unwrap_or_else(|| common::anonymous(node, "assoc_type", source));
            out.push(Entity::new(name, RUST_ASSOC_TYPE, node, Vec::new()));
        }
        _ => {}
    }
}

/// Attach methods from `impl Foo` blocks to the `Foo` type entity.
fn merge_impls(entities: &mut Vec<Entity>) {
    if !entities.iter().any(|entity| entity.ty == RUST_IMPL_BLOCK) {
        return;
    }
    let all = std::mem::take(entities);
    let mut kept = Vec::new();
    let mut impls = Vec::new();
    for entity in all {
        if entity.ty == RUST_IMPL_BLOCK {
            impls.push(entity);
        } else {
            kept.push(entity);
        }
    }
    for mut impl_entity in impls {
        let target = impl_target(&impl_entity.name);
        let mut merged = false;
        if let Some(target) = target
            && let Some(type_entity) = kept.iter_mut().find(|entity| entity.name == target)
        {
            type_entity.children.append(&mut impl_entity.children);
            merged = true;
        }
        if !merged {
            kept.push(impl_entity);
        }
    }
    *entities = kept;
}

fn impl_target(name: &str) -> Option<String> {
    let head = name.split('@').next()?;
    let rest = head.strip_prefix("impl ")?;
    let target = rest.rsplit(" for ").next()?;
    let target = target.split('<').next()?.trim();
    let target = target.rsplit("::").next()?.trim();
    (!target.is_empty()).then(|| target.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::{Language, extract_file};

    #[test]
    fn extracts_items_and_merges_impls() {
        let source = r#"
mod inner {
    pub struct Foo { pub size: usize }
    impl Foo {
        pub fn count(&self) -> usize { self.size }
    }
}
fn helper() {}
"#;
        let elements = extract_file(Language::Rust, "src/lib.rs", &"0".repeat(64), source).unwrap();
        let names: Vec<_> = elements
            .iter()
            .map(|element| element.name.as_str())
            .collect();
        assert!(names.contains(&"crate::inner"));
        assert!(names.contains(&"crate::inner::Foo"));
        assert!(names.contains(&"crate::inner::Foo::count"));
        assert!(names.contains(&"crate::helper"));

        let method = elements
            .iter()
            .find(|element| element.name == "crate::inner::Foo::count")
            .unwrap();
        assert_eq!(method.ty, RUST_METHOD);
        assert_eq!(method.parent.as_deref(), Some("crate::inner::Foo"));
    }
}
