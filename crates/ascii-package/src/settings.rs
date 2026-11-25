use std::sync::LazyLock;

use ascii_config::{Registry, SettingScope, SettingType, SettingValue};
use serde_json::{Map, Value};

static MANIFEST_SCHEMA: LazyLock<Result<Value, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../schemas/manifest-1.0.json"))
        .map_err(|_| "embedded manifest schema is invalid".to_owned())
});
static SCENE_SCHEMA: LazyLock<Result<Value, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../schemas/scene-1.0.json"))
        .map_err(|_| "embedded scene schema is invalid".to_owned())
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

/// Compares manifest setting properties and scene control keys with CFG-001
/// descriptors without synthesizing descriptors/defaults. An empty foundation
/// registry reports drift.
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
    let scene_schema = SCENE_SCHEMA
        .as_ref()
        .map_err(|_| RegistryComparisonError::InvalidSchema)?;
    compare_control_keys(properties, scene_schema, &mut drift)?;

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
fn compare_control_keys(
    manifest_properties: &Map<String, Value>,
    scene_schema: &Value,
    drift: &mut Vec<RegistryDrift>,
) -> Result<(), RegistryComparisonError> {
    let control_keys = scene_schema
        .pointer("/properties/controls/items/properties/setting/enum")
        .and_then(Value::as_array)
        .ok_or(RegistryComparisonError::InvalidSchema)?;
    let mut unique_keys = std::collections::BTreeSet::new();
    for value in control_keys {
        let key = value
            .as_str()
            .ok_or(RegistryComparisonError::InvalidSchema)?;
        if !unique_keys.insert(key) {
            return Err(RegistryComparisonError::InvalidSchema);
        }
    }
    for key in manifest_properties.keys() {
        if !unique_keys.contains(key.as_str()) {
            drift.push(RegistryDrift {
                key: key.clone(),
                reason: "manifest setting is absent from scene control enum",
            });
        }
    }
    for key in unique_keys {
        if !manifest_properties.contains_key(key) {
            drift.push(RegistryDrift {
                key: key.to_owned(),
                reason: "scene control setting is absent from manifest schema",
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RegistryDrift, compare_control_keys};
    use serde_json::{Map, Value, json};

    fn manifest_properties(keys: &[&str]) -> Map<String, Value> {
        keys.iter()
            .map(|key| ((*key).to_owned(), json!({})))
            .collect()
    }

    fn scene_schema(control_keys: &[&str]) -> Value {
        json!({
            "properties": {
                "controls": {
                    "items": {
                        "properties": {
                            "setting": {
                                "enum": control_keys
                            }
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn reports_manifest_settings_missing_from_control_enum() {
        let manifest = manifest_properties(&["color.mode"]);
        let scene = scene_schema(&[]);
        let mut drift: Vec<RegistryDrift> = Vec::new();
        compare_control_keys(&manifest, &scene, &mut drift).unwrap();
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].key, "color.mode");
        assert_eq!(
            drift[0].reason,
            "manifest setting is absent from scene control enum"
        );
    }

    #[test]
    fn reports_control_keys_missing_from_manifest_settings() {
        let manifest = manifest_properties(&["color.mode"]);
        let scene = scene_schema(&["color.mode", "grid.columns"]);
        let mut drift: Vec<RegistryDrift> = Vec::new();
        compare_control_keys(&manifest, &scene, &mut drift).unwrap();
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].key, "grid.columns");
        assert_eq!(
            drift[0].reason,
            "scene control setting is absent from manifest schema"
        );
    }
}
