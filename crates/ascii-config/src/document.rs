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

/// Parses under limits that may reject input before its schema is known.
/// Recovery eligibility is decided by the caller only after independently
/// recognizing the supported schema and checking its setting keys.
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
    encode_with_serializer(revision, overrides, |root| {
        toml::to_string(root).map_err(|_| DocumentError::Serialization)
    })
}

fn encode_with_serializer(
    revision: u64,
    overrides: &BTreeMap<SettingKey, SettingValue>,
    serialize: impl FnOnce(&toml::Value) -> Result<String, DocumentError>,
) -> Result<Vec<u8>, DocumentError> {
    if revision > i64::MAX as u64 {
        return Err(DocumentError::RevisionOverflow);
    }
    if overrides.len() > MAX_SETTING_ENTRIES {
        return Err(DocumentError::TooManyEntries {
            limit: MAX_SETTING_ENTRIES,
        });
    }
    // Preserve scalar validation errors before applying the encoded-size bound.
    for value in overrides.values() {
        if matches!(value, SettingValue::Number(number) if !number.is_finite()) {
            return Err(DocumentError::NonFiniteNumber);
        }
    }
    preflight_encoded_size(revision, overrides)?;

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
    let text = serialize(&toml::Value::Table(root))?;
    if text.len() > MAX_CONFIG_BYTES {
        return Err(DocumentError::TooLarge {
            limit: MAX_CONFIG_BYTES,
        });
    }
    Ok(text.into_bytes())
}

// Count the canonical inline-table output before the TOML serializer allocates its String.
fn preflight_encoded_size(
    revision: u64,
    overrides: &BTreeMap<SettingKey, SettingValue>,
) -> Result<(), DocumentError> {
    let mut size = 0;
    add_encoded_size(&mut size, "revision = ".len())?;
    add_encoded_size(&mut size, signed_integer_encoded_len(revision as i64))?;
    add_encoded_size(&mut size, "\nschema_version = ".len())?;
    add_encoded_size(
        &mut size,
        signed_integer_encoded_len(CURRENT_SCHEMA_VERSION),
    )?;
    add_encoded_size(&mut size, "\nsettings = {".len())?;

    for (index, (key, value)) in overrides.iter().enumerate() {
        if index > 0 {
            add_encoded_size(&mut size, 1)?;
        }
        add_encoded_size(&mut size, 1)?;
        let key = key.as_str();
        let encoded_key_len = key.len() + if key.contains('.') { 2 } else { 0 };
        add_encoded_size(&mut size, encoded_key_len)?;
        add_encoded_size(&mut size, " = ".len())?;
        let value_len = match value {
            SettingValue::Boolean(true) => 4,
            SettingValue::Boolean(false) => 5,
            SettingValue::Integer(value) => signed_integer_encoded_len(*value),
            SettingValue::Number(value) => toml_float_encoded_len(*value),
            SettingValue::Text(value) => {
                toml_string_encoded_len(value).ok_or(DocumentError::TooLarge {
                    limit: MAX_CONFIG_BYTES,
                })?
            }
        };
        add_encoded_size(&mut size, value_len)?;
    }

    if !overrides.is_empty() {
        add_encoded_size(&mut size, 1)?;
    }
    add_encoded_size(&mut size, "}\n".len())?;
    Ok(())
}

fn add_encoded_size(size: &mut usize, additional: usize) -> Result<(), DocumentError> {
    let Some(total) = size
        .checked_add(additional)
        .filter(|total| *total <= MAX_CONFIG_BYTES)
    else {
        return Err(DocumentError::TooLarge {
            limit: MAX_CONFIG_BYTES,
        });
    };
    *size = total;
    Ok(())
}

fn signed_integer_encoded_len(value: i64) -> usize {
    let mut magnitude = value.unsigned_abs();
    let mut length = if value < 0 { 1 } else { 0 };
    loop {
        length += 1;
        if magnitude < 10 {
            return length;
        }
        magnitude /= 10;
    }
}

fn toml_float_encoded_len(value: f64) -> usize {
    if value == 0.0 {
        return if value.is_sign_negative() { 4 } else { 3 };
    }

    let mut length = FormatLength(0);
    let result = if value % 1.0 == 0.0 {
        std::fmt::write(&mut length, format_args!("{value}.0"))
    } else {
        std::fmt::write(&mut length, format_args!("{value}"))
    };
    debug_assert!(result.is_ok());
    length.0
}

struct FormatLength(usize);

impl std::fmt::Write for FormatLength {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.0 += value.len();
        Ok(())
    }
}

/// Counts the TOML writer's chosen string representation without allocating it.
fn toml_string_encoded_len(value: &str) -> Option<usize> {
    if value.len() > MAX_CONFIG_BYTES {
        return None;
    }
    let mut max_single_quote_run = 0;
    let mut max_double_quote_run = 0;
    let mut single_quote_run = 0;
    let mut double_quote_run = 0;
    let mut escape_codes = false;
    let mut escape = false;
    let mut newline = false;

    for byte in value.as_bytes() {
        if *byte == b'\'' {
            single_quote_run += 1;
            max_single_quote_run = max_single_quote_run.max(single_quote_run);
        } else {
            single_quote_run = 0;
        }
        if *byte == b'"' {
            double_quote_run += 1;
            max_double_quote_run = max_double_quote_run.max(double_quote_run);
        } else {
            double_quote_run = 0;
        }

        match *byte {
            b'\\' => escape = true,
            b'\t' => {}
            b'\n' => newline = true,
            byte if byte <= 0x1f || byte == 0x7f => escape_codes = true,
            _ => {}
        }
    }

    let (delimiter_len, multiline, escaped) =
        if !escape_codes && !escape && max_double_quote_run == 0 && !newline {
            (2, false, true)
        } else if !escape_codes && max_single_quote_run == 0 && !newline {
            (2, false, false)
        } else if !escape_codes && !escape && max_double_quote_run <= 2 {
            (6, true, true)
        } else if !escape_codes && max_single_quote_run <= 2 {
            (6, true, false)
        } else if newline {
            (6, true, true)
        } else {
            (2, false, true)
        };

    let mut length = 0;
    add_encoded_size(&mut length, delimiter_len).ok()?;
    if multiline && newline {
        add_encoded_size(&mut length, 1).ok()?;
    }
    if !escaped {
        add_encoded_size(&mut length, value.len()).ok()?;
        return Some(length);
    }

    double_quote_run = 0;
    for byte in value.as_bytes() {
        let encoded_len = match *byte {
            b'"' if multiline => {
                double_quote_run += 1;
                if double_quote_run == 3 {
                    double_quote_run = 0;
                    2
                } else {
                    1
                }
            }
            b'"' => {
                double_quote_run = 0;
                2
            }
            b'\n' if multiline => {
                double_quote_run = 0;
                1
            }
            b'\x08' | b'\t' | b'\n' | b'\x0c' | b'\r' | b'\\' => {
                double_quote_run = 0;
                2
            }
            byte if byte <= 0x1f || byte == 0x7f => {
                double_quote_run = 0;
                6
            }
            _ => {
                double_quote_run = 0;
                1
            }
        };
        add_encoded_size(&mut length, encoded_len).ok()?;
    }
    Some(length)
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

    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Container {
        Array,
        InlineTable,
    }

    let bytes = text.as_bytes();
    let mut quote: Option<Quote> = None;
    let mut escaped = false;
    let mut containers = [Container::Array; MAX_NESTING_DEPTH];
    let mut inline_key_starts = [0usize; MAX_NESTING_DEPTH];
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
                    } else if byte == b'"' {
                        let quote_run = bytes[index..]
                            .iter()
                            .take_while(|next| **next == b'"')
                            .count();
                        if quote_run >= 3 {
                            quote = None;
                            index += quote_run - 1;
                        }
                    }
                }
                Quote::MultiLiteral => {
                    if byte == b'\'' {
                        let quote_run = bytes[index..]
                            .iter()
                            .take_while(|next| **next == b'\'')
                            .count();
                        if quote_run >= 3 {
                            quote = None;
                            index += quote_run - 1;
                        }
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
                    if let Some(header_end) = table_header_terminator(bytes, content_start) {
                        enforce_path_depth(&bytes[content_start..header_end])?;
                    }
                }
                if depth == MAX_NESTING_DEPTH {
                    return Err(DocumentError::ExcessiveNesting {
                        limit: MAX_NESTING_DEPTH,
                    });
                }
                containers[depth] = if byte == b'{' {
                    inline_key_starts[depth] = index + 1;
                    Container::InlineTable
                } else {
                    Container::Array
                };
                depth += 1;
            }
            b']' | b'}' => {
                depth = depth.saturating_sub(1);
            }
            b',' => {
                if depth > 0 && containers[depth - 1] == Container::InlineTable {
                    inline_key_starts[depth - 1] = index + 1;
                }
            }
            b'=' => {
                assignments += 1;
                // The settings assignment itself is an extra envelope entry
                // for inline tables; decode() enforces the exact settings count.
                if assignments > MAX_SETTING_ENTRIES + 3 {
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
                } else if containers[depth - 1] == Container::InlineTable {
                    enforce_path_depth(&bytes[inline_key_starts[depth - 1]..index])?;
                }
            }
            _ => {}
        }
        index += 1;
    }
    Ok(())
}

fn table_header_terminator(bytes: &[u8], start: usize) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate().skip(start) {
        if matches!(*byte, b'\n' | b'\r') {
            return None;
        }
        if let Some(quote_byte) = quote {
            if quote_byte == b'"' {
                if escaped {
                    escaped = false;
                } else if *byte == b'\\' {
                    escaped = true;
                } else if *byte == quote_byte {
                    quote = None;
                }
            } else if *byte == quote_byte {
                quote = None;
            }
        } else {
            match *byte {
                b'"' | b'\'' => quote = Some(*byte),
                b']' => return Some(index),
                _ => {}
            }
        }
    }
    None
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
    let mut depth = 1usize;
    let mut quote = None;
    let mut escaped = false;
    for byte in path {
        if let Some(quote_byte) = quote {
            if quote_byte == b'"' {
                if escaped {
                    escaped = false;
                } else if *byte == b'\\' {
                    escaped = true;
                } else if *byte == quote_byte {
                    quote = None;
                }
            } else if *byte == quote_byte {
                quote = None;
            }
        } else {
            match *byte {
                b'"' | b'\'' => quote = Some(*byte),
                b'.' => depth += 1,
                _ => {}
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::{
        DocumentError, MAX_CONFIG_BYTES, MAX_NESTING_DEPTH, MAX_SETTING_ENTRIES, decode, encode,
        encode_with_serializer,
    };
    use crate::model::{SettingKey, SettingType, SettingValue};
    use std::{cell::Cell, collections::BTreeMap, fmt::Write as _};

    #[test]
    fn inline_table_dotted_key_depth_accepts_limit_and_rejects_over_limit() {
        let dotted_key = (0..MAX_NESTING_DEPTH)
            .map(|_| "segment")
            .collect::<Vec<_>>()
            .join(".");
        let at_limit =
            format!("schema_version = 2\nrevision = 0\nsettings = {{ {dotted_key} = 1 }}\n");
        assert!(at_limit.len() <= MAX_CONFIG_BYTES);
        assert!(matches!(
            decode(at_limit.as_bytes()),
            Err(DocumentError::UnsupportedStructure)
        ));

        let dotted_key = (0..=MAX_NESTING_DEPTH)
            .map(|_| "segment")
            .collect::<Vec<_>>()
            .join(".");
        let over_limit =
            format!("schema_version = 2\nrevision = 0\nsettings = {{ {dotted_key} = 1 }}\n");
        assert!(matches!(
            decode(over_limit.as_bytes()),
            Err(DocumentError::ExcessiveNesting {
                limit: MAX_NESTING_DEPTH
            })
        ));
    }

    #[test]
    fn shallow_inline_settings_table_keeps_quoted_dotted_setting_keys() {
        let document =
            decode(b"schema_version = 2\nrevision = 4\nsettings = { \"grid.columns\" = 240 }\n")
                .unwrap();

        assert_eq!(
            document
                .overrides
                .get(&SettingKey::new("grid.columns").unwrap()),
            Some(&SettingValue::Integer(240))
        );
    }

    #[test]
    fn oversized_schema_string_is_rejected_before_toml_serialization() {
        let max_chars = MAX_CONFIG_BYTES + 1;
        let schema_type = SettingType::String { max_chars };
        let value = SettingValue::text("x".repeat(max_chars));
        assert!(schema_type.validates(&value).is_ok());

        let mut overrides = BTreeMap::new();
        overrides.insert(SettingKey::new("application.label").unwrap(), value);
        let original = overrides.clone();
        let serializer_calls = Cell::new(0);
        let result = encode_with_serializer(0, &overrides, |_| {
            serializer_calls.set(serializer_calls.get() + 1);
            Ok(String::new())
        });

        assert!(matches!(
            result,
            Err(DocumentError::TooLarge {
                limit: MAX_CONFIG_BYTES
            })
        ));
        assert_eq!(serializer_calls.get(), 0);
        assert_eq!(overrides, original);
    }

    #[test]
    fn escaped_toml_expansion_is_rejected_before_serialization() {
        let schema_type = SettingType::String {
            max_chars: MAX_CONFIG_BYTES,
        };
        let value = SettingValue::text("\0".repeat(MAX_CONFIG_BYTES / 2));
        assert!(schema_type.validates(&value).is_ok());
        let SettingValue::Text(text) = &value else {
            unreachable!()
        };
        assert!(text.len() < MAX_CONFIG_BYTES);

        let mut overrides = BTreeMap::new();
        overrides.insert(SettingKey::new("application.label").unwrap(), value);
        assert!(matches!(
            encode(0, &overrides),
            Err(DocumentError::TooLarge {
                limit: MAX_CONFIG_BYTES
            })
        ));
        let serializer_calls = Cell::new(0);
        let result = encode_with_serializer(0, &overrides, |_| {
            serializer_calls.set(serializer_calls.get() + 1);
            Ok(String::new())
        });

        assert!(matches!(
            result,
            Err(DocumentError::TooLarge {
                limit: MAX_CONFIG_BYTES
            })
        ));
        assert_eq!(serializer_calls.get(), 0);
    }

    #[test]
    fn encoder_accepts_a_document_at_the_byte_limit() {
        let key = SettingKey::new("application.label").unwrap();
        let mut overrides = BTreeMap::new();
        overrides.insert(key.clone(), SettingValue::text("x"));
        let one_byte_document = encode(0, &overrides).unwrap();
        let encoded_overhead = one_byte_document
            .len()
            .checked_sub(1)
            .expect("the serialized document contains the x byte");
        let text_length = MAX_CONFIG_BYTES
            .checked_sub(encoded_overhead)
            .expect("the document has less than the configured byte limit of overhead");
        overrides.insert(key.clone(), SettingValue::text("x".repeat(text_length)));

        let encoded = encode(0, &overrides).unwrap();
        assert_eq!(encoded.len(), MAX_CONFIG_BYTES);

        let over_limit_length = text_length
            .checked_add(1)
            .expect("the exact-limit fixture is well below usize::MAX");
        overrides.insert(key, SettingValue::text("x".repeat(over_limit_length)));
        let serializer_calls = Cell::new(0);
        let result = encode_with_serializer(0, &overrides, |_| {
            serializer_calls.set(serializer_calls.get() + 1);
            Ok(String::new())
        });
        assert!(matches!(
            result,
            Err(DocumentError::TooLarge {
                limit: MAX_CONFIG_BYTES
            })
        ));
        assert_eq!(serializer_calls.get(), 0);
    }

    #[test]
    fn table_header_depth_counts_quoted_basic_literal_and_escaped_keys() {
        let suffix = ".a".repeat(MAX_NESTING_DEPTH);
        let headers = [
            format!("[settings.\"x]\"{suffix}]"),
            format!(r#"[settings."x\"]"{suffix}]"#),
            format!("[settings.'x]'{suffix}]"),
        ];

        for header in headers {
            let document = format!("schema_version = 2\nrevision = 0\n{header}\n");
            assert!(matches!(
                decode(document.as_bytes()),
                Err(DocumentError::ExcessiveNesting {
                    limit: MAX_NESTING_DEPTH
                })
            ));
        }
    }

    #[test]
    fn shallow_quoted_bracket_table_headers_reach_schema_validation() {
        for header in [r#"["extra]"]"#, "['extra]']"] {
            let document = format!("schema_version = 2\nrevision = 0\n{header}\n");
            assert!(matches!(
                decode(document.as_bytes()),
                Err(DocumentError::UnknownRootField(field)) if field == "extra]"
            ));
        }
    }

    #[test]
    fn preflight_detects_nesting_after_multiline_basic_closing_quote_run() {
        let nested_array = format!(
            "{}0{}",
            "[".repeat(MAX_NESTING_DEPTH + 1),
            "]".repeat(MAX_NESTING_DEPTH + 1)
        );
        let document = format!(
            "schema_version = 2\nrevision = 0\n[settings]\n\"string.key\" = \"\"\"text\"\"\"\"\n\"grid.columns\" = {nested_array}\n"
        );

        assert!(matches!(
            decode(document.as_bytes()),
            Err(DocumentError::ExcessiveNesting {
                limit: MAX_NESTING_DEPTH
            })
        ));
    }

    #[test]
    fn preflight_detects_nesting_after_multiline_literal_closing_quote_run() {
        let nested_array = format!(
            "{}0{}",
            "[".repeat(MAX_NESTING_DEPTH + 1),
            "]".repeat(MAX_NESTING_DEPTH + 1)
        );
        let document = format!(
            "schema_version = 2\nrevision = 0\n[settings]\n\"string.key\" = '''text''''\n\"grid.columns\" = {nested_array}\n"
        );

        assert!(matches!(
            decode(document.as_bytes()),
            Err(DocumentError::ExcessiveNesting {
                limit: MAX_NESTING_DEPTH
            })
        ));
    }

    fn document_with_entries(count: usize) -> Vec<u8> {
        let mut text = String::from("schema_version = 2\nrevision = 0\n[settings]\n");
        for index in 0..count {
            writeln!(&mut text, "\"setting.key{index:04}\" = {index}").unwrap();
        }
        text.into_bytes()
    }

    fn inline_document_with_entries(count: usize) -> Vec<u8> {
        let mut text = String::from("schema_version = 2\nrevision = 0\nsettings = { ");
        for index in 0..count {
            let separator = if index == 0 { "" } else { ", " };
            write!(&mut text, "{separator}\"setting.key{index:04}\" = {index}").unwrap();
        }
        text.push_str(" }\n");
        text.into_bytes()
    }

    #[test]
    fn decode_accepts_exact_inline_setting_entry_limit() {
        let document = inline_document_with_entries(MAX_SETTING_ENTRIES);
        assert!(document.len() <= MAX_CONFIG_BYTES);

        let decoded = decode(&document).unwrap();
        assert_eq!(decoded.overrides.len(), MAX_SETTING_ENTRIES);
    }

    #[test]
    fn decode_rejects_one_more_than_inline_setting_entry_limit() {
        let document = inline_document_with_entries(MAX_SETTING_ENTRIES + 1);
        assert!(document.len() <= MAX_CONFIG_BYTES);

        assert!(matches!(
            decode(&document),
            Err(DocumentError::TooManyEntries {
                limit: MAX_SETTING_ENTRIES
            })
        ));
    }

    #[test]
    fn decode_accepts_exact_setting_entry_limit() {
        let document = document_with_entries(MAX_SETTING_ENTRIES);
        assert!(document.len() <= MAX_CONFIG_BYTES);

        let decoded = decode(&document).unwrap();
        assert_eq!(decoded.overrides.len(), MAX_SETTING_ENTRIES);
    }

    #[test]
    fn decode_rejects_one_more_than_setting_entry_limit() {
        let document = document_with_entries(MAX_SETTING_ENTRIES + 1);
        assert!(document.len() <= MAX_CONFIG_BYTES);

        assert!(matches!(
            decode(&document),
            Err(DocumentError::TooManyEntries {
                limit: MAX_SETTING_ENTRIES
            })
        ));
    }
}
