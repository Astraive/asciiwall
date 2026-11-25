//! Opaque local identities and immutable package-release descriptors.
//!
//! The package-release wire descriptor is deliberately narrower than any future package or
//! assignment schema. IDs and digests are supplied by callers; this module does not generate
//! randomness or compute hashes.

/// Number of bytes in a package, assignment, or monitor ID.
pub const ID_BYTES: usize = 16;
/// Number of bytes in a SHA-256 content digest.
pub const CONTENT_DIGEST_BYTES: usize = 32;
/// Encoded size of a version-1 package-release descriptor.
pub const PACKAGE_RELEASE_WIRE_BYTES: usize = 116;

const WIRE_MAGIC: [u8; 4] = *b"ACID";
const WIRE_VERSION: u8 = 1;
const PACKAGE_RELEASE_KIND: u8 = 1;
const FLAG_HAS_FORK_LINEAGE: u16 = 1;
const PACKAGE_RELEASE_PAYLOAD_BYTES: u32 = 104;

/// An invalid identity or descriptor construction request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityError {
    /// All-zero bytes are reserved and cannot identify an object.
    ZeroIdentifier,
    /// Package versions start at one.
    ZeroPackageVersion,
    /// A fork must have a different package ID from its upstream release.
    ForkMustUseNewPackageId,
    /// Fallback monitor geometry must have nonzero dimensions.
    InvalidMonitorDimensions,
}

impl core::fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ZeroIdentifier => formatter.write_str("identity bytes must not be all zero"),
            Self::ZeroPackageVersion => formatter.write_str("package version must be nonzero"),
            Self::ForkMustUseNewPackageId => {
                formatter.write_str("a fork must use a new package ID")
            }
            Self::InvalidMonitorDimensions => {
                formatter.write_str("monitor fallback dimensions must be nonzero")
            }
        }
    }
}

impl core::error::Error for IdentityError {}

/// Opaque identity for a published package namespace.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PackageId([u8; ID_BYTES]);

impl PackageId {
    /// Constructs an ID from caller-provided bytes; the all-zero value is reserved.
    pub const fn new(bytes: [u8; ID_BYTES]) -> Result<Self, IdentityError> {
        if bytes_are_zero(&bytes) {
            Err(IdentityError::ZeroIdentifier)
        } else {
            Ok(Self(bytes))
        }
    }

    /// Returns the fixed-width ID bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; ID_BYTES] {
        &self.0
    }
}

/// Opaque identity for one installed assignment.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AssignmentId([u8; ID_BYTES]);

impl AssignmentId {
    /// Constructs an ID from caller-provided bytes; the all-zero value is reserved.
    pub const fn new(bytes: [u8; ID_BYTES]) -> Result<Self, IdentityError> {
        if bytes_are_zero(&bytes) {
            Err(IdentityError::ZeroIdentifier)
        } else {
            Ok(Self(bytes))
        }
    }

    /// Returns the fixed-width ID bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; ID_BYTES] {
        &self.0
    }
}

/// Opaque local identity for a monitor; it is not a hardware serial or OS handle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MonitorId([u8; ID_BYTES]);

impl MonitorId {
    /// Constructs an ID from caller-provided local bytes; the all-zero value is reserved.
    pub const fn new(bytes: [u8; ID_BYTES]) -> Result<Self, IdentityError> {
        if bytes_are_zero(&bytes) {
            Err(IdentityError::ZeroIdentifier)
        } else {
            Ok(Self(bytes))
        }
    }

    /// Returns the fixed-width local ID bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; ID_BYTES] {
        &self.0
    }
}

/// A nonzero caller-assigned package release number.
///
/// Version numbers are monotonically increasing `u32` release counters per package ID, not
/// semantic-version strings. Version zero is reserved.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PackageVersion(u32);

impl PackageVersion {
    /// Creates a package version; zero is reserved.
    pub const fn new(value: u32) -> Result<Self, IdentityError> {
        if value == 0 {
            Err(IdentityError::ZeroPackageVersion)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the release counter.
    #[must_use]
    pub const fn value(self) -> u32 {
        self.0
    }
}

/// A caller-provided SHA-256 digest of exact content bytes.
///
/// The core stores and compares digest bytes but does not calculate or verify them.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ContentDigest([u8; CONTENT_DIGEST_BYTES]);

impl ContentDigest {
    /// Wraps a caller-computed SHA-256 digest.
    #[must_use]
    pub const fn new(bytes: [u8; CONTENT_DIGEST_BYTES]) -> Self {
        Self(bytes)
    }

    /// Returns the digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; CONTENT_DIGEST_BYTES] {
        &self.0
    }
}

/// Exact upstream release identity recorded by an explicitly forked package release.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ForkLineage {
    package_id: PackageId,
    package_version: PackageVersion,
    content_digest: ContentDigest,
}

impl ForkLineage {
    /// Returns the upstream package ID.
    #[must_use]
    pub const fn package_id(self) -> PackageId {
        self.package_id
    }

    /// Returns the upstream package version.
    #[must_use]
    pub const fn package_version(self) -> PackageVersion {
        self.package_version
    }

    /// Returns the upstream content digest.
    #[must_use]
    pub const fn content_digest(self) -> ContentDigest {
        self.content_digest
    }
}

/// Immutable identity of one package release, separate from its content digest.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PackageRelease {
    package_id: PackageId,
    package_version: PackageVersion,
    content_digest: ContentDigest,
    fork_lineage: Option<ForkLineage>,
}

impl PackageRelease {
    /// Creates a release without fork lineage.
    #[must_use]
    pub const fn new(
        package_id: PackageId,
        package_version: PackageVersion,
        content_digest: ContentDigest,
    ) -> Self {
        Self {
            package_id,
            package_version,
            content_digest,
            fork_lineage: None,
        }
    }

    /// Creates a fork release linked to the exact upstream release.
    ///
    /// A fork always uses a new package ID. This preserves upstream immutability while making
    /// ancestry explicit in the descriptor.
    pub fn forked_from(
        package_id: PackageId,
        package_version: PackageVersion,
        content_digest: ContentDigest,
        upstream: &Self,
    ) -> Result<Self, IdentityError> {
        if package_id == upstream.package_id {
            return Err(IdentityError::ForkMustUseNewPackageId);
        }

        Ok(Self {
            package_id,
            package_version,
            content_digest,
            fork_lineage: Some(ForkLineage {
                package_id: upstream.package_id,
                package_version: upstream.package_version,
                content_digest: upstream.content_digest,
            }),
        })
    }

    /// Returns the package namespace identity.
    #[must_use]
    pub const fn package_id(self) -> PackageId {
        self.package_id
    }

    /// Returns the package release number.
    #[must_use]
    pub const fn package_version(self) -> PackageVersion {
        self.package_version
    }

    /// Returns the caller-provided digest of the release's exact content bytes.
    #[must_use]
    pub const fn content_digest(self) -> ContentDigest {
        self.content_digest
    }

    /// Returns exact upstream release identity when this release is explicitly marked as a fork.
    #[must_use]
    pub const fn fork_lineage(self) -> Option<ForkLineage> {
        self.fork_lineage
    }

    /// Reports an attempt to change an already-published package/version record.
    ///
    /// Different package IDs or versions are independent records. A matching package ID and
    /// version must preserve both its content digest and its fork lineage.
    #[must_use]
    pub fn conflict_with(self, proposed: Self) -> Option<ReleaseConflict> {
        if self.package_id != proposed.package_id
            || self.package_version != proposed.package_version
        {
            return None;
        }

        if self.content_digest != proposed.content_digest {
            return Some(ReleaseConflict::ContentDigestChanged {
                package_id: self.package_id,
                package_version: self.package_version,
                stored: self.content_digest,
                proposed: proposed.content_digest,
            });
        }

        if self.fork_lineage != proposed.fork_lineage {
            return Some(ReleaseConflict::ForkLineageChanged {
                package_id: self.package_id,
                package_version: self.package_version,
                stored: self.fork_lineage,
                proposed: proposed.fork_lineage,
            });
        }

        None
    }

    /// Encodes this release as the fixed-width version-1 `ACID` descriptor.
    #[must_use]
    pub fn encode(self) -> [u8; PACKAGE_RELEASE_WIRE_BYTES] {
        let mut wire = [0; PACKAGE_RELEASE_WIRE_BYTES];
        wire[..4].copy_from_slice(&WIRE_MAGIC);
        wire[4] = WIRE_VERSION;
        wire[5] = PACKAGE_RELEASE_KIND;
        let flags = if self.fork_lineage.is_some() {
            FLAG_HAS_FORK_LINEAGE
        } else {
            0
        };
        wire[6..8].copy_from_slice(&flags.to_le_bytes());
        wire[8..12].copy_from_slice(&PACKAGE_RELEASE_PAYLOAD_BYTES.to_le_bytes());
        wire[12..28].copy_from_slice(self.package_id.as_bytes());
        wire[28..32].copy_from_slice(&self.package_version.value().to_le_bytes());
        wire[32..64].copy_from_slice(self.content_digest.as_bytes());

        if let Some(lineage) = self.fork_lineage {
            wire[64..80].copy_from_slice(lineage.package_id.as_bytes());
            wire[80..84].copy_from_slice(&lineage.package_version.value().to_le_bytes());
            wire[84..116].copy_from_slice(lineage.content_digest.as_bytes());
        }

        wire
    }

    /// Decodes and validates an exact fixed-width version-1 `ACID` descriptor.
    pub fn decode(wire: &[u8]) -> Result<Self, DecodeError> {
        if wire.len() != PACKAGE_RELEASE_WIRE_BYTES {
            return Err(DecodeError::InvalidLength);
        }
        if wire[..4] != WIRE_MAGIC {
            return Err(DecodeError::InvalidMagic);
        }
        if wire[4] != WIRE_VERSION {
            return Err(DecodeError::UnsupportedVersion);
        }
        if wire[5] != PACKAGE_RELEASE_KIND {
            return Err(DecodeError::UnsupportedRecordKind);
        }

        let flags = u16::from_le_bytes([wire[6], wire[7]]);
        if flags & !FLAG_HAS_FORK_LINEAGE != 0 {
            return Err(DecodeError::InvalidFlags);
        }
        if u32::from_le_bytes([wire[8], wire[9], wire[10], wire[11]])
            != PACKAGE_RELEASE_PAYLOAD_BYTES
        {
            return Err(DecodeError::InvalidPayloadLength);
        }

        let mut package_id_bytes = [0; ID_BYTES];
        package_id_bytes.copy_from_slice(&wire[12..28]);
        let package_id =
            PackageId::new(package_id_bytes).map_err(|_| DecodeError::InvalidPackageId)?;
        let package_version =
            PackageVersion::new(u32::from_le_bytes([wire[28], wire[29], wire[30], wire[31]]))
                .map_err(|_| DecodeError::InvalidPackageVersion)?;
        let mut content_digest_bytes = [0; CONTENT_DIGEST_BYTES];
        content_digest_bytes.copy_from_slice(&wire[32..64]);
        let content_digest = ContentDigest::new(content_digest_bytes);

        let fork_lineage = if flags == FLAG_HAS_FORK_LINEAGE {
            let mut lineage_id_bytes = [0; ID_BYTES];
            lineage_id_bytes.copy_from_slice(&wire[64..80]);
            let lineage_id =
                PackageId::new(lineage_id_bytes).map_err(|_| DecodeError::InvalidLineageId)?;
            if lineage_id == package_id {
                return Err(DecodeError::ForkMustUseNewPackageId);
            }
            let lineage_version =
                PackageVersion::new(u32::from_le_bytes([wire[80], wire[81], wire[82], wire[83]]))
                    .map_err(|_| DecodeError::InvalidLineageVersion)?;
            let mut lineage_digest_bytes = [0; CONTENT_DIGEST_BYTES];
            lineage_digest_bytes.copy_from_slice(&wire[84..116]);
            Some(ForkLineage {
                package_id: lineage_id,
                package_version: lineage_version,
                content_digest: ContentDigest::new(lineage_digest_bytes),
            })
        } else {
            if !bytes_are_zero(&wire[64..116]) {
                return Err(DecodeError::NonZeroAbsentLineage);
            }
            None
        };

        Ok(Self {
            package_id,
            package_version,
            content_digest,
            fork_lineage,
        })
    }
}

/// An immutable release-record conflict visible to the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseConflict {
    /// The exact bytes for an existing package/version were changed.
    ContentDigestChanged {
        package_id: PackageId,
        package_version: PackageVersion,
        stored: ContentDigest,
        proposed: ContentDigest,
    },
    /// Fork ancestry for an existing package/version was changed.
    ForkLineageChanged {
        package_id: PackageId,
        package_version: PackageVersion,
        stored: Option<ForkLineage>,
        proposed: Option<ForkLineage>,
    },
}

impl core::fmt::Display for ReleaseConflict {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ContentDigestChanged {
                package_id,
                package_version,
                ..
            } => write!(
                formatter,
                "immutable content changed for package {:?} version {}",
                package_id,
                package_version.value()
            ),
            Self::ForkLineageChanged {
                package_id,
                package_version,
                ..
            } => write!(
                formatter,
                "immutable fork lineage changed for package {:?} version {}",
                package_id,
                package_version.value()
            ),
        }
    }
}

impl core::error::Error for ReleaseConflict {}

/// Typed rejection of a malformed or unsupported package-release descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    /// The input is not exactly the fixed v1 wire size.
    InvalidLength,
    /// The four-byte descriptor magic is not `ACID`.
    InvalidMagic,
    /// The wire-format version is not supported.
    UnsupportedVersion,
    /// The record kind is not a package-release descriptor.
    UnsupportedRecordKind,
    /// A reserved flag bit is nonzero.
    InvalidFlags,
    /// The declared payload length does not match the fixed v1 size.
    InvalidPayloadLength,
    /// The package ID is the reserved all-zero value.
    InvalidPackageId,
    /// The package version is zero.
    InvalidPackageVersion,
    /// A present lineage package ID is the reserved all-zero value.
    InvalidLineageId,
    /// A present lineage version is zero.
    InvalidLineageVersion,
    /// An absent lineage field contains nonzero bytes instead of canonical zero padding.
    NonZeroAbsentLineage,
    /// A fork descriptor names its own package ID as its upstream.
    ForkMustUseNewPackageId,
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidLength => formatter.write_str("invalid package-release wire length"),
            Self::InvalidMagic => formatter.write_str("invalid package-release wire magic"),
            Self::UnsupportedVersion => formatter.write_str("unsupported wire-format version"),
            Self::UnsupportedRecordKind => formatter.write_str("unsupported descriptor kind"),
            Self::InvalidFlags => formatter.write_str("invalid package-release flags"),
            Self::InvalidPayloadLength => {
                formatter.write_str("invalid package-release payload length")
            }
            Self::InvalidPackageId => formatter.write_str("invalid package ID"),
            Self::InvalidPackageVersion => formatter.write_str("invalid package version"),
            Self::InvalidLineageId => formatter.write_str("invalid fork-lineage package ID"),
            Self::InvalidLineageVersion => formatter.write_str("invalid fork-lineage version"),
            Self::NonZeroAbsentLineage => {
                formatter.write_str("absent fork-lineage bytes must be zero")
            }
            Self::ForkMustUseNewPackageId => {
                formatter.write_str("a fork must use a new package ID")
            }
        }
    }
}

impl core::error::Error for DecodeError {}

/// A fixed record of the immutable inputs named by an assignment ID.
///
/// This is a local API contract, not a wire-encoded future assignment or package schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssignmentBinding {
    assignment_id: AssignmentId,
    package_release: PackageRelease,
    overrides_digest: ContentDigest,
    output_selectors_digest: ContentDigest,
}

impl AssignmentBinding {
    /// Binds an assignment ID to a release, override digest, and output-selector digest.
    #[must_use]
    pub const fn new(
        assignment_id: AssignmentId,
        package_release: PackageRelease,
        overrides_digest: ContentDigest,
        output_selectors_digest: ContentDigest,
    ) -> Self {
        Self {
            assignment_id,
            package_release,
            overrides_digest,
            output_selectors_digest,
        }
    }

    /// Returns the opaque assignment ID.
    #[must_use]
    pub const fn assignment_id(self) -> AssignmentId {
        self.assignment_id
    }

    /// Returns the exact installed package release.
    #[must_use]
    pub const fn package_release(self) -> PackageRelease {
        self.package_release
    }

    /// Returns the caller-provided digest of the assignment's override set.
    #[must_use]
    pub const fn overrides_digest(self) -> ContentDigest {
        self.overrides_digest
    }

    /// Returns the caller-provided digest of the selected outputs.
    #[must_use]
    pub const fn output_selectors_digest(self) -> ContentDigest {
        self.output_selectors_digest
    }

    /// Reports reuse of one assignment ID for a different immutable binding.
    #[must_use]
    pub fn conflict_with(self, proposed: Self) -> Option<AssignmentBindingConflict> {
        if self.assignment_id == proposed.assignment_id
            && (self.package_release != proposed.package_release
                || self.overrides_digest != proposed.overrides_digest
                || self.output_selectors_digest != proposed.output_selectors_digest)
        {
            Some(AssignmentBindingConflict {
                assignment_id: self.assignment_id,
            })
        } else {
            None
        }
    }
}

/// An attempt to silently rebind an existing assignment ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssignmentBindingConflict {
    assignment_id: AssignmentId,
}

impl AssignmentBindingConflict {
    /// Returns the assignment ID whose immutable binding changed.
    #[must_use]
    pub const fn assignment_id(self) -> AssignmentId {
        self.assignment_id
    }
}

impl core::fmt::Display for AssignmentBindingConflict {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "immutable binding changed for assignment {:?}",
            self.assignment_id
        )
    }
}

impl core::error::Error for AssignmentBindingConflict {}

/// Non-unique, local geometry used only as a best-effort monitor fallback match.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MonitorFallbackMatch {
    desktop_origin_x: i32,
    desktop_origin_y: i32,
    width_px: u32,
    height_px: u32,
}

impl MonitorFallbackMatch {
    /// Creates a fallback record from desktop origin and active pixel dimensions.
    pub const fn new(
        desktop_origin_x: i32,
        desktop_origin_y: i32,
        width_px: u32,
        height_px: u32,
    ) -> Result<Self, IdentityError> {
        if width_px == 0 || height_px == 0 {
            return Err(IdentityError::InvalidMonitorDimensions);
        }
        Ok(Self {
            desktop_origin_x,
            desktop_origin_y,
            width_px,
            height_px,
        })
    }

    /// Returns the local desktop-space horizontal origin.
    #[must_use]
    pub const fn desktop_origin_x(self) -> i32 {
        self.desktop_origin_x
    }

    /// Returns the local desktop-space vertical origin.
    #[must_use]
    pub const fn desktop_origin_y(self) -> i32 {
        self.desktop_origin_y
    }

    /// Returns the active width in pixels.
    #[must_use]
    pub const fn width_px(self) -> u32 {
        self.width_px
    }

    /// Returns the active height in pixels.
    #[must_use]
    pub const fn height_px(self) -> u32 {
        self.height_px
    }
}

/// Local monitor ID plus a best-effort fallback match record.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MonitorIdentity {
    id: MonitorId,
    fallback: MonitorFallbackMatch,
}

impl MonitorIdentity {
    /// Creates a local monitor identity with its non-unique fallback match record.
    #[must_use]
    pub const fn new(id: MonitorId, fallback: MonitorFallbackMatch) -> Self {
        Self { id, fallback }
    }

    /// Returns the local opaque monitor ID.
    #[must_use]
    pub const fn id(self) -> MonitorId {
        self.id
    }

    /// Returns the local best-effort fallback match record.
    #[must_use]
    pub const fn fallback(self) -> MonitorFallbackMatch {
        self.fallback
    }
}

const fn bytes_are_zero(bytes: &[u8]) -> bool {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}
