use crate::{Path, PathSegment, ValueError};
use indexmap::IndexMap;
use rustc_hash::FxBuildHasher;
use serde_json::{Map, Number, Value as JsonValue};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

/// An insertion-ordered object mapping string keys to values.
#[derive(Debug, Clone)]
pub struct Object {
    entries: IndexMap<Box<str>, Value, FxBuildHasher>,
}

impl Default for Object {
    fn default() -> Self {
        Self::new()
    }
}

impl Object {
    pub fn new() -> Self {
        Self {
            entries: IndexMap::with_hasher(FxBuildHasher),
        }
    }

    pub fn set(&mut self, key: &str, value: Value) {
        self.entries.insert(key.into(), value);
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Box<str>, &Value)> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

enum HeapKind {
    Float(f64),
    Int64(i64),
    String(Box<str>),
    Array(Vec<Value>),
    Object(Object),
}

struct HeapObject {
    refs: AtomicUsize,
    kind: HeapKind,
}

const TAG_BIT: u64 = 1;
const SPECIAL_NULL: u64 = 1;
const SPECIAL_FALSE: u64 = 3;
const SPECIAL_TRUE: u64 = 5;
const SPECIAL_MAX: u64 = 7;

const SMI_MIN: i64 = -(1 << 53);
const SMI_MAX: i64 = 1 << 53;

const I64_MIN_F64: f64 = -9_223_372_036_854_775_808.0;
const I64_MAX_EXCLUSIVE_F64: f64 = 9_223_372_036_854_775_808.0;

/// A self-contained JavaScript-like value.
///
/// This is a tagged 64-bit word:
///
/// ```text
/// bit 0:
///   0 = small integer (SMI)
///   1 = either a special value or a heap pointer
///
/// special values: 1 = null, 3 = false, 5 = true
/// heap pointers:  address | 1, always greater than 7
/// ```
///
/// Numbers have only one public type. Internally an integer value may live in
/// an SMI, in a heap `Int64`, or in a heap `Float`. The `as_*` methods decide
/// representability from the value, not from that internal representation.
#[repr(transparent)]
#[derive(PartialEq, Eq, Hash)]
pub struct Value(u64);

impl Value {
    pub fn null() -> Self {
        Self(SPECIAL_NULL)
    }

    pub fn bool(value: bool) -> Self {
        if value {
            Self(SPECIAL_TRUE)
        } else {
            Self(SPECIAL_FALSE)
        }
    }

    /// Create an exact integer value.
    pub fn integer(value: i64) -> Self {
        if (SMI_MIN..=SMI_MAX).contains(&value) {
            return Self((value as u64) << 1);
        }
        Self::heap(HeapKind::Int64(value))
    }

    /// Create a number from an `f64`.
    ///
    /// Integer-valued finite numbers are normalized to the integer
    /// representation when they fit in `i64`.
    pub fn number(value: f64) -> Self {
        if value == 0.0 && value.is_sign_negative() {
            return Self::heap(HeapKind::Float(value));
        }
        if let Some(integer) = f64_to_i64_exact(value) {
            return Self::integer(integer);
        }
        Self::heap(HeapKind::Float(value))
    }

    pub fn string(value: impl Into<Box<str>>) -> Self {
        Self::heap(HeapKind::String(value.into()))
    }

    pub fn array(values: Vec<Value>) -> Self {
        Self::heap(HeapKind::Array(values))
    }

    pub fn object(object: Object) -> Self {
        Self::heap(HeapKind::Object(object))
    }

    fn heap(kind: HeapKind) -> Self {
        let object = Box::new(HeapObject {
            refs: AtomicUsize::new(1),
            kind,
        });
        let pointer = Box::into_raw(object);
        Self(pointer as u64 | TAG_BIT)
    }

    fn is_smi(&self) -> bool {
        self.0 & TAG_BIT == 0
    }

    fn is_heap(&self) -> bool {
        self.0 & TAG_BIT == TAG_BIT && self.0 > SPECIAL_MAX
    }

    fn heap_pointer(&self) -> *mut HeapObject {
        (self.0 & !TAG_BIT) as *mut HeapObject
    }

    fn heap_kind(&self) -> Option<&HeapKind> {
        if !self.is_heap() {
            return None;
        }
        // Safety: heap values hold one reference count for this allocation.
        Some(unsafe { &(*self.heap_pointer()).kind })
    }

    fn integer_value(&self) -> Option<i64> {
        if self.is_smi() {
            return Some((self.0 as i64) >> 1);
        }
        match self.heap_kind()? {
            HeapKind::Int64(value) => Some(*value),
            _ => None,
        }
    }

    fn finite_float_value(&self) -> Option<f64> {
        match self.heap_kind()? {
            HeapKind::Float(value) if value.is_finite() => Some(*value),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        self.0 == SPECIAL_NULL
    }

    pub fn as_bool(&self) -> Option<bool> {
        if self.0 == SPECIAL_TRUE {
            Some(true)
        } else if self.0 == SPECIAL_FALSE {
            Some(false)
        } else {
            None
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        i32::try_from(self.as_i64()?).ok()
    }

    pub fn as_i64(&self) -> Option<i64> {
        if let Some(value) = self.integer_value() {
            return Some(value);
        }
        f64_to_i64_exact(self.finite_float_value()?)
    }

    pub fn as_f32(&self) -> Option<f32> {
        if let Some(value) = self.integer_value() {
            return i64_to_f32_exact(value);
        }
        f64_to_f32_exact(self.finite_float_value()?)
    }

    pub fn as_f64(&self) -> Option<f64> {
        if let Some(value) = self.integer_value() {
            return i64_to_f64_exact(value);
        }
        self.finite_float_value()
    }

    pub fn as_str(&self) -> Option<&str> {
        match self.heap_kind()? {
            HeapKind::String(value) => Some(value.as_ref()),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self.heap_kind()? {
            HeapKind::Array(values) => Some(values.as_slice()),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self.heap_kind()? {
            HeapKind::Object(object) => Some(object),
            _ => None,
        }
    }

    pub fn type_of(&self) -> ValueType {
        if self.is_null() {
            return ValueType::Null;
        }
        if self.as_bool().is_some() {
            return ValueType::Bool;
        }
        if self.integer_value().is_some() || self.finite_float_value().is_some() {
            return ValueType::Number;
        }

        match self.heap_kind().expect("heap variant has a kind") {
            HeapKind::Float(_) | HeapKind::Int64(_) => ValueType::Number,
            HeapKind::String(_) => ValueType::String,
            HeapKind::Array(_) => ValueType::Array,
            HeapKind::Object(object) => ValueType::Object(
                object
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.type_of()))
                    .collect(),
            ),
        }
    }

    pub fn get(&self, path: &Path) -> Option<Value> {
        let mut current = self.clone();

        for segment in path.segments() {
            current = match segment {
                PathSegment::Key(key) => current.as_object()?.get(key)?.clone(),
                PathSegment::Index(index) => current.as_array()?.get(*index)?.clone(),
            };
        }

        Some(current)
    }

    pub fn to_json(&self) -> Result<JsonValue, ValueError> {
        if self.is_null() {
            return Ok(JsonValue::Null);
        }
        if let Some(value) = self.as_bool() {
            return Ok(JsonValue::Bool(value));
        }
        if let Some(value) = self.integer_value() {
            return Ok(JsonValue::Number(value.into()));
        }
        if let Some(value) = self.finite_float_value() {
            return Ok(JsonValue::Number(
                Number::from_f64(value).expect("finite float"),
            ));
        }

        match self.heap_kind().expect("non-scalar value has a kind") {
            HeapKind::Float(_) => Err(ValueError::NonFiniteNumber),
            HeapKind::Int64(_) => unreachable!("integer values handled above"),
            HeapKind::String(value) => Ok(JsonValue::String(value.to_string())),
            HeapKind::Array(values) => {
                let values = values
                    .iter()
                    .map(Value::to_json)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(JsonValue::Array(values))
            }
            HeapKind::Object(object) => {
                let mut map = Map::new();
                for (key, value) in object.iter() {
                    map.insert(key.to_string(), value.to_json()?);
                }
                Ok(JsonValue::Object(map))
            }
        }
    }

    pub fn from_json(value: &JsonValue) -> Result<Value, ValueError> {
        match value {
            JsonValue::Null => Ok(Value::null()),
            JsonValue::Bool(value) => Ok(Value::bool(*value)),
            JsonValue::Number(number) => {
                if let Some(value) = number.as_i64() {
                    return Ok(Value::integer(value));
                }
                if let Some(value) = number.as_u64() {
                    return match i64::try_from(value) {
                        Ok(value) => Ok(Value::integer(value)),
                        Err(_) => Ok(Value::number(value as f64)),
                    };
                }
                if let Some(value) = number.as_f64() {
                    return Ok(Value::number(value));
                }
                Err(ValueError::NonFiniteNumber)
            }
            JsonValue::String(value) => Ok(Value::string(value.as_str())),
            JsonValue::Array(values) => {
                let values = values
                    .iter()
                    .map(Value::from_json)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Value::array(values))
            }
            JsonValue::Object(map) => {
                let mut object = Object::new();
                for (key, value) in map {
                    object.set(key.as_str(), Value::from_json(value)?);
                }
                Ok(Value::object(object))
            }
        }
    }
}

fn f64_to_i64_exact(value: f64) -> Option<i64> {
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }
    if !(I64_MIN_F64..I64_MAX_EXCLUSIVE_F64).contains(&value) {
        return None;
    }

    let integer = value as i64;
    if integer as f64 == value {
        Some(integer)
    } else {
        None
    }
}

fn i64_to_f64_exact(value: i64) -> Option<f64> {
    let float = value as f64;
    if !(I64_MIN_F64..I64_MAX_EXCLUSIVE_F64).contains(&float) {
        return None;
    }
    if float as i64 == value {
        Some(float)
    } else {
        None
    }
}

fn f64_to_f32_exact(value: f64) -> Option<f32> {
    if !value.is_finite() {
        return None;
    }

    let narrowed = value as f32;
    if !narrowed.is_finite() {
        return None;
    }
    if narrowed as f64 == value {
        Some(narrowed)
    } else {
        None
    }
}

fn i64_to_f32_exact(value: i64) -> Option<f32> {
    f64_to_f32_exact(i64_to_f64_exact(value)?)
}

impl Clone for Value {
    fn clone(&self) -> Self {
        if self.is_heap() {
            // Safety: the pointer belongs to a live heap allocation.
            unsafe {
                (*self.heap_pointer()).refs.fetch_add(1, Ordering::Relaxed);
            }
        }
        Value(self.0)
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        if !self.is_heap() {
            return;
        }

        // Safety: `pointer` was allocated by `Value::heap`.
        let pointer = self.heap_pointer();
        let previous = unsafe { (*pointer).refs.fetch_sub(1, Ordering::AcqRel) };

        if previous == 1 {
            // Safety: this was the last reference.
            unsafe {
                drop(Box::from_raw(pointer));
            }
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_null() {
            return f.write_str("Null");
        }
        if let Some(value) = self.as_bool() {
            return f.debug_tuple("Bool").field(&value).finish();
        }
        if let Some(value) = self.integer_value() {
            return f.debug_tuple("Number").field(&value).finish();
        }
        if let Some(value) = self.finite_float_value() {
            return f.debug_tuple("Number").field(&value).finish();
        }

        match self.heap_kind().expect("heap variant has a kind") {
            HeapKind::Float(value) => f.debug_tuple("Number").field(value).finish(),
            HeapKind::Int64(value) => f.debug_tuple("Number").field(value).finish(),
            HeapKind::String(value) => f.debug_tuple("String").field(value).finish(),
            HeapKind::Array(values) => f.debug_tuple("Array").field(values).finish(),
            HeapKind::Object(object) => f.debug_tuple("Object").field(object).finish(),
        }
    }
}

// Safety: heap objects are immutable and the reference count is atomic.
unsafe impl Send for Value {}
unsafe impl Sync for Value {}

/// Runtime type of a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueType {
    Null,
    Bool,
    Number,
    String,
    Array,
    Object(BTreeMap<String, ValueType>),
}
