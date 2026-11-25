//! Versioned, bounded capability snapshots shared by native host adapters.

use ascii_core::MonitorIdentity;

/// Current schema version for platform capability snapshots.
pub const CAPABILITY_CONTRACT_VERSION: ContractVersion = ContractVersion { major: 1, minor: 0 };
/// Maximum records in one capability snapshot, including output-scoped records.
pub const MAX_CAPABILITY_RECORDS: usize = 32;

/// Version of the portable capability/event schema, independent of an adapter's version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContractVersion {
    /// Incompatible schema changes increment the major version.
    pub major: u16,
    /// Additive schema changes increment the minor version.
    pub minor: u16,
}

/// Version of a concrete host adapter integration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdapterVersion {
    /// Adapter major version.
    pub major: u16,
    /// Adapter minor version.
    pub minor: u16,
    /// Adapter patch version.
    pub patch: u16,
}

/// Stable adapter label plus a process-local host instance identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostIdentity {
    adapter_id: &'static str,
    adapter_version: AdapterVersion,
    instance_id: u64,
}

impl HostIdentity {
    /// Creates a host identity. Empty adapter names and the reserved zero instance are rejected.
    #[must_use]
    pub const fn new(
        adapter_id: &'static str,
        adapter_version: AdapterVersion,
        instance_id: u64,
    ) -> Option<Self> {
        if adapter_id.is_empty() || instance_id == 0 {
            None
        } else {
            Some(Self {
                adapter_id,
                adapter_version,
                instance_id,
            })
        }
    }

    /// Stable integration label, not a display name or native handle.
    #[must_use]
    pub const fn adapter_id(self) -> &'static str {
        self.adapter_id
    }

    /// Version reported by the integration.
    #[must_use]
    pub const fn adapter_version(self) -> AdapterVersion {
        self.adapter_version
    }

    /// Process-local identity for this adapter instance.
    #[must_use]
    pub const fn instance_id(self) -> u64 {
        self.instance_id
    }
}

/// Host implementation health, independent of individual feed availability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostHealth {
    /// No health observation has been made yet.
    Unknown,
    /// Native startup or attachment is in progress.
    Starting,
    /// The host is operating normally.
    Healthy,
    /// Some optional integration paths are degraded.
    Degraded,
    /// The host is alive but cannot currently present or observe feeds.
    Unavailable,
    /// The host hit an adapter fault; the caller must use a safe downgrade.
    Faulted,
}

/// Privacy or integration restrictions reported by a host, as a fixed-size bit set.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HostRestrictions(u16);

impl HostRestrictions {
    /// Global pointer observation is not exposed by this integration/session.
    pub const GLOBAL_POINTER_UNAVAILABLE: u16 = 1 << 0;
    /// Window geometry is not exposed; state-only observations may still work.
    pub const WINDOW_GEOMETRY_UNAVAILABLE: u16 = 1 << 1;
    /// A shell or toolkit owns presentation rather than a native wgpu surface.
    pub const SHELL_OWNS_PRESENTATION: u16 = 1 << 2;
    /// Only the operating system's static wallpaper path is available.
    pub const STATIC_WALLPAPER_ONLY: u16 = 1 << 3;
    /// No native graphics surface is provided by the host.
    pub const NO_NATIVE_SURFACE: u16 = 1 << 4;
    /// The integration requires separate user consent for its optional behavior.
    pub const USER_CONSENT_REQUIRED: u16 = 1 << 5;
    const KNOWN_BITS: u16 = (1 << 6) - 1;

    /// Creates an empty restriction set.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Creates a restriction set if no unknown bits are set.
    #[must_use]
    pub const fn from_bits(bits: u16) -> Option<Self> {
        if bits & !Self::KNOWN_BITS == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    /// Returns whether the supplied restriction is present.
    #[must_use]
    pub const fn contains(self, restriction: u16) -> bool {
        self.0 & restriction != 0
    }

    /// Returns the encoded, known restriction bits.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Adds a known restriction without allocation.
    #[must_use]
    pub const fn with(self, restriction: u16) -> Option<Self> {
        if restriction == 0 || restriction & !Self::KNOWN_BITS != 0 {
            None
        } else {
            Some(Self(self.0 | restriction))
        }
    }
}

/// Independently reported platform feature. Geometry and state remain separate feeds.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum CapabilityFeature {
    /// A native wallpaper/presentation surface can be leased.
    WallpaperSurface,
    /// Global pointer coordinates can be observed passively.
    PointerPosition,
    /// Passive pointer button state can be observed.
    PointerButtons,
    /// Window rectangles are available; this does not imply titles or process data.
    WindowRectangles,
    /// Coarse window state is available without requiring geometry.
    WindowState,
    /// Lock/unlock state is available for the current session.
    LockState,
    /// Battery/power state is available.
    BatteryState,
    /// Output scale metadata is available, independently per display.
    OutputScale,
    /// The OS static-wallpaper setting path is available.
    StaticWallpaper,
    /// A tray integration is available.
    Tray,
}

impl CapabilityFeature {
    /// All feature keys in stable schema order.
    pub const ALL: [Self; 10] = [
        Self::WallpaperSurface,
        Self::PointerPosition,
        Self::PointerButtons,
        Self::WindowRectangles,
        Self::WindowState,
        Self::LockState,
        Self::BatteryState,
        Self::OutputScale,
        Self::StaticWallpaper,
        Self::Tray,
    ];

    const fn index(self) -> u8 {
        self as u8
    }

    const fn default_scope(self) -> CapabilityScope {
        match self {
            Self::PointerPosition
            | Self::PointerButtons
            | Self::WindowRectangles
            | Self::WindowState => CapabilityScope::Desktop,
            Self::LockState => CapabilityScope::Session,
            Self::OutputScale => CapabilityScope::AllOutputs,
            Self::WallpaperSurface | Self::BatteryState | Self::StaticWallpaper | Self::Tray => {
                CapabilityScope::Host
            }
        }
    }
}

/// Feature dependency bits for the bounded effective-capability lattice.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CapabilitySet(u16);

impl CapabilitySet {
    /// Creates an empty dependency set.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Creates a set containing one feature.
    #[must_use]
    pub const fn one(feature: CapabilityFeature) -> Self {
        Self(1 << feature.index())
    }

    /// Adds one feature to the set.
    #[must_use]
    pub const fn with(self, feature: CapabilityFeature) -> Self {
        Self(self.0 | (1 << feature.index()))
    }

    /// Returns whether this set contains a feature.
    #[must_use]
    pub const fn contains(self, feature: CapabilityFeature) -> bool {
        self.0 & (1 << feature.index()) != 0
    }

    /// Iterates the fixed feature table without allocation.
    pub fn iter(self) -> impl Iterator<Item = CapabilityFeature> {
        CapabilityFeature::ALL
            .into_iter()
            .filter(move |feature| self.contains(*feature))
    }
}

/// Scope of one capability observation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CapabilityScope {
    /// The host cannot yet identify the scope.
    Unknown,
    /// The adapter process or integration as a whole.
    Host,
    /// The active user session.
    Session,
    /// The desktop across outputs.
    Desktop,
    /// A default summary covering all outputs; output records may refine it.
    AllOutputs,
    /// One local display identity from `ascii-core`.
    Output(MonitorIdentity),
}

/// Availability is not a boolean: unknown, denied, unsupported, and empty remain distinct.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityAvailability {
    /// The adapter has not established this feature's state.
    Unknown,
    /// The feature is available within the record's scope.
    Available,
    /// Permission is required before use can be considered.
    PermissionRequired,
    /// Permission was denied or revoked.
    Denied,
    /// The host/session does not implement this behavior.
    Unsupported,
    /// The feed is unavailable for a non-permission reason.
    Unavailable,
    /// The feature is expected to recover without changing the adapter.
    TemporarilyUnavailable,
    /// The adapter reported a fault for this feature.
    Faulted,
}

/// Stable machine-readable reason for an unavailable or unobserved capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityReason {
    /// No probe has reported this feature.
    NotObserved,
    /// Explicit permission is required before the feed can be used.
    PermissionRequired,
    /// The user or policy denied access.
    PermissionDenied,
    /// Permission was previously available but has been revoked.
    PermissionRevoked,
    /// The adapter or current host does not implement the feature.
    UnsupportedByHost,
    /// The feed is absent or disconnected.
    FeedUnavailable,
    /// The current session cannot expose the feature.
    SessionUnavailable,
    /// The requested output is absent or not uniquely identified.
    OutputUnavailable,
    /// A transient native condition prevents use.
    TemporaryFailure,
    /// The adapter failed while querying or operating this feature.
    AdapterFault,
    /// A declared prerequisite is not effective.
    DependencyUnavailable,
    /// A dependency cycle was encountered and was failed closed.
    DependencyCycle,
    /// The last observation is beyond its advertised age limit.
    StaleData,
    /// The observation timestamp is later than the consumer's monotonic clock.
    ClockSkew,
}

/// Freshness bound for a host observation, using a caller-provided monotonic millisecond clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Freshness {
    observed_at_ms: Option<u64>,
    maximum_age_ms: u32,
}

impl Freshness {
    /// Freshness is unknown until the host provides an observation timestamp.
    pub const UNKNOWN: Self = Self {
        observed_at_ms: None,
        maximum_age_ms: 0,
    };

    /// Creates freshness metadata for one observation and its maximum permitted age.
    #[must_use]
    pub const fn observed_at(observed_at_ms: u64, maximum_age_ms: u32) -> Self {
        Self {
            observed_at_ms: Some(observed_at_ms),
            maximum_age_ms,
        }
    }

    /// Returns the monotonic observation time, or `None` when unknown.
    #[must_use]
    pub const fn observed_at_ms(self) -> Option<u64> {
        self.observed_at_ms
    }

    /// Returns the maximum age in milliseconds.
    #[must_use]
    pub const fn maximum_age_ms(self) -> u32 {
        self.maximum_age_ms
    }

    /// Classifies this observation at a caller-supplied monotonic time.
    #[must_use]
    pub const fn state_at(self, now_ms: u64) -> FreshnessState {
        match self.observed_at_ms {
            core::option::Option::None => FreshnessState::Unknown,
            core::option::Option::Some(observed_at_ms) if observed_at_ms > now_ms => {
                FreshnessState::ClockSkew
            }
            core::option::Option::Some(observed_at_ms)
                if now_ms - observed_at_ms > self.maximum_age_ms as u64 =>
            {
                FreshnessState::Stale
            }
            core::option::Option::Some(_) => FreshnessState::Fresh,
        }
    }
}

/// Freshness classification of one capability observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FreshnessState {
    /// The host has not supplied a timestamp.
    Unknown,
    /// The observation is within its declared age limit.
    Fresh,
    /// The observation exceeded its declared age limit.
    Stale,
    /// The consumer clock precedes the observation timestamp.
    ClockSkew,
}

/// Quality of the feed evidence, separate from its availability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Confidence {
    /// Evidence quality is not known.
    Unknown,
    /// Best-effort or coarse observation.
    Low,
    /// Useful but not authoritative observation.
    Medium,
    /// Direct, high-confidence observation.
    High,
}

/// One bounded capability record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityRecord {
    feature: CapabilityFeature,
    scope: CapabilityScope,
    availability: CapabilityAvailability,
    reason: Option<CapabilityReason>,
    freshness: Freshness,
    confidence: Confidence,
    dependencies: CapabilitySet,
}

impl CapabilityRecord {
    /// Creates an explicit unknown record; it does not imply an empty or zero-valued feed.
    #[must_use]
    pub const fn unknown(feature: CapabilityFeature, scope: CapabilityScope) -> Self {
        Self {
            feature,
            scope,
            availability: CapabilityAvailability::Unknown,
            reason: Some(CapabilityReason::NotObserved),
            freshness: Freshness::UNKNOWN,
            confidence: Confidence::Unknown,
            dependencies: CapabilitySet::empty(),
        }
    }

    /// Creates a reported record after checking state/reason consistency.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        feature: CapabilityFeature,
        scope: CapabilityScope,
        availability: CapabilityAvailability,
        reason: Option<CapabilityReason>,
        freshness: Freshness,
        confidence: Confidence,
        dependencies: CapabilitySet,
    ) -> Result<Self, CapabilityError> {
        if !reason_matches(availability, reason) {
            return Err(CapabilityError::InvalidReason);
        }
        if (scope == CapabilityScope::Unknown && availability != CapabilityAvailability::Unknown)
            || (feature == CapabilityFeature::OutputScale
                && !matches!(
                    scope,
                    CapabilityScope::AllOutputs | CapabilityScope::Output(_)
                ))
        {
            return Err(CapabilityError::InvalidScope);
        }
        if dependencies.contains(feature) {
            return Err(CapabilityError::SelfDependency);
        }
        Ok(Self {
            feature,
            scope,
            availability,
            reason,
            freshness,
            confidence,
            dependencies,
        })
    }

    /// Feature represented by this record.
    #[must_use]
    pub const fn feature(self) -> CapabilityFeature {
        self.feature
    }

    /// Scope represented by this record.
    #[must_use]
    pub const fn scope(self) -> CapabilityScope {
        self.scope
    }

    /// Last reported (not dependency-resolved) availability.
    #[must_use]
    pub const fn availability(self) -> CapabilityAvailability {
        self.availability
    }

    /// Explicit source reason, if any.
    #[must_use]
    pub const fn reason(self) -> Option<CapabilityReason> {
        self.reason
    }

    /// Observation age bound and timestamp.
    #[must_use]
    pub const fn freshness(self) -> Freshness {
        self.freshness
    }

    /// Confidence assigned by the adapter.
    #[must_use]
    pub const fn confidence(self) -> Confidence {
        self.confidence
    }

    /// Explicit feature prerequisites used to derive effective availability.
    #[must_use]
    pub const fn dependencies(self) -> CapabilitySet {
        self.dependencies
    }
}

/// Failure while constructing or updating a capability snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityError {
    /// A status and its reason code contradict each other.
    InvalidReason,
    /// The record's scope is unknown for an effective state or invalid for its feature.
    InvalidScope,
    /// A feature cannot depend on itself.
    SelfDependency,
    /// The fixed snapshot has no free record slot.
    CapacityExhausted,
    /// The revision counter cannot advance without wrapping.
    RevisionExhausted,
}

/// Dependency-resolved availability. A blocked feature is never reported as effective.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EffectiveCapability {
    availability: CapabilityAvailability,
    reason: Option<CapabilityReason>,
    blocked_by: Option<CapabilityFeature>,
}

impl EffectiveCapability {
    /// Resolved availability after dependency and freshness checks.
    #[must_use]
    pub const fn availability(self) -> CapabilityAvailability {
        self.availability
    }

    /// Resolved reason, including stale data and failed dependency gates.
    #[must_use]
    pub const fn reason(self) -> Option<CapabilityReason> {
        self.reason
    }

    /// The direct dependency that prevents use, if any.
    #[must_use]
    pub const fn blocked_by(self) -> Option<CapabilityFeature> {
        self.blocked_by
    }

    /// Returns whether this record is currently effective.
    #[must_use]
    pub fn is_available(self) -> bool {
        self.availability == CapabilityAvailability::Available
    }
}

/// One effective capability transition, including dependent-feature revocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityChange {
    feature: CapabilityFeature,
    scope: CapabilityScope,
    previous: EffectiveCapability,
    current: EffectiveCapability,
}

impl CapabilityChange {
    /// Feature whose effective state changed.
    #[must_use]
    pub const fn feature(self) -> CapabilityFeature {
        self.feature
    }

    /// Scope whose effective state changed.
    #[must_use]
    pub const fn scope(self) -> CapabilityScope {
        self.scope
    }

    /// Effective state before the update.
    #[must_use]
    pub const fn previous(self) -> EffectiveCapability {
        self.previous
    }

    /// Effective state after the update.
    #[must_use]
    pub const fn current(self) -> EffectiveCapability {
        self.current
    }
}

/// Fixed-capacity set of effective transitions produced by one snapshot update.
#[derive(Debug, Eq, PartialEq)]
pub struct CapabilityChangeSet {
    revision: u64,
    changes: [Option<CapabilityChange>; MAX_CAPABILITY_RECORDS],
    length: u8,
}

impl CapabilityChangeSet {
    const fn empty(revision: u64) -> Self {
        Self {
            revision,
            changes: [None; MAX_CAPABILITY_RECORDS],
            length: 0,
        }
    }

    fn push(&mut self, change: CapabilityChange) {
        if usize::from(self.length) < MAX_CAPABILITY_RECORDS {
            self.changes[usize::from(self.length)] = Some(change);
            self.length += 1;
        }
    }

    /// Snapshot revision associated with these changes.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Number of transitions in the set.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.length as usize
    }

    /// Whether no effective state changed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Iterates this set without allocation.
    pub fn iter(&self) -> impl Iterator<Item = &CapabilityChange> {
        self.changes[..usize::from(self.length)].iter().flatten()
    }
}

/// Bounded, versioned capability metadata and current adapter health.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilitySnapshot {
    contract_version: ContractVersion,
    host: HostIdentity,
    restrictions: HostRestrictions,
    health: HostHealth,
    revision: u64,
    records: [Option<CapabilityRecord>; MAX_CAPABILITY_RECORDS],
    length: u8,
}

impl CapabilitySnapshot {
    /// Creates a versioned snapshot whose ten baseline feeds are explicitly unknown.
    #[must_use]
    pub fn unknown(host: HostIdentity, restrictions: HostRestrictions) -> Self {
        let mut records = [None; MAX_CAPABILITY_RECORDS];
        let mut index = 0;
        while index < CapabilityFeature::ALL.len() {
            let feature = CapabilityFeature::ALL[index];
            records[index] = Some(CapabilityRecord::unknown(feature, feature.default_scope()));
            index += 1;
        }
        Self {
            contract_version: CAPABILITY_CONTRACT_VERSION,
            host,
            restrictions,
            health: HostHealth::Unknown,
            revision: 0,
            records,
            length: CapabilityFeature::ALL.len() as u8,
        }
    }

    /// Capability/event schema version.
    #[must_use]
    pub const fn contract_version(&self) -> ContractVersion {
        self.contract_version
    }

    /// Host and adapter identity reported by the implementation.
    #[must_use]
    pub const fn host(&self) -> HostIdentity {
        self.host
    }

    /// Restrictions the host reports independently of feed availability.
    #[must_use]
    pub const fn restrictions(&self) -> HostRestrictions {
        self.restrictions
    }

    /// Aggregate health; individual capability states remain authoritative for each feed.
    #[must_use]
    pub const fn health(&self) -> HostHealth {
        self.health
    }

    /// Monotonic snapshot revision, used to reconcile dropped events.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Number of records, including per-output refinements.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.length as usize
    }

    /// Whether this snapshot has no records (normally false because baseline feeds are present).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Iterates all present records in stable insertion order.
    pub fn records(&self) -> impl Iterator<Item = &CapabilityRecord> {
        self.records[..usize::from(self.length)].iter().flatten()
    }

    /// Finds the exact scoped record, falling back to the feature's baseline record.
    #[must_use]
    pub fn record(
        &self,
        feature: CapabilityFeature,
        scope: CapabilityScope,
    ) -> Option<&CapabilityRecord> {
        self.find_record(feature, scope)
            .or_else(|| self.find_record(feature, feature.default_scope()))
    }

    /// Resolves availability at `now_ms`; stale or clock-skewed observations fail closed.
    #[must_use]
    pub fn effective_at(
        &self,
        feature: CapabilityFeature,
        scope: CapabilityScope,
        now_ms: u64,
    ) -> EffectiveCapability {
        self.effective_inner(feature, scope, now_ms, CapabilitySet::empty(), true)
    }

    /// Replaces or adds one record and reports all direct and dependent effective transitions.
    pub fn replace(
        &mut self,
        record: CapabilityRecord,
    ) -> Result<CapabilityChangeSet, CapabilityError> {
        let existing_index = self
            .records()
            .position(|current| current.feature == record.feature && current.scope == record.scope);
        if let Some(index) = existing_index {
            if self.records[index] == Some(record) {
                return Ok(CapabilityChangeSet::empty(self.revision));
            }
        } else if usize::from(self.length) == MAX_CAPABILITY_RECORDS {
            return Err(CapabilityError::CapacityExhausted);
        }

        let revision = self
            .revision
            .checked_add(1)
            .ok_or(CapabilityError::RevisionExhausted)?;
        let mut candidates = [None; MAX_CAPABILITY_RECORDS];
        let mut candidate_count = 0;
        for current in self.records() {
            let feature = current.feature;
            let scope = current.scope;
            let previous = self.effective_status(feature, scope);
            candidates[candidate_count] = Some(CapabilityChange {
                feature,
                scope,
                previous,
                current: previous,
            });
            candidate_count += 1;
        }
        if existing_index.is_none() {
            let previous = self.effective_status(record.feature, record.scope);
            candidates[candidate_count] = Some(CapabilityChange {
                feature: record.feature,
                scope: record.scope,
                previous,
                current: previous,
            });
            candidate_count += 1;
        }

        if let Some(index) = existing_index {
            self.records[index] = Some(record);
        } else {
            self.records[usize::from(self.length)] = Some(record);
            self.length += 1;
        }
        self.revision = revision;

        let mut changes = CapabilityChangeSet::empty(revision);
        for candidate in candidates[..candidate_count].iter().flatten() {
            let current = self.effective_status(candidate.feature, candidate.scope);
            if candidate.previous != current {
                changes.push(CapabilityChange {
                    feature: candidate.feature,
                    scope: candidate.scope,
                    previous: candidate.previous,
                    current,
                });
            }
        }
        Ok(changes)
    }

    /// Changes host health and reports any effective capability transitions.
    pub fn set_health(
        &mut self,
        health: HostHealth,
    ) -> Result<CapabilityChangeSet, CapabilityError> {
        if self.health == health {
            return Ok(CapabilityChangeSet::empty(self.revision));
        }
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(CapabilityError::RevisionExhausted)?;
        let mut candidates = [None; MAX_CAPABILITY_RECORDS];
        let mut candidate_count = 0;
        for record in self.records() {
            let previous = self.effective_status(record.feature, record.scope);
            candidates[candidate_count] = Some(CapabilityChange {
                feature: record.feature,
                scope: record.scope,
                previous,
                current: previous,
            });
            candidate_count += 1;
        }
        self.health = health;
        self.revision = revision;

        let mut changes = CapabilityChangeSet::empty(revision);
        for candidate in candidates[..candidate_count].iter().flatten() {
            let current = self.effective_status(candidate.feature, candidate.scope);
            if candidate.previous != current {
                changes.push(CapabilityChange {
                    feature: candidate.feature,
                    scope: candidate.scope,
                    previous: candidate.previous,
                    current,
                });
            }
        }
        Ok(changes)
    }

    /// Advances the revision for a feed observation not represented by a capability transition.
    ///
    /// Adapters use this before queueing data events so an overflow reconciliation revision also
    /// covers the newest event payload.
    pub fn advance_revision(&mut self) -> Result<u64, CapabilityError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(CapabilityError::RevisionExhausted)?;
        Ok(self.revision)
    }

    fn find_record(
        &self,
        feature: CapabilityFeature,
        scope: CapabilityScope,
    ) -> Option<&CapabilityRecord> {
        self.records()
            .find(|record| record.feature == feature && record.scope == scope)
    }

    fn effective_status(
        &self,
        feature: CapabilityFeature,
        scope: CapabilityScope,
    ) -> EffectiveCapability {
        self.effective_inner(feature, scope, 0, CapabilitySet::empty(), false)
    }

    fn effective_inner(
        &self,
        feature: CapabilityFeature,
        scope: CapabilityScope,
        now_ms: u64,
        visited: CapabilitySet,
        check_freshness: bool,
    ) -> EffectiveCapability {
        if visited.contains(feature) {
            return effective(
                CapabilityAvailability::Unavailable,
                Some(CapabilityReason::DependencyCycle),
                Some(feature),
            );
        }
        if feature == CapabilityFeature::WallpaperSurface {
            match self.health {
                HostHealth::Unavailable | HostHealth::Starting => {
                    return effective(
                        CapabilityAvailability::TemporarilyUnavailable,
                        Some(CapabilityReason::TemporaryFailure),
                        None,
                    );
                }
                HostHealth::Faulted => {
                    return effective(
                        CapabilityAvailability::Faulted,
                        Some(CapabilityReason::AdapterFault),
                        None,
                    );
                }
                HostHealth::Unknown | HostHealth::Healthy | HostHealth::Degraded => {}
            }
        }
        let Some(record) = self.record(feature, scope) else {
            return effective(
                CapabilityAvailability::Unknown,
                Some(CapabilityReason::NotObserved),
                None,
            );
        };
        if record.availability != CapabilityAvailability::Available {
            return effective(record.availability, record.reason, None);
        }
        if check_freshness {
            match record.freshness.state_at(now_ms) {
                FreshnessState::Unknown | FreshnessState::Stale => {
                    return effective(
                        CapabilityAvailability::TemporarilyUnavailable,
                        Some(CapabilityReason::StaleData),
                        None,
                    );
                }
                FreshnessState::ClockSkew => {
                    return effective(
                        CapabilityAvailability::TemporarilyUnavailable,
                        Some(CapabilityReason::ClockSkew),
                        None,
                    );
                }
                FreshnessState::Fresh => {}
            }
        }

        let visited = visited.with(feature);
        for dependency in record.dependencies.iter() {
            let dependency_state =
                self.effective_inner(dependency, scope, now_ms, visited, check_freshness);
            if !dependency_state.is_available() {
                return effective(
                    CapabilityAvailability::Unavailable,
                    Some(CapabilityReason::DependencyUnavailable),
                    Some(dependency),
                );
            }
        }
        effective(CapabilityAvailability::Available, None, None)
    }
}

fn effective(
    availability: CapabilityAvailability,
    reason: Option<CapabilityReason>,
    blocked_by: Option<CapabilityFeature>,
) -> EffectiveCapability {
    EffectiveCapability {
        availability,
        reason,
        blocked_by,
    }
}

fn reason_matches(availability: CapabilityAvailability, reason: Option<CapabilityReason>) -> bool {
    match availability {
        CapabilityAvailability::Unknown => reason == Some(CapabilityReason::NotObserved),
        CapabilityAvailability::Available => reason.is_none(),
        CapabilityAvailability::PermissionRequired => {
            reason == Some(CapabilityReason::PermissionRequired)
        }
        CapabilityAvailability::Denied => matches!(
            reason,
            Some(CapabilityReason::PermissionDenied | CapabilityReason::PermissionRevoked)
        ),
        CapabilityAvailability::Unsupported => reason == Some(CapabilityReason::UnsupportedByHost),
        CapabilityAvailability::Unavailable => matches!(
            reason,
            Some(
                CapabilityReason::FeedUnavailable
                    | CapabilityReason::SessionUnavailable
                    | CapabilityReason::OutputUnavailable
                    | CapabilityReason::DependencyUnavailable
            )
        ),
        CapabilityAvailability::TemporarilyUnavailable => matches!(
            reason,
            Some(
                CapabilityReason::TemporaryFailure
                    | CapabilityReason::StaleData
                    | CapabilityReason::ClockSkew
            )
        ),
        CapabilityAvailability::Faulted => reason == Some(CapabilityReason::AdapterFault),
    }
}

/// Resolves one requested feature for UI or policy display.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityResolution {
    requested: bool,
    effective: bool,
    result: EffectiveCapability,
}

impl CapabilityResolution {
    /// Whether the caller requested this feature.
    #[must_use]
    pub const fn requested(self) -> bool {
        self.requested
    }

    /// Whether the requested feature is currently usable.
    #[must_use]
    pub const fn effective(self) -> bool {
        self.effective
    }

    /// Availability and reason shown to the caller.
    #[must_use]
    pub const fn result(self) -> EffectiveCapability {
        self.result
    }
}

/// Resolves requested versus effective state without changing adapter permissions.
#[must_use]
pub fn resolve_capability(
    snapshot: &CapabilitySnapshot,
    feature: CapabilityFeature,
    scope: CapabilityScope,
    requested: bool,
    now_ms: u64,
) -> CapabilityResolution {
    let result = snapshot.effective_at(feature, scope, now_ms);
    CapabilityResolution {
        requested,
        effective: requested && result.is_available(),
        result,
    }
}

/// Presentation path requested by the controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresentationMode {
    /// A live surface leased by the native host.
    LiveWallpaper,
    /// The operating system's static wallpaper path.
    StaticWallpaper,
    /// A normal application preview; this never claims desktop placement.
    PreviewOnly,
}

/// Requested mode and explicit permission to use a static fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PresentationRequest {
    /// Requested presentation mode.
    pub mode: PresentationMode,
    /// User consent to use the static wallpaper path if live presentation is unavailable.
    pub consent_static_fallback: bool,
}

/// Explicit reason for downgrading presentation instead of claiming unsupported success.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DowngradeReason {
    /// The requested live surface is not currently effective.
    LiveSurfaceUnavailable(EffectiveCapability),
    /// Static wallpaper is available but the user did not consent to the fallback.
    StaticFallbackRequiresConsent(EffectiveCapability),
    /// Neither live presentation nor the requested static wallpaper path is usable.
    StaticWallpaperUnavailable(EffectiveCapability),
}

/// Requested/effective presentation result; never implies an always-on-top emergency window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PresentationResolution {
    requested: PresentationMode,
    effective: PresentationMode,
    downgrade: Option<DowngradeReason>,
}

impl PresentationResolution {
    /// Requested presentation mode.
    #[must_use]
    pub const fn requested(self) -> PresentationMode {
        self.requested
    }

    /// Effective safe mode.
    #[must_use]
    pub const fn effective(self) -> PresentationMode {
        self.effective
    }

    /// Reason the effective mode differs from the request.
    #[must_use]
    pub const fn downgrade(self) -> Option<DowngradeReason> {
        self.downgrade
    }
}

/// Chooses a truthful presentation mode using only effective host capabilities and consent.
#[must_use]
pub fn resolve_presentation(
    snapshot: &CapabilitySnapshot,
    request: PresentationRequest,
    now_ms: u64,
) -> PresentationResolution {
    match request.mode {
        PresentationMode::PreviewOnly => PresentationResolution {
            requested: request.mode,
            effective: PresentationMode::PreviewOnly,
            downgrade: None,
        },
        PresentationMode::LiveWallpaper => {
            let live = snapshot.effective_at(
                CapabilityFeature::WallpaperSurface,
                CapabilityScope::Host,
                now_ms,
            );
            if live.is_available() {
                return PresentationResolution {
                    requested: request.mode,
                    effective: PresentationMode::LiveWallpaper,
                    downgrade: None,
                };
            }
            let static_wallpaper = snapshot.effective_at(
                CapabilityFeature::StaticWallpaper,
                CapabilityScope::Host,
                now_ms,
            );
            if request.consent_static_fallback && static_wallpaper.is_available() {
                PresentationResolution {
                    requested: request.mode,
                    effective: PresentationMode::StaticWallpaper,
                    downgrade: Some(DowngradeReason::LiveSurfaceUnavailable(live)),
                }
            } else if static_wallpaper.is_available() {
                PresentationResolution {
                    requested: request.mode,
                    effective: PresentationMode::PreviewOnly,
                    downgrade: Some(DowngradeReason::StaticFallbackRequiresConsent(live)),
                }
            } else {
                PresentationResolution {
                    requested: request.mode,
                    effective: PresentationMode::PreviewOnly,
                    downgrade: Some(DowngradeReason::StaticWallpaperUnavailable(
                        static_wallpaper,
                    )),
                }
            }
        }
        PresentationMode::StaticWallpaper => {
            let static_wallpaper = snapshot.effective_at(
                CapabilityFeature::StaticWallpaper,
                CapabilityScope::Host,
                now_ms,
            );
            if static_wallpaper.is_available() {
                PresentationResolution {
                    requested: request.mode,
                    effective: PresentationMode::StaticWallpaper,
                    downgrade: None,
                }
            } else {
                PresentationResolution {
                    requested: request.mode,
                    effective: PresentationMode::PreviewOnly,
                    downgrade: Some(DowngradeReason::StaticWallpaperUnavailable(
                        static_wallpaper,
                    )),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_is_not_available_and_freshness_fails_closed() {
        let host = HostIdentity::new(
            "test.adapter",
            AdapterVersion {
                major: 1,
                minor: 2,
                patch: 3,
            },
            7,
        )
        .unwrap();
        let snapshot = CapabilitySnapshot::unknown(host, HostRestrictions::empty());
        assert_eq!(snapshot.contract_version(), CAPABILITY_CONTRACT_VERSION);
        assert_eq!(
            snapshot
                .effective_at(
                    CapabilityFeature::PointerPosition,
                    CapabilityScope::Desktop,
                    0,
                )
                .availability(),
            CapabilityAvailability::Unknown
        );
        assert_eq!(
            Freshness::observed_at(11, 2).state_at(10),
            FreshnessState::ClockSkew
        );
        assert_eq!(
            Freshness::observed_at(11, 2).state_at(14),
            FreshnessState::Stale
        );
    }

    #[test]
    fn requested_and_effective_presentation_never_claims_unavailable_live_host() {
        let host = HostIdentity::new(
            "test.adapter",
            AdapterVersion {
                major: 1,
                minor: 0,
                patch: 0,
            },
            1,
        )
        .unwrap();
        let mut snapshot = CapabilitySnapshot::unknown(host, HostRestrictions::empty());
        let static_record = CapabilityRecord::new(
            CapabilityFeature::StaticWallpaper,
            CapabilityScope::Host,
            CapabilityAvailability::Available,
            None,
            Freshness::observed_at(0, 100),
            Confidence::High,
            CapabilitySet::empty(),
        )
        .unwrap();
        snapshot.replace(static_record).unwrap();
        let resolution = resolve_presentation(
            &snapshot,
            PresentationRequest {
                mode: PresentationMode::LiveWallpaper,
                consent_static_fallback: false,
            },
            1,
        );
        assert_eq!(resolution.requested(), PresentationMode::LiveWallpaper);
        assert_eq!(resolution.effective(), PresentationMode::PreviewOnly);
        assert!(matches!(
            resolution.downgrade(),
            Some(DowngradeReason::StaticFallbackRequiresConsent(_))
        ));
    }
}
