use data_core::{Object, Value};
use template_core::{Template, TemplateError};

fn fixture() -> Value {
    let mut root = Object::new();

    root.set("task", Value::string("login"));
    root.set("count", Value::integer(3));

    let mut project = Object::new();
    project.set("name", Value::string("foundry"));
    root.set("project", Value::object(project));

    root.set(
        "items",
        Value::array(vec![Value::integer(1), Value::integer(2)]),
    );

    root.set("empty", Value::null());

    Value::object(root)
}

#[test]
fn renders_flat_variables() {
    let root = fixture();
    let template = Template::parse("任务：{{task}}，次数：{{count}}").unwrap();

    assert_eq!(template.render(&root).unwrap(), "任务：login，次数：3");
}

#[test]
fn renders_nested_values_and_json_arrays() {
    let root = fixture();
    let template = Template::parse("{{project.name}}|{{items}}").unwrap();

    assert_eq!(template.render(&root).unwrap(), "foundry|[1,2]");
}

#[test]
fn renders_indices_and_literals() {
    let root = fixture();
    let template =
        Template::parse(r#"{{items.0}}|{{items[1]}}|{{"ok"}}|{{3.5}}|{{true}}|{{null}}"#).unwrap();

    assert_eq!(template.render(&root).unwrap(), "1|2|ok|3.5|true|");
}

#[test]
fn evaluates_arithmetic() {
    let root = fixture();
    let template =
        Template::parse("{{1 + 2 * 3}}|{{(1 + 2) * 3}}|{{10 / 4}}|{{10 % 4}}|{{-3 + 1}}").unwrap();

    assert_eq!(template.render(&root).unwrap(), "7|9|2.5|2|-2");
}

#[test]
fn evaluates_comparisons_and_logic() {
    let root = fixture();
    let template = Template::parse(
        r#"{{1 < 2}}|{{2 == 2}}|{{2 != 3}}|{{"a" < "b"}}|{{true && false}}|{{true || missing}}|{{!false}}"#,
    )
    .unwrap();

    assert_eq!(
        template.render(&root).unwrap(),
        "true|true|true|true|false|true|true"
    );
}

#[test]
fn evaluates_ternary_and_string_concat() {
    let root = fixture();
    let template = Template::parse(r#"{{true ? "yes" : "no"}}|{{"a" + 1}}"#).unwrap();

    assert_eq!(template.render(&root).unwrap(), "yes|a1");
}

#[test]
fn short_circuits_logical_operators() {
    let root = fixture();

    assert_eq!(
        Template::parse("{{false && missing}}")
            .unwrap()
            .render(&root)
            .unwrap(),
        "false"
    );
    assert_eq!(
        Template::parse("{{true || missing}}")
            .unwrap()
            .render(&root)
            .unwrap(),
        "true"
    );
}

#[test]
fn fails_on_type_error() {
    let root = fixture();
    let template = Template::parse("{{1 + true}}").unwrap();
    let error = template.render(&root).unwrap_err();

    assert!(matches!(error, TemplateError::Type(_)));
}

#[test]
fn fails_on_missing_field() {
    let root = fixture();
    let template = Template::parse("{{project.missing}}").unwrap();
    let error = template.render(&root).unwrap_err();

    assert!(matches!(error, TemplateError::MissingField(_)));
}

#[test]
fn fails_on_index_out_of_bounds() {
    let root = fixture();
    let template = Template::parse("{{items.9}}").unwrap();
    let error = template.render(&root).unwrap_err();

    assert!(matches!(error, TemplateError::IndexOutOfBounds(9)));
}

#[test]
fn renders_null_as_empty() {
    let root = fixture();
    let template = Template::parse("x{{empty}}y").unwrap();

    assert_eq!(template.render(&root).unwrap(), "xy");
}

#[test]
fn fails_on_missing_variable() {
    let root = fixture();
    let template = Template::parse("{{missing}}").unwrap();
    let error = template.render(&root).unwrap_err();

    assert!(matches!(error, TemplateError::MissingVariable(_)));
}

#[test]
fn fails_on_unclosed_variable() {
    let error = Template::parse("{{task").unwrap_err();
    assert!(matches!(error, TemplateError::UnclosedVariable(_)));
}

#[test]
fn fails_on_empty_variable() {
    let error = Template::parse("{{  }}").unwrap_err();
    assert!(matches!(error, TemplateError::EmptyVariable));
}

#[test]
fn fails_on_invalid_expression() {
    let error = Template::parse("{{a..b}}").unwrap_err();
    assert!(matches!(error, TemplateError::Parse { .. }));
}
