use std::collections::BTreeMap;

use ascii_package::{AssetInventory, ErrorKind, InventoryEntry, validate_manifest, validate_scene};
use serde_json::json;

const ASSET_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn validate_asset(
    path: &str,
    media_type: &str,
) -> Result<ascii_package::ValidatedManifest, ascii_package::Error> {
    let manifest = json!({
        "format": "asciiwall",
        "format_version": "1.0",
        "id": "example.package",
        "version": "1.2.3",
        "name": "Example",
        "summary": "Test package",
        "authors": [{ "name": "Fixture Author" }],
        "license": "CC0-1.0",
        "license_files": ["licenses/artwork.txt"],
        "engine": { "min_version": "1.0.0", "features_required": [] },
        "source": {
            "kind": if path.ends_with(".ascii-clip") { "ascii-clip" } else { "image" },
            "path": path
        },
        "capabilities": { "required": [], "optional": [] },
        "settings": {},
        "tags": [],
        "assets": [{
            "path": path,
            "media_type": media_type,
            "sha256": ASSET_HASH,
            "size": 0,
            "license": "CC0-1.0"
        }]
    });
    let inventory = AssetInventory {
        assets: BTreeMap::from([(
            path.to_owned(),
            InventoryEntry {
                size: 0,
                sha256: ASSET_HASH.to_owned(),
            },
        )]),
    };
    validate_manifest(&serde_json::to_vec(&manifest).unwrap(), &inventory)
}

#[test]
fn accepts_builtin_scene_data_and_bounded_reaction_controls() {
    let scene = json!({
        "scene_version": "1.0",
        "seed": 7,
        "nodes": [{
            "id": "rain",
            "kind": "matrix-rain",
            "params": {
                "density": 0.5,
                "speed": 2.0,
                "trail_cells": 8,
                "color": "#00ff00ff"
            }
        }],
        "edges": [],
        "output": "rain",
        "controls": [{
            "id": "trails",
            "label": "Trails",
            "setting": "effects.trails.enabled"
        }]
    });

    let validated = validate_scene(scene.to_string().as_bytes()).unwrap();
    assert_eq!(validated.nodes.len(), 1);
    assert_eq!(validated.controls.len(), 1);
}

#[test]
fn rejects_unregistered_executable_scene_payload_kinds() {
    for (kind, params) in [
        ("rust", json!({ "source": "fn run() {}" })),
        ("javascript", json!({ "source": "process.exit(0)" })),
        ("shell", json!({ "script": "#!/bin/sh\ntrue" })),
        ("native-library", json!({ "path": "bin/plugin.so" })),
        (
            "process-command",
            json!({ "command": "sh", "args": ["-c", "true"] }),
        ),
    ] {
        let scene = json!({
            "scene_version": "1.0",
            "seed": 0,
            "nodes": [{ "id": "payload", "kind": kind, "params": params }],
            "edges": [],
            "output": "payload",
            "controls": []
        });

        assert!(
            validate_scene(scene.to_string().as_bytes()).is_err(),
            "accepted executable node kind {kind}"
        );
    }
}

#[test]
fn package_settings_cannot_enable_developer_mode() {
    let manifest = json!({
        "format": "asciiwall",
        "format_version": "1.0",
        "id": "example.package",
        "version": "1.2.3",
        "name": "Example",
        "summary": "Test package",
        "authors": [{ "name": "Fixture Author" }],
        "license": "CC0-1.0",
        "license_files": ["licenses/artwork.txt"],
        "engine": { "min_version": "1.0.0", "features_required": [] },
        "source": { "kind": "image", "path": "assets/source.png" },
        "settings": { "developer-mode": true },
        "capabilities": { "required": [], "optional": [] },
        "tags": [],
        "assets": [{
            "path": "assets/source.png",
            "media_type": "image/png",
            "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "size": 0,
            "license": "CC0-1.0"
        }]
    });
    let bytes = serde_json::to_vec(&manifest).unwrap();
    let error = validate_manifest(&bytes, &AssetInventory::default()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Structural);
    assert!(error.location.contains("settings"));
}

#[test]
fn rejects_executable_asset_types_and_extensions_with_matching_inventory() {
    for (path, media_type) in [
        ("assets/plugin.so", "application/x-sharedlib"),
        ("assets/plugin.dll", "application/x-msdownload"),
        ("assets/plugin.wasm", "application/wasm"),
        ("assets/run.js", "text/javascript"),
        ("assets/run.sh", "text/x-shellscript"),
        ("assets/run.exe", "application/octet-stream"),
        ("assets/renamed.wasm", "image/png"),
        ("assets/renamed.so", "text/plain"),
    ] {
        let error = validate_asset(path, media_type).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Structural, "{path} {media_type}");
        assert_eq!(
            error.location, "$.assets[0].media_type",
            "{path} {media_type}"
        );
        assert!(
            error.message.contains("package policy"),
            "{path} {media_type}: {}",
            error.message
        );
    }
}

#[test]
fn accepts_supported_local_media_font_data_and_ascii_clip_assets() {
    for (path, media_type) in [
        ("assets/scene.json", "application/json"),
        ("licenses/artwork.txt", "text/plain"),
        ("assets/image.png", "image/png"),
        ("assets/image.jpg", "image/jpeg"),
        ("assets/movie.webm", "video/webm"),
        ("fonts/typeface.ttf", "font/ttf"),
        (
            "assets/waves.ascii-clip",
            "application/vnd.asciiwall.ascii-clip",
        ),
    ] {
        assert!(
            validate_asset(path, media_type).is_ok(),
            "rejected supported asset {path} {media_type}"
        );
    }
}

#[test]
fn rejects_mismatched_and_executable_ascii_clip_asset_declarations() {
    for (path, media_type) in [
        ("assets/waves.ascii-clip", "application/octet-stream"),
        ("assets/waves.ascii-clip", "application/wasm"),
        ("assets/waves.exe", "application/vnd.asciiwall.ascii-clip"),
    ] {
        let error = validate_asset(path, media_type).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Structural, "{path} {media_type}");
        assert_eq!(
            error.location, "$.assets[0].media_type",
            "{path} {media_type}"
        );
    }
}
