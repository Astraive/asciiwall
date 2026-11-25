use std::collections::BTreeSet;

use crate::{AssetInventory, Error, ErrorKind, Manifest, Source};

const MAX_PACKAGE_ENTRIES: usize = 4096;
const MAX_EXPANDED_BYTES: u64 = 2_147_483_648;
const MAX_ASSET_BYTES: u64 = 1_073_741_824;
const MAX_FONT_BYTES: u64 = 16_777_216;
const MAX_PREVIEW_BYTES: u64 = 8_388_608;

pub(crate) fn validate_manifest_semantics(
    manifest: &Manifest,
    inventory: &AssetInventory,
) -> Result<(), Error> {
    for (index, author) in manifest.authors.iter().enumerate() {
        if let Some(url) = &author.url {
            let Some((scheme, authority)) = url.split_once("://") else {
                return Err(semantic(
                    &format!("$.authors[{index}].url"),
                    "author URL must use HTTP or HTTPS",
                ));
            };
            let host = authority
                .split(|character| matches!(character, '/' | '?' | '#'))
                .next()
                .unwrap_or_default();
            if !matches!(scheme, "http" | "https")
                || host.is_empty()
                || !host.bytes().any(|byte| byte.is_ascii_alphanumeric())
                || authority.chars().any(char::is_whitespace)
                || authority.contains('@')
                || url.chars().any(char::is_control)
            {
                return Err(semantic(
                    &format!("$.authors[{index}].url"),
                    "author URL must be a display-only HTTP(S) URL",
                ));
            }
        }
    }
    let required: BTreeSet<_> = manifest.capabilities.required.iter().collect();
    if manifest
        .capabilities
        .optional
        .iter()
        .any(|capability| required.contains(capability))
    {
        return Err(semantic(
            "$.capabilities",
            "required and optional capabilities must be disjoint",
        ));
    }

    let mut referenced = BTreeSet::new();
    match &manifest.source {
        Source::Scene { path }
        | Source::Image { path }
        | Source::Video { path }
        | Source::AsciiClip { path }
        | Source::RenderedVideo { path } => {
            validate_portable_path(path, "$.source.path")?;
            referenced.insert(path.as_str());
        }
    }
    let mut license_paths = BTreeSet::new();
    for (index, path) in manifest.license_files.iter().enumerate() {
        validate_portable_path(path, &format!("$.license_files[{index}]"))?;
        if !license_paths.insert(path.as_str()) {
            return Err(semantic(
                &format!("$.license_files[{index}]"),
                "license file paths must be unique",
            ));
        }
        referenced.insert(path.as_str());
    }
    if let Some(preview) = &manifest.preview {
        validate_portable_path(&preview.path, "$.preview.path")?;
        referenced.insert(preview.path.as_str());
    }
    if let Some(fallback) = &manifest.fallback {
        validate_portable_path(&fallback.path, "$.fallback.path")?;
        referenced.insert(fallback.path.as_str());
    }
    if let Some(font) = &manifest.font {
        validate_portable_path(&font.path, "$.font.path")?;
        referenced.insert(font.path.as_str());
    }
    for path in referenced {
        if !manifest.assets.iter().any(|asset| asset.path == path) {
            return Err(Error {
                kind: ErrorKind::InventoryMismatch,
                location: "$.assets".into(),
                message: "manifest reference does not resolve to a declared asset".into(),
            });
        }
    }

    for (index, asset) in manifest.assets.iter().enumerate() {
        if asset.license.starts_with("LicenseRef-")
            && !manifest.license_files.iter().any(|path| {
                inventory
                    .license_texts
                    .iter()
                    .any(|(evidence_path, identifier)| {
                        evidence_path == path && identifier == &asset.license
                    })
            })
        {
            return Err(semantic(
                &format!("$.assets[{index}].license"),
                "LicenseRef requires supplied text evidence in a listed license file",
            ));
        }
    }
    if manifest.license.starts_with("LicenseRef-")
        && !manifest.license_files.iter().any(|path| {
            inventory
                .license_texts
                .iter()
                .any(|(evidence_path, identifier)| {
                    evidence_path == path && identifier == &manifest.license
                })
        })
    {
        return Err(semantic(
            "$.license",
            "LicenseRef requires supplied text evidence in a listed license file",
        ));
    }
    if manifest.settings.get("font.face") == Some(&serde_json::Value::String("package-font".into()))
        && manifest.font.is_none()
    {
        return Err(semantic(
            "$.settings.font.face",
            "package-font requires a declared font asset",
        ));
    }
    Ok(())
}

pub(crate) fn validate_asset_inventory(
    manifest_bytes: &[u8],
    manifest: &Manifest,
    inventory: &AssetInventory,
) -> Result<(), Error> {
    let mut paths = BTreeSet::new();
    let mut declared = BTreeSet::new();
    let mut expanded_bytes = u64::try_from(manifest_bytes.len())
        .map_err(|_| semantic("$.assets", "expanded size exceeds the supported range"))?;
    for (index, asset) in manifest.assets.iter().enumerate() {
        validate_portable_path(&asset.path, &format!("$.assets[{index}].path"))?;
        if !declared.insert(asset.path.as_str()) {
            return Err(semantic(
                &format!("$.assets[{index}].path"),
                "asset paths must be unique",
            ));
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
        if actual.size > MAX_ASSET_BYTES {
            return Err(semantic(
                &format!("$.assets[{index}].size"),
                "asset exceeds the v1 individual size limit",
            ));
        }
        expanded_bytes = expanded_bytes
            .checked_add(actual.size)
            .ok_or_else(|| semantic("$.assets", "expanded size exceeds the supported range"))?;
        paths.insert(asset.path.as_str());
    }
    for path in &inventory.directories {
        validate_portable_path(path, "$.inventory")?;
        if !paths.insert(path.as_str()) {
            return Err(semantic("$.inventory", "package paths collide"));
        }
    }
    for path in inventory.assets.keys() {
        validate_portable_path(path, "$.inventory")?;
        if !declared.contains(path.as_str()) {
            return Err(Error {
                kind: ErrorKind::InventoryMismatch,
                location: "$.assets".into(),
                message: "inventory contains undeclared files".into(),
            });
        }
    }
    if !inventory.duplicate_paths.is_empty() {
        return Err(Error {
            kind: ErrorKind::InventoryMismatch,
            location: "$.inventory".into(),
            message: "inventory contains duplicate package entries".into(),
        });
    }
    for path in &paths {
        let mut parent = *path;
        while let Some((prefix, _)) = parent.rsplit_once('/') {
            if inventory.assets.contains_key(prefix) {
                return Err(semantic(
                    "$.inventory",
                    "file path conflicts with a directory path",
                ));
            }
            parent = prefix;
        }
    }
    let entries = inventory
        .assets
        .len()
        .checked_add(inventory.directories.len())
        .and_then(|count| count.checked_add(inventory.duplicate_paths.len()))
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| semantic("$.inventory", "entry count exceeds the supported range"))?;
    if entries > MAX_PACKAGE_ENTRIES {
        return Err(semantic(
            "$.inventory",
            "package exceeds the v1 entry limit",
        ));
    }
    if expanded_bytes > MAX_EXPANDED_BYTES {
        return Err(semantic(
            "$.inventory",
            "package exceeds the v1 expanded-size limit",
        ));
    }
    for asset in &manifest.assets {
        let size = inventory.assets[&asset.path].size;
        if manifest
            .font
            .as_ref()
            .is_some_and(|font| font.path == asset.path)
            && size > MAX_FONT_BYTES
        {
            return Err(semantic(
                "$.font.path",
                "font exceeds the v1 font size limit",
            ));
        }
        if manifest
            .preview
            .as_ref()
            .is_some_and(|preview| preview.path == asset.path)
            && size > MAX_PREVIEW_BYTES
        {
            return Err(semantic(
                "$.preview.path",
                "preview exceeds the v1 preview size limit",
            ));
        }
    }
    Ok(())
}

fn validate_portable_path(path: &str, location: &str) -> Result<(), Error> {
    let components: Vec<_> = path.split('/').collect();
    let valid_chars = path
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-./".contains(&byte));
    let reserved = components.iter().any(|component| {
        let basename = component.split('.').next().unwrap_or_default();
        matches!(
            basename,
            "con"
                | "prn"
                | "aux"
                | "nul"
                | "com1"
                | "com2"
                | "com3"
                | "com4"
                | "com5"
                | "com6"
                | "com7"
                | "com8"
                | "com9"
                | "lpt1"
                | "lpt2"
                | "lpt3"
                | "lpt4"
                | "lpt5"
                | "lpt6"
                | "lpt7"
                | "lpt8"
                | "lpt9"
        )
    });
    if path.is_empty()
        || path.len() > 240
        || components.len() > 8
        || components.iter().any(|component| {
            component.is_empty()
                || *component == "."
                || *component == ".."
                || component.ends_with('.')
                || component.ends_with(' ')
        })
        || !valid_chars
        || reserved
    {
        return Err(semantic(
            location,
            "path is not a normalized v1 portable path",
        ));
    }
    Ok(())
}

fn semantic(location: &str, message: &str) -> Error {
    Error {
        kind: ErrorKind::Semantic,
        location: location.into(),
        message: message.into(),
    }
}
