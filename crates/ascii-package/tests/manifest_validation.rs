use ascii_package::{AssetInventory, ErrorKind, InventoryEntry, Manifest, validate_manifest};

const BASIC: &[u8] = br#"{
  "format": "asciiwall",
  "format_version": "1.0",
  "id": "example.package",
  "version": "1.2.3",
  "name": "Example",
  "summary": "Test package",
  "authors": [{"name": "Fixture Author"}],
  "license": "CC0-1.0",
  "license_files": ["licenses/artwork.txt"],
  "engine": {"min_version": "1.0.0", "features_required": []},
  "source": {"kind": "image", "path": "assets/source.png"},
  "settings": {},
  "capabilities": {"required": [], "optional": []},
  "tags": [],
  "assets": [{"path": "assets/source.png", "media_type": "image/png", "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "size": 0, "license": "CC0-1.0"}, {"path": "licenses/artwork.txt", "media_type": "text/plain", "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "size": 0, "license": "CC0-1.0"}]
}"#;

fn inventory_for(bytes: &[u8]) -> AssetInventory {
    let manifest: Manifest = serde_json::from_slice(bytes).unwrap();
    AssetInventory {
        assets: manifest
            .assets
            .into_iter()
            .map(|asset| {
                (
                    asset.path,
                    InventoryEntry {
                        size: asset.size,
                        sha256: asset.sha256,
                    },
                )
            })
            .collect(),
        directories: std::collections::BTreeSet::new(),
        duplicate_paths: Vec::new(),
        license_texts: std::collections::BTreeSet::new(),
    }
}

#[test]
fn preserves_absent_settings_and_accepts_v1_source() {
    let valid = validate_manifest(BASIC, &inventory_for(BASIC)).unwrap();
    assert!(valid.manifest.settings.is_empty());
}

#[test]
fn reports_unknown_source_as_structural_error() {
    let bytes = String::from_utf8(BASIC.to_vec())
        .unwrap()
        .replace("\"kind\": \"image\"", "\"kind\": \"script\"");
    let error = validate_manifest(bytes.as_bytes(), &AssetInventory::default()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Structural);
}

#[test]
fn separates_future_format_from_structure() {
    let bytes = String::from_utf8(BASIC.to_vec())
        .unwrap()
        .replace("\"1.0\"", "\"99.0\"");
    let error = validate_manifest(bytes.as_bytes(), &AssetInventory::default()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::UnsupportedFormat);
}

#[test]
fn rejects_permission_settings() {
    let bytes = String::from_utf8(BASIC.to_vec()).unwrap().replace(
        "\"settings\": {}",
        "\"settings\": {\"privacy.network-enabled\":true}",
    );
    let error = validate_manifest(bytes.as_bytes(), &AssetInventory::default()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Structural);
    assert!(error.location.contains("settings"));
}

#[test]
fn rejects_unknown_top_level_property() {
    let bytes = String::from_utf8(BASIC.to_vec())
        .unwrap()
        .replace("\"format\":", "\"unexpected\":true,\"format\":");
    let error = validate_manifest(bytes.as_bytes(), &AssetInventory::default()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Structural);
}

#[test]
fn rejects_oversized_manifest_before_parsing() {
    let bytes = vec![b' '; ascii_package::MAX_MANIFEST_BYTES + 1];
    let error = validate_manifest(&bytes, &AssetInventory::default()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::TooLarge);
}

#[test]
fn accepts_minimal_closed_scene() {
    let scene = br##"{
      "scene_version":"1.0",
      "seed":4294967295,
      "nodes":[{"id":"solid-one","kind":"solid","params":{"color":"#112233ff"}}],
      "edges":[],
      "output":"solid-one",
      "controls":[]
    }"##;
    let scene = ascii_package::validate_scene(scene).unwrap();
    assert_eq!(scene.seed, u32::MAX);
}

#[test]
fn rejects_custom_executable_scene_node() {
    let scene = br#"{
      "scene_version":"1.0",
      "seed":1,
      "nodes":[{"id":"custom","kind":"custom-wgsl","params":{}}],
      "edges":[],
      "output":"custom",
      "controls":[]
    }"#;
    assert_eq!(
        ascii_package::validate_scene(scene).unwrap_err().kind,
        ErrorKind::Structural
    );
}

#[test]
fn rejects_scene_parameter_boundary_overflow() {
    let scene = br##"{
      "scene_version":"1.0",
      "seed":1,
      "nodes":[{"id":"solid","kind":"solid","params":{"color":"#112233ff","extra":true}}],
      "edges":[],
      "output":"solid",
      "controls":[]
    }"##;
    assert_eq!(
        ascii_package::validate_scene(scene).unwrap_err().kind,
        ErrorKind::Structural
    );
}

#[test]
fn checked_in_schemas_are_versioned_and_self_contained() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../schemas/manifest-1.0.json")).unwrap();
    let scene: serde_json::Value =
        serde_json::from_str(include_str!("../schemas/scene-1.0.json")).unwrap();
    assert_eq!(
        manifest["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(manifest["$id"], "urn:asciiwall:schema:manifest:1.0");
    assert_eq!(
        scene["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(scene["$id"], "urn:asciiwall:schema:scene:1.0");
    assert!(!include_str!("../schemas/manifest-1.0.json").contains("\"$ref\""));
    assert!(!include_str!("../schemas/scene-1.0.json").contains("\"$ref\""));
}

#[test]
fn registry_comparison_reports_missing_cfg_descriptors() {
    let error =
        ascii_package::compare_portable_settings(&ascii_config::Registry::empty()).unwrap_err();
    let ascii_package::RegistryComparisonError::Drift(drift) = error else {
        panic!("expected drift against empty CFG foundation registry");
    };
    assert!(!drift.is_empty());
    assert!(
        drift
            .iter()
            .all(|item| item.reason == "missing CFG descriptor")
    );
}

#[test]
fn validates_both_positive_documentation_fixtures_locally() {
    let manifest = include_bytes!("fixtures/manifest.json");
    let scene = include_bytes!("fixtures/scene.json");
    assert!(validate_manifest(manifest, &inventory_for(manifest)).is_ok());
    assert!(ascii_package::validate_scene(scene).is_ok());
}

#[test]
fn rejects_manifest_depth_above_limit_before_deserialization() {
    let nested = format!(
        "{}{}{}",
        "[".repeat(ascii_package::MAX_JSON_DEPTH + 1),
        "0",
        "]".repeat(ascii_package::MAX_JSON_DEPTH + 1)
    );
    let error = validate_manifest(nested.as_bytes(), &AssetInventory::default()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Structural);
    assert_eq!(error.location, "$");
}

#[test]
fn deleting_each_document_fixture_required_property_fails() {
    let cases = [
        (
            include_bytes!("fixtures/manifest.json").as_slice(),
            include_str!("../schemas/manifest-1.0.json"),
            true,
        ),
        (
            include_bytes!("fixtures/scene.json").as_slice(),
            include_str!("../schemas/scene-1.0.json"),
            false,
        ),
    ];
    for (fixture, schema_text, is_manifest) in cases {
        let document: serde_json::Value = serde_json::from_slice(fixture).unwrap();
        let schema: serde_json::Value = serde_json::from_str(schema_text).unwrap();
        let mut paths = Vec::new();
        collect_required_paths(&schema, &document, "", &mut paths);
        assert!(!paths.is_empty());
        for path in paths {
            let mut mutated = document.clone();
            let (parent, key) = path.rsplit_once('/').unwrap();
            let parent = if parent.is_empty() { "" } else { parent };
            let key = key.replace("~1", "/").replace("~0", "~");
            assert!(
                mutated
                    .pointer_mut(parent)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(&key)
                    .is_some()
            );
            let bytes = serde_json::to_vec(&mutated).unwrap();
            let result = if is_manifest {
                validate_manifest(&bytes, &AssetInventory::default()).map(|_| ())
            } else {
                ascii_package::validate_scene(&bytes).map(|_| ())
            };
            assert_eq!(
                result.unwrap_err().kind,
                ErrorKind::Structural,
                "mutation at {path}"
            );
        }
    }
}

fn collect_required_paths(
    schema: &serde_json::Value,
    instance: &serde_json::Value,
    path: &str,
    output: &mut Vec<String>,
) {
    if let Some(branches) = schema.get("oneOf").and_then(serde_json::Value::as_array) {
        if let Some(branch) = branches
            .iter()
            .find(|branch| branch_matches(branch, instance))
        {
            collect_required_paths(branch, instance, path, output);
        }
        return;
    }
    if let Some(required) = schema.get("required").and_then(serde_json::Value::as_array) {
        for key in required.iter().filter_map(serde_json::Value::as_str) {
            if instance.get(key).is_some() {
                output.push(format!(
                    "{path}/{}",
                    key.replace('~', "~0").replace('/', "~1")
                ));
            }
        }
    }
    if let (Some(properties), Some(object)) = (
        schema
            .get("properties")
            .and_then(serde_json::Value::as_object),
        instance.as_object(),
    ) {
        for (key, value) in object {
            if let Some(child_schema) = properties.get(key) {
                collect_required_paths(
                    child_schema,
                    value,
                    &format!("{path}/{}", key.replace('~', "~0").replace('/', "~1")),
                    output,
                );
            }
        }
    }
    if let (Some(item_schema), Some(items)) = (schema.get("items"), instance.as_array()) {
        for (index, item) in items.iter().enumerate() {
            collect_required_paths(item_schema, item, &format!("{path}/{index}"), output);
        }
    }
}

fn branch_matches(branch: &serde_json::Value, instance: &serde_json::Value) -> bool {
    branch
        .pointer("/properties/kind/const")
        .is_none_or(|expected| instance.get("kind") == Some(expected))
}

#[test]
fn portable_settings_enforce_inclusive_boundaries() {
    for (columns, valid) in [(32, true), (1024, true), (31, false), (1025, false)] {
        let fixture = String::from_utf8(BASIC.to_vec()).unwrap().replace(
            "\"settings\": {}",
            &format!("\"settings\": {{\"grid.columns\":{columns}}}"),
        );
        assert_eq!(
            validate_manifest(fixture.as_bytes(), &inventory_for(fixture.as_bytes())).is_ok(),
            valid,
            "grid.columns={columns}"
        );
    }
}

#[test]
fn scene_numeric_ranges_are_inclusive_and_enforced() {
    let valid = br##"{
      "scene_version":"1.0","seed":0,
      "nodes":[{"id":"rain","kind":"matrix-rain","params":{"density":1,"speed":0,"trail_cells":1,"color":"#00000000"}}],
      "edges":[],"output":"rain","controls":[]
    }"##;
    assert!(ascii_package::validate_scene(valid).is_ok());
    let invalid = String::from_utf8(valid.to_vec())
        .unwrap()
        .replace("\"density\":1", "\"density\":1.01");
    assert_eq!(
        ascii_package::validate_scene(invalid.as_bytes())
            .unwrap_err()
            .kind,
        ErrorKind::Structural
    );
}

#[test]
fn rejects_asset_inventory_hash_and_size_drift() {
    let mut inventory = inventory_for(BASIC);
    let entry = inventory.assets.get_mut("assets/source.png").unwrap();
    entry.size += 1;
    let error = validate_manifest(BASIC, &inventory).unwrap_err();
    assert_eq!(error.kind, ErrorKind::InventoryMismatch);
    assert_eq!(error.location, "$.assets[0]");
    let mut inventory = inventory_for(BASIC);
    inventory
        .assets
        .get_mut("assets/source.png")
        .unwrap()
        .sha256 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
    let error = validate_manifest(BASIC, &inventory).unwrap_err();
    assert_eq!(error.kind, ErrorKind::InventoryMismatch);
}

#[test]
fn rejects_missing_asset_inventory_entry() {
    let error = validate_manifest(BASIC, &AssetInventory::default()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::InventoryMismatch);
    assert!(error.location.contains("assets[0]"));
}

#[test]
fn rejects_nonportable_and_unresolved_asset_paths() {
    for (from, to) in [
        ("assets/source.png", "assets/../source.png"),
        ("assets/source.png", "assets//source.png"),
        ("assets/source.png", "assets/con.txt"),
        ("assets/source.png", "assets/source.png."),
        (
            "assets/source.png",
            "assets/foo/bar/baz/quux/quuz/corge/grault/garply/source.png",
        ),
    ] {
        let document = String::from_utf8(BASIC.to_vec()).unwrap().replace(from, to);
        let manifest: Manifest = serde_json::from_str(&document).unwrap();
        let inventory = inventory_for(document.as_bytes());
        let error = validate_manifest(document.as_bytes(), &inventory).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Semantic, "{manifest:?}");
    }
    let document = String::from_utf8(BASIC.to_vec())
        .unwrap()
        .replace("assets/source.png", "assets/other.png");
    let inventory = inventory_for(document.as_bytes());
    let missing = document.replace("assets/other.png", "assets/missing.png");
    assert_eq!(
        validate_manifest(missing.as_bytes(), &inventory)
            .unwrap_err()
            .kind,
        ErrorKind::InventoryMismatch
    );
}

#[test]
fn rejects_overlapping_capabilities_and_undeclared_files() {
    let document = String::from_utf8(BASIC.to_vec()).unwrap().replace(
        "\"required\": [], \"optional\": []",
        "\"required\": [\"local-clock\"], \"optional\": [\"local-clock\"]",
    );
    assert_eq!(
        validate_manifest(document.as_bytes(), &inventory_for(document.as_bytes()))
            .unwrap_err()
            .kind,
        ErrorKind::Semantic
    );
    let mut inventory = inventory_for(BASIC);
    inventory.assets.insert(
        "unused/readme.txt".into(),
        InventoryEntry {
            size: 0,
            sha256: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into(),
        },
    );
    assert_eq!(
        validate_manifest(BASIC, &inventory).unwrap_err().kind,
        ErrorKind::InventoryMismatch
    );
    let mut inventory = inventory_for(BASIC);
    inventory.directories.insert("assets/source.png".into());
    assert_eq!(
        validate_manifest(BASIC, &inventory).unwrap_err().kind,
        ErrorKind::Semantic
    );
    let mut inventory = inventory_for(BASIC);
    inventory.duplicate_paths.push("Assets/source.png".into());
    assert_eq!(
        validate_manifest(BASIC, &inventory).unwrap_err().kind,
        ErrorKind::InventoryMismatch
    );
}

#[test]
fn enforces_entry_and_expanded_size_budgets() {
    let mut inventory = inventory_for(BASIC);
    for index in 0..4093 {
        inventory.directories.insert(format!("dir{index}"));
    }
    assert!(validate_manifest(BASIC, &inventory).is_ok());
    inventory.directories.insert("dir4093".into());
    assert_eq!(
        validate_manifest(BASIC, &inventory).unwrap_err().kind,
        ErrorKind::Semantic
    );
    let mut document = String::from_utf8(BASIC.to_vec()).unwrap();
    document = document.replace("\"size\": 0", "\"size\": 1073741824");
    let mut manifest: Manifest = serde_json::from_str(&document).unwrap();
    let mut inventory = inventory_for(document.as_bytes());
    let source = manifest
        .assets
        .iter_mut()
        .find(|asset| asset.path == "assets/source.png")
        .unwrap();
    source.size = 1073741824;
    inventory.assets.get_mut("assets/source.png").unwrap().size = 1073741824;
    manifest.assets[1].size = 1073741824;
    inventory
        .assets
        .get_mut("licenses/artwork.txt")
        .unwrap()
        .size = 1073741824;
    let document = serde_json::to_vec(&manifest).unwrap();
    assert_eq!(
        validate_manifest(&document, &inventory).unwrap_err().kind,
        ErrorKind::Semantic
    );
}

#[test]
fn requires_license_text_evidence_and_package_font_asset() {
    let custom_license = String::from_utf8(BASIC.to_vec()).replacen(
        "\"license\": \"CC0-1.0\"",
        "\"license\": \"LicenseRef-Example\"",
        1,
    );
    let mut inventory = inventory_for(custom_license.as_bytes());
    let error = validate_manifest(custom_license.as_bytes(), &inventory).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Semantic);
    inventory
        .license_texts
        .insert(("licenses/artwork.txt".into(), "LicenseRef-Example".into()));
    assert!(validate_manifest(custom_license.as_bytes(), &inventory).is_ok());

    let package_font = String::from_utf8(BASIC.to_vec()).unwrap().replace(
        "\"settings\": {}",
        "\"settings\": {\"font.face\": \"package-font\"}",
    );
    assert_eq!(
        validate_manifest(
            package_font.as_bytes(),
            &inventory_for(package_font.as_bytes())
        )
        .unwrap_err()
        .kind,
        ErrorKind::Semantic
    );
}

#[test]
fn accepts_only_display_http_urls_without_fetching() {
    let https = String::from_utf8(BASIC.to_vec()).unwrap().replace(
        "{\"name\": \"Fixture Author\"}",
        "{\"name\": \"Fixture Author\", \"url\": \"https://example.invalid/about\"}",
    );
    assert!(validate_manifest(https.as_bytes(), &inventory_for(https.as_bytes())).is_ok());
    let encoded = https.replace(
        "https://example.invalid/about",
        "https://example.invalid/a%20b",
    );
    assert!(validate_manifest(encoded.as_bytes(), &inventory_for(encoded.as_bytes())).is_ok());
    let invalid = https.replace("https://example.invalid/about", "file:///secret");
    assert_eq!(
        validate_manifest(invalid.as_bytes(), &inventory_for(invalid.as_bytes()))
            .unwrap_err()
            .kind,
        ErrorKind::Semantic
    );
    for url in [
        "https://example.invalid/a b",
        "https://example.invalid/a\\u0001b",
        "https:///about",
        "https://example.invalid:bad/about",
    ] {
        let invalid = https.replace("https://example.invalid/about", url);
        let error =
            validate_manifest(invalid.as_bytes(), &inventory_for(invalid.as_bytes())).unwrap_err();
        assert!(
            matches!(error.kind, ErrorKind::Structural | ErrorKind::Semantic),
            "invalid URL was accepted"
        );
    }
}
