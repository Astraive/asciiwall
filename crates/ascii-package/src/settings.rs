use std::sync::LazyLock;

use ascii_config::{Registry, SettingScope, SettingType, SettingValue};
use serde_json::{Map, Value};

static MANIFEST_SCHEMA: LazyLock<Result<Value, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../schemas/manifest-1.0.json"))
        .map_err(|_| "embedded manifest schema is invalid".to_owned())
});

/// A portable metadata mismatch between the package schema and CFG registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryDrift {
    pub key: String,
    pub reason: &'static str,
}

/// A failure to load the schema or detected portable-registry drift.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistryComparisonError {
    InvalidSchema,
    Drift(Vec<RegistryDrift>),
}

/// Compares every portable schema property with CFG-001 descriptors without
/// synthesizing descriptors/defaults. An empty foundation registry reports drift.
pub fn compare_portable_settings(registry: &Registry) -> Result<(), RegistryComparisonError> {
    let schema = MANIFEST_SCHEMA
        .as_ref()
        .map_err(|_| RegistryComparisonError::InvalidSchema)?;
    let properties = schema
        .pointer("/properties/settings/properties")
        .and_then(Value::as_object)
        .ok_or(RegistryComparisonError::InvalidSchema)?;
    let mut drift = Vec::new();
    for (key, property) in properties {
        let property = property
            .as_object()
            .ok_or(RegistryComparisonError::InvalidSchema)?;
        match registry.get_str(key) {
            Some(descriptor) => compare_descriptor(key, property, descriptor, &mut drift),
            None => drift.push(RegistryDrift {
                key: key.clone(),
                reason: "missing CFG descriptor",
            }),
        }
    }
    for descriptor in registry.descriptors() {
        if descriptor.scope() == SettingScope::P
            && !properties.contains_key(descriptor.key().as_str())
        {
            drift.push(RegistryDrift {
                key: descriptor.key().to_string(),
                reason: "portable CFG descriptor is absent from manifest schema",
            });
        }
    }
    if drift.is_empty() {
        Ok(())
    } else {
        Err(RegistryComparisonError::Drift(drift))
    }
}

fn compare_descriptor(
    key: &str,
    property: &Map<String, Value>,
    descriptor: &ascii_config::SettingDescriptor,
    drift: &mut Vec<RegistryDrift>,
) {
    let mut mismatch = |reason| {
        drift.push(RegistryDrift {
            key: key.to_owned(),
            reason,
        })
    };
    if descriptor.scope() != SettingScope::P {
        mismatch("schema setting is not portable in CFG registry");
        return;
    }
    let matches_type = match (
        descriptor.value_type(),
        property.get("type").and_then(Value::as_str),
    ) {
        (SettingType::Boolean, Some("boolean")) => true,
        (SettingType::Integer { min, max }, Some("integer")) => {
            property.get("minimum").and_then(Value::as_i64) == Some(*min)
                && property.get("maximum").and_then(Value::as_i64) == Some(*max)
        }
        (SettingType::Number { min, max }, Some("number")) => {
            property.get("minimum").and_then(Value::as_f64) == Some(*min)
                && property.get("maximum").and_then(Value::as_f64) == Some(*max)
        }
        (SettingType::String { max_chars }, Some("string")) => {
            property.get("maxLength").and_then(Value::as_u64) == Some(*max_chars as u64)
        }
        (SettingType::Enum { values }, Some("string")) => property
            .get("enum")
            .and_then(Value::as_array)
            .is_some_and(|schema_values| {
                schema_values.len() == values.len()
                    && schema_values
                        .iter()
                        .zip(values)
                        .all(|(schema, registry)| schema.as_str() == Some(registry.as_ref()))
            }),
        _ => false,
    };
    if !matches_type {
        mismatch("CFG type, enum, or range differs from schema");
    }
    let matches_default = match (descriptor.default(), property.get("default")) {
        (SettingValue::Boolean(expected), Some(actual)) => actual.as_bool() == Some(*expected),
        (SettingValue::Integer(expected), Some(actual)) => actual.as_i64() == Some(*expected),
        (SettingValue::Number(expected), Some(actual)) => actual.as_f64() == Some(*expected),
        (SettingValue::Text(expected), Some(actual)) => actual.as_str() == Some(expected.as_ref()),
        _ => false,
    };
    if !matches_default {
        mismatch("CFG default differs from schema annotation");
    }
}
