use ascii_core::{Error, MalformedReason};

/// The pinned generator used for all first-party scene random streams.
pub const PRNG_ALGORITHM: &str = "SplitMix64 (reference 64-bit finalizer, v1)";
pub const SEED_DERIVATION: &str = "FNV-1a-64(versioned node/effect domain || u32-seed-le || length-prefixed UTF-8 IDs), then SplitMix64 finalizer";
pub const MAX_NODE_ID_BYTES: usize = 128;

const SPLITMIX_INCREMENT: u64 = 0x9e37_79b9_7f4a_7c15;
const FNV1A_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A_PRIME: u64 = 0x0000_0100_0000_01b3;
const NODE_SEED_DOMAIN: &[u8] = b"asciiwall/scn-001/node-seed/v1\0";
const EFFECT_SEED_DOMAIN: &[u8] = b"asciiwall/scn-001/effect-seed/v1\0";

/// Derives a node-local seed without consuming or depending on another node's stream.
pub fn derive_node_seed(scene_seed: u32, node_id: &str) -> Result<u64, Error> {
    derive_stream_seed(scene_seed, NODE_SEED_DOMAIN, node_id, None)
}

/// Derives a separate effect stream within one node.
pub fn derive_effect_seed(scene_seed: u32, node_id: &str, effect_id: &str) -> Result<u64, Error> {
    derive_stream_seed(scene_seed, EFFECT_SEED_DOMAIN, node_id, Some(effect_id))
}

fn derive_stream_seed(
    scene_seed: u32,
    domain: &[u8],
    node_id: &str,
    effect_id: Option<&str>,
) -> Result<u64, Error> {
    if node_id.is_empty()
        || node_id.len() > MAX_NODE_ID_BYTES
        || effect_id.is_some_and(|id| id.is_empty() || id.len() > MAX_NODE_ID_BYTES)
    {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }
    let mut hash = hash_bytes(FNV1A_OFFSET, domain);
    hash = hash_bytes(hash, &scene_seed.to_le_bytes());
    hash = hash_component(hash, node_id)?;
    if let Some(effect_id) = effect_id {
        hash = hash_component(hash, effect_id)?;
    }
    Ok(splitmix64_finalize(hash))
}

fn hash_component(mut hash: u64, value: &str) -> Result<u64, Error> {
    let length = u64::try_from(value.len()).map_err(|_| Error::SizeOverflow)?;
    hash = hash_bytes(hash, &length.to_le_bytes());
    Ok(hash_bytes(hash, value.as_bytes()))
}

fn hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV1A_PRIME);
    }
    hash
}

/// A small, allocation-free SplitMix64 stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RandomStream {
    state: u64,
}

impl RandomStream {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn for_node(scene_seed: u32, node_id: &str) -> Result<Self, Error> {
        Ok(Self::new(derive_node_seed(scene_seed, node_id)?))
    }

    pub fn for_effect(scene_seed: u32, node_id: &str, effect_id: &str) -> Result<Self, Error> {
        Ok(Self::new(derive_effect_seed(
            scene_seed, node_id, effect_id,
        )?))
    }

    /// Produces the next value using the published SplitMix64 state transition and finalizer.
    #[must_use]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(SPLITMIX_INCREMENT);
        splitmix64_finalize(self.state)
    }

    /// Reduces by fixed multiply-high mapping into `0..upper`; returns `None` for an empty range.
    #[must_use]
    pub fn below(&mut self, upper: u32) -> Option<u32> {
        if upper == 0 {
            return None;
        }
        Some(((u128::from(self.next_u64()) * u128::from(upper)) >> 64) as u32)
    }
}

fn splitmix64_finalize(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
