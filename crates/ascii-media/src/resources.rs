use std::sync::{
    Arc,
    atomic::{AtomicU32, AtomicU64, Ordering},
};

use ascii_core::{BudgetKind, Error};

/// Shared memory and concurrency ceilings for image decodes.
///
/// Clone one value and share it across every image decode that belongs to the same job or
/// process. Retained [`crate::DecodedFrame`] pixels remain charged until their owner is dropped.
#[derive(Clone, Debug)]
pub struct DecodeResources {
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    max_active_decodes: u32,
    max_cpu_bytes: u64,
    active_decodes: AtomicU32,
    cpu_bytes: AtomicU64,
}

impl DecodeResources {
    /// Creates a shared decode ledger with explicit concurrent-job and CPU-memory ceilings.
    #[must_use]
    pub fn new(max_active_decodes: u32, max_cpu_bytes: u64) -> Self {
        Self {
            shared: Arc::new(Shared {
                max_active_decodes,
                max_cpu_bytes,
                active_decodes: AtomicU32::new(0),
                cpu_bytes: AtomicU64::new(0),
            }),
        }
    }

    /// Returns the number of active decodes sharing this ledger.
    #[must_use]
    pub fn active_decodes(&self) -> u32 {
        self.shared.active_decodes.load(Ordering::Acquire)
    }

    /// Returns bytes currently reserved by active decodes and retained frames.
    #[must_use]
    pub fn cpu_bytes_in_use(&self) -> u64 {
        self.shared.cpu_bytes.load(Ordering::Acquire)
    }

    pub(crate) fn try_acquire_decode(&self) -> Result<DecodePermit, Error> {
        let mut active = self.shared.active_decodes.load(Ordering::Acquire);
        loop {
            if active >= self.shared.max_active_decodes {
                return Err(Error::BudgetExceeded(BudgetKind::DecodeConcurrency));
            }
            match self.shared.active_decodes.compare_exchange_weak(
                active,
                active.checked_add(1).ok_or(Error::SizeOverflow)?,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(DecodePermit(Arc::clone(&self.shared))),
                Err(observed) => active = observed,
            }
        }
    }

    pub(crate) fn try_reserve_memory(&self, bytes: u64) -> Result<MemoryPermit, Error> {
        let mut used = self.shared.cpu_bytes.load(Ordering::Acquire);
        loop {
            let next = used.checked_add(bytes).ok_or(Error::SizeOverflow)?;
            if next > self.shared.max_cpu_bytes {
                return Err(Error::BudgetExceeded(BudgetKind::RasterBytes));
            }
            match self.shared.cpu_bytes.compare_exchange_weak(
                used,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(MemoryPermit {
                        shared: Arc::clone(&self.shared),
                        bytes,
                    });
                }
                Err(observed) => used = observed,
            }
        }
    }
}

/// A reservation for a temporary codec/input buffer or a retained decoded raster.
#[derive(Debug)]
pub(crate) struct MemoryPermit {
    shared: Arc<Shared>,
    bytes: u64,
}

impl MemoryPermit {
    pub(crate) const fn bytes(&self) -> u64 {
        self.bytes
    }
}

impl Drop for MemoryPermit {
    fn drop(&mut self) {
        let previous = self
            .shared
            .cpu_bytes
            .fetch_sub(self.bytes, Ordering::AcqRel);
        debug_assert!(previous >= self.bytes, "decoder memory permit underflow");
    }
}

#[derive(Debug)]
pub(crate) struct DecodePermit(Arc<Shared>);

impl Drop for DecodePermit {
    fn drop(&mut self) {
        let previous = self.0.active_decodes.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "decoder concurrency permit underflow");
    }
}

#[cfg(test)]
mod tests {
    use super::DecodeResources;
    use ascii_core::{BudgetKind, Error};

    #[test]
    fn reservations_are_shared_and_release_on_drop() {
        let resources = DecodeResources::new(1, 8);
        let other_handle = resources.clone();
        let decode = resources.try_acquire_decode().unwrap();
        assert_eq!(other_handle.active_decodes(), 1);
        assert!(matches!(
            other_handle.try_acquire_decode(),
            Err(Error::BudgetExceeded(BudgetKind::DecodeConcurrency))
        ));

        let memory = resources.try_reserve_memory(8).unwrap();
        assert_eq!(other_handle.cpu_bytes_in_use(), 8);
        assert!(matches!(
            other_handle.try_reserve_memory(1),
            Err(Error::BudgetExceeded(BudgetKind::RasterBytes))
        ));
        drop(memory);
        drop(decode);
        assert_eq!(resources.cpu_bytes_in_use(), 0);
        assert_eq!(resources.active_decodes(), 0);
    }
}
