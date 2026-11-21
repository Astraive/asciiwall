use std::collections::BTreeMap;

use crate::model::{
    SemanticValidator, SettingDescriptor, SettingKey, SettingType,
};
use crate::resolve::{resolve_settings, Constraints, ResolveError, SettingLayers};

/// Immutable authoritative index of setting descriptors.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    descriptors: BTreeMap<SettingKey, SettingDescriptor>,
}

impl Registry {
    /// Builds a registry and checks key uniqueness and semantic-hook references.
    pub fn new(descriptors: impl IntoIterator<Item = SettingDescriptor>) -> Result<Self, RegistryError> {
        let mut by_key = BTreeMap::new();
        for descriptor in descriptors {
            let key = descriptor.key.clone();
            if by_key.insert(key.clone(), descriptor).is_some() {
                return Err(RegistryError::DuplicateKey(key));
            }
        }
        for descriptor in by_key.values() {
            match &descriptor.semantic_validator {
                SemanticValidator::None => {}
                SemanticValidator::MustBeTrue => {
                    if !matches!(&descriptor.value_type, SettingType::Boolean) {
                        return Err(RegistryError::IncompatibleSemanticHook(
                            descriptor.key.clone(),
                        ));
                    }
                }
                SemanticValidator::LessThan(other)
                | SemanticValidator::GreaterThan(other) => {
                    let Some(other_descriptor) = by_key.get(other) else {
                        return Err(RegistryError::MissingSemanticKey(
                            descriptor.key.clone(),
                            other.clone(),
                        ));
                    };
                    if !descriptor.value_type.same_numeric_kind(&other_descriptor.value_type) {
                        return Err(RegistryError::IncompatibleSemanticHook(
                            descriptor.key.clone(),
                        ));
                    }
                }
            }
        }
        let registry = Self { descriptors: by_key };
        resolve_settings(&registry, &SettingLayers::default(), &Constraints::default())
            .map_err(RegistryError::InvalidDefaults)?;
        Ok(registry)
    }

    /// Constructs the empty foundation registry. Domain owners add descriptors
    /// with source-owned defaults; no values are invented by this crate.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn get(&self, key: &SettingKey) -> Option<&SettingDescriptor> {
        self.descriptors.get(key)
    }

    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&SettingDescriptor> {
        SettingKey::new(key).ok().and_then(|key| self.get(&key))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &SettingDescriptor> {
        self.descriptors.values()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&SettingKey, &SettingDescriptor)> {
        self.descriptors.iter()
    }
}

/// Registry definitions are internally inconsistent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistryError {
    DuplicateKey(SettingKey),
    MissingSemanticKey(SettingKey, SettingKey),
    IncompatibleSemanticHook(SettingKey),
    InvalidDefaults(ResolveError),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid settings registry: {self:?}")
    }
}

impl std::error::Error for RegistryError {}
