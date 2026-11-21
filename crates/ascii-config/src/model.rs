use std::{fmt, sync::Arc};

/// A registry key is an exact dotted name with lowercase ASCII segments.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SettingKey(Arc<str>);

impl SettingKey {
    /// Validates and stores a dotted setting key.
    pub fn new(value: impl AsRef<str>) -> Result<Self, KeyError> {
        let value = value.as_ref();
        if value.is_empty() || value.len() > 128 {
            return Err(KeyError);
        }
        for segment in value.split('.') {
            if segment.is_empty()
                || !segment.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'-'
                        || byte == b'_'
                })
            {
                return Err(KeyError);
            }
        }
        Ok(Self(Arc::from(value)))
    }

    /// Returns the exact dotted key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SettingKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Invalid dotted setting key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyError;

impl fmt::Display for KeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("setting keys must be 1..=128 bytes of lowercase dotted ASCII segments")
    }
}

impl std::error::Error for KeyError {}

/// A scalar value admitted by the versioned TOML settings format.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingValue {
    Boolean(bool),
    Integer(i64),
    Number(f64),
    Text(Arc<str>),
}

impl SettingValue {
    /// Creates a finite floating-point value.
    pub fn number(value: f64) -> Result<Self, ValueError> {
        if value.is_finite() {
            Ok(Self::Number(value))
        } else {
            Err(ValueError::NonFinite)
        }
    }

    /// Creates a string setting value.
    #[must_use]
    pub fn text(value: impl AsRef<str>) -> Self {
        Self::Text(Arc::from(value.as_ref()))
    }

    /// Returns the represented scalar kind.
    #[must_use]
    pub const fn kind(&self) -> ValueKind {
        match self {
            Self::Boolean(_) => ValueKind::Boolean,
            Self::Integer(_) => ValueKind::Integer,
            Self::Number(_) => ValueKind::Number,
            Self::Text(_) => ValueKind::Text,
        }
    }
}

/// Scalar type used in typed diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueKind {
    Boolean,
    Integer,
    Number,
    Text,
}

impl fmt::Display for ValueKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Text => "string",
        })
    }
}

/// A scalar value could not be represented safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueError {
    NonFinite,
}

impl fmt::Display for ValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("non-finite numbers are not valid settings values")
    }
}

impl std::error::Error for ValueError {}

/// Registry value type and inclusive range or enumeration.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingType {
    Boolean,
    Integer { min: i64, max: i64 },
    Number { min: f64, max: f64 },
    String { max_chars: usize },
    Enum { values: Vec<Arc<str>> },
}

impl SettingType {
    pub(crate) fn validates(&self, value: &SettingValue) -> Result<(), TypeViolation> {
        match (self, value) {
            (Self::Boolean, SettingValue::Boolean(_)) => Ok(()),
            (Self::Integer { min, max }, SettingValue::Integer(value)) => {
                if min <= value && value <= max {
                    Ok(())
                } else {
                    Err(TypeViolation::OutOfRange)
                }
            }
            (Self::Number { min, max }, SettingValue::Number(value)) => {
                if !value.is_finite() {
                    return Err(TypeViolation::NonFinite);
                }
                if min <= value && value <= max {
                    Ok(())
                } else {
                    Err(TypeViolation::OutOfRange)
                }
            }
            (Self::String { max_chars }, SettingValue::Text(value)) => {
                if value.chars().count() <= *max_chars {
                    Ok(())
                } else {
                    Err(TypeViolation::OutOfRange)
                }
            }
            (Self::Enum { values }, SettingValue::Text(value)) => {
                if values
                    .iter()
                    .any(|candidate| candidate.as_ref() == value.as_ref())
                {
                    Ok(())
                } else {
                    Err(TypeViolation::NotAllowed)
                }
            }
            _ => Err(TypeViolation::WrongType),
        }
    }

    pub(crate) fn same_numeric_kind(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Integer { .. }, Self::Integer { .. })
                | (Self::Number { .. }, Self::Number { .. })
        )
    }
}

/// The scope which owns a setting and controls which input layer may request it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingScope {
    /// Portable visual request; packages may only suggest values in this scope.
    P,
    /// Application policy.
    A,
    /// Local output assignment.
    O,
    /// Temporary session state.
    S,
}

/// Planned capability phase declared by the source registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingPhase {
    V1,
    Later,
}

/// Typed resource invalidation class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidationClass {
    Uniform,
    Runtime,
    Scene,
    Conversion,
    Atlas,
    Decoder,
    Scheduler,
    Surface,
    Observer,
    Controller,
    Ui,
    Job,
    Restart,
}

/// Cross-field semantic check attached to a descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SemanticValidator {
    None,
    MustBeTrue,
    LessThan(SettingKey),
    GreaterThan(SettingKey),
}

/// One authoritative descriptor for a setting.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingDescriptor {
    pub(crate) key: SettingKey,
    pub(crate) default: SettingValue,
    pub(crate) value_type: SettingType,
    pub(crate) scope: SettingScope,
    pub(crate) phase: SettingPhase,
    pub(crate) capability: Option<Arc<str>>,
    pub(crate) invalidation: InvalidationClass,
    pub(crate) semantic_validator: SemanticValidator,
}

impl SettingDescriptor {
    /// Constructs a descriptor and validates its default and type declaration.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        key: SettingKey,
        default: SettingValue,
        value_type: SettingType,
        scope: SettingScope,
        phase: SettingPhase,
        capability: Option<&str>,
        invalidation: InvalidationClass,
        semantic_validator: SemanticValidator,
    ) -> Result<Self, DescriptorError> {
        match &value_type {
            SettingType::Integer { min, max } if min > max => {
                return Err(DescriptorError::InvalidRange);
            }
            SettingType::Number { min, max }
                if !min.is_finite() || !max.is_finite() || min > max =>
            {
                return Err(DescriptorError::InvalidRange);
            }
            SettingType::Enum { values }
                if values.is_empty()
                    || values.iter().any(|value| value.is_empty())
                    || values.iter().enumerate().any(|(index, value)| {
                        values[..index].iter().any(|previous| previous == value)
                    }) =>
            {
                return Err(DescriptorError::InvalidEnumeration);
            }
            _ => {}
        }
        value_type
            .validates(&default)
            .map_err(DescriptorError::InvalidDefault)?;
        let capability = capability.map(Arc::from);
        if capability
            .as_ref()
            .is_some_and(|value: &Arc<str>| value.is_empty())
        {
            return Err(DescriptorError::EmptyCapability);
        }
        Ok(Self {
            key,
            default,
            value_type,
            scope,
            phase,
            capability,
            invalidation,
            semantic_validator,
        })
    }

    #[must_use]
    pub fn key(&self) -> &SettingKey {
        &self.key
    }

    #[must_use]
    pub fn default(&self) -> &SettingValue {
        &self.default
    }

    #[must_use]
    pub fn value_type(&self) -> &SettingType {
        &self.value_type
    }

    #[must_use]
    pub const fn scope(&self) -> SettingScope {
        self.scope
    }

    #[must_use]
    pub const fn phase(&self) -> SettingPhase {
        self.phase
    }

    #[must_use]
    pub fn capability(&self) -> Option<&str> {
        self.capability.as_deref()
    }

    #[must_use]
    pub const fn invalidation(&self) -> InvalidationClass {
        self.invalidation
    }

    #[must_use]
    pub fn semantic_validator(&self) -> &SemanticValidator {
        &self.semantic_validator
    }
}

/// Invalid registry descriptor declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorError {
    InvalidRange,
    InvalidEnumeration,
    InvalidDefault(TypeViolation),
    EmptyCapability,
}

impl fmt::Display for DescriptorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid setting descriptor: {self:?}")
    }
}

impl std::error::Error for DescriptorError {}

/// A descriptor's value violated its declared type, range, or enumeration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TypeViolation {
    WrongType,
    OutOfRange,
    NotAllowed,
    NonFinite,
}

impl fmt::Display for TypeViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::WrongType => "value has the wrong scalar type",
            Self::OutOfRange => "value is outside the declared range",
            Self::NotAllowed => "value is not one of the declared enum choices",
            Self::NonFinite => "number must be finite",
        })
    }
}

impl std::error::Error for TypeViolation {}
