use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use ascii_config::{Registry, SettingScope, SettingType, SettingValue};
use ascii_core::{AsciiCell, Budget};

use crate::{
    Capability, Manifest, Scene, SceneArity, SceneNode, compare_portable_settings,
    descriptor_for_node, settings::RegistryComparisonError,
};

pub const MAX_SCENE_NODES: usize = 128;
pub const MAX_SCENE_EDGES: usize = 256;
pub const MAX_DECODED_IMAGE_PIXELS: u64 = 64_000_000;
pub const MAX_DECODED_IMAGE_BYTES: u64 = 256 * 1024 * 1024;

/// Availability states use WPH-001 terminology without pretending to provide
/// the native adapter snapshot itself.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SceneFeedState {
    Supported,
    Unknown,
    Unavailable,
    Denied,
    Unsupported,
}

/// Stable adapter-provided reason code. It is carried through unchanged.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SceneFeedReason {
    pub code: String,
}

/// Data-only observation for one capability. `usable` is meaningful only when
/// the state is `Supported`; no platform state is probed by this crate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneFeedStatus {
    pub state: SceneFeedState,
    pub usable: bool,
    pub reason: SceneFeedReason,
}

impl SceneFeedStatus {
    fn not_reported() -> Self {
        Self {
            state: SceneFeedState::Unknown,
            usable: false,
            reason: SceneFeedReason {
                code: "not-reported".to_owned(),
            },
        }
    }
}

/// Capability feeds supplied by a host. Missing entries are explicit unknown /
/// `not-reported`; reason codes are opaque until WPH-001 defines the adapter
/// snapshot and package-to-host mapping. This value is data only: validation
/// never queries the operating system or changes observation policy.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SceneFeeds {
    pub capabilities: BTreeMap<Capability, SceneFeedStatus>,
}

impl SceneFeeds {
    fn status(&self, capability: Capability) -> SceneFeedStatus {
        self.capabilities
            .get(&capability)
            .cloned()
            .unwrap_or_else(SceneFeedStatus::not_reported)
    }
}

/// The observed feed and the deterministic activation decision derived from it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneFeedDecision {
    pub status: SceneFeedStatus,
    pub effective_available: bool,
    pub blocked_by_policy: bool,
}

/// Global policy constraints captured with a deterministic validation result.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScenePolicy {
    pub reduced_motion: bool,
    pub locked: bool,
    pub observation_disabled: bool,
}

/// Resource information known by the owning renderer/importer before allocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceResource {
    pub decoded_pixels: u64,
    pub decoded_bytes: u64,
}

/// Explicit engine-owned budgets. No package-level defaults are invented here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneResourceLimits {
    pub core: Budget,
    pub max_particles: u64,
    pub max_decoders: u64,
    pub max_aggregate_memory_bytes: u64,
}

/// Inputs needed to estimate aggregate graph cost before any execution allocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneResourceInputs {
    pub columns: u32,
    pub rows: u32,
    /// Per-source costs obtained from bounded media inspection, keyed by node id.
    pub sources: BTreeMap<String, SourceResource>,
    pub limits: SceneResourceLimits,
}

/// Checked graph-wide estimate, in cells, particles, decoder instances, bytes and
/// core work units (the latter is the only time-related proxy defined by COR-001).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneResourceEstimate {
    pub cells: u64,
    pub particles: u64,
    pub decoders: u64,
    pub aggregate_memory_bytes: u64,
    pub work_units: u64,
}

/// Deterministic data-only activation candidate. It contains no executor or OS handle.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneValidationPlan {
    pub registry_version: &'static str,
    pub topological_order: Vec<String>,
    pub required_capabilities: BTreeSet<Capability>,
    pub optional_capabilities: BTreeSet<Capability>,
    pub pass_through_nodes: Vec<String>,
    pub suppressed_nodes: Vec<String>,
    pub feed_decisions: BTreeMap<Capability, SceneFeedDecision>,
    pub activation_allowed: bool,
    pub seed: u32,
    pub effective_settings: BTreeMap<String, serde_json::Value>,
    pub normalized_time_millis: u64,
    pub feeds: SceneFeeds,
    pub policy: ScenePolicy,
    pub resources: SceneResourceEstimate,
}

/// Capability inference and feed compatibility are independent of CFG metadata.
pub fn validate_scene_capabilities(
    scene: &Scene,
    manifest: &Manifest,
    feeds: &SceneFeeds,
    policy: &ScenePolicy,
) -> Result<SceneCapabilityPlan, SceneSemanticError> {
    let (required, optional) = capability_sets(manifest)?;
    let inferred = inferred_capabilities(scene);
    for capability in &inferred {
        if !required.contains(capability) && !optional.contains(capability) {
            return Err(SceneSemanticError::InvalidCapability {
                capability: *capability,
                reason: "node capability is absent from both manifest declarations",
            });
        }
    }

    let mut feed_decisions = BTreeMap::new();
    for capability in &inferred {
        let status = feeds.status(*capability);
        validate_feed_status(*capability, &status)?;
        feed_decisions.insert(
            *capability,
            feed_decision(status, policy.observation_disabled),
        );
    }
    let has_click_ripple = scene
        .nodes
        .iter()
        .any(|node| matches!(node, SceneNode::ClickRipple { .. }));
    if has_click_ripple {
        let position_status = feeds.status(Capability::PointerPosition);
        validate_feed_status(Capability::PointerPosition, &position_status)?;
        feed_decisions
            .entry(Capability::PointerPosition)
            .or_insert_with(|| feed_decision(position_status, policy.observation_disabled));
    }

    let mut pass_through_nodes = Vec::new();
    let mut suppressed_nodes = Vec::new();
    for node in &scene.nodes {
        if policy.reduced_motion && motion_sensitive(node) {
            suppressed_nodes.push(node_id(node).to_owned());
        }
        let Some(capability) = descriptor_for_node(node).capability else {
            continue;
        };
        let position_status = if matches!(node, SceneNode::ClickRipple { .. }) {
            Some(feed_decisions[&Capability::PointerPosition].status.clone())
        } else {
            None
        };
        let position_available = !matches!(node, SceneNode::ClickRipple { .. })
            || feed_decisions[&Capability::PointerPosition].effective_available;
        let position_blocked_by_policy = matches!(node, SceneNode::ClickRipple { .. })
            && feed_decisions[&Capability::PointerPosition].blocked_by_policy;
        let decision = feed_decisions
            .get(&capability)
            .expect("inferred capability has a feed decision");
        let effective_available = decision.effective_available && position_available;
        if !effective_available {
            if required.contains(&capability) {
                return Err(SceneSemanticError::MissingRequiredCapability {
                    capability,
                    status: decision.status.clone(),
                    position_status,
                    blocked_by_policy: decision.blocked_by_policy || position_blocked_by_policy,
                });
            }
            pass_through_nodes.push(node_id(node).to_owned());
        }
    }
    Ok(SceneCapabilityPlan {
        required,
        optional,
        pass_through_nodes,
        suppressed_nodes,
        feed_decisions,
    })
}

/// Estimates execution buffers and core work without allocating execution buffers.
pub fn estimate_scene_resources(
    scene: &Scene,
    inputs: &SceneResourceInputs,
    effective_settings: &BTreeMap<String, serde_json::Value>,
    normalized_time_millis: u64,
) -> Result<SceneResourceEstimate, SceneSemanticError> {
    estimate_resources(scene, inputs, effective_settings, normalized_time_millis)
}

/// Capability declarations and deterministic feed decisions without host side effects.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneCapabilityPlan {
    pub required: BTreeSet<Capability>,
    pub optional: BTreeSet<Capability>,
    pub pass_through_nodes: Vec<String>,
    pub suppressed_nodes: Vec<String>,
    pub feed_decisions: BTreeMap<Capability, SceneFeedDecision>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneSemanticError {
    InvalidGraph {
        location: String,
        reason: &'static str,
    },
    InvalidCapability {
        capability: Capability,
        reason: &'static str,
    },
    MissingRequiredCapability {
        capability: Capability,
        status: SceneFeedStatus,
        position_status: Option<SceneFeedStatus>,
        blocked_by_policy: bool,
    },
    InvalidControl {
        location: String,
        reason: &'static str,
    },
    SettingsRegistry(RegistryComparisonError),
    MissingEffectiveSetting(String),
    InvalidEffectiveSetting(String),
    MissingSourceEstimate(String),
    ResourceOverflow(&'static str),
    ResourceLimitExceeded(&'static str),
}

impl fmt::Display for SceneSemanticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGraph { location, reason } => write!(formatter, "{reason} at {location}"),
            Self::InvalidCapability { capability, reason } => {
                write!(formatter, "{reason}: {capability:?}")
            }
            Self::MissingRequiredCapability {
                capability,
                status,
                position_status,
                blocked_by_policy,
            } => write!(
                formatter,
                "required capability {capability:?} is not effective: state={:?}, usable={}, reason={:?}, click_position={position_status:?}, blocked_by_policy={blocked_by_policy}",
                status.state, status.usable, status.reason
            ),
            Self::InvalidControl { location, reason } => {
                write!(formatter, "{reason} at {location}")
            }
            Self::SettingsRegistry(error) => write!(
                formatter,
                "portable settings registry is incomplete or inconsistent: {error:?}"
            ),
            Self::MissingEffectiveSetting(key) => {
                write!(formatter, "effective setting is unavailable: {key}")
            }
            Self::InvalidEffectiveSetting(key) => {
                write!(formatter, "effective setting is invalid: {key}")
            }
            Self::MissingSourceEstimate(id) => write!(
                formatter,
                "source resource estimate is unavailable for node {id}"
            ),
            Self::ResourceOverflow(resource) => {
                write!(formatter, "resource estimate overflowed: {resource}")
            }
            Self::ResourceLimitExceeded(limit) => write!(
                formatter,
                "scene exceeds or lacks required resource limit: {limit}"
            ),
        }
    }
}

impl std::error::Error for SceneSemanticError {}

/// Validates graph, manifest declarations, portable settings, and costs before execution.
pub fn validate_scene_semantics(
    scene: &Scene,
    manifest: &Manifest,
    registry: &Registry,
    inputs: &SceneResourceInputs,
    normalized_time_millis: u64,
    feeds: SceneFeeds,
    policy: ScenePolicy,
) -> Result<SceneValidationPlan, SceneSemanticError> {
    let topological_order = validate_graph(scene)?;
    validate_scene_source_refs(scene, manifest)?;
    let capability_plan = validate_scene_capabilities(scene, manifest, &feeds, &policy)?;
    // Global comparison deliberately exposes CFG metadata drift rather than creating
    // package-owned defaults or treating a partial registry as authoritative.
    compare_portable_settings(registry).map_err(SceneSemanticError::SettingsRegistry)?;
    let effective_settings = effective_settings(manifest, registry)?;
    validate_controls(scene, registry)?;
    let resources = estimate_resources(scene, inputs, &effective_settings, normalized_time_millis)?;
    Ok(SceneValidationPlan {
        registry_version: crate::SCENE_REGISTRY_VERSION,
        topological_order,
        required_capabilities: capability_plan.required,
        optional_capabilities: capability_plan.optional,
        pass_through_nodes: capability_plan.pass_through_nodes,
        feed_decisions: capability_plan.feed_decisions,
        suppressed_nodes: capability_plan.suppressed_nodes,
        activation_allowed: !policy.locked,
        seed: scene.seed,
        effective_settings,
        normalized_time_millis,
        feeds,
        policy,
        resources,
    })
}
/// Ensures each local media source resolves to a declared manifest asset.
pub fn validate_scene_source_refs(
    scene: &Scene,
    manifest: &Manifest,
) -> Result<(), SceneSemanticError> {
    for (index, node) in scene.nodes.iter().enumerate() {
        let path = match node {
            SceneNode::ImageSource { params, .. } | SceneNode::VideoSource { params, .. } => {
                Some(params.path.as_str())
            }
            _ => None,
        };
        match path {
            Some(path) if !manifest.assets.iter().any(|asset| asset.path == path) => {
                return Err(SceneSemanticError::InvalidGraph {
                    location: format!("$.nodes[{index}].params.path"),
                    reason: "source path does not resolve to a declared package asset",
                });
            }
            _ => {}
        }
    }
    Ok(())
}
pub fn validate_graph(scene: &Scene) -> Result<Vec<String>, SceneSemanticError> {
    if scene.nodes.len() > MAX_SCENE_NODES || scene.edges.len() > MAX_SCENE_EDGES {
        return Err(graph_error(
            "$",
            "node or edge count exceeds the v1 graph bound",
        ));
    }
    let mut nodes = BTreeMap::new();
    for node in &scene.nodes {
        let id = node_id(node);
        if nodes.insert(id, node).is_some() {
            return Err(graph_error("$.nodes", "node IDs must be unique"));
        }
    }
    if !nodes.contains_key(scene.output.as_str()) {
        return Err(graph_error(
            "$.output",
            "selected output does not name a node",
        ));
    }

    let mut incoming: BTreeMap<&str, Vec<(u8, &str)>> = BTreeMap::new();
    let mut outgoing: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (index, edge) in scene.edges.iter().enumerate() {
        let location = format!("$.edges[{index}]");
        if !nodes.contains_key(edge.from.as_str()) {
            return Err(graph_error(&location, "edge source does not name a node"));
        }
        let Some(destination) = nodes.get(edge.to.as_str()) else {
            return Err(graph_error(
                &location,
                "edge destination does not name a node",
            ));
        };
        if edge.from == edge.to {
            return Err(graph_error(&location, "self-cycles are not valid"));
        }
        let descriptor = descriptor_for_node(destination);
        let valid_slot = match descriptor.arity {
            SceneArity::Source => false,
            SceneArity::Unary => edge.slot == 0,
            SceneArity::Composite => edge.slot <= 1,
        };
        if !valid_slot {
            return Err(graph_error(
                &location,
                "edge slot is invalid for destination arity",
            ));
        }
        let inputs = incoming.entry(edge.to.as_str()).or_default();
        if inputs.iter().any(|(slot, _)| *slot == edge.slot) {
            return Err(graph_error(&location, "duplicate destination input slot"));
        }
        inputs.push((edge.slot, edge.from.as_str()));
        outgoing
            .entry(edge.from.as_str())
            .or_default()
            .push(edge.to.as_str());
    }
    for (id, node) in &nodes {
        let mut slots = incoming.get(id).map_or_else(Vec::new, |edges| {
            edges.iter().map(|(slot, _)| *slot).collect::<Vec<_>>()
        });
        slots.sort_unstable();
        let valid = match descriptor_for_node(node).arity {
            SceneArity::Source => slots.is_empty(),
            SceneArity::Unary => slots.as_slice() == [0],
            SceneArity::Composite => slots.as_slice() == [0, 1],
        };
        if !valid {
            return Err(graph_error(
                "$.edges",
                "node input count does not match its registered arity",
            ));
        }
    }

    let mut reachable = BTreeSet::new();
    let mut stack = vec![scene.output.as_str()];
    while let Some(id) = stack.pop() {
        if reachable.insert(id) {
            for (_, source) in incoming.get(id).into_iter().flatten() {
                stack.push(source);
            }
        }
    }
    if reachable.len() != nodes.len() {
        return Err(graph_error(
            "$.nodes",
            "every node must contribute to the selected output",
        ));
    }

    let mut indegree: BTreeMap<&str, usize> = nodes.keys().map(|id| (*id, 0)).collect();
    for edge in &scene.edges {
        *indegree
            .get_mut(edge.to.as_str())
            .expect("validated endpoint") += 1;
    }
    let mut ready: BTreeSet<&str> = indegree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
        .collect();
    let mut ordered = Vec::with_capacity(nodes.len());
    while let Some(id) = ready.pop_first() {
        ordered.push(id.to_owned());
        if let Some(destinations) = outgoing.get(id) {
            for destination in destinations {
                let degree = indegree.get_mut(destination).expect("validated endpoint");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(destination);
                }
            }
        }
    }
    if ordered.len() != nodes.len() {
        return Err(graph_error("$.edges", "scene graph contains a cycle"));
    }
    Ok(ordered)
}

fn capability_sets(
    manifest: &Manifest,
) -> Result<(BTreeSet<Capability>, BTreeSet<Capability>), SceneSemanticError> {
    let required: BTreeSet<_> = manifest.capabilities.required.iter().copied().collect();
    let optional: BTreeSet<_> = manifest.capabilities.optional.iter().copied().collect();
    if required.len() != manifest.capabilities.required.len()
        || optional.len() != manifest.capabilities.optional.len()
        || required
            .iter()
            .any(|capability| optional.contains(capability))
    {
        return Err(SceneSemanticError::InvalidGraph {
            location: "$.capabilities".into(),
            reason: "required and optional capability declarations must be unique and disjoint",
        });
    }
    Ok((required, optional))
}

fn inferred_capabilities(scene: &Scene) -> BTreeSet<Capability> {
    scene
        .nodes
        .iter()
        .filter_map(|node| descriptor_for_node(node).capability)
        .collect()
}

fn validate_feed_status(
    capability: Capability,
    status: &SceneFeedStatus,
) -> Result<(), SceneSemanticError> {
    if status.usable && status.state != SceneFeedState::Supported {
        return Err(SceneSemanticError::InvalidCapability {
            capability,
            reason: "only supported feeds may be marked usable",
        });
    }
    if status.reason.code.trim().is_empty() {
        return Err(SceneSemanticError::InvalidCapability {
            capability,
            reason: "feed status requires a non-empty reason code",
        });
    }
    Ok(())
}

fn feed_available(status: &SceneFeedStatus) -> bool {
    status.state == SceneFeedState::Supported && status.usable
}

fn feed_decision(status: SceneFeedStatus, observation_disabled: bool) -> SceneFeedDecision {
    let effective_available = feed_available(&status) && !observation_disabled;
    SceneFeedDecision {
        status,
        effective_available,
        blocked_by_policy: observation_disabled,
    }
}

fn validate_controls(scene: &Scene, registry: &Registry) -> Result<(), SceneSemanticError> {
    let mut ids = BTreeSet::new();
    for (index, control) in scene.controls.iter().enumerate() {
        let location = format!("$.controls[{index}]");
        if !ids.insert(control.id.as_str()) {
            return Err(SceneSemanticError::InvalidControl {
                location,
                reason: "control IDs must be unique",
            });
        }
        if ["permission", "grant", "consent", "sensor", "authorize"]
            .iter()
            .any(|term| contains_ascii_case_insensitive(&control.label, term))
        {
            return Err(SceneSemanticError::InvalidControl {
                location,
                reason: "control labels must not imply permissions or sensor access",
            });
        }
        let descriptor = registry.get_str(&control.setting).ok_or_else(|| {
            SceneSemanticError::InvalidControl {
                location: location.clone(),
                reason: "control setting has no CFG descriptor",
            }
        })?;
        if descriptor.scope() != SettingScope::P {
            return Err(SceneSemanticError::InvalidControl {
                location,
                reason: "control setting is not portable",
            });
        }
    }
    Ok(())
}

fn effective_settings(
    manifest: &Manifest,
    registry: &Registry,
) -> Result<BTreeMap<String, serde_json::Value>, SceneSemanticError> {
    let mut values = BTreeMap::new();
    for descriptor in registry
        .descriptors()
        .filter(|item| item.scope() == SettingScope::P)
    {
        let key = descriptor.key().to_string();
        let json = match manifest.settings.get(&key) {
            Some(value) => value.clone(),
            None => setting_value_json(descriptor.default()),
        };
        if !value_is_valid(&json, descriptor.value_type()) {
            return Err(SceneSemanticError::InvalidEffectiveSetting(key));
        }
        values.insert(key, json);
    }
    for key in manifest.settings.keys() {
        if !values.contains_key(key) {
            return Err(SceneSemanticError::MissingEffectiveSetting(key.clone()));
        }
    }
    Ok(values)
}

fn setting_value_json(value: &SettingValue) -> serde_json::Value {
    match value {
        SettingValue::Boolean(value) => (*value).into(),
        SettingValue::Integer(value) => (*value).into(),
        SettingValue::Number(value) => serde_json::Number::from_f64(*value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        SettingValue::Text(value) => value.as_ref().into(),
    }
}

fn value_is_valid(value: &serde_json::Value, setting_type: &SettingType) -> bool {
    match (setting_type, value) {
        (SettingType::Boolean, serde_json::Value::Bool(_)) => true,
        (SettingType::Integer { min, max }, serde_json::Value::Number(number)) => {
            number.as_i64().is_some_and(|n| min <= &n && &n <= max)
        }
        (SettingType::Number { min, max }, serde_json::Value::Number(number)) => number
            .as_f64()
            .is_some_and(|n| n.is_finite() && min <= &n && &n <= max),
        (SettingType::String { max_chars }, serde_json::Value::String(text)) => {
            text.chars().count() <= *max_chars
        }
        (SettingType::Enum { values }, serde_json::Value::String(text)) => {
            values.iter().any(|candidate| candidate.as_ref() == text)
        }
        _ => false,
    }
}

fn estimate_resources(
    scene: &Scene,
    inputs: &SceneResourceInputs,
    settings: &BTreeMap<String, serde_json::Value>,
    normalized_time_millis: u64,
) -> Result<SceneResourceEstimate, SceneSemanticError> {
    if inputs.columns == 0 || inputs.rows == 0 {
        return Err(SceneSemanticError::ResourceLimitExceeded(
            "nonzero dimensions are required for preflight",
        ));
    }
    let simulation_hz = settings
        .get("scene.simulation-hz")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| SceneSemanticError::MissingEffectiveSetting("scene.simulation-hz".into()))?;
    if simulation_hz == 0 {
        return Err(SceneSemanticError::InvalidEffectiveSetting(
            "scene.simulation-hz".into(),
        ));
    }
    let ticks = normalized_time_millis
        .checked_mul(simulation_hz)
        .and_then(|scaled| scaled.checked_add(999))
        .ok_or(SceneSemanticError::ResourceOverflow(
            "simulation time estimate",
        ))?
        .checked_div(1000)
        .ok_or(SceneSemanticError::ResourceOverflow(
            "simulation time estimate",
        ))?
        .max(1);
    let cells = u64::from(inputs.columns)
        .checked_mul(u64::from(inputs.rows))
        .ok_or(SceneSemanticError::ResourceOverflow("cell count"))?;
    let cell_bytes = u64::try_from(std::mem::size_of::<AsciiCell>())
        .map_err(|_| SceneSemanticError::ResourceOverflow("cell size"))?;
    let frame_bytes = cells
        .checked_mul(cell_bytes)
        .ok_or(SceneSemanticError::ResourceOverflow("frame memory"))?;
    if frame_bytes > inputs.limits.core.max_frame_bytes() {
        return Err(SceneSemanticError::ResourceLimitExceeded(
            "COR-001 frame-byte budget",
        ));
    }

    let mut particles = 0_u64;
    let mut decoders = 0_u64;
    let mut source_bytes = 0_u64;
    let mut required_sources = BTreeSet::new();
    for node in &scene.nodes {
        match node {
            SceneNode::Starfield { params, .. } => {
                particles = particles
                    .checked_add(u64::from(params.count))
                    .ok_or(SceneSemanticError::ResourceOverflow("particles"))?
            }
            SceneNode::MatrixRain { params, .. } => {
                let count = ceil_scaled(cells, params.density)?;
                particles = particles
                    .checked_add(count)
                    .ok_or(SceneSemanticError::ResourceOverflow("particles"))?;
            }
            SceneNode::ImageSource { id, .. } | SceneNode::VideoSource { id, .. } => {
                let source = inputs
                    .sources
                    .get(id)
                    .ok_or_else(|| SceneSemanticError::MissingSourceEstimate(id.clone()))?;
                if source.decoded_pixels > MAX_DECODED_IMAGE_PIXELS
                    || source.decoded_bytes > MAX_DECODED_IMAGE_BYTES
                    || source.decoded_bytes > inputs.limits.core.max_raster_bytes()
                {
                    return Err(SceneSemanticError::ResourceLimitExceeded(
                        "package decoded-image or COR-001 raster-byte budget",
                    ));
                }
                decoders = decoders
                    .checked_add(1)
                    .ok_or(SceneSemanticError::ResourceOverflow("decoder count"))?;
                source_bytes = source_bytes
                    .checked_add(source.decoded_bytes)
                    .ok_or(SceneSemanticError::ResourceOverflow("source memory"))?;
                required_sources.insert(id.as_str());
            }
            _ => {}
        }
    }
    if inputs
        .sources
        .keys()
        .any(|id| !required_sources.contains(id.as_str()))
    {
        return Err(SceneSemanticError::InvalidGraph {
            location: "$.sources".into(),
            reason: "source estimates must correspond to actual image/video nodes",
        });
    }
    if particles > inputs.limits.max_particles {
        return Err(SceneSemanticError::ResourceLimitExceeded(
            "aggregate particle budget",
        ));
    }
    if decoders > inputs.limits.max_decoders {
        return Err(SceneSemanticError::ResourceLimitExceeded(
            "aggregate decoder budget",
        ));
    }
    if let Some(max_particles) = settings
        .get("scene.max-particles")
        .and_then(serde_json::Value::as_u64)
    {
        if particles > max_particles {
            return Err(SceneSemanticError::ResourceLimitExceeded(
                "effective scene.max-particles setting",
            ));
        }
    } else if settings.contains_key("scene.max-particles") {
        return Err(SceneSemanticError::InvalidEffectiveSetting(
            "scene.max-particles".into(),
        ));
    }

    let graph_frame_bytes = frame_bytes.checked_mul(scene.nodes.len() as u64).ok_or(
        SceneSemanticError::ResourceOverflow("aggregate frame memory"),
    )?;
    let aggregate_memory_bytes = graph_frame_bytes
        .checked_add(source_bytes)
        .ok_or(SceneSemanticError::ResourceOverflow("aggregate memory"))?;
    if aggregate_memory_bytes > inputs.limits.max_aggregate_memory_bytes {
        return Err(SceneSemanticError::ResourceLimitExceeded(
            "aggregate memory budget",
        ));
    }
    let work_units = cells
        .checked_mul(scene.nodes.len() as u64)
        .and_then(|work| work.checked_mul(ticks))
        .ok_or(SceneSemanticError::ResourceOverflow("time/work estimate"))?;
    if work_units > inputs.limits.core.max_work_units() {
        return Err(SceneSemanticError::ResourceLimitExceeded(
            "COR-001 work/time budget",
        ));
    }
    Ok(SceneResourceEstimate {
        cells,
        particles,
        decoders,
        aggregate_memory_bytes,
        work_units,
    })
}

/// Applies the schema-bounded binary `f64` density with upward rounding so the
/// estimate cannot undercount a fractional particle.
fn ceil_scaled(count: u64, fraction: f64) -> Result<u64, SceneSemanticError> {
    if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
        return Err(SceneSemanticError::ResourceOverflow("particle density"));
    }
    let bits = fraction.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as u32;
    let fraction_bits = bits & ((1_u64 << 52) - 1);
    let (mantissa, shift) = if exponent == 0 {
        (fraction_bits, 1074_u32)
    } else {
        ((1_u64 << 52) | fraction_bits, 1075 - exponent)
    };
    let product = u128::from(count) * u128::from(mantissa);
    if product == 0 {
        return Ok(0);
    }
    let rounded = if shift >= 128 {
        1
    } else {
        let denominator_mask = (1_u128 << shift) - 1;
        product
            .checked_add(denominator_mask)
            .ok_or(SceneSemanticError::ResourceOverflow("particle rounding"))?
            >> shift
    };
    u64::try_from(rounded).map_err(|_| SceneSemanticError::ResourceOverflow("particles"))
}

fn motion_sensitive(node: &SceneNode) -> bool {
    matches!(
        node,
        SceneNode::MatrixRain { .. }
            | SceneNode::Starfield { .. }
            | SceneNode::Waves { .. }
            | SceneNode::NoiseField { .. }
            | SceneNode::CubeWireframe { .. }
            | SceneNode::PointerForce { .. }
            | SceneNode::ClickRipple { .. }
            | SceneNode::WindowAvoid { .. }
            | SceneNode::Trails { .. }
    )
}
fn node_id(node: &SceneNode) -> &str {
    match node {
        SceneNode::Solid { id, .. }
        | SceneNode::MatrixRain { id, .. }
        | SceneNode::Starfield { id, .. }
        | SceneNode::Waves { id, .. }
        | SceneNode::NoiseField { id, .. }
        | SceneNode::CubeWireframe { id, .. }
        | SceneNode::ImageSource { id, .. }
        | SceneNode::VideoSource { id, .. }
        | SceneNode::GlyphColor { id, .. }
        | SceneNode::PointerForce { id, .. }
        | SceneNode::ClickRipple { id, .. }
        | SceneNode::WindowAvoid { id, .. }
        | SceneNode::Trails { id, .. }
        | SceneNode::Composite { id, .. } => id,
    }
}

fn graph_error(location: &str, reason: &'static str) -> SceneSemanticError {
    SceneSemanticError::InvalidGraph {
        location: location.into(),
        reason,
    }
}

fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}
