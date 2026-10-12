//! Shared bounded operational policy checks, independent of runtime resources.
#[derive(Clone, Copy)]
pub(super) struct Controls<'a> {
    pub(super) poll: u64,
    pub(super) query_timeout: u64,
    pub(super) lookback: u64,
    pub(super) overlap: u64,
    pub(super) output: u64,
    pub(super) identities: u64,
    pub(super) events: u64,
    pub(super) stale: u64,
    pub(super) attempts: u64,
    pub(super) initial_backoff: u64,
    pub(super) max_backoff: u64,
    pub(super) jitter: u64,
    pub(super) jitter_seed: &'a str,
    pub(super) batch: u64,
    pub(super) retention: u64,
    pub(super) cleanup: u64,
    pub(super) health_sample: u64,
    pub(super) revision_cleanup: u64,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Limit,
    Relationship,
    Seed,
}
pub(super) fn validate(input: Controls<'_>) -> Result<u64, Error> {
    for (value, low, high) in [
        (input.poll, 1, 86400),
        (input.query_timeout, 1, 300),
        (input.lookback, 2, 604800),
        (input.overlap, 1, 86400),
        (input.output, 1024, 1048576),
        (input.identities, 1, 100000),
        (input.events, 1, 200000),
        (input.stale, 1, 86400),
        (input.attempts, 1, 20),
        (input.initial_backoff, 1, 3600),
        (input.max_backoff, 1, 86400),
        (input.jitter, 0, 3600),
        (input.batch, 1, 256),
        (input.retention, 1, 31536000),
        (input.cleanup, 1, 256),
        (input.health_sample, 1, 256),
        (input.revision_cleanup, 0, 256),
    ] {
        if !(low..=high).contains(&value) {
            return Err(Error::Limit);
        }
    }
    if input.query_timeout >= input.poll
        || input.overlap >= input.lookback
        || input.stale < input.poll
        || input.max_backoff < input.initial_backoff
        || input.jitter > input.max_backoff
    {
        return Err(Error::Relationship);
    }
    if input.jitter_seed.is_empty()
        || input.jitter_seed.len() > 20
        || !input.jitter_seed.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(Error::Seed);
    }
    let jitter_seed = input.jitter_seed.parse::<u64>().map_err(|_| Error::Seed)?;
    Ok(jitter_seed)
}
