//! Bounded recovery for transient PDH data loss without discarding a valid query.
use super::windows::Failure;
use std::time::{Duration, Instant};
use windows_sys::Win32::System::Performance::*;

#[derive(Default)]
pub(super) struct RetryState {
    failure: Option<Failure>,
    due: Option<Instant>,
    started: Option<Instant>,
    attempts: u32,
}

pub(super) struct RetryDecision {
    pub delay: Duration,
    pub retain_query: bool,
    pub log: bool,
    pub attempts: u32,
}

pub(super) struct Recovery {
    pub failure: Failure,
    pub attempts: u32,
    pub elapsed: Duration,
}

impl RetryState {
    pub fn pending(&self, now: Instant) -> Option<Failure> {
        self.failure
            .filter(|_| self.due.is_some_and(|due| now < due))
    }

    pub fn failed(&mut self, failure: Failure, now: Instant) -> RetryDecision {
        self.attempts = self.attempts.saturating_add(1);
        self.started.get_or_insert(now);
        let transient = matches!(
            failure.stage,
            "collect" | "size" | "format" | "empty" | "resize"
        ) && matches!(
            failure.code,
            PDH_CSTATUS_INVALID_DATA | PDH_INVALID_DATA | PDH_NO_DATA | PDH_MORE_DATA
        );
        let retain_query = transient && self.attempts <= 3;
        let delay = Duration::from_secs(if retain_query {
            1 << (self.attempts - 1)
        } else {
            30
        });
        let log =
            self.failure != Some(failure) || self.attempts <= 4 || self.attempts.is_multiple_of(10);
        self.failure = Some(failure);
        self.due = Some(now + delay);
        RetryDecision {
            delay,
            retain_query,
            log,
            attempts: self.attempts,
        }
    }

    // A baseline only primes a new rate counter; recovery requires a valid value.
    pub fn baseline(&mut self) {
        self.due = None;
    }

    pub fn recovered(&mut self, now: Instant) -> Option<Recovery> {
        let recovery = self.failure.map(|failure| Recovery {
            failure,
            attempts: self.attempts,
            elapsed: now.saturating_duration_since(self.started.unwrap_or(now)),
        });
        *self = Self::default();
        recovery
    }
}

pub(super) fn native_name(code: u32) -> &'static str {
    match code {
        PDH_CSTATUS_INVALID_DATA => "PDH_CSTATUS_INVALID_DATA",
        PDH_INVALID_DATA => "PDH_INVALID_DATA",
        PDH_NO_DATA => "PDH_NO_DATA",
        PDH_MORE_DATA => "PDH_MORE_DATA",
        PDH_CSTATUS_NO_OBJECT => "PDH_CSTATUS_NO_OBJECT",
        PDH_CSTATUS_NO_COUNTER => "PDH_CSTATUS_NO_COUNTER",
        _ => "PDH_ERROR",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_data_recovers_on_next_sample_without_thirty_second_blackout() {
        let now = Instant::now();
        let failure = Failure {
            stage: "format",
            code: PDH_CSTATUS_INVALID_DATA,
        };
        let mut state = RetryState::default();
        let decision = state.failed(failure, now);
        assert!(decision.retain_query);
        assert_eq!(decision.delay, Duration::from_secs(1));
        assert_eq!(state.pending(now), Some(failure));
        assert_eq!(state.pending(now + decision.delay), None);
        let recovery = state.recovered(now + Duration::from_secs(2)).unwrap();
        assert_eq!(recovery.attempts, 1);
        assert_eq!(recovery.elapsed, Duration::from_secs(2));
        assert!(state.recovered(now).is_none());
        assert_eq!(state.failed(failure, now).delay, Duration::from_secs(1));
    }

    #[test]
    fn repeated_data_loss_is_bounded_and_baselines_do_not_claim_recovery() {
        let now = Instant::now();
        let failure = Failure {
            stage: "empty",
            code: PDH_NO_DATA,
        };
        let mut state = RetryState::default();
        for seconds in [1, 2, 4, 30, 30] {
            let decision = state.failed(failure, now);
            assert_eq!(decision.delay, Duration::from_secs(seconds));
            assert_eq!(decision.retain_query, seconds < 30);
        }
        state.baseline();
        assert!(state.pending(now).is_none());
        assert_eq!(state.recovered(now).unwrap().attempts, 5);
    }

    #[test]
    fn missing_counters_and_malformed_native_buffers_require_reopening() {
        for failure in [
            Failure {
                stage: "add",
                code: PDH_CSTATUS_NO_COUNTER,
            },
            Failure {
                stage: "bounds",
                code: PDH_INVALID_DATA,
            },
        ] {
            let decision = RetryState::default().failed(failure, Instant::now());
            assert!(!decision.retain_query);
            assert_eq!(decision.delay, Duration::from_secs(30));
        }
    }
}
