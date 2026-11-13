use data_core::{Object, Path, Value, ValueType};
use serde_json::json;

fn object_with_status() -> Value {
    let mut object = Object::new();
    object.set("status", Value::string("failed"));
    object.set("count", Value::integer(3));
    Value::object(object)
}

#[test]
fn small_integers_are_inline_values() {
    let value = Value::integer(42);

    assert_eq!(value.as_i64(), Some(42));
    assert_eq!(value.type_of(), ValueType::Number);
}

#[test]
fn value_is_eight_bytes() {
    assert_eq!(std::mem::size_of::<Value>(), 8);
}

#[test]
fn large_integers_remain_i64() {
    let value = Value::integer(i64::MAX);

    assert_eq!(value.as_i64(), Some(i64::MAX));
    assert_eq!(value.as_f64(), None);
    assert_eq!(value.as_f32(), None);
}

#[test]
fn accessors_are_value_based() {
    let one = Value::number(1.0);
    assert_eq!(one.as_i32(), Some(1));
    assert_eq!(one.as_i64(), Some(1));
    assert_eq!(one.as_f32(), Some(1.0));
    assert_eq!(one.as_f64(), Some(1.0));

    let half = Value::number(1.5);
    assert_eq!(half.as_i32(), None);
    assert_eq!(half.as_i64(), None);
    assert_eq!(half.as_f32(), Some(1.5));
    assert_eq!(half.as_f64(), Some(1.5));

    let tenth = Value::number(0.1);
    assert_eq!(tenth.as_f32(), None);
    assert_eq!(tenth.as_f64(), Some(0.1));

    let exact = Value::integer((1i64 << 53) + 1);
    assert_eq!(exact.as_i64(), Some((1i64 << 53) + 1));
    assert_eq!(exact.as_f64(), None);
    assert_eq!(exact.as_f32(), None);

    assert_eq!(
        Value::integer(1i64 << 53).as_f64(),
        Some((1i64 << 53) as f64)
    );

    let nan = Value::number(f64::NAN);
    assert_eq!(nan.as_i32(), None);
    assert_eq!(nan.as_i64(), None);
    assert_eq!(nan.as_f32(), None);
    assert_eq!(nan.as_f64(), None);
}

#[test]
fn reads_nested_object_fields() {
    let name = Value::string("foundry");

    let mut project = Object::new();
    project.set("name", name);
    let project = Value::object(project);

    let mut root = Object::new();
    root.set("project", project);
    let root = Value::object(root);

    let path = Path::parse("project.name").unwrap();
    assert_eq!(root.get(&path).unwrap().as_str(), Some("foundry"));
}

#[test]
fn reads_array_elements() {
    let one = Value::integer(1);
    let two = Value::integer(2);
    let array = Value::array(vec![one, two]);

    let path = Path::parse("1").unwrap();
    assert_eq!(array.get(&path).unwrap().as_i64(), Some(2));
}

#[test]
fn reports_object_types() {
    let value = object_with_status();

    assert!(matches!(value.type_of(), ValueType::Object(_)));
}

#[test]
fn round_trips_json() {
    let input = json!({
        "status": "failed",
        "count": 3,
        "items": [1, 2, 3],
        "project": {"name": "foundry"}
    });

    let value = Value::from_json(&input).unwrap();
    let output = value.to_json().unwrap();

    assert_eq!(output, input);
}

#[test]
fn rejects_non_finite_floats_in_json() {
    let value = Value::number(f64::NAN);
    assert!(value.to_json().is_err());
}

#[test]
fn rejects_invalid_paths() {
    assert!(Path::parse("").is_err());
    assert!(Path::parse("a..b").is_err());
    assert!(Path::parse("a.1b").is_err());
}

#[test]
fn clones_keep_heap_values_alive() {
    let value = Value::string("kept");
    let clone = value.clone();

    drop(value);

    assert_eq!(clone.as_str(), Some("kept"));
}
