use std::collections::BTreeMap;

use crate::{
    model::{
        SemanticValidator, SettingKey, SettingScope, SettingType, SettingValue, TypeViolation,
    },
    registry::Registry,
};

/// Input layer and authority for one requested value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingSource {
    Package,
    Preset,
    Assignment,
    Application,
    Session,
}

/// Requested values at each precedence layer.
#[derive(Clone, Debug, Default)]
pub struct SettingLayers {
    package: BTreeMap<SettingKey, SettingValue>,
    preset: BTreeMap<SettingKey, SettingValue>,
    assignment: BTreeMap<SettingKey, SettingValue>,
    application: BTreeMap<SettingKey, SettingValue>,
    session: BTreeMap<SettingKey, SettingValue>,
}

impl SettingLayers {
    /// Sets a value in the named authority layer. Scope and type are validated
    /// when the complete layer set is resolved.
    pub fn set(&mut self, source: SettingSource, key: SettingKey, value: SettingValue) {
        self.map_mut(source).insert(key, value);
    }

    /// Removes a value from a layer.
    pub fn remove(&mut self, source: SettingSource, key: &SettingKey) -> Option<SettingValue> {
        self.map_mut(source).remove(key)
    }

    pub(crate) fn values(&self, source: SettingSource) -> &BTreeMap<SettingKey, SettingValue> {
        match source {
            SettingSource::Package => &self.package,
            SettingSource::Preset => &self.preset,
            SettingSource::Assignment => &self.assignment,
            SettingSource::Application => &self.application,
            SettingSource::Session => &self.session,
        }
    }

    fn map_mut(&mut self, source: SettingSource) -> &mut BTreeMap<SettingKey, SettingValue> {
        match source {
            SettingSource::Package => &mut self.package,
            SettingSource::Preset => &mut self.preset,
            SettingSource::Assignment => &mut self.assignment,
            SettingSource::Application => &mut self.application,
            SettingSource::Session => &mut self.session,
        }
    }
}

/// Reason for an effective value differing from its request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConstraintReason {
    CapabilityUnavailable,
    ResourceLimit,
    AccessibilityPolicy,
    PowerPolicy,
    SafetyPolicy,
}

/// A constraint can cap a numeric request or substitute an explicitly
/// validated fallback without rewriting the requested value.
#[derive(Clone, Debug, PartialEq)]
pub enum EffectiveConstraint {
    AtMost {
        value: SettingValue,
        reason: ConstraintReason,
    },
    Unavailable {
        fallback: SettingValue,
        reason: ConstraintReason,
    },
}

/// Effective-value constraints supplied by runtime policy, capabilities, and
/// resource budgets. They never mutate input layers.
#[derive(Clone, Debug, Default)]
pub struct Constraints {
    values: BTreeMap<SettingKey, EffectiveConstraint>,
}

impl Constraints {
    pub fn set(&mut self, key: SettingKey, constraint: EffectiveConstraint) {
        self.values.insert(key, constraint);
    }

    pub fn remove(&mut self, key: &SettingKey) -> Option<EffectiveConstraint> {
        self.values.remove(key)
    }
}

/// The requested and effective values for one descriptor.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedValue {
    requested: SettingValue,
    effective: SettingValue,
    source: Option<SettingSource>,
    constraint: Option<ConstraintReason>,
}

impl ResolvedValue {
    #[must_use]
    pub fn requested(&self) -> &SettingValue {
        &self.requested
    }

    #[must_use]
    pub fn effective(&self) -> &SettingValue {
        &self.effective
    }

    /// `None` means the descriptor's source-owned default was selected.
    #[must_use]
    pub const fn source(&self) -> Option<SettingSource> {
        self.source
    }

    #[must_use]
    pub const fn constraint_reason(&self) -> Option<ConstraintReason> {
        self.constraint
    }
}

/// Fully validated immutable settings result. Consumers should only prepare
/// resources from this value, never from raw TOML or unvalidated layer maps.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EffectiveSettings {
    values: BTreeMap<SettingKey, ResolvedValue>,
}

impl EffectiveSettings {
    #[must_use]
    pub fn get(&self, key: &SettingKey) -> Option<&ResolvedValue> {
        self.values.get(key)
    }

    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&ResolvedValue> {
        SettingKey::new(key).ok().and_then(|key| self.get(&key))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&SettingKey, &ResolvedValue)> {
        self.values.iter()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// The requested layers or policy did not satisfy the registry contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolveError {
    UnknownKey(SettingKey),
    UnauthorizedSource {
        key: SettingKey,
        source: SettingSource,
        scope: SettingScope,
    },
    InvalidValue {
        key: SettingKey,
        violation: TypeViolation,
    },
    InvalidConstraint(SettingKey),
    SemanticViolation {
        key: SettingKey,
        related: Option<SettingKey>,
    },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "settings resolution failed: {self:?}")
    }
}

impl std::error::Error for ResolveError {}

/// Resolves precedence, validates authority and values, then derives an
/// effective snapshot without changing any requested value.
pub fn resolve_settings(
    registry: &Registry,
    layers: &SettingLayers,
    constraints: &Constraints,
) -> Result<EffectiveSettings, ResolveError> {
    for source in [
        SettingSource::Package,
        SettingSource::Preset,
        SettingSource::Assignment,
        SettingSource::Application,
        SettingSource::Session,
    ] {
        for (key, value) in layers.values(source) {
            let Some(descriptor) = registry.get(key) else {
                return Err(ResolveError::UnknownKey(key.clone()));
            };
            if !source_may_set(source, descriptor.scope) {
                return Err(ResolveError::UnauthorizedSource {
                    key: key.clone(),
                    source,
                    scope: descriptor.scope,
                });
            }
            validate_value(key, &descriptor.value_type, value)?;
        }
    }

    for (key, constraint) in &constraints.values {
        let Some(descriptor) = registry.get(key) else {
            return Err(ResolveError::UnknownKey(key.clone()));
        };
        let candidate = match constraint {
            EffectiveConstraint::AtMost { value, .. } => value,
            EffectiveConstraint::Unavailable { fallback, .. } => fallback,
        };
        validate_value(key, &descriptor.value_type, candidate)
            .map_err(|_| ResolveError::InvalidConstraint(key.clone()))?;
    }

    let mut resolved = BTreeMap::new();
    for (key, descriptor) in registry.iter() {
        let (requested, source) =
            requested_value(key, descriptor.scope, layers, &descriptor.default);
        let (effective, reason) = match constraints.values.get(key) {
            Some(EffectiveConstraint::AtMost { value, reason })
                if is_greater(&requested, value, key)? =>
            {
                (value.clone(), Some(*reason))
            }
            Some(EffectiveConstraint::Unavailable { fallback, reason }) => {
                (fallback.clone(), Some(*reason))
            }
            _ => (requested.clone(), None),
        };
        resolved.insert(
            key.clone(),
            ResolvedValue {
                requested,
                effective,
                source,
                constraint: reason,
            },
        );
    }

    validate_semantics(registry, &resolved, true)?;
    let output = EffectiveSettings { values: resolved };
    validate_semantics(registry, &output.values, false)?;
    Ok(output)
}

pub(crate) fn source_may_set(source: SettingSource, scope: SettingScope) -> bool {
    match source {
        SettingSource::Package | SettingSource::Preset => scope == SettingScope::P,
        SettingSource::Assignment => matches!(scope, SettingScope::P | SettingScope::O),
        SettingSource::Application => scope == SettingScope::A,
        SettingSource::Session => {
            matches!(scope, SettingScope::P | SettingScope::O | SettingScope::S)
        }
    }
}

fn requested_value(
    key: &SettingKey,
    scope: SettingScope,
    layers: &SettingLayers,
    default: &SettingValue,
) -> (SettingValue, Option<SettingSource>) {
    let sources: &[SettingSource] = match scope {
        SettingScope::P => &[
            SettingSource::Package,
            SettingSource::Preset,
            SettingSource::Assignment,
            SettingSource::Session,
        ],
        SettingScope::A => &[SettingSource::Application],
        SettingScope::O => &[SettingSource::Assignment, SettingSource::Session],
        SettingScope::S => &[SettingSource::Session],
    };
    let mut selected = (default.clone(), None);
    for source in sources {
        if let Some(value) = layers.values(*source).get(key) {
            selected = (value.clone(), Some(*source));
        }
    }
    selected
}

fn validate_value(
    key: &SettingKey,
    expected: &SettingType,
    value: &SettingValue,
) -> Result<(), ResolveError> {
    expected
        .validates(value)
        .map_err(|violation| ResolveError::InvalidValue {
            key: key.clone(),
            violation,
        })
}

fn is_greater(
    left: &SettingValue,
    right: &SettingValue,
    key: &SettingKey,
) -> Result<bool, ResolveError> {
    match (left, right) {
        (SettingValue::Integer(left), SettingValue::Integer(right)) => Ok(left > right),
        (SettingValue::Number(left), SettingValue::Number(right)) => Ok(left > right),
        _ => Err(ResolveError::InvalidConstraint(key.clone())),
    }
}

fn validate_semantics(
    registry: &Registry,
    values: &BTreeMap<SettingKey, ResolvedValue>,
    requested: bool,
) -> Result<(), ResolveError> {
    for descriptor in registry.descriptors() {
        let Some(value) = values.get(&descriptor.key) else {
            continue;
        };
        let value = if requested {
            &value.requested
        } else {
            &value.effective
        };
        match &descriptor.semantic_validator {
            SemanticValidator::None => {}
            SemanticValidator::MustBeTrue => {
                if !matches!(value, SettingValue::Boolean(true)) {
                    return Err(ResolveError::SemanticViolation {
                        key: descriptor.key.clone(),
                        related: None,
                    });
                }
            }
            SemanticValidator::LessThan(other) | SemanticValidator::GreaterThan(other) => {
                let Some(other_value) = values.get(other) else {
                    return Err(ResolveError::SemanticViolation {
                        key: descriptor.key.clone(),
                        related: Some(other.clone()),
                    });
                };
                let other_value = if requested {
                    &other_value.requested
                } else {
                    &other_value.effective
                };
                let valid = match (&descriptor.semantic_validator, value, other_value) {
                    (
                        SemanticValidator::LessThan(_),
                        SettingValue::Integer(left),
                        SettingValue::Integer(right),
                    ) => left < right,
                    (
                        SemanticValidator::GreaterThan(_),
                        SettingValue::Integer(left),
                        SettingValue::Integer(right),
                    ) => left > right,
                    (
                        SemanticValidator::LessThan(_),
                        SettingValue::Number(left),
                        SettingValue::Number(right),
                    ) => left < right,
                    (
                        SemanticValidator::GreaterThan(_),
                        SettingValue::Number(left),
                        SettingValue::Number(right),
                    ) => left > right,
                    _ => false,
                };
                if !valid {
                    return Err(ResolveError::SemanticViolation {
                        key: descriptor.key.clone(),
                        related: Some(other.clone()),
                    });
                }
            }
        }
    }
    Ok(())
}
