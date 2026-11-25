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
    Semantic,
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
    let document = crate::unique_json::parse(bytes).map_err(|error| Error {
        kind: ErrorKind::Structural,
        location: json_location(&error),
        message: "scene document is not valid unique-key JSON".into(),
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
    let document = crate::unique_json::parse(bytes).map_err(|error| Error {
        kind: ErrorKind::Structural,
        location: json_location(&error),
        message: "manifest document is not valid unique-key JSON".into(),
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
    crate::manifest_semantics::validate_manifest_semantics(&manifest, inventory)?;
    crate::manifest_semantics::validate_asset_inventory(bytes, &manifest, inventory)?;
    Ok(ValidatedManifest { manifest })
}

fn check_json_depth(bytes: &[u8]) -> Result<(), Error> {
    let mut expected_closers = [0u8; MAX_JSON_DEPTH];
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
                if depth == MAX_JSON_DEPTH {
                    return Err(structural(
                        "$",
                        &format!("JSON nesting exceeds {MAX_JSON_DEPTH} levels"),
                    ));
                }
                expected_closers[depth] = if *byte == b'{' { b'}' } else { b']' };
                depth += 1;
            }
            b'}' | b']' => {
                if depth == 0 || expected_closers[depth - 1] != *byte {
                    return Err(structural("$", "JSON delimiters are not properly nested"));
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(structural("$", "JSON delimiters are not properly nested"));
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

#[cfg(test)]
mod tests {
    use super::{AssetInventory, ErrorKind, MAX_JSON_DEPTH, check_json_depth, validate_manifest};

    #[test]
    fn rejects_mismatched_closing_delimiters() {
        for input in [b"{]".as_slice(), b"[}", b"}"] {
            let error = check_json_depth(input).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Structural);
            let error = validate_manifest(input, &AssetInventory::default()).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Structural);
        }
    }

    #[test]
    fn ignores_delimiters_and_escapes_inside_strings() {
        let input = br#"{"text":"delimiters {}[] and escaped quote: \" and backslash \\"}"#;
        assert!(serde_json::from_slice::<serde_json::Value>(input).is_ok());
        assert!(check_json_depth(input).is_ok());
    }

    #[test]
    fn accepts_limit_and_rejects_one_level_over() {
        let at_limit = format!(
            "{}0{}",
            "[".repeat(MAX_JSON_DEPTH),
            "]".repeat(MAX_JSON_DEPTH)
        );
        assert!(check_json_depth(at_limit.as_bytes()).is_ok());

        let over_limit = format!(
            "{}0{}",
            "[".repeat(MAX_JSON_DEPTH + 1),
            "]".repeat(MAX_JSON_DEPTH + 1)
        );
        let error = check_json_depth(over_limit.as_bytes()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Structural);
        assert_eq!(error.location, "$");
    }
}
