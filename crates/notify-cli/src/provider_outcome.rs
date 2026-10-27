//! Shared sanitized provider outcomes and wall-clock sampling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProviderOutcome {
    Delivered,
    Retry,
    // Validated relative delay from a provider; exponential backoff remains a floor.
    RetryAfter(i64),
    Rejected,
    PauseAuthentication,
    PausePolicy,
}

pub(super) fn unix_time() -> Option<i64> {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs(),
    )
    .ok()
}
