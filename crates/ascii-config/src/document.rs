use std::{collections::BTreeMap, str};

use crate::model::{SettingKey, SettingValue};

/// Current settings-document schema, independent from all other version axes.
pub const CURRENT_SCHEMA_VERSION: i64 = 2;
/// Maximum encoded settings file size.
pub const MAX_CONFIG_BYTES: usize = 256 * 1024;
/// Maximum array/table or dotted-key depth accepted by the preflight scanner.
pub const MAX_NESTING_DEPTH: usize = 16;
/// Maximum setting assignments accepted in one document.
pub const MAX_SETTING_ENTRIES: usize = 4096;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DecodedDocument {
    pub schema_version: i64,
    pub revision: u64,
    pub overrides: BTreeMap<SettingKey, SettingValue>,
}

pub(crate) fn decode(bytes: &[u8]) -> Result<DecodedDocument, DocumentError> {
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(DocumentError::TooLarge {
            limit: MAX_CONFIG_BYTES,
        });
    }
    let text = str::from_utf8(bytes).map_err(|_| DocumentError::InvalidUtf8)?;
    preflight(text)?;
    let root = toml::from_str::<toml::Value>(text).map_err(classify_toml_error)?;
    let toml::Value::Table(mut root) = root else {
        return Err(DocumentError::InvalidRoot);
    };
    let schema_version = root
        .remove("schema_version")
        .and_then(|value| value.as_integer())
        .ok_or(DocumentError::InvalidField("schema_version"))?;
    if schema_version < 1 {
        return Err(DocumentError::UnsupportedVersion(schema_version));
    }
    if schema_version > CURRENT_SCHEMA_VERSION {
        let revision = root
            .remove("revision")
            .and_then(|value| value.as_integer())
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or(0);
        return Ok(DecodedDocument {
            schema_version,
            revision,
            overrides: BTreeMap::new(),
        });
    }

    let revision = root
        .remove("revision")
        .and_then(|value| value.as_integer())
        .and_then(|value| u64::try_from(value).ok())
        .ok_or(DocumentError::InvalidField("revision"))?;
    let settings = root
        .remove("settings")
        .unwrap_or_else(|| toml::Value::Table(toml::map::Map::new()));
    if !root.is_empty() {
        return Err(DocumentError::UnknownRootField(
            root.keys().next().cloned().unwrap_or_default(),
        ));
    }
    let toml::Value::Table(settings) = settings else {
        return Err(DocumentError::InvalidField("settings"));
    };
    if settings.len() > MAX_SETTING_ENTRIES {
        return Err(DocumentError::TooManyEntries {
            limit: MAX_SETTING_ENTRIES,
        });
    }

    let mut overrides = BTreeMap::new();
    for (raw_key, raw_value) in settings {
        let key =
            SettingKey::new(&raw_key).map_err(|_| DocumentError::InvalidKey(raw_key.clone()))?;
        let value = decode_scalar(raw_value)?;
        overrides.insert(key, value);
    }
    Ok(DecodedDocument {
        schema_version,
        revision,
        overrides,
    })
}

pub(crate) fn encode(
    revision: u64,
    overrides: &BTreeMap<SettingKey, SettingValue>,
) -> Result<Vec<u8>, DocumentError> {
    if revision > i64::MAX as u64 {
        return Err(DocumentError::RevisionOverflow);
    }
    if overrides.len() > MAX_SETTING_ENTRIES {
        return Err(DocumentError::TooManyEntries {
            limit: MAX_SETTING_ENTRIES,
        });
    }
    let mut settings = toml::map::Map::new();
    for (key, value) in overrides {
        settings.insert(key.as_str().to_owned(), encode_scalar(value)?);
    }
    let mut root = toml::map::Map::new();
    root.insert(
        "schema_version".to_owned(),
        toml::Value::Integer(CURRENT_SCHEMA_VERSION),
    );
    root.insert("revision".to_owned(), toml::Value::Integer(revision as i64));
    root.insert("settings".to_owned(), toml::Value::Table(settings));
    let text =
        toml::to_string(&toml::Value::Table(root)).map_err(|_| DocumentError::Serialization)?;
    if text.len() > MAX_CONFIG_BYTES {
        return Err(DocumentError::TooLarge {
            limit: MAX_CONFIG_BYTES,
        });
    }
    Ok(text.into_bytes())
}

fn decode_scalar(value: toml::Value) -> Result<SettingValue, DocumentError> {
    match value {
        toml::Value::Boolean(value) => Ok(SettingValue::Boolean(value)),
        toml::Value::Integer(value) => Ok(SettingValue::Integer(value)),
        toml::Value::Float(value) if value.is_finite() => Ok(SettingValue::Number(value)),
        toml::Value::Float(_) => Err(DocumentError::NonFiniteNumber),
        toml::Value::String(value) => Ok(SettingValue::text(value)),
        toml::Value::Array(_) | toml::Value::Table(_) | toml::Value::Datetime(_) => {
            Err(DocumentError::UnsupportedStructure)
        }
    }
}

fn encode_scalar(value: &SettingValue) -> Result<toml::Value, DocumentError> {
    match value {
        SettingValue::Boolean(value) => Ok(toml::Value::Boolean(*value)),
        SettingValue::Integer(value) => Ok(toml::Value::Integer(*value)),
        SettingValue::Number(value) if value.is_finite() => Ok(toml::Value::Float(*value)),
        SettingValue::Number(_) => Err(DocumentError::NonFiniteNumber),
        SettingValue::Text(value) => Ok(toml::Value::String(value.to_string())),
    }
}

fn classify_toml_error(error: toml::de::Error) -> DocumentError {
    let message = error.to_string();
    let lowercase = message.to_ascii_lowercase();
    if lowercase.contains("duplicate key") || lowercase.contains("redefined") {
        DocumentError::DuplicateKey
    } else {
        DocumentError::InvalidToml
    }
}

fn preflight(text: &str) -> Result<(), DocumentError> {
    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Quote {
        Basic,
        Literal,
        MultiBasic,
        MultiLiteral,
    }

    let bytes = text.as_bytes();
    let mut quote: Option<Quote> = None;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut assignments = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(state) = quote {
            match state {
                Quote::Basic => {
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == b'"' {
                        quote = None;
                    }
                }
                Quote::Literal => {
                    if byte == b'\'' {
                        quote = None;
                    }
                }
                Quote::MultiBasic => {
                    if byte == b'\\' {
                        index = index.saturating_add(1);
                    } else if bytes.get(index..index.saturating_add(3)) == Some(&b"\"\"\""[..]) {
                        quote = None;
                        index = index.saturating_add(2);
                    }
                }
                Quote::MultiLiteral => {
                    if bytes.get(index..index.saturating_add(3)) == Some(&b"'''"[..]) {
                        quote = None;
                        index = index.saturating_add(2);
                    }
                }
            }
            index += 1;
            continue;
        }

        match byte {
            b'#' => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'"' if bytes.get(index..index.saturating_add(3)) == Some(&b"\"\"\""[..]) => {
                quote = Some(Quote::MultiBasic);
                index = index.saturating_add(2);
            }
            b'\'' if bytes.get(index..index.saturating_add(3)) == Some(&b"'''"[..]) => {
                quote = Some(Quote::MultiLiteral);
                index = index.saturating_add(2);
            }
            b'"' => quote = Some(Quote::Basic),
            b'\'' => quote = Some(Quote::Literal),
            b'[' | b'{' => {
                if byte == b'[' && depth == 0 && is_line_prefix_whitespace(bytes, index) {
                    let open_count = if bytes.get(index + 1) == Some(&b'[') {
                        2
                    } else {
                        1
                    };
                    let content_start = index + open_count;
                    if let Some(relative_end) =
                        bytes[content_start..].iter().position(|b| *b == b']')
                    {
                        enforce_path_depth(&bytes[content_start..content_start + relative_end])?;
                    }
                }
                depth = depth.saturating_add(1);
                if depth > MAX_NESTING_DEPTH {
                    return Err(DocumentError::ExcessiveNesting {
                        limit: MAX_NESTING_DEPTH,
                    });
                }
            }
            b']' | b'}' => {
                depth = depth.saturating_sub(1);
            }
            b'=' => {
                assignments += 1;
                if assignments > MAX_SETTING_ENTRIES + 2 {
                    return Err(DocumentError::TooManyEntries {
                        limit: MAX_SETTING_ENTRIES,
                    });
                }
                if depth == 0 {
                    let line_start = bytes[..index]
                        .iter()
                        .rposition(|byte| *byte == b'\n')
                        .map_or(0, |position| position + 1);
                    enforce_path_depth(&bytes[line_start..index])?;
                }
            }
            _ => {}
        }
        index += 1;
    }
    Ok(())
}

fn is_line_prefix_whitespace(bytes: &[u8], index: usize) -> bool {
    let line_start = bytes[..index]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    bytes[line_start..index]
        .iter()
        .all(|byte| matches!(*byte, b' ' | b'\t' | b'\r'))
}

fn enforce_path_depth(path: &[u8]) -> Result<(), DocumentError> {
    let depth = path.iter().filter(|byte| **byte == b'.').count() + 1;
    if depth > MAX_NESTING_DEPTH {
        Err(DocumentError::ExcessiveNesting {
            limit: MAX_NESTING_DEPTH,
        })
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocumentError {
    TooLarge { limit: usize },
    InvalidUtf8,
    ExcessiveNesting { limit: usize },
    TooManyEntries { limit: usize },
    InvalidToml,
    DuplicateKey,
    InvalidRoot,
    UnsupportedVersion(i64),
    InvalidField(&'static str),
    UnknownRootField(String),
    InvalidKey(String),
    NonFiniteNumber,
    UnsupportedStructure,
    RevisionOverflow,
    Serialization,
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid settings document: {self:?}")
    }
}

impl std::error::Error for DocumentError {}
