use std::{fmt, sync::LazyLock};

use crate::{AssetInventory, Manifest, Scene, ValidatedManifest};

/// Maximum byte size accepted for an untrusted manifest document.
pub const MAX_MANIFEST_BYTES: usize = 1_048_576;
/// Maximum byte size accepted for an untrusted scene document.
pub const MAX_SCENE_BYTES: usize = 4_194_304;
/// Maximum nested JSON object/array depth accepted by package contracts.
pub const MAX_JSON_DEPTH: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    TooLarge,
    Structural,
    UnsupportedFormat,
    UnsupportedSceneVersion,
    SchemaDefinition,
    InventoryMismatch,
}

/// Validates scene structure and its bounded, declarative parameter values.
pub fn validate_scene(bytes: &[u8]) -> Result<Scene, Error> {
    if bytes.len() > MAX_SCENE_BYTES {
        return Err(Error {
            kind: ErrorKind::TooLarge,
            location: "$".into(),
            message: format!("scene exceeds {MAX_SCENE_BYTES} bytes"),
        });
    }
    check_json_depth(bytes)?;
    let document: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| Error {
        kind: ErrorKind::Structural,
        location: json_location(&error),
        message: "scene document is not valid JSON".into(),
    })?;
    if document
        .get("scene_version")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|version| version != "1.0")
    {
        return Err(Error {
            kind: ErrorKind::UnsupportedSceneVersion,
            location: "$.scene_version".into(),
            message: "only scene version 1.0 is supported".into(),
        });
    }
    validate_schema(scene_validator()?, &document, "scene")?;
    let scene: Scene = serde_json::from_value(document)
        .map_err(|_| structural("$", "scene type conversion failed"))?;
    Ok(scene)
}

/// Bounded validation failure. Structural errors include a stable JSON path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    pub kind: ErrorKind,
    pub location: String,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

impl std::error::Error for Error {}

/// Validates a bounded manifest against the local draft schema, then checks declared
/// asset sizes and hashes against the archive/filesystem inventory.
/// This function does not extract archives, resolve URLs, or populate setting defaults.
pub fn validate_manifest(
    bytes: &[u8],
    inventory: &AssetInventory,
) -> Result<ValidatedManifest, Error> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(Error {
            kind: ErrorKind::TooLarge,
            location: "$".into(),
            message: format!("manifest exceeds {MAX_MANIFEST_BYTES} bytes"),
        });
    }
    check_json_depth(bytes)?;
    let document: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| Error {
        kind: ErrorKind::Structural,
        location: json_location(&error),
        message: "manifest document is not valid JSON".into(),
    })?;
    if document
        .get("format_version")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|version| version != "1.0")
    {
        return Err(Error {
            kind: ErrorKind::UnsupportedFormat,
            location: "$.format_version".into(),
            message: "only format version 1.0 is supported".into(),
        });
    }
    validate_schema(manifest_validator()?, &document, "manifest")?;
    let manifest: Manifest = serde_json::from_value(document)
        .map_err(|_| structural("$", "manifest type conversion failed"))?;
    validate_asset_inventory(&manifest, inventory)?;
    Ok(ValidatedManifest { manifest })
}

fn check_json_depth(bytes: &[u8]) -> Result<(), Error> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > MAX_JSON_DEPTH {
                    return Err(structural("$", "JSON nesting exceeds 32 levels"));
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

fn json_location(error: &serde_json::Error) -> String {
    // serde_json provides line/column but not a JSON pointer; retain a stable root path
    // rather than leaking raw untrusted source text into diagnostics.
    format!("$ (line {}, column {})", error.line(), error.column())
}

fn structural(location: &str, message: &str) -> Error {
    Error {
        kind: ErrorKind::Structural,
        location: location.into(),
        message: message.into(),
    }
}

fn validate_asset_inventory(manifest: &Manifest, inventory: &AssetInventory) -> Result<(), Error> {
    let mut declared = std::collections::BTreeSet::new();
    for (index, asset) in manifest.assets.iter().enumerate() {
        if !declared.insert(asset.path.as_str()) {
            return Err(Error {
                kind: ErrorKind::InventoryMismatch,
                location: format!("$.assets[{index}].path"),
                message: "asset paths must be unique".into(),
            });
        }
        let Some(actual) = inventory.assets.get(&asset.path) else {
            return Err(Error {
                kind: ErrorKind::InventoryMismatch,
                location: format!("$.assets[{index}].path"),
                message: "declared asset is missing from inventory".into(),
            });
        };
        if actual.size != asset.size || actual.sha256 != asset.sha256 {
            return Err(Error {
                kind: ErrorKind::InventoryMismatch,
                location: format!("$.assets[{index}]"),
                message: "declared asset size or digest differs from inventory".into(),
            });
        }
    }
    if declared.len() != inventory.assets.len() {
        return Err(Error {
            kind: ErrorKind::InventoryMismatch,
            location: "$.assets".into(),
            message: "inventory contains undeclared or duplicate asset records".into(),
        });
    }
    Ok(())
}

static MANIFEST_VALIDATOR: LazyLock<Result<jsonschema::Validator, String>> =
    LazyLock::new(|| compile_schema(include_str!("../schemas/manifest-1.0.json")));
static SCENE_VALIDATOR: LazyLock<Result<jsonschema::Validator, String>> =
    LazyLock::new(|| compile_schema(include_str!("../schemas/scene-1.0.json")));

fn compile_schema(source: &str) -> Result<jsonschema::Validator, String> {
    let schema: serde_json::Value =
        serde_json::from_str(source).map_err(|_| "invalid embedded JSON Schema".to_owned())?;
    jsonschema::validator_for(&schema).map_err(|_| "unsupported embedded JSON Schema".to_owned())
}

fn manifest_validator() -> Result<&'static jsonschema::Validator, Error> {
    get_validator(&MANIFEST_VALIDATOR)
}

fn scene_validator() -> Result<&'static jsonschema::Validator, Error> {
    get_validator(&SCENE_VALIDATOR)
}

fn get_validator(
    compiled: &'static LazyLock<Result<jsonschema::Validator, String>>,
) -> Result<&'static jsonschema::Validator, Error> {
    compiled.as_ref().map_err(|_| Error {
        kind: ErrorKind::SchemaDefinition,
        location: "$schema".into(),
        message: "embedded draft v1 schema could not be compiled".into(),
    })
}

fn validate_schema(
    validator: &jsonschema::Validator,
    document: &serde_json::Value,
    label: &str,
) -> Result<(), Error> {
    if let Some(error) = validator.iter_errors(document).next() {
        let path = error.instance_path.to_string();
        let location = if path.is_empty() {
            "$".to_owned()
        } else {
            path
        };
        return Err(structural(
            &location,
            &format!("{label} does not match the closed draft v1 schema"),
        ));
    }
    Ok(())
}
