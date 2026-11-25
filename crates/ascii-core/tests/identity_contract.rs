use std::vec::Vec;

use ascii_core::{
    AssignmentBinding, AssignmentId, ContentDigest, DecodeError, Generation, IdentityError,
    MonitorFallbackMatch, MonitorId, MonitorIdentity, PackageId, PackageRelease, PackageVersion,
    ReleaseConflict, PACKAGE_RELEASE_WIRE_BYTES,
};

const RELEASE_GOLDEN_HEX: &str = concat!(
    "414349440101000068000000",
    "11111111111111111111111111111111",
    "04030201",
    "a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5",
    "a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5",
    "00000000",
    "00000000",
    "00000000",
    "00000000",
    "00000000",
    "00000000",
    "00000000",
    "00000000",
    "00000000",
    "00000000",
    "00000000",
    "00000000",
    "00000000",
);

fn package_id(byte: u8) -> PackageId {
    PackageId::new([byte; 16]).unwrap()
}

fn package_version(value: u32) -> PackageVersion {
    PackageVersion::new(value).unwrap()
}

fn digest(byte: u8) -> ContentDigest {
    ContentDigest::new([byte; 32])
}

fn release() -> PackageRelease {
    PackageRelease::new(
        package_id(0x11),
        package_version(0x0102_0304),
        digest(0xa5),
    )
}

fn decode_hex(hex: &str) -> Vec<u8> {
    fn nibble(byte: u8) -> u8 {
        match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            _ => panic!("invalid fixture hex"),
        }
    }

    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks_exact(2) {
        bytes.push((nibble(pair[0]) << 4) | nibble(pair[1]));
    }
    assert_eq!(hex.len() % 2, 0);
    bytes
}

#[test]
fn package_release_matches_the_fixed_golden_wire_vector() {
    let expected = decode_hex(RELEASE_GOLDEN_HEX);
    let encoded = release().encode();

    assert_eq!(expected.len(), PACKAGE_RELEASE_WIRE_BYTES);
    assert_eq!(encoded.as_slice(), expected.as_slice());
    assert_eq!(PackageRelease::decode(&encoded), Ok(release()));
}

#[test]
fn forked_release_roundtrips_exact_upstream_lineage() {
    let upstream = release();
    let fork = PackageRelease::forked_from(
        package_id(0x22),
        package_version(7),
        digest(0xb6),
        &upstream,
    )
    .unwrap();
    let decoded = PackageRelease::decode(&fork.encode()).unwrap();
    let lineage = decoded.fork_lineage().unwrap();

    assert_eq!(decoded, fork);
    assert_eq!(lineage.package_id(), upstream.package_id());
    assert_eq!(lineage.package_version(), upstream.package_version());
    assert_eq!(lineage.content_digest(), upstream.content_digest());
    assert_eq!(
        PackageRelease::forked_from(
            upstream.package_id(),
            package_version(8),
            digest(0xb7),
            &upstream,
        ),
        Err(IdentityError::ForkMustUseNewPackageId)
    );
}

#[test]
fn package_release_decoder_rejects_malformed_headers_and_values() {
    let valid = release().encode();

    let mut invalid = valid;
    invalid[0] ^= 1;
    assert_eq!(
        PackageRelease::decode(&invalid),
        Err(DecodeError::InvalidMagic)
    );

    let mut invalid = valid;
    invalid[4] = 2;
    assert_eq!(
        PackageRelease::decode(&invalid),
        Err(DecodeError::UnsupportedVersion)
    );

    let mut invalid = valid;
    invalid[5] = 2;
    assert_eq!(
        PackageRelease::decode(&invalid),
        Err(DecodeError::UnsupportedRecordKind)
    );

    let mut invalid = valid;
    invalid[6] = 2;
    assert_eq!(
        PackageRelease::decode(&invalid),
        Err(DecodeError::InvalidFlags)
    );

    let mut invalid = valid;
    invalid[8] = 0x69;
    assert_eq!(
        PackageRelease::decode(&invalid),
        Err(DecodeError::InvalidPayloadLength)
    );

    let mut invalid = valid;
    invalid[12..28].fill(0);
    assert_eq!(
        PackageRelease::decode(&invalid),
        Err(DecodeError::InvalidPackageId)
    );

    let mut invalid = valid;
    invalid[28..32].fill(0);
    assert_eq!(
        PackageRelease::decode(&invalid),
        Err(DecodeError::InvalidPackageVersion)
    );

    let mut invalid = valid;
    invalid[64] = 1;
    assert_eq!(
        PackageRelease::decode(&invalid),
        Err(DecodeError::NonZeroAbsentLineage)
    );

    let mut invalid_lineage = PackageRelease::forked_from(
        package_id(0x22),
        package_version(7),
        digest(0xb6),
        &release(),
    )
    .unwrap()
    .encode();
    invalid_lineage[64..80].copy_from_slice(package_id(0x22).as_bytes());
    assert_eq!(
        PackageRelease::decode(&invalid_lineage),
        Err(DecodeError::ForkMustUseNewPackageId)
    );

    let mut invalid_lineage = PackageRelease::forked_from(
        package_id(0x22),
        package_version(7),
        digest(0xb6),
        &release(),
    )
    .unwrap()
    .encode();
    invalid_lineage[64..80].fill(0);
    assert_eq!(
        PackageRelease::decode(&invalid_lineage),
        Err(DecodeError::InvalidLineageId)
    );

    let mut invalid_lineage = PackageRelease::forked_from(
        package_id(0x22),
        package_version(7),
        digest(0xb6),
        &release(),
    )
    .unwrap()
    .encode();
    invalid_lineage[80..84].fill(0);
    assert_eq!(
        PackageRelease::decode(&invalid_lineage),
        Err(DecodeError::InvalidLineageVersion)
    );

    assert_eq!(
        PackageRelease::decode(&valid[..PACKAGE_RELEASE_WIRE_BYTES - 1]),
        Err(DecodeError::InvalidLength)
    );
    let mut too_long = [0; PACKAGE_RELEASE_WIRE_BYTES + 1];
    too_long[..PACKAGE_RELEASE_WIRE_BYTES].copy_from_slice(&valid);
    assert_eq!(
        PackageRelease::decode(&too_long),
        Err(DecodeError::InvalidLength)
    );
}

#[test]
fn immutable_release_conflicts_are_visible_and_forks_keep_upstream_identity() {
    let stored = release();
    let changed = PackageRelease::new(
        stored.package_id(),
        stored.package_version(),
        digest(0xb6),
    );

    assert_eq!(
        stored.conflict_with(changed),
        Some(ReleaseConflict::ContentDigestChanged {
            package_id: stored.package_id(),
            package_version: stored.package_version(),
            stored: stored.content_digest(),
            proposed: changed.content_digest(),
        })
    );
    assert_eq!(
        stored.conflict_with(PackageRelease::new(
            stored.package_id(),
            package_version(stored.package_version().value() + 1),
            digest(0xb6),
        )),
        None
    );
    let upstream_a = PackageRelease::new(package_id(0x71), package_version(1), digest(0x72));
    let upstream_b = PackageRelease::new(package_id(0x73), package_version(1), digest(0x74));
    let fork_a = PackageRelease::forked_from(
        package_id(0x22),
        package_version(7),
        digest(0xb6),
        &upstream_a,
    )
    .unwrap();
    let fork_b = PackageRelease::forked_from(
        package_id(0x22),
        package_version(7),
        digest(0xb6),
        &upstream_b,
    )
    .unwrap();
    assert!(matches!(
        fork_a.conflict_with(fork_b),
        Some(ReleaseConflict::ForkLineageChanged { .. })
    ));
}

#[test]
fn assignment_binding_and_monitor_fallback_are_explicit_local_records() {
    let assignment_id = AssignmentId::new([0x31; 16]).unwrap();
    let binding = AssignmentBinding::new(assignment_id, release(), digest(0x41), digest(0x51));
    let changed = AssignmentBinding::new(assignment_id, release(), digest(0x41), digest(0x52));

    assert_eq!(binding.conflict_with(changed).unwrap().assignment_id(), assignment_id);

    let fallback = MonitorFallbackMatch::new(-1920, 0, 1920, 1080).unwrap();
    let monitor = MonitorIdentity::new(MonitorId::new([0x61; 16]).unwrap(), fallback);
    assert_eq!(monitor.fallback(), fallback);
    assert_eq!(monitor.fallback().desktop_origin_x(), -1920);
    assert_eq!(monitor.fallback().width_px(), 1920);
    assert_eq!(
        MonitorFallbackMatch::new(0, 0, 0, 1080),
        Err(IdentityError::InvalidMonitorDimensions)
    );
}

#[test]
fn generation_comparison_rejects_stale_work_without_counter_wraparound() {
    let captured = Generation::INITIAL;
    let current = captured.checked_next().unwrap();

    assert!(current.is_current(current));
    assert!(!captured.is_current(current));
    assert_eq!(Generation::from_value(u64::MAX).checked_next(), None);
}
