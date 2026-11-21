use crate::document::{CURRENT_SCHEMA_VERSION, DecodedDocument};

/// Migration error for a schema version without a consecutive path to current.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MigrationError {
    UnsupportedVersion(i64),
}

/// Applies each consecutive migration in memory. The persistence controller
/// writes the result only after the migrated document validates successfully.
pub(crate) fn migrate(mut document: DecodedDocument) -> Result<DecodedDocument, MigrationError> {
    while document.schema_version < CURRENT_SCHEMA_VERSION {
        document = match document.schema_version {
            1 => migrate_v1_to_v2(document),
            version => return Err(MigrationError::UnsupportedVersion(version)),
        };
    }
    if document.schema_version != CURRENT_SCHEMA_VERSION {
        return Err(MigrationError::UnsupportedVersion(document.schema_version));
    }
    Ok(document)
}

/// The synthetic pre-release v1 format already used the closed TOML envelope;
/// v2 records the first supported registry contract. Values are preserved
/// byte-for-value and the original bytes remain the explicit `.bak` rollback.
fn migrate_v1_to_v2(mut document: DecodedDocument) -> DecodedDocument {
    document.schema_version = 2;
    document
}
