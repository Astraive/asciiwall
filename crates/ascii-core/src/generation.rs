//! Monotonic generation tokens for rejecting stale asynchronous results.

/// A monotonically increasing generation for one current assignment/source context.
///
/// Capture a generation when work starts, then compare it with the current value before publishing
/// the result. Advance the value after settings edits, seeks, source replacement, or output-topology
/// resets. Generation zero is a valid initial value; incrementing the maximum value fails rather
/// than wrapping and accidentally making stale work current again.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Generation(u64);

impl Generation {
    /// The initial generation for a newly scoped runtime context.
    pub const INITIAL: Self = Self(0);

    /// Creates a generation from a caller-owned monotonic counter value.
    #[must_use]
    pub const fn from_value(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying fixed-width counter value.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }

    /// Returns whether this captured generation still equals the current generation.
    #[must_use]
    pub const fn is_current(self, current: Self) -> bool {
        self.0 == current.0
    }

    /// Returns the next generation, or `None` rather than wrapping at `u64::MAX`.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}
