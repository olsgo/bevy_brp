//! Extension traits for JSON field access and string collection utilities
//!
//! This module provides generic traits for:
//! - Type-safe JSON field access using any type that implements `AsRef<str>`
//! - Converting iterators to string collections

use serde_json::Map;
use serde_json::Value;

use crate::brp_tools::BrpTypeName;
use crate::json_schema::SchemaField;

/// JSON Schema reference prefix for type definitions
const SCHEMA_REF_PREFIX: &str = "#/$defs/";

/// Extension trait for type-safe JSON field access
pub trait JsonObjectAccess {
    /// Get field value using any type that can be a string reference
    fn get_field<T: AsRef<str>>(&self, field: T) -> Option<&Value>;

    /// Get field value as string
    fn get_field_str<T: AsRef<str>>(&self, field: T) -> Option<&str>;

    /// Get field value as owned String
    fn get_field_string<T: AsRef<str>>(&self, field: T) -> Option<String> {
        self.get_field_str(field).map(String::from)
    }

    /// Get field value as array
    fn get_field_array<T: AsRef<str>>(&self, field: T) -> Option<&Vec<Value>> {
        self.get_field(field).and_then(Value::as_array)
    }

    /// Insert field with value using any type that converts to String and any value that can become
    /// JSON
    fn insert_field<F, V>(&mut self, field: F, value: V)
    where
        F: Into<String>,
        V: Into<Value>;

    /// Extract a `BrpTypeName` from a field definition that contains a type.$ref structure
    ///
    /// This method expects the JSON value to have the structure:
    /// ```json
    /// { "type": { "$ref": "#/$defs/SomeType" } }
    /// ```
    /// and extracts "`SomeType`" as a `BrpTypeName`.
    fn extract_field_type(&self) -> Option<BrpTypeName> {
        self.get_field(SchemaField::Type)
            .and_then(|t| t.get_field(SchemaField::Ref))
            .and_then(Value::as_str)
            .and_then(|ref_str| ref_str.strip_prefix(SCHEMA_REF_PREFIX))
            .map(BrpTypeName::from)
    }

    /// Extract a single type reference from a schema field (Items, `KeyType`, `ValueType`, etc.)
    fn get_type(&self, field: SchemaField) -> Option<BrpTypeName> {
        let field_value = self.get_field(field)?;
        field_value.extract_field_type()
    }

    /// Get Properties field as a Map
    fn get_properties(&self) -> Option<&Map<String, Value>> {
        self.get_field(SchemaField::Properties)
            .and_then(Value::as_object)
    }

    /// Check if this JSON value represents a complex (non-primitive) type
    /// Complex types (Array, Object) cannot be used as `HashMap` keys or `HashSet` elements in BRP
    fn is_complex_type(&self) -> bool;
}

impl JsonObjectAccess for Value {
    fn get_field<T: AsRef<str>>(&self, field: T) -> Option<&Self> { self.get(field.as_ref()) }

    fn get_field_str<T: AsRef<str>>(&self, field: T) -> Option<&str> {
        self.get(field.as_ref()).and_then(Self::as_str)
    }

    fn insert_field<F, V>(&mut self, field: F, value: V)
    where
        F: Into<String>,
        V: Into<Self>,
    {
        if let Some(obj) = self.as_object_mut() {
            obj.insert(field.into(), value.into());
        }
    }

    fn is_complex_type(&self) -> bool { matches!(self, Self::Array(_) | Self::Object(_)) }
}

impl JsonObjectAccess for Map<String, Value> {
    fn get_field<T: AsRef<str>>(&self, field: T) -> Option<&Value> { self.get(field.as_ref()) }

    fn get_field_str<T: AsRef<str>>(&self, field: T) -> Option<&str> {
        self.get(field.as_ref()).and_then(Value::as_str)
    }

    fn insert_field<F, V>(&mut self, field: F, value: V)
    where
        F: Into<String>,
        V: Into<Value>,
    {
        self.insert(field.into(), value.into());
    }

    fn is_complex_type(&self) -> bool {
        // A Map is always a complex type (JSON Object)
        true
    }
}

/// Coerce string values that look like numbers or booleans into their proper JSON types.
///
/// This is needed because MCP clients may serialize numeric values as strings
/// (e.g., `"5"` instead of `5`), which causes deserialization errors when
/// the target expects a numeric type like `f32`.
///
/// This function recursively processes:
/// - Strings that parse as integers → `Value::Number`
/// - Strings that parse as floats → `Value::Number`
/// - Strings "true"/"false" → `Value::Bool`
/// - Arrays → recursively process each element
/// - Objects → recursively process each value
///
/// # Example
/// ```
/// use serde_json::json;
/// let input = json!({"value": "5", "nested": {"x": "3.14", "flag": "true"}});
/// let output = coerce_string_values(input);
/// // output = {"value": 5, "nested": {"x": 3.14, "flag": true}}
/// ```
pub fn coerce_string_values(value: Value) -> Value {
    match value {
        Value::String(s) => {
            // Try to parse as integer first (more specific)
            if let Ok(n) = s.parse::<i64>() {
                return Value::Number(n.into());
            }
            // Try to parse as float
            if let Ok(f) = s.parse::<f64>() {
                if let Some(n) = serde_json::Number::from_f64(f) {
                    return Value::Number(n);
                }
            }
            // Try to parse as boolean
            match s.as_str() {
                "true" => return Value::Bool(true),
                "false" => return Value::Bool(false),
                _ => {},
            }
            // Keep as string if no conversion applies
            Value::String(s)
        },
        Value::Array(arr) => Value::Array(arr.into_iter().map(coerce_string_values).collect()),
        Value::Object(obj) => {
            Value::Object(obj.into_iter().map(|(k, v)| (k, coerce_string_values(v))).collect())
        },
        // Pass through other types unchanged
        other => other,
    }
}

/// Extension trait for converting iterators to `Vec<String>`
///
/// This trait provides a convenient way to collect iterators of string-convertible
/// items into a vector of strings, replacing the common `.map(String::from).collect()`
/// pattern with a more expressive `.into_strings()` call.
///
/// # Examples
///
/// ```
/// use json_traits::IntoStrings;
///
/// // Convert iterator of &str to Vec<String>
/// let strings = ["a", "b", "c"].iter().into_strings();
///
/// // Works with filter chains
/// let filtered = ["hello", "", "world"]
///     .iter()
///     .filter(|s| !s.is_empty())
///     .into_strings();
///
/// // Works with enums that implement Into<String>
/// let variants = enum_values.iter().into_strings();
/// ```
pub trait IntoStrings<T> {
    /// Convert an iterator of items that can become strings into a `Vec<String>`
    fn into_strings(self) -> Vec<String>;
}

impl<I, T> IntoStrings<T> for I
where
    I: Iterator<Item = T>,
    T: Into<String>,
{
    fn into_strings(self) -> Vec<String> { self.map(Into::into).collect() }
}
