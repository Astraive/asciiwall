use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Manifest's package source discriminator.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Source {
    Scene { path: String },
    Image { path: String },
    Video { path: String },
    AsciiClip { path: String },
    RenderedVideo { path: String },
}

/// Declared content record; actual bytes must be checked by a later inventory phase.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub path: String,
    pub media_type: String,
    pub sha256: String,
    pub size: u64,
    pub license: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
}

/// Read-only actual asset information supplied by an archive/filesystem phase.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AssetInventory {
    pub assets: BTreeMap<String, InventoryEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InventoryEntry {
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Author {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EngineRequirement {
    pub min_version: String,
    pub features_required: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub required: Vec<Capability>,
    pub optional: Vec<Capability>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    PointerPosition,
    PointerButtons,
    WindowRectangles,
    WindowState,
    LocalClock,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub path: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FallbackKind {
    Image,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Fallback {
    pub kind: FallbackKind,
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FontAsset {
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub base_id: String,
    pub base_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_sha256: Option<String>,
    pub changes: String,
}

/// Structurally parsed draft package manifest. Absent setting values stay absent.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub format_version: String,
    pub id: String,
    pub version: String,
    pub name: String,
    pub summary: String,
    pub authors: Vec<Author>,
    pub license: String,
    pub license_files: Vec<String>,
    pub engine: EngineRequirement,
    pub source: Source,
    pub assets: Vec<Asset>,
    pub settings: BTreeMap<String, serde_json::Value>,
    pub capabilities: Capabilities,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Preview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Fallback>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<FontAsset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
}

/// Shared scene data remains declarative; graph semantics belong to PKG-003.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub scene_version: String,
    pub seed: u32,
    pub nodes: Vec<SceneNode>,
    pub edges: Vec<SceneEdge>,
    pub output: String,
    pub controls: Vec<SceneControl>,
}

/// Closed built-in node union from the draft scene schema.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum SceneNode {
    #[serde(rename = "solid")]
    Solid {
        id: String,
        params: SolidParams,
    },
    #[serde(rename = "matrix-rain")]
    MatrixRain {
        id: String,
        params: MatrixRainParams,
    },
    #[serde(rename = "starfield")]
    Starfield {
        id: String,
        params: StarfieldParams,
    },
    #[serde(rename = "waves")]
    Waves {
        id: String,
        params: WavesParams,
    },
    #[serde(rename = "noise-field")]
    NoiseField {
        id: String,
        params: NoiseFieldParams,
    },
    #[serde(rename = "cube-wireframe")]
    CubeWireframe {
        id: String,
        params: CubeWireframeParams,
    },
    #[serde(rename = "image-source")]
    ImageSource {
        id: String,
        params: PathParams,
    },
    #[serde(rename = "video-source")]
    VideoSource {
        id: String,
        params: PathParams,
    },
    #[serde(rename = "glyph-color")]
    GlyphColor {
        id: String,
        params: GlyphColorParams,
    },
    #[serde(rename = "pointer-force")]
    PointerForce {
        id: String,
        params: PointerForceParams,
    },
    #[serde(rename = "click-ripple")]
    ClickRipple {
        id: String,
        params: ClickRippleParams,
    },
    #[serde(rename = "window-avoid")]
    WindowAvoid {
        id: String,
        params: WindowAvoidParams,
    },
    Trails {
        id: String,
        params: TrailsParams,
    },
    Composite {
        id: String,
        params: CompositeParams,
    },
}

macro_rules! params {
    ($($name:ident { $($field:ident : $ty:ty),+ $(,)? }),+ $(,)?) => {
        $(
            #[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
            #[serde(deny_unknown_fields)]
            pub struct $name { $(pub $field: $ty),+ }
        )+
    };
}

params! {
    SolidParams { color: String },
    MatrixRainParams { density: f64, speed: f64, trail_cells: u32, color: String },
    StarfieldParams { count: u32, speed: f64, depth: f64, color: String },
    WavesParams { amplitude: f64, frequency: f64, speed: f64, color: String },
    NoiseFieldParams { scale: f64, speed: f64, color: String },
    CubeWireframeParams { speed: f64, scale: f64, color: String },
    PathParams { path: String },
    GlyphColorParams { color: String, strength: f64 },
    PointerForceParams { mode: PointerForceMode, radius_cells: f64, strength: f64 },
    ClickRippleParams { radius_cells: f64, duration_ms: u32, strength: f64 },
    WindowAvoidParams { margin_cells: f64, strength: f64 },
    TrailsParams { decay: f64 },
    CompositeParams { mode: CompositeMode, opacity: f64 },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PointerForceMode {
    Attract,
    Repel,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CompositeMode {
    Over,
    Add,
    Multiply,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneEdge {
    pub from: String,
    pub to: String,
    pub slot: u8,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneControl {
    pub id: String,
    pub label: String,
    pub setting: String,
}

/// Manifest after structural parsing. Later semantic phases can extend this contract.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedManifest {
    pub manifest: Manifest,
}
