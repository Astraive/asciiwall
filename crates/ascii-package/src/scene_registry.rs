use crate::{Capability, SceneNode};

/// Version of the closed built-in scene descriptor set.
pub const SCENE_REGISTRY_VERSION: &str = "1.0";

/// Character-frame port contract used by every v1 built-in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScenePortType {
    CharacterFrame,
}

/// Number of incoming frame edges accepted by a built-in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneArity {
    Source,
    Unary,
    Composite,
}

/// Data-only descriptor for a built-in scene operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneNodeDescriptor {
    pub kind: &'static str,
    pub arity: SceneArity,
    pub input: Option<ScenePortType>,
    pub output: ScenePortType,
    pub capability: Option<Capability>,
    /// Image/video conversion is part of the source node, never an implicit port cast.
    pub embeds_conversion: bool,
}

const SOURCE: SceneNodeDescriptor = SceneNodeDescriptor {
    kind: "",
    arity: SceneArity::Source,
    input: None,
    output: ScenePortType::CharacterFrame,
    capability: None,
    embeds_conversion: false,
};
const UNARY: SceneNodeDescriptor = SceneNodeDescriptor {
    kind: "",
    arity: SceneArity::Unary,
    input: Some(ScenePortType::CharacterFrame),
    output: ScenePortType::CharacterFrame,
    capability: None,
    embeds_conversion: false,
};

/// Exact v1 built-in kind order, shared by schema and semantic validation.
pub const SCENE_NODE_DESCRIPTORS: [SceneNodeDescriptor; 14] = [
    SceneNodeDescriptor {
        kind: "solid",
        ..SOURCE
    },
    SceneNodeDescriptor {
        kind: "matrix-rain",
        ..SOURCE
    },
    SceneNodeDescriptor {
        kind: "starfield",
        ..SOURCE
    },
    SceneNodeDescriptor {
        kind: "waves",
        ..SOURCE
    },
    SceneNodeDescriptor {
        kind: "noise-field",
        ..SOURCE
    },
    SceneNodeDescriptor {
        kind: "cube-wireframe",
        ..SOURCE
    },
    SceneNodeDescriptor {
        kind: "image-source",
        embeds_conversion: true,
        ..SOURCE
    },
    SceneNodeDescriptor {
        kind: "video-source",
        embeds_conversion: true,
        ..SOURCE
    },
    SceneNodeDescriptor {
        kind: "glyph-color",
        ..UNARY
    },
    SceneNodeDescriptor {
        kind: "pointer-force",
        capability: Some(Capability::PointerPosition),
        ..UNARY
    },
    SceneNodeDescriptor {
        kind: "click-ripple",
        capability: Some(Capability::PointerButtons),
        ..UNARY
    },
    SceneNodeDescriptor {
        kind: "window-avoid",
        capability: Some(Capability::WindowRectangles),
        ..UNARY
    },
    SceneNodeDescriptor {
        kind: "trails",
        ..UNARY
    },
    SceneNodeDescriptor {
        kind: "composite",
        arity: SceneArity::Composite,
        input: Some(ScenePortType::CharacterFrame),
        output: ScenePortType::CharacterFrame,
        capability: None,
        embeds_conversion: false,
    },
];

/// Returns the closed descriptor for a deserialized v1 node.
#[must_use]
pub fn descriptor_for_node(node: &SceneNode) -> &'static SceneNodeDescriptor {
    let kind = node.kind_name();
    SCENE_NODE_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.kind == kind)
        .expect("SceneNode is the closed v1 built-in union")
}

/// Returns the JSON Schema parameter object for a registry kind.
///
/// Parameter authority remains the checked-in scene schema, avoiding a second
/// range/default table that could drift from the schema accepted at import.
pub fn parameter_schema(kind: &str) -> Option<&'static serde_json::Value> {
    static SCHEMA: std::sync::LazyLock<Option<serde_json::Value>> =
        std::sync::LazyLock::new(|| {
            serde_json::from_str(include_str!("../schemas/scene-1.0.json")).ok()
        });
    let schema = SCHEMA.as_ref()?;
    let branches = schema
        .pointer("/properties/nodes/items/oneOf")?
        .as_array()?;
    branches.iter().find_map(|branch| {
        (branch.pointer("/properties/kind/const")?.as_str()? == kind)
            .then(|| branch.pointer("/properties/params"))
            .flatten()
    })
}

impl SceneNode {
    /// Kind string fixed by the closed, deserializable v1 union.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Solid { .. } => "solid",
            Self::MatrixRain { .. } => "matrix-rain",
            Self::Starfield { .. } => "starfield",
            Self::Waves { .. } => "waves",
            Self::NoiseField { .. } => "noise-field",
            Self::CubeWireframe { .. } => "cube-wireframe",
            Self::ImageSource { .. } => "image-source",
            Self::VideoSource { .. } => "video-source",
            Self::GlyphColor { .. } => "glyph-color",
            Self::PointerForce { .. } => "pointer-force",
            Self::ClickRipple { .. } => "click-ripple",
            Self::WindowAvoid { .. } => "window-avoid",
            Self::Trails { .. } => "trails",
            Self::Composite { .. } => "composite",
        }
    }
}
