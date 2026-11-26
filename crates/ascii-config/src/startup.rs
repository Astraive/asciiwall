/// Caller-supplied retry limits from the owning recovery descriptors. This
/// crate does not invent default retry policy values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RestorePolicy {
    max_failures: u32,
    window_millis: u64,
}

impl RestorePolicy {
    pub fn new(max_failures: u32, window_millis: u64) -> Result<Self, RestorePolicyError> {
        if window_millis == 0 {
            return Err(RestorePolicyError::ZeroWindow);
        }
        Ok(Self {
            max_failures,
            window_millis,
        })
    }

    #[must_use]
    pub const fn max_failures(self) -> u32 {
        self.max_failures
    }

    #[must_use]
    pub const fn window_millis(self) -> u64 {
        self.window_millis
    }
}

/// Invalid caller-supplied startup restore policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestorePolicyError {
    ZeroWindow,
}

impl std::fmt::Display for RestorePolicyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("startup restore window must be nonzero")
    }
}

impl std::error::Error for RestorePolicyError {}

/// Durable state needed to suppress repeated restore failures. The host must
/// persist this small state alongside its controller-owned recovery record;
/// an in-memory default alone cannot detect a crash loop across launches.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RestoreFailureState {
    window_started_unix_millis: Option<u64>,
    failures: u32,
}

impl RestoreFailureState {
    /// Restores bounded scalar state previously saved by the host. A missing start
    /// timestamp with outstanding failures means elapsed time is unknown.
    #[must_use]
    pub const fn from_persisted(window_started_unix_millis: Option<u64>, failures: u32) -> Self {
        Self {
            window_started_unix_millis,
            failures,
        }
    }

    #[must_use]
    pub const fn failures(&self) -> u32 {
        self.failures
    }

    #[must_use]
    pub const fn window_started_unix_millis(&self) -> Option<u64> {
        self.window_started_unix_millis
    }

    /// Clears failures after the host reports a completely successful restore.
    pub fn record_success(&mut self) {
        self.window_started_unix_millis = None;
        self.failures = 0;
    }

    /// Records a failure reported after validation, such as an activation or
    /// resource-preparation failure in the consuming host. An unavailable clock
    /// leaves elapsed time unknown until a later reading anchors the window.
    pub fn record_failure(&mut self, now_unix_millis: Option<u64>, policy: RestorePolicy) {
        self.reset_expired_window(now_unix_millis, policy);
        self.failures = self.failures.saturating_add(1);
    }

    /// Validates the last-wallpaper candidate before returning a value that a
    /// host may pass to resource preparation. The caller owns the persisted
    /// failure state and supplies the retry policy and optional clock reading.
    pub fn validate_before_restore<T, V, E>(
        &mut self,
        enabled: bool,
        now_unix_millis: Option<u64>,
        policy: RestorePolicy,
        candidate: T,
        validate: impl FnOnce(T) -> Result<V, E>,
    ) -> Result<V, RestoreGateError<E>> {
        if !enabled {
            return Err(RestoreGateError::Disabled);
        }
        if policy.max_failures == 0 {
            return Err(RestoreGateError::Suppressed);
        }
        self.reset_expired_window(now_unix_millis, policy);
        if self.failures >= policy.max_failures {
            return Err(RestoreGateError::Suppressed);
        }
        match validate(candidate) {
            Ok(validated) => Ok(validated),
            Err(error) => {
                self.record_failure(now_unix_millis, policy);
                Err(RestoreGateError::ValidationFailed(error))
            }
        }
    }

    fn reset_expired_window(&mut self, now_unix_millis: Option<u64>, policy: RestorePolicy) {
        match (self.window_started_unix_millis, now_unix_millis) {
            (Some(start), Some(now)) if now.saturating_sub(start) >= policy.window_millis => {
                self.window_started_unix_millis = Some(now);
                self.failures = 0;
            }
            (None, Some(now)) => self.window_started_unix_millis = Some(now),
            _ => {}
        }
    }
}

/// A startup restore was disabled, suppressed, or failed its caller-provided
/// validation boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RestoreGateError<E> {
    Disabled,
    Suppressed,
    ValidationFailed(E),
}

#[cfg(test)]
mod tests {
    use super::{RestoreFailureState, RestoreGateError, RestorePolicy};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn validates_candidate_before_return_and_suppresses_repeated_failures() {
        let policy = RestorePolicy::new(2, 300_000).unwrap();
        let mut state = RestoreFailureState::default();
        let first =
            state.validate_before_restore(true, Some(10_000), policy, "last-wallpaper", |_| {
                Err::<(), _>("missing source")
            });
        assert_eq!(
            first,
            Err(RestoreGateError::ValidationFailed("missing source"))
        );
        assert_eq!(state.failures(), 1);

        let second =
            state.validate_before_restore(true, Some(11_000), policy, "last-wallpaper", |_| {
                Err::<(), _>("invalid source")
            });
        assert_eq!(
            second,
            Err(RestoreGateError::ValidationFailed("invalid source"))
        );
        assert_eq!(state.failures(), 2);

        let validations = AtomicUsize::new(0);
        let third =
            state.validate_before_restore(true, Some(12_000), policy, "last-wallpaper", |_| {
                validations.fetch_add(1, Ordering::Relaxed);
                Ok::<_, ()>(())
            });
        assert_eq!(third, Err(RestoreGateError::Suppressed));
        assert_eq!(validations.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn expired_window_restarts_before_validation() {
        let policy = RestorePolicy::new(1, 100).unwrap();
        let mut state = RestoreFailureState::from_persisted(Some(10), 1);
        let validated = state
            .validate_before_restore(true, Some(110), policy, 42, |value| Ok::<_, ()>(value + 1))
            .unwrap();
        assert_eq!(validated, 43);
        assert_eq!(state.window_started_unix_millis(), Some(110));
        assert_eq!(state.failures(), 0);

        state.record_failure(Some(120), policy);
        assert_eq!(state.window_started_unix_millis(), Some(110));
        assert_eq!(state.failures(), 1);
    }

    #[test]
    fn unavailable_clock_preserves_restore_failure_suppression() {
        let policy = RestorePolicy::new(1, 100).unwrap();
        let mut state = RestoreFailureState::from_persisted(Some(10), 1);
        let validations = AtomicUsize::new(0);

        let result = state.validate_before_restore(true, None, policy, 42, |value| {
            validations.fetch_add(1, Ordering::Relaxed);
            Ok::<_, ()>(value)
        });

        assert_eq!(result, Err(RestoreGateError::Suppressed));
        assert_eq!(validations.load(Ordering::Relaxed), 0);
        assert_eq!(state.window_started_unix_millis(), Some(10));
        assert_eq!(state.failures(), 1);
    }

    #[test]
    fn unknown_elapsed_waits_for_a_known_window_before_resetting_failures() {
        let policy = RestorePolicy::new(1, 100).unwrap();
        let mut state = RestoreFailureState::from_persisted(None, 1);
        let validations = AtomicUsize::new(0);

        let before_boundary = state.validate_before_restore(true, Some(10), policy, 42, |value| {
            validations.fetch_add(1, Ordering::Relaxed);
            Ok::<_, ()>(value)
        });
        assert_eq!(before_boundary, Err(RestoreGateError::Suppressed));
        assert_eq!(state.window_started_unix_millis(), Some(10));
        assert_eq!(state.failures(), 1);

        let still_suppressed =
            state.validate_before_restore(true, Some(109), policy, 42, |value| {
                validations.fetch_add(1, Ordering::Relaxed);
                Ok::<_, ()>(value)
            });
        assert_eq!(still_suppressed, Err(RestoreGateError::Suppressed));
        assert_eq!(state.failures(), 1);

        let expired = state.validate_before_restore(true, Some(110), policy, 42, |value| {
            validations.fetch_add(1, Ordering::Relaxed);
            Ok::<_, ()>(value)
        });
        assert_eq!(expired, Ok(42));
        assert_eq!(state.window_started_unix_millis(), Some(110));
        assert_eq!(state.failures(), 0);
        assert_eq!(validations.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn failure_recorded_without_clock_keeps_retry_state_until_clock_is_known() {
        let policy = RestorePolicy::new(1, 100).unwrap();
        let mut state = RestoreFailureState::default();
        state.record_failure(None, policy);

        assert_eq!(state.window_started_unix_millis(), None);
        assert_eq!(state.failures(), 1);

        let result = state.validate_before_restore(true, Some(10), policy, 42, Ok::<_, ()>);

        assert_eq!(result, Err(RestoreGateError::Suppressed));
        assert_eq!(state.window_started_unix_millis(), Some(10));
        assert_eq!(state.failures(), 1);
    }

    #[test]
    fn clock_rollback_preserves_restore_failure_suppression() {
        let policy = RestorePolicy::new(1, 100).unwrap();
        let mut state = RestoreFailureState::default();
        state.record_failure(Some(100), policy);

        let validations = AtomicUsize::new(0);
        let result = state.validate_before_restore(true, Some(99), policy, 42, |value| {
            validations.fetch_add(1, Ordering::Relaxed);
            Ok::<_, ()>(value)
        });

        assert_eq!(result, Err(RestoreGateError::Suppressed));
        assert_eq!(validations.load(Ordering::Relaxed), 0);
        assert_eq!(state.window_started_unix_millis(), Some(100));
        assert_eq!(state.failures(), 1);
    }

    #[test]
    fn successful_validation_preserves_activation_failures_until_restore_success() {
        let policy = RestorePolicy::new(2, 300_000).unwrap();
        let mut state = RestoreFailureState::default();

        let first = state.validate_before_restore(true, Some(10_000), policy, 1, |value| {
            Ok::<_, ()>(value + 1)
        });
        assert_eq!(first, Ok(2));
        assert_eq!(state.failures(), 0);
        assert_eq!(state.window_started_unix_millis(), Some(10_000));
        state.record_failure(Some(10_500), policy);
        assert_eq!(state.failures(), 1);

        let second = state.validate_before_restore(true, Some(11_000), policy, 2, |value| {
            Ok::<_, ()>(value + 1)
        });
        assert_eq!(second, Ok(3));
        assert_eq!(state.failures(), 1);
        state.record_failure(Some(11_500), policy);
        assert_eq!(state.failures(), 2);

        let suppressed = state.validate_before_restore(true, Some(12_000), policy, 3, |value| {
            Ok::<_, ()>(value + 1)
        });
        assert_eq!(suppressed, Err(RestoreGateError::Suppressed));
        state.record_success();
        assert_eq!(state, RestoreFailureState::default());
    }
}
