use std::sync::atomic::{AtomicU64, Ordering};

use std::sync::Arc;

use ascii_core::{BudgetKind, Error};

use crate::params::MAX_RUNTIME_RESOURCE_BYTES;

#[derive(Debug)]
struct BudgetState {
    maximum_bytes: u64,
    reserved_bytes: AtomicU64,
}

/// Cloneable aggregate budget shared by previews or other concurrent scene runtimes.
#[derive(Clone, Debug)]
pub struct SceneResourceBudget {
    state: Arc<BudgetState>,
}

impl SceneResourceBudget {
    /// Creates a shared aggregate logical-allocation ceiling.
    #[must_use]
    pub fn new(maximum_bytes: u64) -> Self {
        Self {
            state: Arc::new(BudgetState {
                maximum_bytes,
                reserved_bytes: AtomicU64::new(0),
            }),
        }
    }

    #[must_use]
    pub fn maximum_bytes(&self) -> u64 {
        self.state.maximum_bytes
    }

    #[must_use]
    pub fn reserved_bytes(&self) -> u64 {
        self.state.reserved_bytes.load(Ordering::Acquire)
    }

    pub(crate) fn reserve(&self, bytes: u64) -> Result<ResourceReservation, Error> {
        let reserved =
            self.state
                .reserved_bytes
                .try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                    let next = current.checked_add(bytes)?;
                    (next <= self.state.maximum_bytes).then_some(next)
                });
        if reserved.is_err() {
            return Err(Error::BudgetExceeded(BudgetKind::FrameBytes));
        }
        Ok(ResourceReservation {
            state: Arc::clone(&self.state),
            bytes,
        })
    }
}

impl Default for SceneResourceBudget {
    fn default() -> Self {
        Self::new(MAX_RUNTIME_RESOURCE_BYTES * 4)
    }
}

#[derive(Debug)]
pub(crate) struct ResourceReservation {
    state: Arc<BudgetState>,
    bytes: u64,
}

impl Drop for ResourceReservation {
    fn drop(&mut self) {
        self.state
            .reserved_bytes
            .fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
