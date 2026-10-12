//! Shared structural route validation and compiled destination masks.
use std::collections::HashSet;
pub(super) struct Rule<'a> {
    pub(super) events: &'a [&'a str],
    pub(super) destinations: &'a [&'a str],
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Limit,
    Reference,
    Duplicate,
}
fn event(name: &str) -> Result<usize, Error> {
    match name {
        "JOB_STARTED" => Ok(0),
        "JOB_COMPLETED" => Ok(1),
        "JOB_FAILED" => Ok(2),
        "JOB_TIMEOUT" => Ok(3),
        "JOB_CANCELLED" => Ok(4),
        "JOB_OUT_OF_MEMORY" => Ok(5),
        "JOB_NODE_FAILURE" => Ok(6),
        "JOB_BOOT_FAILURE" => Ok(7),
        "JOB_DEADLINE" => Ok(8),
        _ => Err(Error::Reference),
    }
}
pub(super) fn validate(providers: &[&str], rules: &[Rule<'_>]) -> Result<[u64; 9], Error> {
    if providers.is_empty()
        || providers.len() > 64
        || rules.len() > 128
        || rules.iter().any(|rule| {
            rule.events.is_empty()
                || rule.events.len() > 9
                || rule.destinations.is_empty()
                || rule.destinations.len() > 64
        })
    {
        return Err(Error::Limit);
    }
    let mut seen = HashSet::new();
    for name in providers {
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return Err(Error::Reference);
        }
        if !seen.insert(name) {
            return Err(Error::Duplicate);
        }
    }
    let mut masks = [0u64; 9];
    for rule in rules {
        let mut selected = HashSet::new();
        for name in rule.events {
            let index = event(name)?;
            if !selected.insert(index) {
                return Err(Error::Duplicate);
            }
        }
        let mut destinations = HashSet::new();
        let mut mask = 0;
        for name in rule.destinations {
            let index = providers
                .iter()
                .position(|provider| provider == name)
                .ok_or(Error::Reference)?;
            if !destinations.insert(index) {
                return Err(Error::Duplicate);
            }
            mask |= 1u64 << index;
        }
        for index in selected {
            masks[index] |= mask;
        }
    }
    Ok(masks)
}
